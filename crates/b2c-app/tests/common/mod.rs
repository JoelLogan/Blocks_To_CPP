//! Test doubles and helpers shared by the backend's integration tests: a
//! scripted [`FakeDialogs`], a recording [`FakeOpener`], a [`TestProber`] that
//! never runs a compiler to probe it, fake `g++` scripts (one of them a spawn
//! spy that leaves a marker file), and a [`TestApp`] that starts a backend
//! whose every folder is inside a temporary directory.
//!
//! Tests that compile need g++ on `PATH`; without one they return early,
//! unless `B2C_REQUIRE_GXX` is set (as in CI).

// Test helpers fail the test by panicking; not every test file uses every
// helper.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    dead_code,
    unreachable_pub
)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use b2c_app::{Backend, BackendConfig, Dialogs, Services, TrustChoice, TrustPrompt, UrlOpener};
use b2c_build::toolchains::{DiscoveryScope, Prober};
use b2c_build::{BlockReplacement, BlockUpgrades};
use b2c_ipc::dto::{
    AppEvent, BuildConfig, BuildEvent, BuildStartRequest, ProjectOpenDialogResponse, ProjectOpened, RunEvent,
};
use b2c_ipc::sink::testing::{RecordingBytes, RecordingSink};
use b2c_ipc::{BuildId, Handle};
use b2c_store::Dirs;
use b2c_toolchain::fingerprint::Fingerprint;
use b2c_toolchain::probe::{
    Capabilities, CompilerKind, DiagnosticsFormat, PROBE_FORMAT, ProbeError, ProbeOptions, Standards,
    Toolchain,
};
use b2c_toolchain::target::{GccVersion, Target};

/// The test-only retired block type `test.say` v1, replaced by `io.print`
/// v1: its `WHAT` input becomes `ITEM0`. No shipped catalog defines it.
fn say_to_print(block: &mut b2c_model::Block) -> Result<(), String> {
    let what = block.inputs.remove("WHAT").ok_or("the block has no WHAT input")?;
    block.inputs.insert(String::from("ITEM0"), what);
    Ok(())
}

/// The shipped block migrations with only the test-only replacement of
/// `test.say` (the shipped replacement of `var.update` is tested with
/// [`BlockUpgrades::SHIPPED`]).
pub const TEST_UPGRADES: BlockUpgrades<'static> = BlockUpgrades {
    migrations: BlockUpgrades::SHIPPED.migrations,
    replacements: &[BlockReplacement {
        from: "test.say",
        version: 1,
        to: "io.print",
        to_version: 1,
        apply: say_to_print,
    }],
    retired: &["test.say"],
};

/// A project whose `main` says "hi" with the test-only retired `test.say`
/// block (see [`TEST_UPGRADES`]), with a define so that it has a security
/// hash worth keeping.
pub fn old_say_project() -> String {
    serde_json::json!({
        "format": "blocks2cpp/project", "formatVersion": 1,
        "generator": {"app": "0.1.0", "catalog": "1.0.0"},
        "project": {"id": "prj_old_say", "name": "Old say",
                    "language": {"standard": "c++20"},
                    "build": {"defines": [{"name": "LEVEL", "value": {"int": 1}}]}},
        "modules": [{"id": "mod_main", "name": "main", "workspace": {"blocks": [
            {"id": "b_main", "type": "program.main", "v": 1, "x": 10, "y": 20,
             "statements": {"BODY": [
                {"id": "b_say", "type": "test.say", "v": 1, "comment": {"text": "kept"},
                 "inputs": {"WHAT": {"expr": [{"str": "hi"}]}}}
             ]}}
        ]}}]
    })
    .to_string()
}

/// The repository root.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The bytes of `examples/<name>.b2c`.
pub fn example(name: &str) -> Vec<u8> {
    std::fs::read(repo_root().join("examples").join(format!("{name}.b2c"))).unwrap()
}

/// `examples/<name>.b2c` as text.
pub fn example_text(name: &str) -> String {
    String::from_utf8(example(name)).unwrap()
}

/// Every `examples/*.b2c`, by name.
pub fn examples() -> Vec<(String, Vec<u8>)> {
    let mut all: Vec<(String, Vec<u8>)> = std::fs::read_dir(repo_root().join("examples"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "b2c"))
        .map(|path| {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read(&path).unwrap())
        })
        .collect();
    all.sort();
    all
}

/// Edits a project's JSON text.
pub fn edit(text: &str, change: impl FnOnce(&mut serde_json::Value)) -> String {
    let mut value: serde_json::Value = serde_json::from_str(text).unwrap();
    change(&mut value);
    serde_json::to_string_pretty(&value).unwrap()
}

/// The g++ on `PATH`, if any; panics when there is none but
/// `B2C_REQUIRE_GXX` is set (as in CI).
pub fn gxx() -> Option<PathBuf> {
    let name = if cfg!(windows) { "g++.exe" } else { "g++" };
    let found = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    });
    assert!(
        found.is_some() || std::env::var_os("B2C_REQUIRE_GXX").is_none(),
        "B2C_REQUIRE_GXX is set but g++ was not found"
    );
    found.map(|path| b2c_toolchain::paths::canonical(&path).unwrap())
}

/// The probe of the real g++ (once per test binary), or `None` without one.
pub fn real_probe() -> Option<Toolchain> {
    static PROBED: OnceLock<Option<Toolchain>> = OnceLock::new();
    PROBED
        .get_or_init(|| {
            let path = gxx()?;
            let toolchain = b2c_toolchain::probe::probe(&path, &ProbeOptions::default()).unwrap();
            assert!(toolchain.is_usable(), "{:#?}", toolchain.problems);
            Some(toolchain)
        })
        .clone()
}

/// A usable GCC 13 record for the compiler at `fingerprint`, for tests that
/// never compile.
pub fn fake_gcc(fingerprint: Fingerprint) -> Toolchain {
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
    Toolchain {
        format: PROBE_FORMAT,
        fingerprint,
        kind: CompilerKind::Gcc,
        version: GccVersion::parse("13.3.0"),
        version_text: String::from("g++ 13.3.0"),
        target: Target::parse("x86_64-linux-gnu"),
        capabilities,
        problems: Vec::new(),
    }
}

/// A prober that never runs the compiler: every file is the real g++'s
/// probe (when there is a real g++) with the file's own fingerprint, or a
/// fake usable GCC 13. It counts its calls and can hold them until
/// [`TestProber::release`].
#[derive(Default)]
pub struct TestProber {
    calls: AtomicUsize,
    held: Mutex<bool>,
    released: Condvar,
}

impl TestProber {
    /// A prober that answers at once.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// A prober whose calls wait for [`TestProber::release`].
    pub fn held() -> Arc<Self> {
        Arc::new(Self {
            held: Mutex::new(true),
            ..Self::default()
        })
    }

    /// Lets held calls go on.
    pub fn release(&self) {
        *self.held.lock().unwrap() = false;
        self.released.notify_all();
    }

    /// How many probes ran (or started).
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Prober for TestProber {
    fn probe(&self, path: &Path, _options: &ProbeOptions) -> Result<Toolchain, ProbeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        {
            let held = self.held.lock().unwrap();
            let _released = self.released.wait_while(held, |held| *held).unwrap();
        }
        let fingerprint = Fingerprint::compute(path).map_err(|source| ProbeError::Unreadable {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(match real_probe() {
            Some(mut real) => {
                real.fingerprint = fingerprint;
                real
            }
            None => fake_gcc(fingerprint),
        })
    }
}

/// Native dialogs answered from scripts. An unscripted dialog is cancelled
/// (the trust dialog: Stay in Restricted Mode). Every trust prompt and save
/// suggestion is recorded. A dialog can be made to wait until
/// [`FakeDialogs::release`], to test that only one is open at a time.
#[derive(Default)]
pub struct FakeDialogs {
    open: Mutex<VecDeque<Option<PathBuf>>>,
    save_as: Mutex<VecDeque<Option<PathBuf>>>,
    compilers: Mutex<VecDeque<Option<PathBuf>>>,
    trust: Mutex<VecDeque<TrustChoice>>,
    prompts: Mutex<Vec<TrustPrompt>>,
    suggestions: Mutex<Vec<String>>,
    save_folders: Mutex<Vec<Option<PathBuf>>>,
    input_files: Mutex<VecDeque<Option<PathBuf>>>,
    export_folders: Mutex<VecDeque<Option<PathBuf>>>,
    replace_exports: Mutex<VecDeque<bool>>,
    calls: AtomicUsize,
    hold: Mutex<bool>,
    inside: AtomicUsize,
    changed: Condvar,
}

impl FakeDialogs {
    /// Dialogs with nothing scripted.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The next open dialog picks `path`.
    pub fn will_open(&self, path: &Path) {
        self.open.lock().unwrap().push_back(Some(path.to_path_buf()));
    }

    /// The next input-file dialog picks `path` (`None`: cancelled).
    pub fn will_pick_input(&self, path: Option<&Path>) {
        self.input_files
            .lock()
            .unwrap()
            .push_back(path.map(Path::to_path_buf));
    }

    /// The next export dialog picks `path` (`None`: cancelled).
    pub fn will_pick_export(&self, path: Option<&Path>) {
        self.export_folders
            .lock()
            .unwrap()
            .push_back(path.map(Path::to_path_buf));
    }

    /// The next "replace the export?" question is answered `replace`.
    pub fn will_replace_export(&self, replace: bool) {
        self.replace_exports.lock().unwrap().push_back(replace);
    }

    /// The next save dialog picks `path`.
    pub fn will_save_as(&self, path: &Path) {
        self.save_as.lock().unwrap().push_back(Some(path.to_path_buf()));
    }

    /// The next compiler dialog picks `path`.
    pub fn will_pick_compiler(&self, path: &Path) {
        self.compilers.lock().unwrap().push_back(Some(path.to_path_buf()));
    }

    /// The next trust dialog is answered with `choice`.
    pub fn will_trust(&self, choice: TrustChoice) {
        self.trust.lock().unwrap().push_back(choice);
    }

    /// The trust prompts shown so far.
    pub fn prompts(&self) -> Vec<TrustPrompt> {
        self.prompts.lock().unwrap().clone()
    }

    /// The file names the save dialog suggested so far.
    pub fn suggestions(&self) -> Vec<String> {
        self.suggestions.lock().unwrap().clone()
    }

    /// The folders the save dialog was asked to start in so far.
    pub fn save_folders(&self) -> Vec<Option<PathBuf>> {
        self.save_folders.lock().unwrap().clone()
    }

    /// How many dialogs were shown.
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    /// Makes later dialogs wait until [`FakeDialogs::release`].
    pub fn hold(&self) {
        *self.hold.lock().unwrap() = true;
    }

    /// Lets waiting dialogs answer.
    pub fn release(&self) {
        *self.hold.lock().unwrap() = false;
        self.changed.notify_all();
    }

    /// Waits until a dialog is open (at most 10 s).
    pub fn wait_inside(&self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.inside.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < deadline, "no dialog was opened");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn shown(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inside.fetch_add(1, Ordering::SeqCst);
        let held = self.hold.lock().unwrap();
        let _released = self.changed.wait_while(held, |held| *held).unwrap();
        self.inside.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Dialogs for FakeDialogs {
    fn open_project(&self) -> Option<PathBuf> {
        self.shown();
        self.open.lock().unwrap().pop_front().flatten()
    }

    fn save_project_as(&self, suggested_file_name: &str, folder: Option<&Path>) -> Option<PathBuf> {
        self.shown();
        self.suggestions
            .lock()
            .unwrap()
            .push(suggested_file_name.to_owned());
        self.save_folders
            .lock()
            .unwrap()
            .push(folder.map(Path::to_path_buf));
        self.save_as.lock().unwrap().pop_front().flatten()
    }

    fn pick_compiler(&self) -> Option<PathBuf> {
        self.shown();
        self.compilers.lock().unwrap().pop_front().flatten()
    }

    fn pick_input_file(&self, _folder: Option<&Path>) -> Option<PathBuf> {
        self.shown();
        self.input_files.lock().unwrap().pop_front().flatten()
    }

    fn pick_export_folder(&self, _folder: Option<&Path>) -> Option<PathBuf> {
        self.shown();
        self.export_folders.lock().unwrap().pop_front().flatten()
    }

    fn confirm_replace_export(&self, _folder_name: &str) -> bool {
        self.shown();
        self.replace_exports.lock().unwrap().pop_front().unwrap_or(false)
    }

    fn confirm_trust(&self, prompt: &TrustPrompt) -> TrustChoice {
        self.shown();
        self.prompts.lock().unwrap().push(prompt.clone());
        self.trust
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(TrustChoice::StayRestricted)
    }
}

/// An opener that records the URLs instead of starting a browser.
#[derive(Default)]
pub struct FakeOpener {
    urls: Mutex<Vec<String>>,
}

impl FakeOpener {
    /// The URLs opened so far.
    pub fn urls(&self) -> Vec<String> {
        self.urls.lock().unwrap().clone()
    }
}

impl UrlOpener for FakeOpener {
    fn open(&self, url: &'static str) -> std::io::Result<()> {
        self.urls.lock().unwrap().push(url.to_owned());
        Ok(())
    }
}

/// Writes an executable shell script and returns its canonical path.
#[cfg(unix)]
pub fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    let temp = dir.join(format!(".{name}.tmp"));
    std::fs::write(&temp, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::rename(&temp, &path).unwrap();
    b2c_toolchain::paths::canonical(&path).unwrap()
}

/// Which compiler a [`TestApp`] finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compiler {
    /// None: discovery searches an empty folder.
    None,
    /// A spawn spy: a `g++` script that writes the marker file
    /// ([`TestApp::spawned`]) and then runs the real g++ (or fails without
    /// one).
    Spy,
    /// A `g++` script that waits `seconds` and then runs the real g++.
    Slow(u32),
    /// Discovery searches the real g++'s own folder. Windows uses this
    /// instead of [`Compiler::Spy`]: a script cannot stand in for `g++.exe`
    /// there, and a copied driver would not find its own programs.
    Real,
}

impl Compiler {
    /// The spawn spy where scripts can wrap g++, otherwise the real g++
    /// ([`TestApp::spawned`] then looks for the program it built).
    pub fn spy() -> Self {
        if cfg!(unix) { Self::Spy } else { Self::Real }
    }
}

/// A backend whose folders are all inside one temporary directory, with
/// fake dialogs, opener and prober.
pub struct TestApp {
    pub root: tempfile::TempDir,
    pub backend: Arc<Backend>,
    pub dialogs: Arc<FakeDialogs>,
    pub opener: Arc<FakeOpener>,
    pub prober: Arc<TestProber>,
    pub dirs: Dirs,
    /// The one folder discovery searches.
    scope: PathBuf,
}

impl TestApp {
    /// A backend that finds no compiler.
    pub fn new() -> Self {
        Self::with(Compiler::None, TestProber::new())
    }

    /// A backend with `compiler` and `prober`.
    pub fn with(compiler: Compiler, prober: Arc<TestProber>) -> Self {
        Self::with_upgrades(compiler, prober, BlockUpgrades::SHIPPED)
    }

    /// A backend that finds no compiler and upgrades blocks with the shipped
    /// tables and the test-only ones of [`TEST_UPGRADES`].
    pub fn with_test_upgrades() -> Self {
        Self::with_upgrades(Compiler::None, TestProber::new(), TEST_UPGRADES)
    }

    /// A backend with `compiler`, `prober` and the block upgrade tables
    /// `block_upgrades`.
    pub fn with_upgrades(
        compiler: Compiler,
        prober: Arc<TestProber>,
        block_upgrades: BlockUpgrades<'static>,
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        // The plain canonical form: on Windows `canonicalize` gives a `\\?\` path, which
        // MinGW g++ cannot open sources under. The app's own folders are never verbatim.
        let root_path = b2c_toolchain::paths::canonical(root.path()).unwrap();
        let bin = root_path.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let real = gxx().map(|path| path.display().to_string());
        let marker = root_path.join("spawned");
        let run_real = real.map_or_else(|| String::from("exit 1"), |real| format!("exec '{real}' \"$@\""));
        let scope = match (compiler, gxx()) {
            (Compiler::Real, Some(real)) => real.parent().map_or_else(|| bin.clone(), Path::to_path_buf),
            _ => bin.clone(),
        };
        #[cfg(unix)]
        match compiler {
            Compiler::None | Compiler::Real => {}
            Compiler::Spy => {
                write_script(
                    &bin,
                    "g++",
                    &format!("echo spawned >> '{}'\n{run_real}", marker.display()),
                );
            }
            Compiler::Slow(seconds) => {
                write_script(&bin, "g++", &format!("sleep {seconds}\n{run_real}"));
            }
        }
        #[cfg(not(unix))]
        let _ = (compiler, marker, run_real);
        let cwd = root_path.join("cwd");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(root_path.join("projects")).unwrap();
        let dirs = Dirs::under_root(&root_path.join("app"));
        let dialogs = FakeDialogs::new();
        let opener = Arc::new(FakeOpener::default());
        let backend = Backend::start_with_block_upgrades(
            BackendConfig {
                dirs: dirs.clone(),
                app_version: "0.1.0",
                discovery_scope: DiscoveryScope::Only(vec![scope.clone()]),
                cwd: Some(cwd),
            },
            dialogs.clone(),
            Services {
                prober: prober.clone(),
                opener: opener.clone(),
            },
            block_upgrades,
        )
        .unwrap();
        Self {
            root,
            backend,
            dialogs,
            opener,
            prober,
            dirs,
            scope,
        }
    }

    /// Where discovery finds g++: the spy in `bin`, or the real g++ for
    /// [`Compiler::Real`].
    pub fn compiler(&self) -> PathBuf {
        self.scope.join(if cfg!(windows) { "g++.exe" } else { "g++" })
    }

    /// The root folder (canonical).
    pub fn root(&self) -> PathBuf {
        b2c_toolchain::paths::canonical(self.root.path()).unwrap()
    }

    /// Where tests put project files.
    pub fn projects(&self) -> PathBuf {
        self.root().join("projects")
    }

    /// Writes a project file and returns its path.
    pub fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.projects().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// Opens `path` through the open dialog.
    pub fn open(&self, path: &Path) -> ProjectOpened {
        self.dialogs.will_open(path);
        match self.backend.project_open_dialog().unwrap() {
            ProjectOpenDialogResponse::Ok(opened) => opened,
            ProjectOpenDialogResponse::Cancelled => panic!("the open dialog was cancelled"),
        }
    }

    /// Whether the spawn spy ran.
    pub fn spawned(&self) -> bool {
        self.root().join("spawned").exists() || (cfg!(windows) && contains_exe(&self.dirs.builds()))
    }

    /// Whether anything was written below the build cache's `builds/`.
    pub fn has_builds(&self) -> bool {
        std::fs::read_dir(self.dirs.builds()).is_ok_and(|mut entries| entries.next().is_some())
    }

    /// The trust store's JSON, or `None` without a file.
    pub fn trust_json(&self) -> Option<serde_json::Value> {
        let bytes = std::fs::read(self.dirs.machine.join("trust.json")).ok()?;
        Some(serde_json::from_slice(&bytes).unwrap())
    }

    /// Waits until background discovery has finished.
    pub fn wait_discovery(&self) {
        let deadline = Instant::now() + Duration::from_mins(1);
        while self.backend.toolchain_list().unwrap().discovering {
            assert!(Instant::now() < deadline, "discovery did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Starts a build of `document` for `handle`.
    pub fn build(&self, handle: &Handle, document: &str) -> (BuildId, Arc<RecordingSink<BuildEvent>>) {
        let sink = Arc::new(RecordingSink::new());
        let id = self
            .backend
            .build_start(
                BuildStartRequest {
                    handle: handle.clone(),
                    document: document.to_owned(),
                    config: BuildConfig::Debug,
                },
                sink.clone(),
            )
            .unwrap()
            .build_id;
        (id, sink)
    }
}

/// How long a build or run may take in these tests.
pub const TIMEOUT: Duration = Duration::from_mins(3);

/// Waits until `done` holds for the events of `sink`, and returns them.
pub fn wait_events<T: Clone>(sink: &RecordingSink<T>, done: impl Fn(&[T]) -> bool) -> Vec<T> {
    let started = Instant::now();
    loop {
        let events = sink.events();
        if done(&events) {
            return events;
        }
        assert!(started.elapsed() < TIMEOUT, "the events did not arrive in time");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Waits for a build's `finished` event and returns every event.
pub fn wait_finished(sink: &RecordingSink<BuildEvent>) -> Vec<BuildEvent> {
    wait_events(sink, |events| {
        events
            .iter()
            .any(|event| matches!(event, BuildEvent::Finished { .. }))
    })
}

/// Waits for a run's `exit` event and returns every event.
pub fn wait_exit(sink: &RecordingSink<RunEvent>) -> Vec<RunEvent> {
    wait_events(sink, |events| {
        events.iter().any(|event| matches!(event, RunEvent::Exit { .. }))
    })
}

/// A run's two recording channels.
pub fn run_sinks() -> (Arc<RecordingBytes>, Arc<RecordingSink<RunEvent>>) {
    (Arc::new(RecordingBytes::new()), Arc::new(RecordingSink::new()))
}

/// A recording app-event channel.
pub fn app_sink() -> Arc<RecordingSink<AppEvent>> {
    Arc::new(RecordingSink::new())
}

/// Whether `dir` holds an `.exe` file at any depth (links are not followed).
fn contains_exe(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => contains_exe(&path),
                Ok(kind) if kind.is_file() => path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe")),
                _ => false,
            }
        })
    })
}
