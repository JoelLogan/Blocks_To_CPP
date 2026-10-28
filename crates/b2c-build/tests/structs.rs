//! W2's struct and enum blocks run (package C1,
//! docs/plans/m3-contracts/w2.md §1.5, §2.2, §2.11, §5.2): each program is
//! made of blocks as the editor saves them, goes through the whole front end
//! without a diagnostic, is compiled with the `g++` on `PATH` and every
//! `/usr/bin/g++-N` under C++17 and C++20 at the *strict* warning level with
//! `-Werror`, and is run, and its output is compared. Each compiler is
//! probed and run as builds run it: from its canonical path, through
//! b2c-process, in the environment `compiler_env` gives it.
//!
//! * *new … with* with empty parts (C++20 leaves them out, C++17 writes `{}`
//!   before a filled one) and a part cast to its field's type;
//! * *… of …* and *set … of … to*;
//! * a comparable and printable struct holding an enum, compared and
//!   printed; the enum printed, joined and turned into text through
//!   `b2c::to_text`, beside a user function `to_string` taking it;
//! * structs printed after the types they hold, whatever the canvas order,
//!   and two structs holding lists of each other (a forward declaration).

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

/// A command for a built test program. Production code spawns processes
/// only through b2c-process; this test runs the programs it builds directly.
#[allow(clippy::disallowed_methods)]
fn command(program: &Path) -> std::process::Command {
    std::process::Command::new(program)
}

/// The g++ on `PATH`, then every `/usr/bin/g++-N` Blocks2Cpp supports
/// (`MIN_GCC_MAJOR` and newer: an older one beside it, as in the GCC 11
/// container, is one the app refuses too), each probed once per test binary,
/// so that each runs from the path builds run it from.
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

/// Compiles `source` into `program` as a build runs g++. Returns g++'s
/// messages when it fails.
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
    /// A `kind` block with these parts.
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

    /// *print* of these values on one line, without separators.
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

    /// *struct* `name` with `(field, type term)` rows, at a canvas position.
    fn structure(&self, name: &str, fields: &[(&str, Value)], comparable: bool, printable: bool) -> Value {
        let rows: Vec<Value> = fields
            .iter()
            .map(|(field, ty)| json!({ "sym": format!("s_{name}_{field}"), "name": field, "type": ty }))
            .collect();
        self.block(
            "type.struct",
            &json!({
                "x": 40, "y": 40,
                "fields": { "NAME": { "sym": format!("s_{name}"), "name": name },
                            "COMPARABLE": comparable, "PRINTABLE": printable },
                "extra": { "fields": rows },
            }),
        )
    }

    /// *enum* `name` with these values.
    fn enumeration(&self, name: &str, values: &[&str]) -> Value {
        let rows: Vec<Value> = values
            .iter()
            .map(|value| json!({ "sym": format!("s_{name}_{value}"), "name": value }))
            .collect();
        self.block(
            "type.enum",
            &json!({
                "x": 40, "y": 40,
                "fields": { "NAME": { "sym": format!("s_{name}"), "name": name } },
                "extra": { "values": rows },
            }),
        )
    }

    /// *new `ty` with* these parts (`None` for an empty part).
    fn new_with(&self, ty: &str, parts: Vec<Option<Value>>) -> Value {
        let count = parts.len();
        let inputs: Map<String, Value> = parts
            .into_iter()
            .enumerate()
            .filter_map(|(index, part)| part.map(|part| (format!("FIELD{index}"), part)))
            .collect();
        self.value(
            "type.new",
            &json!({ "fields": { "TYPE": user(ty) }, "extra": { "fieldCount": count }, "inputs": inputs }),
        )
    }

    /// *`member` of `object`*.
    fn field_of(&self, owner: &str, member: &str, object: Value) -> Value {
        self.value(
            "type.field",
            &json!({ "fields": { "MEMBER": { "ref": format!("s_{owner}_{member}") } }, "inputs": { "OBJ": object } }),
        )
    }

    /// *`enum` `value`*.
    fn enum_value(&self, owner: &str, value: &str) -> Value {
        self.value(
            "type.enum_value",
            &json!({ "fields": { "ENUM": user(owner), "VALUE": { "ref": format!("s_{owner}_{value}") } } }),
        )
    }

    /// *create* `name` of the type term `ty`, set to `value`.
    fn create(&self, ty: Value, name: &str, value: Value) -> Value {
        self.block(
            "var.declare",
            &json!({ "fields": { "TYPE": ty, "NAME": { "sym": format!("s_{name}"), "name": name } },
                     "inputs": { "VALUE": value } }),
        )
    }

    /// The variable `name`.
    fn get(&self, name: &str) -> Value {
        self.value(
            "var.get",
            &json!({ "fields": { "VAR": { "ref": format!("s_{name}") } } }),
        )
    }
}

/// A user type as a type term.
fn user(name: &str) -> Value {
    json!({ "ref": format!("s_{name}") })
}

/// An expression input of one text literal.
fn text(value: &str) -> Value {
    json!({ "expr": [{ "str": value }] })
}

/// An expression input of one number literal.
fn number(value: &str) -> Value {
    json!({ "expr": [{ "num": value }] })
}

/// The project bytes of a module with these top-level blocks.
fn project(standard: CppStandard, blocks: &[Value]) -> Vec<u8> {
    let standard = match standard {
        CppStandard::Cpp17 => "c++17",
        _ => "c++20",
    };
    let document = json!({
        "format": "blocks2cpp/project",
        "formatVersion": 2,
        "generator": { "app": env!("CARGO_PKG_VERSION"), "catalog": b2c_catalog::CATALOG_VERSION },
        "project": { "id": "prj_structs", "name": "Structs", "language": { "standard": standard } },
        "modules": [{ "id": "mod_main", "name": "main", "workspace": { "blocks": blocks } }],
    });
    serde_json::to_vec_pretty(&document).unwrap()
}

/// Generates the program for both standards, checks that each C++ holds
/// its `snippets`, builds it with every compiler, runs it and compares what
/// it prints with `expected`.
fn runs_and_prints(name: &str, blocks: &[Value], snippets: [&[&str]; 2], expected: &str) {
    let compilers = compilers();
    let dir = tempfile::tempdir().unwrap();
    for ((standard, flag), snippets) in [
        (CppStandard::Cpp17, "-std=c++17"),
        (CppStandard::Cpp20, "-std=c++20"),
    ]
    .into_iter()
    .zip(snippets)
    {
        let frontend = run_frontend(&project(standard, blocks), &FrontendOptions::default());
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
        for snippet in snippets {
            assert!(
                file.contents.contains(snippet),
                "{name} ({flag}) lacks {snippet}:\n{}",
                file.contents
            );
        }
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

/// `when program starts` with these statements.
fn main_block(body: Vec<Value>) -> Value {
    json!({ "id": "main", "type": "program.main", "v": 1, "x": 40, "y": 400, "statements": { "BODY": body } })
}

#[test]
fn structs_are_made_read_changed_compared_and_printed() {
    let b = Blocks::default();
    let student = b.structure(
        "Student",
        &[
            ("name", json!("std::string")),
            ("mark", json!("int")),
            ("average", json!("double")),
        ],
        true,
        true,
    );
    let body = vec![
        b.create(
            user("Student"),
            "ann",
            b.new_with("Student", vec![Some(text("Ann")), Some(number("74")), None]),
        ),
        b.create(json!("int"), "n", number("5")),
        b.create(
            user("Student"),
            "bo",
            b.new_with("Student", vec![None, Some(number("3")), Some(b.get("n"))]),
        ),
        b.block(
            "type.set_field",
            &json!({ "fields": { "MEMBER": { "ref": "s_Student_name" }, "VAR": { "ref": "s_bo" } },
                     "inputs": { "VALUE": text("Bo") } }),
        ),
        b.print(vec![b.get("ann")]),
        b.print(vec![b.get("bo")]),
        b.print(vec![b.field_of("Student", "mark", b.get("ann"))]),
        b.print(vec![b.value(
            "math.compare",
            &json!({ "fields": { "OP": "lt" }, "inputs": { "A": b.get("ann"), "B": b.get("bo") } }),
        )]),
    ];
    runs_and_prints(
        "students",
        &[student, main_block(body)],
        [
            &[
                "Student ann = Student{\"Ann\", 74};",
                "Student bo = Student{{}, 3, static_cast<double>(n)};",
                "bo.name = \"Bo\";",
                "friend bool operator<(const Student& lhs, const Student& rhs) {",
            ],
            &[
                "Student ann = Student{.name = \"Ann\", .mark = 74};",
                "Student bo = Student{.mark = 3, .average = static_cast<double>(n)};",
                "auto operator<=>(const Student&) const = default;",
            ],
        ],
        "Student{name: Ann, mark: 74, average: 0}\nStudent{name: Bo, mark: 3, average: 5}\n74\ntrue\n",
    );
}

#[test]
fn enums_are_printed_joined_and_turned_into_text_beside_a_user_to_string() {
    let b = Blocks::default();
    let colour = b.enumeration("Colour", &["Red", "Green"]);
    // Defined after the struct on the canvas, printed before it.
    let pen = b.structure(
        "Pen",
        &[("colour", user("Colour")), ("size", json!("double"))],
        false,
        true,
    );
    let to_string = b.block(
        "func.define",
        &json!({
            "x": 40, "y": 800,
            "fields": { "NAME": { "sym": "s_to_string", "name": "to_string" }, "RETURNS": "std::string" },
            "extra": { "params": [{ "sym": "s_c", "name": "c", "type": user("Colour"), "mode": "read_only" }] },
            "statements": { "BODY": [b.block("func.return", &json!({ "inputs": { "VALUE": b.value(
                "text.join",
                &json!({ "extra": { "itemCount": 2 }, "inputs": { "ITEM0": text("colour "), "ITEM1": b.get("c") } }),
            ) } }))] },
        }),
    );
    let body = vec![
        b.create(
            user("Pen"),
            "pen",
            b.new_with(
                "Pen",
                vec![Some(b.enum_value("Colour", "Green")), Some(number("2"))],
            ),
        ),
        b.print(vec![b.get("pen")]),
        b.print(vec![b.enum_value("Colour", "Red")]),
        b.print(vec![b.value(
            "text.as_text",
            &json!({ "inputs": { "VALUE": b.field_of("Pen", "colour", b.get("pen")) } }),
        )]),
        b.print(vec![b.value(
            "func.call",
            &json!({ "fields": { "FUNC": { "ref": "s_to_string" } }, "extra": { "argCount": 1 },
                     "inputs": { "ARG0": b.enum_value("Colour", "Red") } }),
        )]),
    ];
    let both: &[&str] = &[
        "enum class Colour { Red, Green };",
        "namespace b2c {\n\ninline std::string to_text(Colour value) {",
        "std::string to_string(Colour c) {",
        "return std::string(\"colour \") + b2c::to_text(c);",
    ];
    runs_and_prints(
        "enums",
        &[pen, to_string, colour, main_block(body)],
        [both, both],
        "Pen{colour: Green, size: 2}\nRed\nGreen\ncolour Red\n",
    );
}

#[test]
fn structs_that_list_each_other_get_a_forward_declaration() {
    let b = Blocks::default();
    let list_of = |name: &str| json!({ "std::vector": user(name) });
    let team = b.structure("Team", &[("players", list_of("Player"))], false, false);
    let player = b.structure("Player", &[("teams", list_of("Team"))], false, false);
    let tree = b.structure("Tree", &[("children", list_of("Tree"))], false, false);
    let body = vec![
        b.create(user("Team"), "team", b.new_with("Team", vec![None])),
        b.create(user("Tree"), "tree", b.new_with("Tree", vec![None])),
        b.print(vec![text("done")]),
    ];
    let both: &[&str] = &[
        "struct Team;\n\nstruct Player {\n    std::vector<Team> teams{};\n};",
        "struct Team {\n    std::vector<Player> players{};\n};",
        "struct Tree {\n    std::vector<Tree> children{};\n};",
    ];
    runs_and_prints(
        "lists",
        &[team, player, tree, main_block(body)],
        [both, both],
        "done\n",
    );
}
