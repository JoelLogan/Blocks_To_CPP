//! The app's log files with the real backend (`docs/spec/08-security.md`
//! §8.11): building and running a project whose strings hold a canary leaves
//! neither the canary nor any absolute path in the logs at the default
//! level; at debug level paths appear (still no project content); a panic is
//! logged with its location only.
//!
//! The logger is process-global, so this binary installs it once and runs its
//! tests one at a time.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use b2c_ipc::dto::{
    BuildConfig, BuildEvent, BuildStartRequest, ProjectNewRequest, RunEvent, RunOptions, RunStartRequest,
    Template,
};
use b2c_ipc::sink::testing::{RecordingBytes, RecordingSink};
use b2c_store::RotatingFile;
use b2c_store::log::{LOG_BASE, LOG_FILES, LOG_MAX_BYTES};
use blocks2cpp_desktop::logging::{self, LevelHandle};
use common::{NoDialogs, gxx, wait_until, with_printed_text};
use tracing::Level;

const CANARY_NAME: &str = "CanaryProject4f1e";
const CANARY_TEXT: &str = "canary-printed-text-9c2d";
const CANARY_PANIC: &str = "canary-panic-message-7a0b";

/// The global log: its folder, its files and its level.
struct Log {
    dir: tempfile::TempDir,
    paths: Vec<PathBuf>,
    level: LevelHandle,
}

impl Log {
    fn text(&self) -> String {
        self.paths
            .iter()
            .filter(|path| path.exists())
            .map(|path| std::fs::read_to_string(path).unwrap())
            .collect()
    }
}

/// Installs the logger once for this test binary, and serialises the tests.
fn log() -> (MutexGuard<'static, ()>, &'static Log) {
    static LOG: OnceLock<Log> = OnceLock::new();
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let guard = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    let log = LOG.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let file = RotatingFile::open(dir.path(), LOG_BASE, LOG_MAX_BYTES, LOG_FILES).unwrap();
        let paths = file.paths();
        let level = LevelHandle::new(Level::INFO);
        tracing::subscriber::set_global_default(logging::subscriber(file, level.clone())).unwrap();
        logging::install_panic_hook();
        Log { dir, paths, level }
    });
    (guard, log)
}

/// Builds and runs Hello World with canary strings in a backend under
/// `root`; returns whether it ran (`false` without g++).
fn build_and_run_a_canary_project(root: &Path) -> bool {
    let Some(compiler) = gxx() else {
        return false;
    };
    let backend = common::backend(
        root,
        vec![compiler.parent().unwrap().to_path_buf()],
        Arc::new(NoDialogs),
    );
    let project = backend
        .project_new(ProjectNewRequest {
            template: Template::HelloWorld,
        })
        .unwrap();
    let mut value: serde_json::Value =
        serde_json::from_str(&with_printed_text(&project.document, CANARY_TEXT)).unwrap();
    value["project"]["name"] = serde_json::Value::from(CANARY_NAME);
    let document = serde_json::to_string(&value).unwrap();

    let build_events = Arc::new(RecordingSink::<BuildEvent>::new());
    let build_id = backend
        .build_start(
            BuildStartRequest {
                handle: project.handle.clone(),
                document,
                config: BuildConfig::Debug,
            },
            build_events.clone(),
        )
        .unwrap();
    let finished = wait_until(Duration::from_mins(3), || {
        build_events
            .events()
            .into_iter()
            .find(|event| matches!(event, BuildEvent::Finished { .. }))
    })
    .expect("the build finished");
    let BuildEvent::Finished { outcome, .. } = finished else {
        unreachable!()
    };
    assert_eq!(outcome, b2c_ipc::dto::BuildOutcome::Built);

    let output = Arc::new(RecordingBytes::new());
    let run_events = Arc::new(RecordingSink::<RunEvent>::new());
    backend
        .run_start(
            RunStartRequest {
                build_id: build_id.build_id,
                run_options: RunOptions { cols: 80, rows: 24 },
            },
            output.clone(),
            run_events.clone(),
        )
        .unwrap();
    wait_until(Duration::from_mins(1), || {
        run_events
            .events()
            .into_iter()
            .find(|event| matches!(event, RunEvent::Exit { .. }))
    })
    .expect("the program ended");
    let printed = String::from_utf8_lossy(&output.concat()).into_owned();
    assert!(printed.contains(CANARY_TEXT), "the program printed {printed:?}");
    backend.shutdown();
    true
}

/// Whether `log` mentions an absolute path: a JSON string that starts like
/// one, or the given folders.
fn absolute_paths(log: &str, folders: &[&Path]) -> Vec<String> {
    let mut found: Vec<String> = log
        .lines()
        .filter(|line| line.contains("\":\"/") || line.contains(":\\\\") || line.contains("\":\"\\\\"))
        .map(str::to_owned)
        .collect();
    for folder in folders {
        let text = folder.display().to_string();
        found.extend(log.lines().filter(|line| line.contains(&text)).map(str::to_owned));
    }
    found
}

#[test]
fn info_logs_hold_no_content_and_no_paths_and_debug_logs_add_paths() {
    let (_guard, log) = log();
    log.level.set(Level::INFO);
    let root = tempfile::tempdir().unwrap();
    if !build_and_run_a_canary_project(root.path()) {
        return;
    }
    let compiler = gxx().unwrap();
    let text = log.text();
    assert!(text.contains("backend started"), "{text}");
    for canary in [CANARY_NAME, CANARY_TEXT] {
        assert!(!text.contains(canary), "the log holds project content ({canary})");
    }
    let paths = absolute_paths(&text, &[root.path(), compiler.parent().unwrap(), log.dir.path()]);
    assert!(paths.is_empty(), "paths at info level: {paths:#?}");

    // At debug level the same work logs paths (still no content).
    log.level.set(Level::DEBUG);
    let before = text.len();
    let debug_root = tempfile::tempdir().unwrap();
    assert!(build_and_run_a_canary_project(debug_root.path()));
    log.level.set(Level::INFO);
    let text = log.text();
    let debug_part = &text[before.min(text.len())..];
    for canary in [CANARY_NAME, CANARY_TEXT] {
        assert!(
            !text.contains(canary),
            "the debug log holds project content ({canary})"
        );
    }
    assert!(
        debug_part.contains(&debug_root.path().display().to_string()),
        "debug lines name the build folder"
    );
    for line in debug_part.lines() {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(
            value["timestamp"].is_string() && value["level"].is_string(),
            "{line}"
        );
    }
}

#[test]
fn panics_are_logged_with_their_location_only() {
    let (_guard, log) = log();
    log.level.set(Level::INFO);
    let caught = std::panic::catch_unwind(|| panic!("{CANARY_PANIC}"));
    assert!(caught.is_err());
    let text = log.text();
    let line = text
        .lines()
        .find(|line| line.contains("a thread panicked"))
        .expect("the panic was logged");
    let value: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(value["level"], "ERROR");
    assert!(
        value["fields"]["file"].as_str().unwrap().ends_with("logging.rs"),
        "{line}"
    );
    assert!(value["fields"]["line"].as_u64().unwrap() > 0);
    assert!(!text.contains(CANARY_PANIC), "the panic's message was logged");
}
