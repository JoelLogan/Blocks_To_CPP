//! `project_export_dialog` (`docs/spec/06-compiler-pipeline.md` §6.11): the
//! folder comes from the (scripted) dialog, the export goes into a folder
//! named after the program, an earlier export is replaced only after the
//! user agrees, and a folder with other files is never written into.

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
    assert_eq!(response.folder_name.as_deref(), Some("main"));
    assert_eq!(response.files, 5);
    let folder = parent.join("main");
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
    std::fs::create_dir_all(other.join("main")).unwrap();
    std::fs::write(other.join("main").join("keep.txt"), "mine").unwrap();
    app.dialogs.will_pick_export(Some(&other));
    assert!(matches!(export(&created.document), Err(IpcError::Io { .. })));
    assert_eq!(
        std::fs::read_to_string(other.join("main/keep.txt")).unwrap(),
        "mine"
    );

    // A document that is not a project is refused before any dialog.
    assert!(export("{}").is_err());
}
