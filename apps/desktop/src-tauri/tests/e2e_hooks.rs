//! The end-to-end seams (feature `e2e-hooks`): a dialog script drives the
//! open, save-as and trust dialogs of the real backend through the app's IPC,
//! as the end-to-end tests do in the real app
//! ([ADR-0009](../../../../docs/adr/0009-e2e-tooling-and-test-seams.md)).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

mod common;

use std::sync::Arc;

use blocks2cpp_desktop::e2e::{DialogScript, ScriptedDialogs};
use common::{Harness, repo_root};
use serde_json::{Value, json};

#[test]
fn a_script_answers_open_save_as_and_trust_in_order() {
    let root = tempfile::tempdir().unwrap();
    let projects = root.path().join("projects");
    std::fs::create_dir(&projects).unwrap();
    let example = std::fs::read(repo_root().join("examples/hello_world.b2c")).unwrap();
    let first = projects.join("first.b2c");
    let second = projects.join("second.b2c");
    std::fs::write(&first, &example).unwrap();
    std::fs::write(&second, &example).unwrap();
    let saved = projects.join("saved.b2c");

    let script = json!({
        "open": [first, second, null],
        "saveAs": [saved],
        "trust": ["trustProject"],
    });
    let script_file = root.path().join("dialogs.json");
    std::fs::write(&script_file, script.to_string()).unwrap();
    let dialogs = Arc::new(ScriptedDialogs::new(DialogScript::read(&script_file).unwrap()));
    let profile = root.path().join("profile");
    let backend = common::backend(&profile, Vec::new(), dialogs);
    let app = Harness::with_backend(backend, root, false);

    // Save as: the script's path, with trust recorded for a project created here.
    let created = app
        .invoke("project_new", json!({ "request": { "template": "helloWorld" } }))
        .unwrap();
    let saved_as = app
        .invoke(
            "project_save_as_dialog",
            json!({ "request": { "handle": created["handle"], "document": created["document"] } }),
        )
        .unwrap();
    assert_eq!(saved_as["status"], "ok", "{saved_as}");
    assert_eq!(saved_as["fileName"], "saved.b2c");
    assert!(saved.is_file());

    // Open: the first file is not trusted; the trust dialog's answer trusts it.
    let opened = open(&app);
    assert_eq!(opened["fileName"], "first.b2c");
    assert_eq!(opened["trust"]["state"], "restricted", "{opened}");
    let granted = app
        .invoke(
            "trust_grant",
            json!({ "request": { "handle": opened["handle"] } }),
        )
        .unwrap();
    assert_eq!(granted["trust"]["state"], "trusted", "{granted}");
    assert_eq!(granted["trust"]["source"], "project");
    assert!(profile.join("machine").join("trust.json").is_file());

    // The second file: the trust answers are used up, so it stays restricted.
    let other = open(&app);
    assert_eq!(other["fileName"], "second.b2c");
    let refused = app
        .invoke("trust_grant", json!({ "request": { "handle": other["handle"] } }))
        .unwrap();
    assert_eq!(refused["trust"]["state"], "restricted", "{refused}");

    // `null`, then an exhausted list: both cancel.
    for _ in 0..2 {
        let cancelled = app.invoke("project_open_dialog", json!({})).unwrap();
        assert_eq!(cancelled, json!({ "status": "cancelled" }));
    }
    let add = app.invoke("toolchain_add_dialog", json!({})).unwrap();
    assert_eq!(add, json!({ "status": "cancelled" }));
}

fn open(app: &Harness) -> Value {
    let opened = app.invoke("project_open_dialog", json!({})).unwrap();
    assert_eq!(opened["status"], "ok", "{opened}");
    opened
}
