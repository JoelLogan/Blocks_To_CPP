//! End-to-end golden tests over the example projects (spec §9.2).
//!
//! For every folder `tests/golden/<name>/` there is a project
//! `examples/<name>.b2c`. Each test checks every example and reports all
//! failures together:
//!
//! * the project goes through the whole front end without errors or
//!   warnings, and the generated C++ matches the checked-in copy
//!   (`tests/golden/<name>/<file>.cpp`); run with `B2C_UPDATE_GOLDEN=1` to
//!   rewrite those copies after an intended change, then review the diff;
//! * generating twice gives byte-identical output;
//! * the program builds with g++ (the debug configuration, so with run-time
//!   checks such as sanitizers where the compiler has them) and, run with
//!   `stdin.txt` as its input in a copy of its golden folder, prints exactly
//!   `stdout.txt` (or matches the regular expression in `stdout.regex`),
//!   exits with `exit_code.txt` and leaves the copy as it was. The run check
//!   is `common::run_data`'s, which `assignments.rs` shares. Without g++ this
//!   test is skipped, unless `B2C_REQUIRE_GXX` is set.

// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use b2c_build::{FrontendOptions, run_frontend};
use b2c_ir::Severity;
use b2c_ir::source_map::GeneratedProject;
use common::repo_root;
use common::run_data::{RunData, build_project};

/// One example with its golden folder.
struct Example {
    name: String,
    project: PathBuf,
    golden: PathBuf,
}

fn examples() -> Vec<Example> {
    let golden_root = repo_root().join("tests/golden");
    let mut examples: Vec<Example> = std::fs::read_dir(&golden_root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .map(|golden| {
            let name = golden.file_name().unwrap().to_str().unwrap().to_owned();
            let project = repo_root().join("examples").join(format!("{name}.b2c"));
            assert!(
                project.is_file(),
                "{} has no project {}",
                golden.display(),
                project.display()
            );
            Example {
                name,
                project,
                golden,
            }
        })
        .collect();
    examples.sort_by(|a, b| a.name.cmp(&b.name));
    assert!(examples.len() >= 10, "expected at least 10 golden examples");
    examples
}

fn update_requested() -> bool {
    std::env::var_os("B2C_UPDATE_GOLDEN").is_some()
}

/// Runs the front end and returns the generated project, or a description
/// of every diagnostic.
fn generate(example: &Example) -> Result<GeneratedProject, String> {
    let bytes = std::fs::read(&example.project).unwrap();
    let frontend = run_frontend(&bytes, &FrontendOptions::default());
    let reported: Vec<String> = frontend
        .diagnostics
        .iter()
        .filter(|d| d.severity != Severity::Info)
        .map(|d| format!("{} {:?}: {}", d.code.0, d.primary, d.message))
        .collect();
    match frontend.generated {
        Some(generated) if reported.is_empty() => Ok(generated),
        _ => Err(format!("{} diagnostics: {reported:#?}", example.name)),
    }
}

/// Panics with every collected failure, if any.
fn finish(failures: &[String]) {
    if !failures.is_empty() {
        let mut message = format!("{} golden failure(s):\n", failures.len());
        for failure in failures {
            let _ = writeln!(message, "--- {failure}");
        }
        panic!("{message}");
    }
}

#[test]
fn examples_generate_the_expected_cpp() {
    let mut failures = Vec::new();
    for example in examples() {
        let generated = match generate(&example) {
            Ok(generated) => generated,
            Err(failure) => {
                failures.push(failure);
                continue;
            }
        };
        for file in &generated.files {
            let expected_path = example.golden.join(&file.path);
            if update_requested() {
                std::fs::write(&expected_path, &file.contents).unwrap();
                continue;
            }
            match std::fs::read_to_string(&expected_path) {
                Ok(expected) if expected == file.contents => {}
                Ok(_) => failures.push(format!(
                    "{}: generated {} differs from {} (run with B2C_UPDATE_GOLDEN=1 and review the diff)",
                    example.name,
                    file.path,
                    expected_path.display()
                )),
                Err(error) => failures.push(format!(
                    "{}: cannot read {}: {error} (run with B2C_UPDATE_GOLDEN=1 to create it)",
                    example.name,
                    expected_path.display()
                )),
            }
        }
    }
    finish(&failures);
}

#[test]
fn generation_is_deterministic() {
    let mut failures = Vec::new();
    for example in examples() {
        match (generate(&example), generate(&example)) {
            (Ok(first), Ok(second)) if first == second => {}
            (Ok(_), Ok(_)) => failures.push(format!("{}: two runs generated different output", example.name)),
            (Err(failure), _) | (_, Err(failure)) => failures.push(failure),
        }
    }
    finish(&failures);
}

/// Builds and runs one example, describing any mismatch.
fn check_run(example: &Example, cache: &Path) -> Result<(), String> {
    let name = &example.name;
    let data = RunData::read(&example.golden)
        .map_err(|problem| format!("{name}: tests/golden/{name}/ {problem}"))?;
    build_project(&example.project, cache)
        .and_then(|executable| data.check(&executable))
        .map_err(|problem| format!("{name}: {problem}"))
}

#[test]
fn examples_run_with_the_expected_output() {
    if common::gxx().is_none() {
        return;
    }
    // One cache for all examples, so the compiler is probed once.
    let cache = tempfile::tempdir().unwrap();
    let failures: Vec<String> = examples()
        .iter()
        .filter_map(|example| check_run(example, cache.path()).err())
        .collect();
    finish(&failures);
}
