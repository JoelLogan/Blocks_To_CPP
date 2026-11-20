//! The nightly case of `docs/plans/m3-contracts/w3-modules.md` §5.3 item 8:
//! a generated project of 256 modules, the most a project may have, each
//! named with a 64-character stem, the longest a module name may give. The
//! app's build compiles its 256 units and links them (an argv of about
//! 26,000 characters on Windows, §2.7), and its export builds with `make`;
//! on Windows in MSYS2's shell, since the one recipe line is longer than
//! cmd.exe takes (§2.8). Both programs print 255.
//!
//! `#[ignore]`d in the normal run: the nightly job (`.github/workflows/
//! nightly.yml`, job `modules-256`) runs it with `--ignored`, on Linux and
//! Windows. It needs g++ and `make` on `PATH`; on Windows, `B2C_MSYS2` names
//! the MSYS2 installation (the folder holding `usr\bin\bash.exe`), whose
//! login shell finds UCRT64's g++ and MSYS2's `make`.

// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;
use std::time::Duration;

use b2c_build::BuildSessions;
use b2c_build::export::{self, ExportMode};
use b2c_ipc::dto::BuildOutcome;
use common::{bytes, diagnostics, job, outcome, ready, real_toolchain, sink, wait_finished};
use serde_json::{Value, json};

/// The modules of the project: the most a project may have.
const MODULES: usize = 256;

/// How long the app's build of 256 units may take (Windows runners are slow
/// to start g++).
const BUILD_TIMEOUT: Duration = Duration::from_mins(30);

/// A module name of 64 characters, the longest a stem may be.
fn module_name(index: usize) -> String {
    let name = format!("module_{index:03}_of_a_project_with_as_many_modules_as_a_project_may_have");
    assert!(name.len() >= 64);
    name[..64].to_owned()
}

/// `define work_<k>` returning `int`, shared: `work_1()` gives 1 and each
/// later one the one before it plus 1.
fn work(k: usize) -> Value {
    let value = if k == 1 {
        json!([{"num": "1"}])
    } else {
        json!([{"ref": format!("sym_work_{}", k - 1)}, {"op": "("}, {"op": ")"}, {"op": "+"}, {"num": "1"}])
    };
    json!({
        "id": format!("b_fn_{k}"), "type": "func.define", "v": 1, "x": 40, "y": 40,
        "fields": {"NAME": {"sym": format!("sym_work_{k}"), "name": format!("work_{k}")},
                   "RETURNS": "int", "SHARED": true},
        "extra": {"params": []},
        "statements": {"BODY": [
            {"id": format!("b_ret_{k}"), "type": "func.return", "v": 1, "inputs": {"VALUE": {"expr": value}}}
        ]}
    })
}

/// The project: the first module prints `work_255()`, and each other module
/// `k` shares `work_<k>`, which calls the one of the module before it.
fn project() -> Value {
    let last = MODULES - 1;
    let main = json!({
        "id": "b_main", "type": "program.main", "v": 1, "x": 40, "y": 40,
        "statements": {"BODY": [{
            "id": "b_print", "type": "io.print", "v": 1, "extra": {"itemCount": 1},
            "fields": {"NEWLINE": true, "SEP": "none", "STREAM": "out"},
            "inputs": {"ITEM0": {"expr": [{"ref": format!("sym_work_{last}")}, {"op": "("}, {"op": ")"}]}}
        }]}
    });
    let modules: Vec<Value> = (0..MODULES)
        .map(|index| {
            let block = if index == 0 { main.clone() } else { work(index) };
            json!({"id": format!("mod_{index}"), "name": module_name(index), "workspace": {"blocks": [block]}})
        })
        .collect();
    json!({
        "format": "blocks2cpp/project",
        "formatVersion": 1,
        "generator": {"app": "0.1.0", "catalog": "1.0.0"},
        "project": {"id": "prj_modules_256", "name": "256 modules", "language": {"standard": "c++20"}},
        "modules": modules
    })
}

/// A command for a program. Production code spawns processes only through
/// b2c-process; tests drive the tools directly.
#[allow(clippy::disallowed_methods)]
fn command(program: &Path) -> std::process::Command {
    std::process::Command::new(program)
}

/// Runs `program` and checks that it prints 255.
fn prints_255(program: &Path, what: &str) {
    let output = command(program).output().unwrap();
    assert!(
        output.status.success(),
        "{what}: {:?}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        "255",
        "{what}"
    );
}

/// `make` in `folder`: on Windows in MSYS2's login shell (UCRT64), elsewhere
/// the `make` on `PATH`.
fn make(folder: &Path) -> std::process::Output {
    if cfg!(windows) {
        let msys2 = std::env::var_os("B2C_MSYS2").expect("B2C_MSYS2 names the MSYS2 installation");
        let bash = Path::new(&msys2).join("usr").join("bin").join("bash.exe");
        command(&bash)
            .args(["-lc", "make"])
            .current_dir(folder)
            .env("MSYSTEM", "UCRT64")
            .env("CHERE_INVOKING", "1")
            .output()
            .unwrap()
    } else {
        command(Path::new("make")).arg("-C").arg(folder).output().unwrap()
    }
}

#[test]
#[ignore = "nightly: builds 256 units and an export of them (nightly.yml, job modules-256)"]
fn a_project_of_256_modules_builds_and_its_export_builds_with_make() {
    let toolchain = real_toolchain().expect("g++ on PATH");
    let document = bytes(&project());

    // The app's build: 256 units compiled, then linked.
    let cache = tempfile::tempdir().unwrap();
    let sessions = BuildSessions::new(cache.path().to_path_buf());
    let events = sink();
    let id = sessions
        .start(
            job("ph_modules_256", document.clone(), ready(toolchain)),
            events.clone(),
        )
        .unwrap();
    let events = wait_finished(&events, BUILD_TIMEOUT);
    assert!(sessions.wait_idle(Duration::from_mins(1)));
    assert_eq!(
        outcome(&events).0,
        BuildOutcome::Built,
        "{:#?}",
        diagnostics(&events)
    );
    let record = sessions.record(&id).unwrap();
    record.verify_executable().unwrap();
    let gen_dir = record.build_dir.as_ref().unwrap().join("gen");
    let sources = std::fs::read_dir(&gen_dir)
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|e| e == "cpp")
        })
        .count();
    assert!(sources >= MODULES, "{sources} sources");
    prints_255(record.executable.as_ref().unwrap(), "the app's build");

    // The export, built with `make` without Blocks2Cpp.
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("export");
    let export = export::prepare(&document, 4, "0.0.0-test").unwrap();
    let written = export::write(&folder, &export, ExportMode::New).unwrap();
    assert!(written.kept.is_empty());
    let readme = std::fs::read_to_string(folder.join("README.md")).unwrap();
    assert!(readme.contains("run `make` in an MSYS2 shell"), "{readme}");
    let output = make(&folder);
    assert!(
        output.status.success(),
        "make failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let program = folder.join(format!("{}-debug{}", export.name, std::env::consts::EXE_SUFFIX));
    prints_255(&program, "the export");
}
