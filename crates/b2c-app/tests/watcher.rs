//! Outside changes to open project files (`docs/spec/05-project-format.md`
//! §5.10 "External changes", `docs/spec/08-security.md` §8.3 "Re-check on
//! outside change"), with the real file watcher of the operating system
//! (inotify on Linux, `ReadDirectoryChangesW` on Windows) in a temporary
//! folder.
//!
//! Each change outside the app gives exactly one `projectChangedOnDisk`; the
//! app's own saves give none; a deleted or renamed file is reported as
//! deleted; content that did not change (or went back) is not reported;
//! closing, *Save as* and shutdown stop the watching; and reloading a file
//! whose defines changed outside the app returns the project to Restricted
//! Mode.

// Test code: helpers fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use b2c_app::{TrustChoice, WATCH_DEBOUNCE, WATCH_MAX_DELAY};
use b2c_ipc::Handle;
use b2c_ipc::dto::{
    AppEvent, ProjectCloseRequest, ProjectOpened, ProjectReloadRequest, ProjectSaveAsDialogRequest,
    ProjectSaveAsDialogResponse, ProjectSaveRequest, RestrictedReason, TrustGrantRequest, TrustSource,
    TrustState,
};
use b2c_ipc::sink::testing::RecordingSink;
use common::{TestApp, app_sink, edit, example_text};

/// How long to wait for a notification before failing.
const EVENT_TIMEOUT: Duration = Duration::from_secs(20);

/// How long without a notification counts as none: well beyond the debounce
/// and the longest delay of a check.
const QUIET: Duration = Duration::from_millis(1500);

/// The `projectChangedOnDisk` notifications received so far.
fn changes(sink: &RecordingSink<AppEvent>) -> Vec<(Handle, bool)> {
    sink.events()
        .into_iter()
        .filter_map(|event| match event {
            AppEvent::ProjectChangedOnDisk { handle, deleted } => Some((handle, deleted)),
            _ => None,
        })
        .collect()
}

/// Waits until `count` notifications have arrived, then for the quiet
/// period, and returns them all (so a duplicate shows up as an extra one).
fn wait_changes(sink: &RecordingSink<AppEvent>, count: usize) -> Vec<(Handle, bool)> {
    let deadline = Instant::now() + EVENT_TIMEOUT;
    while changes(sink).len() < count {
        assert!(
            Instant::now() < deadline,
            "expected {count} notification(s), got {:?}",
            changes(sink)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(QUIET);
    changes(sink)
}

/// Asserts that no notification arrives within the quiet period.
fn assert_quiet(sink: &RecordingSink<AppEvent>, before: usize) {
    std::thread::sleep(QUIET);
    assert_eq!(changes(sink).len(), before, "{:?}", changes(sink));
}

/// A test app subscribed to app events, with `game.b2c` open.
fn open_game(app: &TestApp) -> (Arc<RecordingSink<AppEvent>>, std::path::PathBuf, ProjectOpened) {
    let sink = app_sink();
    app.backend.app_subscribe(sink.clone());
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    (sink, path, opened)
}

/// The document with another project name (not security-relevant).
fn renamed(document: &str, name: &str) -> String {
    edit(document, |project| {
        project["project"]["name"] = serde_json::json!(name);
    })
}

/// Writes `bytes` to a temporary file next to `path` and renames it over
/// `path`, as many editors save.
fn replace_by_rename(path: &Path, bytes: &[u8]) {
    let temp = path.with_extension("b2c.editor-tmp");
    std::fs::write(&temp, bytes).unwrap();
    std::fs::rename(&temp, path).unwrap();
}

#[test]
fn the_timing_constants_match_the_spec() {
    assert_eq!(WATCH_DEBOUNCE, Duration::from_millis(300));
    assert!(WATCH_MAX_DELAY > WATCH_DEBOUNCE);
    assert!(QUIET > WATCH_MAX_DELAY / 2 + WATCH_DEBOUNCE);
}

#[test]
fn an_outside_change_is_reported_exactly_once() {
    let app = TestApp::new();
    let (sink, path, opened) = open_game(&app);
    std::fs::write(&path, renamed(&opened.document, "Changed outside")).unwrap();
    assert_eq!(wait_changes(&sink, 1), [(opened.handle.clone(), false)]);

    // A second, different change is another notification.
    std::fs::write(&path, renamed(&opened.document, "Changed again")).unwrap();
    assert_eq!(
        wait_changes(&sink, 2),
        [(opened.handle.clone(), false), (opened.handle.clone(), false)]
    );
    // The project is still open and its save is refused: the hash check
    // before saving agrees with the watcher.
    assert_eq!(
        app.backend
            .project_save(ProjectSaveRequest {
                handle: opened.handle.clone(),
                document: opened.document.clone(),
            })
            .unwrap_err(),
        b2c_ipc::IpcError::ChangedOnDisk
    );
}

#[test]
fn an_editor_saving_by_rename_is_reported_once() {
    let app = TestApp::new();
    let (sink, path, opened) = open_game(&app);
    replace_by_rename(&path, renamed(&opened.document, "Saved by an editor").as_bytes());
    assert_eq!(wait_changes(&sink, 1), [(opened.handle, false)]);
}

#[test]
fn unchanged_content_is_not_reported() {
    let app = TestApp::new();
    let (sink, path, opened) = open_game(&app);
    // The same bytes written again, and replaced by rename with the same
    // bytes: the hash is the baseline.
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    replace_by_rename(&path, &bytes);
    // Another file in the folder.
    std::fs::write(path.with_file_name("notes.txt"), b"not a project").unwrap();
    assert_quiet(&sink, 0);

    // A change that is undone before the debounce ends is no change either.
    std::fs::write(&path, renamed(&opened.document, "Briefly")).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    assert_quiet(&sink, 0);

    // But a real change still is.
    std::fs::write(&path, renamed(&opened.document, "Really")).unwrap();
    assert_eq!(wait_changes(&sink, 1), [(opened.handle, false)]);
}

#[test]
fn the_apps_own_saves_are_never_reported() {
    let app = TestApp::new();
    let (sink, path, opened) = open_game(&app);
    let mut document = opened.document.clone();
    for round in 0..3 {
        document = renamed(&document, &format!("Saved in the app {round}"));
        app.backend
            .project_save(ProjectSaveRequest {
                handle: opened.handle.clone(),
                document: document.clone(),
            })
            .unwrap();
    }
    // Save as onto the project's own file is a save too.
    app.dialogs.will_save_as(&path);
    let saved = app
        .backend
        .project_save_as_dialog(ProjectSaveAsDialogRequest {
            handle: opened.handle.clone(),
            document: renamed(&document, "Saved as itself"),
        })
        .unwrap();
    assert!(matches!(saved, ProjectSaveAsDialogResponse::Ok(_)));
    // Reloading reads the file and takes it as the new baseline.
    app.backend
        .project_reload(ProjectReloadRequest {
            handle: opened.handle.clone(),
        })
        .unwrap();
    assert_quiet(&sink, 0);

    // The watcher is still working: an outside change after all that is
    // one notification.
    std::fs::write(&path, renamed(&document, "Outside")).unwrap();
    assert_eq!(wait_changes(&sink, 1), [(opened.handle, false)]);
}

#[test]
fn a_save_that_stalls_after_writing_is_not_reported() {
    let app = TestApp::new();
    let (sink, path, opened) = open_game(&app);
    app.dialogs.will_trust(TrustChoice::TrustProject);
    app.backend
        .trust_grant(TrustGrantRequest {
            handle: opened.handle.clone(),
        })
        .unwrap();
    // Saving a trusted project records its new security hash after the file
    // is written and before the new baseline is in place. Holding the trust
    // store's lock (as another app instance would) stalls the save right
    // there, for longer than the debounce: the watcher sees the written file
    // but must not report it.
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(app.dirs.machine.join("trust.json.lock"))
        .unwrap();
    lock.lock().unwrap();
    let saved = renamed(&opened.document, "Saved slowly");
    let save = {
        let backend = Arc::clone(&app.backend);
        let handle = opened.handle.clone();
        let saved = saved.clone();
        std::thread::spawn(move || {
            backend.project_save(ProjectSaveRequest {
                handle,
                document: saved,
            })
        })
    };
    let written = b2c_model::to_canonical_json(&b2c_model::load(saved.as_bytes()).unwrap());
    let deadline = Instant::now() + EVENT_TIMEOUT;
    while std::fs::read_to_string(&path).unwrap() != written {
        assert!(Instant::now() < deadline, "the save did not write the file");
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(WATCH_MAX_DELAY + WATCH_DEBOUNCE);
    assert!(!save.is_finished(), "the save was expected to wait for the lock");
    lock.unlock().unwrap();
    save.join().unwrap().unwrap();
    assert_quiet(&sink, 0);
}

#[test]
fn a_deleted_file_is_reported_as_deleted() {
    let app = TestApp::new();
    let (sink, path, opened) = open_game(&app);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(wait_changes(&sink, 1), [(opened.handle.clone(), true)]);
    // The save is refused, and so is a reload.
    assert_eq!(
        app.backend
            .project_save(ProjectSaveRequest {
                handle: opened.handle.clone(),
                document: opened.document.clone(),
            })
            .unwrap_err(),
        b2c_ipc::IpcError::ChangedOnDisk
    );
    assert_eq!(
        app.backend
            .project_reload(ProjectReloadRequest {
                handle: opened.handle.clone(),
            })
            .unwrap_err(),
        b2c_ipc::IpcError::NotFound
    );
    // Put back as it was: back at the baseline, nothing to report.
    std::fs::write(&path, example_text("guessing_game")).unwrap();
    assert_quiet(&sink, 1);
    // Deleted again: reported again.
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        wait_changes(&sink, 2),
        [(opened.handle.clone(), true), (opened.handle, true)]
    );
}

#[test]
fn a_file_renamed_away_is_reported_as_deleted() {
    let app = TestApp::new();
    let (sink, path, opened) = open_game(&app);
    std::fs::rename(&path, path.with_file_name("moved.b2c")).unwrap();
    assert_eq!(wait_changes(&sink, 1), [(opened.handle, true)]);
}

#[test]
fn reloading_a_file_whose_defines_changed_outside_is_restricted() {
    let app = TestApp::new();
    let (sink, path, opened) = open_game(&app);
    app.dialogs.will_trust(TrustChoice::TrustProject);
    let granted = app
        .backend
        .trust_grant(TrustGrantRequest {
            handle: opened.handle.clone(),
        })
        .unwrap();
    assert_eq!(granted.trust.source, Some(TrustSource::Project));

    let outside = edit(&opened.document, |project| {
        project["project"]["build"]["defines"] = serde_json::json!([{"name": "LEVEL", "value": {"int": 9}}]);
    });
    std::fs::write(&path, &outside).unwrap();
    assert_eq!(wait_changes(&sink, 1), [(opened.handle.clone(), false)]);

    let reloaded = app
        .backend
        .project_reload(ProjectReloadRequest {
            handle: opened.handle.clone(),
        })
        .unwrap();
    assert_eq!(reloaded.trust.state, TrustState::Restricted);
    assert_eq!(
        reloaded.trust.restricted_reason,
        Some(RestrictedReason::ChangedOutside)
    );
    assert_eq!(
        b2c_model::load(reloaded.document.as_bytes())
            .unwrap()
            .project
            .build
            .defines
            .len(),
        1
    );
    // The reloaded file is the new baseline: nothing more to report.
    assert_quiet(&sink, 1);
}

/// A watched folder that is removed loses its OS watch; a project opened
/// later in a folder made again under the same name is watched again. Linux
/// only: on Windows a folder that a watcher has open cannot reliably be
/// removed and made again at once.
#[cfg(target_os = "linux")]
#[test]
fn a_folder_removed_and_made_again_is_watched_again() {
    let app = TestApp::new();
    let sink = app_sink();
    app.backend.app_subscribe(sink.clone());
    let folder = app.projects().join("games");
    std::fs::create_dir_all(&folder).unwrap();
    let first_path = folder.join("first.b2c");
    std::fs::write(&first_path, example_text("guessing_game")).unwrap();
    let first = app.open(&first_path);
    std::fs::remove_dir_all(&folder).unwrap();
    assert_eq!(wait_changes(&sink, 1), [(first.handle.clone(), true)]);

    std::fs::create_dir_all(&folder).unwrap();
    let second_path = folder.join("second.b2c");
    std::fs::write(&second_path, example_text("hello_world")).unwrap();
    let second = app.open(&second_path);
    std::fs::write(&second_path, renamed(&second.document, "Changed")).unwrap();
    assert_eq!(
        wait_changes(&sink, 2),
        [(first.handle, true), (second.handle, false)]
    );
}

#[test]
fn two_projects_in_one_folder_are_told_apart() {
    let app = TestApp::new();
    let sink = app_sink();
    app.backend.app_subscribe(sink.clone());
    let first_path = app.write("first.b2c", example_text("guessing_game").as_bytes());
    let second_path = app.write("second.b2c", example_text("hello_world").as_bytes());
    let first = app.open(&first_path);
    let second = app.open(&second_path);

    std::fs::write(&second_path, renamed(&second.document, "Second, changed")).unwrap();
    assert_eq!(wait_changes(&sink, 1), [(second.handle.clone(), false)]);

    // Closing the first project leaves the folder watched for the second.
    app.backend
        .project_close(ProjectCloseRequest {
            handle: first.handle.clone(),
        })
        .unwrap();
    std::fs::write(&first_path, renamed(&first.document, "First, changed")).unwrap();
    std::fs::write(&second_path, renamed(&second.document, "Second, changed again")).unwrap();
    assert_eq!(
        wait_changes(&sink, 2),
        [(second.handle.clone(), false), (second.handle, false)]
    );
}

#[test]
fn closing_save_as_and_shutdown_stop_the_watching() {
    let app = TestApp::new();
    let sink = app_sink();
    app.backend.app_subscribe(sink.clone());

    // Closed: not watched.
    let closed_path = app.write("closed.b2c", example_text("hello_world").as_bytes());
    let closed = app.open(&closed_path);
    app.backend
        .project_close(ProjectCloseRequest {
            handle: closed.handle,
        })
        .unwrap();
    std::fs::write(&closed_path, renamed(&closed.document, "Closed")).unwrap();

    // Saved as another file in another folder: only the new file is watched.
    let old_path = app.write("old.b2c", example_text("hello_world").as_bytes());
    let moved = app.open(&old_path);
    let elsewhere = app.root().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let new_path = elsewhere.join("new.b2c");
    app.dialogs.will_save_as(&new_path);
    app.backend
        .project_save_as_dialog(ProjectSaveAsDialogRequest {
            handle: moved.handle.clone(),
            document: moved.document.clone(),
        })
        .unwrap();
    std::fs::write(&old_path, renamed(&moved.document, "Old file")).unwrap();
    assert_quiet(&sink, 0);
    std::fs::write(&new_path, renamed(&moved.document, "New file")).unwrap();
    assert_eq!(wait_changes(&sink, 1), [(moved.handle.clone(), false)]);

    // Shut down: nothing is watched any more.
    app.backend.shutdown();
    std::fs::write(&new_path, renamed(&moved.document, "After shutdown")).unwrap();
    assert_quiet(&sink, 1);
}

/// A save, reload or *Save as* that finishes after a close of the same
/// project (the backend runs commands concurrently) does not watch the
/// closed project again: outside changes to its file are never reported for
/// it. The close comes at different points of the save's write.
#[test]
fn a_save_or_reload_racing_a_close_does_not_watch_the_closed_project() {
    use std::sync::Barrier;

    let app = TestApp::new();
    let sink = app_sink();
    app.backend.app_subscribe(sink.clone());
    let base = example_text("guessing_game");
    let mut written = Vec::new();
    for attempt in 0..45_u64 {
        let path = app.write(&format!("race{attempt}.b2c"), base.as_bytes());
        let target = app.projects().join(format!("race{attempt}-as.b2c"));
        let opened = app.open(&path);
        if attempt % 3 == 2 {
            app.dialogs.will_save_as(&target);
        }
        let backend = Arc::clone(&app.backend);
        let handle = opened.handle.clone();
        let document = renamed(&opened.document, &format!("Saved {attempt}"));
        let barrier = Arc::new(Barrier::new(2));
        let worker = {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                match attempt % 3 {
                    0 => backend
                        .project_save(ProjectSaveRequest { handle, document })
                        .map(drop),
                    1 => backend.project_reload(ProjectReloadRequest { handle }).map(drop),
                    _ => backend
                        .project_save_as_dialog(ProjectSaveAsDialogRequest { handle, document })
                        .map(drop),
                }
            })
        };
        barrier.wait();
        std::thread::sleep(Duration::from_micros((attempt / 3 % 15) * 150));
        app.backend
            .project_close(ProjectCloseRequest {
                handle: opened.handle,
            })
            .unwrap();
        // The command may finish or find the project closed; both are fine.
        let _ = worker.join().unwrap();
        written.extend([path, target]);
    }
    std::thread::sleep(QUIET);
    let before = changes(&sink).len();
    for path in &written {
        std::fs::write(path, renamed(&base, "Changed after the close")).unwrap();
    }
    assert_quiet(&sink, before);
}
