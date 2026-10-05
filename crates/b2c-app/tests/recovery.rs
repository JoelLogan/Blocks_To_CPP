//! Autosave and crash recovery through the backend
//! (`docs/spec/05-project-format.md` §5.10, `docs/spec/08-security.md`
//! §8.3.1 "Restored snapshots"): snapshots survive a crash (the backend
//! dropped without shutting down) and are offered to the next start, never
//! to the instance that wrote them; a clean save, close or shutdown deletes
//! them; a restore gives the trust the rules say, binds the project to its
//! file again, writes its own snapshot and discards the old one; and every
//! error is typed.

// Test code: helpers fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use b2c_app::{Backend, BackendConfig, Services, TrustChoice};
use b2c_build::toolchains::DiscoveryScope;
use b2c_ipc::dto::{
    AppEvent, ProjectCloseRequest, ProjectNewRequest, ProjectSaveAsDialogRequest,
    ProjectSaveAsDialogResponse, ProjectSaveRequest, ProjectSetDirtyRequest, RecoveryDiscardRequest,
    RecoveryRestoreRequest, RecoveryRestoreResponse, RecoverySaveRequest, RestrictedReason, SnapshotInfo,
    Template, Trust, TrustGetRequest, TrustGrantRequest, TrustRevokeRequest, TrustSource, TrustState,
};
use b2c_ipc::{Handle, IpcError, SnapshotId, limits::MAX_OPEN_HANDLES};
use common::{FakeDialogs, FakeOpener, TestApp, app_sink, edit, example_text};

/// Drops `backend` once nothing else holds it (background threads hold it
/// only briefly), without shutting it down: what a crash leaves behind.
fn crash(backend: Arc<Backend>) {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut backend = backend;
    loop {
        match Arc::try_unwrap(backend) {
            Ok(backend) => {
                drop(backend);
                return;
            }
            Err(shared) => {
                assert!(Instant::now() < deadline, "the backend is still in use");
                backend = shared;
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

/// Simulates a crash of `app`'s backend and starts a new one on the same
/// folders, with new dialogs.
fn crash_and_restart(app: &mut TestApp) {
    let dialogs = FakeDialogs::new();
    let opener = Arc::new(FakeOpener::default());
    let root = app.root();
    let backend = Backend::start_with(
        BackendConfig {
            dirs: app.dirs.clone(),
            app_version: "0.1.0",
            discovery_scope: DiscoveryScope::Only(vec![root.join("bin")]),
            cwd: Some(root.join("cwd")),
        },
        dialogs.clone(),
        Services {
            prober: app.prober.clone(),
            opener: opener.clone(),
        },
    )
    .unwrap();
    let old = std::mem::replace(&mut app.backend, backend);
    app.dialogs = dialogs;
    app.opener = opener;
    crash(old);
}

/// The snapshot metadata files in the recovery folder, of every instance.
fn snapshot_files(app: &TestApp) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(instances) = std::fs::read_dir(&app.dirs.recovery) else {
        return found;
    };
    for instance in instances {
        let instance = instance.unwrap().path();
        if !instance.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(&instance).unwrap() {
            let file = file.unwrap().path();
            if file.extension().is_some_and(|ext| ext == "json") {
                found.push(file);
            }
        }
    }
    found.sort();
    found
}

fn snapshot(app: &TestApp, handle: &Handle, document: &str) -> Result<(), IpcError> {
    app.backend
        .recovery_save(RecoverySaveRequest {
            handle: handle.clone(),
            document: document.to_owned(),
        })
        .map(|_| ())
}

fn list(app: &TestApp) -> Vec<SnapshotInfo> {
    app.backend.recovery_list().unwrap().snapshots
}

fn restore(app: &TestApp, id: &SnapshotId) -> Result<RecoveryRestoreResponse, IpcError> {
    app.backend.recovery_restore(RecoveryRestoreRequest {
        snapshot_id: id.clone(),
    })
}

/// Restores the only snapshot offered.
fn restore_only(app: &TestApp) -> RecoveryRestoreResponse {
    let listed = list(app);
    assert_eq!(listed.len(), 1, "{listed:?}");
    restore(app, &listed[0].snapshot_id).unwrap()
}

fn grant(app: &TestApp, handle: &Handle, choice: TrustChoice) -> Trust {
    app.dialogs.will_trust(choice);
    app.backend
        .trust_grant(TrustGrantRequest {
            handle: handle.clone(),
        })
        .unwrap()
        .trust
}

fn trust_of(app: &TestApp, handle: &Handle) -> Trust {
    app.backend
        .trust_get(TrustGetRequest {
            handle: handle.clone(),
        })
        .unwrap()
        .trust
}

/// The document with another project name (not security-relevant).
fn renamed(document: &str, name: &str) -> String {
    edit(document, |project| {
        project["project"]["name"] = serde_json::json!(name);
    })
}

/// The document with a define (security-relevant).
fn with_define(document: &str, value: i64) -> String {
    edit(document, |project| {
        project["project"]["build"]["defines"] =
            serde_json::json!([{"name": "LEVEL", "value": {"int": value}}]);
    })
}

/// The canonical form of a document text.
fn canonical(document: &str) -> String {
    b2c_model::to_canonical_json(&b2c_model::load(document.as_bytes()).unwrap())
}

fn assert_trusted(trust: Trust, source: TrustSource) {
    assert_eq!(trust.state, TrustState::Trusted, "{trust:?}");
    assert_eq!(trust.source, Some(source), "{trust:?}");
}

fn assert_restricted(trust: Trust, reason: RestrictedReason) {
    assert_eq!(trust.state, TrustState::Restricted, "{trust:?}");
    assert_eq!(trust.restricted_reason, Some(reason), "{trust:?}");
}

#[test]
fn a_snapshot_survives_a_crash_and_a_clean_save_deletes_it() {
    let mut app = TestApp::new();
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    let edited = renamed(&opened.document, "Unsaved work");
    snapshot(&app, &opened.handle, &edited).unwrap();
    // Written again: still one snapshot for the project.
    snapshot(&app, &opened.handle, &edited).unwrap();
    assert_eq!(snapshot_files(&app).len(), 1);
    // Never offered to the instance that wrote it.
    assert!(list(&app).is_empty());

    crash_and_restart(&mut app);
    let listed = list(&app);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].project_name, "Unsaved work");
    assert!(listed[0].has_path);
    assert!(b2c_store::parse_rfc3339_utc(&listed[0].saved_at).is_some());

    let restored = restore(&app, &listed[0].snapshot_id).unwrap();
    assert_eq!(restored.document, canonical(&edited));
    assert_eq!(restored.file_name.as_deref(), Some("game.b2c"));
    // Never trusted at this path: restricted.
    assert_restricted(restored.trust, RestrictedReason::NoRecord);
    assert_eq!(trust_of(&app, &restored.handle), restored.trust);
    // A restored project has unsaved changes.
    assert!(app.backend.has_dirty());
    // The old snapshot was discarded; the restored project has its own.
    assert!(list(&app).is_empty());
    assert_eq!(snapshot_files(&app).len(), 1);
    assert_eq!(
        restore(&app, &listed[0].snapshot_id).unwrap_err(),
        IpcError::UnknownSnapshot
    );

    // A clean save writes the project's file and deletes its snapshot.
    let saved = app
        .backend
        .project_save(ProjectSaveRequest {
            handle: restored.handle.clone(),
            document: restored.document.clone(),
        })
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), restored.document);
    assert_eq!(saved.hash, b2c_store::sha256_hex(restored.document.as_bytes()));
    assert!(snapshot_files(&app).is_empty());

    // Nothing is left for the next start either.
    crash_and_restart(&mut app);
    assert!(list(&app).is_empty());
}

#[test]
fn closing_and_shutting_down_delete_snapshots_of_clean_projects() {
    let mut app = TestApp::new();
    let closed = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::Empty,
        })
        .unwrap();
    let clean = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::HelloWorld,
        })
        .unwrap();
    let dirty = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::Empty,
        })
        .unwrap();
    for (handle, name) in [
        (&closed.handle, "Closed"),
        (&clean.handle, "Clean"),
        (&dirty.handle, "Dirty"),
    ] {
        let document = if handle == &clean.handle {
            &clean.document
        } else {
            &closed.document
        };
        snapshot(&app, handle, &renamed(document, name)).unwrap();
    }
    assert_eq!(snapshot_files(&app).len(), 3);
    app.backend
        .project_close(ProjectCloseRequest {
            handle: closed.handle.clone(),
        })
        .unwrap();
    assert_eq!(snapshot_files(&app).len(), 2);
    app.backend
        .project_set_dirty(ProjectSetDirtyRequest {
            handle: dirty.handle.clone(),
            dirty: true,
        })
        .unwrap();
    // Shutdown keeps only the snapshot of the project with unsaved changes.
    app.backend.shutdown();
    assert_eq!(snapshot_files(&app).len(), 1);
    // No snapshots after shutdown: the app is exiting.
    assert_eq!(
        snapshot(&app, &dirty.handle, &dirty.document).unwrap_err(),
        IpcError::Internal
    );

    crash_and_restart(&mut app);
    let listed = list(&app);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].project_name, "Dirty");
    assert!(!listed[0].has_path);
}

#[test]
fn a_new_project_that_was_never_saved_restores_as_created_here() {
    let mut app = TestApp::new();
    let created = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::HelloWorld,
        })
        .unwrap();
    let edited = with_define(&created.document, 1);
    snapshot(&app, &created.handle, &edited).unwrap();

    crash_and_restart(&mut app);
    let restored = restore_only(&app);
    assert_trusted(restored.trust, TrustSource::CreatedHere);
    assert_eq!(restored.file_name, None);
    assert_eq!(restored.document, canonical(&edited));
    // It can be saved only through Save as, which records trust for the file.
    assert_eq!(
        app.backend
            .project_save(ProjectSaveRequest {
                handle: restored.handle.clone(),
                document: restored.document.clone(),
            })
            .unwrap_err(),
        IpcError::NoPath
    );
    let target = app.projects().join("restored.b2c");
    app.dialogs.will_save_as(&target);
    let saved = app
        .backend
        .project_save_as_dialog(ProjectSaveAsDialogRequest {
            handle: restored.handle.clone(),
            document: restored.document.clone(),
        })
        .unwrap();
    assert!(matches!(saved, ProjectSaveAsDialogResponse::Ok(_)));
    assert_trusted(trust_of(&app, &restored.handle), TrustSource::Project);
    assert!(snapshot_files(&app).is_empty());
}

#[test]
fn a_new_project_whose_trust_was_revoked_restores_restricted() {
    let mut app = TestApp::new();
    let created = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::HelloWorld,
        })
        .unwrap();
    app.backend
        .trust_revoke(TrustRevokeRequest {
            handle: created.handle.clone(),
        })
        .unwrap();
    snapshot(&app, &created.handle, &created.document).unwrap();

    crash_and_restart(&mut app);
    let listed = list(&app);
    assert!(
        listed[0].has_path,
        "only a project trusted as created here may say it had no file"
    );
    let restored = restore_only(&app);
    assert_restricted(restored.trust, RestrictedReason::NoRecord);
    assert_eq!(restored.file_name, None);
}

#[test]
fn a_project_edited_while_trusted_restores_trusted() {
    let mut app = TestApp::new();
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    assert_trusted(
        grant(&app, &opened.handle, TrustChoice::TrustProject),
        TrustSource::Project,
    );
    // A define added in the app, not saved: the trust record still has the
    // file's hash, but the snapshot was written while trusted.
    let edited = with_define(&opened.document, 5);
    snapshot(&app, &opened.handle, &edited).unwrap();

    crash_and_restart(&mut app);
    let restored = restore_only(&app);
    assert_trusted(restored.trust, TrustSource::Project);
    // Saving it in the app records the new hash, as for any trusted save.
    app.backend
        .project_save(ProjectSaveRequest {
            handle: restored.handle.clone(),
            document: restored.document.clone(),
        })
        .unwrap();
    let saved = b2c_model::load(restored.document.as_bytes()).unwrap();
    assert_eq!(
        app.trust_json().unwrap()["projects"][0]["rawCodeHashAtGrant"],
        b2c_model::hex(&b2c_model::security_hash(&saved))
    );
    assert_trusted(app.open(&path).trust, TrustSource::Project);
}

#[test]
fn a_restricted_snapshot_is_trusted_only_by_a_matching_record() {
    let mut app = TestApp::new();
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    assert_restricted(opened.trust, RestrictedReason::NoRecord);
    // Written while restricted: one snapshot that keeps the file's security
    // content, one that changes it.
    let same = renamed(&opened.document, "Same security content");
    snapshot(&app, &opened.handle, &same).unwrap();
    let other = app.open(&path);
    let changed = with_define(&other.document, 7);
    snapshot(&app, &other.handle, &changed).unwrap();

    crash_and_restart(&mut app);
    // Meanwhile the user trusted the file as it is on disk.
    let reopened = app.open(&path);
    assert_trusted(
        grant(&app, &reopened.handle, TrustChoice::TrustProject),
        TrustSource::Project,
    );

    let listed = list(&app);
    assert_eq!(listed.len(), 2);
    for info in &listed {
        let restored = restore(&app, &info.snapshot_id).unwrap();
        match info.project_name.as_str() {
            // The record's hash equals the snapshot's security hash.
            "Same security content" => assert_trusted(restored.trust, TrustSource::Project),
            // The record is for other content and the snapshot was written
            // while restricted: the change is not trusted.
            _ => assert_restricted(restored.trust, RestrictedReason::ChangedOutside),
        }
    }
}

#[test]
fn a_revoked_record_restores_restricted_and_a_trusted_folder_trusted() {
    let mut app = TestApp::new();
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    grant(&app, &opened.handle, TrustChoice::TrustProject);
    snapshot(&app, &opened.handle, &with_define(&opened.document, 2)).unwrap();
    crash_and_restart(&mut app);
    // The record goes before the restore: trusted at write is not enough.
    let reopened = app.open(&path);
    app.backend
        .trust_revoke(TrustRevokeRequest {
            handle: reopened.handle.clone(),
        })
        .unwrap();
    let restored = restore_only(&app);
    assert_restricted(restored.trust, RestrictedReason::NoRecord);
    // Closed without saving: its snapshot goes too.
    app.backend
        .project_close(ProjectCloseRequest {
            handle: restored.handle,
        })
        .unwrap();

    // A snapshot written while restricted, in a folder trusted since.
    let folder = app.projects().join("trusted");
    std::fs::create_dir_all(&folder).unwrap();
    let in_folder = folder.join("game.b2c");
    std::fs::write(&in_folder, example_text("hello_world")).unwrap();
    let opened = app.open(&in_folder);
    snapshot(&app, &opened.handle, &with_define(&opened.document, 3)).unwrap();
    crash_and_restart(&mut app);
    let other = app.open(&in_folder);
    assert_trusted(
        grant(&app, &other.handle, TrustChoice::TrustFolder),
        TrustSource::Folder,
    );
    let restored = restore_only(&app);
    assert_trusted(restored.trust, TrustSource::Folder);
}

#[test]
fn a_snapshot_that_disagrees_with_its_metadata_restores_restricted() {
    let mut app = TestApp::new();
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    grant(&app, &opened.handle, TrustChoice::TrustProject);
    snapshot(&app, &opened.handle, &opened.document).unwrap();
    crash_and_restart(&mut app);
    // The metadata now claims another security hash (the document and its
    // documentHash still match).
    let files = snapshot_files(&app);
    assert_eq!(files.len(), 1);
    let mut meta: serde_json::Value = serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap();
    assert_eq!(meta["trustedAtWrite"], true);
    meta["securityHash"] = serde_json::json!("0".repeat(64));
    std::fs::write(&files[0], serde_json::to_vec(&meta).unwrap()).unwrap();
    let restored = restore_only(&app);
    assert_restricted(restored.trust, RestrictedReason::NoRecord);
}

#[test]
fn a_restored_project_is_bound_to_its_file_and_watched() {
    let mut app = TestApp::new();
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    snapshot(&app, &opened.handle, &renamed(&opened.document, "Restored")).unwrap();
    crash_and_restart(&mut app);
    // The file changed after the snapshot (before the restore): that is the
    // baseline now, so saving the restored project is allowed.
    let on_disk = renamed(&opened.document, "Changed before the restore");
    std::fs::write(&path, &on_disk).unwrap();
    let sink = app_sink();
    app.backend.app_subscribe(sink.clone());
    let restored = restore_only(&app);

    // A change after the restore is reported for the restored project.
    std::fs::write(&path, renamed(&opened.document, "Changed after the restore")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let changes: Vec<AppEvent> = sink
            .events()
            .into_iter()
            .filter(|event| matches!(event, AppEvent::ProjectChangedOnDisk { .. }))
            .collect();
        if !changes.is_empty() {
            assert_eq!(
                changes,
                [AppEvent::ProjectChangedOnDisk {
                    handle: restored.handle.clone(),
                    deleted: false,
                }]
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no notification for the restored project"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        app.backend
            .project_save(ProjectSaveRequest {
                handle: restored.handle.clone(),
                document: restored.document.clone(),
            })
            .unwrap_err(),
        IpcError::ChangedOnDisk
    );
}

#[test]
fn a_restored_project_whose_file_is_gone_is_never_saved_over_it() {
    let mut app = TestApp::new();
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    snapshot(&app, &opened.handle, &renamed(&opened.document, "Orphan")).unwrap();
    crash_and_restart(&mut app);
    std::fs::remove_file(&path).unwrap();
    let restored = restore_only(&app);
    assert_eq!(restored.file_name.as_deref(), Some("game.b2c"));
    // Its file is gone: like a deleted file, it is kept with Save as.
    let document = restored.document.clone();
    let save = |app: &TestApp| {
        app.backend.project_save(ProjectSaveRequest {
            handle: restored.handle.clone(),
            document: document.clone(),
        })
    };
    assert_eq!(save(&app).unwrap_err(), IpcError::ChangedOnDisk);
    // Even when a file appears there later.
    std::fs::write(&path, b"someone else's file").unwrap();
    assert_eq!(save(&app).unwrap_err(), IpcError::ChangedOnDisk);
    assert_eq!(std::fs::read(&path).unwrap(), b"someone else's file");
    let kept = app.projects().join("kept.b2c");
    app.dialogs.will_save_as(&kept);
    let saved = app
        .backend
        .project_save_as_dialog(ProjectSaveAsDialogRequest {
            handle: restored.handle.clone(),
            document: restored.document.clone(),
        })
        .unwrap();
    assert!(matches!(saved, ProjectSaveAsDialogResponse::Ok(_)));
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), restored.document);
}

#[test]
fn the_snapshot_is_the_document_the_trust_dialog_lists() {
    let app = TestApp::new();
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    let edited = with_define(&opened.document, 11);
    snapshot(&app, &opened.handle, &edited).unwrap();
    grant(&app, &opened.handle, TrustChoice::TrustProject);
    let listed = b2c_model::load(edited.as_bytes()).unwrap();
    assert_eq!(
        app.trust_json().unwrap()["projects"][0]["rawCodeHashAtGrant"],
        b2c_model::hex(&b2c_model::security_hash(&listed))
    );
}

#[test]
fn discarding_removes_a_snapshot_for_good() {
    let mut app = TestApp::new();
    let created = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::Empty,
        })
        .unwrap();
    snapshot(&app, &created.handle, &created.document).unwrap();
    crash_and_restart(&mut app);
    let listed = list(&app);
    assert_eq!(listed.len(), 1);
    app.backend
        .recovery_discard(RecoveryDiscardRequest {
            snapshot_id: listed[0].snapshot_id.clone(),
        })
        .unwrap();
    assert!(list(&app).is_empty());
    assert!(snapshot_files(&app).is_empty());
    assert_eq!(
        app.backend
            .recovery_discard(RecoveryDiscardRequest {
                snapshot_id: listed[0].snapshot_id.clone(),
            })
            .unwrap_err(),
        IpcError::UnknownSnapshot
    );
    assert_eq!(
        restore(&app, &listed[0].snapshot_id).unwrap_err(),
        IpcError::UnknownSnapshot
    );
}

#[test]
fn requests_are_validated_and_errors_are_typed() {
    let app = TestApp::new();
    let created = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::Empty,
        })
        .unwrap();
    // The document is checked first: size, then the strict loader.
    let too_large = " ".repeat(b2c_ipc::limits::MAX_DOCUMENT_BYTES + 1);
    assert!(matches!(
        snapshot(&app, &created.handle, &too_large).unwrap_err(),
        IpcError::PayloadTooLarge { .. }
    ));
    assert!(matches!(
        snapshot(&app, &created.handle, "{\"not\": \"a project\"}").unwrap_err(),
        IpcError::InvalidDocument { .. }
    ));
    let newer = edit(&created.document, |project| {
        project["formatVersion"] = serde_json::json!(99);
    });
    assert!(matches!(
        snapshot(&app, &created.handle, &newer).unwrap_err(),
        IpcError::NewerFormat { .. }
    ));
    assert_eq!(
        snapshot(&app, &Handle::example(), &created.document).unwrap_err(),
        IpcError::UnknownHandle
    );
    assert!(snapshot_files(&app).is_empty());
    // Unknown snapshots.
    let unknown = SnapshotId::random().unwrap();
    assert_eq!(restore(&app, &unknown).unwrap_err(), IpcError::UnknownSnapshot);
    assert_eq!(
        app.backend
            .recovery_discard(RecoveryDiscardRequest { snapshot_id: unknown })
            .unwrap_err(),
        IpcError::UnknownSnapshot
    );
    // This instance's own snapshot is not restorable by it.
    snapshot(&app, &created.handle, &created.document).unwrap();
    let own = snapshot_files(&app)[0]
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let own = SnapshotId::parse(&own).unwrap();
    assert_eq!(restore(&app, &own).unwrap_err(), IpcError::UnknownSnapshot);
}

#[test]
fn a_restore_counts_against_the_open_project_limit() {
    let mut app = TestApp::new();
    let created = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::Empty,
        })
        .unwrap();
    snapshot(&app, &created.handle, &created.document).unwrap();
    crash_and_restart(&mut app);
    let listed = list(&app);
    for _ in 0..MAX_OPEN_HANDLES {
        app.backend
            .project_new(ProjectNewRequest {
                template: Template::Empty,
            })
            .unwrap();
    }
    assert_eq!(
        restore(&app, &listed[0].snapshot_id).unwrap_err(),
        IpcError::TooManyHandles
    );
    // Still offered, and restorable once there is room.
    assert_eq!(list(&app), listed);
}

#[test]
fn snapshots_of_damaged_documents_can_be_discarded() {
    let mut app = TestApp::new();
    let created = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::Empty,
        })
        .unwrap();
    snapshot(&app, &created.handle, &created.document).unwrap();
    crash_and_restart(&mut app);
    // Replace the document by one from a newer version, with metadata that
    // matches it (as a newer app would leave it).
    let meta_path = snapshot_files(&app).remove(0);
    let document_path = meta_path.with_extension("b2c");
    let newer = edit(&created.document, |project| {
        project["formatVersion"] = serde_json::json!(99);
    });
    std::fs::write(&document_path, &newer).unwrap();
    let mut meta: serde_json::Value = serde_json::from_slice(&std::fs::read(&meta_path).unwrap()).unwrap();
    meta["documentHash"] = serde_json::json!(b2c_store::sha256_hex(newer.as_bytes()));
    std::fs::write(&meta_path, serde_json::to_vec(&meta).unwrap()).unwrap();

    let listed = list(&app);
    assert!(matches!(
        restore(&app, &listed[0].snapshot_id).unwrap_err(),
        IpcError::NewerFormat { .. }
    ));
    // A failed restore leaves it offered, so it can be discarded.
    assert_eq!(list(&app), listed);
    app.backend
        .recovery_discard(RecoveryDiscardRequest {
            snapshot_id: listed[0].snapshot_id.clone(),
        })
        .unwrap();
    assert!(list(&app).is_empty());
}

#[test]
fn snapshots_are_private_files_in_the_recovery_folder() {
    let app = TestApp::new();
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    snapshot(&app, &opened.handle, &opened.document).unwrap();
    let files = snapshot_files(&app);
    assert_eq!(files.len(), 1);
    assert!(files[0].starts_with(&app.dirs.recovery));
    // Nothing was written next to the project.
    let beside: Vec<PathBuf> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(beside, std::slice::from_ref(&path));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&files[0]), 0o600);
        assert_eq!(mode(&files[0].with_extension("b2c")), 0o600);
        assert_eq!(mode(files[0].parent().unwrap()), 0o700);
    }
    let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap();
    assert_eq!(meta["boundPath"], serde_json::json!(path.to_str().unwrap()));
    assert_eq!(meta["trustedAtWrite"], false);
}

/// [project-close]: closing a project whose program is running kills the
/// program's process tree and deletes the project's snapshot.
#[cfg(target_os = "linux")]
#[test]
fn closing_a_running_project_kills_its_program_and_deletes_its_snapshot() {
    use b2c_ipc::dto::{BuildEvent, BuildOutcome, RunEvent, RunOptions, RunStartRequest};
    use common::{Compiler, TestProber, gxx, run_sinks, wait_exit, wait_finished};

    if gxx().is_none() {
        return;
    }
    let app = TestApp::with(Compiler::spy(), TestProber::new());
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    grant(&app, &opened.handle, TrustChoice::TrustProject);
    snapshot(&app, &opened.handle, &renamed(&opened.document, "Running")).unwrap();
    assert_eq!(snapshot_files(&app).len(), 1);

    let (build_id, sink) = app.build(&opened.handle, &opened.document);
    let events = wait_finished(&sink);
    assert!(
        matches!(
            events.last(),
            Some(BuildEvent::Finished {
                outcome: BuildOutcome::Built,
                ..
            })
        ),
        "{events:#?}"
    );
    let (output, run_events) = run_sinks();
    app.backend
        .run_start(
            RunStartRequest {
                build_id,
                run_options: RunOptions { cols: 80, rows: 24 },
            },
            output.clone(),
            run_events.clone(),
        )
        .unwrap();
    // The guessing game waits for input once it has printed its prompt.
    let deadline = Instant::now() + common::TIMEOUT;
    while output.concat().is_empty() {
        assert!(Instant::now() < deadline, "the program printed nothing");
        std::thread::sleep(Duration::from_millis(10));
    }
    let pids = program_processes(&app.dirs.builds());
    assert!(!pids.is_empty(), "the program is not running");

    app.backend
        .project_close(ProjectCloseRequest {
            handle: opened.handle.clone(),
        })
        .unwrap();
    assert!(matches!(
        wait_exit(&run_events).last(),
        Some(RunEvent::Exit { .. })
    ));
    let deadline = Instant::now() + Duration::from_secs(10);
    while pids
        .iter()
        .any(|pid| std::path::Path::new(&format!("/proc/{pid}")).exists())
    {
        assert!(Instant::now() < deadline, "the program survived its project");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(snapshot_files(&app).is_empty());
    assert_eq!(
        snapshot(&app, &opened.handle, &opened.document).unwrap_err(),
        IpcError::UnknownHandle
    );
}

/// The processes whose executable lies below `builds` (Linux:
/// `/proc/<pid>/exe`).
#[cfg(target_os = "linux")]
fn program_processes(builds: &std::path::Path) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let exe = std::fs::read_link(entry.path().join("exe")).ok()?;
            exe.starts_with(builds).then_some(pid)
        })
        .collect()
}
