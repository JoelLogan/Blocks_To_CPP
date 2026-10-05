//! The toolchain registry (`b2c_build::toolchains`) with fake compilers in
//! temporary folders and a fake prober that never runs them: opening without
//! probing, rescans, manual compilers, the choice for a build with its
//! fallbacks and refusals, re-probing changed compilers, the
//! `toolchains.json` file, exclusions, background discovery and the epoch
//! that keeps stale scans out.

// Test code: helpers outside `#[test]`s fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use b2c_build::toolchains::{
    Chosen, DiscoveryScope, MAX_PARALLEL_PROBES, PROBE_TEMP_DIR, Prober, STORE_FILE, ToolchainRegistry,
    toolchain_dto,
};
use b2c_ipc::ToolchainId;
use b2c_ipc::dto::{CppStandard, Platform as IpcPlatform, ToolchainSource};
use b2c_ir::{DiagSource, Diagnostic, Location, Severity};
use b2c_toolchain::discovery::explicit_candidate;
use b2c_toolchain::fingerprint::Fingerprint;
use b2c_toolchain::probe::{
    Capabilities, CompilerKind, DiagnosticsFormat, PROBE_FORMAT, PROBE_TIMEOUT, ProbeError, ProbeOptions,
    Standards, Toolchain,
};
use b2c_toolchain::target::{GccVersion, Platform, Target};

/// The compiler's file name where discovery looks for it.
const GXX: &str = if cfg!(windows) { "g++.exe" } else { "g++" };

const NO_TOOLCHAIN: &str = "B2C-T1001";
const BAD_PATH: &str = "B2C-T1002";
const NOT_RUNNABLE: &str = "B2C-T1003";
const CHANGED: &str = "B2C-T1009";
const SELECTED_UNAVAILABLE: &str = "B2C-T1022";

/// A usable GCC 13 record for the compiler at `fingerprint`.
fn gcc(fingerprint: Fingerprint) -> Toolchain {
    let mut capabilities = Capabilities {
        hello_world: true,
        standards: Standards {
            cpp17: Some(String::from("c++17")),
            cpp20: Some(String::from("c++20")),
            cpp23: Some(String::from("c++23")),
            cpp26: Some(String::from("c++2c")),
        },
        diagnostics: Some(DiagnosticsFormat::SarifFile),
        ..Capabilities::default()
    };
    capabilities.library.format = true;
    capabilities.sanitizers.address_undefined = true;
    Toolchain {
        format: PROBE_FORMAT,
        fingerprint,
        kind: CompilerKind::Gcc,
        version: GccVersion::parse("13.3.0"),
        version_text: String::from("g++ (Ubuntu 13.3.0-6ubuntu2~24.04) 13.3.0"),
        target: Target::parse("x86_64-linux-gnu"),
        capabilities,
        problems: Vec::new(),
    }
}

/// A prober that never runs anything. The compiler file's text decides the
/// answer: `broken` gives an unusable compiler, `unprobeable` a probe
/// failure, anything else a usable GCC 13. It counts calls, remembers the
/// options and the most probes that ran at once, and can hold its first
/// call until released.
#[derive(Default)]
struct FakeProber {
    calls: AtomicUsize,
    running: AtomicUsize,
    most_at_once: AtomicUsize,
    delay: Option<Duration>,
    options: Mutex<Vec<ProbeOptions>>,
    gate: Option<Gate>,
}

/// Holds the first probe until [`Gate::open`].
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
    entered: Mutex<Option<mpsc::Sender<()>>>,
}

impl Gate {
    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }

    /// Called by the first probe: says so, then waits. (The sender is taken
    /// in its own statement, so its lock is released before the wait.)
    fn pass(&self) {
        let first = self.entered.lock().unwrap().take();
        if let Some(entered) = first {
            entered.send(()).unwrap();
            let open = self.open.lock().unwrap();
            let _open = self.changed.wait_while(open, |open| !*open).unwrap();
        }
    }
}

impl FakeProber {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// A prober whose first probe waits for the gate; the receiver says when
    /// it is waiting.
    fn gated() -> (Arc<Self>, mpsc::Receiver<()>) {
        let (sender, receiver) = mpsc::channel();
        let gate = Gate {
            entered: Mutex::new(Some(sender)),
            ..Gate::default()
        };
        (
            Arc::new(Self {
                gate: Some(gate),
                ..Self::default()
            }),
            receiver,
        )
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn open_gate(&self) {
        self.gate.as_ref().unwrap().open();
    }
}

impl Prober for FakeProber {
    fn probe(&self, path: &Path, options: &ProbeOptions) -> Result<Toolchain, ProbeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.options.lock().unwrap().push(options.clone());
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.most_at_once.fetch_max(now, Ordering::SeqCst);
        if let Some(gate) = &self.gate {
            gate.pass();
        }
        if let Some(delay) = self.delay {
            std::thread::sleep(delay);
        }
        let result = answer(path);
        self.running.fetch_sub(1, Ordering::SeqCst);
        result
    }
}

/// What [`FakeProber`] says about the compiler at `path`.
fn answer(path: &Path) -> Result<Toolchain, ProbeError> {
    let fingerprint = Fingerprint::compute(path).map_err(|source| ProbeError::Unreadable {
        path: path.to_path_buf(),
        source,
    })?;
    let text = fs::read_to_string(path).unwrap_or_default();
    if text.contains("unprobeable") {
        return Err(ProbeError::TempDir(std::io::Error::other("no space left")));
    }
    let mut toolchain = gcc(fingerprint);
    if text.contains("broken") {
        toolchain.problems.push(Diagnostic::error(
            "B2C-T1006",
            DiagSource::Toolchain,
            Location::project(),
            "broken",
        ));
    }
    Ok(toolchain)
}

/// A temporary root (canonical) with `machine/` and `cache/` below it.
struct Root {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

impl Root {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = b2c_toolchain::paths::canonical(dir.path()).unwrap();
        Self { _dir: dir, path }
    }

    /// `relative` (with `/` between its parts) below the root, built part by
    /// part so the path has the platform's separators: on Windows the
    /// registry's canonical paths never contain `/`, and the IDs the tests
    /// compute from these paths must match them.
    fn join(&self, relative: &str) -> PathBuf {
        relative
            .split('/')
            .fold(self.path.clone(), |path, part| path.join(part))
    }

    fn store(&self) -> PathBuf {
        self.join("machine").join(STORE_FILE)
    }

    /// A registry over this root that searches only `dirs`.
    fn registry(&self, dirs: &[PathBuf], prober: &Arc<FakeProber>) -> ToolchainRegistry {
        ToolchainRegistry::open(
            self.store(),
            self.join("cache").join(PROBE_TEMP_DIR),
            DiscoveryScope::Only(dirs.to_vec()),
            Arc::clone(prober) as Arc<dyn Prober>,
        )
    }
}

/// Installs a fake compiler with `text` in `dir`; returns its path.
fn install(dir: &Path, text: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let path = dir.join(GXX);
    fs::write(&path, text).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
    diagnostics.iter().map(|d| d.code.0.as_str()).collect()
}

fn listed_paths(registry: &ToolchainRegistry) -> Vec<String> {
    registry
        .list(None)
        .toolchains
        .into_iter()
        .map(|toolchain| toolchain.display_path)
        .collect()
}

fn shown(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn ready(chosen: Chosen) -> (Toolchain, Vec<Diagnostic>) {
    match chosen {
        Chosen::Ready { toolchain, notes } => {
            assert!(
                notes.iter().all(|note| note.severity != Severity::Error),
                "{notes:?}"
            );
            (*toolchain, notes)
        }
        Chosen::Unavailable { diagnostics } => panic!("no toolchain: {diagnostics:?}"),
    }
}

fn unavailable(chosen: Chosen) -> Vec<Diagnostic> {
    match chosen {
        Chosen::Ready { toolchain, .. } => panic!("chose {}", toolchain.path().display()),
        Chosen::Unavailable { diagnostics } => diagnostics,
    }
}

#[test]
fn opening_reads_the_list_and_never_probes() {
    let root = Root::new();
    let bin = root.join("bin");
    install(&bin, "gcc");
    let first = FakeProber::new();
    let found = root
        .registry(std::slice::from_ref(&bin), &first)
        .rescan(&[], None);
    assert_eq!(first.calls(), 1);
    assert_eq!(found.toolchains.len(), 1);

    let prober = FakeProber::new();
    let reopened = root.registry(std::slice::from_ref(&bin), &prober);
    let listed = reopened.list(None);
    assert_eq!(prober.calls(), 0, "opening and listing never probe");
    assert!(!listed.discovering);
    assert_eq!(listed.toolchains, found.toolchains);
    assert!(reopened.get(&listed.toolchains[0].id).is_some());
    assert!(!reopened.setup_info().no_usable_toolchain);
    assert_eq!(prober.calls(), 0);

    let empty_root = Root::new();
    let empty = empty_root.registry(&[], &prober);
    assert!(empty.list(None).toolchains.is_empty());
    let setup = empty.setup_info();
    assert!(setup.no_usable_toolchain);
    assert_eq!(setup.platform, IpcPlatform::current());
    if cfg!(windows) {
        assert_eq!(setup.distro, None);
    }
    assert_eq!(prober.calls(), 0);
}

#[test]
fn a_rescan_finds_a_newly_installed_compiler_and_probes_only_what_changed() {
    let root = Root::new();
    let bin = root.join("bin");
    let prober = FakeProber::new();
    let registry = root.registry(std::slice::from_ref(&bin), &prober);
    assert!(registry.rescan(&[], None).toolchains.is_empty());

    let gxx = install(&bin, "gcc");
    let listed = registry.rescan(&[], None);
    assert_eq!(listed.toolchains.len(), 1);
    let toolchain = &listed.toolchains[0];
    assert_eq!(toolchain.source, ToolchainSource::Path);
    assert_eq!(toolchain.display_path, shown(&gxx));
    assert_eq!(toolchain.id, ToolchainId::for_driver(&gxx));
    assert_eq!(toolchain.version.as_deref(), Some("13.3.0"));
    assert!(toolchain.usable && !toolchain.selected);
    assert!(!listed.discovering);
    assert_eq!(prober.calls(), 1);

    // Unchanged: not probed again.
    registry.rescan(&[], None);
    assert_eq!(prober.calls(), 1);

    // Probes run with the 10 s timeout, the sanitised environment and the
    // private probe folder, which is created owner-only.
    let options = prober.options.lock().unwrap()[0].clone();
    assert_eq!(options.timeout, PROBE_TIMEOUT);
    assert!(options.host.passthrough.is_empty());
    let probe_temp = root.join("cache").join(PROBE_TEMP_DIR);
    assert_eq!(options.temp_root.as_deref(), Some(probe_temp.as_path()));
    assert!(options.jobs >= 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            fs::metadata(&probe_temp).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    // Uninstalled: gone.
    fs::remove_file(&gxx).unwrap();
    assert!(registry.rescan(&[], None).toolchains.is_empty());
}

#[test]
fn at_most_four_compilers_are_probed_at_once() {
    let root = Root::new();
    let dirs: Vec<PathBuf> = (0..10)
        .map(|n| {
            let dir = root.join(&format!("gcc{n}/bin"));
            install(&dir, &format!("gcc {n}"));
            dir
        })
        .collect();
    let prober = Arc::new(FakeProber {
        delay: Some(Duration::from_millis(50)),
        ..FakeProber::default()
    });
    let listed = root.registry(&dirs, &prober).rescan(&[], None);
    assert_eq!(prober.calls(), 10);
    let most = prober.most_at_once.load(Ordering::SeqCst);
    assert!((1..=MAX_PARALLEL_PROBES).contains(&most), "{most} probes at once");
    // Discovery order is kept whatever order the probes finished in.
    let expected: Vec<String> = dirs.iter().map(|dir| shown(&dir.join(GXX))).collect();
    let listed: Vec<String> = listed.toolchains.into_iter().map(|t| t.display_path).collect();
    assert_eq!(listed, expected);
}

#[test]
fn batch_files_and_bad_paths_are_never_added() {
    let root = Root::new();
    let prober = FakeProber::new();
    let registry = root.registry(&[], &prober);
    let script = root.join("tools/g++.bat");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(&script, "@echo off\r\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    for bad in [
        script.clone(),
        root.join("tools/G++.CMD"),
        root.join("missing").join(GXX),
        PathBuf::from(GXX),
    ] {
        let refused = registry.add_explicit(&bad).unwrap_err();
        assert_eq!(codes(&refused), [BAD_PATH], "{}", bad.display());
        assert_eq!(refused[0].severity, Severity::Error);
    }
    // The discovery rule on Windows refuses the script too.
    let refused = explicit_candidate(&script, Platform::Windows).unwrap_err();
    assert_eq!(refused.code.0, BAD_PATH);
    assert_eq!(prober.calls(), 0);
    assert!(registry.list(None).toolchains.is_empty());
    assert!(!root.store().exists(), "nothing was saved");
}

#[test]
fn compilers_added_by_hand_are_kept_as_manual() {
    let root = Root::new();
    let bin = root.join("bin");
    let prober = FakeProber::new();
    let registry = root.registry(std::slice::from_ref(&bin), &prober);
    let picked = install(&root.join("opt/gcc-13/bin"), "gcc");
    let added = registry.add_explicit(&picked).unwrap();
    assert_eq!(added.source, ToolchainSource::Manual);
    assert_eq!(added.display_path, shown(&picked));
    assert!(added.usable && !added.selected);
    assert_eq!(prober.calls(), 1);

    // A broken one is added too, as not usable, with its problems.
    let broken = install(&root.join("opt/broken/bin"), "broken");
    let added_broken = registry.add_explicit(&broken).unwrap();
    assert!(!added_broken.usable);
    assert_eq!(added_broken.problems[0].code, "B2C-T1006");

    // One that cannot be probed at all is refused.
    let unprobeable = install(&root.join("opt/odd/bin"), "unprobeable");
    let refused = registry.add_explicit(&unprobeable).unwrap_err();
    assert_eq!(codes(&refused), [NOT_RUNNABLE]);

    // Discovery keeps them, after the discovered ones, and so does the file.
    install(&bin, "gcc");
    let listed = registry.rescan(&[], None);
    let sources: Vec<_> = listed.toolchains.iter().map(|t| t.source).collect();
    assert_eq!(
        sources,
        [
            ToolchainSource::Path,
            ToolchainSource::Manual,
            ToolchainSource::Manual
        ]
    );
    let reopened = root.registry(std::slice::from_ref(&bin), &FakeProber::new());
    assert_eq!(reopened.list(None), listed);

    // Adding the same file again replaces its entry.
    registry.add_explicit(&picked).unwrap();
    assert_eq!(registry.list(None).toolchains.len(), 3);

    // A manual compiler whose file is gone is forgotten by the next rescan.
    fs::remove_file(&broken).unwrap();
    registry.rescan(&[], None);
    assert_eq!(listed_paths(&registry), [shown(&bin.join(GXX)), shown(&picked)]);
}

#[test]
fn an_unknown_selection_warns_and_falls_back() {
    let root = Root::new();
    let bin = root.join("bin");
    let gxx = install(&bin, "gcc");
    let prober = FakeProber::new();
    let registry = root.registry(std::slice::from_ref(&bin), &prober);
    registry.rescan(&[], None);

    let unknown = ToolchainId::example();
    let (toolchain, notes) = ready(registry.choose(Some(&unknown), None));
    assert_eq!(toolchain.path(), gxx);
    assert_eq!(codes(&notes)[0], SELECTED_UNAVAILABLE);
    assert_eq!(notes[0].severity, Severity::Warning);
    assert!(
        notes[0].message.contains("no longer available"),
        "{}",
        notes[0].message
    );
    assert!(notes[0].message.contains(&shown(&gxx)), "{}", notes[0].message);

    // The selected one, or none: no warning.
    let id = ToolchainId::for_driver(&gxx);
    let (_, notes) = ready(registry.choose(Some(&id), None));
    assert!(!codes(&notes).contains(&SELECTED_UNAVAILABLE));
    let (_, notes) = ready(registry.choose(None, None));
    assert!(notes.is_empty(), "{notes:?}");
    assert!(registry.list(Some(&id)).toolchains[0].selected);

    // Nothing else to fall back to.
    fs::remove_file(&gxx).unwrap();
    registry.rescan(&[], None);
    let diagnostics = unavailable(registry.choose(Some(&unknown), None));
    assert_eq!(codes(&diagnostics), [SELECTED_UNAVAILABLE, NO_TOOLCHAIN]);
}

#[test]
fn a_selection_that_fails_its_checks_falls_back_to_discovery_order() {
    let root = Root::new();
    let (first, second) = (root.join("a/bin"), root.join("b/bin"));
    let broken = install(&first, "broken");
    let good = install(&second, "gcc");
    let prober = FakeProber::new();
    let registry = root.registry(&[first, second], &prober);
    registry.rescan(&[], None);

    let (toolchain, notes) = ready(registry.choose(None, None));
    assert_eq!(toolchain.path(), good, "the broken one is skipped");
    assert!(notes.is_empty());

    let (toolchain, notes) = ready(registry.choose(Some(&ToolchainId::for_driver(&broken)), None));
    assert_eq!(toolchain.path(), good);
    assert_eq!(codes(&notes), [SELECTED_UNAVAILABLE]);
    assert!(
        notes[0].message.contains("failed its checks"),
        "{}",
        notes[0].message
    );

    fs::write(&good, "broken too").unwrap();
    let diagnostics = unavailable(registry.choose(Some(&ToolchainId::for_driver(&broken)), None));
    assert_eq!(
        codes(&diagnostics),
        [SELECTED_UNAVAILABLE, NO_TOOLCHAIN, "B2C-T1006", "B2C-T1006"]
    );
}

#[test]
fn compilers_inside_the_project_folder_are_refused() {
    let root = Root::new();
    let project = root.join("game");
    let planted = install(&project.join("bin"), "gcc");
    let prober = FakeProber::new();
    let registry = root.registry(&[project.join("bin")], &prober);
    // Found by a scan that did not exclude the project (another project was
    // open then), but never used for a build of this one.
    registry.rescan(&[], None);
    let planted_id = ToolchainId::for_driver(&planted);
    let diagnostics = unavailable(registry.choose(None, Some(&project)));
    assert_eq!(codes(&diagnostics), [NO_TOOLCHAIN, BAD_PATH]);
    assert!(diagnostics[1].message.contains("inside the project's folder"));
    let diagnostics = unavailable(registry.choose(Some(&planted_id), Some(&project)));
    assert_eq!(
        codes(&diagnostics),
        [SELECTED_UNAVAILABLE, NO_TOOLCHAIN, BAD_PATH]
    );
    // A project path given in another spelling is canonicalised first.
    let spelled = root.join("game/bin/..");
    assert_eq!(
        codes(&unavailable(registry.choose(None, Some(&spelled)))),
        [NO_TOOLCHAIN, BAD_PATH]
    );

    // With a compiler outside, the build falls back to it and says why.
    let outside = install(&root.join("tools/bin"), "gcc");
    let registry = root.registry(&[project.join("bin"), root.join("tools/bin")], &prober);
    registry.rescan(&[], None);
    let (toolchain, notes) = ready(registry.choose(Some(&planted_id), Some(&project)));
    assert_eq!(toolchain.path(), outside);
    assert_eq!(codes(&notes), [SELECTED_UNAVAILABLE]);
    assert!(notes[0].message.contains("inside the project's folder"));
    // For another project it is fine.
    let (toolchain, _) = ready(registry.choose(Some(&planted_id), Some(&root.join("tools"))));
    assert_eq!(toolchain.path(), planted);

    // A rescan that excludes the project does not even list it.
    registry.rescan(std::slice::from_ref(&project), None);
    assert_eq!(listed_paths(&registry), [shown(&outside)]);
}

#[test]
fn a_changed_compiler_is_probed_again_with_a_note() {
    let root = Root::new();
    let bin = root.join("bin");
    let gxx = install(&bin, "gcc 13.3");
    let prober = FakeProber::new();
    let registry = root.registry(std::slice::from_ref(&bin), &prober);
    registry.rescan(&[], None);
    assert_eq!(prober.calls(), 1);

    // Unchanged: no probe, no note.
    let (_, notes) = ready(registry.choose(None, None));
    assert!(notes.is_empty());
    assert_eq!(prober.calls(), 1);

    fs::write(&gxx, "gcc 13.4 (updated)").unwrap();
    let (toolchain, notes) = ready(registry.choose(None, None));
    assert_eq!(codes(&notes), [CHANGED]);
    assert_eq!(notes[0].severity, Severity::Info);
    assert_eq!(prober.calls(), 2);
    assert!(toolchain.is_current());
    // The list now holds the new probe: no further probe.
    ready(registry.choose(None, None));
    assert_eq!(prober.calls(), 2);

    // The selected toolchain is checked the same way.
    fs::write(&gxx, "gcc 13.5").unwrap();
    let (_, notes) = ready(registry.choose(Some(&ToolchainId::for_driver(&gxx)), None));
    assert_eq!(codes(&notes), [CHANGED]);
    assert_eq!(prober.calls(), 3);

    // And so is every compiler in a rescan.
    fs::write(&gxx, "gcc 14").unwrap();
    registry.rescan(&[], None);
    assert_eq!(prober.calls(), 4);
    // The new probe was saved.
    let reopened = root.registry(std::slice::from_ref(&bin), &prober);
    ready(reopened.choose(None, None));
    assert_eq!(prober.calls(), 4);
}

#[test]
fn the_list_file_round_trips_and_older_forms_count_as_empty() {
    let root = Root::new();
    let bin = root.join("bin");
    let gxx = install(&bin, "gcc");
    let prober = FakeProber::new();
    let registry = root.registry(std::slice::from_ref(&bin), &prober);
    let listed = registry.rescan(&[], None);

    let json: serde_json::Value = serde_json::from_slice(&fs::read(root.store()).unwrap()).unwrap();
    assert_eq!(json["format"], "blocks2cpp/toolchains");
    assert_eq!(json["formatVersion"], 1);
    let entry = &json["toolchains"][0];
    assert_eq!(entry["source"], "path");
    assert_eq!(entry["foundAs"], shown(&gxx));
    assert_eq!(entry["probe"]["fingerprint"]["path"], shown(&gxx));
    assert_eq!(
        root.registry(std::slice::from_ref(&bin), &prober).list(None),
        listed
    );

    // Written atomically: nothing but the file is left in its folder, and
    // it is owner-only.
    let names: Vec<_> = fs::read_dir(root.join("machine"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, [STORE_FILE]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            fs::metadata(root.store()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(root.join("machine")).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    // M1's bare array, a newer version and broken JSON all count as empty;
    // the next scan writes the current format again.
    let probe = &json["toolchains"][0]["probe"];
    let newer = serde_json::json!({"format": "blocks2cpp/toolchains", "formatVersion": 2, "toolchains": []});
    for old in [
        serde_json::to_vec(&serde_json::json!([probe])).unwrap(),
        serde_json::to_vec(&newer).unwrap(),
        b"{\"format\": ".to_vec(),
    ] {
        fs::write(root.store(), &old).unwrap();
        let reopened = root.registry(std::slice::from_ref(&bin), &prober);
        assert!(reopened.list(None).toolchains.is_empty());
        reopened.rescan(&[], None);
        let json: serde_json::Value = serde_json::from_slice(&fs::read(root.store()).unwrap()).unwrap();
        assert_eq!(json["formatVersion"], 1);
        assert_eq!(json["toolchains"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn excluded_folders_and_the_cache_root_are_never_searched() {
    let root = Root::new();
    let (project, cache) = (root.join("game"), root.join("cache"));
    let (in_project, in_cache, tools) = (project.join("bin"), cache.join("bin"), root.join("tools/bin"));
    install(&in_project, "gcc");
    install(&in_cache, "gcc");
    let outside = install(&tools, "gcc");
    let prober = FakeProber::new();
    let registry = root.registry(&[in_project, in_cache, tools], &prober);
    // The open project is excluded by the caller; the cache root, which
    // holds the probes' folder, always is.
    registry.rescan(std::slice::from_ref(&project), None);
    assert_eq!(listed_paths(&registry), [shown(&outside)]);
    assert_eq!(prober.calls(), 1);
    // Through a link, too: exclusions compare canonical paths.
    #[cfg(unix)]
    {
        let link = root.join("link-to-game");
        std::os::unix::fs::symlink(&project, &link).unwrap();
        registry.rescan(&[link], None);
        assert_eq!(listed_paths(&registry), [shown(&outside)]);
    }
}

/// Set in the child process of [`the_current_directory_is_never_searched`]:
/// the folder to search.
const CHILD_SEARCH: &str = "B2C_TEST_REGISTRY_SEARCH";
/// Set in that child: the root for its list.
const CHILD_ROOT: &str = "B2C_TEST_REGISTRY_ROOT";

/// Runs this test binary again in `cwd`, running only
/// [`current_directory_child`], and returns how many toolchains it found
/// when searching only `search`.
fn found_from(cwd: &Path, search: &Path, store_root: &Path) -> usize {
    let exe = std::env::current_exe().unwrap();
    let mut command = b2c_process::Command::new(exe, cwd).unwrap();
    command
        .args([
            "--exact",
            "current_directory_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_SEARCH, search)
        .env(CHILD_ROOT, store_root)
        .timeout(Duration::from_mins(1));
    // Variables the child needs to run at all (Windows) or that a coverage
    // run uses to collect the child's counters.
    for name in ["SystemRoot", "LLVM_PROFILE_FILE"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let captured = b2c_process::run_captured(&command).unwrap();
    let stdout = String::from_utf8_lossy(&captured.stdout);
    assert!(
        captured.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&captured.stderr)
    );
    // The harness may print the test's name on the same line first.
    stdout
        .lines()
        .find_map(|line| line.rsplit_once("found: ").map(|(_, count)| count))
        .unwrap_or_else(|| panic!("nothing reported:\n{stdout}"))
        .trim()
        .parse()
        .unwrap()
}

/// Not a check of its own: in the child process started by [`found_from`]
/// it rescans and prints how many toolchains it found. In an ordinary test
/// run it does nothing.
#[test]
fn current_directory_child() {
    let (Some(search), Some(store_root)) = (std::env::var_os(CHILD_SEARCH), std::env::var_os(CHILD_ROOT))
    else {
        return;
    };
    let store_root = PathBuf::from(store_root);
    let registry = ToolchainRegistry::open(
        store_root.join(STORE_FILE),
        store_root.join(PROBE_TEMP_DIR),
        DiscoveryScope::Only(vec![PathBuf::from(search)]),
        FakeProber::new() as Arc<dyn Prober>,
    );
    println!("found: {}", registry.rescan(&[], None).toolchains.len());
}

#[test]
fn the_current_directory_is_never_searched() {
    let root = Root::new();
    let planted = root.join("project");
    install(&planted, "gcc");
    let elsewhere = root.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    assert_eq!(found_from(&elsewhere, &planted, &root.join("one")), 1);
    assert_eq!(found_from(&planted, &planted, &root.join("two")), 0);
}

#[test]
fn background_discovery_reports_progress_and_calls_back_once() {
    let root = Root::new();
    let bin = root.join("bin");
    install(&bin, "gcc");
    let (prober, entered) = FakeProber::gated();
    let registry = Arc::new(root.registry(std::slice::from_ref(&bin), &prober));
    let (done, finished) = mpsc::channel();
    registry.spawn_discovery(
        Vec::new(),
        Box::new(move || {
            done.send(()).unwrap();
        }),
    );
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let listed = registry.list(None);
    assert!(listed.discovering);
    assert!(listed.toolchains.is_empty());

    prober.open_gate();
    finished.recv_timeout(Duration::from_secs(10)).unwrap();
    let listed = registry.list(None);
    assert!(!listed.discovering);
    assert_eq!(listed.toolchains.len(), 1);
    assert!(
        finished.recv_timeout(Duration::from_millis(200)).is_err(),
        "called back once"
    );
}

#[test]
fn a_stale_scan_never_overwrites_a_newer_one() {
    let root = Root::new();
    let (first, second) = (root.join("first/bin"), root.join("second/bin"));
    let old = install(&first, "gcc old");
    let (prober, entered) = FakeProber::gated();
    let registry = Arc::new(root.registry(&[first, second.clone()], &prober));
    let (done, finished) = mpsc::channel();
    registry.spawn_discovery(Vec::new(), Box::new(move || done.send(()).unwrap()));
    // The background scan found the old compiler and is probing it.
    entered.recv_timeout(Duration::from_secs(10)).unwrap();

    // Meanwhile the old compiler is uninstalled, a new one installed, and
    // the user rescans.
    fs::remove_file(&old).unwrap();
    let newer = install(&second, "gcc new");
    let rescanned = registry.rescan(&[], None);
    assert!(rescanned.discovering, "the old scan is still running");
    assert_eq!(listed_paths(&registry), [shown(&newer)]);

    // The old scan ends: its result (the old compiler) is dropped.
    prober.open_gate();
    finished.recv_timeout(Duration::from_secs(10)).unwrap();
    let listed = registry.list(None);
    assert!(!listed.discovering);
    assert_eq!(listed_paths(&registry), [shown(&newer)]);
    let (toolchain, notes) = ready(registry.choose(None, None));
    assert!(notes.is_empty(), "the newer probe is current: {notes:?}");
    assert_eq!(toolchain.path(), newer);
    // And the file holds the newer list too.
    let reopened = root.registry(&[], &FakeProber::new());
    assert_eq!(listed_paths(&reopened), [shown(&newer)]);
}

#[test]
fn choosing_waits_for_a_running_discovery_or_runs_one() {
    let root = Root::new();
    let bin = root.join("bin");
    let gxx = install(&bin, "gcc");
    let (prober, entered) = FakeProber::gated();
    let registry = Arc::new(root.registry(std::slice::from_ref(&bin), &prober));
    registry.spawn_discovery(Vec::new(), Box::new(|| {}));
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let opener = {
        let prober = Arc::clone(&prober);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            prober.open_gate();
        })
    };
    // Nothing is known yet: the choice waits for the discovery.
    let (toolchain, _) = ready(registry.choose(None, None));
    assert_eq!(toolchain.path(), gxx);
    opener.join().unwrap();

    // A registry no discovery ever ran on runs one itself.
    let fresh_root = Root::new();
    let fresh_prober = FakeProber::new();
    let fresh = fresh_root.registry(std::slice::from_ref(&bin), &fresh_prober);
    let (toolchain, _) = ready(fresh.choose(None, None));
    assert_eq!(toolchain.path(), gxx);
    assert_eq!(fresh_prober.calls(), 1);
    // After one scan, nothing usable means unavailable, without waiting.
    fs::write(&gxx, "broken").unwrap();
    assert_eq!(
        codes(&unavailable(fresh.choose(None, None))),
        [NO_TOOLCHAIN, "B2C-T1006"]
    );
}

#[test]
fn a_compiler_that_cannot_be_probed_is_listed_as_not_usable() {
    let root = Root::new();
    let bin = root.join("bin");
    let gxx = install(&bin, "unprobeable");
    let prober = FakeProber::new();
    let registry = root.registry(std::slice::from_ref(&bin), &prober);
    let listed = registry.rescan(&[], None);
    assert_eq!(listed.toolchains.len(), 1);
    let toolchain = &listed.toolchains[0];
    assert!(!toolchain.usable);
    assert_eq!(toolchain.problems.len(), 1);
    assert_eq!(toolchain.problems[0].code, NOT_RUNNABLE);
    assert_eq!(toolchain.version, None);
    // It is tried again whenever it is needed, without a "changed" note.
    fs::write(&gxx, "gcc").unwrap();
    let (_, notes) = ready(registry.choose(None, None));
    assert!(notes.is_empty(), "{notes:?}");
}

/// The toolchain DTO for a probed g++ 13 on Ubuntu, with the ID (which
/// hashes the path in the platform's encoding) checked separately.
#[test]
fn the_toolchain_dto_snapshot() {
    let path = PathBuf::from("/usr/bin/x86_64-linux-gnu-g++-13");
    let mut toolchain = gcc(Fingerprint {
        path: path.clone(),
        size: 1_040_592,
        modified_ns: 1_712_000_000_000_000_000,
        sha256: String::from("5b0f3c4e2a9d7b8c1f6e0d4a3b2c1d0e9f8a7b6c5d4e3f2a1b0c9d8e7f6a5b4c"),
    });
    toolchain.capabilities.standards.cpp26 = None;
    toolchain.capabilities.cc1plus = Some(PathBuf::from("/usr/libexec/gcc/x86_64-linux-gnu/13/cc1plus"));
    toolchain.problems.push(Diagnostic::info(
        "B2C-T1021",
        DiagSource::Toolchain,
        Location::project(),
        "AddressSanitizer works, but its leak detection does not.",
    ));
    let id = ToolchainId::for_driver(&path);
    let dto = toolchain_dto(
        &toolchain,
        ToolchainSource::Path,
        Path::new("/usr/bin/g++"),
        Some(&id),
    );
    assert_eq!(dto.id, id);
    assert_eq!(
        dto.capabilities.standards,
        [CppStandard::Cpp17, CppStandard::Cpp20, CppStandard::Cpp23]
    );
    // Serialised from the struct, so the key order is the declaration order
    // whatever `serde_json` features other crates enable.
    let json = serde_json::to_string_pretty(&dto)
        .unwrap()
        .replace(id.as_str(), "<ToolchainId::for_driver(canonical path)>");
    insta::assert_snapshot!("probed_toolchain_dto", json);
}

#[test]
fn ids_are_stable_across_registries() {
    let root = Root::new();
    let bin = root.join("bin");
    let gxx = install(&bin, "gcc");
    let prober = FakeProber::new();
    let first = root
        .registry(std::slice::from_ref(&bin), &prober)
        .rescan(&[], None);
    let second = Root::new()
        .registry(std::slice::from_ref(&bin), &prober)
        .rescan(&[], None);
    assert_eq!(first.toolchains[0].id, second.toolchains[0].id);
    assert_eq!(first.toolchains[0].id, ToolchainId::for_driver(&gxx));
}
