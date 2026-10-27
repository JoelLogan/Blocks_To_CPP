//! W2's control and file blocks run (package D,
//! docs/plans/m3-contracts/w2.md §5.2): each program is made of blocks as
//! the editor saves them, goes through the whole front end without a
//! diagnostic, is compiled with the `g++` on `PATH` and every
//! `/usr/bin/g++-N` Blocks2Cpp supports under C++17 and C++20 at the
//! *strict* warning level with `-Werror`, and is run in a folder of its own,
//! as a project runs in its project folder (decision M3-D3). Each compiler
//! is probed and run as builds run it: from its canonical path, through
//! b2c-process, in the environment `compiler_env` gives it.
//!
//! * *switch on* a whole number and a character: the case that matches
//!   runs, the default part when none does, and a case that goes on into
//!   the next one only when its blocks say so;
//! * *do … then repeat* runs its body once although its condition is false;
//! * *for each line in file* reads a file named by a relative path from the
//!   folder the program runs in, line by line (an empty line, a last line
//!   without a line break), also nested in another file loop; and when the
//!   file is missing, its *if it can't be opened* part runs, or without the
//!   part the loop runs for no line. No run changes its folder.

// Test code: unwrap/expect/panic and builders that take JSON by value are
// fine here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_pass_by_value
)]

mod common;

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use b2c_build::{FrontendOptions, run_frontend};
use b2c_ir::Severity;
use b2c_ir::sast::CppStandard;
use b2c_toolchain::env::{HostEnv, compiler_env};
use b2c_toolchain::probe::{MIN_GCC_MAJOR, ProbeOptions, Toolchain, probe};
use serde_json::{Map, Value, json};

/// Warning flags of the *strict* level (spec §7.4.2) with `-Werror`.
const STRICT: &[&str] = &[
    "-Wall",
    "-Wextra",
    "-Wpedantic",
    "-Werror",
    "-Wshadow",
    "-Wconversion",
    "-Wsign-conversion",
    "-Wold-style-cast",
    "-Wnon-virtual-dtor",
    "-Woverloaded-virtual",
    "-Wnull-dereference",
    "-Wdouble-promotion",
    "-Wformat=2",
    "-Wimplicit-fallthrough",
];

/// A command for a built test program, run in `dir`. Production code spawns
/// processes only through b2c-process; this test runs the programs it
/// builds directly.
#[allow(clippy::disallowed_methods)]
fn command(program: &Path, dir: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    command.current_dir(dir);
    command
}

/// The g++ on `PATH`, then every `/usr/bin/g++-N` Blocks2Cpp supports
/// (`MIN_GCC_MAJOR` and newer), each probed once per test binary.
fn compilers() -> &'static [Toolchain] {
    static PROBED: OnceLock<Vec<Toolchain>> = OnceLock::new();
    PROBED.get_or_init(|| {
        let Some(on_path) = common::real_toolchain() else {
            return Vec::new();
        };
        let mut found = vec![on_path];
        for major in MIN_GCC_MAJOR..=20 {
            let versioned = PathBuf::from(format!("/usr/bin/g++-{major}"));
            if versioned.exists() {
                let toolchain = probe(&versioned, &ProbeOptions::default()).unwrap();
                assert!(toolchain.is_usable(), "{:#?}", toolchain.problems);
                found.push(toolchain);
            }
        }
        found
    })
}

/// Compiles `source` into `program` with `toolchain` and `flags`, as a build
/// runs g++. Returns g++'s messages when it fails.
fn compile(
    toolchain: &Toolchain,
    flags: &[&str],
    source: &Path,
    program: &Path,
    dir: &Path,
) -> Result<(), String> {
    let env = compiler_env(
        toolchain.platform(),
        toolchain.bin_dir(),
        dir,
        &HostEnv::from_process(&[]),
    );
    let mut compile = b2c_process::Command::new(toolchain.path(), dir).map_err(|error| error.to_string())?;
    compile
        .args(flags.iter().copied())
        .arg(source)
        .arg("-o")
        .arg(program)
        .envs(env.vars.iter().cloned());
    let captured = b2c_process::run_captured(&compile).map_err(|error| error.to_string())?;
    if captured.status == b2c_process::ExitStatus::Exited(0) {
        Ok(())
    } else {
        Err(format!(
            "{:?}\n{}",
            captured.status,
            String::from_utf8_lossy(&captured.stderr)
        ))
    }
}

/// Makes block nodes as the editor saves them, each with a new ID.
#[derive(Default)]
struct Blocks {
    made: Cell<u32>,
}

impl Blocks {
    /// A `kind` block with these parts; the catalog fills in the rest when
    /// the project loads.
    fn block(&self, kind: &str, parts: &Value) -> Value {
        let made = self.made.get() + 1;
        self.made.set(made);
        let mut block = json!({ "id": format!("b{made:03}"), "type": kind, "v": 1 });
        for (key, value) in parts.as_object().unwrap() {
            block[key] = value.clone();
        }
        block
    }

    /// *print* of these values on one line, to the output or error stream.
    fn print_to(&self, stream: &str, items: Vec<Value>) -> Value {
        let inputs: Map<String, Value> = items
            .into_iter()
            .enumerate()
            .map(|(index, item)| (format!("ITEM{index}"), item))
            .collect();
        let count = inputs.len();
        self.block(
            "io.print",
            &json!({ "extra": { "itemCount": count }, "fields": { "STREAM": stream }, "inputs": inputs }),
        )
    }

    /// *print* of these values on one line.
    fn print(&self, items: Vec<Value>) -> Value {
        self.print_to("out", items)
    }

    /// *create* `name` (symbol `s_<name>`) of `ty` set to `value`.
    fn create(&self, ty: &str, name: &str, value: Value) -> Value {
        self.block(
            "var.declare",
            &json!({ "fields": { "TYPE": ty, "NAME": { "sym": format!("s_{name}"), "name": name } },
                     "inputs": { "VALUE": value } }),
        )
    }

    /// *change* `name` by 1.
    fn bump(&self, name: &str) -> Value {
        self.block(
            "var.change",
            &json!({ "fields": { "VAR": { "ref": format!("s_{name}") } }, "inputs": { "BY": number("1") } }),
        )
    }

    /// *switch on (value)* with these cases, and a default part when given.
    fn switch(&self, value: Value, cases: Vec<(Value, Vec<Value>)>, default: Option<Vec<Value>>) -> Value {
        let mut inputs = Map::new();
        let mut statements = Map::new();
        inputs.insert(String::from("VALUE"), value);
        let count = cases.len();
        for (index, (label, body)) in cases.into_iter().enumerate() {
            inputs.insert(format!("CASE{index}"), label);
            statements.insert(format!("DO{index}"), Value::Array(body));
        }
        let has_default = default.is_some();
        if let Some(body) = default {
            statements.insert(String::from("DEFAULT"), Value::Array(body));
        }
        self.block(
            "control.switch",
            &json!({ "extra": { "caseCount": count, "hasDefault": has_default },
                     "inputs": inputs, "statements": statements }),
        )
    }

    /// *for each line [name] in file (path)*, with its *if it can't be
    /// opened* part when `failed` is given.
    fn lines(&self, name: &str, path: &str, body: Vec<Value>, failed: Option<Vec<Value>>) -> Value {
        let mut statements = json!({ "BODY": body });
        if let Some(failed) = &failed {
            statements["FAILED"] = json!(failed);
        }
        self.block(
            "file.for_each_line",
            &json!({ "extra": { "hasFailed": failed.is_some() },
                     "fields": { "LINE": { "sym": format!("s_{name}"), "name": name } },
                     "inputs": { "PATH": text(path) }, "statements": statements }),
        )
    }
}

/// An expression input of one text literal.
fn text(value: &str) -> Value {
    json!({ "expr": [{ "str": value }] })
}

/// An expression input of one number literal.
fn number(value: &str) -> Value {
    json!({ "expr": [{ "num": value }] })
}

/// An expression input of one character literal.
fn letter(value: &str) -> Value {
    json!({ "expr": [{ "chr": value }] })
}

/// An expression input of these tokens: names are references to `s_<name>`,
/// the rest operators and numbers.
fn tokens(list: &[&str]) -> Value {
    let tokens: Vec<Value> = list
        .iter()
        .map(|token| {
            if token.chars().all(|c| c.is_ascii_alphabetic() || c == '_') {
                json!({ "ref": format!("s_{token}") })
            } else if token.chars().all(|c| c.is_ascii_digit()) {
                json!({ "num": token })
            } else {
                json!({ "op": token })
            }
        })
        .collect();
    json!({ "expr": tokens })
}

/// The project bytes of a module with these top-level blocks.
fn project(standard: CppStandard, blocks: &[Value]) -> Vec<u8> {
    let standard = match standard {
        CppStandard::Cpp17 => "c++17",
        _ => "c++20",
    };
    let document = json!({
        "format": "blocks2cpp/project",
        "formatVersion": 1,
        "generator": { "app": env!("CARGO_PKG_VERSION"), "catalog": b2c_catalog::CATALOG_VERSION },
        "project": { "id": "prj_control_files", "name": "Control and files", "language": { "standard": standard } },
        "modules": [{ "id": "mod_main", "name": "main", "workspace": { "blocks": blocks } }],
    });
    serde_json::to_vec_pretty(&document).unwrap()
}

/// What a run gives.
#[derive(Debug, PartialEq, Eq)]
struct Ran {
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

/// The files of `dir` and their contents.
fn listing(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let contents = std::fs::read(entry.path()).unwrap_or_default();
            (entry.file_name().to_string_lossy().into_owned(), contents)
        })
        .collect()
}

/// Generates `main` with `body` for both standards, builds it with every
/// compiler, and runs it in a fresh folder holding `files`; checks that it
/// gives `expected` and leaves the folder as it was.
fn runs_and_gives(name: &str, body: Vec<Value>, files: &[(&str, &str)], expected: &Ran) {
    let main = json!({
        "id": "main", "type": "program.main", "v": 1, "x": 40, "y": 40, "statements": { "BODY": Value::Array(body) },
    });
    let compilers = compilers();
    let build = tempfile::tempdir().unwrap();
    for (standard, flag) in [
        (CppStandard::Cpp17, "-std=c++17"),
        (CppStandard::Cpp20, "-std=c++20"),
    ] {
        let frontend = run_frontend(
            &project(standard, std::slice::from_ref(&main)),
            &FrontendOptions::default(),
        );
        let reported: Vec<String> = frontend
            .diagnostics
            .iter()
            .filter(|d| d.severity != Severity::Info)
            .map(|d| format!("{} {:?}: {}", d.code.0, d.primary, d.message))
            .collect();
        assert!(reported.is_empty(), "{name} ({flag}): {reported:#?}");
        let generated = frontend.generated.unwrap();
        let [file] = generated.files.as_slice() else {
            panic!("{name}: one file expected");
        };
        let source = build.path().join(format!("{name}.cpp"));
        std::fs::write(&source, &file.contents).unwrap();
        for (index, toolchain) in compilers.iter().enumerate() {
            let gxx = toolchain.path().display();
            let program = build.path().join(format!("{name}-{index}"));
            let flags: Vec<&str> = std::iter::once(flag).chain(STRICT.iter().copied()).collect();
            if let Err(messages) = compile(toolchain, &flags, &source, &program, build.path()) {
                panic!(
                    "{gxx} {flag} rejected {name}: {messages}\n--- source ---\n{}",
                    file.contents
                );
            }
            let folder = tempfile::tempdir().unwrap();
            for (file_name, contents) in files {
                std::fs::write(folder.path().join(file_name), contents).unwrap();
            }
            let before = listing(folder.path());
            let run = command(&program, folder.path()).output().unwrap();
            let ran = Ran {
                stdout: String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n"),
                stderr: String::from_utf8_lossy(&run.stderr).replace("\r\n", "\n"),
                code: run.status.code(),
            };
            assert_eq!(&ran, expected, "{name} ({flag}, {gxx})\n{}", file.contents);
            assert_eq!(listing(folder.path()), before, "{name} changed its folder");
        }
    }
}

/// A run that prints `stdout` and ends with exit code 0.
fn printed(stdout: &str) -> Ran {
    Ran {
        stdout: stdout.to_owned(),
        stderr: String::new(),
        code: Some(0),
    }
}

#[test]
fn switch_runs_the_matching_case_or_the_default_part() {
    let b = Blocks::default();
    let for_each = |range: &str, body: Vec<Value>| {
        b.block(
            "control.for_range",
            &json!({ "fields": { "VAR": { "sym": "s_n", "name": "n" }, "DIRECTION": "through" },
                     "inputs": { "FROM": number("1"), "TO": number(range) }, "statements": { "BODY": body } }),
        )
    };
    let body = vec![
        for_each(
            "4",
            vec![b.switch(
                tokens(&["n"]),
                vec![
                    (number("1"), vec![b.print(vec![text("one")])]),
                    (number("2"), vec![b.print(vec![text("two")])]),
                    (tokens(&["1", "+", "2"]), vec![b.print(vec![text("three")])]),
                ],
                Some(vec![b.print(vec![text("other")])]),
            )],
        ),
        b.create("char", "grade", letter("b")),
        b.switch(
            tokens(&["grade"]),
            vec![
                (letter("a"), vec![b.print(vec![text("top")])]),
                (letter("b"), vec![b.print(vec![text("good")])]),
            ],
            None,
        ),
        b.switch(
            tokens(&["grade"]),
            vec![(letter("z"), vec![b.print(vec![text("never")])])],
            None,
        ),
    ];
    runs_and_gives("switch", body, &[], &printed("one\ntwo\nthree\nother\ngood\n"));
}

#[test]
fn do_while_runs_its_body_once_before_its_condition() {
    let b = Blocks::default();
    let do_while = |mode: &str, cond: Value, body: Vec<Value>| {
        b.block(
            "control.do_while",
            &json!({ "fields": { "MODE": mode }, "inputs": { "COND": cond }, "statements": { "BODY": body } }),
        )
    };
    let body = vec![
        b.create("int", "count", number("10")),
        do_while(
            "while",
            tokens(&["count", "<", "3"]),
            vec![b.print(vec![tokens(&["count"])]), b.bump("count")],
        ),
        b.create("int", "rounds", number("0")),
        do_while("until", tokens(&["rounds", ">=", "3"]), vec![b.bump("rounds")]),
        b.print(vec![tokens(&["rounds"])]),
    ];
    runs_and_gives("do_while", body, &[], &printed("10\n3\n"));
}

#[test]
fn a_file_is_read_line_by_line_from_the_folder_the_program_runs_in() {
    let b = Blocks::default();
    let length = |name: &str| json!({ "block": b.block("text.length", &json!({ "inputs": { "TEXT": tokens(&[name]) } })) });
    let body = vec![
        b.create("int", "count", number("0")),
        b.lines(
            "line",
            "data.txt",
            vec![
                b.bump("count"),
                b.print(vec![
                    tokens(&["count"]),
                    text(": "),
                    tokens(&["line"]),
                    text(" ("),
                    length("line"),
                    text(")"),
                ]),
            ],
            Some(vec![b.print_to("err", vec![text("Cannot open data.txt")])]),
        ),
        // Nested: each line of names.txt reads letters.txt again.
        b.lines(
            "name",
            "names.txt",
            vec![b.lines(
                "letter",
                "letters.txt",
                vec![b.print(vec![tokens(&["name"]), text(" "), tokens(&["letter"])])],
                None,
            )],
            None,
        ),
    ];
    runs_and_gives(
        "lines",
        body,
        &[
            ("data.txt", "first\n\nthird line"),
            ("names.txt", "Ann\nBo\n"),
            ("letters.txt", "x\ny\n"),
        ],
        &printed("1: first (5)\n2:  (0)\n3: third line (10)\nAnn x\nAnn y\nBo x\nBo y\n"),
    );
}

#[test]
fn a_missing_file_runs_the_part_for_it_or_no_line() {
    let b = Blocks::default();
    let exit = b.block("program.exit", &json!({ "inputs": { "CODE": number("1") } }));
    let body = vec![
        b.lines(
            "line",
            "missing.txt",
            vec![b.print(vec![tokens(&["line"])])],
            None,
        ),
        b.print(vec![text("no line read")]),
        b.lines(
            "other",
            "missing.txt",
            vec![b.print(vec![tokens(&["other"])])],
            Some(vec![
                b.print_to("err", vec![text("Cannot open missing.txt")]),
                exit,
            ]),
        ),
        b.print(vec![text("not reached")]),
    ];
    runs_and_gives(
        "missing",
        body,
        &[],
        &Ran {
            stdout: String::from("no line read\n"),
            stderr: String::from("Cannot open missing.txt\n"),
            code: Some(1),
        },
    );
}
