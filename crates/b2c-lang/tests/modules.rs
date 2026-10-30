//! The analyser's part of W3's contracts step for modules
//! (`docs/plans/m3-contracts/w3-modules.md` §3.1): `B2C-E0221`, the shared
//! flag of items and symbols, and the layout every program gets until the
//! packages that fill headers arrive. Functions across modules are tested
//! in `modules_functions.rs` (package F), types and globals in
//! `modules_types.rs` (package T).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod common;

use b2c_ir::Severity;
use b2c_ir::sast::{ItemKind, Program};
use common::{analyze, doc, doc_modules, func, main, sym};
use serde_json::{Value, json};

const RESERVED: &str =
    "Module names that start with `b2c` are kept for files that Blocks2Cpp writes. Rename the module.";

/// `define <name>` with *share with other modules* ticked or not.
fn define(name: &str, shared: bool) -> Value {
    let mut block = func(name, "void", &[], vec![]);
    block["fields"]["SHARED"] = json!(shared);
    block
}

/// `global variable [int] [name] = 0`, ticked or not.
fn global(name: &str, shared: bool) -> Value {
    json!({
        "id": format!("g_{name}"), "type": "var.global", "v": 1,
        "fields": {"KIND": "variable", "TYPE": "int", "NAME": {"sym": sym(name), "name": name},
                   "SHARED": shared},
        "inputs": {"VALUE": {"expr": [{"num": "0"}]}}
    })
}

/// `struct [name]` with no fields, ticked or not.
fn structure(name: &str, shared: bool) -> Value {
    json!({
        "id": format!("s_{name}"), "type": "type.struct", "v": 1,
        "fields": {"NAME": {"sym": sym(name), "name": name}, "SHARED": shared},
        "extra": {"fields": []}
    })
}

/// The names of a program's shared items, module by module.
fn shared_items(program: &Program) -> Vec<Vec<String>> {
    program
        .modules
        .iter()
        .map(|module| {
            module
                .items
                .iter()
                .filter(|item| item.shared)
                .map(|item| {
                    let symbol = match &item.kind {
                        ItemKind::Function(def) => &def.symbol,
                        ItemKind::Struct(def) => &def.symbol,
                        ItemKind::Enum(def) => &def.symbol,
                        ItemKind::Global(def) => &def.symbol,
                        ItemKind::Main(_) | ItemKind::UseHeader { .. } => unreachable!("never shared"),
                    };
                    program
                        .symbols
                        .get(symbol)
                        .expect("symbol")
                        .name
                        .as_str()
                        .to_owned()
                })
                .collect()
        })
        .collect()
}

#[test]
fn a_module_name_starting_with_b2c_is_e0221_in_a_project_of_one_module() {
    let mut document = doc(vec![main(vec![])]);
    document.modules[0].name = String::from("b2c_main");
    let analysis = analyze(&document);
    let found: Vec<_> = analysis
        .diagnostics
        .iter()
        .map(|d| (d.code.0.as_str(), d.severity, d.message.as_str()))
        .collect();
    assert_eq!(found, [("B2C-E0221", Severity::Error, RESERVED)]);
    let primary = &analysis.diagnostics[0].primary;
    assert_eq!(
        primary.module.as_ref().map(b2c_ir::ids::ModuleId::as_str),
        Some("mod_0")
    );
    assert_eq!(primary.block, None);
    // The file is named by the module's place, never by its name.
    assert_eq!(analysis.program.layout.modules[0].stem.as_str(), "module1");
}

#[test]
fn a_module_name_starting_with_b2c_is_e0221_in_a_project_of_several_modules() {
    let document = doc_modules(vec![
        ("main", vec![main(vec![])]),
        ("b2c-tools", vec![]),
        ("tools_b2c", vec![]),
    ]);
    let analysis = analyze(&document);
    let found: Vec<_> = analysis
        .diagnostics
        .iter()
        .map(|d| {
            (
                d.code.0.as_str(),
                d.primary.module.as_ref().map(|m| m.as_str().to_owned()),
            )
        })
        .collect();
    assert_eq!(found, [("B2C-E0221", Some(String::from("mod_1")))]);
    assert_eq!(analysis.diagnostics[0].message, RESERVED);
    let stems: Vec<&str> = analysis
        .program
        .layout
        .modules
        .iter()
        .map(|files| files.stem.as_str())
        .collect();
    assert_eq!(stems, ["main", "module2", "tools_b2c"]);
}

#[test]
fn definitions_are_shared_only_in_a_project_of_several_modules() {
    let blocks = || {
        vec![
            main(vec![]),
            define("helper", true),
            define("other", false),
            global("count", true),
            structure("Point", true),
        ]
    };
    // One module: the ticks are kept in the file but share nothing.
    let single = analyze(&doc(blocks()));
    assert!(single.diagnostics.is_empty(), "{:?}", single.diagnostics);
    assert_eq!(shared_items(&single.program), [Vec::<String>::new()]);
    assert!(single.symbol_infos().iter().all(|info| !info.shared));

    // Two modules: the ticked definitions are shared, in either module (in
    // item order, by block ID).
    let multi = analyze(&doc_modules(vec![
        ("main", blocks()),
        ("utils", vec![define("tool", true), define("spare", false)]),
    ]));
    assert!(multi.diagnostics.is_empty(), "{:?}", multi.diagnostics);
    assert_eq!(
        shared_items(&multi.program),
        [vec!["helper", "count", "Point"], vec!["tool"]]
    );
    let mut shared: Vec<String> = multi
        .symbol_infos()
        .into_iter()
        .filter(|info| info.shared)
        .map(|info| info.name)
        .collect();
    shared.sort();
    assert_eq!(shared, ["Point", "count", "helper", "tool"]);
    // The SAST writes `shared` only when it is true: `spare`, then `tool`.
    let json = serde_json::to_value(&multi.program.modules[1].items).expect("json");
    assert!(json[0].get("shared").is_none(), "{json}");
    assert_eq!(json[1]["shared"], true);
}

#[test]
fn every_program_keeps_the_flat_layout_until_headers_arrive() {
    let analysis = analyze(&doc_modules(vec![
        ("main", vec![main(vec![]), define("helper", false)]),
        ("utils", vec![define("tool", true)]),
    ]));
    let layout = &analysis.program.layout;
    assert!(layout.multi_module());
    for files in &layout.modules {
        assert!(!files.header, "{}", files.stem.as_str());
        assert!(files.header_order.is_empty());
        assert!(files.header_includes.is_empty() && files.forward.is_empty());
        assert!(files.source_includes.is_empty());
    }
    assert_eq!(layout.modules[0].source_order.len(), 1);
    assert_eq!(layout.modules[1].source_order.len(), 1);
    // No namespace and no spelling other than the default.
    assert!(analysis.program.spellings.is_empty());
    let items = analysis.program.modules.iter().flat_map(|module| &module.items);
    assert!(items.clone().all(|item| item.namespace.is_none()));
}
