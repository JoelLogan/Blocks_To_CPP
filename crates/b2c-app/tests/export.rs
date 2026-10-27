//! `project_export_dialog` (`docs/spec/06-compiler-pipeline.md` §6.11): the
//! folder comes from the (scripted) dialog, the export goes into a folder
//! named after the project, an earlier export of the same project is
//! replaced only after the user agrees, and a folder with other files or
//! another project's export is never written into.

// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use b2c_ipc::IpcError;
use b2c_ipc::dto::{ProjectExportDialogRequest, ProjectNewRequest, Template};
use common::TestApp;

#[test]
fn a_project_is_exported_into_a_folder_of_its_own() {
    let app = TestApp::new();
    let created = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::HelloWorld,
        })
        .unwrap();
    let export = |document: &str| {
        app.backend.project_export_dialog(ProjectExportDialogRequest {
            handle: created.handle.clone(),
            document: document.to_owned(),
        })
    };
    // Cancelled: nothing is written.
    app.dialogs.will_pick_export(None);
    let response = export(&created.document).unwrap();
    assert!(response.cancelled);

    let parent = app.root.path().join("exports");
    std::fs::create_dir(&parent).unwrap();
    app.dialogs.will_pick_export(Some(&parent));
    let response = export(&created.document).unwrap();
    assert!(!response.cancelled);
    let project: serde_json::Value = serde_json::from_str(&created.document).unwrap();
    let name = b2c_build::export::folder_name(project["project"]["name"].as_str().unwrap()).unwrap();
    assert_eq!(response.folder_name.as_deref(), Some(name.as_str()));
    assert_eq!(response.files, 5);
    let folder = parent.join(&name);
    assert!(folder.join("src/main.cpp").is_file());
    assert!(folder.join("CMakeLists.txt").is_file());

    // Again: the user is asked; no means nothing changes.
    std::fs::write(folder.join("Makefile"), "edited").unwrap();
    app.dialogs.will_pick_export(Some(&parent));
    app.dialogs.will_replace_export(false);
    assert!(export(&created.document).unwrap().cancelled);
    assert_eq!(
        std::fs::read_to_string(folder.join("Makefile")).unwrap(),
        "edited"
    );
    app.dialogs.will_pick_export(Some(&parent));
    app.dialogs.will_replace_export(true);
    assert!(!export(&created.document).unwrap().cancelled);
    assert_ne!(
        std::fs::read_to_string(folder.join("Makefile")).unwrap(),
        "edited"
    );

    // A folder of the same name with other files is refused.
    let other = app.root.path().join("other");
    std::fs::create_dir_all(other.join(&name)).unwrap();
    std::fs::write(other.join(&name).join("keep.txt"), "mine").unwrap();
    app.dialogs.will_pick_export(Some(&other));
    assert!(matches!(export(&created.document), Err(IpcError::Io { .. })));
    assert_eq!(
        std::fs::read_to_string(other.join(&name).join("keep.txt")).unwrap(),
        "mine"
    );

    // Another project of the same name (its first module also `main`) is
    // refused without a question, and the first one's code stays.
    let second = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::HelloWorld,
        })
        .unwrap();
    std::fs::write(folder.join("src/main.cpp"), "// edited").unwrap();
    app.dialogs.will_pick_export(Some(&parent));
    let shown = app.dialogs.calls();
    let refused = app.backend.project_export_dialog(ProjectExportDialogRequest {
        handle: second.handle.clone(),
        document: second.document.clone(),
    });
    assert!(
        matches!(
            refused,
            Err(IpcError::Io {
                kind: b2c_ipc::IoKind::AlreadyExists
            })
        ),
        "{refused:?}"
    );
    assert_eq!(app.dialogs.calls(), shown + 1, "only the folder was asked for");
    assert_eq!(
        std::fs::read_to_string(folder.join("src/main.cpp")).unwrap(),
        "// edited"
    );

    // A document that is not a project is refused before any dialog.
    assert!(export("{}").is_err());

    // A text define the build files cannot pass gives its own error.
    let mut document: serde_json::Value = serde_json::from_str(&created.document).unwrap();
    document["project"]["build"]["defines"] =
        serde_json::json!([{ "name": "PRICE", "value": { "string": "$5" } }]);
    assert_eq!(export(&document.to_string()), Err(IpcError::DefineNotExportable));
}
