//! User namespaces, W3's package N1 (`docs/plans/m3-contracts/w3-modules.md`
//! §2.10, §2.12, §5.2): the *namespace* block's symbols and nesting, the
//! definitions in its mouth, `B2C-E0219` past 14 levels, `B2C-E0210` for a
//! namespace named like a definition, `B2C-E0220` for a reserved name, and
//! *use namespace*'s values. Lookup and spellings are package N2's.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_pass_by_value,
    clippy::print_stderr
)]

mod common;

use b2c_ir::Severity;
use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{ItemKind, SymbolKind};
use common::{analyse, analyze, assert_clean, assert_codes, doc, doc_modules, func, main, only, print};
use serde_json::{Value, json};

/// A *namespace* block named `name` (symbol `ns_<sym>`) holding `definitions`.
fn namespace(id: &str, sym: &str, name: &str, definitions: Vec<Value>) -> Value {
    json!({"id": id, "type": "program.namespace", "v": 1,
           "fields": {"NAME": {"sym": format!("ns_{sym}"), "name": name}},
           "statements": {"DEFINITIONS": definitions}})
}

fn struct_block(id: &str, name: &str) -> Value {
    json!({"id": id, "type": "type.struct", "v": 1,
           "fields": {"NAME": {"sym": format!("sym_{name}"), "name": name}},
           "extra": {"fields": [{"sym": format!("sym_{name}_x"), "name": "x", "type": "int"}]}})
}

fn ns(sym: &str) -> SymbolId {
    SymbolId::new(&format!("ns_{sym}")).unwrap()
}

/// Namespaces `n1` (outermost) to `n<depth>`, each in the one before, the
/// innermost holding a function.
fn nested(depth: usize) -> Value {
    let mut inner = vec![func("deep", "void", &[], vec![])];
    for level in (1..=depth).rev() {
        inner = vec![namespace(
            &format!("b_n{level}"),
            &format!("n{level}"),
            &format!("n{level}"),
            inner,
        )];
    }
    inner.remove(0)
}

#[test]
fn namespaces_are_symbols_nested_in_their_parent() {
    let a = analyse(&doc(vec![
        main(vec![]),
        namespace(
            "b_geo",
            "geo",
            "geo",
            vec![
                func("area", "void", &[], vec![]),
                namespace("b_shapes", "shapes", "shapes", vec![struct_block("b_box", "Box")]),
            ],
        ),
    ]));
    assert_clean(&a);
    let symbols = &a.program.symbols;
    let geo = symbols.get(&ns("geo")).unwrap();
    assert_eq!(geo.name.as_str(), "geo");
    assert_eq!(geo.kind, SymbolKind::Namespace { parent: None });
    let shapes = symbols.get(&ns("shapes")).unwrap();
    assert_eq!(
        shapes.kind,
        SymbolKind::Namespace {
            parent: Some(ns("geo"))
        }
    );
    assert_eq!(
        symbols.namespace_path(Some(&ns("shapes"))),
        [&ns("geo"), &ns("shapes")]
    );
    // The definitions in the mouths are definitions of the module.
    let kinds: Vec<&str> = a.program.modules[0]
        .items
        .iter()
        .map(|item| match &item.kind {
            ItemKind::Function(_) => "function",
            ItemKind::Struct(_) => "struct",
            ItemKind::Main(_) => "main",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds.len(), 3, "{kinds:?}");
    assert!(kinds.contains(&"function") && kinds.contains(&"struct"));
    // The editor learns of namespaces like any symbol.
    let info = a
        .symbol_infos()
        .into_iter()
        .find(|info| info.name == "shapes")
        .unwrap();
    assert_eq!(
        serde_json::to_value(&info).unwrap()["parent"],
        json!("ns_geo"),
        "{info:?}"
    );
}

/// A namespace opened in several places, in one module or in several, is one
/// namespace: the first block (in module order, then by ID) stands for it,
/// so the namespaces nested in each block have that block's symbol as their
/// parent.
#[test]
fn one_namespace_in_several_blocks_and_modules() {
    let a = analyze(&doc_modules(vec![
        (
            "main",
            vec![
                main(vec![]),
                namespace(
                    "b2",
                    "geo_b",
                    "geo",
                    vec![namespace("b3", "in_b", "inner", vec![])],
                ),
                namespace("b1", "geo_a", "geo", vec![]),
            ],
        ),
        (
            "utils",
            vec![namespace(
                "c1",
                "geo_c",
                "geo",
                vec![namespace("c2", "in_c", "inner", vec![])],
            )],
        ),
    ]));
    assert_clean(&a);
    let parent = |sym: &str| match &a.program.symbols.get(&ns(sym)).unwrap().kind {
        SymbolKind::Namespace { parent } => parent.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(parent("in_b"), Some(ns("geo_a")));
    assert_eq!(parent("in_c"), Some(ns("geo_a")));
    assert_eq!(parent("geo_c"), None);
}

#[test]
fn e0219_past_fourteen_levels() {
    let deep = analyse(&doc(vec![main(vec![]), nested(14)]));
    assert_clean(&deep);
    let too_deep = analyse(&doc(vec![main(vec![]), nested(16)]));
    // Only the first namespace past the limit is reported.
    assert_codes(&too_deep, &["B2C-E0219"]);
    let error = only(&too_deep, "B2C-E0219");
    assert_eq!(error.primary.block.as_ref().unwrap().as_str(), "b_n15");
    assert_eq!(
        error.message,
        "This namespace is inside 14 other namespaces, which is as deep as namespaces go. Move it, or the \
         namespaces in it, out of one of the namespaces around it."
    );
}

/// A namespace may not have the name of a definition of any module in the
/// same enclosing namespace, shared or not (w3-modules.md §2.1, K10).
#[test]
fn e0210_for_a_namespace_named_like_a_definition() {
    let a = analyze(&doc_modules(vec![
        (
            "main",
            vec![main(vec![]), namespace("b_ns", "helper", "helper", vec![])],
        ),
        ("utils", vec![func("helper", "void", &[], vec![])]),
    ]));
    assert_codes(&a, &["B2C-E0210"]);
    let error = only(&a, "B2C-E0210");
    assert_eq!(error.primary.block.as_ref().unwrap().as_str(), "b_ns");
    assert_eq!(
        error.message,
        "There is already a function called `helper` of module `utils` in the same place, and C++ would \
         not know which of the two the name means. Rename the namespace or the function."
    );
    assert_eq!(error.related.len(), 1);
    // In another namespace, the names do not meet.
    let apart = analyze(&doc(vec![
        main(vec![]),
        namespace(
            "b_ns",
            "geo",
            "geo",
            vec![namespace("b_in", "area", "area", vec![])],
        ),
        func("area", "void", &[], vec![]),
    ]));
    assert_clean(&apart);
    // A struct in the same enclosing namespace meets it.
    let inside = analyze(&doc(vec![
        main(vec![]),
        namespace(
            "b_ns",
            "geo",
            "geo",
            vec![
                namespace("b_in", "box", "Box", vec![]),
                struct_block("b_box", "Box"),
            ],
        ),
    ]));
    assert_codes(&inside, &["B2C-E0210"]);
}

#[test]
fn e0220_for_reserved_names_and_e0430_for_none() {
    let a = analyze(&doc(vec![
        main(vec![]),
        namespace("b_std", "s", "std", vec![]),
        namespace("b_b2c", "b", "b2c_tools", vec![]),
        json!({"id": "b_none", "type": "program.namespace", "v": 1}),
    ]));
    let found: Vec<(String, &str)> = a
        .diagnostics
        .iter()
        .map(|d| (d.code.0.clone(), d.primary.block.as_ref().unwrap().as_str()))
        .collect();
    assert_eq!(
        found,
        [
            ("B2C-E0220".to_owned(), "b_b2c"),
            ("B2C-E0430".to_owned(), "b_none"),
            ("B2C-E0220".to_owned(), "b_std"),
        ],
        "{:?}",
        a.diagnostics
    );
    assert!(a.diagnostics.iter().all(|d| d.severity == Severity::Error));
}

/// A disabled namespace takes its definitions out of the program with it.
#[test]
fn a_disabled_namespace_disables_its_definitions() {
    let mut off = namespace("b_ns", "geo", "geo", vec![func("area", "void", &[], vec![])]);
    off["disabled"] = json!(true);
    let a = analyze(&doc(vec![main(vec![]), off]));
    assert_clean(&a);
    assert_eq!(a.program.modules[0].items.len(), 1);
    assert!(a.program.symbols.get(&ns("geo")).is_none());
}

/// *use namespace*'s field holds `::std` for the standard namespace and a
/// user namespace's symbol ID otherwise (w3-modules.md §2.9): a namespace
/// whose symbol ID is `std` is never the standard one. The directive itself
/// is package N2's; here both values load and analyse cleanly.
#[test]
fn the_value_std_is_never_a_symbol_id() {
    let tools = json!({"id": "b_ns", "type": "program.namespace", "v": 1,
                       "fields": {"NAME": {"sym": "std", "name": "tools"}},
                       "statements": {"DEFINITIONS": []}});
    let uses = |value: &str, id: &str| json!({"id": id, "type": "program.use_namespace", "v": 1, "fields": {"NS": value}});
    let a = analyse(&doc(vec![
        main(vec![]),
        tools,
        uses("::std", "b_u1"),
        uses("std", "b_u2"),
    ]));
    assert_clean(&a);
    let symbol = a.program.symbols.get(&SymbolId::new("std").unwrap()).unwrap();
    assert_eq!(symbol.name.as_str(), "tools");
    // Neither directive is an item yet.
    assert_eq!(a.program.modules[0].items.len(), 1);
}

/// A namespace is no value: a reference to one, as a hand-edited file can
/// make, is `B2C-E0207` with its own wording.
#[test]
fn e0207_for_a_namespace_used_as_a_variable() {
    let a = analyze(&doc(vec![
        main(vec![print(vec![json!(
            {"block": {"id": "b_get", "type": "var.get", "v": 1, "fields": {"VAR": {"ref": "ns_geo"}}}}
        )])]),
        namespace("b_ns", "geo", "geo", vec![]),
    ]));
    assert_codes(&a, &["B2C-E0207"]);
    assert_eq!(
        only(&a, "B2C-E0207").message,
        "`geo` is a namespace, not a variable: choose one of the definitions it holds."
    );
}
