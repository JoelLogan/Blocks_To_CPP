//! The M3 acceptance tests: the intro-CS assignments of decision M3-D1
//! (`docs/plans/next-steps.md` §3.3), listed with their status in
//! `tests/assignments/status.toml` (see `tests/assignments/README.md`).
//!
//! * A **pending** assignment's folder, `tests/assignments/<name>/`, holds
//!   the assignment (`README.md`), a reference solution (`reference.cpp`)
//!   and its run data as the golden tests have it: `stdin.txt`,
//!   `stdout.txt` or `stdout.regex`, `exit_code.txt` and any input files.
//!   The reference solution is built with g++ as the debug configuration
//!   builds generated code, with warnings as errors, and run on that data:
//!   it must give the expected output and exit code, which checks the run
//!   data until the blocks exist. While its block project is being built,
//!   it is kept here too, as `<name>.b2c`, and not in `examples/`: every
//!   file there must already pass the tests that read all examples, which
//!   want no diagnostic at all and the canonical form. The project is built
//!   and run as a golden example is; once it passes and is ready for
//!   `examples/`, this test fails with "now passes: mark it green", so
//!   progress is never silent.
//! * A **green** assignment has its block project `examples/<name>.b2c` and
//!   its run data in `tests/golden/<name>/`, where `golden.rs` builds and
//!   runs it and checks the generated C++. Its folder here keeps only
//!   `README.md` and, if it has one, `reference.cpp`, which must pass on the
//!   golden run data.
//!
//! Each assignment's README says where it stands in its opening "Status:"
//! sentence, which must match `status.toml`, as must the index of
//! `tests/assignments/README.md`.
//!
//! Every program runs through `common::run_data`, as in `golden.rs`: in a
//! copy of its run folder, which must be unchanged afterwards. When g++ is
//! there, the run check prints a progress line, such as "M3 assignments: 1
//! of 10 green", even when the test passes. Without g++ it is skipped,
//! unless `B2C_REQUIRE_GXX` is set; the layout checks run everywhere.

// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use b2c_build::{FrontendOptions, run_frontend};
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

    /// `tests/assignments/<name>/<name>.b2c`, the block project while the
    /// assignment is pending.
    fn draft(&self) -> PathBuf {
        self.folder().join(format!("{}.b2c", self.name))
    }

    /// `examples/<name>.b2c`, the block project once the assignment is
    /// green.
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
    match std::fs::read_to_string(folder.join("README.md")) {
        Ok(readme) => problems.extend(status_line_problem(&readme, assignment)),
        Err(_) => problems.push(format!("{name}: tests/assignments/{name}/ has no README.md")),
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
            if assignment.project().exists() {
                problems.push(format!(
                    "{name}: is pending but examples/{name}.b2c exists; the tests that read every example fail on \
                     a project that is not finished, so keep it at tests/assignments/{name}/{name}.b2c until this \
                     test says it passes"
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
            // The project and the run data have moved; a copy left here
            // could drift.
            let draft = format!("{name}.b2c");
            let files: BTreeSet<String> = std::fs::read_dir(&folder)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            for file in files {
                if file == "README.md" || file == "reference.cpp" {
                    continue;
                }
                let home = if file == draft {
                    "examples/".to_owned()
                } else {
                    format!("tests/golden/{name}/")
                };
                problems.push(format!(
                    "{name}: is green, so {file} belongs in {home}, not tests/assignments/{name}/"
                ));
            }
        }
    }
    problems
}

/// What is wrong with the "Status:" sentence of an assignment's own
/// README, if anything: its first word after "Status:" (where the text may
/// wrap) must be the assignment's status in `status.toml`, as in "Status:
/// pending, until …" or "Status: green since W2.".
fn status_line_problem(readme: &str, assignment: &Assignment) -> Option<String> {
    let name = &assignment.name;
    let at = format!("tests/assignments/{name}/README.md");
    let mut words = readme.split_whitespace();
    if !words.any(|word| word == "Status:") {
        return Some(format!("{name}: {at} has no \"Status:\" sentence"));
    }
    let said = words
        .next()
        .unwrap_or_default()
        .trim_end_matches(|c: char| !c.is_ascii_alphanumeric());
    let expected = assignment.status.word();
    (said != expected).then(|| {
        format!(
            "{name}: is {expected} in status.toml but its {at} says \"Status: {said}\"; \
             update that sentence (tests/assignments/README.md, \"Turning an assignment green\")"
        )
    })
}

/// One assignment row of the README's index.
#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexRow {
    /// Its number in the file, from 1.
    line: usize,
    name: String,
    /// The last column.
    status: String,
}

/// The assignment rows of the README's index, in order: every line that
/// starts with ``| [` ``, which must go on as ``| [`name`](name/README.md) |``
/// and end with ` | <status> |`.
fn index_rows(readme: &str) -> Result<Vec<IndexRow>, String> {
    let mut rows = Vec::new();
    for (index, line) in readme.lines().enumerate() {
        let number = index + 1;
        let Some(rest) = line.strip_prefix("| [`") else {
            continue;
        };
        let name = rest.split_once('`').map_or("", |(name, _)| name);
        let start = format!("| [`{name}`]({name}/README.md) |");
        if name.is_empty() || !line.starts_with(&start) {
            return Err(format!(
                "tests/assignments/README.md line {number}: an index row starts with ``| [`name`](name/README.md) |``"
            ));
        }
        let status = line
            .trim_end()
            .strip_suffix('|')
            .and_then(|row| row.rsplit_once('|'))
            .map_or("", |(_, last)| last.trim());
        rows.push(IndexRow {
            line: number,
            name: name.to_owned(),
            status: status.to_owned(),
        });
    }
    Ok(rows)
}

/// What is wrong with the README's index: it must have one row for each
/// assignment of `status.toml`, in the same order, with its status in the
/// last column, and no other row.
fn index_problems(readme: &str, assignments: &[Assignment]) -> Vec<String> {
    let rows = match index_rows(readme) {
        Ok(rows) => rows,
        Err(problem) => return vec![problem],
    };
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    for row in &rows {
        let at = format!("tests/assignments/README.md line {}", row.line);
        if !seen.insert(row.name.as_str()) {
            problems.push(format!("{at}: `{}` has a second index row", row.name));
            continue;
        }
        match assignments.iter().find(|assignment| assignment.name == row.name) {
            None => problems.push(format!(
                "{at}: `{}` has an index row but is not in status.toml",
                row.name
            )),
            Some(assignment) if assignment.status.word() != row.status => problems.push(format!(
                "{at}: `{}` is {} in status.toml but `{}` in the index",
                row.name,
                assignment.status.word(),
                row.status
            )),
            Some(_) => {}
        }
    }
    for assignment in assignments {
        if !seen.contains(assignment.name.as_str()) {
            problems.push(format!(
                "{}: tests/assignments/README.md has no index row for it",
                assignment.name
            ));
        }
    }
    let indexed: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
    let listed: Vec<&str> = assignments.iter().map(|a| a.name.as_str()).collect();
    if problems.is_empty() && indexed != listed {
        problems.push(format!(
            "the index of tests/assignments/README.md is not in status.toml's order:\n  index: {}\n  status.toml: {}",
            indexed.join(", "),
            listed.join(", ")
        ));
    }
    problems
}

/// Why a block project that passes its run is not ready for `examples/`
/// yet, if it is not: the tests that read every example also want no
/// diagnostic at all, notes included, and the canonical form that `b2c fmt`
/// writes.
fn not_ready_for_examples(project: &Path) -> Option<String> {
    let bytes = std::fs::read(project).unwrap();
    let frontend = run_frontend(&bytes, &FrontendOptions::default());
    if let Some(first) = frontend.diagnostics.first() {
        return Some(format!(
            "{} diagnostic(s), notes included; the first: {} {}",
            frontend.diagnostics.len(),
            first.code.0,
            first.message
        ));
    }
    match b2c_model::load(&bytes) {
        Ok(document) if b2c_model::to_canonical_json(&document).as_bytes() == bytes.as_slice() => None,
        Ok(_) => Some("it is not in canonical form (`b2c fmt` writes it)".to_owned()),
        Err(error) => Some(error.to_string()),
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
fn the_index_is_read_strictly() {
    let listed = parse_status("[assignments]\na = \"pending\"\nb = \"green\"\n").unwrap();
    let row = |name: &str, status: &str| format!("| [`{name}`]({name}/README.md) | x | W2 | {status} |\n");
    let header = "| Assignment | Concepts | Wave | Status |\n| --- | --- | --- | --- |\n";
    let good = format!("{header}{}{}", row("a", "pending"), row("b", "green"));
    assert_eq!(index_problems(&good, &listed), Vec::<String>::new());
    for (bad, expected) in [
        (
            format!("{header}{}", row("a", "pending")),
            "b: tests/assignments/README.md has no index row",
        ),
        (
            format!(
                "{header}{}{}{}",
                row("a", "pending"),
                row("b", "green"),
                row("c", "green")
            ),
            "`c` has an index row but is not in status.toml",
        ),
        (
            format!(
                "{header}{}{}{}",
                row("a", "pending"),
                row("b", "green"),
                row("a", "pending")
            ),
            "`a` has a second index row",
        ),
        (
            format!("{header}{}{}", row("b", "green"), row("a", "pending")),
            "not in status.toml's order",
        ),
        (
            format!("{header}{}{}", row("a", "green"), row("b", "green")),
            "`a` is pending in status.toml but `green` in the index",
        ),
        (
            format!(
                "{header}| [`a`](b/README.md) | x | W2 | pending |\n{}",
                row("b", "green")
            ),
            "line 3: an index row starts with",
        ),
    ] {
        let problems = index_problems(&bad, &listed).join("\n");
        assert!(problems.contains(expected), "{bad}\n{problems}");
    }
}

#[test]
fn the_status_sentence_is_read_strictly() {
    let pending = Assignment {
        name: "a".to_owned(),
        status: Status::Pending,
    };
    let green = Assignment {
        name: "a".to_owned(),
        status: Status::Green,
    };
    let page =
        |status: &str| format!("# A\n\nAn M3 acceptance assignment. Status: {status}\n\n## The assignment\n");
    assert_eq!(status_line_problem(&page("green since W2."), &green), None);
    assert_eq!(status_line_problem(&page("green since M2."), &green), None);
    assert_eq!(
        status_line_problem(&page("pending, until\nthe blocks arrive."), &pending),
        None
    );
    // The sentence may wrap right after "Status:".
    assert_eq!(
        status_line_problem("Status:\npending, until W2.\n", &pending),
        None
    );
    for (readme, assignment, expected) in [
        (
            page("pending, until\nthe blocks it needs arrive in wave W2."),
            &green,
            "a: is green in status.toml but its tests/assignments/a/README.md says \"Status: pending\"",
        ),
        (
            page("green since W2."),
            &pending,
            "a: is pending in status.toml but its tests/assignments/a/README.md says \"Status: green\"",
        ),
        (page("done."), &green, "says \"Status: done\""),
        (page(""), &green, "says \"Status: \""),
        (
            "# A\n\nAn M3 acceptance assignment.\n".to_owned(),
            &green,
            "has no \"Status:\" sentence",
        ),
    ] {
        let problem = status_line_problem(&readme, assignment).unwrap_or_default();
        assert!(problem.contains(expected), "{readme:?}: {problem}");
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
    let readme = std::fs::read_to_string(assignments_root().join("README.md")).unwrap();
    failures.extend(index_problems(&readme, &assignments));
    for assignment in &assignments {
        if !assignment.folder().is_dir() {
            let name = &assignment.name;
            failures.push(format!("{name}: tests/assignments/{name}/ does not exist"));
            continue;
        }
        failures.extend(layout_problems(assignment));
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

/// Builds and runs the reference solution and, while the assignment is
/// pending, its block project. A green assignment's project is
/// `golden.rs`'s to run.
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
    let draft = assignment.draft();
    if assignment.status == Status::Pending && draft.is_file() {
        let shown = format!("tests/assignments/{name}/{name}.b2c");
        match build_project(&draft, &work.join("cache")).and_then(|program| data.check(&program)) {
            Ok(()) => match not_ready_for_examples(&draft) {
                None => checked.failures.push(format!(
                    "{name}: {shown} now passes: mark it green (tests/assignments/README.md says how)"
                )),
                Some(reason) => {
                    checked.not_yet = Some(format!(
                        "{name}: {shown} passes its run but is not ready for examples/ yet: {reason}"
                    ));
                }
            },
            Err(problem) => {
                let first = problem.lines().next().unwrap_or_default();
                checked.not_yet = Some(format!("{name}: {shown} does not pass yet: {first}"));
            }
        }
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
    // Green assignments that passed every check here.
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
