//! Namespaces in the generated code, W3's package N2
//! (`docs/plans/m3-contracts/w3-modules.md` §1.4, §1.7, §2.12, §5.2): each
//! reference printed as the analyser spelled it (`area`, `::size`,
//! `geo::area`, `geo::flat::f`), the user types of declarations and values
//! too; each `.cpp`'s directives with their source-map ranges, after the
//! empty declaration of a namespace no included header declares; standard
//! names written short after `using namespace std;`; and one namespace
//! spread over two modules. Each program goes through the stages a build
//! runs, and is compiled and run with every g++.

// The block builders take their parts by value, as `json!` reads them.
#![allow(clippy::needless_pass_by_value)]

use b2c_ir::source_map::GeneratedProject;
use serde_json::{Value, json};

use super::tests::compilers;
use super::w3_tests::{analyse, compile_and_run, generate, render};

/// A project of these modules (`(id, name, blocks)`), with *Textbook
/// style* on or off.
fn project(modules: &[(&str, &str, Value)], textbook: bool) -> Value {
    let modules: Vec<Value> = modules
        .iter()
        .map(|(id, name, blocks)| json!({"id": id, "name": name, "workspace": {"blocks": blocks}}))
        .collect();
    let mut project = json!({"id": "prj_shapes", "name": "Shapes", "language": {"standard": "c++17"}});
    if textbook {
        project["options"] = json!({"usingNamespaceStd": true});
    }
    json!({
        "format": "blocks2cpp/project",
        "formatVersion": if textbook { 2 } else { 1 },
        "generator": {"app": "0.1.0", "catalog": "1.0.0"},
        "project": project,
        "modules": modules
    })
}

/// A *namespace* block `name` (symbol `ns_<name>`) holding `definitions`.
fn namespace(name: &str, definitions: Vec<Value>) -> Value {
    json!({"id": format!("b_{name}"), "type": "program.namespace", "v": 1,
           "fields": {"NAME": {"sym": format!("ns_{name}"), "name": name}},
           "statements": {"DEFINITIONS": definitions}})
}

/// `define <name>` (block `fn_<sym>`) giving `ret`, with `int` inputs
/// `(symbol, name)` taken as copies, its body `return <value>`.
fn define(sym: &str, name: &str, params: &[(&str, &str, Value)], value: Value) -> Value {
    let params: Vec<Value> = params
        .iter()
        .map(|(sym, name, ty)| json!({"sym": sym, "name": name, "type": ty, "mode": "copy"}))
        .collect();
    json!({"id": format!("fn_{sym}"), "type": "func.define", "v": 1,
    "fields": {"NAME": {"sym": sym, "name": name}, "RETURNS": "int"},
    "extra": {"params": params},
    "statements": {"BODY": [
        {"id": format!("ret_{sym}"), "type": "func.return", "v": 1, "inputs": {"VALUE": value}}
    ]}})
}

fn shared(mut block: Value) -> Value {
    block["fields"]["SHARED"] = json!(true);
    block
}

/// A value made of expression tokens.
fn tokens(tokens: Value) -> Value {
    json!({"expr": tokens})
}

/// `print` (block `id`) of `items`, separated by spaces.
fn print(id: &str, items: Vec<Value>) -> Value {
    let count = items.len();
    let inputs: serde_json::Map<String, Value> = items
        .into_iter()
        .enumerate()
        .map(|(i, item)| (format!("ITEM{i}"), item))
        .collect();
    json!({"id": id, "type": "io.print", "v": 1, "extra": {"itemCount": count},
           "fields": {"NEWLINE": true, "SEP": "space", "STREAM": "out"}, "inputs": inputs})
}

/// `use namespace <value> in this file` (block `id`).
fn uses(id: &str, value: &str) -> Value {
    json!({"id": id, "type": "program.use_namespace", "v": 1, "fields": {"NS": value}})
}

fn main_block(body: Vec<Value>) -> Value {
    json!({"id": "b_main", "type": "program.main", "v": 1, "statements": {"BODY": body}})
}

/// Compiles and runs a project with every g++, if there is one.
fn runs(project: &GeneratedProject, output: &str) {
    let compilers = compilers();
    if compilers.is_empty() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    compile_and_run(project, &compilers, dir.path(), "", output);
}

/// The text of the file `path`.
fn text<'p>(project: &'p GeneratedProject, path: &str) -> &'p str {
    &project
        .files
        .iter()
        .find(|file| file.path == path)
        .unwrap()
        .contents
}

/// The blocks the ranges starting on the line holding `needle` map to, in
/// the file `path`.
fn blocks_on(project: &GeneratedProject, path: &str, needle: &str) -> Vec<String> {
    let contents = text(project, path);
    let line = contents
        .lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle} not in\n{contents}"));
    let map = project
        .source_map
        .files
        .iter()
        .find(|file| file.path == path)
        .unwrap();
    map.ranges
        .iter()
        .filter(|range| range.start.line as usize == line + 1)
        .map(|range| range.block.as_str().to_owned())
        .collect()
}

/// 06 §6.14.2: outside `geo` its members are `geo::…` (a call, a global
/// read and written, a struct value, an enum value, the type of a local);
/// inside it they are plain, and the global `size`, which `geo::size`
/// hides there, is `::size()`. A global outside `geo` has the type
/// `geo::Point`.
#[test]
fn references_are_written_as_the_analyser_spelled_them() {
    let geo = namespace(
        "geo",
        vec![
            json!({"id": "b_point", "type": "type.struct", "v": 1,
                   "fields": {"NAME": {"sym": "s_Point", "name": "Point"}, "COMPARABLE": false, "PRINTABLE": false},
                   "extra": {"fields": [{"sym": "s_point_x", "name": "x", "type": "int"}]}}),
            json!({"id": "b_colour", "type": "type.enum", "v": 1,
                   "fields": {"NAME": {"sym": "s_Colour", "name": "Colour"}},
                   "extra": {"values": [{"name": "Red", "sym": "s_Red"}, {"name": "Green", "sym": "s_Green"}]}}),
            json!({"id": "gl_count", "type": "var.global", "v": 1,
                   "fields": {"KIND": "variable", "TYPE": "int", "NAME": {"sym": "s_count", "name": "count"}},
                   "inputs": {"VALUE": {"expr": [{"num": "0"}]}}}),
            json!({"id": "gl_origin", "type": "var.global", "v": 1,
                   "fields": {"KIND": "variable", "TYPE": {"ref": "s_Point"},
                              "NAME": {"sym": "s_origin", "name": "origin"}}}),
            define("g_size", "size", &[], tokens(json!([{"num": "10"}]))),
            define(
                "g_area",
                "area",
                &[("p_w", "w", json!("int"))],
                tokens(
                    json!([{"ref": "p_w"}, {"op": "*"}, {"ref": "g_size"}, {"op": "("}, {"op": ")"},
                              {"op": "+"}, {"ref": "t_size"}, {"op": "("}, {"op": ")"}]),
                ),
            ),
            define(
                "g_shift",
                "shift",
                &[("p_p", "p", json!({"ref": "s_Point"}))],
                json!({"block": {"id": "b_shift_sum", "type": "math.arithmetic", "v": 1, "fields": {"OP": "add"},
                    "inputs": {
                        "A": {"block": {"id": "b_px", "type": "type.field", "v": 1,
                            "fields": {"MEMBER": {"ref": "s_point_x"}}, "inputs": {"OBJ": {"expr": [{"ref": "p_p"}]}}}},
                        "B": {"expr": [{"ref": "s_count"}]}}}}),
            ),
        ],
    );
    let program = project(
        &[(
            "mod_main",
            "main",
            json!([
                main_block(vec![
                    json!({"id": "b_set", "type": "var.set", "v": 1, "fields": {"VAR": {"ref": "s_count"}},
                           "inputs": {"VALUE": {"expr": [{"num": "5"}]}}}),
                    json!({"id": "b_change", "type": "var.change", "v": 1,
                           "fields": {"VAR": {"ref": "s_count"}, "OP": "add"},
                           "inputs": {"BY": {"expr": [{"num": "1"}]}}}),
                    json!({"id": "b_p", "type": "var.declare", "v": 1,
                           "fields": {"CONST": false, "NAME": {"sym": "s_p", "name": "p"}, "TYPE": {"ref": "s_Point"}},
                           "inputs": {"VALUE": {"block": {"id": "b_new", "type": "type.new", "v": 1,
                               "extra": {"fieldCount": 1}, "fields": {"TYPE": {"ref": "s_Point"}},
                               "inputs": {"FIELD0": {"expr": [{"num": "2"}]}}}}}}),
                    json!({"id": "b_c", "type": "var.declare", "v": 1,
                           "fields": {"CONST": false, "NAME": {"sym": "s_c", "name": "c"}, "TYPE": {"ref": "s_Colour"}},
                           "inputs": {"VALUE": {"block": {"id": "b_green", "type": "type.enum_value", "v": 1,
                               "fields": {"ENUM": {"ref": "s_Colour"}, "VALUE": {"ref": "s_Green"}}}}}}),
                    print(
                        "b_print",
                        vec![
                            tokens(json!([{"ref": "g_area"}, {"op": "("}, {"num": "3"}, {"op": ")"}])),
                            tokens(json!([{"ref": "s_count"}])),
                            tokens(json!([{"ref": "t_size"}, {"op": "("}, {"op": ")"}])),
                            tokens(json!([{"ref": "g_shift"}, {"op": "("}, {"ref": "s_p"}, {"op": ")"}])),
                            tokens(
                                json!([{"ref": "g_shift"}, {"op": "("}, {"ref": "s_origin"}, {"op": ")"}])
                            ),
                            tokens(json!([{"ref": "g_shift"}, {"op": "("}, {"ref": "s_start"}, {"op": ")"}])),
                            tokens(json!([{"ref": "s_c"}])),
                        ],
                    ),
                ]),
                define("t_size", "size", &[], tokens(json!([{"num": "1"}]))),
                json!({"id": "gl_start", "type": "var.global", "v": 1,
                       "fields": {"KIND": "variable", "TYPE": {"ref": "s_Point"},
                                  "NAME": {"sym": "s_start", "name": "start"}}}),
                geo
            ]),
        )],
        false,
    );
    let generated = generate(&analyse(&program, &[]));
    insta::assert_snapshot!("n2_references", render(&generated));
    let main = text(&generated, "main.cpp");
    for spelled in [
        "geo::count = 5;",
        "++geo::count;",
        "geo::Point p = geo::Point{2};",
        "geo::Colour c = geo::Colour::Green;",
        "geo::area(3)",
        "geo::shift(geo::origin)",
        "return w * size() + ::size();",
        "Point origin{};",
        "geo::Point start{};",
        "geo::shift(start)",
    ] {
        assert!(main.contains(spelled), "{spelled} not in\n{main}");
    }
    runs(&generated, "31 6 1 8 6 6 Green\n");
}

/// Namespaces in namespaces: a member of `geo::flat` is written with both
/// outside them, and qualified with its full path from `geo` too.
#[test]
fn nested_namespaces_are_written_with_their_full_path() {
    let program = project(
        &[(
            "mod_main",
            "main",
            json!([
                main_block(vec![print(
                    "b_print",
                    vec![
                        tokens(json!([{"ref": "f_f"}, {"op": "("}, {"op": ")"}])),
                        tokens(json!([{"ref": "f_g"}, {"op": "("}, {"op": ")"}])),
                    ],
                )]),
                namespace(
                    "geo",
                    vec![
                        namespace(
                            "flat",
                            vec![define("f_f", "f", &[], tokens(json!([{"num": "7"}])))]
                        ),
                        define(
                            "f_g",
                            "g",
                            &[],
                            tokens(
                                json!([{"ref": "f_f"}, {"op": "("}, {"op": ")"}, {"op": "+"}, {"num": "1"}])
                            ),
                        ),
                    ],
                ),
            ]),
        )],
        false,
    );
    let generated = generate(&analyse(&program, &[]));
    insta::assert_snapshot!("n2_nested", render(&generated));
    let main = text(&generated, "main.cpp");
    assert!(main.contains("geo::flat::f() << \" \" << geo::g()"), "{main}");
    assert!(main.contains("namespace geo {\nnamespace flat {"), "{main}");
    runs(&generated, "7 8\n");
}

/// The blocks of a single-module program that nominates `geo` and `std`,
/// printing `geo::area()`.
fn directives_program(std_block: bool, textbook: bool) -> Value {
    let mut blocks = vec![
        main_block(vec![print(
            "b_print",
            vec![
                json!({"block": {"id": "b_text", "type": "text.literal", "v": 1, "fields": {"VALUE": "area"}}}),
                tokens(json!([{"ref": "g_area"}, {"op": "("}, {"op": ")"}])),
            ],
        )]),
        uses("b_use_geo", "ns_geo"),
        namespace(
            "geo",
            vec![define("g_area", "area", &[], tokens(json!([{"num": "4"}])))],
        ),
    ];
    if std_block {
        blocks.push(uses("b_use_std", "::std"));
    }
    project(&[("mod_main", "main", json!(blocks))], textbook)
}

/// 06 §6.14.3: a single-module `main.cpp` declares `geo` empty, then writes
/// its directives at global scope after the includes, each line mapped to
/// its block (Textbook style's to none); after them `cout` is short and
/// `area` plain.
#[test]
fn directives_of_a_single_module() {
    let generated = generate(&analyse(&directives_program(true, false), &[]));
    insta::assert_snapshot!("n2_directives_single", render(&generated));
    let main = text(&generated, "main.cpp");
    assert!(
        main.contains("namespace geo {}\n\nusing namespace std;\nusing namespace geo;\n"),
        "{main}"
    );
    assert!(
        main.contains("cout << \"area\" << \" \" << area() << '\\n';"),
        "{main}"
    );
    assert!(!main.contains("std::"), "{main}");
    assert_eq!(
        blocks_on(&generated, "main.cpp", "namespace geo {}"),
        ["b_use_geo"]
    );
    assert_eq!(
        blocks_on(&generated, "main.cpp", "using namespace std;"),
        ["b_use_std"]
    );
    assert_eq!(
        blocks_on(&generated, "main.cpp", "using namespace geo;"),
        ["b_use_geo"]
    );
    runs(&generated, "area 4\n");

    // Textbook style writes the same code; its line maps to no block.
    let textbook = generate(&analyse(&directives_program(false, true), &[]));
    assert_eq!(text(&textbook, "main.cpp"), main);
    assert!(blocks_on(&textbook, "main.cpp", "using namespace std;").is_empty());
}

/// Textbook style writes no directive in a file without a standard header;
/// a *use namespace `::std`* block writes it with `<cstddef>`.
#[test]
fn std_without_a_standard_header() {
    let program = |block: bool, textbook: bool| {
        let mut blocks = vec![main_block(vec![])];
        if block {
            blocks.push(uses("b_use_std", "::std"));
        }
        project(&[("mod_main", "main", json!(blocks))], textbook)
    };
    let textbook = generate(&analyse(&program(false, true), &[]));
    let main = text(&textbook, "main.cpp");
    assert!(!main.contains("using") && !main.contains("#include"), "{main}");
    let block = generate(&analyse(&program(true, false), &[]));
    let main = text(&block, "main.cpp");
    assert!(
        main.contains("#include <cstddef>\n\nusing namespace std;\n\nint main() {"),
        "{main}"
    );
    assert_eq!(
        blocks_on(&block, "main.cpp", "using namespace std;"),
        ["b_use_std"]
    );
    runs(&block, "");
}

/// One namespace in two modules is one C++ namespace (w3-modules.md
/// §2.12): `main` shares `geo::area`, `shapes` shares `geo::perimeter` and
/// keeps `geo::helper` in `namespace geo { namespace { … } }`. `main.cpp`
/// nominates `geo`, which its headers declare, so it declares nothing
/// empty and writes its directives as the first lines of an
/// anonymous-namespace block; with Textbook style, `shapes.cpp`, which
/// includes no standard header, writes no `using namespace std;`.
#[test]
fn a_namespace_spread_over_two_modules() {
    let main = json!([
        main_block(vec![print(
            "b_print",
            vec![
                tokens(json!([{"ref": "g_area"}, {"op": "("}, {"num": "2"}, {"op": ")"}])),
                tokens(json!([{"ref": "g_perimeter"}, {"op": "("}, {"num": "3"}, {"op": ")"}])),
            ],
        )]),
        uses("b_use_geo", "ns_geo"),
        namespace(
            "geo",
            vec![shared(define(
                "g_area",
                "area",
                &[("p_w", "w", json!("int"))],
                tokens(json!([{"ref": "p_w"}, {"op": "*"}, {"num": "2"}])),
            ))],
        ),
    ]);
    let shapes = json!([json!({"id": "b_geo2", "type": "program.namespace", "v": 1,
    "fields": {"NAME": {"sym": "ns_geo2", "name": "geo"}},
    "statements": {"DEFINITIONS": [
        shared(define(
            "g_perimeter",
            "perimeter",
            &[("p_s", "s", json!("int"))],
            tokens(json!([{"ref": "g_helper"}, {"op": "("}, {"ref": "p_s"}, {"op": ")"}, {"op": "*"}, {"num": "4"}])),
        )),
        define("g_helper", "helper", &[("p_h", "h", json!("int"))], tokens(json!([{"ref": "p_h"}]))),
    ]}})]);
    for textbook in [false, true] {
        let program = project(
            &[
                ("mod_main", "main", main.clone()),
                ("mod_shapes", "shapes", shapes.clone()),
            ],
            textbook,
        );
        let generated = generate(&analyse(&program, &[]));
        let name = if textbook {
            "n2_two_modules_textbook"
        } else {
            "n2_two_modules"
        };
        insta::assert_snapshot!(name, render(&generated));
        let main_cpp = text(&generated, "main.cpp");
        assert!(!main_cpp.contains("namespace geo {}"), "{main_cpp}");
        let directives = if textbook {
            "namespace {\n\nusing namespace std;\nusing namespace geo;\n"
        } else {
            "namespace {\n\nusing namespace geo;\n"
        };
        assert!(main_cpp.contains(directives), "{main_cpp}");
        assert!(
            main_cpp.contains("area(2) << \" \" << perimeter(3)"),
            "{main_cpp}"
        );
        let shapes_cpp = text(&generated, "shapes.cpp");
        assert!(!shapes_cpp.contains("using"), "{shapes_cpp}");
        assert!(
            shapes_cpp.contains("namespace geo {\nnamespace {"),
            "{shapes_cpp}"
        );
        assert_eq!(
            blocks_on(&generated, "main.cpp", "using namespace geo;"),
            ["b_use_geo"]
        );
        runs(&generated, "4 12\n");
    }
}

/// A header never has a directive (06 §6.14.3): a shared function's
/// signature names `geo::Point` in full in `shapes.hpp`, although
/// `shapes.cpp` nominates `geo`, where its body writes `Point` plainly.
#[test]
fn a_header_spells_without_the_directives_of_its_cpp() {
    let shapes = json!([
        uses("b_use_geo", "ns_geo"),
        namespace(
            "geo",
            vec![shared(json!({"id": "b_point", "type": "type.struct", "v": 1,
                "fields": {"NAME": {"sym": "s_Point", "name": "Point"}, "COMPARABLE": false, "PRINTABLE": false},
                "extra": {"fields": [{"sym": "s_point_x", "name": "x", "type": "int"}]}}))],
        ),
        shared(json!({"id": "fn_twice", "type": "func.define", "v": 1,
        "fields": {"NAME": {"sym": "s_twice", "name": "twice"}, "RETURNS": "int"},
        "extra": {"params": [{"sym": "p_p", "name": "p", "type": {"ref": "s_Point"}, "mode": "copy"}]},
        "statements": {"BODY": [
            {"id": "b_copy", "type": "var.declare", "v": 1,
             "fields": {"CONST": false, "NAME": {"sym": "s_q", "name": "q"}, "TYPE": {"ref": "s_Point"}},
             "inputs": {"VALUE": {"expr": [{"ref": "p_p"}]}}},
            {"id": "b_ret", "type": "func.return", "v": 1, "inputs": {"VALUE":
                {"block": {"id": "b_sum", "type": "math.arithmetic", "v": 1, "fields": {"OP": "mul"},
                 "inputs": {
                    "A": {"block": {"id": "b_qx", "type": "type.field", "v": 1,
                        "fields": {"MEMBER": {"ref": "s_point_x"}}, "inputs": {"OBJ": {"expr": [{"ref": "s_q"}]}}}},
                    "B": {"expr": [{"num": "2"}]}}}}}}
        ]}})),
    ]);
    let main = json!([main_block(vec![
        json!({"id": "b_pt", "type": "var.declare", "v": 1,
               "fields": {"CONST": false, "NAME": {"sym": "s_pt", "name": "pt"}, "TYPE": {"ref": "s_Point"}},
               "inputs": {"VALUE": {"block": {"id": "b_new", "type": "type.new", "v": 1,
                   "extra": {"fieldCount": 1}, "fields": {"TYPE": {"ref": "s_Point"}},
                   "inputs": {"FIELD0": {"expr": [{"num": "3"}]}}}}}}),
        print(
            "b_print",
            vec![tokens(
                json!([{"ref": "s_twice"}, {"op": "("}, {"ref": "s_pt"}, {"op": ")"}])
            )],
        ),
    ])]);
    let program = project(
        &[("mod_main", "main", main), ("mod_shapes", "shapes", shapes)],
        false,
    );
    let generated = generate(&analyse(&program, &[]));
    insta::assert_snapshot!("n2_header_without_directives", render(&generated));
    let header = text(&generated, "shapes.hpp");
    assert!(header.contains("int twice(geo::Point p);"), "{header}");
    let source = text(&generated, "shapes.cpp");
    assert!(
        source.contains("int twice(geo::Point p) {\n    Point q = p;"),
        "{source}"
    );
    runs(&generated, "6\n");
}

/// A shared `int` global `<kind> <name> = <value>` (block `b_<sym>`).
fn shared_global(sym: &str, name: &str, kind: &str, value: Value) -> Value {
    json!({"id": format!("b_{sym}"), "type": "var.global", "v": 1,
           "fields": {"KIND": kind, "TYPE": "int", "NAME": {"sym": sym, "name": name}, "SHARED": true},
           "inputs": {"VALUE": {"expr": value}}})
}

/// A header is spelled for every `.cpp` that includes it, which may have
/// included any module's header before it (w3-modules.md §7.1): in
/// `zeta.hpp`, inside `geo`, the default value of `geo::f` reads the
/// global `limit` as `::limit`, because `main.cpp` includes `alpha.hpp`,
/// which shares `geo::limit`, first, although `zeta.cpp` does not.
#[test]
fn a_header_is_spelled_for_every_file_that_includes_it() {
    let alpha = json!([namespace(
        "geo",
        vec![shared_global(
            "g_limit",
            "limit",
            "constant",
            json!([{"num": "7"}])
        )],
    )]);
    let zeta = json!([
        shared_global("t_limit", "limit", "constant", json!([{"num": "3"}])),
        {"id": "b_geo2", "type": "program.namespace", "v": 1,
         "fields": {"NAME": {"sym": "ns_geo2", "name": "geo"}},
         "statements": {"DEFINITIONS": [
            {"id": "fn_g_f", "type": "func.define", "v": 1,
             "fields": {"NAME": {"sym": "g_f", "name": "f"}, "RETURNS": "int", "SHARED": true},
             "extra": {"params": [{"mode": "copy", "name": "x", "sym": "p_x", "type": "int"}]},
             "inputs": {"DEFAULT0": {"expr": [{"ref": "t_limit"}]}},
             "statements": {"BODY": [
                {"id": "ret_g_f", "type": "func.return", "v": 1, "inputs": {"VALUE": {"expr": [{"ref": "p_x"}]}}}
             ]}}
         ]}}
    ]);
    let main = json!([main_block(vec![print(
        "b_print",
        vec![
            tokens(json!([{"ref": "g_f"}, {"op": "("}, {"op": ")"}])),
            tokens(json!([{"ref": "g_limit"}])),
        ],
    )])]);
    let program = project(
        &[
            ("mod_main", "main", main),
            ("mod_alpha", "alpha", alpha),
            ("mod_zeta", "zeta", zeta),
        ],
        false,
    );
    let generated = generate(&analyse(&program, &[]));
    let header = text(&generated, "zeta.hpp");
    assert!(header.contains("int f(int x = ::limit);"), "{header}");
    runs(&generated, "3 7\n");
}

/// 06 §6.14.2: `::geo::area` when `geo` itself is contested. Inside
/// `shapes`, which has a `geo` of its own, the top-level `geo::area` is
/// `::geo::area`; `geo::area` would call `shapes::geo::area`.
#[test]
fn a_contested_namespace_is_written_from_the_global_namespace() {
    let program = project(
        &[(
            "mod_main",
            "main",
            json!([
                main_block(vec![print(
                    "b_print",
                    vec![
                        tokens(json!([{"ref": "s_g"}, {"op": "("}, {"op": ")"}])),
                        tokens(json!([{"ref": "s_area"}, {"op": "("}, {"op": ")"}])),
                    ],
                )]),
                namespace(
                    "geo",
                    vec![define("g_area", "area", &[], tokens(json!([{"num": "1"}])))]
                ),
                namespace(
                    "shapes",
                    vec![
                        json!({"id": "b_geo_in", "type": "program.namespace", "v": 1,
                        "fields": {"NAME": {"sym": "ns_geo_in", "name": "geo"}},
                        "statements": {"DEFINITIONS": [
                            define("s_area", "area", &[], tokens(json!([{"num": "2"}])))
                        ]}}),
                        define(
                            "s_g",
                            "g",
                            &[],
                            tokens(json!([{"ref": "g_area"}, {"op": "("}, {"op": ")"}]))
                        ),
                    ],
                ),
            ]),
        )],
        false,
    );
    let generated = generate(&analyse(&program, &[]));
    let main = text(&generated, "main.cpp");
    assert!(main.contains("return ::geo::area();"), "{main}");
    assert!(
        main.contains("shapes::g() << \" \" << shapes::geo::area()"),
        "{main}"
    );
    runs(&generated, "1 2\n");
}

/// A shared global that is not `constexpr` gets its starting value in its
/// module's `.cpp`, after the directives, so the value is spelled with
/// them: `calc.cpp` uses `geo`, which `places` shares a `limit` of, so
/// `start` starts at `::limit`.
#[test]
fn a_shared_globals_starting_value_is_spelled_with_the_directives() {
    let places = json!([namespace(
        "geo",
        vec![shared_global(
            "g_limit",
            "limit",
            "variable",
            json!([{"num": "7"}])
        )],
    )]);
    let calc = json!([
        uses("b_use_geo", "ns_geo"),
        shared_global("t_limit", "limit", "variable", json!([{"num": "3"}])),
        shared_global("t_start", "start", "variable", json!([{"ref": "t_limit"}])),
    ]);
    let main = json!([main_block(vec![print(
        "b_print",
        vec![
            tokens(json!([{"ref": "t_start"}])),
            tokens(json!([{"ref": "g_limit"}])),
        ],
    )])]);
    let program = project(
        &[
            ("mod_main", "main", main),
            ("mod_calc", "calc", calc),
            ("mod_places", "places", places),
        ],
        false,
    );
    let generated = generate(&analyse(&program, &[]));
    let source = text(&generated, "calc.cpp");
    assert!(source.contains("int start = ::limit;"), "{source}");
    runs(&generated, "3 7\n");
}
