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

/// The lines of `log` that mention an absolute path: a JSON string that starts
/// like one, or that names one of `folders`. Strings are compared as JSON
/// decodes them (on Windows the file holds every `\` of a path as `\\`).
fn absolute_paths(log: &str, folders: &[&Path]) -> Vec<String> {
    let names: Vec<String> = folders.iter().flat_map(|folder| spellings(folder)).collect();
    log.lines()
        .filter(|line| {
            json_strings(line)
                .iter()
                .any(|text| looks_absolute(text) || names.iter().any(|name| text.contains(name)))
        })
        .map(str::to_owned)
        .collect()
}

/// Whether one of `log`'s lines names `folder`.
fn names_folder(log: &str, folder: &Path) -> bool {
    let names = spellings(folder);
    log.lines().any(|line| {
        json_strings(line)
            .iter()
            .any(|text| names.iter().any(|name| text.contains(name)))
    })
}

/// Every string (keys and values) of the JSON line `line`, decoded; the line
/// itself when it is not JSON.
fn json_strings(line: &str) -> Vec<String> {
    fn collect(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::String(text) => out.push(text.clone()),
            serde_json::Value::Array(items) => items.iter().for_each(|item| collect(item, out)),
            serde_json::Value::Object(map) => {
                for (key, item) in map {
                    out.push(key.clone());
                    collect(item, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    match serde_json::from_str::<serde_json::Value>(line) {
        Ok(value) => collect(&value, &mut out),
        Err(_) => out.push(line.to_owned()),
    }
    out
}

/// Whether `text` starts like an absolute path: `/…`, `C:\…`, `C:/…` or `\\…`.
fn looks_absolute(text: &str) -> bool {
    let bytes = text.as_bytes();
    text.starts_with('/')
        || text.starts_with("\\\\")
        || (bytes.len() > 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}

/// The ways a log can spell `folder`: as given, and canonical in its plain
/// form (the app canonicalises paths; on Windows that also expands short
/// names such as `RUNNER~1`).
fn spellings(folder: &Path) -> Vec<String> {
    let mut names = vec![folder.display().to_string()];
    if let Ok(canonical) = std::fs::canonicalize(folder) {
        let text = canonical.display().to_string();
        let plain = match text.strip_prefix(r"\\?\UNC\") {
            Some(rest) => format!(r"\\{rest}"),
            None => text.strip_prefix(r"\\?\").map_or(text.clone(), str::to_owned),
        };
        if !names.contains(&plain) {
            names.push(plain);
        }
    }
    names
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
        names_folder(debug_part, debug_root.path()),
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
