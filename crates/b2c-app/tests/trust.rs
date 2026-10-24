//! Workspace trust through the backend (`docs/spec/08-security.md` §8.3 and
//! §8.3.1): every answer of the native dialog, what the dialog lists, the
//! rate limit, revocation, folder trust, outside changes against saves in
//! the app, and that nothing but the dialog (and saves of trusted projects)
//! ever changes trust.

// Test code: helpers fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

mod common;

use std::sync::Arc;

use b2c_app::TrustChoice;
use b2c_ipc::dto::{
    BuildConfig, BuildStartRequest, ProjectCloseRequest, ProjectNewRequest, ProjectReloadRequest,
    ProjectSaveRequest, ProjectSetDirtyRequest, RestrictedReason, SettingsPatch, Template, TrustGetRequest,
    TrustGrantRequest, TrustRevokeRequest, TrustSource, TrustState,
};
use b2c_ipc::sink::testing::RecordingSink;
use b2c_ipc::{Handle, IpcError};
use common::{TestApp, edit, example_text};

fn grant(app: &TestApp, handle: &Handle) -> Result<b2c_ipc::dto::Trust, IpcError> {
    app.backend
        .trust_grant(TrustGrantRequest {
            handle: handle.clone(),
        })
        .map(|response| response.trust)
}

fn trust(app: &TestApp, handle: &Handle) -> b2c_ipc::dto::Trust {
    app.backend
        .trust_get(TrustGetRequest {
            handle: handle.clone(),
        })
        .unwrap()
        .trust
}

/// Waits out the trust dialog's rate limit.
fn after_rate_limit() {
    std::thread::sleep(b2c_app::limits::TRUST_GRANT_INTERVAL);
}

#[test]
fn trusting_the_project_records_its_hash() {
    let app = TestApp::new();
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    assert_eq!(opened.trust.state, TrustState::Restricted);
    app.dialogs.will_trust(TrustChoice::TrustProject);
    let granted = grant(&app, &opened.handle).unwrap();
    assert_eq!(granted.state, TrustState::Trusted);
    assert_eq!(granted.source, Some(TrustSource::Project));
    assert_eq!(trust(&app, &opened.handle), granted);

    let json = app.trust_json().unwrap();
    let records = json["projects"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    let document = b2c_model::load(opened.document.as_bytes()).unwrap();
    assert_eq!(records[0]["projectId"], document.project.id.as_str());
    assert_eq!(records[0]["canonicalPath"], path.display().to_string());
    assert_eq!(
        records[0]["rawCodeHashAtGrant"],
        b2c_model::hex(&b2c_model::security_hash(&document))
    );
    assert!(json["folders"].as_array().unwrap().is_empty());

    // Another open of the file is trusted by the record.
    let again = app.open(&path);
    assert_eq!(again.trust.source, Some(TrustSource::Project));
    // A trusted project is not asked about again.
    let calls = app.dialogs.calls();
    assert_eq!(grant(&app, &again.handle).unwrap(), again.trust);
    assert_eq!(app.dialogs.calls(), calls);
}

#[test]
fn trusting_the_folder_covers_every_project_in_it() {
    let app = TestApp::new();
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    app.dialogs.will_trust(TrustChoice::TrustFolder);
    let granted = grant(&app, &opened.handle).unwrap();
    assert_eq!(granted.source, Some(TrustSource::Folder));
    let json = app.trust_json().unwrap();
    assert!(json["projects"].as_array().unwrap().is_empty());
    assert_eq!(
        json["folders"][0]["canonicalPath"],
        app.projects().display().to_string()
    );

    let sibling = app.write("other.b2c", example_text("countdown").as_bytes());
    assert_eq!(app.open(&sibling).trust.source, Some(TrustSource::Folder));
    // Folder trust stores no hash, so outside changes do not re-flag.
    let changed = edit(&example_text("countdown"), |project| {
        project["project"]["build"]["defines"] = serde_json::json!([{"name": "LEVEL", "value": {"int": 9}}]);
    });
    std::fs::write(&sibling, changed).unwrap();
    assert_eq!(app.open(&sibling).trust.source, Some(TrustSource::Folder));

    // Revoking removes only project records: the folder still covers it.
    let revoked = app
        .backend
        .trust_revoke(TrustRevokeRequest {
            handle: opened.handle.clone(),
        })
        .unwrap()
        .trust;
    assert_eq!(revoked.state, TrustState::Trusted);
    assert_eq!(revoked.source, Some(TrustSource::Folder));
}

#[test]
fn staying_restricted_changes_nothing() {
    let app = TestApp::new();
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    app.dialogs.will_trust(TrustChoice::StayRestricted);
    assert_eq!(grant(&app, &opened.handle).unwrap(), opened.trust);
    assert_eq!(trust(&app, &opened.handle), opened.trust);
    assert!(app.trust_json().is_none());
    // An unscripted dialog is a closed one: also restricted.
    after_rate_limit();
    assert_eq!(grant(&app, &opened.handle).unwrap(), opened.trust);
    assert_eq!(app.dialogs.prompts().len(), 2);
    assert!(app.trust_json().is_none());
}

#[test]
fn trust_grant_is_rate_limited() {
    let app = TestApp::new();
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    grant(&app, &opened.handle).unwrap();
    assert_eq!(grant(&app, &opened.handle).unwrap_err(), IpcError::RateLimited);
    assert_eq!(app.dialogs.prompts().len(), 1);
    after_rate_limit();
    app.dialogs.will_trust(TrustChoice::TrustProject);
    assert_eq!(grant(&app, &opened.handle).unwrap().state, TrustState::Trusted);
}

#[test]
fn the_dialog_lists_the_latest_document() {
    let app = TestApp::new();
    let path = app.write("game.b2c", example_text("hello_world").as_bytes());
    let opened = app.open(&path);
    // The webview sends a document with library requirements in a build
    // request: refused (restricted), but it becomes the latest document.
    let with_libraries = edit(&opened.document, |project| {
        project["project"]["build"]["libraries"] = serde_json::json!(["sfml-graphics", "zlib"]);
        project["project"]["name"] = serde_json::json!("Evil\u{200b}name");
    });
    let error = app
        .backend
        .build_start(
            BuildStartRequest {
                handle: opened.handle.clone(),
                document: with_libraries.clone(),
                config: BuildConfig::Debug,
            },
            Arc::new(RecordingSink::new()),
        )
        .unwrap_err();
    assert_eq!(error, IpcError::Restricted);
    app.dialogs.will_trust(TrustChoice::TrustProject);
    grant(&app, &opened.handle).unwrap();
    let prompt = app.dialogs.prompts().pop().unwrap();
    assert_eq!(prompt.libraries, ["sfml-graphics", "zlib"]);
    assert_eq!(prompt.raw_cpp_blocks, 0);
    assert_eq!(prompt.file_system_blocks, 0);
    assert_eq!(
        prompt.project_name, "Evil⟨U+200B⟩name",
        "hidden characters are shown"
    );
    assert_eq!(prompt.folder_display, app.projects().display().to_string());
    assert!(!prompt.mark_of_the_web);
    // The record holds that document's security hash.
    let listed = b2c_model::load(with_libraries.as_bytes()).unwrap();
    assert_eq!(
        app.trust_json().unwrap()["projects"][0]["rawCodeHashAtGrant"],
        b2c_model::hex(&b2c_model::security_hash(&listed))
    );
    let (title, body) = b2c_app::trust_dialog_text(&prompt);
    assert_eq!(title, "Trust this project?");
    assert!(body.contains(b2c_app::TRUST_WARNING));
}

#[test]
fn an_outside_change_restricts_but_a_save_in_the_app_does_not() {
    let app = TestApp::new();
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    app.dialogs.will_trust(TrustChoice::TrustProject);
    grant(&app, &opened.handle).unwrap();

    // The same define edit, saved in the app: the record follows.
    let with_define = edit(&opened.document, |project| {
        project["project"]["build"]["defines"] = serde_json::json!([{"name": "LEVEL", "value": {"int": 3}}]);
    });
    app.backend
        .project_save(ProjectSaveRequest {
            handle: opened.handle.clone(),
            document: with_define.clone(),
        })
        .unwrap();
    let saved = b2c_model::load(with_define.as_bytes()).unwrap();
    assert_eq!(
        app.trust_json().unwrap()["projects"][0]["rawCodeHashAtGrant"],
        b2c_model::hex(&b2c_model::security_hash(&saved))
    );
    let reopened = app.open(&path);
    assert_eq!(reopened.trust.source, Some(TrustSource::Project));

    // A define edited outside the app: restricted at the next load.
    let outside = edit(&with_define, |project| {
        project["project"]["build"]["defines"][0]["value"] = serde_json::json!({"int": 4});
    });
    std::fs::write(&path, outside).unwrap();
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
    let reopened = app.open(&path);
    assert_eq!(
        reopened.trust.restricted_reason,
        Some(RestrictedReason::ChangedOutside)
    );
    // Saving the restricted project does not make the change trusted.
    app.backend
        .project_save(ProjectSaveRequest {
            handle: reopened.handle.clone(),
            document: reopened.document.clone(),
        })
        .unwrap();
    assert_eq!(
        app.open(&path).trust.restricted_reason,
        Some(RestrictedReason::ChangedOutside)
    );
    assert_eq!(
        app.trust_json().unwrap()["projects"][0]["rawCodeHashAtGrant"],
        b2c_model::hex(&b2c_model::security_hash(&saved))
    );
}

#[test]
fn nothing_but_the_dialog_trusts_a_project() {
    let app = TestApp::new();
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    let handle = opened.handle.clone();
    let backend = &app.backend;
    // Every command that does not show the trust dialog.
    backend
        .project_save(ProjectSaveRequest {
            handle: handle.clone(),
            document: opened.document.clone(),
        })
        .unwrap();
    backend
        .project_reload(ProjectReloadRequest {
            handle: handle.clone(),
        })
        .unwrap();
    backend
        .project_set_dirty(ProjectSetDirtyRequest {
            handle: handle.clone(),
            dirty: true,
        })
        .unwrap();
    assert_eq!(
        backend
            .build_start(
                BuildStartRequest {
                    handle: handle.clone(),
                    document: opened.document.clone(),
                    config: BuildConfig::Release,
                },
                Arc::new(RecordingSink::new()),
            )
            .unwrap_err(),
        IpcError::Restricted
    );
    backend.settings_update(SettingsPatch::default()).unwrap();
    backend.toolchain_list().unwrap();
    backend.recent_list().unwrap();
    let revoked = backend
        .trust_revoke(TrustRevokeRequest {
            handle: handle.clone(),
        })
        .unwrap()
        .trust;
    assert_eq!(revoked.state, TrustState::Restricted);
    assert_eq!(trust(&app, &handle).state, TrustState::Restricted);
    assert!(app.trust_json().is_none());
    assert!(app.dialogs.prompts().is_empty());
    backend.project_close(ProjectCloseRequest { handle }).unwrap();
    assert_eq!(app.open(&path).trust.state, TrustState::Restricted);
}

#[test]
fn revoking_a_project_record() {
    let app = TestApp::new();
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    app.dialogs.will_trust(TrustChoice::TrustProject);
    grant(&app, &opened.handle).unwrap();
    let revoked = app
        .backend
        .trust_revoke(TrustRevokeRequest {
            handle: opened.handle.clone(),
        })
        .unwrap()
        .trust;
    assert_eq!(revoked.state, TrustState::Restricted);
    assert_eq!(revoked.restricted_reason, Some(RestrictedReason::NoRecord));
    assert!(
        app.trust_json().unwrap()["projects"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        app.open(&path).trust.restricted_reason,
        Some(RestrictedReason::NoRecord)
    );
}

#[test]
fn new_projects_are_trusted_until_revoked() {
    let app = TestApp::new();
    let created = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::HelloWorld,
        })
        .unwrap();
    assert_eq!(created.trust.source, Some(TrustSource::CreatedHere));
    let revoked = app
        .backend
        .trust_revoke(TrustRevokeRequest {
            handle: created.handle.clone(),
        })
        .unwrap()
        .trust;
    assert_eq!(revoked.state, TrustState::Restricted);
    assert!(app.trust_json().is_none());
    // Trusting it again needs the dialog; without a file it is trusted in
    // memory, as a new project.
    app.dialogs.will_trust(TrustChoice::TrustFolder);
    let granted = grant(&app, &created.handle).unwrap();
    assert_eq!(granted.source, Some(TrustSource::CreatedHere));
    assert!(app.trust_json().is_none());
    assert_eq!(app.dialogs.prompts()[0].folder_display, "");
}
