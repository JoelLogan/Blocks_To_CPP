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

/// A block left loose on the canvas is parked (ADR-0012): `check` lists one
/// warning and exits with 0, and `generate` writes exactly the C++ of the
/// project without it (the golden `main.cpp`).
#[test]
fn a_parked_block_is_a_warning_and_changes_no_cpp() {
    let dir = tempfile::tempdir().unwrap();
    let mut project: serde_json::Value =
        serde_json::from_slice(&std::fs::read(example("hello_world")).unwrap()).unwrap();
    project["modules"][0]["workspace"]["blocks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": "b_parked", "type": "io.print", "v": 1, "x": 400, "y": 40,
            "inputs": {"ITEM0": {"expr": [{"str": "not run"}]}}
        }));
    let file = dir.path().join("parked.b2c");
    std::fs::write(&file, serde_json::to_vec(&project).unwrap()).unwrap();
    let file = file.to_str().unwrap();

    let output = b2c(&["check", file]);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    let text = stdout(&output);
    assert_eq!(text.matches("warning[B2C-W0505]").count(), 1, "{text}");
    assert_eq!(text.matches("[B2C-").count(), 1, "{text}");
    assert!(text.contains("1 warning\n"), "{text}");
    assert!(text.ends_with(": no errors\n"), "{text}");

    let output = b2c(&["check", file, "--format", "json"]);
    assert_eq!(output.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ok"], true);
    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "B2C-W0505");
    assert_eq!(diagnostics[0]["severity"], "warning");

    let out = dir.path().join("generated");
    let output = b2c(&["generate", file, "--out", out.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let golden = std::fs::read(repo_root().join("tests/golden/hello_world/main.cpp")).unwrap();
    assert_eq!(std::fs::read(out.join("main.cpp")).unwrap(), golden);
}

/// `examples/hello_world.b2c` with `change` applied, written to `dir`.
fn hello_world_with(dir: &Path, name: &str, change: impl FnOnce(&mut serde_json::Value)) -> PathBuf {
    let mut project: serde_json::Value =
        serde_json::from_slice(&std::fs::read(example("hello_world")).unwrap()).unwrap();
    change(&mut project);
    let file = dir.join(name);
    std::fs::write(&file, serde_json::to_vec(&project).unwrap()).unwrap();
    file
}

/// An `ask` straight from the toolbox, before a variable is chosen (the
/// editor saves it without the field `VAR`), is parked like any other
/// block: alone on the canvas, or below a `create` in a loose stack, it is
/// one warning and `check` exits with 0 (ADR-0012). Attached, the missing
/// variable is one error, reported once.
#[test]
fn a_parked_ask_without_a_variable_is_only_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let ask = serde_json::json!({
        "id": "b_ask", "type": "io.ask", "v": 1,
        "inputs": {"PROMPT": {"expr": [{"str": "Your answer: "}]}}
    });
    let mut loose = ask.clone();
    loose["x"] = 400.into();
    loose["y"] = 40.into();
    let mut stack = serde_json::json!({
        "id": "b_create", "type": "var.declare", "v": 1, "x": 400, "y": 200,
        "fields": {"TYPE": "int", "NAME": {"sym": "sym_guess", "name": "guess"}},
        "stack": [ask.clone()]
    });
    stack["stack"][0]["id"] = "b_ask2".into();
    for (name, block) in [("loose.b2c", loose), ("stack.b2c", stack)] {
        let file = hello_world_with(dir.path(), name, |project| {
            project["modules"][0]["workspace"]["blocks"]
                .as_array_mut()
                .unwrap()
                .push(block);
        });
        let output = b2c(&["check", file.to_str().unwrap()]);
        let text = stdout(&output);
        assert_eq!(output.status.code(), Some(0), "{name}: {text}");
        assert_eq!(text.matches("[B2C-").count(), 1, "{name}: {text}");
        assert!(text.contains("warning[B2C-W0505]"), "{name}: {text}");
    }

    let file = hello_world_with(dir.path(), "attached.b2c", |project| {
        project["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"]
            .as_array_mut()
            .unwrap()
            .push(ask);
    });
    let output = b2c(&["check", file.to_str().unwrap()]);
    let text = stdout(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert_eq!(text.matches("[B2C-").count(), 1, "{text}");
    assert!(
        text.contains("error[B2C-E0430]: Choose a variable in this block."),
        "{text}"
    );
    assert!(text.contains("1 error"), "{text}");
}

/// A problem the block catalog reports is not reported again by the
/// analyser: a dropdown value that is not an option is one error, not two
/// (06 §6.1).
#[test]
fn check_reports_each_problem_once() {
    let dir = tempfile::tempdir().unwrap();
    let file = hello_world_with(dir.path(), "bogus.b2c", |project| {
        project["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"][0]["fields"]["SEP"] =
            "bogus".into();
    });
    let output = b2c(&["check", file.to_str().unwrap()]);
    let text = stdout(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert_eq!(text.matches("[B2C-").count(), 1, "{text}");
    assert!(text.contains("error[B2C-E0607]"), "{text}");
}

/// `check` runs the analyser even when the catalog rejects a block, so it
/// lists the problems the app lists (06 §6.1).
#[test]
fn check_lists_catalog_and_analyser_problems_together() {
    let dir = tempfile::tempdir().unwrap();
    let mut project: serde_json::Value =
        serde_json::from_slice(&std::fs::read(example("hello_world")).unwrap()).unwrap();
    let body = project["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"]
        .as_array_mut()
        .unwrap();
    body.push(serde_json::json!({"id": "b_unknown", "type": "pack.unknown", "v": 1}));
    body.push(serde_json::json!({
        "id": "b_ghost", "type": "io.print", "v": 1,
        "inputs": {"ITEM0": {"expr": [{"ref": "sym_ghost"}]}}
    }));
    let file = dir.path().join("both.b2c");
    std::fs::write(&file, serde_json::to_vec(&project).unwrap()).unwrap();
    let output = b2c(&["check", file.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    let text = stdout(&output);
    assert!(text.contains("error[B2C-E0601]"), "{text}");
    assert!(text.contains("error[B2C-E0201]"), "{text}");
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

/// Every file below `dir` (recursively), with its contents.
fn files_below(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let contents = std::fs::read(&path).unwrap();
                files.push((path, contents));
            }
        }
    }
    files
}

/// Whether `haystack` contains `needle`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

/// The IDE init unit never reaches generated or exported code (07 §7.6.3).
#[test]
fn generate_and_export_never_contain_the_ide_init_unit() {
    let dir = tempfile::tempdir().unwrap();
    let project = example("hello_world");
    for (folder, export) in [("generated", false), ("export", true)] {
        let out = dir.path().join(folder);
        let mut args = vec![
            "generate",
            project.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ];
        if export {
            args.push("--export");
        }
        let output = b2c(&args);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        let files = files_below(&out);
        assert!(!files.is_empty());
        for (path, contents) in files {
            assert!(!path.to_string_lossy().contains("b2c_ide"), "{}", path.display());
            assert!(!contains(&contents, b"b2c_ide"), "{}", path.display());
            assert!(!contains(&contents, b"SetConsoleCP"), "{}", path.display());
        }
    }
}

/// `b2c build` never links the IDE init unit: no IDE file in its build
/// folder and none in any compiler command of its manifest, and a second
/// build is served from the cache.
#[test]
fn cli_builds_never_contain_the_ide_init_unit() {
    if !have_gxx() {
        return;
    }
    let cache = tempfile::tempdir().unwrap();
    let build = || {
        b2c(&[
            "build",
            example("hello_world").to_str().unwrap(),
            "--cache-dir",
            cache.path().to_str().unwrap(),
        ])
    };
    let output = build();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let executable = PathBuf::from(stdout(&output).trim_end());
    let built_at = std::fs::metadata(&executable).unwrap().modified().unwrap();
    let files = files_below(&cache.path().join("builds"));
    let manifests: Vec<&(PathBuf, Vec<u8>)> = files
        .iter()
        .filter(|(path, _)| path.ends_with("build-manifest.json"))
        .collect();
    assert_eq!(manifests.len(), 1, "{files:?}");
    let manifest: serde_json::Value = serde_json::from_slice(&manifests[0].1).unwrap();
    assert_eq!(manifest["ide"], false);
    assert_eq!(manifest["result"], "success");
    for (path, _) in &files {
        assert!(!path.to_string_lossy().contains("b2c_ide"), "{}", path.display());
    }
    assert!(!manifest["steps"].to_string().contains("b2c_ide"));

    std::thread::sleep(std::time::Duration::from_millis(20));
    let output = build();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(PathBuf::from(stdout(&output).trim_end()), executable);
    assert_eq!(
        std::fs::metadata(&executable).unwrap().modified().unwrap(),
        built_at,
        "an unchanged project is not compiled again"
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
fn migrate_in_place_keeps_a_backup_and_leaves_a_current_file_alone() {
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("copy.b2c");
    let original = std::fs::read_to_string(example("hello_world")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&original).unwrap();
    let compact = serde_json::to_string(&value).unwrap();
    std::fs::write(&copy, &compact).unwrap();
    // A path relative to the current folder, as people type it.
    let output = Command::new(env!("CARGO_BIN_EXE_b2c"))
        .args(["migrate", "--in-place", "copy.b2c"])
        .current_dir(dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), original);
    let backup = dir.path().join("copy.b2c.bak");
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), compact);
    // Already current: nothing is written, and the backup stays.
    std::fs::remove_file(&backup).unwrap();
    let output = b2c(&["migrate", "--in-place", copy.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(!backup.exists());
}

/// Every example is current: `migrate` prints it byte for byte as it is
/// (`primes` has *change* blocks saved without the menu `OP` that catalog
/// 1.2.0 added; they stay without it).
#[test]
fn migrate_leaves_every_example_as_it_is() {
    let mut seen = 0;
    for entry in std::fs::read_dir(repo_root().join("examples")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "b2c") {
            continue;
        }
        let output = b2c(&["migrate", path.to_str().unwrap()]);
        assert_eq!(output.status.code(), Some(0), "{}", path.display());
        assert_eq!(
            stdout(&output),
            std::fs::read_to_string(&path).unwrap(),
            "{}",
            path.display()
        );
        seen += 1;
    }
    assert!(seen > 10, "too few examples were found");
}

/// A file saved before catalog 1.2.0 with *update* blocks: `migrate`
/// writes them as *change* blocks (the golden file of the replacement),
/// names catalog 1.2.0 in `generator`, and leaves nothing to upgrade.
#[test]
fn migrate_turns_update_blocks_into_change_blocks() {
    let cases = repo_root().join("crates/b2c-catalog/tests/migrations");
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("old.b2c");
    std::fs::copy(cases.join("var_update.b2c"), &copy).unwrap();
    let path = copy.to_str().unwrap();
    let output = b2c(&["migrate", "--in-place", path]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let expected = std::fs::read_to_string(cases.join("var_update.upgraded.b2c"))
        .unwrap()
        .replacen("\"catalog\": \"1.1.0\"", "\"catalog\": \"1.2.0\"", 1);
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), expected);
    let backup = dir.path().join("old.b2c.bak");
    assert!(backup.exists());
    // Current now: nothing more is written, and it checks with warnings only
    // (its parked blocks).
    std::fs::remove_file(&backup).unwrap();
    assert_eq!(b2c(&["migrate", "--in-place", path]).status.code(), Some(0));
    assert!(!backup.exists());
    let output = b2c(&["check", path]);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
}

/// `hello_world` with its print block at version 0, which no catalog can
/// upgrade.
fn with_unupgradable_block(dir: &Path) -> (PathBuf, String) {
    let original = std::fs::read_to_string(example("hello_world")).unwrap();
    let old = original.replacen(
        "\"type\": \"io.print\",\n                  \"v\": 1",
        "\"type\": \"io.print\",\n                  \"v\": 0",
        1,
    );
    assert_ne!(old, original, "hello_world has no print block at that place");
    let path = dir.join("old.b2c");
    std::fs::write(&path, &old).unwrap();
    (path, old)
}

#[test]
fn migrate_writes_nothing_when_a_block_cannot_be_upgraded() {
    let dir = tempfile::tempdir().unwrap();
    let (path, old) = with_unupgradable_block(dir.path());
    for args in [&["migrate", "--in-place"][..], &["migrate"]] {
        let mut args = args.to_vec();
        args.push(path.to_str().unwrap());
        let output = b2c(&args);
        assert_eq!(output.status.code(), Some(1));
        assert!(stderr(&output).contains("B2C-E0603"), "{}", stderr(&output));
        assert!(
            stderr(&output).contains("has blocks that cannot be upgraded; nothing was written"),
            "{}",
            stderr(&output)
        );
        assert_eq!(stdout(&output), "");
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), old);
    assert!(!dir.path().join("old.b2c.bak").exists());
}

#[test]
fn fmt_never_upgrades_blocks_and_check_reports_them() {
    let dir = tempfile::tempdir().unwrap();
    let (path, old) = with_unupgradable_block(dir.path());
    let output = b2c(&["fmt", "--check", path.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert_eq!(b2c(&["fmt", path.to_str().unwrap()]).status.code(), Some(0));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), old);
    // check upgrades in memory, so the block that cannot be upgraded is a
    // problem there, and the file is not touched.
    let output = b2c(&["check", path.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("B2C-E0603"), "{}", stdout(&output));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), old);
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

    // `--out` never writes through a link.
    #[cfg(unix)]
    {
        let victim = cache.path().join("victim.txt");
        std::fs::write(&victim, "keep me").unwrap();
        let link = cache.path().join("link");
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        let output = b2c(&[
            "build",
            example("hello_world").to_str().unwrap(),
            "--cache-dir",
            cache.path().to_str().unwrap(),
            "--out",
            link.to_str().unwrap(),
        ]);
        assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep me");
    }
}

#[test]
fn a_relative_cache_folder_works() {
    if !have_gxx() {
        return;
    }
    let folder = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_b2c"))
        .args([
            "run",
            example("hello_world").to_str().unwrap(),
            "--cache-dir",
            "cache",
        ])
        .current_dir(folder.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "Hello, world!\n");
    assert!(folder.path().join("cache/builds").is_dir());
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
