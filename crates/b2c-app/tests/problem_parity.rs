//! The command line and the editor list the same problems
//! (`docs/spec/06-compiler-pipeline.md` §6.1): for every example and every
//! file of the malicious-project suite (`tests/security/projects/`), the
//! diagnostics of `b2c_build::run_frontend`, which `b2c check` and the build
//! run, equal those of the editor's live preview
//! (`b2c_core_wasm::preview_document`), code for code and in order, and the
//! two agree on whether the project builds. Parked blocks (ADR-0012) only
//! warn, so a project with one previews as buildable.
//!
//! The test lives here because this crate is where the build front end and
//! the WebAssembly core both compile natively.

// Test code: helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use b2c_build::{FrontendOptions, Stage, run_frontend};
use b2c_core_wasm::{PreviewOptions, preview_document};
use b2c_ir::{Diagnostic, Severity};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The `.b2c` files of a folder of the repository, by name.
fn project_files(folder: &str) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(repo_root().join(folder))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "b2c"))
        .map(|path| {
            let name = format!("{folder}/{}", path.file_name().unwrap().to_string_lossy());
            (name, std::fs::read(&path).unwrap())
        })
        .collect();
    files.sort();
    files
}

/// `code severity` for each diagnostic, in order.
fn listed(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .map(|d| format!("{} {:?}", d.code.0, d.severity))
        .collect()
}

#[test]
fn the_cli_and_the_preview_list_the_same_problems() {
    let examples = project_files("examples");
    let security = project_files("tests/security/projects");
    assert!(examples.len() >= 15, "{}", examples.len());
    assert!(security.len() >= 80, "{}", security.len());
    let mut compared = 0;
    for (name, bytes) in examples.iter().chain(&security) {
        let cli = run_frontend(bytes, &FrontendOptions::default());
        // The editor only ever hands the core text: a file that is not
        // UTF-8 is refused by the loader before it could reach the preview.
        let Ok(text) = std::str::from_utf8(bytes) else {
            assert_eq!(cli.stage, Stage::Load, "{name}");
            continue;
        };
        let preview = preview_document(text, &PreviewOptions::default());
        assert_eq!(listed(&cli.diagnostics), listed(&preview.diagnostics), "{name}");
        assert_eq!(cli.diagnostics, preview.diagnostics, "{name}");
        let builds = cli.generated.is_some() && !cli.has_errors();
        assert_eq!(builds, preview.buildable, "{name}");
        compared += 1;
    }
    assert!(compared >= 90, "{compared}");
}

/// A `print` left loose next to `when program starts`: one warning in both,
/// and both would build (`buildable` in the preview, which keeps ▶ Run
/// enabled).
#[test]
fn a_parked_block_is_buildable_in_the_cli_and_the_preview() {
    let bytes = std::fs::read(repo_root().join("examples/hello_world.b2c")).unwrap();
    let mut project: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    project["modules"][0]["workspace"]["blocks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": "b_parked", "type": "io.print", "v": 1, "x": 400, "y": 40,
            "inputs": {"ITEM0": {"expr": [{"str": "not run"}]}}
        }));
    let text = serde_json::to_string(&project).unwrap();
    let preview = preview_document(&text, &PreviewOptions::default());
    assert!(preview.buildable);
    assert_eq!(listed(&preview.diagnostics), ["B2C-W0505 Warning"]);
    assert_eq!(preview.diagnostics[0].severity, Severity::Warning);
    let cli = run_frontend(text.as_bytes(), &FrontendOptions::default());
    assert_eq!(cli.diagnostics, preview.diagnostics);
    assert!(!cli.has_errors());
    // The preview shows exactly the C++ of the project without the block.
    let plain = preview_document(std::str::from_utf8(&bytes).unwrap(), &PreviewOptions::default());
    assert_eq!(preview.files, plain.files);
}
