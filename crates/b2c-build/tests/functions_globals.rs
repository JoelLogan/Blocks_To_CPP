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
//! * overloads: three functions called `area` run and print each, the calls
//!   cast their arguments to their own function's inputs, and quoted text
//!   for an overload set with a `bool` one becomes a `std::string`, and a
//!   *list of* names its type;
//! * globals and constants: `constexpr` or `const`, a constant as a `case`
//!   label beside a switch on an enum, and a global set from a function
//!   printed after the global that function reads;
//! * *change*'s *add to the end* and bitwise forms, *flip*, and `main` with
//!   its arguments;

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

impl Blocks {
    /// *global* `name` (symbol `s_<name>`): a `variable` or `constant` of
    /// type `ty`, set to `value` when one is given.
    fn global(&self, kind: &str, ty: &str, name: &str, value: Option<Value>) -> Value {
        let mut parts = json!({ "x": 800, "y": 40, "fields": { "KIND": kind, "TYPE": ty,
                                "NAME": { "sym": format!("s_{name}"), "name": name } } });
        if let Some(value) = value {
            parts["inputs"] = json!({ "VALUE": value });
        }
        self.block("var.global", &parts)
    }

    /// *switch on (value)* with one case per `(label, statements)`.
    fn switch(&self, value: Value, cases: Vec<(Value, Vec<Value>)>) -> Value {
        let mut inputs = Map::new();
        let mut statements = Map::new();
        inputs.insert(String::from("VALUE"), value);
        let count = cases.len();
        for (index, (label, body)) in cases.into_iter().enumerate() {
            inputs.insert(format!("CASE{index}"), label);
            statements.insert(format!("DO{index}"), Value::Array(body));
        }
        self.block(
            "control.switch",
            &json!({ "extra": { "caseCount": count, "hasDefault": false },
                     "inputs": inputs, "statements": statements }),
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
    builds_and_runs_main(name, &json!({}), top, body, files, warnings, expected)
}

/// [`builds_and_runs`] with these fields on `main` (its arguments).
fn builds_and_runs_main(
    name: &str,
    fields: &Value,
    top: &[Value],
    body: Vec<Value>,
    files: &[(&str, &str)],
    warnings: &[&str],
    expected: &Ran,
) -> String {
    let main = json!({
        "id": "main", "type": "program.main", "v": 1, "x": 40, "y": 40, "fields": fields,
        "statements": { "BODY": Value::Array(body) },
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

#[test]
fn three_overloads_of_area_run_and_print_each() {
    let b = Blocks::default();
    let define = |sym: &str, params: &[(&str, &str, &str)], body: Vec<Value>| {
        let mut block = b.define("area", "void", params, vec![], body);
        block["fields"]["NAME"]["sym"] = json!(format!("s_{sym}"));
        block
    };
    let square = define(
        "area_square",
        &[("side", "int", "copy")],
        vec![b.print(vec![text("square "), tokens(&["side", "*", "side"])])],
    );
    let rectangle = define(
        "area_rect",
        &[("w", "int", "copy"), ("h", "int", "copy")],
        vec![b.print(vec![text("rectangle "), tokens(&["w", "*", "h"])])],
    );
    let real = define(
        "area_real",
        &[("x", "double", "copy"), ("y", "double", "copy")],
        vec![b.print(vec![text("real "), tokens(&["x", "*", "y"])])],
    );
    let call = |sym: &str, args: Vec<Value>| {
        let count = args.len();
        let inputs: Map<String, Value> = args
            .into_iter()
            .enumerate()
            .map(|(index, arg)| (format!("ARG{index}"), arg))
            .collect();
        b.block(
            "func.call_stmt",
            &json!({ "fields": { "FUNC": { "ref": format!("s_{sym}") } }, "extra": { "argCount": count },
                     "inputs": inputs }),
        )
    };
    let body = vec![
        b.block(
            "var.declare",
            &json!({ "fields": { "TYPE": "int", "NAME": { "sym": "s_n", "name": "n" } },
                     "inputs": { "VALUE": number("3") } }),
        ),
        call("area_square", vec![tokens(&["n"])]),
        call("area_rect", vec![number("2"), tokens(&["n"])]),
        // An int for the double overload: cast, so that the call means it.
        call("area_real", vec![tokens(&["n"]), number("1.5")]),
        // A function without overloads takes it as it is.
        call("half", vec![tokens(&["n"])]),
    ];
    let half = b.define(
        "half",
        "void",
        &[("value", "double", "copy")],
        vec![],
        vec![b.print(vec![text("half "), tokens(&["value", "/", "2"])])],
    );
    let cpp = builds_and_runs(
        "overloads",
        &[square, rectangle, real, half],
        body,
        &[],
        &[],
        &printed("square 9\nrectangle 6\nreal 4.5\nhalf 1.5\n"),
    );
    assert!(
        cpp.contains("    area(n);\n    area(2, n);\n    area(static_cast<double>(n), 1.5);\n"),
        "{cpp}"
    );
}

/// `show(bool)` beside `show(std::string)`: quoted text would pick the bool
/// one without `std::string(…)` (w2.md §2.8).
#[test]
fn quoted_text_calls_the_text_overload_beside_a_bool_one() {
    let b = Blocks::default();
    let mut show_bool = b.define(
        "show",
        "void",
        &[("flag", "bool", "copy")],
        vec![],
        vec![b.print(vec![text("bool "), tokens(&["flag"])])],
    );
    show_bool["fields"]["NAME"]["sym"] = json!("s_show_bool");
    let mut show_text = b.define(
        "show",
        "void",
        &[("words", "std::string", "read_only")],
        vec![],
        vec![b.print(vec![text("text "), tokens(&["words"])])],
    );
    show_text["fields"]["NAME"]["sym"] = json!("s_show_text");
    let body = vec![
        b.block(
            "func.call_stmt",
            &json!({ "fields": { "FUNC": { "ref": "s_show_text" } }, "extra": { "argCount": 1 },
                     "inputs": { "ARG0": text("hello") } }),
        ),
        b.block(
            "func.call_stmt",
            &json!({ "fields": { "FUNC": { "ref": "s_show_bool" } }, "extra": { "argCount": 1 },
                     "inputs": { "ARG0": json!({ "expr": [{ "kw": "true" }] }) } }),
        ),
    ];
    let cpp = builds_and_runs(
        "show",
        &[show_bool, show_text],
        body,
        &[],
        &[],
        &printed("text hello\nbool true\n"),
    );
    assert!(
        cpp.contains("    show(std::string(\"hello\"));\n    show(true);\n"),
        "{cpp}"
    );
}

/// `show(int)` beside `show(std::vector<int>)`, and `sum(std::vector<int>)`
/// beside `sum(std::vector<double>)`: a *list of* passed to one of them
/// names its type, since `show({1})` would call `show(int)` and `sum({1,
/// 2})` would be ambiguous (w2.md §2.8).
#[test]
fn a_list_of_calls_the_overload_its_block_names() {
    let b = Blocks::default();
    // An `int` input prints itself; a list input (`std::vector<elem>`)
    // prints its size.
    let define = |name: &str, sym: &str, param: &str, elem: Option<&str>, label: &str| {
        let value = match elem {
            None => tokens(&[param]),
            Some(_) => json!({ "block": b.block(
                "coll.length",
                &json!({ "fields": { "LIST": { "ref": format!("s_{param}") } } }),
            ) }),
        };
        let mode = if elem.is_some() { "read_only" } else { "copy" };
        let mut block = b.define(
            name,
            "void",
            &[(param, "int", mode)],
            vec![],
            vec![b.print(vec![text(label), value])],
        );
        block["fields"]["NAME"]["sym"] = json!(format!("s_{sym}"));
        if let Some(elem) = elem {
            block["extra"]["params"][0]["type"] = json!({ "std::vector": elem });
        }
        block
    };
    let top = vec![
        define("show", "show_int", "number", None, "int "),
        define("show", "show_list", "numbers", Some("int"), "list "),
        define("sum", "sum_int", "whole", Some("int"), "whole "),
        define("sum", "sum_real", "real", Some("double"), "real "),
    ];
    let list_of = |items: Vec<Value>| {
        let count = items.len();
        let inputs: Map<String, Value> = items
            .into_iter()
            .enumerate()
            .map(|(index, item)| (format!("ITEM{index}"), item))
            .collect();
        json!({ "block": b.block("coll.list_of", &json!({ "extra": { "itemCount": count }, "inputs": inputs })) })
    };
    let body = vec![
        b.call_stmt("show_list", vec![list_of(vec![number("1")])]),
        b.call_stmt("show_list", vec![list_of(vec![])]),
        b.call_stmt("show_int", vec![number("7")]),
        b.call_stmt("sum_int", vec![list_of(vec![number("1"), number("2")])]),
        b.call_stmt(
            "sum_real",
            vec![list_of(vec![number("1"), number("2"), number("3")])],
        ),
    ];
    let cpp = builds_and_runs(
        "list_overloads",
        &top,
        body,
        &[],
        &[],
        &printed("list 1\nlist 0\nint 7\nwhole 2\nreal 3\n"),
    );
    assert!(
        cpp.contains(concat!(
            "    show(std::vector<int>{1});\n    show(std::vector<int>{});\n    show(7);\n",
            "    sum(std::vector<int>{1, 2});\n    sum(std::vector<double>{1, 2, 3});\n",
        )),
        "{cpp}"
    );
}

#[test]
fn globals_and_constants_run_in_the_order_they_need() {
    let b = Blocks::default();
    let values: Vec<Value> = ["Fast", "Slow"]
        .iter()
        .map(|value| json!({ "sym": format!("s_Mode_{value}"), "name": value }))
        .collect();
    let mode = b.block(
        "type.enum",
        &json!({ "x": 440, "y": 400, "fields": { "NAME": { "sym": "s_Mode", "name": "Mode" } },
                 "extra": { "values": values } }),
    );
    let mode_value = |name: &str| {
        json!({ "block": b.block(
            "type.enum_value",
            &json!({ "fields": { "ENUM": { "ref": "s_Mode" }, "VALUE": { "ref": format!("s_Mode_{name}") } } }),
        ) })
    };
    let top = vec![
        mode,
        // `kLimit` reads `kBase`, which comes after it by block ID.
        b.global("constant", "int", "kLimit", Some(tokens(&["kBase", "*", "2"]))),
        b.global("constant", "int", "kBase", Some(number("5"))),
        b.global(
            "constant",
            "double",
            "kRate",
            Some(json!({ "expr": [{ "num": "1.5" }] })),
        ),
        b.global("constant", "std::string", "kName", Some(text("Ann"))),
        // `counter` is set from `next_id`, which reads `start`: `start` is
        // printed first, though `counter` comes first by name.
        b.global("variable", "int", "counter", Some(b.call("next_id", vec![]))),
        b.global("variable", "int", "start", Some(number("41"))),
        b.define(
            "next_id",
            "int",
            &[],
            vec![],
            vec![b.ret(tokens(&["start", "+", "1"]))],
        ),
    ];
    let body = vec![
        b.block(
            "var.declare",
            &json!({ "fields": { "TYPE": "int", "NAME": { "sym": "s_n", "name": "n" } },
                     "inputs": { "VALUE": number("10") } }),
        ),
        b.switch(
            tokens(&["n"]),
            vec![
                (tokens(&["kLimit"]), vec![b.print(vec![text("limit")])]),
                (tokens(&["kBase"]), vec![b.print(vec![text("base")])]),
            ],
        ),
        b.block(
            "var.declare",
            &json!({ "fields": { "TYPE": { "ref": "s_Mode" }, "NAME": { "sym": "s_m", "name": "m" } },
                     "inputs": { "VALUE": mode_value("Slow") } }),
        ),
        b.switch(
            tokens(&["m"]),
            vec![
                (mode_value("Fast"), vec![b.print(vec![text("fast")])]),
                (mode_value("Slow"), vec![b.print(vec![text("slow")])]),
            ],
        ),
        b.print(vec![tokens(&["counter"])]),
        b.print(vec![tokens(&["kName"])]),
        b.print(vec![tokens(&["kRate"])]),
        b.block(
            "var.set",
            &json!({ "fields": { "VAR": { "ref": "s_counter" } }, "inputs": { "VALUE": tokens(&["counter", "+", "kBase"]) } }),
        ),
        b.print(vec![tokens(&["counter"])]),
    ];
    let cpp = builds_and_runs(
        "globals",
        &top,
        body,
        &[],
        &[],
        &printed("limit\nslow\n42\nAnn\n1.5\n47\n"),
    );
    for expected in [
        "constexpr int kBase = 5;\nconstexpr int kLimit = kBase * 2;\nconst std::string kName = \"Ann\";\nconstexpr double kRate = 1.5;\n",
        "int next_id();\n",
        "int start = 41;\nint counter = next_id();\n",
    ] {
        assert!(cpp.contains(expected), "{expected}\n{cpp}");
    }
    let at = |text: &str| cpp.find(text).unwrap();
    assert!(at("enum class Mode") < at("int next_id();"), "{cpp}");
    assert!(at("int next_id();") < at("int start = 41;"), "{cpp}");
}

#[test]
fn change_flip_and_the_programs_arguments_run() {
    let b = Blocks::default();
    let create = |ty: &str, name: &str, value: Value| {
        b.block(
            "var.declare",
            &json!({ "fields": { "TYPE": ty, "NAME": { "sym": format!("s_{name}"), "name": name } },
                     "inputs": { "VALUE": value } }),
        )
    };
    let change = |name: &str, op: &str, by: Value| {
        b.block(
            "var.change",
            &json!({ "fields": { "VAR": { "ref": format!("s_{name}") }, "OP": op }, "inputs": { "BY": by } }),
        )
    };
    let body = vec![
        create("std::string", "word", text("ab")),
        change("word", "append", text("cd")),
        change("word", "append", letter("e")),
        create("int", "bits", number("6")),
        change("bits", "and", number("3")),
        change("bits", "shl", number("4")),
        change("bits", "xor", number("1")),
        create("bool", "done", json!({ "expr": [{ "kw": "false" }] })),
        b.block("var.flip", &json!({ "fields": { "VAR": { "ref": "s_done" } } })),
        b.print(vec![tokens(&["word"])]),
        b.print(vec![tokens(&["bits"])]),
        b.print(vec![tokens(&["done"])]),
        // The program's own name is its only argument here.
        b.print(vec![json!({ "block": b.block(
            "coll.length",
            &json!({ "fields": { "LIST": { "ref": "s_args" } } }),
        ) })]),
    ];
    let cpp = builds_and_runs_main(
        "change_flip_args",
        &json!({ "ARGS": true, "ARGV": { "sym": "s_args", "name": "args" } }),
        &[],
        body,
        &[],
        &[],
        &printed("abcde\n33\ntrue\n1\n"),
    );
    for expected in [
        "int main(int argc, char* argv[]) {\n    const std::vector<std::string> args(argv, argv + argc);\n",
        "    word += \"cd\";\n    word += 'e';\n",
        "    bits &= 3;\n    bits <<= 4;\n    bits ^= 1;\n",
        "    done = !done;\n",
    ] {
        assert!(cpp.contains(expected), "{expected}\n{cpp}");
    }
}

#[test]
fn struct_defaults_run_when_a_value_is_made_and_follow_the_globals_they_read() {
    let b = Blocks::default();
    let rows = json!([
        { "sym": "s_Config_limit", "name": "limit", "type": "int" },
        { "sym": "s_Config_label", "name": "label", "type": "std::string" },
        { "sym": "s_Config_ready", "name": "ready", "type": "bool" },
    ]);
    let config = b.block(
        "type.struct",
        &json!({ "x": 440, "y": 400, "fields": { "NAME": { "sym": "s_Config", "name": "Config" } },
                 "extra": { "fields": rows },
                 "inputs": { "DEFAULT0": b.call("next_id", vec![]), "DEFAULT1": text("box") } }),
    );
    let mut built = b.global("variable", "int", "config", None);
    built["fields"]["TYPE"] = json!({ "ref": "s_Config" });
    let top = vec![
        // `config` is built with `limit = next_id()`, and `next_id` reads
        // `start`: `start` is printed first, though `config` comes first
        // by name.
        built,
        config,
        b.global("variable", "int", "start", Some(number("41"))),
        b.define(
            "next_id",
            "int",
            &[],
            vec![],
            vec![b.ret(tokens(&["start", "+", "1"]))],
        ),
    ];
    let field = |object: &str, member: &str| {
        json!({ "block": b.block(
            "type.field",
            &json!({ "fields": { "MEMBER": { "ref": format!("s_Config_{member}") } },
                     "inputs": { "OBJ": tokens(&[object]) } }),
        ) })
    };
    let body = vec![
        b.print(vec![field("config", "limit")]),
        b.print(vec![field("config", "label")]),
        b.block(
            "var.declare",
            &json!({ "fields": { "TYPE": { "ref": "s_Config" }, "NAME": { "sym": "s_local", "name": "local" } } }),
        ),
        b.print(vec![field("local", "ready")]),
    ];
    let cpp = builds_and_runs(
        "struct_defaults",
        &top,
        body,
        &[],
        &[],
        &printed("42\nbox\nfalse\n"),
    );
    assert!(
        cpp.contains("    int limit = next_id();\n    std::string label = \"box\";\n    bool ready{};\n"),
        "{cpp}"
    );
    let at = |text: &str| cpp.find(text).unwrap();
    assert!(at("int next_id();") < at("struct Config"), "{cpp}");
    assert!(at("int start = 41;") < at("Config config{};"), "{cpp}");
}

/// *new P with* an empty part before a filled one, over a field with a
/// default (`int x = 10;`): C++20 leaves the field out (`P{.y = 5}`), so it
/// keeps its default; C++17 would print `P{{}, 5}`, which makes it 0, so
/// the front end refuses it there (`B2C-E0430`).
#[test]
fn new_with_an_empty_part_keeps_the_fields_default() {
    let b = Blocks::default();
    let rows = json!([
        { "sym": "s_P_x", "name": "x", "type": "int" },
        { "sym": "s_P_y", "name": "y", "type": "int" },
    ]);
    let point = b.block(
        "type.struct",
        &json!({ "x": 440, "y": 400, "fields": { "NAME": { "sym": "s_P", "name": "P" } },
                 "extra": { "fields": rows }, "inputs": { "DEFAULT0": number("10") } }),
    );
    let made = b.block(
        "type.new",
        &json!({ "fields": { "TYPE": { "ref": "s_P" } }, "extra": { "fieldCount": 2 },
                 "inputs": { "FIELD1": number("5") } }),
    );
    let field = |member: &str| {
        json!({ "block": b.block(
            "type.field",
            &json!({ "fields": { "MEMBER": { "ref": format!("s_P_{member}") } }, "inputs": { "OBJ": tokens(&["p"]) } }),
        ) })
    };
    let main = json!({
        "id": "main", "type": "program.main", "v": 1, "x": 40, "y": 40,
        "statements": { "BODY": [
            b.block(
                "var.declare",
                &json!({ "fields": { "TYPE": { "ref": "s_P" }, "NAME": { "sym": "s_p", "name": "p" } },
                         "inputs": { "VALUE": { "block": made } } }),
            ),
            b.print(vec![field("x"), text(" "), field("y")]),
        ] },
    });
    let blocks = [main, point];
    let cpp17 = run_frontend(&project(CppStandard::Cpp17, &blocks), &FrontendOptions::default());
    let codes: Vec<&str> = cpp17.diagnostics.iter().map(|d| d.code.0.as_str()).collect();
    assert_eq!(codes, ["B2C-E0430"], "{:#?}", cpp17.diagnostics);
    let cpp20 = run_frontend(&project(CppStandard::Cpp20, &blocks), &FrontendOptions::default());
    assert!(
        cpp20.diagnostics.iter().all(|d| d.severity == Severity::Info),
        "{:#?}",
        cpp20.diagnostics
    );
    let generated = cpp20.generated.unwrap();
    let [file] = generated.files.as_slice() else {
        panic!("one file expected");
    };
    assert!(file.contents.contains("P p = P{.y = 5};"), "{}", file.contents);
    let build = tempfile::tempdir().unwrap();
    let source = build.path().join("gap.cpp");
    std::fs::write(&source, &file.contents).unwrap();
    for (index, toolchain) in compilers().iter().enumerate() {
        let program = build.path().join(format!("gap-{index}"));
        let flags: Vec<&str> = std::iter::once("-std=c++20")
            .chain(STRICT.iter().copied())
            .collect();
        if let Err(messages) = compile(toolchain, &flags, &source, &program, build.path()) {
            panic!(
                "{} rejected gap: {messages}\n{}",
                toolchain.path().display(),
                file.contents
            );
        }
        let run = command(&program, build.path()).output().unwrap();
        assert_eq!(
            String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n"),
            "10 5\n"
        );
    }
}
