//! W2's functions and globals run (package C2,
//! docs/plans/m3-contracts/w2.md §2.7, §2.8, §5.2): each program is made of
//! blocks as the editor saves them, goes through the whole front end
//! without a diagnostic, is compiled with the `g++` on `PATH` and every
//! `/usr/bin/g++-N` Blocks2Cpp supports under C++17 and C++20 at the
//! *strict* warning level with `-Werror`, and is run. Each compiler is
//! probed and run as builds run it: from its canonical path, through
//! b2c-process, in the environment `compiler_env` gives it.
//!
//! * default values of parameters: a call leaves out the last arguments,
//!   and the declaration alone shows the defaults;

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
}

impl Blocks {
    /// *define* `name` (symbol `s_<name>`) giving back `returns`, with
    /// parameters `(name, type, mode)` (symbols `s_<name>`), the default
    /// values of their rows (`None` for none) and `body`.
    fn define(
        &self,
        name: &str,
        returns: &str,
        params: &[(&str, &str, &str)],
        defaults: Vec<Option<Value>>,
        body: Vec<Value>,
    ) -> Value {
        let rows: Vec<Value> = params
            .iter()
            .map(|(param, ty, mode)| json!({ "sym": format!("s_{param}"), "name": param, "type": ty, "mode": mode }))
            .collect();
        let inputs: Map<String, Value> = defaults
            .into_iter()
            .enumerate()
            .filter_map(|(index, value)| value.map(|value| (format!("DEFAULT{index}"), value)))
            .collect();
        self.block(
            "func.define",
            &json!({ "x": 400, "y": 40, "fields": { "NAME": { "sym": format!("s_{name}"), "name": name }, "RETURNS": returns },
                     "extra": { "params": rows }, "inputs": inputs, "statements": { "BODY": body } }),
        )
    }

    /// The arguments of a call, as inputs `ARG<i>`, with their count.
    fn call_parts(name: &str, args: Vec<Value>) -> Value {
        let count = args.len();
        let inputs: Map<String, Value> = args
            .into_iter()
            .enumerate()
            .map(|(index, arg)| (format!("ARG{index}"), arg))
            .collect();
        json!({ "fields": { "FUNC": { "ref": format!("s_{name}") } }, "extra": { "argCount": count }, "inputs": inputs })
    }

    /// *call* `name` with these arguments, as a statement.
    fn call_stmt(&self, name: &str, args: Vec<Value>) -> Value {
        self.block("func.call_stmt", &Self::call_parts(name, args))
    }

    /// *call* `name` with these arguments, as a value (an input).
    fn call(&self, name: &str, args: Vec<Value>) -> Value {
        json!({ "block": self.block("func.call", &Self::call_parts(name, args)) })
    }

    /// *return* `value`.
    fn ret(&self, value: Value) -> Value {
        self.block("func.return", &json!({ "inputs": { "VALUE": value } }))
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
        "project": { "id": "prj_functions_globals", "name": "Functions and globals", "language": { "standard": standard } },
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

/// Generates `main` with `body` and these top-level blocks beside it for
/// both standards, builds it with every compiler, and runs it in a fresh
/// folder holding `files`; checks that it gives `expected` and leaves the
/// folder as it was. `warnings` are the codes of the warnings the front end
/// must report (and nothing else); a warning changes nothing g++ accepts at
/// the *strict* level with `-Werror`. Returns the C++ generated under C++20.
fn builds_and_runs(
    name: &str,
    top: &[Value],
    body: Vec<Value>,
    files: &[(&str, &str)],
    warnings: &[&str],
    expected: &Ran,
) -> String {
    let main = json!({
        "id": "main", "type": "program.main", "v": 1, "x": 40, "y": 40, "statements": { "BODY": Value::Array(body) },
    });
    let blocks: Vec<Value> = std::iter::once(main).chain(top.iter().cloned()).collect();
    let compilers = compilers();
    let build = tempfile::tempdir().unwrap();
    let mut cpp20 = String::new();
    for (standard, flag) in [
        (CppStandard::Cpp17, "-std=c++17"),
        (CppStandard::Cpp20, "-std=c++20"),
    ] {
        let frontend = run_frontend(&project(standard, &blocks), &FrontendOptions::default());
        let reported: Vec<String> = frontend
            .diagnostics
            .iter()
            .filter(|d| d.severity != Severity::Info)
            .map(|d| format!("{} {:?}: {}", d.code.0, d.primary, d.message))
            .collect();
        let codes: Vec<&str> = frontend
            .diagnostics
            .iter()
            .filter(|d| d.severity != Severity::Info)
            .map(|d| d.code.0.as_str())
            .collect();
        assert_eq!(codes, warnings, "{name} ({flag}): {reported:#?}");
        assert!(
            frontend.diagnostics.iter().all(|d| d.severity != Severity::Error),
            "{name} ({flag}): {reported:#?}"
        );
        let generated = frontend.generated.unwrap();
        let [file] = generated.files.as_slice() else {
            panic!("{name}: one file expected");
        };
        if standard == CppStandard::Cpp20 {
            cpp20.clone_from(&file.contents);
        }
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
    cpp20
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
fn a_call_leaves_out_the_arguments_with_a_default_value() {
    let b = Blocks::default();
    // `line(width, fill = '*', end = "|")`, printing `width` fills then the
    // end, each on a line, and `area(w, h = w)` is not allowed (a default does not see the
    // parameters), so `square(side = 3)` calls `area`.
    let line = b.define(
        "line",
        "void",
        &[
            ("width", "int", "copy"),
            ("fill", "char", "copy"),
            ("end", "std::string", "read_only"),
        ],
        vec![None, Some(letter("*")), Some(text("|"))],
        vec![
            b.block(
                "control.repeat",
                &json!({ "inputs": { "TIMES": tokens(&["width"]) },
                         "statements": { "BODY": [b.print_to("out", vec![tokens(&["fill"])])] } }),
            ),
            b.print(vec![tokens(&["end"])]),
        ],
    );
    let area = b.define(
        "area",
        "int",
        &[("w", "int", "copy"), ("h", "int", "copy")],
        vec![None, None],
        vec![b.ret(tokens(&["w", "*", "h"]))],
    );
    let square = b.define(
        "square",
        "int",
        &[("side", "int", "copy")],
        vec![Some(b.call("area", vec![number("2"), number("2")]))],
        vec![b.ret(tokens(&["side", "*", "side"]))],
    );
    let body = vec![
        b.call_stmt("line", vec![number("3")]),
        b.call_stmt("line", vec![number("2"), letter("-")]),
        b.call_stmt("line", vec![number("1"), letter("="), text("!")]),
        b.print(vec![b.call("square", vec![])]),
        b.print(vec![b.call("square", vec![number("5")])]),
    ];
    let cpp = builds_and_runs(
        "defaults",
        &[line, area, square],
        body,
        &[],
        &[],
        &printed("*\n*\n*\n|\n-\n-\n|\n=\n!\n16\n25\n"),
    );
    // The defaults are in the declarations only; `area` is declared before
    // `square`, whose default calls it.
    assert!(cpp.contains("int area(int w, int h);\nvoid line(int width, char fill = '*', const std::string& end = \"|\");\nint square(int side = area(2, 2));\n"), "{cpp}");
    assert!(
        cpp.contains("\nvoid line(int width, char fill, const std::string& end) {"),
        "{cpp}"
    );
    assert!(cpp.contains("\nint square(int side) {"), "{cpp}");
}
