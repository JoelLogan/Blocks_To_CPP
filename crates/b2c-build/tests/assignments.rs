//! The M3 acceptance tests: the intro-CS assignments of decision M3-D1
//! (`docs/plans/next-steps.md` §3.3), listed with their status in
//! `tests/assignments/status.toml` (see `tests/assignments/README.md`).
//!
//! * A **pending** assignment's folder, `tests/assignments/<name>/`, holds
//!   the assignment (`README.md`), a reference solution (`reference.cpp`)
//!   and its run data as the golden tests have it: `stdin.txt`,
//!   `stdout.txt` or `stdout.regex`, `exit_code.txt` and any input files.
//!   The reference solution is built with g++ as the debug configuration
//!   builds generated code, with warnings as errors, and run in that folder:
//!   it must give the expected output and exit code, which checks the run
//!   data until the blocks exist. Once `examples/<name>.b2c` exists, it is
//!   built and run the same way as a golden example; when it passes, this
//!   test fails with "now passes: mark it green", so progress is never
//!   silent.
//! * A **green** assignment has its block project `examples/<name>.b2c` and
//!   its run data in `tests/golden/<name>/`, where `golden.rs` also checks
//!   the generated C++; its folder here keeps only `README.md` and, if it
//!   has one, `reference.cpp`. The project must pass the golden run check,
//!   and so must the reference solution.
//!
//! The run check prints a progress line, such as "M3 assignments: 1 of 10
//! green", even when the test passes. Without g++ it is skipped, unless
//! `B2C_REQUIRE_GXX` is set; the layout checks run everywhere.

// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use common::repo_root;
use common::run_data::{RunData, build_cpp, build_project};

/// Where an assignment stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    /// Only the reference solution exists, or the block project does not
    /// pass yet.
    Pending,
    /// The block project passes; its run data is a golden test's.
    Green,
}

impl Status {
    /// The word in `status.toml` and in the README's index.
    fn word(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Green => "green",
        }
    }
}

/// One assignment of `status.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Assignment {
    name: String,
    status: Status,
}

impl Assignment {
    /// `tests/assignments/<name>/`.
    fn folder(&self) -> PathBuf {
        assignments_root().join(&self.name)
    }

    /// `examples/<name>.b2c`, the block project.
    fn project(&self) -> PathBuf {
        repo_root().join("examples").join(format!("{}.b2c", self.name))
    }

    /// `tests/golden/<name>/`.
    fn golden(&self) -> PathBuf {
        repo_root().join("tests/golden").join(&self.name)
    }

    /// The folder with the run data: this assignment's own while it is
    /// pending, the golden test's once it is green.
    fn run_folder(&self) -> PathBuf {
        match self.status {
            Status::Pending => self.folder(),
            Status::Green => self.golden(),
        }
    }

    /// `reference.cpp`, the hand-written solution.
    fn reference(&self) -> PathBuf {
        self.folder().join("reference.cpp")
    }
}

fn assignments_root() -> PathBuf {
    repo_root().join("tests/assignments")
}

/// The assignments of `status.toml`, in its order.
fn assignments() -> Vec<Assignment> {
    let path = assignments_root().join("status.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    parse_status(&text).unwrap_or_else(|problem| panic!("{}: {problem}", path.display()))
}

/// Reads `status.toml`: comments, blank lines, the `[assignments]` header
/// and then one `name = "pending"` or `name = "green"` line per assignment,
/// a comment allowed after the value. The file is TOML, read by hand to
/// this narrow form, so anything else is an error rather than a guess.
fn parse_status(text: &str) -> Result<Vec<Assignment>, String> {
    let mut in_table = false;
    let mut assignments: Vec<Assignment> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[assignments]" && !in_table {
            in_table = true;
            continue;
        }
        if !in_table {
            return Err(format!("line {number}: expected the [assignments] header first"));
        }
        let Some((name, value)) = line.split_once('=') else {
            return Err(format!(
                "line {number}: expected name = \"pending\" or name = \"green\""
            ));
        };
        let name = name.trim();
        let value = value
            .split_once('#')
            .map_or(value, |(value, _comment)| value)
            .trim();
        let status = match value {
            "\"pending\"" => Status::Pending,
            "\"green\"" => Status::Green,
            other => {
                return Err(format!(
                    "line {number}: {other} is neither \"pending\" nor \"green\""
                ));
            }
        };
        let snake_case = !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
        if !snake_case {
            return Err(format!("line {number}: `{name}` is not a snake_case name"));
        }
        if assignments.iter().any(|assignment| assignment.name == name) {
            return Err(format!("line {number}: `{name}` is listed twice"));
        }
        assignments.push(Assignment {
            name: name.to_owned(),
            status,
        });
    }
    if assignments.is_empty() {
        return Err("lists no assignment".to_owned());
    }
    Ok(assignments)
}

/// Panics with every collected failure, if any.
fn finish(failures: &[String]) {
    if !failures.is_empty() {
        let mut message = format!("{} assignment failure(s):\n", failures.len());
        for failure in failures {
            let _ = writeln!(message, "--- {failure}");
        }
        panic!("{message}");
    }
}

/// The names of the folders in `folder`.
fn subfolders(folder: &Path) -> BTreeSet<String> {
    std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .map(|path| path.file_name().unwrap().to_str().unwrap().to_owned())
        .collect()
}

/// What is wrong with where one assignment's files are, for its status.
fn layout_problems(assignment: &Assignment) -> Vec<String> {
    let name = &assignment.name;
    let folder = assignment.folder();
    let mut problems = Vec::new();
    if !folder.join("README.md").is_file() {
        problems.push(format!("{name}: tests/assignments/{name}/ has no README.md"));
    }
    match assignment.status {
        Status::Pending => {
            if !assignment.reference().is_file() {
                problems.push(format!("{name}: is pending but has no reference.cpp"));
            }
            if let Err(problem) = RunData::read(&folder) {
                problems.push(format!("{name}: tests/assignments/{name}/ {problem}"));
            }
            if assignment.golden().exists() {
                problems.push(format!(
                    "{name}: is pending but tests/golden/{name}/ exists; mark it green or move the run data back"
                ));
            }
        }
        Status::Green => {
            if !assignment.project().is_file() {
                problems.push(format!("{name}: is green but examples/{name}.b2c does not exist"));
            }
            if let Err(problem) = RunData::read(&assignment.golden()) {
                problems.push(format!("{name}: is green but tests/golden/{name}/ {problem}"));
            }
            // The run data has moved; a copy left here could drift.
            let files: BTreeSet<String> = std::fs::read_dir(&folder)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            for file in files {
                if file != "README.md" && file != "reference.cpp" {
                    problems.push(format!(
                        "{name}: is green, so {file} belongs in tests/golden/{name}/, not tests/assignments/{name}/"
                    ));
                }
            }
        }
    }
    problems
}

/// Whether the README's index has the assignment's row with its status in
/// the last column.
fn index_problem(index: &str, assignment: &Assignment) -> Option<String> {
    let name = &assignment.name;
    let start = format!("| [`{name}`]({name}/README.md) |");
    let status = format!("| {} |", assignment.status.word());
    let row = index.lines().find(|line| line.starts_with(&start));
    match row {
        Some(row) if row.trim_end().ends_with(&status) => None,
        Some(_) => Some(format!(
            "{name}: its row in tests/assignments/README.md does not end with `{status}`"
        )),
        None => Some(format!(
            "{name}: tests/assignments/README.md has no index row starting with `{start}`"
        )),
    }
}

#[test]
fn status_toml_is_read_strictly() {
    let good = "# comment\n\n[assignments]\na_1 = \"pending\"\nb = \"green\" # since W2\n";
    let parsed = parse_status(good).unwrap();
    assert_eq!(
        parsed,
        [
            Assignment {
                name: "a_1".to_owned(),
                status: Status::Pending
            },
            Assignment {
                name: "b".to_owned(),
                status: Status::Green
            },
        ]
    );
    for (bad, expected) in [
        ("a = \"pending\"\n", "header first"),
        ("[assignments]\na = \"done\"\n", "neither"),
        ("[assignments]\na = pending\n", "neither"),
        ("[assignments]\na\n", "expected name"),
        ("[assignments]\nA-b = \"green\"\n", "snake_case"),
        ("[assignments]\na = \"green\"\na = \"pending\"\n", "twice"),
        ("[assignments]\n[assignments]\n", "expected name"),
        ("# nothing\n[assignments]\n", "no assignment"),
    ] {
        let problem = parse_status(bad).unwrap_err();
        assert!(problem.contains(expected), "{bad:?}: {problem}");
    }
}

#[test]
fn assignments_are_laid_out_as_their_status_says() {
    let assignments = assignments();
    let mut failures = Vec::new();
    let listed: BTreeSet<String> = assignments.iter().map(|a| a.name.clone()).collect();
    for folder in subfolders(&assignments_root()).difference(&listed) {
        failures.push(format!(
            "tests/assignments/{folder}/ is not listed in tests/assignments/status.toml"
        ));
    }
    let index = std::fs::read_to_string(assignments_root().join("README.md")).unwrap();
    for assignment in &assignments {
        if !assignment.folder().is_dir() {
            let name = &assignment.name;
            failures.push(format!("{name}: tests/assignments/{name}/ does not exist"));
            continue;
        }
        failures.extend(layout_problems(assignment));
        failures.extend(index_problem(&index, assignment));
    }
    finish(&failures);
}

/// What the run check found for one assignment.
#[derive(Default)]
struct Checked {
    failures: Vec<String>,
    /// A pending assignment's block project that does not pass yet.
    not_yet: Option<String>,
}

/// Builds and runs the reference solution and the block project, as the
/// assignment's status asks.
fn check(assignment: &Assignment, work: &Path) -> Checked {
    let name = &assignment.name;
    let mut checked = Checked::default();
    let data = match RunData::read(&assignment.run_folder()) {
        Ok(data) => data,
        Err(problem) => {
            checked.failures.push(format!("{name}: run data: {problem}"));
            return checked;
        }
    };
    if assignment.reference().is_file() {
        let out = work.join("references").join(name);
        std::fs::create_dir_all(&out).unwrap();
        if let Err(problem) =
            build_cpp(&assignment.reference(), &out).and_then(|program| data.check(&program))
        {
            checked.failures.push(format!("{name}: reference.cpp: {problem}"));
        }
    }
    let project = assignment.project();
    let cache = work.join("cache");
    match assignment.status {
        Status::Green => {
            if let Err(problem) = build_project(&project, &cache).and_then(|program| data.check(&program)) {
                checked
                    .failures
                    .push(format!("{name}: examples/{name}.b2c: {problem}"));
            }
        }
        Status::Pending if project.is_file() => {
            match build_project(&project, &cache).and_then(|program| data.check(&program)) {
                Ok(()) => checked.failures.push(format!(
                    "{name}: examples/{name}.b2c now passes: mark it green (tests/assignments/README.md says how)"
                )),
                Err(problem) => {
                    let first = problem.lines().next().unwrap_or_default();
                    checked.not_yet = Some(format!("{name}: examples/{name}.b2c does not pass yet: {first}"));
                }
            }
        }
        Status::Pending => {}
    }
    checked
}

#[test]
fn assignments_pass_their_run_check() {
    if common::gxx().is_none() {
        return;
    }
    let assignments = assignments();
    // One folder for the reference programs and one build cache for the
    // projects, so the compiler is probed once for each.
    let work = tempfile::tempdir().unwrap();
    let mut failures = Vec::new();
    let mut not_yet = Vec::new();
    // Green assignments that passed every check.
    let mut green = 0;
    for assignment in &assignments {
        let checked = check(assignment, work.path());
        if assignment.status == Status::Green && checked.failures.is_empty() {
            green += 1;
        }
        failures.extend(checked.failures);
        not_yet.extend(checked.not_yet);
    }
    // Written to the real standard error, which the test harness does not
    // capture, so the progress shows in every run.
    let mut progress = format!("M3 assignments: {green} of {} green", assignments.len());
    if !not_yet.is_empty() {
        let _ = write!(progress, "; {} pending with a block project", not_yet.len());
    }
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "{progress}");
    for line in &not_yet {
        let _ = writeln!(stderr, "  {line}");
    }
    drop(stderr);
    finish(&failures);
}
