//! Build sessions end to end (`docs/spec/07-toolchain-build-run.md` §7.5,
//! `docs/spec/02-architecture.md` §2.5.3 and §2.6): events, outcomes, the
//! build manifest, cancellation, the IDE init unit, the toolchain re-check,
//! the build folder lock and the code style.
//!
//! The tests that compile need g++ on `PATH`; without one they are skipped,
//! unless `B2C_REQUIRE_GXX` is set (as in CI). The tests with fake compilers
//! (shell scripts) run on Unix only.

// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;
#[cfg(target_os = "linux")]
use std::time::Instant;

#[cfg(unix)]
use b2c_build::cache;
use b2c_build::{
    BuildJob, BuildOutcome as SyncOutcome, BuildRequest, BuildSessions, Configuration, FrontendOptions,
    RecordOutcome, StaleReason, ToolchainChoice, ToolchainForBuild,
};
use b2c_ipc::dto::{BuildEvent, BuildOutcome, BuildStage};
use b2c_ipc::sink::testing::RecordingSink;
use b2c_ipc::{BuildId, EventSink, IpcError};
use b2c_process::CancelToken;
use common::{
    BUILD_TIMEOUT, bytes, diagnostics, example, example_json, job, outcome, progress, ready, real_toolchain,
    sink, unavailable, wait_finished,
};

/// Starts `job`, waits for its finished event, and returns the events.
fn run(sessions: &BuildSessions, job: BuildJob) -> (BuildId, Vec<BuildEvent>) {
    let events = sink();
    let id = sessions.start(job, events.clone()).unwrap();
    let all = wait_finished(&events, BUILD_TIMEOUT);
    assert!(sessions.wait_idle(Duration::from_secs(30)));
    (id, all)
}

/// The build folder (`builds/<project>/<config>`) of a record.
fn build_dir(sessions: &BuildSessions, id: &BuildId) -> PathBuf {
    sessions.record(id).unwrap().build_dir.clone().unwrap()
}

/// The manifest of a build folder, as JSON.
fn manifest(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(dir.join("build-manifest.json")).unwrap()).unwrap()
}

#[test]
fn the_guessing_game_builds_with_a_manifest() {
    let Some(toolchain) = real_toolchain() else {
        return;
    };
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let document = example("guessing_game");
    let (id, events) = run(&sessions, job("ph_game", document.clone(), ready(toolchain)));
    let (result, hash) = outcome(&events);
    assert_eq!(result, BuildOutcome::Built, "{:#?}", diagnostics(&events));

    // The events in order: generate, then one compile-and-link step.
    assert_eq!(
        progress(&events),
        [
            (String::from("generate"), 0, 1),
            (String::from("generate"), 1, 1),
            (String::from("compile"), 0, 1),
            (String::from("compile"), 1, 1),
            (String::from("link"), 1, 1),
        ]
    );
    assert!(matches!(
        events.first(),
        Some(BuildEvent::Progress { done: 0, .. })
    ));

    // The project hash is the content hash of the document, in the event,
    // the record and the manifest.
    let expected = b2c_model::hex(&b2c_model::content_hash(&b2c_model::load(&document).unwrap()));
    assert_eq!(hash.as_deref(), Some(expected.as_str()));
    let record = sessions.record(&id).unwrap();
    assert_eq!(record.outcome, RecordOutcome::Built);
    assert_eq!(
        record.project_hash.map(|hash| b2c_model::hex(&hash)),
        Some(expected.clone())
    );
    assert_eq!(record.project_key, "ph_game");
    assert!(record.ide);
    assert!(record.document.is_some());
    assert!(record.config_key.starts_with("debug-"), "{}", record.config_key);
    let executable = record.executable.clone().unwrap();
    assert!(executable.is_file());
    let dir = record.build_dir.clone().unwrap();
    assert!(executable.starts_with(dir.join("out")));
    assert_eq!(
        dir.file_name().unwrap().to_str(),
        Some(record.config_key.as_str())
    );
    let manifest = manifest(&dir);
    assert_eq!(manifest["format"], "blocks2cpp/build-manifest");
    assert_eq!(manifest["formatVersion"], 1);
    assert_eq!(manifest["projectHash"], expected.as_str());
    assert_eq!(manifest["ide"], true);
    assert_eq!(manifest["result"], "success");
    assert_eq!(manifest["steps"][0]["kind"], "compileAndLink");
    let argv = manifest["steps"][0]["argv"].to_string();
    assert!(argv.contains("b2c_ide_init.cpp"), "{argv}");
    assert!(argv.contains("main.cpp"), "{argv}");
    assert_eq!(
        manifest["executable"]["size"],
        std::fs::metadata(&executable).unwrap().len()
    );
    assert!(!dir.join("build-stamp").exists());
    assert!(dir.join("sourcemap.json").is_file());
    assert!(dir.join("ide").join("b2c_ide_init.cpp").is_file());
    record.verify_executable().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&executable).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        let mode = std::fs::metadata(dir.join("build-manifest.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

/// An analyser error ends the build before any compiler is chosen or run.
#[cfg(unix)]
#[test]
fn analyser_errors_never_start_a_compiler() {
    let Some(real) = real_toolchain() else {
        return;
    };
    let scripts = tempfile::tempdir().unwrap();
    let marker = scripts.path().join("compiler-ran");
    let script = common::write_script(
        scripts.path(),
        "g++",
        &format!(
            "touch '{}'\nexec '{}' \"$@\"",
            marker.display(),
            real.path().display()
        ),
    );
    let mut document = example_json("hello_world");
    document["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"][0]["inputs"]["ITEM0"]["expr"] =
        serde_json::json!([{"ref": "s_missing"}]);
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let (id, events) = run(
        &sessions,
        job(
            "ph_broken",
            bytes(&document),
            ready(common::fake_toolchain(&real, &script)),
        ),
    );
    assert_eq!(outcome(&events).0, BuildOutcome::ProjectErrors);
    let found = diagnostics(&events);
    assert!(
        found
            .iter()
            .any(|d| d.code.starts_with("B2C-E") && d.severity == b2c_ipc::diag::Severity::Error),
        "{found:#?}"
    );
    assert!(!marker.exists(), "the compiler ran");
    // Nothing was prepared in the cache.
    assert!(!cache.path().join("builds").exists());
    let record = sessions.record(&id).unwrap();
    assert_eq!(record.config_key, "");
    assert_eq!(record.build_dir, None);
    assert_eq!(record.executable, None);
    assert!(record.project_hash.is_some());
    assert_eq!(record.verify_executable(), Err(StaleReason::NotBuilt));

    // The command-line build does not even choose a compiler.
    let report = b2c_build::build(
        &bytes(&document),
        &BuildRequest {
            configuration: Configuration::Debug,
            toolchain: ToolchainChoice::Path(script),
            cache_root: cache.path().to_path_buf(),
            frontend: FrontendOptions::default(),
            ide: false,
        },
    )
    .unwrap();
    assert_eq!(report.outcome, SyncOutcome::ProjectErrors);
    assert!(!marker.exists(), "the compiler ran");
}

/// A document that does not load is reported with no project hash.
#[test]
fn a_document_that_does_not_load_is_a_project_error() {
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let unavailable = ToolchainForBuild::Unavailable {
        diagnostics: vec![b2c_toolchain::codes::no_toolchain()],
    };
    let (_, events) = run(&sessions, job("ph_bad", b"{".to_vec(), unavailable.clone()));
    let (result, hash) = outcome(&events);
    assert_eq!(result, BuildOutcome::ProjectErrors);
    assert_eq!(hash, None);
    // Without a compiler, a valid project is a toolchain problem, with the
    // reason in the diagnostics.
    let (_, events) = run(&sessions, job("ph_bad", example("hello_world"), unavailable));
    let (result, hash) = outcome(&events);
    assert_eq!(result, BuildOutcome::ToolchainProblem);
    assert!(hash.is_some());
    assert!(diagnostics(&events).iter().any(|d| d.code == "B2C-T1001"));
}

#[test]
fn a_second_unchanged_build_is_up_to_date() {
    let Some(toolchain) = real_toolchain() else {
        return;
    };
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let (first, events) = run(
        &sessions,
        job("ph_hello", example("hello_world"), ready(toolchain.clone())),
    );
    assert_eq!(outcome(&events).0, BuildOutcome::Built);
    let executable = sessions.record(&first).unwrap().executable.clone().unwrap();
    let built_at = std::fs::metadata(&executable).unwrap().modified().unwrap();
    let lock = build_dir(&sessions, &first).join("lock");
    let used_at = std::fs::metadata(&lock).unwrap().modified().unwrap();
    std::thread::sleep(Duration::from_millis(50));

    let (second, events) = run(
        &sessions,
        job("ph_hello", example("hello_world"), ready(toolchain.clone())),
    );
    assert_eq!(outcome(&events).0, BuildOutcome::UpToDate);
    assert_eq!(
        progress(&events),
        [(String::from("generate"), 0, 1), (String::from("generate"), 1, 1)]
    );
    let record = sessions.record(&second).unwrap();
    assert_eq!(record.executable.as_ref(), Some(&executable));
    assert_eq!(
        std::fs::metadata(&executable).unwrap().modified().unwrap(),
        built_at
    );
    // The folder was marked as used again (cache eviction reads this).
    assert!(std::fs::metadata(&lock).unwrap().modified().unwrap() > used_at);
    record.verify_executable().unwrap();

    // A changed program is noticed: by the next build, which compiles
    // again, and before that by the record.
    let mut contents = std::fs::read(&executable).unwrap();
    contents.push(0);
    std::fs::write(&executable, &contents).unwrap();
    assert_eq!(record.verify_executable(), Err(StaleReason::ExecutableChanged));
    let (third, events) = run(
        &sessions,
        job("ph_hello", example("hello_world"), ready(toolchain.clone())),
    );
    assert_eq!(outcome(&events).0, BuildOutcome::Built);
    sessions.record(&third).unwrap().verify_executable().unwrap();
    // A build folder without its manifest is reported, and never up to date.
    std::fs::remove_file(build_dir(&sessions, &third).join("build-manifest.json")).unwrap();
    assert_eq!(
        sessions.record(&third).unwrap().verify_executable(),
        Err(StaleReason::ManifestMissing)
    );
    // Without a manifest the build is never up to date.
    let (_, events) = run(
        &sessions,
        job("ph_hello", example("hello_world"), ready(toolchain)),
    );
    assert_eq!(outcome(&events).0, BuildOutcome::Built);
}

#[test]
fn a_changed_project_is_built_again_and_old_records_go_stale() {
    let Some(toolchain) = real_toolchain() else {
        return;
    };
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let (first, events) = run(
        &sessions,
        job("ph_hello", example("hello_world"), ready(toolchain.clone())),
    );
    assert_eq!(outcome(&events).0, BuildOutcome::Built);
    let mut changed = example_json("hello_world");
    changed["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"][0]["inputs"]["ITEM0"]["expr"] =
        serde_json::json!([{"str": "Hello again!"}]);
    let (second, events) = run(&sessions, job("ph_hello", bytes(&changed), ready(toolchain)));
    assert_eq!(outcome(&events).0, BuildOutcome::Built);
    assert_eq!(
        sessions.record(&first).unwrap().build_dir,
        sessions.record(&second).unwrap().build_dir
    );
    // The first build's program was replaced by the second's.
    assert_eq!(
        sessions.record(&first).unwrap().verify_executable(),
        Err(StaleReason::ManifestMismatch)
    );
    sessions.record(&second).unwrap().verify_executable().unwrap();
}

/// With warnings as errors, g++'s unused-variable warning fails the build
/// and lands on the variable's block.
#[test]
fn compiler_errors_are_mapped_to_their_block() {
    let Some(toolchain) = real_toolchain() else {
        return;
    };
    let mut document = example_json("hello_world");
    document["project"]["build"]["configurations"]["debug"]["warningsAsErrors"] = serde_json::json!(true);
    let body = document["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"]
        .as_array_mut()
        .unwrap();
    body.push(serde_json::json!({
        "id": "b100",
        "type": "var.declare",
        "v": 1,
        "fields": {"CONST": false, "NAME": {"sym": "s_unused", "name": "unused"}, "TYPE": "int"},
        "inputs": {"VALUE": {"expr": [{"num": "7"}]}}
    }));
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let (id, events) = run(&sessions, job("ph_unused", bytes(&document), ready(toolchain)));
    assert_eq!(outcome(&events).0, BuildOutcome::ProjectErrors);
    let found = diagnostics(&events);
    let unused = found
        .iter()
        .find(|d| d.code == "C:-Wunused-variable")
        .unwrap_or_else(|| panic!("{found:#?}"));
    assert_eq!(unused.severity, b2c_ipc::diag::Severity::Error);
    assert_eq!(unused.primary.block.as_deref(), Some("b100"));
    assert!(unused.raw.as_deref().is_some_and(|raw| raw.contains("unused")));
    // A failed build leaves no manifest and no program.
    let record = sessions.record(&id).unwrap();
    let dir = record.build_dir.clone().unwrap();
    assert!(!dir.join("build-manifest.json").exists());
    let program = if cfg!(windows) { "main.exe" } else { "main" };
    assert!(!dir.join("out").join(program).exists());
    assert_eq!(record.executable, None);
}

/// Several translation units compile in parallel, report progress per unit,
/// and then link.
#[test]
fn several_translation_units_compile_then_link() {
    let Some(toolchain) = real_toolchain() else {
        return;
    };
    let mut document = example_json("hello_world");
    for (id, name) in [("mod_util", "util"), ("mod_more", "more")] {
        document["modules"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"id": id, "name": name, "workspace": {"blocks": []}}));
    }
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let (id, events) = run(&sessions, job("ph_three", bytes(&document), ready(toolchain)));
    assert_eq!(
        outcome(&events).0,
        BuildOutcome::Built,
        "{:#?}",
        diagnostics(&events)
    );
    // Three modules plus the IDE init unit.
    assert_eq!(
        progress(&events),
        [
            (String::from("generate"), 0, 1),
            (String::from("generate"), 1, 1),
            (String::from("compile"), 0, 4),
            (String::from("compile"), 1, 4),
            (String::from("compile"), 2, 4),
            (String::from("compile"), 3, 4),
            (String::from("compile"), 4, 4),
            (String::from("link"), 0, 1),
            (String::from("link"), 1, 1),
        ]
    );
    let dir = build_dir(&sessions, &id);
    let steps = manifest(&dir)["steps"].clone();
    let kinds: Vec<&str> = steps
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["compile", "compile", "compile", "compile", "link"]);
    assert!(steps[3]["argv"].to_string().contains("b2c_ide_init.cpp"));
    sessions.record(&id).unwrap().verify_executable().unwrap();
}

/// Starting a second build of the same project cancels the first.
#[cfg(unix)]
#[test]
fn a_new_build_cancels_the_running_one() {
    let Some(real) = real_toolchain() else {
        return;
    };
    let scripts = tempfile::tempdir().unwrap();
    // Slow enough that the first build is still running when the second
    // starts.
    let script = common::write_script(
        scripts.path(),
        "g++",
        &format!("sleep 2\nexec '{}' \"$@\"", real.path().display()),
    );
    let fake = common::fake_toolchain(&real, &script);
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let first_events = sink();
    let first = sessions
        .start(
            job("ph_same", example("hello_world"), ready(fake.clone())),
            first_events.clone(),
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let second_events = sink();
    let second = sessions
        .start(
            job("ph_same", example("hello_world"), ready(fake)),
            second_events.clone(),
        )
        .unwrap();
    let first_all = wait_finished(&first_events, BUILD_TIMEOUT);
    let second_all = wait_finished(&second_events, BUILD_TIMEOUT);
    assert!(sessions.wait_idle(Duration::from_secs(30)));
    assert_eq!(outcome(&first_all).0, BuildOutcome::Cancelled);
    assert_eq!(outcome(&second_all).0, BuildOutcome::Built);
    assert_eq!(sessions.record(&first).unwrap().outcome, RecordOutcome::Cancelled);
    assert_eq!(sessions.record(&first).unwrap().executable, None);
    sessions.record(&second).unwrap().verify_executable().unwrap();
    // Cancelling a finished build does nothing; an unknown one is an error.
    sessions.cancel(&first).unwrap();
    sessions.cancel(&second).unwrap();
    assert_eq!(sessions.cancel(&BuildId::example()), Err(IpcError::UnknownBuild));
}

/// Whether a process is still alive (a zombie waiting to be reaped is not).
#[cfg(target_os = "linux")]
fn alive(pid: &str) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().next())
            .is_some_and(|state| state != "Z" && state != "X")
    })
}

/// Cancelling kills a compiler that ignores `SIGTERM`, and its child, within
/// the 2 s grace and a margin (07 §7.5.4); the build ends as cancelled and
/// leaves no manifest or program.
#[cfg(target_os = "linux")]
#[test]
fn cancel_kills_the_whole_compiler_tree() {
    let Some(real) = real_toolchain() else {
        return;
    };
    let scripts = tempfile::tempdir().unwrap();
    let pids = scripts.path().join("pids");
    let script = common::write_script(
        scripts.path(),
        "g++",
        &format!(
            "trap '' TERM\nsleep 600 &\necho $! > '{pids}.tmp'\necho $$ >> '{pids}.tmp'\nmv '{pids}.tmp' '{pids}'\nwait",
            pids = pids.display()
        ),
    );
    let fake = common::fake_toolchain(&real, &script);
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let events = sink();
    let id = sessions
        .start(
            job("ph_cancel", example("hello_world"), ready(fake)),
            events.clone(),
        )
        .unwrap();
    let started = Instant::now();
    while !pids.exists() {
        assert!(
            started.elapsed() < BUILD_TIMEOUT,
            "the fake compiler never started"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let pids = std::fs::read_to_string(&pids).unwrap();
    let pids: Vec<&str> = pids.split_whitespace().collect();
    assert_eq!(pids.len(), 2);
    assert!(pids.iter().all(|pid| alive(pid)));
    // Running builds have no record yet.
    assert!(sessions.record(&id).is_none());

    let cancelled_at = Instant::now();
    sessions.cancel(&id).unwrap();
    let all = wait_finished(&events, Duration::from_secs(30));
    let took = cancelled_at.elapsed();
    assert_eq!(outcome(&all).0, BuildOutcome::Cancelled);
    assert!(took < Duration::from_secs(3), "cancelling took {took:?}");
    for pid in &pids {
        assert!(!alive(pid), "process {pid} survived the cancel");
    }
    assert!(sessions.wait_idle(Duration::from_secs(30)));
    let record = sessions.record(&id).unwrap();
    assert_eq!(record.outcome, RecordOutcome::Cancelled);
    let dir = record.build_dir.clone().unwrap();
    assert!(!dir.join("build-manifest.json").exists());
    assert!(std::fs::read_dir(dir.join("out")).unwrap().next().is_none());
}

/// A compiler that changed since it was probed is probed again before the
/// build, with `B2C-T1009` among the diagnostics.
#[cfg(unix)]
#[test]
fn a_changed_compiler_is_checked_again() {
    let Some(real) = real_toolchain() else {
        return;
    };
    let scripts = tempfile::tempdir().unwrap();
    let body = format!("exec '{}' \"$@\"", real.path().display());
    let script = common::write_script(scripts.path(), "g++", &body);
    let fake = common::fake_toolchain(&real, &script);
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let (_, events) = run(
        &sessions,
        job("ph_changed", example("hello_world"), ready(fake.clone())),
    );
    assert_eq!(outcome(&events).0, BuildOutcome::Built);
    assert!(!diagnostics(&events).iter().any(|d| d.code == "B2C-T1009"));

    // The "compiler" is updated.
    common::write_script(scripts.path(), "g++", &format!("# updated\n{body}"));
    let (id, events) = run(&sessions, job("ph_changed", example("hello_world"), ready(fake)));
    assert_eq!(
        outcome(&events).0,
        BuildOutcome::Built,
        "{:#?}",
        diagnostics(&events)
    );
    let changed = diagnostics(&events);
    let note = changed
        .iter()
        .find(|d| d.code == "B2C-T1009")
        .unwrap_or_else(|| panic!("{changed:#?}"));
    assert_eq!(note.severity, b2c_ipc::diag::Severity::Info);
    // The new fingerprint is in the manifest.
    let recorded = manifest(&build_dir(&sessions, &id));
    assert_eq!(
        recorded["toolchain"]["sha256"],
        b2c_toolchain::fingerprint::Fingerprint::compute(&script)
            .unwrap()
            .sha256
    );

    // A compiler that disappeared is a toolchain problem.
    std::fs::remove_file(&script).unwrap();
    let mut gone = real.clone();
    gone.fingerprint.path = script.clone();
    let (_, events) = run(&sessions, job("ph_changed", example("hello_world"), ready(gone)));
    assert_eq!(outcome(&events).0, BuildOutcome::ToolchainProblem);
    assert!(diagnostics(&events).iter().any(|d| d.code == "B2C-T1003"));
}

/// IDE and command-line builds of one project never share a build folder,
/// and only IDE builds contain the init unit.
#[test]
fn ide_and_cli_builds_use_different_folders() {
    let Some(toolchain) = real_toolchain() else {
        return;
    };
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let (id, events) = run(
        &sessions,
        job("ph_both", example("hello_world"), ready(toolchain.clone())),
    );
    assert_eq!(outcome(&events).0, BuildOutcome::Built);
    let ide_dir = build_dir(&sessions, &id);

    let report = b2c_build::build(
        &example("hello_world"),
        &BuildRequest {
            configuration: Configuration::Debug,
            toolchain: ToolchainChoice::Path(toolchain.path().to_path_buf()),
            cache_root: cache.path().to_path_buf(),
            frontend: FrontendOptions::default(),
            ide: false,
        },
    )
    .unwrap();
    let SyncOutcome::Built { executable } = report.outcome else {
        panic!("{:#?}", report.diagnostics);
    };
    let cli_dir = executable.parent().unwrap().parent().unwrap().to_path_buf();
    assert_ne!(cli_dir, ide_dir);
    assert_eq!(cli_dir.parent(), ide_dir.parent(), "the same project folder");
    assert_eq!(manifest(&ide_dir)["ide"], true);
    assert_eq!(manifest(&cli_dir)["ide"], false);
    assert!(ide_dir.join("ide/b2c_ide_init.cpp").is_file());
    assert!(!cli_dir.join("ide/b2c_ide_init.cpp").exists());
    assert!(!manifest(&cli_dir)["steps"].to_string().contains("b2c_ide"));
    // The generated code itself is identical.
    assert_eq!(
        std::fs::read(ide_dir.join("gen/main.cpp")).unwrap(),
        std::fs::read(cli_dir.join("gen/main.cpp")).unwrap()
    );
}

/// Two builds of the same project at once (two app instances, or the app
/// and the command-line tool) take turns: the second waits for the folder's
/// lock and then finds the program up to date.
#[cfg(unix)]
#[test]
fn concurrent_builds_of_one_project_wait_for_the_lock() {
    let Some(real) = real_toolchain() else {
        return;
    };
    let scripts = tempfile::tempdir().unwrap();
    let log = scripts.path().join("log");
    let script = common::write_script(
        scripts.path(),
        "g++",
        &format!(
            "echo start >> '{log}'\nsleep 1\n'{gxx}' \"$@\"\nstatus=$?\necho end >> '{log}'\nexit $status",
            log = log.display(),
            gxx = real.path().display()
        ),
    );
    let fake = common::fake_toolchain(&real, &script);
    let cache = tempfile::tempdir().unwrap();
    // Two independent session managers, like two app instances.
    let one = BuildSessions::new(cache.path().to_path_buf());
    let two = BuildSessions::new(cache.path().to_path_buf());
    let one_events = sink();
    let two_events = sink();
    one.start(
        job("ph_one", example("hello_world"), ready(fake.clone())),
        one_events.clone(),
    )
    .unwrap();
    two.start(
        job("ph_two", example("hello_world"), ready(fake)),
        two_events.clone(),
    )
    .unwrap();
    let mut outcomes = vec![
        outcome(&wait_finished(&one_events, BUILD_TIMEOUT)).0,
        outcome(&wait_finished(&two_events, BUILD_TIMEOUT)).0,
    ];
    outcomes.sort_by_key(|outcome| outcome.as_str());
    assert_eq!(outcomes, [BuildOutcome::Built, BuildOutcome::UpToDate]);
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "start\nend\n");
}

/// The indent width changes the generated files and the build folder.
#[test]
fn indent_widths_change_the_generated_files() {
    let Some(toolchain) = real_toolchain() else {
        return;
    };
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let mut folders = Vec::new();
    for indent_width in [2, 4] {
        let mut job = job("ph_indent", example("guessing_game"), ready(toolchain.clone()));
        job.frontend = FrontendOptions {
            indent_width,
            ..FrontendOptions::default()
        };
        let (id, events) = run(&sessions, job);
        assert_eq!(outcome(&events).0, BuildOutcome::Built);
        folders.push(build_dir(&sessions, &id));
    }
    assert_ne!(folders[0], folders[1]);
    let two = std::fs::read_to_string(folders[0].join("gen/main.cpp")).unwrap();
    let four = std::fs::read_to_string(folders[1].join("gen/main.cpp")).unwrap();
    // The first indented line shows the width.
    let first_indent = |text: &str| {
        text.lines()
            .find(|line| line.starts_with(' '))
            .map(|line| line.len() - line.trim_start_matches(' ').len())
    };
    assert_eq!(first_indent(&two), Some(2), "{two}");
    assert_eq!(first_indent(&four), Some(4), "{four}");
    // Exactly what the front end alone generates with that width.
    for (width, built) in [(2, &two), (4, &four)] {
        let generated = b2c_build::run_frontend(
            &example("guessing_game"),
            &FrontendOptions {
                indent_width: width,
                ..FrontendOptions::default()
            },
        )
        .generated
        .unwrap();
        let main = generated
            .files
            .iter()
            .find(|file| file.path == "main.cpp")
            .unwrap();
        assert_eq!(&main.contents, built);
    }
}

/// The init unit compiles without a single warning, also as an error, for
/// every supported standard in debug and release, alone and in a build.
#[test]
fn the_init_unit_compiles_without_warnings() {
    let Some(toolchain) = real_toolchain() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join(b2c_build::ide::INIT_UNIT_FILE);
    std::fs::write(&source, b2c_build::ide::INIT_UNIT_SOURCE).unwrap();
    for standard in ["c++17", "c++20", "c++23"] {
        for optimisation in ["-O0", "-O2"] {
            let mut command = b2c_process::Command::new(toolchain.path(), dir.path()).unwrap();
            command
                .args([
                    format!("-std={standard}"),
                    String::from(optimisation),
                    String::from("-Wall"),
                    String::from("-Wextra"),
                    String::from("-Wpedantic"),
                    String::from("-Werror"),
                    String::from("-c"),
                    source.to_string_lossy().into_owned(),
                    String::from("-o"),
                    dir.path().join("init.o").to_string_lossy().into_owned(),
                ])
                .env("PATH", toolchain.bin_dir());
            for name in ["SystemRoot", "TEMP", "TMP", "TMPDIR"] {
                if let Some(value) = std::env::var_os(name) {
                    command.env(name, value);
                }
            }
            let captured = b2c_process::run_captured(&command).unwrap();
            assert!(
                captured.status.success() && captured.stderr.is_empty(),
                "{standard} {optimisation}: {}",
                String::from_utf8_lossy(&captured.stderr)
            );
        }
    }

    // IDE builds with the strict warning level as errors, in debug and
    // release, for each standard.
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    for standard in ["c++17", "c++20", "c++23"] {
        for configuration in [Configuration::Debug, Configuration::Release] {
            let mut document = example_json("hello_world");
            document["project"]["language"]["standard"] = serde_json::json!(standard);
            for name in ["debug", "release"] {
                let settings = &mut document["project"]["build"]["configurations"][name];
                settings["warnings"] = serde_json::json!("strict");
                settings["warningsAsErrors"] = serde_json::json!(true);
            }
            let mut job = job("ph_strict", bytes(&document), ready(toolchain.clone()));
            job.configuration = configuration;
            let (_, events) = run(&sessions, job);
            let found = diagnostics(&events);
            assert_eq!(
                outcome(&events).0,
                BuildOutcome::Built,
                "{standard} {configuration:?}: {found:#?}"
            );
            assert!(
                found.iter().all(|d| d.severity == b2c_ipc::diag::Severity::Info),
                "{standard} {configuration:?}: {found:#?}"
            );
        }
    }
}

/// A module whose name Windows reserves for a device is refused on every
/// system, before anything is written (the loader refuses the name; the
/// build folder refuses such file names again, see `build_dir`).
#[test]
fn device_module_names_are_refused() {
    let mut document = example_json("hello_world");
    document["modules"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"id": "mod_con", "name": "con", "workspace": {"blocks": []}}));
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let unavailable = ToolchainForBuild::Unavailable {
        diagnostics: Vec::new(),
    };
    for name in ["con", "NUL", "com1", "lpt9"] {
        document["modules"][1]["name"] = serde_json::json!(name);
        let (id, events) = run(&sessions, job("ph_device", bytes(&document), unavailable.clone()));
        let (result, hash) = outcome(&events);
        assert_eq!(result, BuildOutcome::ProjectErrors, "{name}");
        assert_eq!(hash, None, "{name}");
        assert!(
            diagnostics(&events).iter().any(|d| d.code.starts_with("B2C-E01")),
            "{name}: {:#?}",
            diagnostics(&events)
        );
        assert_eq!(sessions.record(&id).unwrap().build_dir, None);
    }
    assert!(!cache.path().join("builds").exists());
}

/// Records are kept per project up to a limit, and dropped with the
/// project.
#[test]
fn records_are_bounded_and_dropped_with_their_project() {
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let unavailable = ToolchainForBuild::Unavailable {
        diagnostics: Vec::new(),
    };
    let mut ids = Vec::new();
    for _ in 0..b2c_build::session::MAX_RECORDS_PER_PROJECT + 2 {
        let (id, events) = run(&sessions, job("ph_many", b"{".to_vec(), unavailable.clone()));
        assert_eq!(outcome(&events).0, BuildOutcome::ProjectErrors);
        ids.push(id);
    }
    let (other, _) = run(&sessions, job("ph_other", b"{".to_vec(), unavailable));
    assert!(sessions.record(&ids[0]).is_none());
    assert!(sessions.record(&ids[1]).is_none());
    assert_eq!(sessions.cancel(&ids[0]), Err(IpcError::UnknownBuild));
    for id in &ids[2..] {
        assert!(sessions.record(id).is_some());
    }
    sessions.forget_project("ph_many");
    assert!(ids.iter().all(|id| sessions.record(id).is_none()));
    assert!(sessions.record(&other).is_some());
}

/// The finished hook sees every record, after the finished event.
#[test]
fn the_finished_hook_sees_every_build() {
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let hook_seen = Arc::clone(&seen);
    sessions.set_on_finished(Box::new(move |record| {
        hook_seen
            .lock()
            .unwrap()
            .push((record.build_id.clone(), record.outcome));
    }));
    let (id, _) = run(
        &sessions,
        job(
            "ph_hook",
            b"{".to_vec(),
            ToolchainForBuild::Unavailable {
                diagnostics: Vec::new(),
            },
        ),
    );
    assert_eq!(*seen.lock().unwrap(), [(id, RecordOutcome::ProjectErrors)]);
}

/// A sink that cancels its project's build as soon as the front end starts
/// (the `generate` 0/1 progress event), and records every event.
struct CancelWhenGenerating {
    sessions: Arc<BuildSessions>,
    key: &'static str,
    events: Arc<RecordingSink<BuildEvent>>,
}

impl EventSink<BuildEvent> for CancelWhenGenerating {
    fn send(&self, event: BuildEvent) -> bool {
        if matches!(
            event,
            BuildEvent::Progress {
                stage: BuildStage::Generate,
                done: 0,
                ..
            }
        ) {
            self.sessions.cancel_project(self.key);
        }
        self.events.send(event)
    }
}

/// A build cancelled while the front end runs stops there and ends as
/// `cancelled`, also when the project has errors (07 §7.5.4), and no
/// toolchain is chosen for it.
#[test]
fn a_build_cancelled_while_generating_ends_as_cancelled() {
    let cache = tempfile::tempdir().unwrap();
    let sessions = Arc::new(BuildSessions::new(cache.path().to_path_buf()));
    let mut broken = example_json("hello_world");
    broken["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"][0]["inputs"]["ITEM0"]["expr"] =
        serde_json::json!([{"ref": "s_missing"}]);
    for document in [bytes(&broken), b"{".to_vec(), example("hello_world")] {
        let asked = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&asked);
        let mut generating = job("ph_generating", document, unavailable());
        generating.toolchain = Box::new(move |_| {
            flag.store(true, Ordering::SeqCst);
            unavailable()
        });
        let events = sink();
        let canceller = Arc::new(CancelWhenGenerating {
            sessions: Arc::clone(&sessions),
            key: "ph_generating",
            events: events.clone(),
        });
        sessions.start(generating, canceller).unwrap();
        let all = wait_finished(&events, BUILD_TIMEOUT);
        assert_eq!(outcome(&all).0, BuildOutcome::Cancelled, "{all:#?}");
        assert!(
            diagnostics(&all).is_empty(),
            "nothing is reported after the cancel: {all:#?}"
        );
        assert!(!asked.load(Ordering::SeqCst), "no toolchain is chosen");
        assert!(sessions.wait_idle(Duration::from_secs(30)));
    }
}

/// The toolchain is chosen on the session's thread after `start` returned
/// the ID, so a choice that waits (for a toolchain discovery) never delays
/// `build_start`, and cancelling the build ends the wait with `cancelled`.
/// A project with errors never asks for a toolchain.
#[test]
fn the_toolchain_is_chosen_after_start_and_its_wait_can_be_cancelled() {
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let (entered, choosing) = mpsc::channel();
    let mut waiting = job("ph_lazy", example("hello_world"), unavailable());
    waiting.toolchain = Box::new(move |cancel: &CancelToken| {
        entered.send(()).unwrap();
        // Like waiting for a discovery: only the build's cancel ends it.
        while !cancel.is_cancelled() {
            std::thread::sleep(Duration::from_millis(5));
        }
        unavailable()
    });
    let events = sink();
    let id = sessions.start(waiting, events.clone()).unwrap();
    choosing.recv_timeout(BUILD_TIMEOUT).unwrap();
    assert!(sessions.record(&id).is_none(), "the build is still choosing");
    sessions.cancel(&id).unwrap();
    let all = wait_finished(&events, BUILD_TIMEOUT);
    assert_eq!(outcome(&all).0, BuildOutcome::Cancelled);
    assert!(
        !diagnostics(&all).iter().any(|d| d.code == "B2C-T1001"),
        "what a cancelled choice returned is not reported: {all:#?}"
    );
    assert!(sessions.wait_idle(Duration::from_secs(30)));

    let asked = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&asked);
    let mut broken = job("ph_lazy", b"{".to_vec(), unavailable());
    broken.toolchain = Box::new(move |_| {
        flag.store(true, Ordering::SeqCst);
        unavailable()
    });
    let (_, all) = run(&sessions, broken);
    assert_eq!(outcome(&all).0, BuildOutcome::ProjectErrors);
    assert!(!asked.load(Ordering::SeqCst));
    // The job hides the document's bytes in its debug form.
    let shown = format!("{:?}", job("ph_lazy", b"secret text".to_vec(), unavailable()));
    assert!(shown.contains("11 bytes") && !shown.contains("secret"), "{shown}");
}

/// A finished build marks its folder as used again, so eviction right after
/// it keeps the program that was just built even when another entry was
/// used while it compiled (07 §7.5.1), without being told which entry to
/// keep.
#[cfg(unix)]
#[test]
fn eviction_right_after_a_build_keeps_the_program_just_built() {
    let Some(real) = real_toolchain() else {
        return;
    };
    let cache = tempfile::tempdir().unwrap();
    // Another project's entry, used while this build compiles.
    let other = cache.path().join("builds/prj_other-00000000/debug-00000000");
    std::fs::create_dir_all(other.join("out")).unwrap();
    std::fs::write(other.join("out/program"), vec![0u8; 4096]).unwrap();
    std::fs::write(other.join("lock"), b"").unwrap();
    let scripts = tempfile::tempdir().unwrap();
    let script = common::write_script(
        scripts.path(),
        "g++",
        &format!(
            "touch '{}'\nexec '{}' \"$@\"",
            other.join("lock").display(),
            real.path().display()
        ),
    );
    let fake = common::fake_toolchain(&real, &script);
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let root = cache.path().to_path_buf();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let hook_seen = Arc::clone(&seen);
    sessions.set_on_finished(Box::new(move |record| {
        let report = cache::evict_to_cap(&root, &cache::EvictionPolicy::with_max_bytes(1)).unwrap();
        hook_seen
            .lock()
            .unwrap()
            .push((report, record.verify_executable()));
    }));
    let (id, events) = run(&sessions, job("ph_evict", example("hello_world"), ready(fake)));
    assert_eq!(
        outcome(&events).0,
        BuildOutcome::Built,
        "{:#?}",
        diagnostics(&events)
    );
    let (report, verified) = seen.lock().unwrap().remove(0);
    assert_eq!(verified, Ok(()), "{report:?}");
    assert_eq!(report.removed, 1, "{report:?}");
    assert!(!other.exists());
    sessions.record(&id).unwrap().verify_executable().unwrap();
}

/// A receiver that went away stops the events but not the build.
#[test]
fn a_closed_channel_does_not_stop_the_build() {
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let events = sink();
    events.close();
    let id = sessions
        .start(
            job(
                "ph_closed",
                example("hello_world"),
                ToolchainForBuild::Unavailable {
                    diagnostics: Vec::new(),
                },
            ),
            events.clone(),
        )
        .unwrap();
    assert!(sessions.wait_idle(Duration::from_mins(1)));
    assert!(events.is_empty());
    assert_eq!(
        sessions.record(&id).unwrap().outcome,
        RecordOutcome::ToolchainProblem
    );
}

/// A `tracing` subscriber that keeps every span's and event's level and
/// fields as text.
#[derive(Clone, Default)]
struct Capture {
    records: Arc<std::sync::Mutex<Vec<(tracing::Level, String)>>>,
    next_id: Arc<std::sync::atomic::AtomicU64>,
}

/// Collects fields as `name=value` text.
#[derive(Default)]
struct Fields(String);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write as _;
        let _ = write!(self.0, "{}={value:?} ", field.name());
    }
}

impl tracing::Subscriber for Capture {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut fields = Fields::default();
        span.record(&mut fields);
        self.records
            .lock()
            .unwrap()
            .push((*span.metadata().level(), fields.0));
        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, _: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        // Recorded span fields belong to the `build` span (information level).
        self.records
            .lock()
            .unwrap()
            .push((tracing::Level::INFO, fields.0));
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        self.records
            .lock()
            .unwrap()
            .push((*event.metadata().level(), fields.0));
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// The log never contains project content (block text, names, generated
/// C++, compiler output), and paths only at debug level (08 §8.11).
#[test]
fn logs_hold_no_project_content_and_paths_only_at_debug() {
    let Some(toolchain) = real_toolchain() else {
        return;
    };
    let folder = tempfile::tempdir().unwrap();
    let cache = folder.path().join("pathcanary");
    std::fs::create_dir(&cache).unwrap();
    let mut good = example_json("hello_world");
    good["project"]["name"] = serde_json::json!("NAMECANARY");
    good["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"][0]["inputs"]["ITEM0"]["expr"] =
        serde_json::json!([{"str": "TEXTCANARY"}]);
    // The same with an unused variable as an error: g++ quotes the code.
    let mut bad = good.clone();
    bad["project"]["build"]["configurations"]["debug"]["warningsAsErrors"] = serde_json::json!(true);
    bad["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": "b100",
            "type": "var.declare",
            "v": 1,
            "fields": {"CONST": false, "NAME": {"sym": "s_unused", "name": "varcanary"}, "TYPE": "int"},
            "inputs": {"VALUE": {"expr": [{"num": "7"}]}}
        }));
    // With a single registered subscriber, `tracing` decides whether a call
    // site is enabled from the first thread that reaches it, which may be
    // another test's thread without a subscriber. A second registered
    // dispatcher makes it ask every subscriber instead.
    let _keeper = tracing::Dispatch::new(Capture::default());
    let capture = Capture::default();
    let records = Arc::clone(&capture.records);
    let request = BuildRequest {
        configuration: Configuration::Debug,
        toolchain: ToolchainChoice::Path(toolchain.path().to_path_buf()),
        cache_root: cache.clone(),
        frontend: FrontendOptions::default(),
        ide: true,
    };
    let (built, failed) = tracing::subscriber::with_default(capture, || {
        (
            b2c_build::build(&bytes(&good), &request).unwrap(),
            b2c_build::build(&bytes(&bad), &request).unwrap(),
        )
    });
    assert!(
        matches!(built.outcome, SyncOutcome::Built { .. }),
        "{:#?}",
        built.diagnostics
    );
    assert_eq!(failed.outcome, SyncOutcome::ProjectErrors);
    assert!(
        failed
            .diagnostics
            .iter()
            .any(|d| d.raw.as_deref().is_some_and(|raw| raw.contains("varcanary")))
    );

    let records = records.lock().unwrap();
    assert!(
        records
            .iter()
            .any(|(level, text)| *level == tracing::Level::INFO && text.contains("outcome=\"built\"")),
        "{records:#?}"
    );
    assert!(
        records
            .iter()
            .any(|(level, text)| *level == tracing::Level::INFO && text.contains("outcome=\"projectErrors\"")),
        "{records:#?}"
    );
    assert!(
        records
            .iter()
            .any(|(level, text)| *level == tracing::Level::INFO && text.contains("step=\"compileAndLink\"")),
        "{records:#?}"
    );
    for (level, text) in records.iter() {
        for canary in ["NAMECANARY", "TEXTCANARY", "varcanary"] {
            assert!(!text.contains(canary), "{level}: {text}");
        }
        if *level <= tracing::Level::INFO {
            // INFO, WARN and ERROR are "at most information level".
            assert!(!text.contains("pathcanary"), "{level}: {text}");
        }
    }
    assert!(
        records
            .iter()
            .any(|(level, text)| *level == tracing::Level::DEBUG && text.contains("pathcanary")),
        "{records:#?}"
    );
}
