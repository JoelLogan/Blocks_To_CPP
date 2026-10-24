//! The command line and the editor list the same problems
//! (`docs/spec/06-compiler-pipeline.md` §6.1): for every example, every
//! file of the malicious-project suite (`tests/security/projects/`) and
//! every golden file of the block upgrades
//! (`crates/b2c-catalog/tests/migrations/`), the diagnostics of
//! `b2c_build::run_frontend`, which `b2c check` and the build run, equal
//! those of the editor's live preview (`b2c_core_wasm::preview_document`),
//! code for code and in order, and the two agree on whether the project
//! builds. Parked blocks (ADR-0012) only warn, so a project with one
//! previews as buildable. Both upgrade old blocks first (05 §5.7), so a
//! block that cannot be upgraded, or one from a newer catalog, is reported
//! the same way.
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
    let upgrades = project_files("crates/b2c-catalog/tests/migrations");
    assert!(examples.len() >= 15, "{}", examples.len());
    assert!(security.len() >= 80, "{}", security.len());
    assert!(upgrades.len() >= 4, "{}", upgrades.len());
    let mut compared = 0;
    for (name, bytes) in examples.iter().chain(&security).chain(&upgrades) {
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
    assert!(compared >= 94, "{compared}");
}

/// `examples/hello_world.b2c` with its `print` block changed by `change`.
fn hello_world_with(change: impl FnOnce(&mut serde_json::Value)) -> String {
    let bytes = std::fs::read(repo_root().join("examples/hello_world.b2c")).unwrap();
    let mut project: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    change(&mut project);
    serde_json::to_string(&project).unwrap()
}

/// A block the upgrade cannot upgrade (`B2C-E0603`), and a block from a
/// newer catalog with a field this one does not know (`B2C-E0602`, not
/// `E0605`): the same problem, once, in both, and neither builds.
#[test]
fn old_blocks_and_newer_catalogs_are_reported_alike() {
    let print = "/modules/0/workspace/blocks/0/statements/BODY/0";
    let cases = [
        (
            hello_world_with(|project| {
                project.pointer_mut(print).unwrap()["v"] = 0.into();
            }),
            "B2C-E0603 Error",
        ),
        (
            hello_world_with(|project| {
                project["generator"]["catalog"] = "99.0.0".into();
                project.pointer_mut(print).unwrap()["fields"]["VOLUME"] = "3".into();
            }),
            "B2C-E0602 Error",
        ),
    ];
    for (text, expected) in cases {
        let preview = preview_document(&text, &PreviewOptions::default());
        assert_eq!(listed(&preview.diagnostics), [expected]);
        assert!(!preview.buildable);
        let cli = run_frontend(text.as_bytes(), &FrontendOptions::default());
        assert_eq!(cli.diagnostics, preview.diagnostics);
        assert!(cli.generated.is_none());
    }
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

/// `/statements/BODY` of hello world's `when program starts`.
const BODY: &str = "/modules/0/workspace/blocks/0/statements/BODY";

/// Hello world's `print` block.
const PRINT: &str = "/modules/0/workspace/blocks/0/statements/BODY/0";

/// An `ask` straight from the toolbox: no variable chosen yet, so the
/// editor saves it without the field `VAR`.
fn unchosen_ask() -> serde_json::Value {
    serde_json::json!({
        "id": "b_ask", "type": "io.ask", "v": 1,
        "inputs": {"PROMPT": {"expr": [{"str": "Your answer: "}]}}
    })
}

/// `create int guess = 1`.
fn create_guess() -> serde_json::Value {
    serde_json::json!({
        "id": "b_create", "type": "var.declare", "v": 1,
        "fields": {"TYPE": "int", "NAME": {"sym": "sym_guess", "name": "guess"}},
        "inputs": {"VALUE": {"expr": [{"num": "1"}]}}
    })
}

/// Puts `block` directly on hello world's canvas, at (400, 40).
fn on_canvas(project: &mut serde_json::Value, mut block: serde_json::Value) {
    block["x"] = 400.into();
    block["y"] = 40.into();
    project["modules"][0]["workspace"]["blocks"]
        .as_array_mut()
        .unwrap()
        .push(block);
}

/// Attaches `block` at the end of hello world's `when program starts`.
fn attach(project: &mut serde_json::Value, block: serde_json::Value) {
    project
        .pointer_mut(BODY)
        .unwrap()
        .as_array_mut()
        .unwrap()
        .push(block);
}

/// A case of [`each_problem_is_listed_once`]: what it is, the project, the
/// diagnostics it lists and whether it builds.
type Case = (&'static str, String, &'static [&'static str], bool);

/// Blocks in which nothing is chosen yet, parked and attached.
fn unchosen_cases() -> Vec<Case> {
    vec![
        (
            "a parked ask without a variable",
            hello_world_with(|project| on_canvas(project, unchosen_ask())),
            &["B2C-W0505 Warning"],
            true,
        ),
        (
            "an ask without a variable below a create, in a loose stack",
            hello_world_with(|project| {
                let mut head = create_guess();
                head["stack"] = serde_json::json!([unchosen_ask()]);
                on_canvas(project, head);
            }),
            &["B2C-W0505 Warning"],
            true,
        ),
        (
            "an attached ask without a variable",
            hello_world_with(|project| attach(project, unchosen_ask())),
            &["B2C-E0430 Error"],
            false,
        ),
        (
            "an attached create without a name",
            hello_world_with(|project| {
                let mut block = create_guess();
                block["fields"].as_object_mut().unwrap().remove("NAME");
                attach(project, block);
            }),
            &["B2C-E0430 Error"],
            false,
        ),
    ]
}

/// Problems the catalog reports, and blocks it keeps unchecked.
fn catalog_cases() -> Vec<Case> {
    vec![
        (
            "a dropdown value that is not an option",
            hello_world_with(|project| {
                project.pointer_mut(PRINT).unwrap()["fields"]["SEP"] = "bogus".into();
            }),
            &["B2C-E0607 Error"],
            false,
        ),
        (
            "newer dropdown options",
            hello_world_with(|project| {
                project["generator"]["catalog"] = "99.0.0".into();
                let print = project.pointer_mut(PRINT).unwrap();
                print["fields"]["SEP"] = "tabs".into();
                print["inputs"]["ITEM0"] = serde_json::json!({"block": {
                    "id": "b_pow", "type": "math.arithmetic", "v": 1, "fields": {"OP": "pow"},
                    "inputs": {"A": {"expr": [{"num": "2"}]}, "B": {"expr": [{"num": "3"}]}}
                }});
            }),
            &["B2C-E0602 Error", "B2C-E0602 Error"],
            false,
        ),
        (
            "a newer field, with an input left to its default",
            hello_world_with(|project| {
                project["generator"]["catalog"] = "99.0.0".into();
                attach(project, create_guess());
                let change = serde_json::json!({
                    "id": "b_change", "type": "var.change", "v": 1,
                    "fields": {"VAR": {"ref": "sym_guess"}, "ROUNDING": "up"}
                });
                attach(project, change);
            }),
            &["B2C-E0602 Error"],
            false,
        ),
        (
            "an old block with a choice that is not an option",
            hello_world_with(|project| {
                let print = project.pointer_mut(PRINT).unwrap();
                print["v"] = 0.into();
                print["fields"]["SEP"] = "bogus".into();
            }),
            &["B2C-E0603 Error"],
            false,
        ),
    ]
}

/// Each problem is listed once, in both, and a block straight from the
/// toolbox in which nothing is chosen yet is parked like any other:
///
/// * an `ask` without its variable (the editor saves an unchosen variable as
///   an absent field), alone on the canvas or below a `create` in a loose
///   stack, is one warning and builds (▶ Run stays enabled); attached, it is
///   one error (`B2C-E0430`), not also a catalog error;
/// * a field the catalog rejects is the catalog's error alone, not also the
///   analyser's (`B2C-E0607` for a dropdown value that is not an option);
/// * a block the catalog keeps unchecked, from a newer catalog (`B2C-E0602`)
///   or too old to upgrade (`B2C-E0603`), is that error alone: a choice this
///   catalog does not know, or an input left to a default it did not fill
///   in, is no second error.
#[test]
fn each_problem_is_listed_once() {
    for (what, text, expected, buildable) in unchosen_cases().into_iter().chain(catalog_cases()) {
        let preview = preview_document(&text, &PreviewOptions::default());
        assert_eq!(
            listed(&preview.diagnostics),
            expected,
            "{what}: {:#?}",
            preview.diagnostics
        );
        assert_eq!(preview.buildable, buildable, "{what}");
        let cli = run_frontend(text.as_bytes(), &FrontendOptions::default());
        assert_eq!(cli.diagnostics, preview.diagnostics, "{what}");
        assert_eq!(cli.generated.is_some(), buildable, "{what}");
    }
}
