//! The project lifecycle (`docs/spec/02-architecture.md` §2.5.2,
//! `docs/spec/05-project-format.md` §5.6–§5.10, `docs/spec/08-security.md`
//! §8.3 and §8.6): new projects from templates, opening through the dialog
//! and the recent list, the size bound and the format version, saving
//! (byte-stable, with `.bak`, never over an outside change), *Save as*,
//! reloading, closing, dirty tracking, the handle limit and one dialog at a
//! time. Untrusted files (`tests/security/projects/`) and the examples open
//! in Restricted Mode, or not at all, and are never compiled.

// Test code: helpers fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

mod common;

use std::sync::Arc;

use b2c_build::FrontendOptions;
use b2c_ipc::dto::{
    AppEvent, ProjectCloseRequest, ProjectNewRequest, ProjectOpenDialogResponse, ProjectOpenRecentRequest,
    ProjectReloadRequest, ProjectSaveAsDialogRequest, ProjectSaveAsDialogResponse, ProjectSaveRequest,
    ProjectSetDirtyRequest, RecentRemoveRequest, RestrictedReason, Template, TrustGetRequest, TrustSource,
    TrustState,
};
use b2c_ipc::{IpcError, limits::MAX_OPEN_HANDLES};
use common::{Compiler, TestApp, TestProber, app_sink, edit, example_text, examples, repo_root};

#[test]
fn new_projects_come_from_the_templates() {
    let app = TestApp::new();
    let mut ids = Vec::new();
    let mut handles = Vec::new();
    for template in [Template::Empty, Template::HelloWorld, Template::Empty] {
        let created = app.backend.project_new(ProjectNewRequest { template }).unwrap();
        assert_eq!(created.trust.state, TrustState::Trusted);
        assert_eq!(created.trust.source, Some(TrustSource::CreatedHere));
        let trust = app
            .backend
            .trust_get(TrustGetRequest {
                handle: created.handle.clone(),
            })
            .unwrap();
        assert_eq!(trust.trust, created.trust);
        // The document is canonical, loads, resolves and analyses without
        // errors, and generates C++.
        let document = b2c_model::load(created.document.as_bytes()).unwrap();
        assert_eq!(b2c_model::to_canonical_json(&document), created.document);
        let frontend = b2c_build::run_frontend(created.document.as_bytes(), &FrontendOptions::default());
        assert!(!frontend.has_errors(), "{:#?}", frontend.diagnostics);
        assert!(frontend.generated.is_some());
        assert_eq!(
            document.project.language.standard,
            b2c_ir::sast::CppStandard::Cpp20
        );
        assert_eq!(document.generator.app, "0.1.0");
        ids.push(document.project.id);
        handles.push(created.handle);
    }
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 3, "every project gets its own ID");
    handles.sort();
    handles.dedup();
    assert_eq!(handles.len(), 3, "every project gets its own handle");
    // No file, so no save and no reload.
    let handle = handles.remove(0);
    assert_eq!(
        app.backend
            .project_save(ProjectSaveRequest {
                handle: handle.clone(),
                document: example_text("hello_world"),
            })
            .unwrap_err(),
        IpcError::NoPath
    );
    assert_eq!(
        app.backend
            .project_reload(ProjectReloadRequest { handle })
            .unwrap_err(),
        IpcError::NoPath
    );
}

#[test]
fn examples_open_restricted_with_an_empty_trust_store() {
    let app = TestApp::new();
    for (name, bytes) in examples() {
        let path = app.write(&format!("{name}.b2c"), &bytes);
        let opened = app.open(&path);
        assert_eq!(opened.trust.state, TrustState::Restricted, "{name}");
        assert_eq!(opened.trust.restricted_reason, Some(RestrictedReason::NoRecord));
        assert!(!opened.trust.mark_of_the_web);
        assert_eq!(opened.file_name, format!("{name}.b2c"));
        assert_eq!(opened.migrated_from, None);
        let loaded = b2c_model::load(&bytes).unwrap();
        assert_eq!(opened.document, b2c_model::to_canonical_json(&loaded), "{name}");
        app.backend
            .project_close(ProjectCloseRequest {
                handle: opened.handle,
            })
            .unwrap();
    }
    assert!(app.trust_json().is_none());
    assert!(!app.has_builds());
}

#[test]
fn security_test_projects_are_rejected_or_restricted_and_never_compiled() {
    let app = TestApp::with(Compiler::Spy, TestProber::new());
    app.wait_discovery();
    let folder = repo_root().join("tests/security/projects");
    let mut files: Vec<_> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "b2c"))
        .collect();
    files.sort();
    assert!(files.len() > 20, "{}", files.len());
    let (mut rejected, mut restricted) = (0, 0);
    for file in files {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        let path = app.write(&name, &std::fs::read(&file).unwrap());
        app.dialogs.will_open(&path);
        match app.backend.project_open_dialog() {
            Err(error) => {
                assert!(
                    matches!(
                        error,
                        IpcError::InvalidDocument { .. }
                            | IpcError::NewerFormat { .. }
                            | IpcError::PayloadTooLarge { .. }
                    ),
                    "{name}: {error:?}"
                );
                rejected += 1;
            }
            Ok(ProjectOpenDialogResponse::Ok(opened)) => {
                assert_eq!(opened.trust.state, TrustState::Restricted, "{name}");
                // Asking to build it directly, as a compromised webview would.
                let sink = Arc::new(b2c_ipc::sink::testing::RecordingSink::new());
                let error = app
                    .backend
                    .build_start(
                        b2c_ipc::dto::BuildStartRequest {
                            handle: opened.handle.clone(),
                            document: opened.document.clone(),
                            config: b2c_ipc::dto::BuildConfig::Debug,
                        },
                        sink.clone(),
                    )
                    .unwrap_err();
                assert_eq!(error, IpcError::Restricted, "{name}");
                assert!(sink.is_empty());
                app.backend
                    .project_close(ProjectCloseRequest {
                        handle: opened.handle,
                    })
                    .unwrap();
                restricted += 1;
            }
            Ok(ProjectOpenDialogResponse::Cancelled) => panic!("{name}: cancelled"),
        }
    }
    assert!(
        rejected > 0 && restricted > 0,
        "{rejected} rejected, {restricted} restricted"
    );
    assert!(!app.spawned(), "a compiler ran for an untrusted project");
    assert!(!app.has_builds());
    assert!(app.trust_json().is_none());
}

#[test]
fn oversized_files_and_newer_formats_are_refused() {
    let app = TestApp::new();
    let huge = app.projects().join("huge.b2c");
    std::fs::File::create(&huge)
        .unwrap()
        .set_len(b2c_ipc::limits::MAX_DOCUMENT_BYTES as u64 + 1)
        .unwrap();
    app.dialogs.will_open(&huge);
    assert_eq!(
        app.backend.project_open_dialog().unwrap_err(),
        IpcError::PayloadTooLarge { limit: 33_554_432 }
    );

    let newer = edit(&example_text("hello_world"), |project| {
        project["formatVersion"] = serde_json::json!(9);
        project["generator"]["app"] = serde_json::json!("4.2.0");
    });
    let path = app.write("newer.b2c", newer.as_bytes());
    app.dialogs.will_open(&path);
    assert_eq!(
        app.backend.project_open_dialog().unwrap_err(),
        IpcError::NewerFormat {
            needs: Some(String::from("4.2.0"))
        }
    );

    let missing = app.projects().join("missing.b2c");
    app.dialogs.will_open(&missing);
    assert_eq!(app.backend.project_open_dialog().unwrap_err(), IpcError::NotFound);

    // Invalid UTF-8 is refused by the loader with its diagnostics.
    let path = app.write(
        "latin1.b2c",
        b"{\"format\": \"blocks2cpp/project\", \"x\": \"\xe9\"}",
    );
    app.dialogs.will_open(&path);
    assert!(matches!(
        app.backend.project_open_dialog().unwrap_err(),
        IpcError::InvalidDocument { .. }
    ));
    // Nothing was opened.
    assert!(app.backend.recent_list().unwrap().entries.is_empty());
}

#[test]
fn a_cancelled_open_dialog_is_not_an_error() {
    let app = TestApp::new();
    assert_eq!(
        app.backend.project_open_dialog().unwrap(),
        ProjectOpenDialogResponse::Cancelled
    );
    assert_eq!(app.dialogs.calls(), 1);
}

#[test]
fn saving_is_byte_stable_keeps_a_backup_and_never_overwrites_outside_changes() {
    let app = TestApp::new();
    let original = example_text("guessing_game");
    let path = app.write("game.b2c", original.as_bytes());
    let opened = app.open(&path);
    let handle = opened.handle.clone();

    let first = app
        .backend
        .project_save(ProjectSaveRequest {
            handle: handle.clone(),
            document: opened.document.clone(),
        })
        .unwrap();
    let after_first = std::fs::read(&path).unwrap();
    assert_eq!(
        after_first,
        opened.document.as_bytes(),
        "the canonical form is saved"
    );
    assert_eq!(
        first.hash,
        b2c_model::hex(&b2c_store::project_file::sha256(&after_first))
    );
    assert!(
        b2c_store::parse_rfc3339_utc(&first.saved_at).is_some(),
        "{}",
        first.saved_at
    );
    assert_eq!(
        std::fs::read(app.projects().join("game.b2c.bak")).unwrap(),
        original.as_bytes(),
        "the .bak holds the previous file"
    );

    let second = app
        .backend
        .project_save(ProjectSaveRequest {
            handle: handle.clone(),
            document: opened.document.clone(),
        })
        .unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        after_first,
        "saving twice is byte-identical"
    );
    assert_eq!(second.hash, first.hash);
    assert_eq!(
        std::fs::read(app.projects().join("game.b2c.bak")).unwrap(),
        after_first
    );

    // A change outside the app between two saves: refused, file untouched.
    let outside = edit(&original, |project| {
        project["project"]["name"] = serde_json::json!("Changed");
    });
    std::fs::write(&path, &outside).unwrap();
    let edited = edit(&opened.document, |project| {
        project["project"]["name"] = serde_json::json!("Mine");
    });
    assert_eq!(
        app.backend
            .project_save(ProjectSaveRequest {
                handle: handle.clone(),
                document: edited.clone(),
            })
            .unwrap_err(),
        IpcError::ChangedOnDisk
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), outside);
    // A deleted file counts as changed too.
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        app.backend
            .project_save(ProjectSaveRequest {
                handle,
                document: edited,
            })
            .unwrap_err(),
        IpcError::ChangedOnDisk
    );
    assert!(!path.exists());
}

#[test]
fn save_as_writes_only_the_new_file_and_rebinds() {
    let app = TestApp::new();
    let original = example_text("hello_world");
    let old = app.write("old.b2c", original.as_bytes());
    let opened = app.open(&old);
    let handle = opened.handle.clone();
    let renamed = edit(&opened.document, |project| {
        project["project"]["name"] = serde_json::json!("Renamed");
    });

    // Cancelled: nothing written.
    assert_eq!(
        app.backend
            .project_save_as_dialog(ProjectSaveAsDialogRequest {
                handle: handle.clone(),
                document: renamed.clone(),
            })
            .unwrap(),
        ProjectSaveAsDialogResponse::Cancelled
    );
    assert_eq!(app.dialogs.suggestions(), ["old.b2c"]);

    let new = app.projects().join("new.b2c");
    app.dialogs.will_save_as(&new);
    let ProjectSaveAsDialogResponse::Ok(saved) = app
        .backend
        .project_save_as_dialog(ProjectSaveAsDialogRequest {
            handle: handle.clone(),
            document: renamed.clone(),
        })
        .unwrap()
    else {
        panic!("cancelled");
    };
    assert_eq!(saved.handle, handle);
    assert_eq!(saved.file_name, "new.b2c");
    assert_eq!(
        std::fs::read_to_string(&new).unwrap(),
        renamed_canonical(&renamed)
    );
    assert_eq!(
        std::fs::read_to_string(&old).unwrap(),
        original,
        "the old file is untouched"
    );

    // Later saves go to the new file only.
    let again = edit(&renamed, |project| {
        project["project"]["name"] = serde_json::json!("Again");
    });
    app.backend
        .project_save(ProjectSaveRequest {
            handle: handle.clone(),
            document: again.clone(),
        })
        .unwrap();
    assert_eq!(std::fs::read_to_string(&new).unwrap(), renamed_canonical(&again));
    assert_eq!(std::fs::read_to_string(&old).unwrap(), original);
    assert!(!app.projects().join("old.b2c.bak").exists());
    // A restricted project stays restricted.
    let trust = app.backend.trust_get(TrustGetRequest { handle }).unwrap().trust;
    assert_eq!(trust.state, TrustState::Restricted);
    assert!(app.trust_json().is_none());
    // The new file is the most recent project.
    let recent = app.backend.recent_list().unwrap().entries;
    assert_eq!(recent[0].display_path, new.display().to_string());
}

/// The canonical text of a document.
fn renamed_canonical(text: &str) -> String {
    b2c_model::to_canonical_json(&b2c_model::load(text.as_bytes()).unwrap())
}

#[test]
fn the_first_save_of_a_new_project_goes_through_save_as() {
    let app = TestApp::new();
    let created = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::HelloWorld,
        })
        .unwrap();
    let path = app.projects().join("hello.b2c");
    app.dialogs.will_save_as(&path);
    let response = app
        .backend
        .project_save_as_dialog(ProjectSaveAsDialogRequest {
            handle: created.handle.clone(),
            document: created.document.clone(),
        })
        .unwrap();
    assert!(matches!(response, ProjectSaveAsDialogResponse::Ok(_)));
    assert_eq!(app.dialogs.suggestions(), ["Hello World.b2c"]);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), created.document);
    // Trust is recorded for the file.
    let trust = app
        .backend
        .trust_get(TrustGetRequest {
            handle: created.handle,
        })
        .unwrap()
        .trust;
    assert_eq!(trust.state, TrustState::Trusted);
    assert_eq!(trust.source, Some(TrustSource::Project));
    let json = app.trust_json().unwrap();
    assert_eq!(json["projects"][0]["canonicalPath"], path.display().to_string());
    // Reopening it is trusted by its record.
    let reopened = app.open(&path);
    assert_eq!(reopened.trust.source, Some(TrustSource::Project));
}

#[test]
fn the_recent_list_follows_opens() {
    let app = TestApp::new();
    let first = app.write("first.b2c", example_text("hello_world").as_bytes());
    let second = app.write("second.b2c", example_text("countdown").as_bytes());
    app.open(&first);
    app.open(&second);
    let entries = app.backend.recent_list().unwrap().entries;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].display_path, second.display().to_string());
    assert_eq!(entries[1].display_path, first.display().to_string());
    assert_eq!(entries[1].project_name, "Hello World");
    // Opening by ID gives the same document and moves the entry to the front.
    let reopened = app
        .backend
        .project_open_recent(ProjectOpenRecentRequest {
            recent_id: entries[1].recent_id.clone(),
        })
        .unwrap();
    assert_eq!(reopened.document, renamed_canonical(&example_text("hello_world")));
    assert_eq!(reopened.trust.state, TrustState::Restricted);
    let after = app.backend.recent_list().unwrap().entries;
    assert_eq!(after[0].recent_id, entries[1].recent_id);
    assert_eq!(after[1].recent_id, entries[0].recent_id);
    // A deleted file gives notFound and stays in the list.
    std::fs::remove_file(&second).unwrap();
    assert_eq!(
        app.backend
            .project_open_recent(ProjectOpenRecentRequest {
                recent_id: entries[0].recent_id.clone(),
            })
            .unwrap_err(),
        IpcError::NotFound
    );
    assert_eq!(app.backend.recent_list().unwrap().entries.len(), 2);
    // Removing it.
    app.backend
        .recent_remove(RecentRemoveRequest {
            recent_id: entries[0].recent_id.clone(),
        })
        .unwrap();
    assert_eq!(
        app.backend
            .recent_remove(RecentRemoveRequest {
                recent_id: entries[0].recent_id.clone(),
            })
            .unwrap_err(),
        IpcError::UnknownRecent
    );
    assert_eq!(app.backend.recent_list().unwrap().entries.len(), 1);
}

#[test]
fn reload_reads_the_file_again() {
    let app = TestApp::new();
    let path = app.write("game.b2c", example_text("hello_world").as_bytes());
    let opened = app.open(&path);
    let changed = edit(&example_text("hello_world"), |project| {
        project["project"]["name"] = serde_json::json!("Changed outside");
    });
    std::fs::write(&path, &changed).unwrap();
    let reloaded = app
        .backend
        .project_reload(ProjectReloadRequest {
            handle: opened.handle.clone(),
        })
        .unwrap();
    assert_eq!(reloaded.document, renamed_canonical(&changed));
    // The reloaded file is the new baseline: saving it works.
    app.backend
        .project_save(ProjectSaveRequest {
            handle: opened.handle.clone(),
            document: reloaded.document,
        })
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        app.backend
            .project_reload(ProjectReloadRequest {
                handle: opened.handle
            })
            .unwrap_err(),
        IpcError::NotFound
    );
}

#[test]
fn closing_forgets_the_handle() {
    let app = TestApp::new();
    let created = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::Empty,
        })
        .unwrap();
    app.backend
        .project_close(ProjectCloseRequest {
            handle: created.handle.clone(),
        })
        .unwrap();
    assert_eq!(
        app.backend
            .trust_get(TrustGetRequest {
                handle: created.handle.clone()
            })
            .unwrap_err(),
        IpcError::UnknownHandle
    );
    assert_eq!(
        app.backend
            .project_close(ProjectCloseRequest {
                handle: created.handle
            })
            .unwrap_err(),
        IpcError::UnknownHandle
    );
}

#[test]
fn dirty_projects_ask_before_the_window_closes() {
    let app = TestApp::new();
    let created = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::Empty,
        })
        .unwrap();
    assert!(!app.backend.has_dirty());
    assert!(app.backend.request_close());
    app.backend
        .project_set_dirty(ProjectSetDirtyRequest {
            handle: created.handle.clone(),
            dirty: true,
        })
        .unwrap();
    assert!(app.backend.has_dirty());
    // Nobody to ask: the window closes.
    assert!(app.backend.request_close());
    let events = app_sink();
    app.backend.app_subscribe(events.clone());
    assert!(!app.backend.request_close());
    assert_eq!(events.events(), [AppEvent::CloseRequested]);
    app.backend
        .project_set_dirty(ProjectSetDirtyRequest {
            handle: created.handle,
            dirty: false,
        })
        .unwrap();
    assert!(app.backend.request_close());
    assert_eq!(events.len(), 1);
}

#[test]
fn at_most_32_projects_are_open() {
    let app = TestApp::new();
    let mut handles = Vec::new();
    for _ in 0..MAX_OPEN_HANDLES {
        handles.push(
            app.backend
                .project_new(ProjectNewRequest {
                    template: Template::Empty,
                })
                .unwrap()
                .handle,
        );
    }
    assert_eq!(
        app.backend
            .project_new(ProjectNewRequest {
                template: Template::Empty,
            })
            .unwrap_err(),
        IpcError::TooManyHandles
    );
    // No dialog is shown when no project could be opened anyway.
    assert_eq!(
        app.backend.project_open_dialog().unwrap_err(),
        IpcError::TooManyHandles
    );
    assert_eq!(app.dialogs.calls(), 0);
    app.backend
        .project_close(ProjectCloseRequest {
            handle: handles.remove(0),
        })
        .unwrap();
    app.backend
        .project_new(ProjectNewRequest {
            template: Template::Empty,
        })
        .unwrap();
}

#[test]
fn one_native_dialog_at_a_time() {
    let app = TestApp::new();
    app.dialogs.hold();
    let backend = Arc::clone(&app.backend);
    let waiting = std::thread::spawn(move || backend.project_open_dialog());
    app.dialogs.wait_inside();
    assert_eq!(app.backend.project_open_dialog().unwrap_err(), IpcError::Busy);
    assert_eq!(app.backend.toolchain_add_dialog().unwrap_err(), IpcError::Busy);
    let created = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::Empty,
        })
        .unwrap();
    assert_eq!(
        app.backend
            .project_save_as_dialog(ProjectSaveAsDialogRequest {
                handle: created.handle,
                document: created.document,
            })
            .unwrap_err(),
        IpcError::Busy
    );
    app.dialogs.release();
    assert_eq!(
        waiting.join().unwrap().unwrap(),
        ProjectOpenDialogResponse::Cancelled
    );
    // Free again.
    assert_eq!(
        app.backend.project_open_dialog().unwrap(),
        ProjectOpenDialogResponse::Cancelled
    );
}
