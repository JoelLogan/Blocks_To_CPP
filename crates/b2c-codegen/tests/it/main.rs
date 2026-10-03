//! Integration tests for b2c-codegen: snapshots of the generated C++ (each
//! one also compiled with g++), the support helpers at run time, encoder round
//! trips, an expression oracle, source maps, determinism and robustness.

// Test code: unwrap/expect/panic and printing to stderr are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod builder;
mod determinism;
mod examples;
mod gxx;
mod oracle;
mod robustness;
mod roundtrip;
mod runtime;
mod snapshots;
mod source_map;

use b2c_codegen::{CodegenOptions, generate};
use b2c_ir::sast::Program;
use b2c_ir::source_map::GeneratedProject;

/// Options with a fixed app version, so snapshots do not change with releases.
pub(crate) fn options(project: &str) -> CodegenOptions {
    CodegenOptions {
        project_name: project.to_owned(),
        app_version: String::from("1.0.0"),
        ..CodegenOptions::default()
    }
}

/// All files of a project as one text (one file: just its contents).
pub(crate) fn render(project: &GeneratedProject) -> String {
    if let [file] = project.files.as_slice() {
        return file.contents.clone();
    }
    project
        .files
        .iter()
        .map(|f| format!("==> {} <==\n{}", f.path, f.contents))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Checks the layout rules every generated file follows.
pub(crate) fn assert_style(project: &GeneratedProject) {
    for file in &project.files {
        let text = &file.contents;
        assert!(
            text.ends_with('\n') && !text.ends_with("\n\n"),
            "{}: must end with exactly one newline",
            file.path
        );
        assert!(
            !text.contains("\n\n\n"),
            "{}: at most one empty line in a row",
            file.path
        );
        assert!(!text.contains('\r'), "{}: only \\n line endings", file.path);
        for (index, line) in text.lines().enumerate() {
            assert_eq!(
                line,
                line.trim_end(),
                "{}:{}: trailing whitespace",
                file.path,
                index + 1
            );
        }
    }
}

/// Generates a program, checks the layout rules and the source map's basic
/// invariants, and returns the result.
pub(crate) fn generate_checked(program: &Program, options: &CodegenOptions) -> GeneratedProject {
    let project = generate(program, options);
    assert_style(&project);
    source_map::assert_well_formed(&project);
    project
}
