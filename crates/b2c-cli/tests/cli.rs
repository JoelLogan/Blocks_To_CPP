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

/// Standard output as text. On Windows a program's output is in text mode
/// (each `\n` arrives as `\r\n`), which this undoes exactly.
fn stdout(output: &Output) -> String {
    text(&output.stdout)
}

fn text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes).into_owned();
    if cfg!(windows) {
        text.replace("\r\n", "\n")
    } else {
        text
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Whether a g++ is on PATH. Build and run tests are skipped without one,
/// unless `B2C_REQUIRE_GXX` is set (as in CI), when they fail instead.
fn have_gxx() -> bool {
    let found = Command::new("g++")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok();
    assert!(
        found || std::env::var_os("B2C_REQUIRE_GXX").is_none(),
        "B2C_REQUIRE_GXX is set but g++ was not found"
    );
    found
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
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
    assert_eq!(
        b2c(&["run", "x.b2c", "--timeout", "forever"]).status.code(),
        Some(2)
    );
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

#[test]
fn run_prints_output_and_returns_the_exit_code() {
    if !have_gxx() {
        return;
    }
    let cache = tempfile::tempdir().unwrap();
    let cache_dir = cache.path().to_str().unwrap();
    let output = b2c(&[
        "run",
        example("hello_world").to_str().unwrap(),
        "--cache-dir",
        cache_dir,
    ]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "Hello, world!\n");

    let output = b2c(&[
        "run",
        example("exit_code").to_str().unwrap(),
        "--cache-dir",
        cache_dir,
    ]);
    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
}

#[test]
fn run_feeds_stdin_from_a_file() {
    if !have_gxx() {
        return;
    }
    let cache = tempfile::tempdir().unwrap();
    let input = repo_root().join("tests/golden/greeting/stdin.txt");
    let output = b2c(&[
        "run",
        example("greeting").to_str().unwrap(),
        "--cache-dir",
        cache.path().to_str().unwrap(),
        "--stdin",
        input.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "What is your name? Hello, Ada Lovelace!\n");
}

#[test]
fn build_copies_the_executable() {
    if !have_gxx() {
        return;
    }
    let cache = tempfile::tempdir().unwrap();
    let out = cache
        .path()
        .join(if cfg!(windows) { "hello.exe" } else { "hello" });
    let output = b2c(&[
        "build",
        example("hello_world").to_str().unwrap(),
        "--cache-dir",
        cache.path().to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let run = Command::new(&out).output().unwrap();
    assert_eq!(text(&run.stdout), "Hello, world!\n");
}

#[test]
fn run_stops_a_program_at_the_timeout() {
    if !have_gxx() {
        return;
    }
    let cache = tempfile::tempdir().unwrap();
    let started = std::time::Instant::now();
    let output = b2c(&[
        "run",
        fixture("forever.b2c").to_str().unwrap(),
        "--cache-dir",
        cache.path().to_str().unwrap(),
        "--timeout",
        "1s",
    ]);
    assert_eq!(output.status.code(), Some(124), "{}", stderr(&output));
    assert!(started.elapsed() < std::time::Duration::from_mins(1));
}

#[test]
fn run_returns_125_when_the_project_cannot_build() {
    if !have_gxx() {
        return;
    }
    let cache = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.b2c");
    std::fs::write(&bad, "{}").unwrap();
    let output = b2c(&[
        "run",
        bad.to_str().unwrap(),
        "--cache-dir",
        cache.path().to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(125), "{}", stderr(&output));
}
