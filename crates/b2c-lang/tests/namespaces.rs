//! User namespaces, W3's package N1 (`docs/plans/m3-contracts/w3-modules.md`
//! §2.10, §2.12, §5.2): the *namespace* block's symbols and nesting, the
//! definitions in its mouth, `B2C-E0219` past 14 levels, `B2C-E0210` for a
//! namespace named like a definition, `B2C-E0220` for a reserved name, and
//! *use namespace*'s values; package N2's lookup model: each definition's
//! namespace, names unique per namespace, the spellings of references
//! (06 §6.14.2, §6.14.6), the directives, `B2C-E0215`, `B2C-I0523`,
//! `B2C-E0201`, `B2C-E0207` and `B2C-E0203` for *use namespace*, `B2C-E0213`
//! per namespace and the scope query's namespaces.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_pass_by_value,
    clippy::print_stderr
)]

mod common;

use b2c_ir::Severity;
use b2c_ir::ids::{BlockId, ModuleId, SymbolId};
use b2c_ir::sast::{ItemKind, SymbolKind};
use b2c_ir::spelling::{DirectiveSource, Spelling};
use common::{
    analyse, analyze, assert_clean, assert_codes, codes, doc, doc_modules, func, main, only, print,
};
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

// --- Package N2: lookup, spellings and directives (w3-modules.md §2.12) -----

/// `define <name>` (block `fn_<sym>`) giving `ret`, with inputs `(symbol,
/// name, type)` taken as copies, shared or not.
fn define(sym: &str, name: &str, ret: &str, params: &[(&str, &str, Value)], body: Vec<Value>) -> Value {
    let params: Vec<Value> = params
        .iter()
        .map(|(sym, name, ty)| json!({"sym": sym, "name": name, "type": ty, "mode": "copy"}))
        .collect();
    json!({"id": format!("fn_{sym}"), "type": "func.define", "v": 1,
           "fields": {"NAME": {"sym": sym, "name": name}, "RETURNS": ret},
           "extra": {"params": params}, "statements": {"BODY": body}})
}

/// `define <name>` giving `int`, its body `return <value>`.
fn giving(sym: &str, name: &str, value: Value) -> Value {
    define(sym, name, "int", &[], vec![common::ret(Some(value))])
}

fn shared(mut block: Value) -> Value {
    block["fields"]["SHARED"] = json!(true);
    block
}

/// A call (block `id`) of the function `sym`, as a value.
fn call_at(id: &str, sym: &str, args: Vec<Value>) -> Value {
    let inputs: serde_json::Map<String, Value> = args
        .into_iter()
        .enumerate()
        .map(|(i, arg)| (format!("ARG{i}"), arg))
        .collect();
    let count = inputs.len();
    json!({"block": {"id": id, "type": "func.call", "v": 1, "fields": {"FUNC": {"ref": sym}},
                     "extra": {"argCount": count}, "inputs": inputs}})
}

/// Reads the variable or global `sym` (block `id`).
fn read_at(id: &str, sym: &str) -> Value {
    json!({"block": {"id": id, "type": "var.get", "v": 1, "fields": {"VAR": {"ref": sym}}}})
}

/// `use namespace <value> in this file` (block `id`).
fn uses(id: &str, value: &str) -> Value {
    json!({"id": id, "type": "program.use_namespace", "v": 1, "fields": {"NS": value}})
}

/// A global `int` variable (block `gl_<sym>`) starting at 0.
fn int_global(sym: &str, name: &str) -> Value {
    json!({"id": format!("gl_{sym}"), "type": "var.global", "v": 1,
           "fields": {"KIND": "variable", "TYPE": "int", "NAME": {"sym": sym, "name": name}},
           "inputs": {"VALUE": common::num("0")}})
}

fn spelled(a: &b2c_lang::Analysis, block: &str, sym: &str) -> Spelling {
    a.program
        .spellings
        .reference(&BlockId::new(block).unwrap(), &SymbolId::new(sym).unwrap())
}

/// Each definition in a mouth knows its innermost namespace; the others
/// none.
#[test]
fn items_carry_their_namespace() {
    let a = analyze(&doc(vec![
        main(vec![]),
        giving("t_one", "one", common::num("1")),
        namespace(
            "b_geo",
            "geo",
            "geo",
            vec![
                giving("g_area", "area", common::num("2")),
                namespace("b_flat", "flat", "flat", vec![struct_block("b_box", "Box")]),
            ],
        ),
    ]));
    assert_clean(&a);
    let of = |block: &str| {
        a.program.modules[0]
            .items
            .iter()
            .find(|item| item.origin.block.as_str() == block)
            .unwrap()
            .namespace
            .clone()
    };
    assert_eq!(of("fn_t_one"), None);
    assert_eq!(of("fn_g_area"), Some(ns("geo")));
    assert_eq!(of("b_box"), Some(ns("flat")));
    // No directive and no reference across namespaces: nothing to record
    // beyond the defaults.
    assert!(a.program.spellings.directives.is_empty());
}

/// A name is unique in its namespace (`B2C-E0210`), not in its module:
/// `count` and `geo::count` may both exist.
#[test]
fn e0210_per_namespace() {
    let a = analyze(&doc(vec![
        main(vec![]),
        int_global("t_count", "count"),
        giving("t_area", "area", common::num("1")),
        namespace(
            "b_geo",
            "geo",
            "geo",
            vec![
                int_global("g_count", "count"),
                giving("g_area", "area", common::num("2")),
            ],
        ),
    ]));
    assert_clean(&a);
    let a = analyze(&doc(vec![
        main(vec![]),
        namespace(
            "b_geo",
            "geo",
            "geo",
            vec![int_global("g_count", "count"), int_global("g_count2", "count")],
        ),
    ]));
    assert_codes(&a, &["B2C-E0210"]);
}

/// 06 §6.14.2: a member of `geo` is written `geo::area` outside `geo` and
/// plainly inside it; a global hidden by a member of `geo` is `::size`
/// there.
#[test]
fn qualified_and_global_spellings() {
    let a = analyze(&doc(vec![
        main(vec![print(vec![
            call_at("c_main_area", "g_area", vec![]),
            call_at("c_main_size", "t_size", vec![]),
            call_at("c_main_gsize", "g_size", vec![]),
        ])]),
        giving("t_size", "size", common::num("2")),
        namespace(
            "b_geo",
            "geo",
            "geo",
            vec![
                define(
                    "g_area",
                    "area",
                    "int",
                    &[],
                    vec![common::ret(Some(common::arith(
                        "add",
                        call_at("c_in_size", "t_size", vec![]),
                        call_at("c_in_gsize", "g_size", vec![]),
                    )))],
                ),
                giving("g_size", "size", common::num("1")),
            ],
        ),
    ]));
    assert_clean(&a);
    assert_eq!(spelled(&a, "c_main_area", "g_area"), Spelling::Qualified);
    assert_eq!(spelled(&a, "c_main_size", "t_size"), Spelling::Plain);
    assert_eq!(spelled(&a, "c_main_gsize", "g_size"), Spelling::Qualified);
    assert_eq!(spelled(&a, "c_in_size", "t_size"), Spelling::Global);
    assert_eq!(spelled(&a, "c_in_gsize", "g_size"), Spelling::Plain);
}

/// *use namespace geo in this file* makes `area` plain in `main`, while a
/// name both the global namespace and `geo` declare stays qualified; the
/// `.cpp` declares `geo` empty first, as no header declares it.
#[test]
fn a_user_directive_shortens_what_it_can() {
    let a = analyze(&doc(vec![
        main(vec![print(vec![
            call_at("c_area", "g_area", vec![]),
            call_at("c_size", "t_size", vec![]),
            call_at("c_gsize", "g_size", vec![]),
        ])]),
        uses("b_use", "ns_geo"),
        giving("t_size", "size", common::num("2")),
        namespace(
            "b_geo",
            "geo",
            "geo",
            vec![
                giving("g_area", "area", common::num("1")),
                giving("g_size", "size", common::num("3")),
            ],
        ),
    ]));
    assert_clean(&a);
    assert_eq!(spelled(&a, "c_area", "g_area"), Spelling::Plain);
    assert_eq!(spelled(&a, "c_size", "t_size"), Spelling::Global);
    assert_eq!(spelled(&a, "c_gsize", "g_size"), Spelling::Qualified);
    let module = &a.program.modules[0].id;
    let directives = &a.program.spellings.directives[module];
    assert_eq!(directives.std, None);
    assert_eq!(directives.user, [(ns("geo"), BlockId::new("b_use").unwrap())]);
    assert_eq!(directives.declare_empty, [ns("geo")]);
}

/// Under `using namespace std;` a user `max` called with numbers is
/// `::max(…)` and a user global `count` is `::count`; neither is written
/// short as a standard name, `cout` is. Textbook style does the same.
#[test]
fn std_directive_qualifies_user_names_the_standard_declares() {
    let program = |directive: bool| {
        let mut blocks = vec![
            main(vec![print(vec![
                call_at("c_max", "t_max", vec![common::num("1"), common::num("2")]),
                read_at("r_count", "t_count"),
            ])]),
            define(
                "t_max",
                "max",
                "int",
                &[("p_a", "a", json!("int")), ("p_b", "b", json!("int"))],
                vec![common::ret(Some(read_at("r_a", "p_a")))],
            ),
            int_global("t_count", "count"),
        ];
        if directive {
            blocks.push(uses("b_std", "::std"));
        }
        doc(blocks)
    };
    let plain = analyze(&program(false));
    assert_clean(&plain);
    assert!(
        plain.program.spellings.is_empty(),
        "{:?}",
        plain.program.spellings
    );
    for textbook in [false, true] {
        let mut document = program(!textbook);
        document.project.options.using_namespace_std = textbook;
        let a = analyze(&document);
        assert_clean(&a);
        assert_eq!(spelled(&a, "c_max", "t_max"), Spelling::Global);
        assert_eq!(spelled(&a, "r_count", "t_count"), Spelling::Global);
        let module = &a.program.modules[0].id;
        let short = &a.program.spellings.short_standard[module];
        assert!(short.contains("cout") && short.contains("string"));
        assert!(!short.contains("count") && !short.contains("max") && !short.contains("move"));
        let source = a.program.spellings.directives[module].std.clone();
        if textbook {
            assert_eq!(source, Some(DirectiveSource::Setting));
        } else {
            assert_eq!(
                source,
                Some(DirectiveSource::Block(BlockId::new("b_std").unwrap()))
            );
        }
    }
}

/// 06 §6.14.6: argument-dependent lookup. A global `show` called with a
/// `geo::Point` is `::show(…)` because `geo` declares a `show` too. A user
/// `size` called with text stays plain: its parameter matches exactly, so
/// it wins over `std::size`'s template (w3-modules.md §7.1).
#[test]
fn argument_dependent_lookup_qualifies_calls() {
    let point = json!({"block": {"id": "b_new", "type": "type.new", "v": 1, "extra": {"fieldCount": 1},
                                 "fields": {"TYPE": {"ref": "sym_Point"}}, "inputs": {"FIELD0": common::num("1")}}});
    let text = json!({"block": {"id": "b_text", "type": "text.literal", "v": 1, "fields": {"VALUE": "abc"}}});
    let a = analyze(&doc(vec![
        main(vec![print(vec![
            call_at("c_show", "t_show", vec![point]),
            call_at("c_size", "t_size", vec![text]),
        ])]),
        define(
            "t_show",
            "show",
            "int",
            &[("p_p", "p", json!({"ref": "sym_Point"}))],
            vec![common::ret(Some(common::num("1")))],
        ),
        define(
            "t_size",
            "size",
            "int",
            &[("p_t", "t", json!("std::string"))],
            vec![common::ret(Some(common::num("2")))],
        ),
        namespace(
            "b_geo",
            "geo",
            "geo",
            vec![
                struct_block("b_point", "Point"),
                define(
                    "g_show",
                    "show",
                    "int",
                    &[("p_q", "q", json!({"ref": "sym_Point"}))],
                    vec![common::ret(Some(common::num("3")))],
                ),
            ],
        ),
    ]));
    assert_clean(&a);
    assert_eq!(spelled(&a, "c_show", "t_show"), Spelling::Global);
    assert_eq!(spelled(&a, "c_size", "t_size"), Spelling::Plain);
    // The parameter's type, written in `show`'s signature outside `geo`.
    assert_eq!(spelled(&a, "fn_t_show", "sym_Point"), Spelling::Qualified);
}

/// `B2C-E0215`: a type named like a standard type where `std` is
/// nominated, with the project's standard's names only; a namespace named
/// like a standard namespace named where `std` is nominated.
#[test]
fn e0215_for_standard_type_and_namespace_names() {
    let with = |name: &str, standard: &str, directive: bool| {
        let mut blocks = vec![main(vec![]), struct_block("b_struct", name)];
        if directive {
            blocks.push(uses("b_std", "::std"));
        }
        let mut document = doc(blocks);
        document.project.language.standard = serde_json::from_value(json!(standard)).unwrap();
        analyze(&document)
    };
    assert_clean(&with("list", "c++20", false));
    let a = with("list", "c++20", true);
    assert_codes(&a, &["B2C-E0215"]);
    let problem = only(&a, "B2C-E0215");
    assert_eq!(problem.primary.block.as_ref().unwrap().as_str(), "b_struct");
    assert!(problem.message.contains("`List`"), "{}", problem.message);
    // `std::span` is C++20's.
    assert_clean(&with("span", "c++17", true));
    assert_codes(&with("span", "c++20", true), &["B2C-E0215"]);
    // A namespace `chrono` whose member `main` calls.
    let a = analyze(&doc(vec![
        main(vec![print(vec![call_at("c_tick", "c_tick_fn", vec![])])]),
        uses("b_std", "::std"),
        namespace(
            "b_chrono",
            "chrono",
            "chrono",
            vec![giving("c_tick_fn", "tick", common::num("1"))],
        ),
    ]));
    assert_codes(&a, &["B2C-E0215"]);
    assert_eq!(
        only(&a, "B2C-E0215").primary.block.as_ref().unwrap().as_str(),
        "b_chrono"
    );
}

/// `B2C-I0523` (06 §6.14.3 rule 5): a second block for one namespace, and
/// a `std` block with Textbook style on.
#[test]
fn i0523_for_a_namespace_already_in_use() {
    let blocks = || {
        vec![
            main(vec![]),
            uses("b_std2", "::std"),
            uses("b_std1", "::std"),
            uses("b_geo2", "ns_geo"),
            uses("b_geo1", "ns_geo"),
            namespace(
                "b_geo",
                "geo",
                "geo",
                vec![giving("g_area", "area", common::num("1"))],
            ),
        ]
    };
    let a = analyze(&doc(blocks()));
    assert_codes(&a, &["B2C-I0523", "B2C-I0523"]);
    let flagged: Vec<&str> = a
        .diagnostics
        .iter()
        .map(|d| d.primary.block.as_ref().unwrap().as_str())
        .collect();
    assert!(
        flagged.contains(&"b_std2") && flagged.contains(&"b_geo2"),
        "{flagged:?}"
    );
    assert!(a.diagnostics.iter().all(|d| d.severity == Severity::Info));
    let mut document = doc(blocks());
    document.project.options.using_namespace_std = true;
    let a = analyze(&document);
    assert_codes(&a, &["B2C-I0523", "B2C-I0523", "B2C-I0523"]);
    assert!(a.diagnostics.iter().any(|d| d.message.contains("Textbook style")));
}

/// What *use namespace* may name: `B2C-E0201` for a namespace that is gone,
/// `B2C-E0207` for a nested one, `B2C-E0203` for one only other modules
/// have and do not share anything of. Naming one another module's header
/// declares includes that header, and needs no empty declaration.
#[test]
fn what_use_namespace_may_name() {
    let a = analyze(&doc(vec![main(vec![]), uses("b_use", "ns_gone")]));
    assert_codes(&a, &["B2C-E0201"]);
    let a = analyze(&doc(vec![
        main(vec![]),
        uses("b_use", "ns_inner"),
        namespace(
            "b_geo",
            "geo",
            "geo",
            vec![namespace("b_inner", "inner", "inner", vec![])],
        ),
    ]));
    assert_codes(&a, &["B2C-E0207"]);
    assert!(only(&a, "B2C-E0207").message.contains("`geo`"));
    let project = |share: bool| {
        let area = giving("g_area", "area", common::num("1"));
        let area = if share { shared(area) } else { area };
        doc_modules(vec![
            (
                "main",
                vec![
                    main(vec![print(vec![call_at("c_area", "g_area", vec![])])]),
                    uses("b_use", "ns_geo"),
                ],
            ),
            ("utils", vec![namespace("b_geo", "geo", "geo", vec![area])]),
        ])
    };
    let a = analyze(&project(false));
    assert!(codes(&a).contains(&String::from("B2C-E0203")), "{:?}", codes(&a));
    let a = analyze(&project(true));
    assert_clean(&a);
    let main_files = &a.program.layout.modules[0];
    assert!(
        main_files
            .source_includes
            .contains(&ModuleId::new("mod_1").unwrap())
    );
    let directives = &a.program.spellings.directives[&main_files.module];
    assert!(directives.declare_empty.is_empty());
    assert_eq!(spelled(&a, "c_area", "g_area"), Spelling::Plain);
}

/// `B2C-E0213` per namespace (w3-modules.md §1.9, W3M-D10): a member that
/// is not shared may not have the name of a member of the same namespace
/// another module shares, whatever its file includes; a global of that
/// name is fine.
#[test]
fn e0213_per_namespace() {
    let project = |mine: Value| {
        doc_modules(vec![
            ("main", vec![main(vec![]), mine]),
            (
                "utils",
                vec![namespace(
                    "b_geo",
                    "geo",
                    "geo",
                    vec![shared(giving("u_area", "area", common::num("1")))],
                )],
            ),
        ])
    };
    let a = analyze(&project(namespace(
        "b_geo_main",
        "geo_main",
        "geo",
        vec![giving("m_area", "area", common::num("2"))],
    )));
    assert_codes(&a, &["B2C-E0213"]);
    let a = analyze(&project(giving("m_area", "area", common::num("2"))));
    assert_clean(&a);
}

/// The scope query lists a namespace's members and the top-level
/// namespaces: the module's own, and another module's that shares a
/// member.
#[test]
fn the_scope_query_lists_members_and_namespaces() {
    let a = analyze(&doc_modules(vec![
        (
            "main",
            vec![
                main(vec![print(vec![call_at("c_area", "g_area", vec![])])]),
                giving("t_area", "area", common::num("2")),
                namespace(
                    "b_geo",
                    "geo",
                    "geo",
                    vec![giving("g_area", "area", common::num("1"))],
                ),
            ],
        ),
        (
            "utils",
            vec![namespace(
                "b_shapes",
                "shapes",
                "shapes",
                vec![shared(giving("s_circle", "circle", common::num("3")))],
            )],
        ),
    ]));
    assert_clean(&a);
    let seen: Vec<String> = a
        .symbols_in_scope(&BlockId::new("c_area").unwrap(), None)
        .into_iter()
        .map(|info| info.id.as_str().to_owned())
        .collect();
    for wanted in ["g_area", "t_area", "ns_geo", "ns_shapes", "s_circle"] {
        assert!(seen.iter().any(|sym| sym == wanted), "{wanted} in {seen:?}");
    }
}
