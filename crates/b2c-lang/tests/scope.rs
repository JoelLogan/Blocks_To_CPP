//! The editor's questions about an analysis (spec §6.5): the scope query
//! `symbols_in_scope`, `symbol_infos`, `block_types`, and the published
//! conversion rule (spec §3.5.3).
//!
//! Besides cases for each rule, two tests tie the scope query to the
//! analyser's own name resolution: every reference in the examples is
//! offered where it is, and a property test checks on random programs that
//! a reference at a position is accepted exactly when the query offers it.

// Test code: unwrap/expect/panic and printing to stderr are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod common;

use std::collections::BTreeSet;
use std::fmt::Write as _;

use b2c_ir::diag::Severity;
use b2c_ir::sast::PassMode;
use b2c_ir::{BlockId, ModuleId, SymbolId, SymbolInfo, SymbolInfoKind, Type};
use b2c_lang::{Analysis, Conversion, analyze, conversion};
use b2c_model::{Document, FieldValue, Input, Token};
use common::*;
use proptest::prelude::*;
use serde_json::{Value, json};

fn block(id: &str) -> BlockId {
    BlockId::new(id).expect("block id")
}

fn sym(id: &str) -> SymbolId {
    SymbolId::new(id).expect("symbol id")
}

/// The ID of a block given as a statement (`{"id": …}`) or as an input
/// (`{"block": {"id": …}}`).
fn id_of(value: &Value) -> String {
    value.get("block").unwrap_or(value)["id"]
        .as_str()
        .expect("a block")
        .to_owned()
}

/// The names the scope query lists, in its order.
fn visible(a: &Analysis, block_id: &str, input: Option<&str>) -> Vec<String> {
    a.symbols_in_scope(&block(block_id), input)
        .into_iter()
        .map(|s| s.name)
        .collect()
}

/// A statement that refers to nothing, to ask what is visible at a place.
fn marker() -> Value {
    print(vec![num("1")])
}

/// `create <ty> <name>` with a symbol ID of its own (`common::declare`
/// derives the ID from the name).
fn declare_as(ty: &str, name: &str, sym: &str, value: Option<Value>) -> Value {
    let mut block = declare(ty, name, value);
    block["fields"]["NAME"]["sym"] = json!(sym);
    block
}

fn disabled(mut block: Value) -> Value {
    block["disabled"] = json!(true);
    block
}

const NONE: [&str; 0] = [];

// --- Visibility rules -----------------------------------------------------------

#[test]
fn a_variable_is_visible_after_its_declaration_in_the_same_and_nested_lists() {
    let before = marker();
    let decl = declare("int", "x", Some(num("1")));
    let after = marker();
    let in_if = marker();
    let in_nested_loop = marker();
    let in_while = marker();
    let ids = [&before, &decl, &after, &in_if, &in_nested_loop, &in_while].map(id_of);
    let a = run_main(vec![
        before,
        decl,
        after,
        if_then(boolean(true), vec![in_if, repeat(num("2"), vec![in_nested_loop])]),
        while_loop("while", boolean(true), vec![in_while]),
    ]);
    assert_eq!(visible(&a, &ids[0], None), NONE, "not before it");
    assert_eq!(visible(&a, &ids[1], None), NONE, "not at its own block");
    for id in &ids[2..] {
        assert_eq!(visible(&a, id, None), ["x"], "at {id}");
    }
    assert_eq!(visible(&a, "main", Some("BODY")), NONE);
    assert_eq!(visible(&a, "main", None), NONE);
}

#[test]
fn a_variable_is_not_visible_after_its_list_ends() {
    let inner = declare("int", "inner", None);
    let in_if = marker();
    let after_if = marker();
    let ids = [&in_if, &after_if].map(id_of);
    let a = run_main(vec![
        declare("int", "outer", None),
        if_then(boolean(true), vec![inner, in_if]),
        after_if,
    ]);
    assert_eq!(visible(&a, &ids[0], None), ["inner", "outer"]);
    assert_eq!(visible(&a, &ids[1], None), ["outer"]);
}

#[test]
fn sibling_lists_do_not_see_each_others_declarations() {
    let (first, second, other) = (marker(), marker(), marker());
    let ids = [&first, &second, &other].map(id_of);
    let branches = if_else(
        vec![
            (boolean(true), vec![declare("int", "a", None), first]),
            (boolean(false), vec![second]),
        ],
        Some(vec![other]),
    );
    let if_id = id_of(&branches);
    let a = run_main(vec![branches]);
    assert_eq!(visible(&a, &ids[0], None), ["a"]);
    assert_eq!(visible(&a, &ids[1], None), NONE);
    assert_eq!(visible(&a, &ids[2], None), NONE);
    for list in ["DO0", "DO1", "ELSE"] {
        assert_eq!(visible(&a, &if_id, Some(list)), NONE, "{list}");
    }
}

#[test]
fn the_for_counter_is_visible_only_in_its_body() {
    let (inside, after) = (marker(), marker());
    let header_value = get("i");
    let ids = [&inside, &after, &header_value].map(id_of);
    let looped = for_range("i", num("1"), "to", header_value, None, vec![inside]);
    let for_id = id_of(&looped);
    let a = run_main(vec![declare("int", "n", None), looped, after]);
    assert_eq!(visible(&a, &ids[0], None), ["i", "n"]);
    assert_eq!(visible(&a, &ids[1], None), ["n"]);
    assert_eq!(visible(&a, &for_id, None), ["n"], "not at the loop itself");
    assert_eq!(visible(&a, &for_id, Some("BODY")), ["i", "n"]);
    assert_eq!(visible(&a, &for_id, Some("TO")), ["n"], "not in its own header");
    assert_eq!(visible(&a, &ids[2], None), ["n"]);
    let loop_counter = a
        .symbols_in_scope(&block(&ids[0]), None)
        .into_iter()
        .find(|s| s.name == "i")
        .expect("the counter");
    assert_eq!(loop_counter.kind, SymbolInfoKind::LoopVariable);
    assert_eq!(loop_counter.ty, Type::Int);
    assert_eq!(loop_counter.decl_block, block(&for_id));
}

#[test]
fn parameters_are_visible_only_in_their_function() {
    let (in_function, in_main) = (marker(), marker());
    let ids = [&in_function, &in_main].map(id_of);
    let a = run(vec![
        main(vec![in_main]),
        func(
            "area",
            "void",
            &[("w", "double", "copy"), ("h", "int", "read_only")],
            vec![in_function],
        ),
    ]);
    assert_eq!(visible(&a, &ids[0], None), ["area", "h", "w"]);
    assert_eq!(visible(&a, &ids[1], None), ["area"]);
    assert_eq!(
        visible(&a, "fn_area", None),
        ["area"],
        "not at the definition itself"
    );
    assert_eq!(visible(&a, "fn_area", Some("BODY")), ["area", "h", "w"]);
    let symbols = a.symbols_in_scope(&block("fn_area"), Some("BODY"));
    let h = symbols.iter().find(|s| s.name == "h").expect("h");
    assert_eq!(
        h.kind,
        SymbolInfoKind::Parameter {
            mode: PassMode::ReadOnly
        }
    );
    assert_eq!(h.decl_block, block("fn_area"));
    let area = symbols.iter().find(|s| s.name == "area").expect("area");
    assert_eq!(
        area.kind,
        SymbolInfoKind::Function {
            params: vec![sym("sym_w"), sym("sym_h")],
            returns: Type::Void
        }
    );
}

#[test]
fn functions_are_visible_everywhere_in_their_module() {
    let (in_main, in_first, in_last, in_other_module) = (marker(), marker(), marker(), marker());
    let ids = [&in_main, &in_first, &in_last, &in_other_module].map(id_of);
    let mut off = func("off", "void", &[], vec![]);
    off["disabled"] = json!(true);
    let a = analyze(&doc_modules(vec![
        (
            "main",
            vec![
                // Defined after `main` (by block ID), and in either order.
                func("zeta", "int", &[], vec![in_last, ret(Some(num("1")))]),
                main(vec![in_main]),
                func("alpha", "void", &[], vec![in_first]),
                off,
            ],
        ),
        ("other", vec![func("helper", "void", &[], vec![in_other_module])]),
    ]));
    for id in &ids[..3] {
        assert_eq!(visible(&a, id, None), ["alpha", "zeta"], "at {id}");
    }
    assert_eq!(visible(&a, &ids[3], None), ["helper"]);
    let zeta = a
        .symbols_in_scope(&block(&ids[0]), None)
        .into_iter()
        .find(|s| s.name == "zeta")
        .expect("zeta");
    assert_eq!(zeta.ty, Type::Int);
    assert_eq!(zeta.module, ModuleId::new("mod_0").expect("id"));
}

#[test]
fn a_local_name_hides_a_function_with_that_name() {
    let (before, after) = (marker(), marker());
    let ids = [&before, &after].map(id_of);
    let a = run(vec![
        main(vec![before, declare_as("int", "tick", "v_tick", None), after]),
        func("tick", "void", &[], vec![]),
    ]);
    let kinds = |id: &str| -> Vec<(String, bool)> {
        a.symbols_in_scope(&block(id), None)
            .into_iter()
            .map(|s| (s.name, matches!(s.kind, SymbolInfoKind::Function { .. })))
            .collect()
    };
    assert_eq!(kinds(&ids[0]), [(String::from("tick"), true)]);
    assert_eq!(kinds(&ids[1]), [(String::from("tick"), false)]);
}

#[test]
fn an_inner_declaration_hides_the_outer_one() {
    let (before_inner, after_inner, after_if) = (marker(), marker(), marker());
    let ids = [&before_inner, &after_inner, &after_if].map(id_of);
    let a = run_main(vec![
        declare_as("int", "x", "x_outer", None),
        if_then(
            boolean(true),
            vec![
                before_inner,
                declare_as("std::string", "x", "x_inner", None),
                after_inner,
            ],
        ),
        after_if,
    ]);
    let ids_at = |id: &str| -> Vec<String> {
        a.symbols_in_scope(&block(id), None)
            .into_iter()
            .map(|s| s.id.to_string())
            .collect()
    };
    assert_eq!(ids_at(&ids[0]), ["x_outer"]);
    assert_eq!(ids_at(&ids[1]), ["x_inner"]);
    assert_eq!(ids_at(&ids[2]), ["x_outer"]);
}

#[test]
fn a_starting_value_sees_neither_its_variable_nor_one_it_hides() {
    let value = get("y");
    let value_id = id_of(&value);
    let shadowing = declare_as("int", "x", "x_inner", Some(value));
    let shadowing_id = id_of(&shadowing);
    let a = run_main(vec![
        declare_as("int", "x", "x_outer", None),
        declare("int", "y", None),
        if_then(boolean(true), vec![shadowing]),
    ]);
    assert_eq!(
        visible(&a, &shadowing_id, None),
        ["x", "y"],
        "at the block: the outer x"
    );
    assert_eq!(visible(&a, &shadowing_id, Some("VALUE")), ["y"]);
    assert_eq!(visible(&a, &value_id, None), ["y"]);
    // The analyser rejects the outer x there too (slot tokens refer to
    // `sym_<name>`, so the outer x is `sym_outer` here).
    let hidden = run_main(vec![
        declare_as("int", "x", "sym_outer", None),
        if_then(
            boolean(true),
            vec![declare_as("int", "x", "x_inner", Some(e("outer")))],
        ),
    ]);
    assert!(
        codes(&hidden).contains(&String::from("B2C-E0205")),
        "{}",
        render_all(&hidden)
    );
}

#[test]
fn disabled_declarations_are_left_out() {
    let (after, inside_disabled) = (marker(), marker());
    let ids = [&after, &inside_disabled].map(id_of);
    let a = run_main(vec![
        disabled(declare("int", "off", None)),
        disabled(if_then(
            boolean(true),
            vec![declare("int", "deeper", None), inside_disabled],
        )),
        declare("int", "on", None),
        after,
    ]);
    assert_eq!(visible(&a, &ids[0], None), ["on"]);
    assert_eq!(
        visible(&a, &ids[1], None),
        NONE,
        "a disabled block is not analysed"
    );
}

#[test]
fn statement_inputs_answer_at_the_start_of_the_list() {
    let body_marker = marker();
    let looped = while_loop(
        "while",
        boolean(true),
        vec![declare("int", "inside", None), body_marker],
    );
    let while_id = id_of(&looped);
    let branch = if_then(boolean(true), vec![]);
    let if_id = id_of(&branch);
    let a = run_main(vec![declare("int", "before", None), looped, branch]);
    assert_eq!(visible(&a, &while_id, Some("BODY")), ["before"]);
    assert_eq!(visible(&a, &while_id, Some("COND")), ["before"], "a value input");
    assert_eq!(visible(&a, &while_id, Some("MODE")), ["before"], "a field");
    assert_eq!(visible(&a, &if_id, Some("DO0")), ["before"], "an empty list");
    assert_eq!(
        visible(&a, &if_id, Some("ELSE")),
        ["before"],
        "a list the block does not have answers at the block"
    );
    assert_eq!(visible(&a, &while_id, Some("#params")), ["before"]);
}

#[test]
fn unknown_and_unattached_blocks_give_nothing() {
    let loose_marker = marker();
    let loose_id = id_of(&loose_marker);
    let nested = marker();
    let nested_id = id_of(&nested);
    let loose_loop = forever(vec![nested]);
    let a = run(vec![
        main(vec![declare("int", "x", None)]),
        func("f", "void", &[], vec![]),
        loose_marker,
        loose_loop,
    ]);
    assert_eq!(visible(&a, &loose_id, None), NONE);
    assert_eq!(visible(&a, &nested_id, None), NONE);
    assert_eq!(visible(&a, "no_such_block", None), NONE);
    assert_eq!(visible(&a, "no_such_block", Some("BODY")), NONE);
    // A document with no module at all.
    let empty: Document = serde_json::from_value(json!({
        "format": "blocks2cpp/project", "formatVersion": 1,
        "generator": {"app": "0.1.0", "catalog": "1.0.0"},
        "project": {"id": "p", "name": "T", "language": {"standard": "c++20"}},
        "modules": []
    }))
    .expect("document");
    let a = analyze(&empty);
    assert!(a.symbols_in_scope(&block("main"), None).is_empty());
    assert!(a.symbol_infos().is_empty());
    assert!(a.block_types().is_empty());
}

#[test]
fn results_are_sorted_by_name_then_id_with_the_users_names() {
    let probe = marker();
    let probe_id = id_of(&probe);
    let a = run_main(vec![
        declare_as("int", "b", "s2", None),
        if_then(
            boolean(true),
            vec![
                declare_as("int", "a", "s9", None),
                declare_as("int", "my var", "s1", None),
                declare_as("int", "Z", "s3", None),
                probe,
            ],
        ),
    ]);
    // An invalid name keeps the user's spelling (the program uses a placeholder).
    assert!(
        codes(&a).contains(&String::from("B2C-E0220")),
        "{}",
        render_all(&a)
    );
    assert_eq!(visible(&a, &probe_id, None), ["Z", "a", "b", "my var"]);
    let infos = a.symbol_infos();
    let names: Vec<&str> = infos.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Z", "a", "b", "my var"]);
}

#[test]
fn symbol_infos_lists_the_program_and_nothing_else() {
    let a = run(vec![
        main(vec![
            declare_const("double", "rate", num("1.5")),
            for_range("i", num("0"), "to", num("3"), None, vec![]),
            disabled(declare("int", "off", None)),
        ]),
        func(
            "twice",
            "int",
            &[("n", "int", "editable")],
            vec![ret(Some(e("n * 2")))],
        ),
        declare("int", "loose", None),
    ]);
    let infos = a.symbol_infos();
    let json: Vec<Value> = infos
        .iter()
        .map(|s| serde_json::to_value(s).expect("json"))
        .collect();
    let for_block = &json[0]["declBlock"];
    assert_eq!(
        json,
        vec![
            json!({"id": "sym_i", "name": "i", "kind": "loopVariable", "type": "int",
                   "module": "mod_0", "declBlock": for_block}),
            json!({"id": "sym_n", "name": "n", "kind": "parameter", "mode": "editable", "type": "int",
                   "module": "mod_0", "declBlock": "fn_twice"}),
            json!({"id": "sym_rate", "name": "rate", "kind": "variable", "isConst": true,
                   "type": "double", "module": "mod_0", "declBlock": json[2]["declBlock"]}),
            json!({"id": "sym_twice", "name": "twice", "kind": "function", "params": ["sym_n"],
                   "returns": "int", "type": "int", "module": "mod_0", "declBlock": "fn_twice"}),
        ]
    );
    // Round trip through JSON, as the editor receives them.
    let back: Vec<SymbolInfo> = serde_json::from_value(Value::Array(json)).expect("read back");
    assert_eq!(back, infos);
}

// --- The examples -----------------------------------------------------------------

#[test]
fn guessing_game_offers_what_each_block_can_use() {
    let a = analyze(&resolved_example("guessing_game.b2c"));
    assert!(a.diagnostics.is_empty(), "{}", render_all(&a));
    // b005 is the `ask` inside the loop, b002 creates `secret` (the first block).
    assert_eq!(visible(&a, "b005", None), ["guess", "secret"]);
    assert_eq!(visible(&a, "b002", None), NONE);
    assert_eq!(visible(&a, "b001", None), NONE, "inside secret's starting value");
    assert_eq!(visible(&a, "b003", None), ["secret"]);
    assert_eq!(visible(&a, "b010", Some("BODY")), ["guess", "secret"]);
    assert_eq!(visible(&a, "b009", Some("ELSE")), ["guess", "secret"]);
    assert_eq!(visible(&a, "b011", Some("BODY")), NONE);
    let at_ask: Vec<Value> = a
        .symbols_in_scope(&block("b005"), Some("VAR"))
        .iter()
        .map(|s| serde_json::to_value(s).expect("json"))
        .collect();
    assert_eq!(
        at_ask,
        [
            json!({"id": "s_guess", "name": "guess", "kind": "variable", "isConst": false,
                   "type": "int", "module": "mod_main", "declBlock": "b003"}),
            json!({"id": "s_secret", "name": "secret", "kind": "variable", "isConst": false,
                   "type": "int", "module": "mod_main", "declBlock": "b002"}),
        ]
    );
}

/// Every reference of a document: the block, the field or input it is in,
/// and the symbol.
fn references(document: &Document) -> Vec<(BlockId, String, SymbolId)> {
    let mut found = Vec::new();
    for (_, _, block) in all_blocks(document) {
        for (name, value) in &block.fields {
            if let FieldValue::Ref(reference) = value {
                found.push((block.id.clone(), name.clone(), reference.target.clone()));
            }
        }
        for (name, value) in &block.inputs {
            if let Input::Expr(slot) = value {
                for token in &slot.expr {
                    if let Token::Ref(target) = token {
                        found.push((block.id.clone(), name.clone(), target.clone()));
                    }
                }
            }
        }
    }
    found
}

#[test]
fn every_reference_in_the_examples_is_offered_where_it_is() {
    let mut total = 0;
    for (name, document) in resolved_examples() {
        let a = analyze(&document);
        assert!(
            !a.diagnostics.iter().any(|d| d.severity == Severity::Error),
            "{name}: {}",
            render_all(&a)
        );
        let all: BTreeSet<SymbolId> = a.symbol_infos().into_iter().map(|s| s.id).collect();
        for (block_id, input, target) in references(&document) {
            let offered = a.symbols_in_scope(&block_id, Some(&input));
            assert!(
                offered.iter().any(|s| s.id == target),
                "{name}: {block_id} ({input}) uses {target}, which the scope query does not offer there: {offered:?}"
            );
            total += 1;
        }
        // Every answer is sorted, without duplicates, and from the program.
        for (_, _, block) in all_blocks(&document) {
            let infos = a.symbols_in_scope(&block.id, None);
            let keys: Vec<(&str, &SymbolId)> = infos.iter().map(|s| (s.name.as_str(), &s.id)).collect();
            let mut sorted = keys.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(keys, sorted, "{name}: {}", block.id);
            assert!(infos.iter().all(|s| all.contains(&s.id)), "{name}: {}", block.id);
        }
    }
    assert!(total >= 40, "only {total} references checked");
}

#[test]
fn block_types_of_the_examples() {
    let mut text = String::new();
    for (name, document) in resolved_examples() {
        let a = analyze(&document);
        let types = a.block_types();
        writeln!(text, "{name}:").expect("write");
        if types.is_empty() {
            writeln!(text, "  (no value blocks)").expect("write");
        }
        for (block_id, ty) in &types {
            writeln!(text, "  {block_id}: {}", serde_json::to_value(ty).expect("type")).expect("write");
        }
        // Every typed block is a value block of the document.
        let value_blocks: BTreeSet<&BlockId> = all_blocks(&document)
            .into_iter()
            .flat_map(|(_, _, block)| block.inputs.values())
            .filter_map(|input| match input {
                Input::Block(nested) => Some(&nested.block.id),
                Input::Expr(_) => None,
            })
            .collect();
        for block_id in types.keys() {
            assert!(
                value_blocks.contains(block_id),
                "{name}: {block_id} is not a value block"
            );
        }
        assert!(types.values().all(|ty| *ty != Type::Error), "{name}: {types:?}");
    }
    insta::assert_snapshot!(text);
}

#[test]
fn block_types_cover_every_reporter() {
    let blocks = [
        (num("7"), Type::Int),
        (num("-2147483648"), Type::Int),
        (num("2.5"), Type::Double),
        (arith("add", num("1"), num("2")), Type::Int),
        (arith("div", num("1"), num("2.0")), Type::Double),
        (compare("lt", num("1"), num("2")), Type::Bool),
        (random_int(num("1"), num("6")), Type::Int),
        (convert("double", num("65")), Type::Double),
        (convert("int", num("6.5")), Type::Int),
        (boolean(true), Type::Bool),
        (
            logic("and", vec![boolean(true), boolean(false), boolean(true)]),
            Type::Bool,
        ),
        (not(boolean(false)), Type::Bool),
        (ternary(boolean(true), num("1"), num("2.0")), Type::Double),
        (text("hi"), Type::String),
        (chr("c"), Type::Char),
        (join(vec![text("a"), num("1")]), Type::String),
        (get("count"), Type::Int),
        (call("half", vec![num("4")]), Type::Double),
        (get("missing"), Type::Error),
        (call("count", vec![]), Type::Error),
    ];
    let expected: Vec<(String, Type)> = blocks.iter().map(|(b, ty)| (id_of(b), ty.clone())).collect();
    let mut body = vec![declare("int", "count", Some(num("0")))];
    body.extend(blocks.into_iter().map(|(b, _)| print(vec![b])));
    let ignored = call_stmt("half", vec![num("8")]);
    let (ignored_id, argument_id) = (id_of(&ignored), id_of(&ignored["inputs"]["ARG0"]));
    body.push(ignored);
    let document = doc(vec![
        main(body),
        func(
            "half",
            "double",
            &[("n", "int", "copy")],
            vec![ret(Some(e("n / 2.0")))],
        ),
    ]);
    let types = analyze(&document).block_types();
    for (block_id, ty) in &expected {
        assert_eq!(types.get(&block(block_id)), Some(ty), "{block_id}");
    }
    assert_eq!(types.get(&block(&argument_id)), Some(&Type::Int));
    assert!(
        !types.contains_key(&block(&ignored_id)),
        "a call used as a statement is not a value block"
    );
    // Exactly the value blocks are typed, the operands inside others too.
    let value_blocks: BTreeSet<BlockId> = all_blocks(&document)
        .into_iter()
        .flat_map(|(_, _, b)| b.inputs.values())
        .filter_map(|input| match input {
            Input::Block(nested) => Some(nested.block.id.clone()),
            Input::Expr(_) => None,
        })
        .collect();
    let with_types: BTreeSet<BlockId> = types.keys().cloned().collect();
    assert_eq!(with_types, value_blocks);
}

// --- The conversion rule -------------------------------------------------------------

const TYPES: [Type; 7] = [
    Type::Void,
    Type::Bool,
    Type::Char,
    Type::Int,
    Type::Double,
    Type::String,
    Type::Error,
];

#[test]
fn conversion_over_every_pair_of_types() {
    use Conversion::{BoolNumber as B, Invalid as X, Narrowing as N, Same as S, Widening as W};
    // Rows: from; columns: to; both in the order of `TYPES`.
    let table = [
        //   void bool char int double string error
        [S, X, X, X, X, X, S], // void
        [X, S, B, B, B, X, S], // bool
        [X, B, S, W, W, X, S], // char
        [X, B, N, S, W, X, S], // int
        [X, B, N, N, S, X, S], // double
        [X, X, X, X, X, S, S], // string
        [S, S, S, S, S, S, S], // error
    ];
    for (from, row) in TYPES.iter().zip(table) {
        for (to, expected) in TYPES.iter().zip(row) {
            assert_eq!(conversion(from, to), expected, "{from:?} -> {to:?}");
        }
    }
    let names: Vec<&str> = Conversion::ALL.iter().map(|c| c.name()).collect();
    assert_eq!(names, ["same", "widening", "narrowing", "boolNumber", "invalid"]);
    let errors: Vec<bool> = Conversion::ALL.iter().map(|c| c.is_error()).collect();
    assert_eq!(errors, [false, false, false, false, true]);
}

#[test]
fn the_analyser_reports_what_the_conversion_rule_says() {
    // Each type with the TYPE field that declares it.
    let types = [
        (Type::Bool, "bool"),
        (Type::Char, "char"),
        (Type::Int, "int"),
        (Type::Double, "double"),
        (Type::String, "std::string"),
    ];
    // A value block of a type.
    let value = |ty: &Type| match ty {
        Type::Bool => boolean(true),
        Type::Char => chr("a"),
        Type::Int => num("1"),
        Type::Double => num("1.5"),
        _ => text("a"),
    };
    for (from, _) in &types {
        for (to, field) in &types {
            let a = run_main(vec![declare(field, "v", Some(value(from)))]);
            let reported = codes(&a);
            let expected: &[&str] = match conversion(from, to) {
                Conversion::Same | Conversion::Widening => &[],
                Conversion::Narrowing => &["B2C-W0518"],
                Conversion::BoolNumber => &["B2C-W0519"],
                Conversion::Invalid => &["B2C-E0301"],
            };
            assert_eq!(reported, expected, "{from:?} -> {to:?}: {}", render_all(&a));
        }
    }
}

// --- The query matches name resolution on random programs -------------------------

/// Names shared by variables, parameters and functions, so that programs
/// redeclare, shadow and hide often.
const NAMES: [&str; 3] = ["a", "b", "f"];

/// A random statement.
#[derive(Debug, Clone)]
enum Gen {
    /// `create int <name>`, with a probe as its starting value or none.
    Declare {
        name: usize,
        probe_value: bool,
    },
    /// `print (probe)`: a variable probe.
    Probe,
    /// A call probe (`func.call_stmt` with no arguments).
    CallProbe,
    If {
        body: Vec<Gen>,
        else_body: Option<Vec<Gen>>,
    },
    While(Vec<Gen>),
    /// `for <name> from (0 or a probe) to 3`.
    For {
        name: usize,
        probe_from: bool,
        body: Vec<Gen>,
    },
    Disabled(Box<Gen>),
}

#[derive(Debug, Clone)]
struct GenFunction {
    name: usize,
    params: Vec<usize>,
    body: Vec<Gen>,
    disabled: bool,
    other_module: bool,
}

#[derive(Debug, Clone)]
struct GenProgram {
    main: Vec<Gen>,
    functions: Vec<GenFunction>,
}

fn gen_statement() -> impl Strategy<Value = Gen> {
    let leaf = prop_oneof![
        3 => (0..NAMES.len(), any::<bool>()).prop_map(|(name, probe_value)| Gen::Declare { name, probe_value }),
        2 => Just(Gen::Probe),
        1 => Just(Gen::CallProbe),
    ];
    leaf.prop_recursive(3, 24, 3, |inner| {
        let list = prop::collection::vec(inner.clone(), 1..3);
        prop_oneof![
            (list.clone(), prop::option::of(list.clone()))
                .prop_map(|(body, else_body)| Gen::If { body, else_body }),
            list.clone().prop_map(Gen::While),
            (0..NAMES.len(), any::<bool>(), list).prop_map(|(name, probe_from, body)| Gen::For {
                name,
                probe_from,
                body
            }),
            inner.prop_map(|g| Gen::Disabled(Box::new(g))),
        ]
    })
}

fn gen_program() -> impl Strategy<Value = GenProgram> {
    let function = (
        0..NAMES.len(),
        prop::collection::vec(0..NAMES.len(), 0..3),
        prop::collection::vec(gen_statement(), 0..3),
        prop::bool::weighted(0.15),
        prop::bool::weighted(0.25),
    )
        .prop_map(|(name, params, body, disabled, other_module)| GenFunction {
            name,
            params,
            body,
            disabled,
            other_module,
        });
    (
        prop::collection::vec(gen_statement(), 2..6),
        prop::collection::vec(function, 0..3),
    )
        .prop_map(|(main, functions)| GenProgram { main, functions })
}

/// A probe: a block whose reference the test points at each candidate.
#[derive(Debug)]
struct Probe {
    block: String,
    /// The symbol ID it refers to in the rendered text, unique to it.
    placeholder: String,
    call: bool,
    /// Inside a disabled block (so never analysed).
    disabled: bool,
    /// The variable whose starting value it is in, if any.
    initialises: Option<String>,
}

#[derive(Default)]
struct Renderer {
    next: usize,
    variables: Vec<String>,
    functions: Vec<String>,
    probes: Vec<Probe>,
}

impl Renderer {
    fn fresh(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}{}", self.next)
    }

    fn probe(&mut self, call: bool, disabled: bool, initialises: Option<String>) -> (String, String) {
        let block = self.fresh(if call { "pc" } else { "pv" });
        let placeholder = self.fresh("q");
        self.probes.push(Probe {
            block: block.clone(),
            placeholder: placeholder.clone(),
            call,
            disabled,
            initialises,
        });
        (block, placeholder)
    }

    fn var_probe(&mut self, disabled: bool, initialises: Option<String>) -> Value {
        let (id, target) = self.probe(false, disabled, initialises);
        json!({"block": {"id": id, "type": "var.get", "v": 1, "fields": {"VAR": {"ref": target}}}})
    }

    fn list(&mut self, list: &[Gen], disabled: bool) -> Value {
        Value::Array(list.iter().map(|g| self.statement(g, disabled)).collect())
    }

    fn statement(&mut self, statement: &Gen, disabled: bool) -> Value {
        let id = self.fresh("k");
        match statement {
            Gen::Declare { name, probe_value } => {
                let symbol = self.fresh("s");
                self.variables.push(symbol.clone());
                let mut block = json!({"id": id, "type": "var.declare", "v": 1,
                    "fields": {"TYPE": "int", "NAME": {"sym": symbol, "name": NAMES[*name]}}});
                if *probe_value {
                    block["inputs"] = json!({"VALUE": self.var_probe(disabled, Some(symbol))});
                }
                block
            }
            Gen::Probe => {
                let item = self.var_probe(disabled, None);
                json!({"id": id, "type": "io.print", "v": 1, "extra": {"itemCount": 1},
                       "inputs": {"ITEM0": item}})
            }
            Gen::CallProbe => {
                let (id, target) = self.probe(true, disabled, None);
                json!({"id": id, "type": "func.call_stmt", "v": 1, "fields": {"FUNC": {"ref": target}},
                       "extra": {"argCount": 0}})
            }
            Gen::If { body, else_body } => {
                let mut statements = serde_json::Map::new();
                statements.insert(String::from("DO0"), self.list(body, disabled));
                if let Some(other) = else_body {
                    statements.insert(String::from("ELSE"), self.list(other, disabled));
                }
                json!({"id": id, "type": "control.if", "v": 1,
                       "extra": {"elseIfCount": 0, "hasElse": else_body.is_some()},
                       "inputs": {"COND0": {"expr": [{"kw": "true"}]}}, "statements": statements})
            }
            Gen::While(body) => {
                let body = self.list(body, disabled);
                json!({"id": id, "type": "control.while", "v": 1, "fields": {"MODE": "while"},
                       "inputs": {"COND": {"expr": [{"kw": "true"}]}}, "statements": {"BODY": body}})
            }
            Gen::For {
                name,
                probe_from,
                body,
            } => {
                let symbol = self.fresh("s");
                self.variables.push(symbol.clone());
                let from = if *probe_from {
                    self.var_probe(disabled, None)
                } else {
                    json!({"expr": [{"num": "0"}]})
                };
                let body = self.list(body, disabled);
                json!({"id": id, "type": "control.for_range", "v": 1,
                       "fields": {"VAR": {"sym": symbol, "name": NAMES[*name]}, "DIRECTION": "to"},
                       "inputs": {"FROM": from, "TO": {"expr": [{"num": "3"}]}},
                       "statements": {"BODY": body}})
            }
            Gen::Disabled(inner) => {
                let mut block = self.statement(inner, true);
                block["disabled"] = json!(true);
                block
            }
        }
    }

    fn function(&mut self, function: &GenFunction) -> Value {
        let id = self.fresh("fn");
        let symbol = self.fresh("s");
        self.functions.push(symbol.clone());
        let params: Vec<Value> = function
            .params
            .iter()
            .map(|name| {
                let param = self.fresh("s");
                self.variables.push(param.clone());
                json!({"sym": param, "name": NAMES[*name], "type": "int", "mode": "copy"})
            })
            .collect();
        let body = self.list(&function.body, function.disabled);
        json!({"id": id, "type": "func.define", "v": 1, "disabled": function.disabled,
               "fields": {"NAME": {"sym": symbol, "name": NAMES[function.name]}, "RETURNS": "void"},
               "extra": {"params": params}, "statements": {"BODY": body}})
    }

    /// The project text and the probes and candidates in it.
    fn render(program: &GenProgram) -> (String, Self) {
        let mut renderer = Self::default();
        let body = renderer.list(&program.main, false);
        let mut modules = [
            vec![json!({"id": "main", "type": "program.main", "v": 1,
                                       "statements": {"BODY": body}})],
            Vec::new(),
        ];
        for function in &program.functions {
            let block = renderer.function(function);
            modules[usize::from(function.other_module)].push(block);
        }
        let [first, second] = modules;
        let text = json!({
            "format": "blocks2cpp/project", "formatVersion": 1,
            "generator": {"app": "0.1.0", "catalog": "1.0.0"},
            "project": {"id": "prj_test", "name": "Test", "language": {"standard": "c++20"}},
            "modules": [
                {"id": "mod_0", "name": "main", "workspace": {"blocks": first}},
                {"id": "mod_1", "name": "other", "workspace": {"blocks": second}}
            ]
        })
        .to_string();
        (text, renderer)
    }
}

/// Codes of the errors that say a reference cannot be used where it is.
const NAME_ERRORS: [&str; 7] = [
    "B2C-E0201",
    "B2C-E0202",
    "B2C-E0203",
    "B2C-E0204",
    "B2C-E0205",
    "B2C-E0206",
    "B2C-E0207",
];

fn analyse_text(text: &str) -> Analysis {
    analyze(&serde_json::from_str::<Document>(text).expect("generated document"))
}

/// Points each probe at each candidate and compares the analyser's verdict
/// with what the scope query offered there.
fn check_program(program: &GenProgram) -> Result<(), TestCaseError> {
    let (text, renderer) = Renderer::render(program);
    let base = analyse_text(&text);
    for probe in &renderer.probes {
        let here = base.symbols_in_scope(&block(&probe.block), None);
        if probe.disabled {
            prop_assert!(here.is_empty(), "{} is in a disabled block", probe.block);
            continue;
        }
        let offered: BTreeSet<String> = here
            .iter()
            .filter(|s| matches!(s.kind, SymbolInfoKind::Function { .. }) == probe.call)
            .map(|s| s.id.to_string())
            .collect();
        let candidates = if probe.call {
            &renderer.functions
        } else {
            &renderer.variables
        };
        for candidate in candidates {
            let edited = text.replace(&format!("\"{}\"", probe.placeholder), &format!("\"{candidate}\""));
            let a = analyse_text(&edited);
            let rejected = a.diagnostics.iter().any(|d| {
                d.primary
                    .block
                    .as_ref()
                    .is_some_and(|b| b.as_str() == probe.block)
                    && NAME_ERRORS.contains(&d.code.0.as_str())
            });
            let listed = offered.contains(candidate);
            // `int a = a + 1;` is legal C++ (with a warning), but the query
            // never offers a variable inside its own starting value.
            let own_value = probe.initialises.as_deref() == Some(candidate.as_str());
            prop_assert!(
                listed != rejected || (own_value && !listed),
                "probe {} with {candidate}: offered {listed}, rejected {rejected}\noffered: {offered:?}\n{}\n{text}",
                probe.block,
                render_all(&a)
            );
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn the_query_offers_exactly_what_the_analyser_accepts(program in gen_program()) {
        check_program(&program)?;
    }
}

#[test]
fn the_probe_harness_sees_hidden_names() {
    // A fixed program through the property test's harness: an outer `a`,
    // a loop counter `a` that hides it, a function `f` hidden by a parameter
    // and a function in another module.
    let program = GenProgram {
        main: vec![
            Gen::Declare {
                name: 0,
                probe_value: false,
            },
            Gen::For {
                name: 0,
                probe_from: true,
                body: vec![Gen::Probe, Gen::CallProbe],
            },
            Gen::Declare {
                name: 1,
                probe_value: true,
            },
            Gen::Disabled(Box::new(Gen::Probe)),
        ],
        functions: vec![
            GenFunction {
                name: 2,
                params: vec![2],
                body: vec![Gen::CallProbe, Gen::Probe],
                disabled: false,
                other_module: false,
            },
            GenFunction {
                name: 1,
                params: vec![],
                body: vec![],
                disabled: false,
                other_module: true,
            },
        ],
    };
    let (_, renderer) = Renderer::render(&program);
    assert_eq!(renderer.probes.len(), 7);
    check_program(&program).expect("the query matches the analyser");
}
