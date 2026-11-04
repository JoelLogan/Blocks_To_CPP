//! End-to-end golden tests over the example projects (spec §9.2).
//!
//! For every folder `tests/golden/<name>/` there is a project
//! `examples/<name>.b2c`. Each test checks every example and reports all
//! failures together:
//!
//! * the project goes through the whole front end without errors or
//!   warnings, and every file it generates matches the checked-in copy:
//!   `tests/golden/<name>/main.cpp` for a single-module project; for a
//!   project of several modules each module's `<stem>.cpp`, its `<stem>.hpp`
//!   when it shares something, and `b2c_support.hpp` when a file uses a
//!   helper (w3-modules.md §1.7). Run with `B2C_UPDATE_GOLDEN=1` to rewrite
//!   those copies after an intended change, then review the diff;
//! * the folder holds no other `.cpp` or `.hpp` (run data never ends so): a
//!   file the generator no longer writes, such as the `.cpp` of a renamed
//!   module, fails the test, and `B2C_UPDATE_GOLDEN=1` deletes it;
//! * each generated header compiles alone, as the only line of a unit
//!   (`#include "<stem>.hpp"`), with the debug configuration's flags and
//!   warnings as errors (modules.md §12), so a header never relies on what
//!   a `.cpp` includes before it;
//! * generating twice gives byte-identical output;
//! * the program builds with g++ (the debug configuration, so with run-time
//!   checks such as sanitizers where the compiler has them) and, run with
//!   `stdin.txt` as its input in a copy of its golden folder, prints exactly
//!   `stdout.txt` (or matches the regular expression in `stdout.regex`),
//!   exits with `exit_code.txt` and leaves the copy as it was. The run check
//!   is `common::run_data`'s, which `assignments.rs` shares. A project of
//!   several modules builds each `.cpp` as a unit of its own and links them,
//!   and any warning fails it as for one module (an unused function in a
//!   module's anonymous namespace is one, so every example uses each of its
//!   modules' functions that it does not share). Without g++ the checks that
//!   compile are skipped, unless `B2C_REQUIRE_GXX` is set.

// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use b2c_build::{FrontendOptions, run_frontend};
use b2c_ir::Severity;
use b2c_ir::source_map::{FileKind, GeneratedProject};
use b2c_model::{Configurations, Language};
use b2c_toolchain::command::{BuildInputs, CommandPlan};
use b2c_toolchain::env::{HostEnv, compiler_env};
use b2c_toolchain::flags::ExtraFlags;
use common::run_data::{RunData, build_project};
use common::{real_toolchain, repo_root};

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

/// The `.cpp` and `.hpp` files of a golden folder, by name.
fn golden_sources(folder: &Path) -> BTreeSet<String> {
    std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .filter(|path| path.extension().is_some_and(|ext| ext == "cpp" || ext == "hpp"))
        .map(|path| path.file_name().unwrap().to_str().unwrap().to_owned())
        .collect()
}

/// The `.cpp` and `.hpp` files of a golden folder that the generator does
/// not write: each one fails the test, or is deleted with
/// `B2C_UPDATE_GOLDEN=1`.
fn stale_files(example: &Example, generated: &GeneratedProject) -> Vec<String> {
    let written: BTreeSet<&str> = generated.files.iter().map(|file| file.path.as_str()).collect();
    golden_sources(&example.golden)
        .into_iter()
        .filter(|name| !written.contains(name.as_str()))
        .collect()
}

#[test]
fn golden_folders_hold_only_the_files_generated() {
    let mut failures = Vec::new();
    for example in examples() {
        let generated = match generate(&example) {
            Ok(generated) => generated,
            Err(failure) => {
                failures.push(failure);
                continue;
            }
        };
        for stale in stale_files(&example, &generated) {
            let path = example.golden.join(&stale);
            if update_requested() {
                std::fs::remove_file(&path).unwrap();
                continue;
            }
            failures.push(format!(
                "{}: {} is not generated any more (run with B2C_UPDATE_GOLDEN=1 to delete it)",
                example.name,
                path.display()
            ));
        }
    }
    finish(&failures);
}

/// A golden folder with a file the generator does not write fails, whatever
/// the file's kind among C++ files; run data does not.
#[test]
fn a_stale_cpp_or_hpp_in_a_golden_folder_is_found() {
    let example = examples()
        .into_iter()
        .find(|e| e.name == "unit_converter")
        .unwrap();
    let generated = generate(&example).unwrap();
    let copy = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(&example.golden).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, copy.path().join(path.file_name().unwrap())).unwrap();
    }
    let in_copy = Example {
        name: example.name.clone(),
        project: example.project.clone(),
        golden: copy.path().to_path_buf(),
    };
    assert_eq!(stale_files(&in_copy, &generated), Vec::<String>::new());
    // The files a renamed module leaves behind, and run data.
    for name in ["tools.cpp", "tools.hpp", "notes.txt", "stdout.regex"] {
        std::fs::write(copy.path().join(name), "// old\n").unwrap();
    }
    assert_eq!(stale_files(&in_copy, &generated), ["tools.cpp", "tools.hpp"]);
}

/// Compiles `#include "<header>"` alone, as a unit in `folder` (which holds
/// the generated files), with the debug configuration's flags for the
/// project's standard and warnings as errors.
fn compile_header_alone(folder: &Path, header: &str, language: Language) -> Result<(), String> {
    let toolchain = real_toolchain().expect("g++ on PATH");
    let mut configuration = Configurations::default().debug;
    configuration.warnings_as_errors = true;
    let extra = ExtraFlags::none();
    let inputs = BuildInputs::new(&toolchain, &configuration, language, folder.to_path_buf(), &extra);
    let plan = CommandPlan::new(&inputs).map_err(|problem| problem.message)?;
    let unit = folder.join(format!("{header}.unit.cpp"));
    std::fs::write(&unit, format!("#include \"{header}\"\n")).map_err(|error| error.to_string())?;
    let object = folder.join(format!("{header}.o"));
    let step = plan.compile(&unit, &object);
    let step = step.with_plain_diagnostics().unwrap_or(step);
    let env = compiler_env(
        toolchain.platform(),
        toolchain.bin_dir(),
        folder,
        &HostEnv::from_process(&[]),
    );
    let command = step
        .process_command(folder, &env, None)
        .map_err(|error| error.to_string())?;
    let captured = b2c_process::run_captured(&command).map_err(|error| error.to_string())?;
    if captured.status == b2c_process::ExitStatus::Exited(0) {
        Ok(())
    } else {
        Err(format!(
            "{header} does not compile alone ({:?}):\n{}",
            captured.status,
            String::from_utf8_lossy(&captured.stderr)
        ))
    }
}

#[test]
fn each_header_compiles_alone() {
    if common::gxx().is_none() {
        return;
    }
    let mut failures = Vec::new();
    let mut headers = 0;
    for example in examples() {
        let generated = match generate(&example) {
            Ok(generated) => generated,
            Err(failure) => {
                failures.push(failure);
                continue;
            }
        };
        if !generated.files.iter().any(|file| file.kind == FileKind::Header) {
            continue;
        }
        let document = b2c_model::load(&std::fs::read(&example.project).unwrap()).unwrap();
        let folder = tempfile::tempdir().unwrap();
        for file in &generated.files {
            std::fs::write(folder.path().join(&file.path), &file.contents).unwrap();
        }
        for file in generated
            .files
            .iter()
            .filter(|file| file.kind == FileKind::Header)
        {
            headers += 1;
            if let Err(problem) = compile_header_alone(folder.path(), &file.path, document.project.language) {
                failures.push(format!("{}: {problem}", example.name));
            }
        }
    }
    finish(&failures);
    assert!(headers >= 3, "the multi-module examples' headers: {headers}");
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

#[test]
fn a_file_saved_with_windows_line_endings_reads_the_same_lines() {
    // On Linux, `std::getline` splits at '\n' only, so each line of a file
    // saved with "\r\n" ends in '\r': word_count would count one character
    // too many per line and miss a wanted word at a line's end.
    if common::gxx().is_none() {
        return;
    }
    let golden = repo_root().join("tests/golden/word_count");
    let crlf = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(&golden).unwrap() {
        let path = entry.unwrap().path();
        let mut bytes = std::fs::read(&path).unwrap();
        if path.file_name().is_some_and(|name| name == "poem.txt") {
            let text = String::from_utf8(bytes).unwrap();
            assert!(!text.contains('\r'));
            bytes = text.replace('\n', "\r\n").into_bytes();
        }
        std::fs::write(crlf.path().join(path.file_name().unwrap()), bytes).unwrap();
    }
    let data = RunData::read(crlf.path()).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let project = repo_root().join("examples/word_count.b2c");
    let result = build_project(&project, cache.path()).and_then(|executable| data.check(&executable));
    assert_eq!(result, Ok(()));
}
