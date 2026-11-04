//! Functions across modules, W3's package F
//! (`docs/plans/m3-contracts/w3-modules.md` §1.2, §2.1 to §2.4, §5.2): which
//! modules see which functions and in which order, the codes that guard
//! shared functions (`B2C-E0206`, `B2C-E0211`, `B2C-E0213`, `B2C-E0214`,
//! `B2C-E0421`, `B2C-E0422`), the layout of headers and includes, and
//! properties of random programs of two to five modules: generation is
//! deterministic, headers never include each other when nothing is
//! reported, a body edit never changes a header, the module order changes
//! no file, and, with g++, each header compiles alone and the program links.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::needless_pass_by_value,
    clippy::too_many_lines
)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use b2c_codegen::{CodegenOptions, generate};
use b2c_ir::Severity;
use b2c_ir::diag::{Diagnostic, Part};
use b2c_ir::ids::{BlockId, ModuleId};
use b2c_ir::layout::Layout;
use b2c_ir::sast::{ItemKind, Program};
use b2c_ir::source_map::{FileKind, GeneratedProject};
use common::{analyze, assert_clean, doc, doc_modules, render_all};
use proptest::prelude::*;
use serde_json::{Value, json};

// --- Builders ---------------------------------------------------------------------

/// A reference to a symbol in a slot.
fn r(sym: &str) -> Value {
    json!({"ref": sym})
}

fn op(text: &str) -> Value {
    json!({"op": text})
}

fn n(text: &str) -> Value {
    json!({"num": text})
}

/// A slot calling `sym` with these argument tokens, each argument one token.
fn call(sym: &str, args: &[Value]) -> Value {
    let mut tokens = vec![r(sym), op("(")];
    for (index, arg) in args.iter().enumerate() {
        if index > 0 {
            tokens.push(op(","));
        }
        tokens.push(arg.clone());
    }
    tokens.push(op(")"));
    json!({"expr": tokens})
}

/// `return <value>`.
fn ret(value: Value) -> Value {
    json!({"id": format!("ret_{}", common::id()), "type": "func.return", "v": 1, "inputs": {"VALUE": value}})
}

/// `print <value>`.
fn print(value: Value) -> Value {
    json!({"id": format!("print_{}", common::id()), "type": "io.print", "v": 1, "extra": {"itemCount": 1},
           "fields": {"NEWLINE": true, "SEP": "none", "STREAM": "out"}, "inputs": {"ITEM0": value}})
}

/// `when program starts` with these statements.
fn main_block(body: Vec<Value>) -> Value {
    json!({"id": "main", "type": "program.main", "v": 1, "statements": {"BODY": body}})
}

/// `define <name>` declaring `sym`, with block ID `fn_<sym>`: inputs as
/// `(symbol, name, type)` taken as copies, the result type, the body, and
/// the *share with other modules* tick.
fn define(
    sym: &str,
    name: &str,
    ret_type: Value,
    params: &[(&str, &str, Value)],
    body: Vec<Value>,
    shared: bool,
) -> Value {
    let params: Vec<Value> = params
        .iter()
        .map(|(sym, name, ty)| json!({"sym": sym, "name": name, "type": ty, "mode": "copy"}))
        .collect();
    json!({
        "id": format!("fn_{sym}"), "type": "func.define", "v": 1,
        "fields": {"NAME": {"sym": sym, "name": name}, "RETURNS": ret_type, "SHARED": shared},
        "extra": {"params": params},
        "statements": {"BODY": body}
    })
}

/// `define <name>` returning `int` with no input, its body `return 1`.
fn simple(sym: &str, name: &str, shared: bool) -> Value {
    define(
        sym,
        name,
        json!("int"),
        &[],
        vec![ret(json!({"expr": [n("1")]}))],
        shared,
    )
}

/// `define <name>(int x = <default>)` returning `int`, its body `return x`.
fn with_default(sym: &str, name: &str, default: Value, shared: bool) -> Value {
    let x = format!("{sym}_x");
    let mut block = define(
        sym,
        name,
        json!("int"),
        &[(x.as_str(), "x", json!("int"))],
        vec![ret(json!({"expr": [r(&x)]}))],
        shared,
    );
    block["inputs"] = json!({"DEFAULT0": default});
    block
}

/// `struct <name>` with one `int` field.
fn structure(sym: &str, name: &str, shared: bool) -> Value {
    json!({"id": format!("st_{sym}"), "type": "type.struct", "v": 1,
           "fields": {"NAME": {"sym": sym, "name": name}, "SHARED": shared},
           "extra": {"fields": [{"sym": format!("{sym}_f"), "name": "size", "type": "int"}]}})
}

/// `enum <name>` with one value.
fn enumeration(sym: &str, name: &str, shared: bool) -> Value {
    json!({"id": format!("en_{sym}"), "type": "type.enum", "v": 1,
           "fields": {"NAME": {"sym": sym, "name": name}, "SHARED": shared},
           "extra": {"values": [{"sym": format!("{sym}_v"), "name": "First"}]}})
}

/// The diagnostics with a code.
fn with_code<'a>(analysis: &'a b2c_lang::Analysis, code: &str) -> Vec<&'a Diagnostic> {
    analysis.diagnostics.iter().filter(|d| d.code.0 == code).collect()
}

/// A diagnostic's related locations as `(module, block, note)`.
fn related(d: &Diagnostic) -> Vec<(String, String, String)> {
    d.related
        .iter()
        .map(|related| {
            (
                related
                    .location
                    .module
                    .as_ref()
                    .map(ModuleId::to_string)
                    .unwrap_or_default(),
                related
                    .location
                    .block
                    .as_ref()
                    .map(BlockId::to_string)
                    .unwrap_or_default(),
                related.message.clone(),
            )
        })
        .collect()
}

/// Where a diagnostic is: `(module, block)`.
fn at(d: &Diagnostic) -> (String, String) {
    (
        d.primary
            .module
            .as_ref()
            .map(ModuleId::to_string)
            .unwrap_or_default(),
        d.primary
            .block
            .as_ref()
            .map(BlockId::to_string)
            .unwrap_or_default(),
    )
}

/// The names of the symbols the scope query lists at a block.
fn scope(analysis: &b2c_lang::Analysis, block: &str) -> Vec<String> {
    analysis
        .symbols_in_scope(&BlockId::new(block).unwrap(), None)
        .into_iter()
        .map(|info| info.id.to_string())
        .collect()
}

/// The module IDs of a list of a layout.
fn ids(list: &[ModuleId]) -> Vec<&str> {
    list.iter().map(ModuleId::as_str).collect()
}

// --- Visibility and precedence ------------------------------------------------------

#[test]
fn a_module_sees_its_own_functions_then_the_others_shared_ones() {
    let document = doc_modules(vec![
        (
            "main",
            vec![
                main_block(vec![print(call("s_area_utils", &[]))]),
                simple("s_total", "total", false),
            ],
        ),
        (
            "utils",
            vec![
                simple("s_area_utils", "area", true),
                simple("s_hidden", "hidden", false),
                simple("s_total_utils", "total_all", true),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    assert_clean(&analysis);
    // `main` sees its own `total`, and `utils`' shared `area` and
    // `total_all`; not `hidden`.
    let print = analysis.program.modules[0]
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Main(main) => main.body.stmts.first().map(|stmt| stmt.origin.block.to_string()),
            _ => None,
        });
    assert_eq!(
        scope(&analysis, &print.unwrap()),
        ["s_area_utils", "s_total", "s_total_utils"]
    );
    // The parked scope of `main` too, and `utils` sees all of its own.
    let parked: Vec<String> = analysis
        .parked_scope(&ModuleId::new("mod_0").unwrap())
        .into_iter()
        .map(|info| info.id.to_string())
        .collect();
    assert_eq!(parked, ["s_area_utils", "s_total", "s_total_utils"]);
    assert_eq!(
        scope(&analysis, "fn_s_hidden"),
        ["s_area_utils", "s_hidden", "s_total_utils"]
    );
    let shared: Vec<String> = analysis
        .symbol_infos()
        .into_iter()
        .filter(|info| info.shared)
        .map(|info| info.id.to_string())
        .collect();
    assert_eq!(shared, ["s_area_utils", "s_total_utils"]);
}

#[test]
fn a_module_s_own_definition_hides_another_module_s_shared_one() {
    // `main` has a non-shared `area` (which is `B2C-E0213`, W3M-D10), so
    // its menus, typed names and pastes find its own `area`, never
    // `geometry`'s (w3-modules.md §2.1).
    let document = doc_modules(vec![
        (
            "main",
            vec![
                main_block(vec![print(call("s_area_main", &[]))]),
                simple("s_area_main", "area", false),
            ],
        ),
        (
            "geometry",
            vec![
                simple("s_area_geo", "area", true),
                simple("s_perimeter", "perimeter", true),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    let codes: Vec<&str> = analysis.diagnostics.iter().map(|d| d.code.0.as_str()).collect();
    assert_eq!(codes, ["B2C-E0213"], "{}", render_all(&analysis));
    assert_eq!(scope(&analysis, "fn_s_area_main"), ["s_area_main", "s_perimeter"]);
    let parked: Vec<String> = analysis
        .parked_scope(&ModuleId::new("mod_0").unwrap())
        .into_iter()
        .map(|info| info.id.to_string())
        .collect();
    assert_eq!(parked, ["s_area_main", "s_perimeter"]);
    // `geometry` sees its own `area`, not `main`'s, which is not shared.
    assert_eq!(scope(&analysis, "fn_s_area_geo"), ["s_area_geo", "s_perimeter"]);
}

// --- B2C-E0206 and B2C-E0211 ---------------------------------------------------------

#[test]
fn e0206_names_the_module_that_does_not_share_the_function() {
    let document = doc_modules(vec![
        (
            "main",
            vec![main_block(vec![
                print(call("s_helper", &[])),
                print(call("s_tool", &[])),
            ])],
        ),
        (
            "utils",
            vec![
                simple("s_helper", "helper", false),
                simple("s_tool", "tool", true),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    let found = with_code(&analysis, "B2C-E0206");
    assert_eq!(found.len(), 1, "{}", render_all(&analysis));
    assert_eq!(
        found[0].message,
        "`helper` belongs to module `utils`, which does not share it. To use it here, tick 'share with other \
         modules' on its 'define' block."
    );
    assert_eq!(
        found[0].primary.module.as_ref().map(ModuleId::as_str),
        Some("mod_0")
    );
    assert_eq!(
        related(found[0]),
        [(
            String::from("mod_1"),
            String::from("fn_s_helper"),
            String::from("`helper` is created here")
        )]
    );
    // The shared one is called like one of the module's own.
    assert_eq!(analysis.diagnostics.len(), 1, "{}", render_all(&analysis));
}

#[test]
fn e0211_refuses_two_shared_functions_with_one_name_in_two_modules() {
    let document = doc_modules(vec![
        ("main", vec![main_block(vec![])]),
        ("report", vec![simple("s_show_report", "show", true)]),
        (
            "table",
            vec![define(
                "s_show_table",
                "show",
                json!("int"),
                &[("s_show_n", "n", json!("double"))],
                vec![ret(json!({"expr": [n("2")]}))],
                true,
            )],
        ),
    ]);
    let analysis = analyze(&document);
    let found = with_code(&analysis, "B2C-E0211");
    assert_eq!(found.len(), 1, "{}", render_all(&analysis));
    // Even with other inputs: an overload set lives in one module.
    assert_eq!(
        found[0].message,
        "`show` is already shared by module `report`. Overloads of a shared function must be in one module, so \
         rename this one."
    );
    assert_eq!(
        at(found[0]),
        (String::from("mod_2"), String::from("fn_s_show_table"))
    );
    assert_eq!(
        found[0].primary.part,
        Part::Field {
            name: String::from("NAME")
        }
    );
    assert_eq!(
        related(found[0]),
        [(
            String::from("mod_1"),
            String::from("fn_s_show_report"),
            String::from("the other function")
        )]
    );
}

#[test]
fn functions_that_are_not_shared_may_share_a_name_across_modules() {
    let scale = |sym: &str| {
        define(
            sym,
            "scale",
            json!("double"),
            &[(&format!("{sym}_v"), "value", json!("double"))],
            vec![ret(json!({"expr": [r(&format!("{sym}_v")), op("*"), n("2")]}))],
            false,
        )
    };
    let document = doc_modules(vec![
        (
            "main",
            vec![
                main_block(vec![print(call("s_scale_main", &[n("1.5")]))]),
                scale("s_scale_main"),
            ],
        ),
        (
            "convert",
            vec![
                scale("s_scale_convert"),
                with_default("s_rounded", "rounded", call("s_twice", &[]), true),
                simple("s_twice", "twice", false),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    // `twice`, not shared, in a default value of the shared `rounded` is
    // `B2C-E0214`; the two `scale`s are fine.
    let codes: Vec<&str> = analysis.diagnostics.iter().map(|d| d.code.0.as_str()).collect();
    assert_eq!(codes, ["B2C-E0214"], "{}", render_all(&analysis));
    let document = doc_modules(vec![
        (
            "main",
            vec![
                main_block(vec![print(call("s_scale_main", &[n("1.5")]))]),
                scale("s_scale_main"),
            ],
        ),
        ("convert", vec![scale("s_scale_convert")]),
    ]);
    assert_clean(&analyze(&document));
}

// --- B2C-E0213 -------------------------------------------------------------------------

/// `convert` shares `to_celsius` and `scale`; `table` shares `print_table`,
/// whose default value calls `convert`'s `to_celsius`, so `table.hpp`
/// includes `convert.hpp`. `main`'s body is `main_body`; `main` also has a
/// `scale` it does not share.
fn clash(main_body: Vec<Value>) -> b2c_lang::Analysis {
    analyze(&doc_modules(vec![
        (
            "main",
            vec![main_block(main_body), simple("s_scale_main", "scale", false)],
        ),
        (
            "convert",
            vec![
                simple("s_to_celsius", "to_celsius", true),
                simple("s_scale_convert", "scale", true),
            ],
        ),
        (
            "table",
            vec![with_default(
                "s_print_table",
                "print_table",
                call("s_to_celsius", &[]),
                true,
            )],
        ),
    ]))
}

#[test]
fn e0213_is_program_wide_and_names_no_cause_without_an_include() {
    let analysis = clash(vec![print(call("s_scale_main", &[]))]);
    let found = with_code(&analysis, "B2C-E0213");
    assert_eq!(found.len(), 1, "{}", render_all(&analysis));
    assert_eq!(analysis.diagnostics.len(), 1, "{}", render_all(&analysis));
    assert_eq!(
        found[0].message,
        "`scale` is also shared by module `convert`. A function that is not shared can't have the name of a \
         shared function of any module: once this file includes `convert.hpp`, C++ could not tell the two \
         apart. Rename this `scale`."
    );
    assert_eq!(
        at(found[0]),
        (String::from("mod_0"), String::from("fn_s_scale_main"))
    );
    assert_eq!(
        found[0].primary.part,
        Part::Field {
            name: String::from("NAME")
        }
    );
    assert_eq!(
        related(found[0]),
        [(
            String::from("mod_1"),
            String::from("fn_s_scale_convert"),
            String::from("the shared `scale`")
        )]
    );
}

#[test]
fn e0213_names_the_use_that_includes_the_header() {
    // An unrelated use of `convert` added to `main`: the untouched `scale`'s
    // message now names it.
    let analysis = clash(vec![
        print(call("s_scale_main", &[])),
        print(call("s_to_celsius", &[])),
    ]);
    let found = with_code(&analysis, "B2C-E0213");
    assert_eq!(found.len(), 1, "{}", render_all(&analysis));
    assert_eq!(
        found[0].message,
        "`scale` is also shared by module `convert`, whose header `convert.hpp` this file includes because it \
         uses `to_celsius`. Rename this `scale`."
    );
    let notes = related(found[0]);
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert_eq!(notes[0].0, "mod_0");
    assert_eq!(notes[0].2, "this file uses `to_celsius` of module `convert` here");
    assert_eq!(
        notes[1],
        (
            String::from("mod_1"),
            String::from("fn_s_scale_convert"),
            String::from("the shared `scale`")
        )
    );
    assert_eq!(
        analysis.program.layout.modules[0].source_includes,
        [ModuleId::new("mod_1").unwrap()]
    );
}

#[test]
fn e0213_names_the_chain_of_headers_through_a_second_one() {
    // `main` uses only `table`, whose header includes `convert.hpp`.
    let analysis = clash(vec![
        print(call("s_scale_main", &[])),
        print(call("s_print_table", &[])),
    ]);
    let found = with_code(&analysis, "B2C-E0213");
    assert_eq!(found.len(), 1, "{}", render_all(&analysis));
    assert_eq!(
        found[0].message,
        "`scale` is also shared by module `convert`, whose header `convert.hpp` this file includes through \
         `table.hpp`, which it includes because it uses `print_table`. Rename this `scale`."
    );
    let notes = related(found[0]);
    assert_eq!(notes.len(), 3, "{notes:?}");
    assert_eq!(
        (notes[0].0.as_str(), notes[0].2.as_str()),
        ("mod_0", "this file uses `print_table` of module `table` here")
    );
    assert_eq!(
        (notes[1].0.as_str(), notes[1].1.as_str(), notes[1].2.as_str()),
        (
            "mod_2",
            "fn_s_print_table",
            "`table.hpp` includes `convert.hpp` for the default value of `print_table`"
        )
    );
    assert_eq!(notes[2].2, "the shared `scale`");
    let layout = &analysis.program.layout;
    assert_eq!(ids(&layout.modules[0].source_includes), ["mod_2"]);
    assert_eq!(ids(&layout.modules[2].header_includes), ["mod_1"]);
}

#[test]
fn e0213_for_a_name_the_module_itself_shares() {
    let document = doc_modules(vec![
        ("main", vec![main_block(vec![])]),
        (
            "utils",
            vec![
                simple("s_pick_shared", "pick", true),
                define(
                    "s_pick_own",
                    "pick",
                    json!("int"),
                    &[("s_pick_n", "n", json!("int"))],
                    vec![ret(json!({"expr": [r("s_pick_n")]}))],
                    false,
                ),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    let found = with_code(&analysis, "B2C-E0213");
    assert_eq!(found.len(), 1, "{}", render_all(&analysis));
    assert_eq!(
        found[0].message,
        "`pick` is also the name of a function this module shares, and C++ could not tell the two apart. Rename \
         one of them, or share this one too."
    );
    assert_eq!(
        at(found[0]),
        (String::from("mod_1"), String::from("fn_s_pick_own"))
    );
}

// --- B2C-E0214 -------------------------------------------------------------------------

#[test]
fn e0214_for_what_a_shared_function_s_header_shows() {
    let document = doc_modules(vec![
        ("main", vec![main_block(vec![])]),
        (
            "shapes",
            vec![
                structure("s_Box", "Box", false),
                enumeration("s_Kind", "Kind", false),
                structure("s_Shown", "Shown", true),
                simple("s_unit", "unit", false),
                // An input of a struct that is not shared.
                define(
                    "s_area",
                    "area",
                    json!("int"),
                    &[
                        ("s_box", "box", json!({"ref": "s_Box"})),
                        ("s_shown", "shown", json!({"ref": "s_Shown"})),
                    ],
                    vec![ret(json!({"expr": [n("1")]}))],
                    true,
                ),
                // A list of them, and a result of an enum that is not shared.
                define(
                    "s_kind_of",
                    "kind_of",
                    json!({"ref": "s_Kind"}),
                    &[("s_boxes", "boxes", json!({"std::vector": {"ref": "s_Box"}}))],
                    vec![ret(
                        json!({"block": {"id": "b_first", "type": "type.enum_value", "v": 1,
                        "fields": {"ENUM": {"ref": "s_Kind"}, "VALUE": {"ref": "s_Kind_v"}}}}),
                    )],
                    true,
                ),
                // A default value calling a function that is not shared.
                with_default("s_scaled", "scaled", call("s_unit", &[]), true),
                // A body may use all of them.
                define(
                    "s_count",
                    "count",
                    json!("int"),
                    &[],
                    vec![
                        print(call("s_unit", &[])),
                        print(
                            json!({"block": {"id": "b_kind", "type": "type.enum_value", "v": 1,
                            "fields": {"ENUM": {"ref": "s_Kind"}, "VALUE": {"ref": "s_Kind_v"}}}}),
                        ),
                        ret(json!({"expr": [n("1")]})),
                    ],
                    true,
                ),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    let found: Vec<(String, String, String)> = with_code(&analysis, "B2C-E0214")
        .into_iter()
        .map(|d| {
            let part = match &d.primary.part {
                Part::Field { name } => name.clone(),
                Part::Tokens { input, start, end } => format!("{input}[{start}..{end}]"),
                other => format!("{other:?}"),
            };
            (
                d.primary
                    .block
                    .as_ref()
                    .map(BlockId::to_string)
                    .unwrap_or_default(),
                part,
                d.message.clone(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            (
                String::from("fn_s_area"),
                String::from("params[0]"),
                String::from(
                    "`area` is shared, but its input `box` has the type `Box`, which is not. Share `Box` too."
                )
            ),
            (
                String::from("fn_s_kind_of"),
                String::from("params[0]"),
                String::from(
                    "`kind_of` is shared, but the type of its input `boxes` uses `Box`, which is not. Share `Box` too."
                )
            ),
            (
                String::from("fn_s_kind_of"),
                String::from("RETURNS"),
                String::from(
                    "`kind_of` is shared, but its result has the type `Kind`, which is not. Share `Kind` too."
                )
            ),
            (
                String::from("fn_s_scaled"),
                String::from("DEFAULT0[0..3]"),
                String::from(
                    "`scaled` is shared, but the default value of its input `x` uses `unit`, which is not. Share \
                     `unit` too."
                )
            ),
        ],
        "{}",
        render_all(&analysis)
    );
    let first = with_code(&analysis, "B2C-E0214")[0];
    assert_eq!(
        related(first),
        [(
            String::from("mod_1"),
            String::from("st_s_Box"),
            String::from("`Box` is defined here, not shared")
        )]
    );
    assert_eq!(analysis.diagnostics.len(), 4, "{}", render_all(&analysis));
}

// --- B2C-E0421 and B2C-E0422 ------------------------------------------------------

#[test]
fn e0421_only_for_shared_functions_whose_default_values_call_each_other() {
    let document = doc_modules(vec![
        ("main", vec![main_block(vec![])]),
        ("a", vec![with_default("s_f", "f", call("s_g", &[]), true)]),
        ("b", vec![with_default("s_g", "g", call("s_f", &[]), true)]),
    ]);
    let analysis = analyze(&document);
    let codes: Vec<&str> = analysis.diagnostics.iter().map(|d| d.code.0.as_str()).collect();
    assert_eq!(codes, ["B2C-E0421", "B2C-E0421"], "{}", render_all(&analysis));
    assert_eq!(
        analysis.diagnostics[0].message,
        "These definitions each need the next one first, in a circle that C++ cannot put in order: `f` needs `g` \
         needs `f`. Change one of them so that it no longer needs the next."
    );
    assert_eq!(at(&analysis.diagnostics[0]).0, "mod_1");
    assert_eq!(at(&analysis.diagnostics[1]).0, "mod_2");
}

#[test]
fn e0422_for_headers_that_would_include_each_other_without_a_circle_of_definitions() {
    let document = doc_modules(vec![
        ("main", vec![main_block(vec![])]),
        (
            "a",
            vec![
                with_default("s_f", "f", call("s_g", &[]), true),
                simple("s_k", "k", true),
            ],
        ),
        (
            "b",
            vec![
                simple("s_g", "g", true),
                with_default("s_h", "h", call("s_k", &[]), true),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    let found = with_code(&analysis, "B2C-E0422");
    assert_eq!(found.len(), 2, "{}", render_all(&analysis));
    assert_eq!(analysis.diagnostics.len(), 2, "{}", render_all(&analysis));
    assert_eq!(
        found[0].message,
        "`a.hpp` would need `b.hpp`, because the default value of `f` uses `g` of module `b`, but `b.hpp` already \
         needs `a.hpp`. Two headers can't each include the other. Move `f` into module `b`, or `h` into module \
         `a`."
    );
    assert_eq!(at(found[0]), (String::from("mod_1"), String::from("fn_s_f")));
    assert_eq!(
        related(found[0]),
        [(
            String::from("mod_2"),
            String::from("fn_s_h"),
            String::from("`b.hpp` needs `a.hpp` for the default value of `h`")
        )]
    );
    assert_eq!(at(found[1]), (String::from("mod_2"), String::from("fn_s_h")));
}

#[test]
fn e0422_names_the_way_round_a_circle_of_three_headers() {
    let document = doc_modules(vec![
        ("main", vec![main_block(vec![])]),
        (
            "a",
            vec![
                with_default("s_f", "f", call("s_g", &[]), true),
                simple("s_k", "k", true),
            ],
        ),
        (
            "b",
            vec![
                simple("s_g", "g", true),
                with_default("s_h", "h", call("s_m", &[]), true),
            ],
        ),
        (
            "c",
            vec![
                simple("s_m", "m", true),
                with_default("s_p", "p", call("s_k", &[]), true),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    let found = with_code(&analysis, "B2C-E0422");
    assert_eq!(found.len(), 3, "{}", render_all(&analysis));
    assert_eq!(
        found[0].message,
        "`a.hpp` would need `b.hpp`, because the default value of `f` uses `g` of module `b`, but `b.hpp` already \
         needs `a.hpp` through `c.hpp`. Two headers can't each include the other. Move one of these shared \
         functions into another module."
    );
    assert_eq!(related(found[0]).len(), 2);
}

#[test]
fn e0422_on_each_shared_function_whose_default_value_closes_the_circle() {
    let document = doc_modules(vec![
        ("main", vec![main_block(vec![])]),
        (
            "a",
            vec![
                with_default("s_f", "f", call("s_g", &[]), true),
                with_default("s_f2", "f2", call("s_g", &[]), true),
                simple("s_k", "k", true),
            ],
        ),
        (
            "b",
            vec![
                simple("s_g", "g", true),
                with_default("s_h", "h", call("s_k", &[]), true),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    let found = with_code(&analysis, "B2C-E0422");
    assert_eq!(analysis.diagnostics.len(), 3, "{}", render_all(&analysis));
    let places: Vec<(String, String)> = found.iter().map(|d| at(d)).collect();
    assert_eq!(
        places,
        [
            (String::from("mod_1"), String::from("fn_s_f")),
            (String::from("mod_1"), String::from("fn_s_f2")),
            (String::from("mod_2"), String::from("fn_s_h")),
        ]
    );
    // Moving `f` alone into `b` would leave `f2`'s need, so no single move
    // is named.
    for diagnostic in &found {
        assert!(
            diagnostic
                .message
                .ends_with("Move one of these shared functions into another module."),
            "{}",
            diagnostic.message
        );
    }
}

#[test]
fn e0422_for_a_circle_of_headers_beside_a_circle_of_functions() {
    // `f` and `g` call each other (`B2C-E0421`); `b.hpp` → `c.hpp` → `a.hpp`
    // → `b.hpp` through `h`, `p` and `f` is a circle of headers no circle of
    // functions explains.
    let document = doc_modules(vec![
        ("main", vec![main_block(vec![])]),
        (
            "a",
            vec![
                with_default("s_f", "f", call("s_g", &[]), true),
                simple("s_k", "k", true),
            ],
        ),
        (
            "b",
            vec![
                with_default("s_g", "g", call("s_f", &[]), true),
                with_default("s_h", "h", call("s_m", &[]), true),
            ],
        ),
        (
            "c",
            vec![
                simple("s_m", "m", true),
                with_default("s_p", "p", call("s_k", &[]), true),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    let cycles: Vec<(String, String)> = with_code(&analysis, "B2C-E0421").iter().map(|d| at(d)).collect();
    assert_eq!(
        cycles,
        [
            (String::from("mod_1"), String::from("fn_s_f")),
            (String::from("mod_2"), String::from("fn_s_g")),
        ],
        "{}",
        render_all(&analysis)
    );
    // Only the needs the circle of functions does not explain.
    let headers: Vec<(String, String)> = with_code(&analysis, "B2C-E0422").iter().map(|d| at(d)).collect();
    assert_eq!(
        headers,
        [
            (String::from("mod_2"), String::from("fn_s_h")),
            (String::from("mod_3"), String::from("fn_s_p")),
        ],
        "{}",
        render_all(&analysis)
    );
    assert_eq!(analysis.diagnostics.len(), 4, "{}", render_all(&analysis));
}

#[test]
fn e0421_once_for_a_circle_within_a_module_that_a_wider_circle_holds() {
    // `f` and `g` call each other in module `a` (the module's own
    // `B2C-E0421`), and `g` → `h` → `f` crosses into `b`.
    let document = doc_modules(vec![
        ("main", vec![main_block(vec![])]),
        (
            "a",
            vec![
                with_default("s_f", "f", call("s_g", &[]), true),
                with_default(
                    "s_g",
                    "g",
                    json!({"expr": [r("s_f"), op("("), op(")"), op("+"), r("s_h"), op("("), op(")")]}),
                    true,
                ),
            ],
        ),
        ("b", vec![with_default("s_h", "h", call("s_f", &[]), true)]),
    ]);
    let analysis = analyze(&document);
    let codes: Vec<&str> = analysis.diagnostics.iter().map(|d| d.code.0.as_str()).collect();
    assert_eq!(codes, ["B2C-E0421"; 3], "{}", render_all(&analysis));
    let blocks: BTreeSet<(String, String)> = analysis.diagnostics.iter().map(at).collect();
    assert_eq!(
        blocks,
        BTreeSet::from([
            (String::from("mod_1"), String::from("fn_s_f")),
            (String::from("mod_1"), String::from("fn_s_g")),
            (String::from("mod_2"), String::from("fn_s_h")),
        ])
    );
}

// --- main and the layout ----------------------------------------------------------------

#[test]
fn when_program_starts_may_be_in_any_module() {
    let document = doc_modules(vec![
        ("tools", vec![simple("s_tool", "tool", true)]),
        ("main", vec![main_block(vec![print(call("s_tool", &[]))])]),
    ]);
    let analysis = analyze(&document);
    assert_clean(&analysis);
    let project = generate(&analysis.program, &CodegenOptions::default());
    let paths: Vec<&str> = project.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, ["tools.cpp", "tools.hpp", "main.cpp"]);
    let main = &project.files[2].contents;
    assert!(main.contains("#include \"tools.hpp\"\n"), "{main}");
    assert!(main.trim_end().ends_with("return 0;\n}"), "{main}");
    assert!(!project.files[0].contents.contains("int main"));
}

#[test]
fn the_layout_lists_headers_includes_and_the_header_order() {
    let document = doc_modules(vec![
        (
            "main",
            vec![
                main_block(vec![print(call("s_print_table", &[]))]),
                simple("s_local", "local", false),
            ],
        ),
        (
            "convert",
            vec![
                simple("s_to_f", "to_fahrenheit", true),
                simple("s_to_c", "to_celsius", true),
                with_default("s_rounded", "rounded", json!({"expr": [n("1")]}), true),
                simple("s_scale", "scale", false),
            ],
        ),
        (
            "table",
            vec![
                with_default("s_print_table", "print_table", call("s_rounded", &[]), true),
                simple("s_scale_t", "scale", false),
            ],
        ),
        ("empty", vec![]),
    ]);
    let analysis = analyze(&document);
    assert_clean(&analysis);
    let layout = &analysis.program.layout;
    let names = |module: usize, list: &[usize]| -> Vec<String> {
        list.iter()
            .map(
                |index| match &analysis.program.modules[module].items[*index].kind {
                    ItemKind::Function(def) => analysis
                        .program
                        .symbols
                        .get(&def.symbol)
                        .unwrap()
                        .name
                        .as_str()
                        .to_owned(),
                    other => format!("{other:?}"),
                },
            )
            .collect()
    };
    let [main, convert, table, empty] = &layout.modules[..] else {
        panic!("four modules")
    };
    assert!(!main.header && main.header_order.is_empty());
    assert_eq!(ids(&main.source_includes), ["mod_2"]);
    assert!(convert.header);
    assert_eq!(
        names(1, &convert.header_order),
        ["rounded", "to_celsius", "to_fahrenheit"]
    );
    assert_eq!(ids(&convert.source_includes), ["mod_1"], "its own header, always");
    assert!(convert.header_includes.is_empty());
    assert_eq!(names(2, &table.header_order), ["print_table"]);
    assert_eq!(ids(&table.header_includes), ["mod_1"], "for the default value");
    assert_eq!(
        ids(&table.source_includes),
        ["mod_2"],
        "the default value is printed in the header only"
    );
    assert!(!empty.header && empty.source_includes.is_empty());
}

#[test]
fn a_single_module_program_has_no_header_and_shares_nothing() {
    let document = doc(vec![
        main_block(vec![print(call("s_tool", &[]))]),
        simple("s_tool", "tool", true),
        with_default("s_other", "other", call("s_tool", &[]), true),
    ]);
    let analysis = analyze(&document);
    assert_clean(&analysis);
    assert!(analysis.program.modules[0].items.iter().all(|item| !item.shared));
    let files = &analysis.program.layout.modules[0];
    assert!(!files.header && files.header_order.is_empty() && files.source_includes.is_empty());
    let project = generate(&analysis.program, &CodegenOptions::default());
    assert_eq!(project.files.len(), 1);
    assert!(
        project.files[0].contents.contains("int other(int x = tool());"),
        "{}",
        project.files[0].contents
    );
}

// --- Properties --------------------------------------------------------------------------

/// One function of a random program: shared or not, what its default
/// value and its body call (indexes into all the program's functions,
/// kept only where the call is allowed: the module's own functions and the
/// others' shared ones), and whether it takes a name another module's
/// function may also have.
#[derive(Debug, Clone)]
struct Function {
    shared: bool,
    default_call: Option<usize>,
    body_call: Option<usize>,
    body_value: u8,
    common_name: bool,
}

fn function() -> impl Strategy<Value = Function> {
    (
        any::<bool>(),
        proptest::option::weighted(0.4, 0..16_usize),
        proptest::option::of(0..16_usize),
        0..9_u8,
        proptest::bool::weighted(0.2),
    )
        .prop_map(
            |(shared, default_call, body_call, body_value, common_name)| Function {
                shared,
                default_call,
                body_call,
                body_value,
                common_name,
            },
        )
}

/// Two to five modules of up to four functions each.
fn modules() -> impl Strategy<Value = Vec<Vec<Function>>> {
    proptest::collection::vec(proptest::collection::vec(function(), 0..4), 2..6)
}

/// The document of a random program, for `standard`; `body_offset` changes
/// every function's returned number (an edit inside the bodies), and
/// `order` the order of the modules.
fn program_document(
    modules: &[Vec<Function>],
    standard: &str,
    body_offset: u8,
    order: &[usize],
) -> b2c_model::Document {
    // Each function's module, symbol, whether it is shared and takes its
    // input with a default value.
    let mut all: Vec<(usize, String, bool, bool)> = Vec::new();
    for (m, functions) in modules.iter().enumerate() {
        for (i, function) in functions.iter().enumerate() {
            all.push((
                m,
                format!("s_m{m}_f{i}"),
                function.shared,
                function.default_call.is_some(),
            ));
        }
    }
    let callable = |from: usize, target: Option<usize>| -> Option<Value> {
        let (m, sym, shared, defaulted) = all.get(target? % all.len().max(1))?;
        if *m != from && !shared {
            return None;
        }
        Some(if *defaulted {
            call(sym, &[])
        } else {
            call(sym, &[n("1")])
        })
    };
    let mut flat = 0;
    let mut built: Vec<(String, Vec<Value>)> = Vec::new();
    for (m, functions) in modules.iter().enumerate() {
        let mut blocks = Vec::new();
        if m == 0 {
            blocks.push(main_block(vec![print(json!({"expr": [json!({"str": "ok"})]}))]));
        }
        for (i, function) in functions.iter().enumerate() {
            let sym = &all[flat].1;
            flat += 1;
            let name = if function.common_name && !function.shared {
                String::from("helper")
            } else {
                format!("f{m}_{i}")
            };
            let x = format!("{sym}_x");
            let value = u32::from(function.body_value) + u32::from(body_offset);
            let body_value = match callable(m, function.body_call) {
                Some(call) => {
                    let mut tokens = call["expr"].as_array().unwrap().clone();
                    tokens.extend([op("+"), n(&value.to_string()), op("+"), r(&x)]);
                    json!({"expr": tokens})
                }
                None => json!({"expr": [n(&value.to_string()), op("+"), r(&x)]}),
            };
            let mut block = define(
                sym,
                &name,
                json!("int"),
                &[(x.as_str(), "x", json!("int"))],
                vec![ret(body_value)],
                function.shared,
            );
            if function.default_call.is_some() {
                let default = callable(m, function.default_call).unwrap_or_else(|| json!({"expr": [n("2")]}));
                block["inputs"] = json!({"DEFAULT0": default});
            }
            blocks.push(block);
        }
        built.push((format!("m{m}"), blocks));
    }
    let ordered: Vec<(&str, Vec<Value>)> = order
        .iter()
        .map(|&m| (built[m].0.as_str(), built[m].1.clone()))
        .collect();
    let mut document = doc_modules(ordered);
    for (place, module) in document.modules.iter_mut().enumerate() {
        module.id = ModuleId::new(&format!("mod_m{}", order[place])).unwrap();
    }
    document.project.language.standard = match standard {
        "c++17" => b2c_ir::sast::CppStandard::Cpp17,
        _ => b2c_ir::sast::CppStandard::Cpp20,
    };
    document
}

/// The program and its files.
fn generated(document: &b2c_model::Document) -> (b2c_lang::Analysis, GeneratedProject) {
    let analysis = analyze(document);
    let project = generate(&analysis.program, &CodegenOptions::default());
    (analysis, project)
}

/// Whether the headers include each other in a circle.
fn headers_cycle(layout: &Layout) -> bool {
    let edges: BTreeMap<&ModuleId, Vec<&ModuleId>> = layout
        .modules
        .iter()
        .map(|files| (&files.module, files.header_includes.iter().collect()))
        .collect();
    layout.modules.iter().any(|files| {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<&ModuleId> = edges.get(&files.module).cloned().unwrap_or_default();
        while let Some(next) = stack.pop() {
            if next == &files.module {
                return true;
            }
            if seen.insert(next) {
                stack.extend(edges.get(next).cloned().unwrap_or_default());
            }
        }
        false
    })
}

/// The files by path.
fn by_path(project: &GeneratedProject) -> BTreeMap<&str, &str> {
    project
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.contents.as_str()))
        .collect()
}

/// How many cases have been compiled with g++ (bounded: g++ is slow).
static COMPILED: AtomicUsize = AtomicUsize::new(0);

/// How many cases are compiled with g++ in one run.
const COMPILED_CASES: usize = 12;

/// A command for g++. Production code spawns processes only through
/// b2c-process (spec §8.5); tests drive g++ directly.
#[allow(clippy::disallowed_methods)]
fn gxx() -> Command {
    Command::new("g++")
}

/// Compiles each header alone through a one-line unit, then each `.cpp`,
/// and links the program.
fn compile_and_link(project: &GeneratedProject, standard: &str) -> Result<(), String> {
    let folder = std::env::temp_dir().join(format!(
        "b2c-modules-functions-{}-{}",
        std::process::id(),
        COMPILED.load(Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    for file in &project.files {
        std::fs::write(folder.join(&file.path), &file.contents).map_err(|e| e.to_string())?;
    }
    let std_flag = format!("-std={standard}");
    let run = |command: &mut Command, what: &str| -> Result<(), String> {
        let output = command.output().map_err(|e| format!("{what}: {e}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!("{what}:\n{}", String::from_utf8_lossy(&output.stderr)))
        }
    };
    let mut objects = Vec::new();
    for file in &project.files {
        match file.kind {
            FileKind::Header => {
                let unit = folder.join(format!("{}.unit.cpp", file.path));
                std::fs::write(&unit, format!("#include \"{}\"\n", file.path)).map_err(|e| e.to_string())?;
                run(
                    gxx()
                        .arg(&std_flag)
                        .args(["-Wall", "-Wextra", "-fsyntax-only", "-iquote"])
                        .arg(&folder)
                        .arg(&unit),
                    &format!("{} alone", file.path),
                )?;
            }
            FileKind::Source => {
                let object = folder.join(format!("{}.o", file.path));
                run(
                    gxx()
                        .arg(&std_flag)
                        .args(["-Wall", "-Wextra", "-c", "-iquote"])
                        .arg(&folder)
                        .arg(folder.join(&file.path))
                        .arg("-o")
                        .arg(&object),
                    &file.path,
                )?;
                objects.push(object);
            }
        }
    }
    run(
        gxx().args(&objects).arg("-o").arg(folder.join("program")),
        "the link",
    )?;
    std::fs::remove_dir_all(&folder).map_err(|e| e.to_string())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn random_programs_of_functions_across_modules(modules in modules(), cxx20 in any::<bool>(), seed in any::<u64>()) {
        let standard = if cxx20 { "c++20" } else { "c++17" };
        let order: Vec<usize> = (0..modules.len()).collect();
        let document = program_document(&modules, standard, 0, &order);
        let (analysis, project) = generated(&document);
        // Deterministic.
        let (again, project_again) = generated(&document);
        prop_assert_eq!(&again.diagnostics, &analysis.diagnostics);
        prop_assert_eq!(&project_again, &project);
        let errors = analysis.diagnostics.iter().any(|d| d.severity == Severity::Error);
        // Headers never include each other when nothing is reported.
        if !errors {
            prop_assert!(!headers_cycle(&analysis.program.layout), "{}", render_all(&analysis));
        }
        // A body edit leaves every header byte-identical.
        let edited = program_document(&modules, standard, 3, &order);
        let (_, edited_project) = generated(&edited);
        let headers = |project: &GeneratedProject| -> Vec<(String, String)> {
            project.files.iter().filter(|f| f.kind == FileKind::Header).map(|f| (f.path.clone(), f.contents.clone())).collect()
        };
        prop_assert_eq!(headers(&edited_project), headers(&project));
        // The module order changes no file's text.
        let mut shuffled: Vec<usize> = order.clone();
        let count = shuffled.len();
        shuffled.rotate_left(usize::try_from(seed % u64::try_from(count).unwrap()).unwrap());
        let reordered = program_document(&modules, standard, 0, &shuffled);
        let (_, reordered_project) = generated(&reordered);
        prop_assert_eq!(by_path(&reordered_project), by_path(&project));
        // With g++, a bounded number of programs without errors: each
        // header alone, and the whole program linked.
        if !errors && common::gxx::available() && COMPILED.fetch_add(1, Ordering::Relaxed) < COMPILED_CASES {
            let compiled = compile_and_link(&project, standard);
            prop_assert!(compiled.is_ok(), "{}", compiled.unwrap_err());
        }
    }
}

/// The program [`Program`] of a random case is kept whole by the analysis:
/// a quick check that the property's documents are the ones meant.
#[test]
fn the_random_programs_are_built_as_meant() {
    let modules = vec![
        vec![Function {
            shared: true,
            default_call: Some(1),
            body_call: None,
            body_value: 1,
            common_name: false,
        }],
        vec![Function {
            shared: true,
            default_call: None,
            body_call: Some(0),
            body_value: 2,
            common_name: false,
        }],
    ];
    let document = program_document(&modules, "c++20", 0, &[0, 1]);
    let (analysis, project) = generated(&document);
    assert_clean(&analysis);
    let program: &Program = &analysis.program;
    assert_eq!(program.modules.len(), 2);
    let files = by_path(&project);
    assert_eq!(
        files.keys().copied().collect::<Vec<_>>(),
        ["m0.cpp", "m0.hpp", "m1.cpp", "m1.hpp"]
    );
    assert!(
        files["m0.hpp"].contains("#include \"m1.hpp\""),
        "{}",
        files["m0.hpp"]
    );
    assert!(
        files["m0.hpp"].contains("int f0_0(int x = f1_0(1));"),
        "{}",
        files["m0.hpp"]
    );
    assert!(
        files["m1.cpp"].contains("#include \"m0.hpp\""),
        "{}",
        files["m1.cpp"]
    );
}
