//! W2's text, input and output, math and error blocks run (package B,
//! docs/plans/m3-contracts/w2.md §5.2): each program is made of blocks as
//! the editor saves them, goes through the whole front end without a
//! diagnostic, is compiled with the `g++` on `PATH` and every
//! `/usr/bin/g++-N` under C++17 and C++20 at the *strict* warning level with
//! `-Werror`, and is run, and its output is compared. Each compiler is
//! probed and run as builds run it: from its canonical path, through
//! b2c-process, in the environment `compiler_env` gives it.
//!
//! * *text as number* reads the number at the start of a text and throws
//!   both of its errors, which catch parts catch;
//! * *show numbers with* changes how later decimal numbers print, also in
//!   *join* and *as text*, and leaves whole numbers alone;
//! * the text blocks on the edge cases of their helpers (empty text,
//!   characters outside ASCII left alone, an empty text to replace);
//! * *larger*, *smaller* and *kept between* with operands of mixed types,
//!   and the functions of *%FN of*;
//! * a catch part for each error type, nested ones that throw the error
//!   again, a catch variable no block reads, and *letter of* out of range.

// Test code: unwrap/expect/panic are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use b2c_build::{FrontendOptions, run_frontend};
use b2c_ir::Severity;
use b2c_ir::sast::CppStandard;
use b2c_toolchain::env::{HostEnv, compiler_env};
use b2c_toolchain::probe::{ProbeOptions, Toolchain, probe};
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

/// A command for a built test program. Production code spawns processes
/// only through b2c-process; this test runs the programs it builds directly.
#[allow(clippy::disallowed_methods)]
fn command(program: &Path) -> std::process::Command {
    std::process::Command::new(program)
}

/// The g++ on `PATH`, then every `/usr/bin/g++-N`, each probed (probed once
/// per test binary). A probed toolchain's path is the one builds run: on
/// Windows not the verbatim `\\?\` form, from which g++ cannot find
/// `cc1plus`.
fn compilers() -> &'static [Toolchain] {
    static PROBED: OnceLock<Vec<Toolchain>> = OnceLock::new();
    PROBED.get_or_init(|| {
        let Some(on_path) = common::real_toolchain() else {
            return Vec::new();
        };
        let mut found = vec![on_path];
        for major in 9..=20 {
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
/// runs g++ (through b2c-process, in the compiler's environment, with
/// `dir` as its working and temporary folder). Returns g++'s messages when
/// it fails.
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
    /// A `kind` block with these parts (`fields`, `inputs`, `statements`,
    /// `extra`); the catalog fills in the rest when the project loads.
    fn block(&self, kind: &str, parts: &Value) -> Value {
        let made = self.made.get() + 1;
        self.made.set(made);
        let mut block = json!({ "id": format!("b{made:03}"), "type": kind, "v": 1 });
        for (key, value) in parts.as_object().unwrap() {
            block[key] = value.clone();
        }
        block
    }

    /// A value input holding a `kind` block with these parts.
    fn value(&self, kind: &str, parts: &Value) -> Value {
        json!({ "block": self.block(kind, parts) })
    }

    /// *print* of these values (inputs) on one line, without separators.
    fn print(&self, items: Vec<Value>) -> Value {
        let inputs: Map<String, Value> = items
            .into_iter()
            .enumerate()
            .map(|(index, item)| (format!("ITEM{index}"), item))
            .collect();
        let count = inputs.len();
        self.block(
            "io.print",
            &json!({ "extra": { "itemCount": count }, "inputs": inputs }),
        )
    }

    /// *try … catch*: one catch part per `(error, symbol, statements)`.
    fn try_catch(&self, body: Vec<Value>, parts: Vec<(&str, &str, Vec<Value>)>) -> Value {
        let mut fields = Map::new();
        let mut statements = Map::new();
        statements.insert(String::from("TRY"), Value::Array(body));
        let count = parts.len();
        for (index, (error, symbol, part)) in parts.into_iter().enumerate() {
            fields.insert(format!("ERROR{index}"), json!(error));
            fields.insert(format!("NAME{index}"), json!({ "sym": symbol, "name": "e" }));
            statements.insert(format!("CATCH{index}"), Value::Array(part));
        }
        self.block(
            "error.try",
            &json!({ "extra": { "catchCount": count }, "fields": fields, "statements": statements }),
        )
    }

    /// *message of* the error `symbol`.
    fn message(&self, symbol: &str) -> Value {
        self.value(
            "error.message",
            &json!({ "fields": { "ERROR": { "ref": symbol } } }),
        )
    }

    /// *throw* of `error` with `message`.
    fn throw(&self, error: &str, message: &str) -> Value {
        self.block(
            "error.throw",
            &json!({ "fields": { "ERROR": error }, "inputs": { "MESSAGE": text(message) } }),
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
        "project": { "id": "prj_text_errors", "name": "Text and errors", "language": { "standard": standard } },
        "modules": [{ "id": "mod_main", "name": "main", "workspace": { "blocks": blocks } }],
    });
    serde_json::to_vec_pretty(&document).unwrap()
}

/// Generates `main` with `body` for both standards, builds it with every
/// compiler, runs it and compares what it prints with `expected`.
fn runs_and_prints(name: &str, body: Vec<Value>, expected: &str) {
    let main = json!({
        "id": "main", "type": "program.main", "v": 1, "x": 40, "y": 40, "statements": { "BODY": Value::Array(body) },
    });
    let compilers = compilers();
    let dir = tempfile::tempdir().unwrap();
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
        let source = dir.path().join(format!("{name}.cpp"));
        std::fs::write(&source, &file.contents).unwrap();
        for (index, toolchain) in compilers.iter().enumerate() {
            let gxx = toolchain.path().display();
            let program = dir.path().join(format!("{name}-{index}"));
            let flags: Vec<&str> = std::iter::once(flag).chain(STRICT.iter().copied()).collect();
            if let Err(messages) = compile(toolchain, &flags, &source, &program, dir.path()) {
                panic!(
                    "{gxx} {flag} rejected {name}: {messages}\n--- source ---\n{}",
                    file.contents
                );
            }
            let run = command(&program).output().unwrap();
            assert!(run.status.success(), "{name} ({flag}, {gxx}) failed");
            let stdout = String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n");
            assert_eq!(stdout, expected, "{name} ({flag}, {gxx})");
        }
    }
}

#[test]
fn text_as_number_reads_and_throws_both_errors() {
    let b = Blocks::default();
    let as_int = |value: &str| b.value("text.to_number", &json!({ "inputs": { "TEXT": text(value) } }));
    let as_double = |value: &str| {
        b.value(
            "text.to_number",
            &json!({ "fields": { "TO": "double" }, "inputs": { "TEXT": text(value) } }),
        )
    };
    let body = vec![
        b.print(vec![as_int("12abc")]),
        b.print(vec![as_double(" 2.5x")]),
        b.try_catch(
            vec![b.print(vec![as_int("abc")]), b.print(vec![text("not reached")])],
            vec![
                (
                    "std::out_of_range",
                    "s_range",
                    vec![b.print(vec![text("wrong part")])],
                ),
                (
                    "std::invalid_argument",
                    "s_invalid",
                    vec![b.print(vec![text("invalid argument")])],
                ),
            ],
        ),
        b.try_catch(
            vec![b.print(vec![as_int("99999999999")])],
            vec![
                (
                    "std::invalid_argument",
                    "s_invalid2",
                    vec![b.print(vec![text("wrong part")])],
                ),
                (
                    "std::out_of_range",
                    "s_range2",
                    vec![b.print(vec![text("out of range")])],
                ),
            ],
        ),
    ];
    runs_and_prints("to_number", body, "12\n2.5\ninvalid argument\nout of range\n");
}

#[test]
fn decimals_change_how_later_decimal_numbers_print() {
    let b = Blocks::default();
    let join = b.value(
        "text.join",
        &json!({ "extra": { "itemCount": 2 }, "inputs": { "ITEM0": text("x = "), "ITEM1": number("2.5") } }),
    );
    let third = json!({ "expr": [{ "num": "1.0" }, { "op": "/" }, { "num": "3" }] });
    let as_text = b.value("text.as_text", &json!({ "inputs": { "VALUE": third.clone() } }));
    let body = vec![
        b.print(vec![third.clone()]),
        b.block("io.decimals", &json!({ "inputs": { "DIGITS": number("2") } })),
        b.print(vec![number("2.5")]),
        b.print(vec![join]),
        b.print(vec![as_text]),
        b.print(vec![number("7")]),
        b.block("io.decimals", &json!({ "inputs": { "DIGITS": number("0") } })),
        b.print(vec![third]),
    ];
    runs_and_prints("decimals", body, "0.333333\n2.50\nx = 2.50\n0.33\n7\n0\n");
}

#[test]
fn text_blocks_on_the_edges_of_their_helpers() {
    let b = Blocks::default();
    let show = |value: Value| b.print(vec![text("["), value, text("]")]);
    let case = |which: &str, value: &str| {
        b.value(
            "text.case",
            &json!({ "fields": { "CASE": which }, "inputs": { "TEXT": text(value) } }),
        )
    };
    let trim = |value: &str| b.value("text.trim", &json!({ "inputs": { "TEXT": text(value) } }));
    let replace = |old: &str, new: &str, value: &str| {
        b.value(
            "text.replace",
            &json!({ "inputs": { "OLD": text(old), "NEW": text(new), "TEXT": text(value) } }),
        )
    };
    let find = |op: &str, value: &str, part: &str| {
        b.value(
            "text.find",
            &json!({ "fields": { "OP": op }, "inputs": { "TEXT": text(value), "PART": text(part) } }),
        )
    };
    let body = vec![
        show(case("lower", "Hello, WÖRLD 123")),
        show(case("upper", "ab-é")),
        show(case("upper", "")),
        show(trim(" \t hi there \t ")),
        show(trim("   ")),
        show(replace("a", "aa", "banana")),
        show(replace("", "x", "abc")),
        show(find("contains", "hello", "ell")),
        show(find("starts", "hello", "hex")),
        show(find("ends", "hello", "")),
        show(b.value(
            "text.part",
            &json!({ "inputs": { "TEXT": text("hello"), "START": number("1"), "LENGTH": number("10") } }),
        )),
        show(b.value(
            "text.letter",
            &json!({ "inputs": { "INDEX": number("1"), "TEXT": text("hello") } }),
        )),
        show(b.value("text.length", &json!({ "inputs": { "TEXT": text("héllo") } }))),
        show(b.value("text.length", &json!({ "inputs": { "TEXT": text("") } }))),
        show(b.value("text.as_text", &json!({ "inputs": { "VALUE": letter("c") } }))),
    ];
    runs_and_prints(
        "text_edges",
        body,
        "[hello, wÖrld 123]\n[AB-é]\n[]\n[hi there]\n[]\n[baanaanaa]\n[abc]\n[true]\n[false]\n[true]\n\
         [ello]\n[e]\n[6]\n[0]\n[c]\n",
    );
}

#[test]
fn math_blocks_with_mixed_operands() {
    let b = Blocks::default();
    let minmax = |which: &str, a: Value, b2: Value| {
        b.value(
            "math.minmax",
            &json!({ "fields": { "FN": which }, "inputs": { "A": a, "B": b2 } }),
        )
    };
    let function = |which: &str, value: &str| {
        b.value(
            "math.fn",
            &json!({ "fields": { "FN": which }, "inputs": { "VALUE": number(value) } }),
        )
    };
    let two = |which: &str, a: &str, b2: &str| {
        b.value(
            "math.fn_two",
            &json!({ "fields": { "FN": which }, "inputs": { "A": number(a), "B": number(b2) } }),
        )
    };
    let clamp = |value: Value, low: &str, high: &str| {
        b.value(
            "math.clamp",
            &json!({ "inputs": { "VALUE": value, "LOW": number(low), "HIGH": number(high) } }),
        )
    };
    let truth = json!({ "expr": [{ "kw": "true" }] });
    let body = vec![
        b.print(vec![minmax("max", truth.clone(), number("2"))]),
        b.print(vec![minmax("min", letter("a"), number("2.5"))]),
        b.print(vec![minmax("max", letter("a"), number("2"))]),
        b.print(vec![clamp(letter("z"), "0", "99.5")]),
        b.print(vec![clamp(truth, "-1", "1")]),
        b.print(vec![
            b.value("math.abs", &json!({ "inputs": { "VALUE": number("-3") } })),
        ]),
        b.print(vec![
            b.value("math.abs", &json!({ "inputs": { "VALUE": number("-2.5") } })),
        ]),
        b.print(vec![function("sqrt", "9")]),
        b.print(vec![function("round", "2.5")]),
        b.print(vec![function("floor", "-2.5")]),
        b.print(vec![function("log10", "1000")]),
        b.print(vec![two("pow", "2", "10")]),
        b.print(vec![two("hypot", "3", "4")]),
        b.print(vec![b.value(
            "math.random_real",
            &json!({ "inputs": { "LOW": number("3.0"), "HIGH": number("3.0") } }),
        )]),
    ];
    runs_and_prints(
        "math",
        body,
        "2\n2.5\n97\n99.5\n1\n3\n2.5\n3\n3\n-3\n3\n1024\n5\n3\n",
    );
}

#[test]
fn errors_are_caught_by_their_part_and_thrown_again() {
    let b = Blocks::default();
    let body = vec![
        // A catch part for each error type; the others are skipped.
        b.try_catch(
            vec![b.throw("std::out_of_range", "far")],
            vec![
                (
                    "std::invalid_argument",
                    "s_a",
                    vec![b.print(vec![text("wrong part")])],
                ),
                (
                    "std::runtime_error",
                    "s_b",
                    vec![b.print(vec![text("wrong part")])],
                ),
                (
                    "std::out_of_range",
                    "s_c",
                    vec![b.print(vec![text("range: "), b.message("s_c")])],
                ),
                ("std::exception", "s_d", vec![b.print(vec![text("wrong part")])]),
            ],
        ),
        // Any error catches every kind; a variable no block reads.
        b.try_catch(
            vec![b.throw("std::invalid_argument", "bad")],
            vec![(
                "std::exception",
                "s_unused",
                vec![b.print(vec![text("any error")])],
            )],
        ),
        // Thrown again to the try block around it.
        b.try_catch(
            vec![b.try_catch(
                vec![b.throw("std::runtime_error", "inner")],
                vec![(
                    "std::runtime_error",
                    "s_inner",
                    vec![
                        b.print(vec![text("caught "), b.message("s_inner")]),
                        b.block("error.rethrow", &json!({})),
                    ],
                )],
            )],
            vec![(
                "std::exception",
                "s_outer",
                vec![b.print(vec![text("again "), b.message("s_outer")])],
            )],
        ),
        // *letter of* outside the text throws an out-of-range error.
        b.try_catch(
            vec![b.print(vec![b.value(
                "text.letter",
                &json!({ "inputs": { "INDEX": number("10"), "TEXT": text("abc") } }),
            )])],
            vec![(
                "std::out_of_range",
                "s_letter",
                vec![b.print(vec![text("no letter 10")])],
            )],
        ),
    ];
    runs_and_prints(
        "errors",
        body,
        "range: far\nany error\ncaught inner\nagain inner\nno letter 10\n",
    );
}
