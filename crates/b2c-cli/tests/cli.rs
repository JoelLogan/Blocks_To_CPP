//! End-to-end tests of the `b2c` binary: arguments, exit codes and output.

// Spawning the binary under test is the point of these tests, and helper
// functions outside `#[test]`s fail the test by panicking.
#![allow(clippy::disallowed_methods, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn example(name: &str) -> PathBuf {
    repo_root().join("examples").join(format!("{name}.b2c"))
}

fn b2c(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_b2c"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn version_and_help_succeed() {
    let output = b2c(&["--version"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).starts_with("b2c "));
    assert_eq!(b2c(&["--help"]).status.code(), Some(0));
}

#[test]
fn usage_errors_exit_with_2() {
    assert_eq!(b2c(&[]).status.code(), Some(2));
    assert_eq!(b2c(&["frobnicate"]).status.code(), Some(2));
    assert_eq!(b2c(&["check"]).status.code(), Some(2));
}

#[test]
fn missing_file_is_a_usage_error() {
    let output = b2c(&["check", "definitely/not/here.b2c"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).starts_with("b2c: cannot read definitely/not/here.b2c"));
}

#[test]
fn check_accepts_every_example() {
    for entry in std::fs::read_dir(repo_root().join("examples")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "b2c") {
            let output = b2c(&["check", path.to_str().unwrap()]);
            assert_eq!(
                output.status.code(),
                Some(0),
                "{}:\n{}",
                path.display(),
                stdout(&output)
            );
            assert!(stdout(&output).ends_with(": no errors\n"));
        }
    }
}

#[test]
fn check_reports_errors_with_exit_1() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.b2c");
    std::fs::write(&bad, "{ \"format\": \"something else\" }").unwrap();
    let output = b2c(&["check", bad.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("error[B2C-E01"), "{}", stdout(&output));
}

#[test]
fn check_json_is_versioned() {
    let output = b2c(&[
        "check",
        example("hello_world").to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["version"], 1);
    assert_eq!(report["ok"], true);
    assert!(report["diagnostics"].as_array().unwrap().is_empty());
}

#[test]
fn generate_writes_cpp() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("generated");
    let output = b2c(&[
        "generate",
        example("hello_world").to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let cpp = std::fs::read_to_string(out.join("main.cpp")).unwrap();
    assert!(cpp.contains("int main()"));
    assert!(cpp.contains("Edit the blocks"));

    let export = dir.path().join("export");
    let output = b2c(&[
        "generate",
        example("hello_world").to_str().unwrap(),
        "--out",
        export.to_str().unwrap(),
        "--export",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert!(
        !std::fs::read_to_string(export.join("main.cpp"))
            .unwrap()
            .contains("Edit the blocks")
    );
}

#[test]
fn examples_are_canonically_formatted() {
    for entry in std::fs::read_dir(repo_root().join("examples")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "b2c") {
            let output = b2c(&["fmt", "--check", path.to_str().unwrap()]);
            assert_eq!(
                output.status.code(),
                Some(0),
                "{}: {}",
                path.display(),
                stdout(&output)
            );
        }
    }
}

#[test]
fn fmt_rewrites_and_then_passes_check() {
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("copy.b2c");
    let original = std::fs::read_to_string(example("hello_world")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&original).unwrap();
    // Compact JSON is valid but not canonical.
    std::fs::write(&copy, serde_json::to_string(&value).unwrap()).unwrap();
    let path = copy.to_str().unwrap();
    assert_eq!(b2c(&["fmt", "--check", path]).status.code(), Some(1));
    assert_eq!(b2c(&["fmt", path]).status.code(), Some(0));
    assert_eq!(b2c(&["fmt", "--check", path]).status.code(), Some(0));
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), original);
}

#[test]
fn migrate_prints_the_canonical_document() {
    let output = b2c(&["migrate", example("hello_world").to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stdout(&output),
        std::fs::read_to_string(example("hello_world")).unwrap()
    );
}
