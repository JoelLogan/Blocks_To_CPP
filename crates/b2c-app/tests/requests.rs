//! Untrusted requests (`docs/spec/02-architecture.md` §2.5.1 and §2.5.4,
//! `docs/spec/08-security.md` §8.8 and §8.12 T5, T8): every request is
//! decoded as the desktop adapter decodes it (`b2c_ipc::decode`) and then
//! handled by the backend. Malformed requests are rejected with the right
//! code and change nothing; forged opaque IDs give typed errors.

// Test code: helpers fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

mod common;

use std::sync::Arc;

use b2c_ipc::dto::{
    BuildCancelRequest, BuildConfig, BuildStartRequest, ProjectCloseRequest, ProjectNewRequest,
    ProjectOpenRecentRequest, ProjectReloadRequest, ProjectSaveAsDialogRequest, ProjectSaveRequest,
    ProjectSetDirtyRequest, RecentRemoveRequest, RunAckRequest, RunInputRequest, RunOptions,
    RunResizeRequest, RunStartRequest, RunStopRequest, SettingsPatch, Template, ToolchainSelectRequest,
    TrustGetRequest, TrustGrantRequest, TrustRevokeRequest,
};
use b2c_ipc::sink::testing::RecordingSink;
use b2c_ipc::{
    BuildId, Handle, InvalidReason, IpcError, RecentId, RunId, ToolchainId, decode, encode_base64,
};
use common::{TestApp, example_text, run_sinks};
use serde_json::json;

/// A JSON request with a real handle in it.
fn with_handle(handle: &Handle, mut value: serde_json::Value) -> serde_json::Value {
    value["handle"] = json!(handle.as_str());
    value
}

#[test]
fn malformed_requests_change_nothing() {
    let app = TestApp::new();
    let path = app.write("hello.b2c", example_text("hello_world").as_bytes());
    let opened = app.open(&path);
    let handle = opened.handle;
    let before = std::fs::read(&path).unwrap();
    let document = example_text("hello_world");

    // An unknown nested field.
    let error =
        decode::<SettingsPatch>(json!({"console": {"scrollbackLines": 2000, "extra": 1}})).unwrap_err();
    assert_eq!(
        error,
        IpcError::invalid(InvalidReason::UnknownField, Some("console.extra"))
    );
    let error = decode::<RunStartRequest>(json!({
        "buildId": BuildId::example().as_str(),
        "runOptions": {"cols": 80, "rows": 24, "shell": "/bin/sh"}
    }))
    .unwrap_err();
    assert!(
        matches!(
            error,
            IpcError::InvalidRequest {
                reason: InvalidReason::UnknownField,
                ..
            }
        ),
        "{error:?}"
    );

    // An oversize document is refused by the decoder and by the backend,
    // without being parsed.
    let oversize = format!("{{{}", " ".repeat(b2c_ipc::limits::MAX_DOCUMENT_BYTES));
    let error =
        decode::<ProjectSaveRequest>(with_handle(&handle, json!({"document": oversize}))).unwrap_err();
    assert_eq!(error, IpcError::PayloadTooLarge { limit: 33_554_432 });
    let error = app
        .backend
        .project_save(ProjectSaveRequest {
            handle: handle.clone(),
            document: oversize.clone(),
        })
        .unwrap_err();
    assert_eq!(error, IpcError::PayloadTooLarge { limit: 33_554_432 });
    let error = app
        .backend
        .build_start(
            BuildStartRequest {
                handle: handle.clone(),
                document: oversize,
                config: BuildConfig::Debug,
            },
            Arc::new(RecordingSink::new()),
        )
        .unwrap_err();
    assert_eq!(error, IpcError::PayloadTooLarge { limit: 33_554_432 });

    // A duplicate key: the strict loader refuses it (B2C-E0105).
    let duplicate = document.replacen("\"name\": \"Hello World\"", "\"name\": \"A\", \"name\": \"B\"", 1);
    assert_ne!(duplicate, document);
    let request = decode::<ProjectSaveRequest>(with_handle(&handle, json!({"document": duplicate}))).unwrap();
    let IpcError::InvalidDocument { diagnostics } = app.backend.project_save(request).unwrap_err() else {
        panic!("expected invalidDocument");
    };
    assert!(
        diagnostics.iter().any(|d| d.code == "B2C-E0105"),
        "{diagnostics:?}"
    );

    // A bad enum value.
    let error = decode::<ProjectNewRequest>(json!({"template": "guessingGame"})).unwrap_err();
    assert_eq!(error, IpcError::invalid(InvalidReason::BadEnum, Some("template")));
    let error = decode::<BuildStartRequest>(with_handle(
        &handle,
        json!({"document": document, "config": "fastest"}),
    ))
    .unwrap_err();
    assert_eq!(error, IpcError::invalid(InvalidReason::BadEnum, Some("config")));

    // A malformed ID.
    for bad in [
        "ph_xyz",
        "PH_0123456789abcdef0123456789abcdef",
        "../../etc/passwd",
        "",
    ] {
        let error = decode::<TrustGetRequest>(json!({"handle": bad})).unwrap_err();
        assert_eq!(
            error,
            IpcError::invalid(InvalidReason::BadId, Some("handle")),
            "{bad}"
        );
    }
    let error = decode::<ToolchainSelectRequest>(json!({"toolchainId": "/usr/bin/g++"})).unwrap_err();
    assert_eq!(
        error,
        IpcError::invalid(InvalidReason::BadId, Some("toolchainId"))
    );

    // run_input of 64 KiB + 1 byte.
    let data = encode_base64(&vec![b'x'; 65_537]);
    let error =
        decode::<RunInputRequest>(json!({"runId": RunId::example().as_str(), "data": data})).unwrap_err();
    assert!(matches!(error, IpcError::PayloadTooLarge { .. }), "{error:?}");
    let error = app
        .backend
        .run_input(RunInputRequest {
            run_id: RunId::example(),
            data,
        })
        .unwrap_err();
    assert!(matches!(error, IpcError::PayloadTooLarge { .. }), "{error:?}");

    // A path where a request has none.
    let error = decode::<ProjectSaveRequest>(with_handle(
        &handle,
        json!({"document": document, "path": "/tmp/evil.b2c"}),
    ))
    .unwrap_err();
    assert_eq!(
        error,
        IpcError::invalid(InvalidReason::UnknownField, Some("path"))
    );

    // Nothing changed: the file, the handle, the projects, the settings.
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!app.projects().join("hello.b2c.bak").exists());
    assert!(!app.dirs.config.join("settings.json").exists());
    let trust = app
        .backend
        .trust_get(TrustGetRequest {
            handle: handle.clone(),
        })
        .unwrap();
    assert_eq!(trust.trust, opened.trust);
    assert!(!app.has_builds());
    assert!(!app.spawned());
}

#[test]
fn settings_updates_with_unknown_keys_leave_the_file_unchanged() {
    let app = TestApp::new();
    let request = decode::<SettingsPatch>(json!({"console": {"scrollbackLines": 2000}})).unwrap();
    app.backend.settings_update(request).unwrap();
    let file = app.dirs.config.join("settings.json");
    let before = std::fs::read(&file).unwrap();
    for bad in [
        json!({"toolchain": {"selectedId": "tc_0123456789abcdef"}}),
        json!({"newProject": {"standard": "c++26"}}),
        json!({"buildCache": {"maxBytes": 1}}),
        json!({"codeStyle": {"indentWidth": 3}}),
        json!({"codeStyle": {"indentWidth": 2, "tabs": true}}),
        json!({"console": {"scrollbackLines": 999}}),
        json!({"run": {"onErrors": "explode"}}),
        json!({"console": null}),
    ] {
        let error = decode::<SettingsPatch>(bad.clone()).unwrap_err();
        assert!(
            matches!(error, IpcError::InvalidRequest { .. }),
            "{bad}: {error:?}"
        );
    }
    assert_eq!(std::fs::read(&file).unwrap(), before);
    assert_eq!(
        app.backend
            .settings_get()
            .unwrap()
            .settings
            .console
            .scrollback_lines,
        2000
    );
}

#[test]
fn forged_ids_give_typed_errors() {
    let app = TestApp::new();
    let handle = Handle::random().unwrap();
    let document = example_text("hello_world");
    let backend = &app.backend;

    assert_eq!(
        backend
            .trust_get(TrustGetRequest {
                handle: handle.clone()
            })
            .unwrap_err(),
        IpcError::UnknownHandle
    );
    assert_eq!(
        backend
            .trust_grant(TrustGrantRequest {
                handle: handle.clone()
            })
            .unwrap_err(),
        IpcError::UnknownHandle
    );
    assert_eq!(
        backend
            .trust_revoke(TrustRevokeRequest {
                handle: handle.clone()
            })
            .unwrap_err(),
        IpcError::UnknownHandle
    );
    assert_eq!(
        backend
            .project_save(ProjectSaveRequest {
                handle: handle.clone(),
                document: document.clone(),
            })
            .unwrap_err(),
        IpcError::UnknownHandle
    );
    assert_eq!(
        backend
            .project_save_as_dialog(ProjectSaveAsDialogRequest {
                handle: handle.clone(),
                document: document.clone(),
            })
            .unwrap_err(),
        IpcError::UnknownHandle
    );
    assert_eq!(
        backend
            .project_reload(ProjectReloadRequest {
                handle: handle.clone()
            })
            .unwrap_err(),
        IpcError::UnknownHandle
    );
    assert_eq!(
        backend
            .project_close(ProjectCloseRequest {
                handle: handle.clone()
            })
            .unwrap_err(),
        IpcError::UnknownHandle
    );
    assert_eq!(
        backend
            .project_set_dirty(ProjectSetDirtyRequest {
                handle: handle.clone(),
                dirty: true,
            })
            .unwrap_err(),
        IpcError::UnknownHandle
    );
    assert_eq!(
        backend
            .build_start(
                BuildStartRequest {
                    handle,
                    document,
                    config: BuildConfig::Release,
                },
                Arc::new(RecordingSink::new()),
            )
            .unwrap_err(),
        IpcError::UnknownHandle
    );
    assert_eq!(app.dialogs.calls(), 0);

    let build_id = BuildId::random().unwrap();
    assert_eq!(
        backend
            .build_cancel(BuildCancelRequest {
                build_id: build_id.clone()
            })
            .unwrap_err(),
        IpcError::UnknownBuild
    );
    let (output, events) = run_sinks();
    assert_eq!(
        backend
            .run_start(
                RunStartRequest {
                    build_id,
                    run_options: RunOptions { cols: 80, rows: 24 },
                },
                output.clone(),
                events.clone(),
            )
            .unwrap_err(),
        IpcError::UnknownBuild
    );
    assert!(events.is_empty() && output.batches().is_empty());

    let run_id = RunId::random().unwrap();
    assert_eq!(
        backend
            .run_input(RunInputRequest {
                run_id: run_id.clone(),
                data: String::from("NDIK"),
            })
            .unwrap_err(),
        IpcError::UnknownRun
    );
    assert_eq!(
        backend
            .run_resize(RunResizeRequest {
                run_id: run_id.clone(),
                cols: 100,
                rows: 30,
            })
            .unwrap_err(),
        IpcError::UnknownRun
    );
    assert_eq!(
        backend
            .run_stop(RunStopRequest {
                run_id: run_id.clone()
            })
            .unwrap_err(),
        IpcError::UnknownRun
    );
    assert_eq!(
        backend.run_ack(RunAckRequest { run_id, seq: 0 }).unwrap_err(),
        IpcError::UnknownRun
    );

    let recent_id = RecentId::random().unwrap();
    assert_eq!(
        backend
            .project_open_recent(ProjectOpenRecentRequest {
                recent_id: recent_id.clone()
            })
            .unwrap_err(),
        IpcError::UnknownRecent
    );
    assert_eq!(
        backend
            .recent_remove(RecentRemoveRequest { recent_id })
            .unwrap_err(),
        IpcError::UnknownRecent
    );

    let toolchain_id = ToolchainId::for_driver(std::path::Path::new("/opt/forged/g++"));
    assert_eq!(
        backend
            .toolchain_select(ToolchainSelectRequest { toolchain_id })
            .unwrap_err(),
        IpcError::UnknownToolchain
    );
    assert!(!app.dirs.config.join("settings.json").exists());
}

#[test]
fn resize_bounds_are_checked_before_the_run_is_looked_up() {
    let app = TestApp::new();
    let run_id = RunId::random().unwrap();
    for (cols, rows, field) in [
        (1, 24, "cols"),
        (1001, 24, "cols"),
        (80, 0, "rows"),
        (80, 1001, "rows"),
    ] {
        let error = decode::<RunResizeRequest>(json!({"runId": run_id.as_str(), "cols": cols, "rows": rows}))
            .unwrap_err();
        assert_eq!(error, IpcError::invalid(InvalidReason::OutOfRange, Some(field)));
        let error = app
            .backend
            .run_resize(RunResizeRequest {
                run_id: run_id.clone(),
                cols,
                rows,
            })
            .unwrap_err();
        assert_eq!(error, IpcError::invalid(InvalidReason::OutOfRange, Some(field)));
    }
    let error = decode::<RunStartRequest>(json!({
        "buildId": BuildId::example().as_str(),
        "runOptions": {"cols": 80, "rows": 5000}
    }))
    .unwrap_err();
    assert_eq!(
        error,
        IpcError::invalid(InvalidReason::OutOfRange, Some("runOptions.rows"))
    );
}

#[test]
fn new_projects_need_a_known_template() {
    let app = TestApp::new();
    for template in ["", "Empty", "../empty", "helloworld"] {
        assert!(decode::<ProjectNewRequest>(json!({"template": template})).is_err());
    }
    let first = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::Empty,
        })
        .unwrap();
    assert!(first.handle.as_str().starts_with("ph_"));
}
