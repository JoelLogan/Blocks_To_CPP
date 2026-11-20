//! *Textbook style* and `using namespace` (06 §6.14.13, w3-modules.md §5.2,
//! package N2), with the programs built and run as the golden tests do:
//!
//! * **The differential test.** Every example, generated with Textbook style
//!   the other way round from its project file (on for all but
//!   `textbook_grades`, which has it on already), builds without a warning
//!   and prints exactly its golden run data; the golden test checks the
//!   example as it is, so each example gives the same output both ways. A
//!   `.cpp` that includes a standard header then holds `using namespace
//!   std;` exactly when Textbook style is on. The catalog blocks' default
//!   cases are compiled with Textbook style on in `catalog_syntax.rs`.
//! * **The lookup cases.** The cases of ADR-0006's studies that M3 can build
//!   (no class, library pack or Raw C++), made from blocks: a user `max`
//!   and a global `count` beside `std::max` and `std::count`; other common
//!   names the standard library declares; argument-dependent lookup into a
//!   user namespace; a name of a user namespace hidden by a global; and
//!   *use namespace* naming a namespace that is declared further down. Each
//!   is built and run under C++17 and C++20 without a directive, with
//!   Textbook style and with a *use namespace std in this file* block, and
//!   prints the same output every time.
//!
//! Without g++ these tests do nothing, unless `B2C_REQUIRE_GXX` is set.

// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// The block builders take their parts by value, as `json!` reads them.
#![allow(clippy::needless_pass_by_value)]

mod common;

use std::path::Path;

use b2c_build::{FrontendOptions, run_frontend};
use b2c_ir::Severity;
use b2c_ir::source_map::{FileKind, GeneratedProject};
use common::repo_root;
use common::run_data::{RunData, build_project};
use serde_json::{Value, json};

/// `project` with Textbook style set to `on` (format version 2 at least,
/// which has the option).
fn with_textbook_style(project: &Value, on: bool) -> Value {
    let mut project = project.clone();
    if project["formatVersion"].as_u64().unwrap_or(1) < 2 {
        project["formatVersion"] = json!(2);
    }
    let options = &mut project["project"]["options"];
    if !options.is_object() {
        *options = json!({});
    }
    options["usingNamespaceStd"] = json!(on);
    project
}

/// Whether `project` has Textbook style on.
fn textbook_style(project: &Value) -> bool {
    project["project"]["options"]["usingNamespaceStd"].as_bool() == Some(true)
}

/// The generated files of `project`, or every diagnostic but a note.
fn generate(project: &Value) -> Result<GeneratedProject, String> {
    let frontend = run_frontend(&serde_json::to_vec(project).unwrap(), &FrontendOptions::default());
    let reported: Vec<String> = frontend
        .diagnostics
        .iter()
        .filter(|d| d.severity != Severity::Info)
        .map(|d| format!("{} {:?}: {}", d.code.0, d.primary, d.message))
        .collect();
    match frontend.generated {
        Some(generated) if reported.is_empty() => Ok(generated),
        _ => Err(format!("{reported:#?}")),
    }
}

/// Builds `project` (written into `dir`) as the golden tests do and checks
/// its run against `data`.
fn build_and_check(project: &Value, dir: &Path, cache: &Path, data: &RunData) -> Result<(), String> {
    let path = dir.join("project.b2c");
    std::fs::write(&path, serde_json::to_vec_pretty(project).unwrap()).unwrap();
    build_project(&path, cache).and_then(|executable| data.check(&executable))
}

/// Panics with every collected failure, if any.
fn finish(failures: &[String]) {
    assert!(
        failures.is_empty(),
        "{} failure(s):\n--- {}",
        failures.len(),
        failures.join("\n--- ")
    );
}

/// The `.cpp` files that include a standard header (`#include <…>`) and
/// whether each holds `using namespace std;` on a line of its own.
fn directives(generated: &GeneratedProject) -> Vec<(String, bool)> {
    generated
        .files
        .iter()
        .filter(|file| file.kind == FileKind::Source)
        .filter(|file| file.contents.lines().any(|line| line.starts_with("#include <")))
        .map(|file| {
            let directive = file
                .contents
                .lines()
                .any(|line| line.trim_start() == "using namespace std;");
            (file.path.clone(), directive)
        })
        .collect()
}

#[test]
fn every_example_gives_the_same_output_with_textbook_style_on_and_off() {
    if common::gxx().is_none() {
        return;
    }
    let cache = tempfile::tempdir().unwrap();
    let mut failures = Vec::new();
    let mut names: Vec<String> = std::fs::read_dir(repo_root().join("tests/golden"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    names.sort();
    assert!(names.len() >= 10, "{names:?}");
    for name in names {
        let bytes = std::fs::read(repo_root().join("examples").join(format!("{name}.b2c"))).unwrap();
        let project: Value = serde_json::from_slice(&bytes).unwrap();
        let on = !textbook_style(&project);
        let changed = with_textbook_style(&project, on);
        let generated = match generate(&changed) {
            Ok(generated) => generated,
            Err(problem) => {
                failures.push(format!("{name} with Textbook style {on}: {problem}"));
                continue;
            }
        };
        let files = directives(&generated);
        assert!(!files.is_empty(), "{name}");
        // A *use namespace std* block writes the directive whatever the
        // setting; no example has one.
        for (path, directive) in files {
            if directive != on {
                failures.push(format!(
                    "{name} with Textbook style {on}: {path} has `using namespace std;`: {directive}"
                ));
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let data = RunData::read(&repo_root().join("tests/golden").join(&name)).unwrap();
        if let Err(problem) = build_and_check(&changed, dir.path(), cache.path(), &data) {
            failures.push(format!("{name} with Textbook style {on}: {problem}"));
        }
    }
    finish(&failures);
}

// ---- the lookup cases ----------------------------------------------------------------

/// How a case nominates `std`.
#[derive(Debug, Clone, Copy)]
enum Directive {
    None,
    TextbookStyle,
    Block,
}

/// One program of blocks and what it prints.
struct Case {
    name: &'static str,
    /// The modules' top-level blocks, `main` first.
    modules: Vec<(&'static str, Value)>,
    output: &'static str,
    /// Text each variant's generated code must hold, as `(file, text)`,
    /// where the spelling is the point of the case.
    spelled: Vec<(&'static str, &'static str)>,
    /// The same, for the variants that nominate `std` only.
    nominated: Vec<(&'static str, &'static str)>,
}

impl Case {
    /// The case as a project for `standard` with `directive`.
    fn project(&self, standard: &str, directive: Directive) -> Value {
        let modules: Vec<Value> = self
            .modules
            .iter()
            .enumerate()
            .map(|(index, (name, blocks))| {
                let mut blocks = blocks.clone();
                if index == 0 && matches!(directive, Directive::Block) {
                    blocks.as_array_mut().unwrap().push(json!({
                        "id": "b_use_std", "type": "program.use_namespace", "v": 1,
                        "x": 40, "y": 2000, "fields": {"NS": "::std"}
                    }));
                }
                json!({"id": format!("mod_{name}"), "name": name, "workspace": {"blocks": blocks}})
            })
            .collect();
        let project = json!({
            "format": "blocks2cpp/project",
            "formatVersion": 2,
            "generator": {"app": "0.1.0", "catalog": "1.3.0"},
            "project": {
                "id": "prj_lookup_case",
                "name": self.name,
                "language": {"standard": standard},
            },
            "modules": modules,
        });
        with_textbook_style(&project, matches!(directive, Directive::TextbookStyle))
    }
}

/// A value input of expression tokens.
fn tokens(tokens: Value) -> Value {
    json!({"expr": tokens})
}

/// A call `function(arguments…)` as tokens; each argument is a token list.
fn call(function: &str, arguments: &[Value]) -> Value {
    let mut out = vec![json!({"ref": function}), json!({"op": "("})];
    for (index, argument) in arguments.iter().enumerate() {
        if index > 0 {
            out.push(json!({"op": ","}));
        }
        out.extend(argument.as_array().unwrap().iter().cloned());
    }
    out.push(json!({"op": ")"}));
    tokens(Value::Array(out))
}

/// `print` (block `id`) of `items`, separated by spaces.
fn print(id: &str, items: Vec<Value>) -> Value {
    let count = items.len();
    let inputs: serde_json::Map<String, Value> = items
        .into_iter()
        .enumerate()
        .map(|(index, item)| (format!("ITEM{index}"), item))
        .collect();
    json!({"id": id, "type": "io.print", "v": 1, "extra": {"itemCount": count},
           "fields": {"NEWLINE": true, "SEP": "space", "STREAM": "out"}, "inputs": inputs})
}

/// `define name(int params…) -> int` (block `fn_<sym>`) returning `value`.
fn define(sym: &str, name: &str, params: &[(&str, &str)], value: Value) -> Value {
    let params: Vec<Value> = params
        .iter()
        .map(|(sym, name)| json!({"sym": sym, "name": name, "type": "int", "mode": "copy"}))
        .collect();
    json!({"id": format!("fn_{sym}"), "type": "func.define", "v": 1,
    "fields": {"NAME": {"sym": sym, "name": name}, "RETURNS": "int"},
    "extra": {"params": params},
    "statements": {"BODY": [
        {"id": format!("ret_{sym}"), "type": "func.return", "v": 1, "inputs": {"VALUE": value}}
    ]}})
}

/// `block` with *share with other modules* ticked.
fn shared(mut block: Value) -> Value {
    block["fields"]["SHARED"] = json!(true);
    block
}

/// A global `int` variable.
fn global(sym: &str, name: &str, value: i64) -> Value {
    json!({"id": format!("g_{sym}"), "type": "var.global", "v": 1,
           "fields": {"KIND": "variable", "NAME": {"sym": sym, "name": name}, "TYPE": "int"},
           "inputs": {"VALUE": tokens(json!([{"num": value.to_string()}]))}})
}

/// A *namespace* block.
fn namespace(sym: &str, name: &str, definitions: Vec<Value>) -> Value {
    json!({"id": format!("b_{sym}"), "type": "program.namespace", "v": 1,
           "fields": {"NAME": {"sym": sym, "name": name}},
           "statements": {"DEFINITIONS": definitions}})
}

fn main_block(body: Vec<Value>) -> Value {
    json!({"id": "b_main", "type": "program.main", "v": 1, "statements": {"BODY": body}})
}

#[allow(clippy::too_many_lines)] // literal test programs, clearest in one piece
fn cases() -> Vec<Case> {
    vec![
        // 06 §6.14.2: a user function named like a standard one is called
        // `::max(…)` under a directive, and a global named like one is read
        // and written `::count`; the *larger of* block keeps `std::max`.
        Case {
            name: "max and count",
            modules: vec![(
                "main",
                json!([
                    global("s_count", "count", 0),
                    define(
                        "s_max",
                        "max",
                        &[("s_a", "a"), ("s_b", "b")],
                        tokens(
                            json!([{"ref": "s_a"}, {"op": "*"}, {"num": "100"}, {"op": "+"}, {"ref": "s_b"}])
                        ),
                    ),
                    main_block(vec![
                        json!({"id": "b_set", "type": "var.set", "v": 1, "fields": {"VAR": {"ref": "s_count"}},
                               "inputs": {"VALUE": call("s_max", &[json!([{"num": "3"}]), json!([{"num": "7"}])])}}),
                        print(
                            "b_print",
                            vec![
                                tokens(json!([{"ref": "s_count"}])),
                                json!({"block": {"id": "b_larger", "type": "math.minmax", "v": 1,
                                             "fields": {"FN": "max"},
                                             "inputs": {"A": tokens(json!([{"num": "2.5"}])),
                                                        "B": tokens(json!([{"num": "1.5"}]))}}}),
                            ]
                        ),
                    ]),
                ]),
            )],
            output: "307 2.5\n",
            spelled: vec![("main.cpp", "max(3, 7)"), ("main.cpp", "std::max")],
            nominated: vec![("main.cpp", "::max(3, 7)"), ("main.cpp", "::count = ")],
        },
        // ADR-0006: common learner names that the standard library declares
        // (`size`, `left`, `next`, `distance`, `sort`) as globals and
        // functions.
        Case {
            name: "common names",
            modules: vec![(
                "main",
                json!([
                    global("s_size", "size", 3),
                    global("s_left", "left", 4),
                    define(
                        "s_next",
                        "next",
                        &[("s_n", "n")],
                        tokens(json!([{"ref": "s_n"}, {"op": "+"}, {"num": "1"}]))
                    ),
                    define(
                        "s_distance",
                        "distance",
                        &[("s_x", "x"), ("s_y", "y")],
                        tokens(json!([{"ref": "s_y"}, {"op": "-"}, {"ref": "s_x"}]))
                    ),
                    define(
                        "s_sort",
                        "sort",
                        &[("s_v", "v")],
                        tokens(json!([{"ref": "s_v"}, {"op": "*"}, {"num": "2"}]))
                    ),
                    main_block(vec![print(
                        "b_print",
                        vec![
                            tokens(json!([{"ref": "s_size"}])),
                            tokens(json!([{"ref": "s_left"}])),
                            call("s_next", &[json!([{"ref": "s_size"}])]),
                            call(
                                "s_distance",
                                &[json!([{"ref": "s_size"}]), json!([{"ref": "s_left"}])]
                            ),
                            call("s_sort", &[json!([{"num": "5"}])]),
                        ]
                    )]),
                ]),
            )],
            output: "3 4 4 1 10\n",
            spelled: vec![],
            nominated: vec![],
        },
        // 06 §6.14.6: a global `describe` called with a `geo::Point` is
        // `::describe(p)`, since argument-dependent lookup also finds
        // `geo::describe`, with or without a directive.
        Case {
            name: "argument-dependent lookup",
            modules: vec![(
                "main",
                json!([
                    namespace(
                        "s_geo",
                        "geo",
                        vec![
                            json!({"id": "b_point", "type": "type.struct", "v": 1,
                               "extra": {"fields": [{"name": "x", "sym": "s_point_x", "type": "int"}]},
                               "fields": {"COMPARABLE": false, "NAME": {"sym": "s_Point", "name": "Point"},
                                          "PRINTABLE": false}}),
                            json!({"id": "fn_geo_describe", "type": "func.define", "v": 1,
                               "fields": {"NAME": {"sym": "s_geo_describe", "name": "describe"}, "RETURNS": "int"},
                               "extra": {"params": [{"sym": "s_gp", "name": "p", "type": {"ref": "s_Point"},
                                                     "mode": "read_only"}]},
                               "statements": {"BODY": [{"id": "ret_geo_describe", "type": "func.return", "v": 1,
                                   "inputs": {"VALUE": {"block": {"id": "b_gx", "type": "type.field", "v": 1,
                                       "fields": {"MEMBER": {"ref": "s_point_x"}},
                                       "inputs": {"OBJ": tokens(json!([{"ref": "s_gp"}]))}}}}}]}}),
                        ]
                    ),
                    json!({"id": "fn_describe", "type": "func.define", "v": 1,
                           "fields": {"NAME": {"sym": "s_describe", "name": "describe"}, "RETURNS": "int"},
                           "extra": {"params": [{"sym": "s_p", "name": "p", "type": {"ref": "s_Point"},
                                                 "mode": "read_only"}]},
                           "statements": {"BODY": [{"id": "ret_describe", "type": "func.return", "v": 1,
                               "inputs": {"VALUE": {"block": {"id": "b_times", "type": "math.arithmetic", "v": 1,
                                   "fields": {"OP": "mul"},
                                   "inputs": {"A": {"block": {"id": "b_x", "type": "type.field", "v": 1,
                                                    "fields": {"MEMBER": {"ref": "s_point_x"}},
                                                    "inputs": {"OBJ": tokens(json!([{"ref": "s_p"}]))}}},
                                              "B": tokens(json!([{"num": "10"}]))}}}}}]}}),
                    main_block(vec![
                        json!({"id": "b_declare", "type": "var.declare", "v": 1,
                               "fields": {"CONST": false, "NAME": {"sym": "s_pt", "name": "point"},
                                          "TYPE": {"ref": "s_Point"}},
                               "inputs": {"VALUE": {"block": {"id": "b_new", "type": "type.new", "v": 1,
                                   "extra": {"fieldCount": 1}, "fields": {"TYPE": {"ref": "s_Point"}},
                                   "inputs": {"FIELD0": tokens(json!([{"num": "4"}]))}}}}}),
                        print(
                            "b_print",
                            vec![
                                call("s_describe", &[json!([{"ref": "s_pt"}])]),
                                call("s_geo_describe", &[json!([{"ref": "s_pt"}])]),
                            ]
                        ),
                    ]),
                ]),
            )],
            output: "40 4\n",
            spelled: vec![
                ("main.cpp", "::describe(point)"),
                ("main.cpp", "geo::describe(point)"),
            ],
            nominated: vec![],
        },
        // 06 §6.14.2: inside `geo`, its `size` hides the global one, which
        // is `::size()` there.
        Case {
            name: "a hidden global",
            modules: vec![(
                "main",
                json!([
                    define("s_size", "size", &[], tokens(json!([{"num": "1"}]))),
                    namespace(
                        "s_geo",
                        "geo",
                        vec![
                            define("s_geo_size", "size", &[], tokens(json!([{"num": "10"}]))),
                            define(
                                "s_area",
                                "area",
                                &[("s_w", "w")],
                                tokens(json!([
                                    {"ref": "s_w"}, {"op": "*"}, {"ref": "s_geo_size"}, {"op": "("}, {"op": ")"},
                                    {"op": "+"}, {"ref": "s_size"}, {"op": "("}, {"op": ")"}
                                ]))
                            ),
                        ]
                    ),
                    main_block(vec![print(
                        "b_print",
                        vec![
                            call("s_area", &[json!([{"num": "3"}])]),
                            call("s_size", &[]),
                            call("s_geo_size", &[]),
                        ]
                    )]),
                ]),
            )],
            output: "31 1 10\n",
            spelled: vec![("main.cpp", "::size()")],
            nominated: vec![],
        },
        // 06 §6.14.3: *use namespace geo* where `geo` is declared further
        // down: `namespace geo {}` comes first; one module and two.
        Case {
            name: "a directive before its namespace",
            modules: vec![(
                "main",
                json!([
                    json!({"id": "b_use_geo", "type": "program.use_namespace", "v": 1, "fields": {"NS": "s_geo"}}),
                    main_block(vec![print(
                        "b_print",
                        vec![call("s_twice", &[json!([{"num": "21"}])])]
                    )]),
                    namespace(
                        "s_geo",
                        "geo",
                        vec![define(
                            "s_twice",
                            "twice",
                            &[("s_n", "n")],
                            tokens(json!([{"ref": "s_n"}, {"op": "*"}, {"num": "2"}]))
                        )]
                    ),
                ]),
            )],
            output: "42\n",
            spelled: vec![("main.cpp", "namespace geo {}"), ("main.cpp", "twice(21)")],
            nominated: vec![],
        },
        Case {
            name: "a directive before its namespace, two modules",
            modules: vec![
                (
                    "main",
                    json!([main_block(vec![print(
                        "b_print",
                        vec![call("s_shown", &[json!([{"num": "5"}])]),]
                    )])]),
                ),
                (
                    "shapes",
                    json!([
                        json!({"id": "b_use_geo", "type": "program.use_namespace", "v": 1, "fields": {"NS": "s_geo"}}),
                        shared(define(
                            "s_shown",
                            "shown",
                            &[("s_k", "k")],
                            call("s_triple", &[json!([{"ref": "s_k"}])])
                        )),
                        namespace(
                            "s_geo",
                            "geo",
                            vec![define(
                                "s_triple",
                                "triple",
                                &[("s_t", "t")],
                                tokens(json!([{"ref": "s_t"}, {"op": "*"}, {"num": "3"}]))
                            )]
                        ),
                    ]),
                ),
            ],
            output: "15\n",
            spelled: vec![("shapes.cpp", "namespace geo {}"), ("shapes.cpp", "triple(k)")],
            nominated: vec![],
        },
    ]
}

#[test]
fn the_lookup_cases_print_the_same_with_and_without_a_directive() {
    let mut failures = Vec::new();
    let compiler = common::gxx().is_some();
    let cache = tempfile::tempdir().unwrap();
    for case in cases() {
        let run_data = tempfile::tempdir().unwrap();
        std::fs::write(run_data.path().join("stdin.txt"), "").unwrap();
        std::fs::write(run_data.path().join("stdout.txt"), case.output).unwrap();
        std::fs::write(run_data.path().join("exit_code.txt"), "0\n").unwrap();
        let data = RunData::read(run_data.path()).unwrap();
        for standard in ["c++17", "c++20"] {
            for directive in [Directive::None, Directive::TextbookStyle, Directive::Block] {
                let what = format!("{} ({standard}, {directive:?})", case.name);
                let project = case.project(standard, directive);
                let generated = match generate(&project) {
                    Ok(generated) => generated,
                    Err(problem) => {
                        failures.push(format!("{what}: {problem}"));
                        continue;
                    }
                };
                let nominated: &[(&str, &str)] = if matches!(directive, Directive::None) {
                    &[]
                } else {
                    &case.nominated
                };
                for (path, text) in case.spelled.iter().chain(nominated) {
                    let file = generated.files.iter().find(|file| file.path == *path).unwrap();
                    if !file.contents.contains(text) {
                        failures.push(format!("{what}: {path} lacks {text}:\n{}", file.contents));
                    }
                }
                let writes_directive = generated.files.iter().any(|file| {
                    file.contents
                        .lines()
                        .any(|line| line.trim_start() == "using namespace std;")
                });
                if writes_directive == matches!(directive, Directive::None) {
                    failures.push(format!(
                        "{what}: `using namespace std;` written: {writes_directive}"
                    ));
                }
                if compiler {
                    let dir = tempfile::tempdir().unwrap();
                    if let Err(problem) = build_and_check(&project, dir.path(), cache.path(), &data) {
                        failures.push(format!("{what}: {problem}"));
                    }
                }
            }
        }
    }
    finish(&failures);
}
