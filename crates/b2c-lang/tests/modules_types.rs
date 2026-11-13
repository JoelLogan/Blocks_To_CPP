//! Structs, enums, globals and constants across modules, W3's package T
//! (`docs/plans/m3-contracts/w3-modules.md` §1.3, §2.1 to §2.4, §5.2): which
//! modules see which definitions, the codes that guard them (`B2C-E0206`,
//! `B2C-E0210`, `B2C-E0213`, `B2C-E0214`, `B2C-E0420`, `B2C-E0422`,
//! `B2C-E0423`, `B2C-E0424`), what each header includes or forward-declares,
//! and properties of random programs of two to four modules with structs,
//! constants and globals: generation is deterministic, headers never
//! include each other when nothing is reported, an edit that keeps every
//! shared constant's form never changes a header, the module order changes
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
use b2c_ir::diag::Diagnostic;
use b2c_ir::ids::{BlockId, ModuleId, SymbolId};
use b2c_ir::layout::Layout;
use b2c_ir::sast::ItemKind;
use b2c_ir::source_map::{FileKind, GeneratedProject};
use common::{analyze, assert_clean, doc_modules, render_all};
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

/// A slot of these tokens.
fn slot(tokens: &[Value]) -> Value {
    json!({"expr": tokens})
}

/// A slot calling `sym` with no argument.
fn call(sym: &str) -> Value {
    slot(&[r(sym), op("("), op(")")])
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

/// `define <name>` returning `int` with no input, declaring `sym` (block
/// `fn_<sym>`), its body `return <value>`.
fn function(sym: &str, name: &str, value: Value, shared: bool) -> Value {
    json!({
        "id": format!("fn_{sym}"), "type": "func.define", "v": 1,
        "fields": {"NAME": {"sym": sym, "name": name}, "RETURNS": "int", "SHARED": shared},
        "extra": {"params": []},
        "statements": {"BODY": [ret(value)]}
    })
}

/// `define <name>(<input>)` returning `int` (block `fn_<sym>`), its body
/// `return 1`, with one input `(symbol, name, type)` taken as a copy.
fn taking(sym: &str, name: &str, input: (&str, &str, Value), shared: bool) -> Value {
    json!({
        "id": format!("fn_{sym}"), "type": "func.define", "v": 1,
        "fields": {"NAME": {"sym": sym, "name": name}, "RETURNS": "int", "SHARED": shared},
        "extra": {"params": [{"sym": input.0, "name": input.1, "type": input.2, "mode": "copy"}]},
        "statements": {"BODY": [ret(slot(&[n("1")]))]}
    })
}

/// `struct <name>` (block `st_<sym>`) with fields `(symbol, name, type)`.
fn structure(sym: &str, name: &str, fields: &[(&str, &str, Value)], shared: bool) -> Value {
    let fields: Vec<Value> = fields
        .iter()
        .map(|(sym, name, ty)| json!({"sym": sym, "name": name, "type": ty}))
        .collect();
    json!({"id": format!("st_{sym}"), "type": "type.struct", "v": 1,
           "fields": {"NAME": {"sym": sym, "name": name}, "COMPARABLE": false, "PRINTABLE": false,
                      "SHARED": shared},
           "extra": {"fields": fields}})
}

/// `struct <name>` with one `int` field, `size`.
fn small_struct(sym: &str, name: &str, shared: bool) -> Value {
    structure(
        sym,
        name,
        &[(&format!("{sym}_size"), "size", json!("int"))],
        shared,
    )
}

/// The struct `block` with these default values, by field row.
fn with_defaults(mut block: Value, defaults: &[(usize, Value)]) -> Value {
    let inputs: serde_json::Map<String, Value> = defaults
        .iter()
        .map(|(row, value)| (format!("DEFAULT{row}"), value.clone()))
        .collect();
    block["inputs"] = Value::Object(inputs);
    block
}

/// The struct `block`, comparable.
fn comparable(mut block: Value) -> Value {
    block["fields"]["COMPARABLE"] = json!(true);
    block
}

/// The struct `block`, printable.
fn printable(mut block: Value) -> Value {
    block["fields"]["PRINTABLE"] = json!(true);
    block
}

/// `enum <name>` (block `en_<sym>`) with one value, `<sym>_v`.
fn enumeration(sym: &str, name: &str, shared: bool) -> Value {
    json!({"id": format!("en_{sym}"), "type": "type.enum", "v": 1,
           "fields": {"NAME": {"sym": sym, "name": name}, "SHARED": shared},
           "extra": {"values": [{"sym": format!("{sym}_v"), "name": "First"}]}})
}

/// The value `<sym>_v` of the enum `sym`.
fn enum_value(sym: &str) -> Value {
    json!({"block": {"id": format!("ev_{}", common::id()), "type": "type.enum_value", "v": 1,
                     "fields": {"ENUM": r(sym), "VALUE": r(&format!("{sym}_v"))}}})
}

/// A global (block `gl_<sym>`): `variable` or `constant`, of type `ty`,
/// with this starting value.
fn global(sym: &str, name: &str, kind: &str, ty: Value, value: Value, shared: bool) -> Value {
    json!({"id": format!("gl_{sym}"), "type": "var.global", "v": 1,
           "fields": {"KIND": kind, "TYPE": ty, "NAME": {"sym": sym, "name": name}, "SHARED": shared},
           "inputs": {"VALUE": value}})
}

/// A `const int` global with this value.
fn constant(sym: &str, name: &str, value: Value, shared: bool) -> Value {
    global(sym, name, "constant", json!("int"), value, shared)
}

/// An `int` global variable with this value.
fn variable(sym: &str, name: &str, value: Value, shared: bool) -> Value {
    global(sym, name, "variable", json!("int"), value, shared)
}

/// *switch on* `value` with one `case` labelled `label`.
fn switch(id: &str, value: Value, label: Value) -> Value {
    json!({"id": id, "type": "control.switch", "v": 1, "extra": {"caseCount": 1, "hasDefault": false},
           "inputs": {"VALUE": value, "CASE0": label},
           "statements": {"DO0": [print(slot(&[n("1")]))]}})
}

/// The diagnostics with a code.
fn with_code<'a>(analysis: &'a b2c_lang::Analysis, code: &str) -> Vec<&'a Diagnostic> {
    analysis.diagnostics.iter().filter(|d| d.code.0 == code).collect()
}

/// The messages of the diagnostics with a code.
fn messages<'a>(analysis: &'a b2c_lang::Analysis, code: &str) -> Vec<&'a str> {
    with_code(analysis, code)
        .into_iter()
        .map(|d| d.message.as_str())
        .collect()
}

/// The codes reported, in order.
fn codes(analysis: &b2c_lang::Analysis) -> Vec<&str> {
    analysis.diagnostics.iter().map(|d| d.code.0.as_str()).collect()
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

/// A diagnostic's related locations as `(block, note)`.
fn related(d: &Diagnostic) -> Vec<(String, String)> {
    d.related
        .iter()
        .map(|related| {
            (
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

/// The module IDs of a list of a layout.
fn ids(list: &[ModuleId]) -> Vec<&str> {
    list.iter().map(ModuleId::as_str).collect()
}

/// The names of the symbols a list of the layout names.
fn symbol_names(analysis: &b2c_lang::Analysis, list: &[SymbolId]) -> Vec<String> {
    list.iter()
        .map(|id| analysis.program.symbols.get(id).unwrap().name.as_str().to_owned())
        .collect()
}

/// The names of the symbols the scope query lists at a block.
fn scope(analysis: &b2c_lang::Analysis, block: &str) -> Vec<String> {
    analysis
        .symbols_in_scope(&BlockId::new(block).unwrap(), None)
        .into_iter()
        .map(|info| info.id.to_string())
        .collect()
}

// --- Visibility ---------------------------------------------------------------------

#[test]
fn a_module_sees_the_types_and_globals_the_others_share() {
    let document = doc_modules(vec![
        (
            "main",
            vec![
                main_block(vec![print(slot(&[r("s_total")]))]),
                variable("s_count_main", "count", slot(&[n("1")]), false),
            ],
        ),
        (
            "stock",
            vec![
                small_struct("s_Item", "Item", true),
                small_struct("s_Hidden", "Hidden", false),
                enumeration("s_Kind", "Kind", true),
                variable("s_total", "total", slot(&[n("0")]), true),
                variable("s_secret", "secret", slot(&[n("0")]), false),
                constant("s_kMax", "kMax", slot(&[n("5")]), true),
                // Shared too, but `main`'s own `count` hides it there.
                variable("s_count_stock", "counted", slot(&[n("0")]), true),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    assert_clean(&analysis);
    let at_print = analysis.program.modules[0]
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Main(main) => main.body.stmts.first().map(|stmt| stmt.origin.block.to_string()),
            _ => None,
        })
        .unwrap();
    let seen = scope(&analysis, &at_print);
    for shared in [
        "s_Item",
        "s_Kind",
        "s_total",
        "s_kMax",
        "s_count_main",
        "s_count_stock",
    ] {
        assert!(seen.contains(&shared.to_owned()), "{shared} in {seen:?}");
    }
    for hidden in ["s_Hidden", "s_secret"] {
        assert!(!seen.contains(&hidden.to_owned()), "{hidden} in {seen:?}");
    }
    let shared: BTreeSet<String> = analysis
        .symbol_infos()
        .into_iter()
        .filter(|info| info.shared)
        .map(|info| info.id.to_string())
        .collect();
    assert_eq!(
        shared,
        ["s_Item", "s_Kind", "s_count_stock", "s_kMax", "s_total"]
            .into_iter()
            .map(String::from)
            .collect()
    );
}

#[test]
fn a_type_and_a_global_of_another_module_are_used_where_it_shares_them() {
    // `main` names `shapes`' shared struct as a variable's type, in a list
    // and in a field, its shared enum's value, and reads and sets its shared
    // variable: no problem, and `main.cpp` includes `shapes.hpp`.
    let document = doc_modules(vec![
        (
            "main",
            vec![
                structure(
                    "s_Pair",
                    "Pair",
                    &[("s_pair_box", "box", json!({"ref": "s_Box"}))],
                    false,
                ),
                main_block(vec![
                    json!({"id": "declare_b", "type": "var.declare", "v": 1,
                           "fields": {"CONST": false, "NAME": {"sym": "s_b", "name": "b"}, "TYPE": {"ref": "s_Box"}}}),
                    json!({"id": "declare_p", "type": "var.declare", "v": 1,
                           "fields": {"CONST": false, "NAME": {"sym": "s_p", "name": "p"}, "TYPE": {"ref": "s_Pair"}}}),
                    json!({"id": "declare_list", "type": "var.declare", "v": 1,
                           "fields": {"CONST": false, "NAME": {"sym": "s_list", "name": "list"},
                                      "TYPE": {"std::vector": {"ref": "s_Box"}}}}),
                    json!({"id": "declare_k", "type": "var.declare", "v": 1,
                           "fields": {"CONST": false, "NAME": {"sym": "s_k", "name": "k"}, "TYPE": {"ref": "s_Kind"}},
                           "inputs": {"VALUE": enum_value("s_Kind")}}),
                    json!({"id": "set_total", "type": "var.set", "v": 1, "fields": {"VAR": r("s_total")},
                           "inputs": {"VALUE": slot(&[r("s_total"), op("+"), n("1")])}}),
                ]),
            ],
        ),
        (
            "shapes",
            vec![
                small_struct("s_Box", "Box", true),
                enumeration("s_Kind", "Kind", true),
                variable("s_total", "total", slot(&[n("0")]), true),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    let found: Vec<&str> = codes(&analysis)
        .into_iter()
        .filter(|code| code.starts_with("B2C-E"))
        .collect();
    assert!(found.is_empty(), "{}", render_all(&analysis));
    let layout = &analysis.program.layout;
    assert_eq!(ids(&layout.modules[0].source_includes), ["mod_1"]);
}

// --- B2C-E0206 ------------------------------------------------------------------------

#[test]
fn e0206_for_another_module_s_type_and_global_it_does_not_share() {
    let document = doc_modules(vec![
        (
            "main",
            vec![main_block(vec![
                print(slot(&[r("s_count")])),
                json!({"id": "declare_b", "type": "var.declare", "v": 1,
                       "fields": {"CONST": false, "NAME": {"sym": "s_b", "name": "b"}, "TYPE": {"ref": "s_Box"}}}),
            ])],
        ),
        (
            "shapes",
            vec![
                small_struct("s_Box", "Box", false),
                variable("s_count", "count", slot(&[n("0")]), false),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    assert_eq!(
        messages(&analysis, "B2C-E0206"),
        [
            "`count` belongs to module `shapes`, which does not share it. To use it here, tick 'share with \
             other modules' on its 'global' block.",
            "`Box` belongs to module `shapes`, which does not share it. To use it here, tick 'share with other \
             modules' on its 'struct' block.",
        ],
        "{}",
        render_all(&analysis)
    );
    let type_error = with_code(&analysis, "B2C-E0206")[1];
    assert_eq!(at(type_error), (String::from("mod_0"), String::from("declare_b")));
    assert_eq!(
        related(type_error),
        [(String::from("st_s_Box"), String::from("`Box` is created here"))]
    );
}

// --- B2C-E0210 and B2C-E0213 ------------------------------------------------------------

#[test]
fn e0210_for_two_modules_sharing_definitions_with_one_name() {
    let document = doc_modules(vec![
        ("main", vec![main_block(vec![])]),
        ("shapes", vec![small_struct("s_Point_a", "Point", true)]),
        (
            "maps",
            vec![variable("s_Point_b", "Point", slot(&[n("0")]), true)],
        ),
    ]);
    let analysis = analyze(&document);
    assert_eq!(codes(&analysis), ["B2C-E0210"], "{}", render_all(&analysis));
    let error = with_code(&analysis, "B2C-E0210")[0];
    assert_eq!(
        error.message,
        "`Point` is already shared by module `shapes`. Two modules can't share definitions with one name, so \
         rename one of them."
    );
    assert_eq!(at(error), (String::from("mod_2"), String::from("gl_s_Point_b")));
    assert_eq!(
        related(error),
        [(
            String::from("st_s_Point_a"),
            String::from("the other shared `Point`")
        )]
    );
}

#[test]
fn definitions_that_are_not_shared_may_share_a_name_across_modules() {
    // Each module's own `count`, `Box` and `Kind`: no clash, each in its
    // anonymous namespace.
    let module = |suffix: &str| {
        vec![
            variable(&format!("s_count_{suffix}"), "count", slot(&[n("0")]), false),
            small_struct(&format!("s_Box_{suffix}"), "Box", false),
            enumeration(&format!("s_Kind_{suffix}"), "Kind", false),
        ]
    };
    let mut main = module("main");
    main.push(main_block(vec![]));
    let document = doc_modules(vec![("main", main), ("other", module("other"))]);
    assert_clean(&analyze(&document));
}

#[test]
fn e0213_for_a_type_or_global_named_like_another_module_s_shared_one() {
    let document = doc_modules(vec![
        (
            "main",
            vec![
                main_block(vec![print(slot(&[r("s_limit")]))]),
                variable("s_count_main", "count", slot(&[n("0")]), false),
                small_struct("s_Box_main", "Box", false),
            ],
        ),
        (
            "stock",
            vec![
                variable("s_count_stock", "count", slot(&[n("0")]), true),
                constant("s_limit", "limit", slot(&[n("3")]), true),
                small_struct("s_Box_stock", "Box", true),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    assert_eq!(
        messages(&analysis, "B2C-E0213"),
        [
            "`count` is also shared by module `stock`, whose header `stock.hpp` this file includes because it \
             uses `limit`. Rename this `count`.",
            "`Box` is also shared by module `stock`, whose header `stock.hpp` this file includes because it \
             uses `limit`. Rename this `Box`.",
        ],
        "{}",
        render_all(&analysis)
    );
    // Without the use, the message names the header that would bring the
    // clash, and the kinds of both.
    let document = doc_modules(vec![
        (
            "main",
            vec![main_block(vec![]), small_struct("s_Box_main", "Box", false)],
        ),
        (
            "stock",
            vec![variable("s_Box_stock", "Box", slot(&[n("0")]), true)],
        ),
    ]);
    let analysis = analyze(&document);
    assert_eq!(
        messages(&analysis, "B2C-E0213"),
        [
            "`Box` is also shared by module `stock`. A struct that is not shared can't have the name of a shared \
          global variable of any module: once this file includes `stock.hpp`, C++ could not tell the two \
          apart. Rename this `Box`."
        ],
        "{}",
        render_all(&analysis)
    );
}

// --- B2C-E0214 ------------------------------------------------------------------------

#[test]
fn e0214_for_each_header_part_of_a_struct_a_constant_and_a_variable() {
    let document = doc_modules(vec![
        ("main", vec![main_block(vec![])]),
        (
            "shapes",
            vec![
                small_struct("s_Box", "Box", false),
                enumeration("s_Kind", "Kind", false),
                function("s_unit", "unit", slot(&[n("1")]), false),
                constant("s_kSide", "kSide", slot(&[n("2")]), false),
                // A field of a struct that is not shared, a list of it, and
                // default values that call and read what is not shared.
                with_defaults(
                    structure(
                        "s_Crate",
                        "Crate",
                        &[
                            ("s_crate_box", "box", json!({"ref": "s_Box"})),
                            ("s_crate_boxes", "boxes", json!({"std::vector": {"ref": "s_Box"}})),
                            ("s_crate_size", "size", json!("int")),
                            ("s_crate_side", "side", json!("int")),
                        ],
                        true,
                    ),
                    &[(2, call("s_unit")), (3, slot(&[r("s_kSide")]))],
                ),
                // A constant's type and a variable's type.
                global(
                    "s_kFirst",
                    "kFirst",
                    "constant",
                    json!({"ref": "s_Kind"}),
                    enum_value("s_Kind"),
                    true,
                ),
                json!({"id": "gl_s_spare", "type": "var.global", "v": 1,
                       "fields": {"KIND": "variable", "TYPE": {"std::vector": {"ref": "s_Box"}},
                                  "NAME": {"sym": "s_spare", "name": "spare"}, "SHARED": true}}),
                // A shared constant's value may name what is not shared: it
                // is then `extern const`, and no error.
                constant("s_kTwice", "kTwice", slot(&[r("s_kSide"), op("*"), n("2")]), true),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    assert_eq!(
        messages(&analysis, "B2C-E0214"),
        [
            // The header's order: constants, then types, then variables.
            "`kFirst` is shared, but its type is `Kind`, which is not. Share `Kind` too.",
            "`Crate` is shared, but its field `box` has the type `Box`, which is not. Share `Box` too.",
            "`Crate` is shared, but the type of its field `boxes` uses `Box`, which is not. Share `Box` too.",
            "`Crate` is shared, but the default value of its field `size` uses `unit`, which is not. Share \
             `unit` too.",
            "`Crate` is shared, but the default value of its field `side` uses `kSide`, which is not. Share \
             `kSide` too.",
            "`spare` is shared, but its type uses `Box`, which is not. Share `Box` too.",
        ],
        "{}",
        render_all(&analysis)
    );
    assert_eq!(codes(&analysis).len(), 6, "{}", render_all(&analysis));
    let first = with_code(&analysis, "B2C-E0214")[1];
    assert_eq!(
        related(first),
        [(
            String::from("st_s_Box"),
            String::from("`Box` is defined here, not shared")
        )]
    );
    // `kTwice` is `extern const` in the header.
    let twice = analysis.program.modules[1]
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Global(def) if def.symbol.as_str() == "s_kTwice" => Some(def),
            _ => None,
        })
        .unwrap();
    assert!(!twice.constexpr);
}

#[test]
fn a_shared_function_finds_the_shared_types_and_globals_of_its_module_in_its_header() {
    // Package F's gap (w3-modules.md §7.1): a shared function that names a
    // shared struct, enum or global of its own module draws no `B2C-E0214`,
    // and now the header holds them, before the function.
    let document = |box_shared: bool| {
        doc_modules(vec![
            ("main", vec![main_block(vec![])]),
            (
                "shapes",
                vec![
                    small_struct("s_Box", "Box", box_shared),
                    enumeration("s_Kind", "Kind", true),
                    variable("s_made", "made", slot(&[n("0")]), true),
                    taking(
                        "s_area",
                        "area",
                        ("s_area_box", "box", json!({"ref": "s_Box"})),
                        true,
                    ),
                    json!({"id": "fn_s_kind_of", "type": "func.define", "v": 1,
                           "fields": {"NAME": {"sym": "s_kind_of", "name": "kind_of"},
                                      "RETURNS": {"ref": "s_Kind"}, "SHARED": true},
                           "extra": {"params": [{"sym": "s_kind_of_x", "name": "x", "type": "int",
                                                 "mode": "copy"}]},
                           "inputs": {"DEFAULT0": slot(&[r("s_made")])},
                           "statements": {"BODY": [ret(enum_value("s_Kind"))]}}),
                ],
            ),
        ])
    };
    let analysis = analyze(&document(true));
    assert_clean(&analysis);
    let project = generate(&analysis.program, &CodegenOptions::default());
    let header = project
        .files
        .iter()
        .find(|file| file.path == "shapes.hpp")
        .map(|file| file.contents.as_str())
        .unwrap();
    let place = |text: &str| {
        header
            .find(text)
            .unwrap_or_else(|| panic!("{text:?} in:\n{header}"))
    };
    assert!(place("struct Box {") < place("int area(Box box);"), "{header}");
    assert!(
        place("enum class Kind { First };") < place("Kind kind_of(int x = made);"),
        "{header}"
    );
    assert!(
        place("extern int made;") < place("Kind kind_of(int x = made);"),
        "{header}"
    );
    // Not shared, the struct is `B2C-E0214` again.
    let analysis = analyze(&document(false));
    assert_eq!(codes(&analysis), ["B2C-E0214"], "{}", render_all(&analysis));
}

// --- Constant expressions across modules: B2C-E0424 ----------------------------------------

#[test]
fn e0424_for_another_module_s_const_constant_but_not_a_constexpr_one() {
    let document = doc_modules(vec![
        (
            "main",
            vec![main_block(vec![
                json!({"id": "declare_n", "type": "var.declare", "v": 1,
                       "fields": {"CONST": false, "NAME": {"sym": "s_n", "name": "n"}, "TYPE": "int"},
                       "inputs": {"VALUE": slot(&[n("0")])}}),
                switch("switch_fixed", slot(&[r("s_n")]), slot(&[r("s_kFixed")])),
                switch("switch_extern", slot(&[r("s_n")]), slot(&[r("s_kExtern")])),
                json!({"id": "check_extern", "type": "program.check", "v": 1,
                       "fields": {"MESSAGE": "too small"},
                       "inputs": {"COND": slot(&[r("s_kExtern"), op(">"), n("0")])}}),
            ])],
        ),
        (
            "stock",
            vec![
                constant("s_kBase", "kBase", slot(&[n("4")]), false),
                constant("s_kFixed", "kFixed", slot(&[n("5")]), true),
                // Its value names `kBase`, which is not shared: other modules
                // see only `extern const int kExtern;`.
                constant(
                    "s_kExtern",
                    "kExtern",
                    slot(&[r("s_kBase"), op("+"), n("2")]),
                    true,
                ),
                // In its own module it is a constant expression after its
                // definition.
                function("s_pick", "pick", slot(&[r("s_kExtern")]), true),
                json!({"id": "fn_s_use", "type": "func.define", "v": 1,
                       "fields": {"NAME": {"sym": "s_use", "name": "use"}, "RETURNS": "void", "SHARED": true},
                       "extra": {"params": [{"sym": "s_use_n", "name": "n", "type": "int", "mode": "copy"}]},
                       "statements": {"BODY": [switch("switch_own", slot(&[r("s_use_n")]), slot(&[r("s_kExtern")]))]}}),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    let found: Vec<(String, String)> = with_code(&analysis, "B2C-E0424").into_iter().map(at).collect();
    assert_eq!(
        found,
        [
            (String::from("mod_0"), String::from("switch_extern")),
            (String::from("mod_0"), String::from("check_extern")),
        ],
        "{}",
        render_all(&analysis)
    );
    assert_eq!(
        codes(&analysis),
        ["B2C-E0424", "B2C-E0424"],
        "{}",
        render_all(&analysis)
    );
}

// --- B2C-E0420, B2C-E0421 and B2C-E0422 -----------------------------------------------------

#[test]
fn e0420_for_structs_of_two_modules_that_hold_each_other() {
    let document = doc_modules(vec![
        ("main", vec![main_block(vec![])]),
        (
            "a",
            vec![structure(
                "s_A",
                "A",
                &[("s_a_b", "b", json!({"ref": "s_B"}))],
                true,
            )],
        ),
        (
            "b",
            vec![structure(
                "s_B",
                "B",
                &[("s_b_a", "a", json!({"ref": "s_A"}))],
                true,
            )],
        ),
    ]);
    let analysis = analyze(&document);
    assert_eq!(
        messages(&analysis, "B2C-E0420"),
        [
            "`A` holds a `B` in its field `b`, which holds an `A` in its field `a`, so each `A` would have to \
             hold a whole copy of itself, which C++ does not allow. Make one of these fields a list (a list \
             keeps its items elsewhere), or remove it.",
            "`B` holds an `A` in its field `a`, which holds a `B` in its field `b`, so each `B` would have to \
             hold a whole copy of itself, which C++ does not allow. Make one of these fields a list (a list \
             keeps its items elsewhere), or remove it.",
        ],
        "{}",
        render_all(&analysis)
    );
    // One code per circle: the headers' circle is explained by it.
    assert_eq!(
        codes(&analysis),
        ["B2C-E0420", "B2C-E0420"],
        "{}",
        render_all(&analysis)
    );
}

#[test]
fn e0421_for_globals_of_two_modules_set_from_each_other() {
    let document = doc_modules(vec![
        ("main", vec![main_block(vec![])]),
        (
            "a",
            vec![constant("s_x", "x", slot(&[r("s_y"), op("+"), n("1")]), true)],
        ),
        (
            "b",
            vec![constant("s_y", "y", slot(&[r("s_x"), op("+"), n("1")]), true)],
        ),
    ]);
    let analysis = analyze(&document);
    let found = codes(&analysis);
    assert!(found.contains(&"B2C-E0421"), "{}", render_all(&analysis));
    assert!(!found.contains(&"B2C-E0422"), "{}", render_all(&analysis));
}

#[test]
fn e0422_for_two_headers_of_types_and_constants_that_need_each_other() {
    // `shapes`' struct holds `colours`' struct by value, while `colours`'
    // constant has `shapes`' enum as its type: no definition needs itself.
    let document = doc_modules(vec![
        ("main", vec![main_block(vec![])]),
        (
            "shapes",
            vec![
                enumeration("s_Kind", "Kind", true),
                structure(
                    "s_Shape",
                    "Shape",
                    &[("s_shape_palette", "palette", json!({"ref": "s_Palette"}))],
                    true,
                ),
            ],
        ),
        (
            "colours",
            vec![
                small_struct("s_Palette", "Palette", true),
                global(
                    "s_kKind",
                    "kKind",
                    "constant",
                    json!({"ref": "s_Kind"}),
                    enum_value("s_Kind"),
                    true,
                ),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    assert_eq!(
        messages(&analysis, "B2C-E0422"),
        [
            "`shapes.hpp` would need `colours.hpp`, because `Shape` holds `Palette` of module `colours`, but \
             `colours.hpp` already needs `shapes.hpp`. Two headers can't each include the other. Move `Shape` \
             into module `colours`, or `kKind` into module `shapes`.",
            "`colours.hpp` would need `shapes.hpp`, because the type of `kKind` uses `Kind` of module `shapes`, \
             but `shapes.hpp` already needs `colours.hpp`. Two headers can't each include the other. Move \
             `kKind` into module `shapes`, or `Shape` into module `colours`.",
        ],
        "{}",
        render_all(&analysis)
    );
    let first = with_code(&analysis, "B2C-E0422")[0];
    assert_eq!(at(first).0, "mod_1");
    assert_eq!(
        related(first),
        [(
            String::from("gl_s_kKind"),
            String::from("`colours.hpp` needs `shapes.hpp` for the type of `kKind`")
        )]
    );
}

#[test]
fn e0422_names_the_way_round_three_headers_of_types_and_constants() {
    // `a`'s struct holds `b`'s, `b`'s holds `c`'s, and `c`'s constant has
    // `a`'s enum as its type.
    let document = doc_modules(vec![
        ("main", vec![main_block(vec![])]),
        (
            "a",
            vec![
                enumeration("s_EA", "EA", true),
                structure("s_SA", "SA", &[("s_sa_b", "b", json!({"ref": "s_SB"}))], true),
            ],
        ),
        (
            "b",
            vec![structure(
                "s_SB",
                "SB",
                &[("s_sb_c", "c", json!({"ref": "s_SC"}))],
                true,
            )],
        ),
        (
            "c",
            vec![
                small_struct("s_SC", "SC", true),
                global(
                    "s_kA",
                    "kA",
                    "constant",
                    json!({"ref": "s_EA"}),
                    enum_value("s_EA"),
                    true,
                ),
            ],
        ),
    ]);
    let analysis = analyze(&document);
    let found = messages(&analysis, "B2C-E0422");
    assert_eq!(found.len(), 3, "{}", render_all(&analysis));
    assert_eq!(
        found[0],
        "`a.hpp` would need `b.hpp`, because `SA` holds `SB` of module `b`, but `b.hpp` already needs `a.hpp` \
         through `c.hpp`. Two headers can't each include the other. Move one of these shared definitions into \
         another module."
    );
    let first = with_code(&analysis, "B2C-E0422")[0];
    assert_eq!(
        related(first),
        [
            (
                String::from("st_s_SB"),
                String::from("`b.hpp` needs `c.hpp` for the fields of `SB`")
            ),
            (
                String::from("gl_s_kA"),
                String::from("`c.hpp` needs `a.hpp` for the type of `kA`")
            ),
        ]
    );
}

// --- B2C-E0423 ------------------------------------------------------------------------

/// A project whose module `main` has the globals `globals` (and an empty
/// `when program starts`), and whose module `people` has `people`.
fn initialised(globals: Vec<Value>, people: Vec<Value>) -> b2c_lang::Analysis {
    let mut main = globals;
    main.push(main_block(vec![print(slot(&[r("s_name")]))]));
    analyze(&doc_modules(vec![("main", main), ("people", people)]))
}

/// `people`'s shared globals: the variable `name` (a `std::string`), the
/// `int` variable `count`, the `constexpr` constant `kTen`, the constant
/// `kTwelve` set from a constant it does not share (so `extern const`, but
/// still set before any code runs) and the `const std::string` `kTitle`.
fn people() -> Vec<Value> {
    vec![
        global(
            "s_name",
            "name",
            "variable",
            json!("std::string"),
            slot(&[json!({"str": "Ann"})]),
            true,
        ),
        variable("s_count", "count", slot(&[n("0")]), true),
        constant("s_kTen", "kTen", slot(&[n("10")]), true),
        constant("s_kTwo", "kTwo", slot(&[n("2")]), false),
        constant(
            "s_kTwelve",
            "kTwelve",
            slot(&[r("s_kTen"), op("+"), r("s_kTwo")]),
            true,
        ),
        global(
            "s_kTitle",
            "kTitle",
            "constant",
            json!("std::string"),
            slot(&[json!({"str": "Dr"})]),
            true,
        ),
        // A function that reads `count`, and one that calls it.
        function("s_read", "read_count", slot(&[r("s_count")]), true),
        function(
            "s_twice",
            "twice",
            slot(&[r("s_read"), op("("), op(")"), op("*"), n("2")]),
            true,
        ),
        // A function whose default value reads `count`.
        json!({"id": "fn_s_plus", "type": "func.define", "v": 1,
               "fields": {"NAME": {"sym": "s_plus", "name": "plus"}, "RETURNS": "int", "SHARED": true},
               "extra": {"params": [{"sym": "s_plus_x", "name": "x", "type": "int", "mode": "copy"}]},
               "inputs": {"DEFAULT0": slot(&[r("s_count")])},
               "statements": {"BODY": [ret(slot(&[r("s_plus_x")]))]}}),
        // A struct whose field's default value reads `count`.
        with_defaults(
            small_struct("s_Tally", "Tally", true),
            &[(0, slot(&[r("s_count")]))],
        ),
    ]
}

#[test]
fn e0423_for_a_global_set_from_another_module_s_global() {
    let analysis = initialised(
        vec![
            // Directly, a variable and a `const std::string`.
            variable(
                "s_direct",
                "direct",
                slot(&[r("s_count"), op("+"), n("1")]),
                false,
            ),
            global(
                "s_greeting",
                "greeting",
                "variable",
                json!("std::string"),
                slot(&[r("s_kTitle")]),
                false,
            ),
            // Through a chain of calls, a default value and a struct's
            // default member initialiser.
            variable("s_chain", "chain", call("s_twice"), false),
            variable("s_by_default", "by_default", call("s_plus"), false),
            json!({"id": "gl_s_tally", "type": "var.global", "v": 1,
                   "fields": {"KIND": "variable", "TYPE": {"ref": "s_Tally"},
                              "NAME": {"sym": "s_tally", "name": "tally"}, "SHARED": false}}),
            // None for a `constexpr` source or a `const int` C++ sets
            // before any code runs.
            variable(
                "s_fixed",
                "fixed",
                slot(&[r("s_kTen"), op("+"), r("s_kTwelve")]),
                false,
            ),
        ],
        people(),
    );
    let found: Vec<(String, &str)> = with_code(&analysis, "B2C-E0423")
        .into_iter()
        .map(|d| (at(d).1, d.message.as_str()))
        .collect();
    // In the order of the globals' blocks.
    assert_eq!(
        found,
        [
            (
                String::from("gl_s_by_default"),
                "`by_default` is set from `count` of module `people` before the program starts, and C++ does \
                 not say which of the two is set first. Set `by_default` at the start of 'when program \
                 starts', or make `count` a constant with a fixed value."
            ),
            (
                String::from("gl_s_chain"),
                "`chain` is set from `count` of module `people` before the program starts, and C++ does not \
                 say which of the two is set first. Set `chain` at the start of 'when program starts', or make \
                 `count` a constant with a fixed value."
            ),
            (
                String::from("gl_s_direct"),
                "`direct` is set from `count` of module `people` before the program starts, and C++ does not \
                 say which of the two is set first. Set `direct` at the start of 'when program starts', or make \
                 `count` a constant with a fixed value."
            ),
            (
                String::from("gl_s_greeting"),
                "`greeting` is set from `kTitle` of module `people` before the program starts, and C++ does \
                 not say which of the two is set first. Set `greeting` at the start of 'when program starts' \
                 instead."
            ),
            (
                String::from("gl_s_tally"),
                "`tally` is set from `count` of module `people` before the program starts, and C++ does not \
                 say which of the two is set first. Set `tally` at the start of 'when program starts', or make \
                 `count` a constant with a fixed value."
            ),
        ],
        "{}",
        render_all(&analysis)
    );
    let direct = with_code(&analysis, "B2C-E0423")[2];
    assert_eq!(
        related(direct),
        [(
            String::from("gl_s_count"),
            String::from("`count` is defined here, in module `people`")
        )]
    );
    assert_eq!(
        codes(&analysis)
            .iter()
            .filter(|code| code.starts_with("B2C-E"))
            .count(),
        5,
        "{}",
        render_all(&analysis)
    );
}

#[test]
fn reads_in_main_and_in_functions_it_calls_are_never_e0423() {
    let analysis = initialised(
        vec![json!({"id": "fn_s_show", "type": "func.define", "v": 1,
                    "fields": {"NAME": {"sym": "s_show", "name": "show"}, "RETURNS": "int", "SHARED": false},
                    "extra": {"params": []},
                    "statements": {"BODY": [ret(slot(&[r("s_twice"), op("("), op(")"), op("+"), r("s_count")]))]}})],
        people(),
    );
    // `show` is never called, which is fine for the analyser.
    assert!(
        with_code(&analysis, "B2C-E0423").is_empty(),
        "{}",
        render_all(&analysis)
    );
}

// --- Includes and forward declarations (modules.md §3.3, w3-modules.md §2.4) ------------

#[test]
fn a_header_includes_what_its_parts_need_complete_and_forward_declares_the_rest() {
    let shared_struct = |module: &str, sym: &str, name: &str, field: Value| {
        let _ = module;
        structure(sym, name, &[(&format!("{sym}_f"), "item", field)], true)
    };
    let document = doc_modules(vec![
        ("main", vec![main_block(vec![])]),
        (
            "base",
            vec![
                printable(comparable(small_struct("s_Point", "Point", true))),
                enumeration("s_Colour", "Colour", true),
                function("s_unit", "unit", slot(&[n("1")]), true),
                constant("s_kFixed", "kFixed", slot(&[n("3")]), true),
                constant("s_kHidden", "kHidden", slot(&[n("3")]), false),
                constant("s_kExtern", "kExtern", slot(&[r("s_kHidden")]), true),
                variable("s_total", "total", slot(&[n("0")]), true),
            ],
        ),
        // A forward declaration only: an input's type, and a list's.
        (
            "forward",
            vec![
                taking("s_f1", "f1", ("s_f1_p", "p", json!({"ref": "s_Point"})), true),
                taking(
                    "s_f2",
                    "f2",
                    ("s_f2_p", "points", json!({"std::vector": {"ref": "s_Point"}})),
                    true,
                ),
            ],
        ),
        // Each of these needs `base.hpp`: a field by value, in a list (whose
        // `{}` needs the type complete), in a map and in each other
        // container, in a comparable and in a printable struct.
        (
            "by_value",
            vec![shared_struct("by_value", "s_Bv", "Bv", json!({"ref": "s_Point"}))],
        ),
        (
            "listed",
            vec![shared_struct(
                "listed",
                "s_Ls",
                "Ls",
                json!({"std::vector": {"ref": "s_Point"}}),
            )],
        ),
        (
            "in_map",
            vec![shared_struct(
                "in_map",
                "s_Im",
                "Im",
                json!({"std::map": ["int", {"ref": "s_Point"}]}),
            )],
        ),
        (
            "compared",
            vec![comparable(shared_struct(
                "compared",
                "s_Cm",
                "Cm",
                json!({"std::vector": {"ref": "s_Point"}}),
            ))],
        ),
        (
            "printed",
            vec![printable(shared_struct(
                "printed",
                "s_Pr",
                "Pr",
                json!({"ref": "s_Point"}),
            ))],
        ),
        (
            "in_deque",
            vec![shared_struct(
                "in_deque",
                "s_Dq",
                "Dq",
                json!({"std::deque": {"ref": "s_Point"}}),
            )],
        ),
        (
            "in_set",
            vec![shared_struct(
                "in_set",
                "s_St",
                "St",
                json!({"std::set": {"ref": "s_Point"}}),
            )],
        ),
        (
            "in_queue",
            vec![shared_struct(
                "in_queue",
                "s_Qu",
                "Qu",
                json!({"std::queue": {"ref": "s_Point"}}),
            )],
        ),
        (
            "in_stack",
            vec![shared_struct(
                "in_stack",
                "s_Sk",
                "Sk",
                json!({"std::stack": {"ref": "s_Point"}}),
            )],
        ),
        (
            "in_priority_queue",
            vec![shared_struct(
                "in_priority_queue",
                "s_Pq",
                "Pq",
                json!({"std::priority_queue": {"ref": "s_Point"}}),
            )],
        ),
        (
            "defaults",
            vec![with_defaults(
                structure(
                    "s_Df",
                    "Df",
                    &[
                        ("s_df_a", "a", json!("int")),
                        ("s_df_b", "b", json!("int")),
                        ("s_df_c", "c", json!("int")),
                        ("s_df_d", "d", json!("int")),
                    ],
                    true,
                ),
                &[
                    (0, call("s_unit")),
                    (1, slot(&[r("s_kFixed")])),
                    (2, slot(&[r("s_kExtern")])),
                    (3, slot(&[r("s_total")])),
                ],
            )],
        ),
        (
            "typed",
            vec![global(
                "s_where",
                "where",
                "variable",
                json!({"ref": "s_Point"}),
                json!({"block": {"id": "b_new_point", "type": "type.new", "v": 1, "extra": {"fieldCount": 1},
                                 "fields": {"TYPE": r("s_Point")}, "inputs": {"FIELD0": slot(&[n("1")])}}}),
                true,
            )],
        ),
        (
            "valued",
            vec![global(
                "s_kColour",
                "kColour",
                "constant",
                json!({"ref": "s_Colour"}),
                enum_value("s_Colour"),
                true,
            )],
        ),
    ]);
    let analysis = analyze(&document);
    let errors: Vec<&str> = codes(&analysis)
        .into_iter()
        .filter(|code| code.starts_with("B2C-E"))
        .collect();
    assert!(errors.is_empty(), "{}", render_all(&analysis));
    let layout = &analysis.program.layout;
    let forward = &layout.modules[2];
    assert!(forward.header_includes.is_empty());
    assert_eq!(symbol_names(&analysis, &forward.forward), ["Point"]);
    // Its `.cpp` includes `base.hpp`, for the types its header
    // forward-declares.
    assert_eq!(ids(&forward.source_includes), ["mod_1", "mod_2"]);
    for module in 3..layout.modules.len() {
        let files = &layout.modules[module];
        assert_eq!(
            ids(&files.header_includes),
            ["mod_1"],
            "{}",
            analysis.program.modules[module].name
        );
        assert!(
            files.forward.is_empty(),
            "{}",
            analysis.program.modules[module].name
        );
    }
}

// --- Properties ----------------------------------------------------------------------

/// One module of a random program: a struct whose field is an `int` with a
/// default value, or another module's struct held by value or in a list; a
/// constant set from a number, from its module's own constant that it does
/// not share, or from another module's constant; and a shared variable and
/// function. What is shared is chosen too.
#[derive(Debug, Clone)]
struct RandomModule {
    struct_shared: bool,
    field: u8,
    field_target: usize,
    constant_shared: bool,
    constant_value: u8,
    constant_target: usize,
}

fn random_module() -> impl Strategy<Value = RandomModule> {
    (
        any::<bool>(),
        0..3_u8,
        0..4_usize,
        any::<bool>(),
        0..3_u8,
        0..4_usize,
    )
        .prop_map(
            |(struct_shared, field, field_target, constant_shared, constant_value, constant_target)| {
                RandomModule {
                    struct_shared,
                    field,
                    field_target,
                    constant_shared,
                    constant_value,
                    constant_target,
                }
            },
        )
}

/// The document of a random program; `offset` changes what no header shows
/// (a function's body, a variable's starting value, a non-shared constant
/// that only `extern const` constants read), and `order` the order of the
/// modules.
fn random_document(modules: &[RandomModule], offset: u8, order: &[usize]) -> b2c_model::Document {
    let count = modules.len();
    let mut built: Vec<(String, Vec<Value>)> = Vec::new();
    for (m, module) in modules.iter().enumerate() {
        let other = |target: usize| {
            let k = (m + 1 + target % (count - 1)) % count;
            (k != m).then_some(k)
        };
        let field = match (module.field, other(module.field_target)) {
            (1, Some(k)) if modules[k].struct_shared => json!({"ref": format!("s_S{k}")}),
            (2, Some(k)) if modules[k].struct_shared => json!({"std::vector": {"ref": format!("s_S{k}")}}),
            _ => json!("int"),
        };
        let defaulted = field == json!("int");
        let mut st = structure(
            &format!("s_S{m}"),
            &format!("S{m}"),
            &[(&format!("s_S{m}_f"), "f", field)],
            module.struct_shared,
        );
        if defaulted {
            st = with_defaults(st, &[(0, slot(&[n("7")]))]);
        }
        let value = match (module.constant_value, other(module.constant_target)) {
            (1, _) => slot(&[r(&format!("s_H{m}")), op("+"), n("1")]),
            (2, Some(k)) if modules[k].constant_shared => slot(&[r(&format!("s_K{k}")), op("+"), n("1")]),
            _ => slot(&[n("3")]),
        };
        let mut blocks = vec![
            st,
            constant(
                &format!("s_H{m}"),
                &format!("h{m}"),
                slot(&[n(&(10 + u32::from(offset)).to_string())]),
                false,
            ),
            constant(
                &format!("s_K{m}"),
                &format!("k{m}"),
                value,
                module.constant_shared,
            ),
            variable(
                &format!("s_V{m}"),
                &format!("v{m}"),
                slot(&[n(&offset.to_string())]),
                true,
            ),
            function(
                &format!("s_F{m}"),
                &format!("f{m}"),
                slot(&[
                    r(&format!("s_H{m}")),
                    op("+"),
                    r(&format!("s_V{m}")),
                    op("+"),
                    n(&offset.to_string()),
                ]),
                true,
            ),
        ];
        if m == 0 {
            blocks.push(main_block(vec![print(slot(&[json!({"str": "ok"})]))]));
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

/// The headers, by path.
fn headers(project: &GeneratedProject) -> Vec<(String, String)> {
    project
        .files
        .iter()
        .filter(|file| file.kind == FileKind::Header)
        .map(|file| (file.path.clone(), file.contents.clone()))
        .collect()
}

/// How many cases have been compiled with g++ (bounded: g++ is slow).
static COMPILED: AtomicUsize = AtomicUsize::new(0);

/// How many cases are compiled with g++ in one run.
const COMPILED_CASES: usize = 10;

/// A command for g++. Production code spawns processes only through
/// b2c-process (spec §8.5); tests drive g++ directly.
#[allow(clippy::disallowed_methods)]
fn gxx() -> Command {
    Command::new("g++")
}

/// Compiles each header alone through a one-line unit, then each `.cpp`,
/// and links the program.
fn compile_and_link(project: &GeneratedProject) -> Result<(), String> {
    let folder = std::env::temp_dir().join(format!(
        "b2c-modules-types-{}-{}",
        std::process::id(),
        COMPILED.load(Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    for file in &project.files {
        std::fs::write(folder.join(&file.path), &file.contents).map_err(|e| e.to_string())?;
    }
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
                        .args(["-std=c++20", "-Wall", "-Wextra", "-fsyntax-only", "-iquote"])
                        .arg(&folder)
                        .arg(&unit),
                    &format!("{} alone", file.path),
                )?;
            }
            FileKind::Source => {
                let object = folder.join(format!("{}.o", file.path));
                run(
                    gxx()
                        .args(["-std=c++20", "-Wall", "-Wextra", "-c", "-iquote"])
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
    fn random_programs_of_types_constants_and_globals_across_modules(
        modules in proptest::collection::vec(random_module(), 2..5),
        seed in any::<u64>(),
    ) {
        let order: Vec<usize> = (0..modules.len()).collect();
        let document = random_document(&modules, 0, &order);
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
        // An edit of what no header shows, keeping each shared constant's
        // form, leaves every header byte-identical.
        let edited = random_document(&modules, 3, &order);
        let (_, edited_project) = generated(&edited);
        prop_assert_eq!(headers(&edited_project), headers(&project));
        // The module order changes no file's text.
        let mut shuffled: Vec<usize> = order.clone();
        let count = shuffled.len();
        shuffled.rotate_left(usize::try_from(seed % u64::try_from(count).unwrap()).unwrap());
        let reordered = random_document(&modules, 0, &shuffled);
        let (_, reordered_project) = generated(&reordered);
        prop_assert_eq!(by_path(&reordered_project), by_path(&project));
        // With g++, a bounded number of programs without errors: each
        // header alone, and the whole program linked.
        if !errors && common::gxx::available() && COMPILED.fetch_add(1, Ordering::Relaxed) < COMPILED_CASES {
            let compiled = compile_and_link(&project);
            prop_assert!(compiled.is_ok(), "{}", compiled.unwrap_err());
        }
    }
}

/// A quick check that the property's documents are the ones meant: a struct
/// held by value from another module, a constant set from another module's,
/// one set from a constant its module does not share (`extern const`).
#[test]
fn the_random_programs_are_built_as_meant() {
    let modules = vec![
        RandomModule {
            struct_shared: true,
            field: 1,
            field_target: 0,
            constant_shared: true,
            constant_value: 2,
            constant_target: 0,
        },
        RandomModule {
            struct_shared: true,
            field: 0,
            field_target: 0,
            constant_shared: true,
            constant_value: 1,
            constant_target: 0,
        },
    ];
    let document = random_document(&modules, 0, &[0, 1]);
    let (analysis, project) = generated(&document);
    assert_clean(&analysis);
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
    assert!(files["m0.hpp"].contains("    S1 f{};"), "{}", files["m0.hpp"]);
    assert!(
        files["m0.hpp"].contains("extern const int k0;"),
        "{}",
        files["m0.hpp"]
    );
    assert!(
        files["m1.hpp"].contains("extern const int k1;"),
        "{}",
        files["m1.hpp"]
    );
    assert!(files["m1.hpp"].contains("int f = 7;"), "{}", files["m1.hpp"]);
}
