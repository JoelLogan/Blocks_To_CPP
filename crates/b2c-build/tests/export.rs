//! Export as a C++ project (`docs/spec/06-compiler-pipeline.md` §6.11):
//! exported examples build with `make` and with CMake, without Blocks2Cpp,
//! and the programs do what their golden run data says (the same check as
//! `golden.rs`). The builds need g++ (required with `B2C_REQUIRE_GXX`), and
//! `make` or `cmake` on `PATH`; a tool that is missing skips its part. CMake
//! is used on Linux only (on Windows it would choose MSVC).

// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::{Path, PathBuf};

use b2c_build::export::{self, EXPORT_MANIFEST, ExportMode, FolderState};
use common::repo_root;
use common::run_data::RunData;

/// The examples built here: plain output, input, vectors and text.
const EXAMPLES: [&str; 4] = ["hello_world", "greeting", "matrix_sum", "palindrome"];

/// A command for a build tool. Production code spawns processes only
/// through b2c-process; tests drive the tools directly.
#[allow(clippy::disallowed_methods)]
fn tool(name: &str) -> std::process::Command {
    std::process::Command::new(name)
}

/// Whether `name` runs (`--version`).
fn available(name: &str) -> bool {
    tool(name)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn exported(name: &str, folder: &Path) -> export::Export {
    let bytes = std::fs::read(repo_root().join("examples").join(format!("{name}.b2c"))).unwrap();
    let export = export::prepare(&bytes, 4, "0.0.0-test").unwrap();
    export::write(folder, &export, ExportMode::New).unwrap();
    export
}

fn run(tool_output: &std::process::Output, what: &str) {
    assert!(
        tool_output.status.success(),
        "{what} failed:\n{}\n{}",
        String::from_utf8_lossy(&tool_output.stdout),
        String::from_utf8_lossy(&tool_output.stderr)
    );
}

fn check(name: &str, executable: &Path) {
    let data = RunData::read(&repo_root().join("tests/golden").join(name)).unwrap();
    data.check(executable)
        .unwrap_or_else(|problem| panic!("{name}: {problem}"));
}

fn program(folder: &Path, name: &str) -> PathBuf {
    folder.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

#[test]
fn exported_examples_build_with_make() {
    if common::gxx().is_none() || !available("make") {
        return;
    }
    for name in EXAMPLES {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("export");
        let export = exported(name, &folder);
        for config in ["debug", "release"] {
            run(
                &tool("make")
                    .arg("-C")
                    .arg(&folder)
                    .arg(format!("CONFIG={config}"))
                    .arg("-B")
                    .output()
                    .unwrap(),
                &format!("make {name} {config}"),
            );
            check(name, &program(&folder, &export.name));
        }
    }
}

#[cfg(unix)]
#[test]
fn exported_examples_build_with_cmake() {
    if common::gxx().is_none() || !available("cmake") {
        return;
    }
    for name in &EXAMPLES[..2] {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("export");
        let export = exported(name, &folder);
        for config in ["Debug", "Release"] {
            let build = folder.join(format!("build-{config}"));
            run(
                &tool("cmake")
                    .arg("-S")
                    .arg(&folder)
                    .arg("-B")
                    .arg(&build)
                    .arg(format!("-DCMAKE_BUILD_TYPE={config}"))
                    .arg("-DCMAKE_CXX_COMPILER=g++")
                    .output()
                    .unwrap(),
                &format!("cmake configure {name} {config}"),
            );
            run(
                &tool("cmake").arg("--build").arg(&build).output().unwrap(),
                &format!("cmake build {name} {config}"),
            );
            check(name, &build.join(&export.name));
        }
    }
}

#[test]
fn an_export_never_writes_over_what_it_did_not_make() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("export");
    let export = exported("hello_world", &folder);
    assert_eq!(export::folder_state(&folder), FolderState::EarlierExport);
    assert!(folder.join(EXPORT_MANIFEST).is_file());
    // Again as new: refused; as a replacement: only its own files change.
    assert!(matches!(
        export::write(&folder, &export, ExportMode::New),
        Err(export::ExportError::NotEmpty)
    ));
    std::fs::write(folder.join("notes.txt"), "mine").unwrap();
    std::fs::write(folder.join("src").join("mine.cpp"), "// mine").unwrap();
    std::fs::write(folder.join("Makefile"), "changed").unwrap();
    let written = export::write(&folder, &export, ExportMode::Replace).unwrap();
    assert_eq!(written.last().map(String::as_str), Some(EXPORT_MANIFEST));
    assert_eq!(std::fs::read_to_string(folder.join("notes.txt")).unwrap(), "mine");
    assert_eq!(
        std::fs::read_to_string(folder.join("src/mine.cpp")).unwrap(),
        "// mine"
    );
    assert_ne!(
        std::fs::read_to_string(folder.join("Makefile")).unwrap(),
        "changed"
    );

    // A folder with other things in it is not an export.
    let other = dir.path().join("other");
    std::fs::create_dir(&other).unwrap();
    std::fs::write(other.join("a.txt"), "a").unwrap();
    assert_eq!(export::folder_state(&other), FolderState::Other);
    assert!(matches!(
        export::write(&other, &export, ExportMode::Replace),
        Err(export::ExportError::NoEarlierExport)
    ));
    assert_eq!(
        export::folder_state(&dir.path().join("missing")),
        FolderState::Empty
    );
}

#[cfg(unix)]
#[test]
fn links_are_never_written_through() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("Makefile"), "outside").unwrap();
    // The folder itself is a link.
    let link = dir.path().join("export");
    std::os::unix::fs::symlink(outside.path(), &link).unwrap();
    let bytes = std::fs::read(repo_root().join("examples/hello_world.b2c")).unwrap();
    let export = export::prepare(&bytes, 4, "0.0.0-test").unwrap();
    assert!(export::write(&link, &export, ExportMode::New).is_err());
    assert!(export::write(&link, &export, ExportMode::Replace).is_err());
    // A file of an earlier export replaced by a link.
    let folder = dir.path().join("real");
    export::write(&folder, &export, ExportMode::New).unwrap();
    std::fs::remove_file(folder.join("Makefile")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("Makefile"), folder.join("Makefile")).unwrap();
    assert!(matches!(
        export::write(&folder, &export, ExportMode::Replace),
        Err(export::ExportError::NotAPlainFile)
    ));
    assert_eq!(
        std::fs::read_to_string(outside.path().join("Makefile")).unwrap(),
        "outside"
    );
}

#[test]
fn a_project_with_errors_is_not_exported() {
    let error = export::prepare(b"{ not a project", 4, "0.0.0-test").unwrap_err();
    assert!(matches!(error, export::ExportError::ProjectErrors(problems) if !problems.is_empty()));
}

#[test]
fn a_replacement_never_writes_over_a_file_the_earlier_export_did_not_list() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("export");
    let export = exported("hello_world", &folder);
    // As if the earlier export had no Makefile and someone wrote their own.
    let manifest = std::fs::read_to_string(folder.join(EXPORT_MANIFEST)).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    value["files"]
        .as_array_mut()
        .unwrap()
        .retain(|file| file != "Makefile");
    std::fs::write(folder.join(EXPORT_MANIFEST), value.to_string()).unwrap();
    std::fs::write(folder.join("Makefile"), "mine").unwrap();
    assert!(matches!(
        export::write(&folder, &export, ExportMode::Replace),
        Err(export::ExportError::NotWrittenByExport(path)) if path == "Makefile"
    ));
    assert_eq!(std::fs::read_to_string(folder.join("Makefile")).unwrap(), "mine");
}

#[test]
fn a_text_define_the_build_tools_cannot_pass_is_refused() {
    let bytes = std::fs::read(repo_root().join("examples/hello_world.b2c")).unwrap();
    let mut project: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for (text, exportable) in [
        ("a b \"c\" \\ d", true),
        ("a;b", false),
        ("x#y", false),
        ("q $<1:z>", false),
    ] {
        project["project"]["build"]["defines"] =
            serde_json::json!([{ "name": "GREETING", "value": { "string": text } }]);
        let result = export::prepare(project.to_string().as_bytes(), 4, "0.0.0-test");
        match result {
            Ok(_) => assert!(exportable, "{text}"),
            Err(export::ExportError::ProjectErrors(problems)) => {
                assert!(!exportable, "{text}");
                assert_eq!(problems.len(), 1);
                assert_eq!(problems[0].code, b2c_ir::DiagCode::new("B2C-T1019"));
            }
            Err(error) => panic!("{text}: {error}"),
        }
    }
}
