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
        declare("int", "before", None),
        disabled(declare("int", "off", None)),
        disabled(if_then(
            boolean(true),
            vec![declare("int", "deeper", None), inside_disabled],
        )),
        declare("int", "on", None),
        after,
    ]);
    assert_eq!(visible(&a, &ids[0], None), ["before", "on"]);
    assert_eq!(
        visible(&a, &ids[1], None),
        ["before"],
        "a block inside a disabled block answers like it, and `deeper` is not declared"
    );
}

#[test]
fn a_disabled_statement_answers_for_its_position() {
    let off_marker = disabled(marker());
    let off_declare = disabled(declare("int", "off", None));
    let off_loop = disabled(repeat(num("2"), vec![]));
    let inner_off = disabled(marker());
    let ids = [&off_marker, &off_declare, &off_loop, &inner_off].map(id_of);
    let a = run(vec![
        main(vec![
            declare("int", "first", None),
            off_marker,
            declare("int", "second", None),
            off_declare,
            if_then(boolean(true), vec![off_loop]),
        ]),
        func("helper", "void", &[], vec![]),
        // A disabled statement in a loose block is parked like the block.
        forever(vec![inner_off]),
    ]);
    // Like an enabled block there: what is declared before it, and the
    // module's functions. A disabled declaration declares nothing, so the
    // same is visible at it and after it.
    assert_eq!(visible(&a, &ids[0], None), ["first", "helper"]);
    assert_eq!(visible(&a, &ids[1], None), ["first", "helper", "second"]);
    assert_eq!(
        visible(&a, &ids[1], Some("VALUE")),
        ["first", "helper", "second"],
        "its inputs answer at the block"
    );
    assert_eq!(visible(&a, &ids[2], None), ["first", "helper", "second"]);
    assert_eq!(
        visible(&a, &ids[2], Some("BODY")),
        ["first", "helper", "second"],
        "its own lists are never lowered, so they answer at the block"
    );
    assert_eq!(
        visible(&a, &ids[3], None),
        ["first", "helper", "second"],
        "the parked scope: the end of main"
    );
    // Nothing a disabled block declares is in the program.
    assert!(a.symbol_infos().iter().all(|s| s.name != "off"));
}

#[test]
fn a_disabled_statement_answers_like_an_enabled_one_near_the_depth_limit() {
    // A disabled and an enabled statement side by side, inside more and
    // more loops: both answer, or (nested too deeply) neither does.
    let mut empty_or_not = BTreeSet::new();
    for levels in 60..68 {
        let (off, on) = (disabled(marker()), marker());
        let ids = [&off, &on].map(id_of);
        let mut nested = forever(vec![off, on]);
        for _ in 1..levels {
            nested = forever(vec![nested]);
        }
        let a = run_main(vec![declare("int", "x", None), nested]);
        let found = visible(&a, &ids[1], None);
        assert_eq!(visible(&a, &ids[0], None), found, "at {levels} levels");
        empty_or_not.insert(found.is_empty());
    }
    assert_eq!(empty_or_not.len(), 2, "both sides of the limit are tested");
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

/// A loose stack (05 §5.4): `head` on the canvas with `stacked` below it.
fn loose(mut head: Value, stacked: Vec<Value>) -> Value {
    head["x"] = json!(40);
    head["y"] = json!(400);
    if !stacked.is_empty() {
        head["stack"] = Value::Array(stacked);
    }
    head
}

#[test]
fn parked_blocks_answer_with_the_end_of_main() {
    // 06 §6.5: a parked block (loose, in a parked stack, or nested in one)
    // sees what is visible at the end of its module's `main` body.
    let head = marker();
    let parked_declare = declare("int", "parked", Some(num("1")));
    let below = marker();
    let nested = marker();
    let parked_loop = for_range("k", num("0"), "to", num("3"), None, vec![nested.clone()]);
    let in_value = get("x");
    let off_head = disabled(marker());
    let under_off = marker();
    let reporter = json!({"id": "loose_value", "type": "var.get", "v": 1,
                          "fields": {"VAR": {"ref": "sym_x"}}, "x": 0, "y": 0});
    let ids = [
        &head,
        &parked_declare,
        &below,
        &nested,
        &parked_loop,
        &in_value,
        &off_head,
        &under_off,
        &reporter,
    ]
    .map(id_of);
    let a = run(vec![
        main(vec![
            declare("int", "x", None),
            if_then(boolean(true), vec![declare("int", "inner", None)]),
            declare("int", "y", None),
        ]),
        func("f", "void", &[], vec![]),
        loose(
            head,
            vec![parked_declare, below, print(vec![in_value]), parked_loop],
        ),
        // A disabled head parks its stack all the same.
        loose(off_head, vec![under_off]),
        reporter,
    ]);
    let end = ["f", "x", "y"];
    for id in &ids {
        assert_eq!(visible(&a, id, None), end, "at {id}");
    }
    // Their inputs and statement lists answer the same: a declaration in a
    // parked stack declares nothing, and neither does a parked loop's counter.
    assert_eq!(visible(&a, &ids[4], Some("BODY")), end);
    assert_eq!(visible(&a, &ids[1], Some("VALUE")), end);
    assert_eq!(visible(&a, &ids[0], Some("NO_SUCH_INPUT")), end);
    assert!(
        a.symbol_infos()
            .iter()
            .all(|s| s.name != "parked" && s.name != "k"),
        "parked blocks declare nothing"
    );
}

#[test]
fn without_main_parked_blocks_see_their_modules_functions() {
    // With no `main`, with a disabled one, or with `main` in another module:
    // the functions of the parked block's own module alone.
    let mains = [
        None,
        Some(disabled(main(vec![declare("int", "x", None)]))),
        Some(main(vec![declare("int", "x", None)])),
    ];
    for (case, main_block) in mains.into_iter().enumerate() {
        let (here, there) = (marker(), marker());
        let ids = [&here, &there].map(id_of);
        let mut first = vec![
            func("f", "void", &[], vec![]),
            func("g", "int", &[("n", "int", "copy")], vec![ret(Some(num("1")))]),
            loose(here, vec![]),
        ];
        let mut second = vec![func("h", "void", &[], vec![]), loose(there, vec![])];
        match main_block {
            Some(block) if case == 2 => second.push(block),
            Some(block) => first.push(block),
            None => {}
        }
        let a = analyze(&doc_modules(vec![("main", first), ("other", second)]));
        let second_expected: &[&str] = if case == 2 { &["h", "x"] } else { &["h"] };
        assert_eq!(visible(&a, &ids[0], None), ["f", "g"], "case {case}");
        assert_eq!(visible(&a, &ids[1], None), second_expected, "case {case}");
    }
}

#[test]
fn parked_blocks_answer_down_to_the_depth_limit() {
    // A parked tree is walked to the same depth as the program: nested as
    // deeply as one that is attached, a parked block answers exactly when
    // that one does.
    let mut empty_or_not = BTreeSet::new();
    for levels in 60..68 {
        let (attached, parked) = (marker(), marker());
        let ids = [&attached, &parked].map(id_of);
        let mut in_main = forever(vec![attached]);
        let mut on_canvas = forever(vec![parked]);
        for _ in 1..levels {
            in_main = forever(vec![in_main]);
            on_canvas = forever(vec![on_canvas]);
        }
        let a = run(vec![
            main(vec![declare("int", "x", None), in_main]),
            loose(on_canvas, vec![]),
        ]);
        let found = visible(&a, &ids[0], None);
        let expected: &[&str] = if found.is_empty() { &NONE } else { &["x"] };
        assert_eq!(visible(&a, &ids[1], None), expected, "at {levels} levels");
        empty_or_not.insert(found.is_empty());
    }
    assert_eq!(empty_or_not.len(), 2, "both sides of the limit are tested");
}

#[test]
fn blocks_in_a_disabled_statement_answer_like_it() {
    // 06 §6.5: a disabled `if` and disabled loops. Everything nested in them
    // (conditions, statements, nested lists, and declarations, which declare
    // nothing) answers like the disabled statement itself.
    let in_cond = get("a");
    let first = marker();
    let hidden = declare("int", "hidden", None);
    let after_hidden = marker();
    let in_nested_loop = marker();
    let nested_loop = for_range("i", num("0"), "to", num("3"), None, vec![in_nested_loop.clone()]);
    let off_if = disabled(if_then(
        compare("lt", in_cond.clone(), num("3")),
        vec![
            first.clone(),
            hidden.clone(),
            after_hidden.clone(),
            nested_loop.clone(),
        ],
    ));
    let in_times = get("a");
    let in_body = marker();
    let off_loop = disabled(repeat(in_times.clone(), vec![in_body.clone()]));
    let in_counter_body = marker();
    let off_for = disabled(for_range(
        "j",
        num("0"),
        "to",
        num("3"),
        None,
        vec![in_counter_body.clone()],
    ));
    let in_function = marker();
    let off_in_function = disabled(forever(vec![in_function.clone()]));
    let a = run(vec![
        main(vec![
            declare("int", "a", None),
            off_if.clone(),
            declare("int", "b", None),
            off_loop.clone(),
            off_for.clone(),
        ]),
        func(
            "f",
            "void",
            &[("p", "int", "copy")],
            vec![off_in_function.clone()],
        ),
    ]);
    let like_if = ["a", "f"];
    for block in [
        &off_if,
        &in_cond,
        &first,
        &hidden,
        &after_hidden,
        &nested_loop,
        &in_nested_loop,
    ] {
        assert_eq!(visible(&a, &id_of(block), None), like_if, "{}", id_of(block));
    }
    for list in ["DO0", "ELSE"] {
        assert_eq!(visible(&a, &id_of(&off_if), Some(list)), like_if, "{list}");
    }
    assert_eq!(
        visible(&a, &id_of(&nested_loop), Some("BODY")),
        like_if,
        "a loop inside declares no counter"
    );
    let like_loop = ["a", "b", "f"];
    for block in [&off_loop, &in_times, &in_body, &off_for, &in_counter_body] {
        assert_eq!(visible(&a, &id_of(block), None), like_loop, "{}", id_of(block));
    }
    assert_eq!(visible(&a, &id_of(&off_for), Some("BODY")), like_loop);
    for block in [&off_in_function, &in_function] {
        assert_eq!(visible(&a, &id_of(block), None), ["f", "p"], "{}", id_of(block));
    }
    assert!(
        a.symbol_infos()
            .iter()
            .all(|s| !["hidden", "i", "j"].contains(&s.name.as_str()))
    );
}

#[test]
fn a_definition_the_analyser_skips_answers_with_the_modules_functions() {
    // A disabled function, and everything in it: what is visible everywhere
    // in the module (its functions), not the parameters it would have.
    let inside = marker();
    let inside_id = id_of(&inside);
    let a = run(vec![
        main(vec![declare("int", "x", None)]),
        func("f", "void", &[], vec![]),
        disabled(func(
            "off",
            "void",
            &[("p", "int", "copy")],
            vec![declare("int", "local", None), inside],
        )),
    ]);
    assert_eq!(visible(&a, "fn_off", None), ["f"]);
    assert_eq!(visible(&a, "fn_off", Some("BODY")), ["f"]);
    assert_eq!(visible(&a, &inside_id, None), ["f"]);

    // A disabled `main`, and everything in it.
    let in_main = marker();
    let in_main_id = id_of(&in_main);
    let a = run(vec![
        disabled(main(vec![declare("int", "x", None), in_main])),
        func("f", "void", &[], vec![]),
    ]);
    assert_eq!(visible(&a, "main", Some("BODY")), ["f"]);
    assert_eq!(visible(&a, &in_main_id, None), ["f"]);
}

#[test]
fn unknown_blocks_give_nothing() {
    let a = run(vec![
        main(vec![declare("int", "x", None)]),
        func("f", "void", &[], vec![]),
    ]);
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
    assert_eq!(a.scope_at(&block("main"), None), None);
    assert!(a.parked_scope(&ModuleId::new("m").unwrap()).is_empty());
    assert!(a.symbol_infos().is_empty());
    assert!(a.block_types().is_empty());
}

/// The names in an answer that may be missing.
fn names_of(found: Option<Vec<SymbolInfo>>) -> Option<Vec<String>> {
    found.map(|symbols| symbols.into_iter().map(|s| s.name).collect())
}

#[test]
fn the_query_tells_an_unknown_block_from_one_that_sees_nothing() {
    // `scope_at` is `None` only when the analysis has no answer for the
    // block: the document does not have it, or it is nested too deeply.
    let first = marker();
    let first_id = id_of(&first);
    let mut deep = marker();
    let deep_id = id_of(&deep);
    for _ in 0..70 {
        deep = forever(vec![deep]);
    }
    let a = run_main(vec![first, deep]);
    assert_eq!(names_of(a.scope_at(&block(&first_id), None)), Some(vec![]));
    assert_eq!(names_of(a.scope_at(&block("main"), Some("BODY"))), Some(vec![]));
    assert_eq!(a.scope_at(&block("no_such_block"), None), None);
    assert_eq!(a.scope_at(&block(&deep_id), None), None);
    assert_eq!(visible(&a, &deep_id, None), NONE);
}

#[test]
fn after_a_block_its_own_variable_is_visible() {
    // `scope_after` is where a block inserted after this one would be: a
    // `create` the analyser accepted adds its variable, hiding an outer one
    // with its name; a loop's counter, a disabled `create` and a parked one
    // add nothing.
    let outer = declare_as("int", "x", "s_outer", None);
    let inner = declare_as("int", "x", "s_inner", None);
    let counted = for_range("i", num("0"), "to", num("3"), None, vec![]);
    let off = disabled(declare("int", "off", None));
    let parked = declare("int", "parked", None);
    let ids = [&inner, &counted, &off, &parked].map(id_of);
    let a = run(vec![
        main(vec![outer, if_then(boolean(true), vec![inner, counted, off])]),
        loose(parked, vec![]),
    ]);
    let after = |id: &str| a.scope_after(&block(id)).expect("an answer");
    let inner_after = after(&ids[0]);
    assert_eq!(
        inner_after.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["s_inner"],
        "the new x hides the outer one"
    );
    assert_eq!(names_of(Some(after(&ids[1]))), Some(vec![String::from("x")]));
    assert_eq!(after(&ids[1]), a.symbols_in_scope(&block(&ids[1]), None));
    assert_eq!(after(&ids[2]), a.symbols_in_scope(&block(&ids[2]), None));
    assert_eq!(after(&ids[3]), a.parked_scope(&ModuleId::new("mod_0").unwrap()));
    assert_eq!(a.scope_after(&block("no_such_block")), None);
}

#[test]
fn a_disabled_value_answers_where_an_enabled_one_would() {
    // A disabled reporter in an input the analyser reaches, and the blocks
    // nested in it, answer for that input: inside a `create`'s starting
    // value, without the variable being created.
    let disabled_value = |mut input: Value| {
        input["block"]["disabled"] = json!(true);
        input
    };
    let in_print = disabled_value(arith("add", get("x"), num("1")));
    let nested = in_print["block"]["inputs"]["A"].clone();
    let in_start = disabled_value(get("x"));
    let ids = [&in_print, &nested, &in_start].map(id_of);
    let a = run_main(vec![
        declare("int", "x", None),
        print(vec![in_print]),
        declare("int", "y", Some(in_start)),
    ]);
    assert_eq!(visible(&a, &ids[0], None), ["x"]);
    assert_eq!(visible(&a, &ids[1], None), ["x"]);
    assert_eq!(
        visible(&a, &ids[2], None),
        ["x"],
        "not y in its own starting value"
    );
}

#[test]
fn every_module_has_its_parked_scope() {
    let a = analyze(&doc_modules(vec![
        (
            "main",
            vec![
                main(vec![declare("int", "x", None)]),
                func("f", "void", &[], vec![]),
            ],
        ),
        ("other", vec![func("g", "void", &[], vec![])]),
    ]));
    let names = |module: &str| -> Vec<String> {
        a.parked_scope(&ModuleId::new(module).unwrap())
            .into_iter()
            .map(|s| s.name)
            .collect()
    };
    assert_eq!(names("mod_0"), ["f", "x"]);
    assert_eq!(names("mod_1"), ["g"]);
    assert_eq!(
        names("mod_9"),
        Vec::<String>::new(),
        "a module the document does not have"
    );
}

/// The block with this ID anywhere in a JSON tree, mutably.
fn find_mut<'a>(value: &'a mut Value, id: &str) -> Option<&'a mut Value> {
    if value.get("id").and_then(Value::as_str) == Some(id) && value.get("type").is_some() {
        return Some(value);
    }
    match value {
        Value::Object(map) => map.values_mut().find_map(|v| find_mut(v, id)),
        Value::Array(items) => items.iter_mut().find_map(|v| find_mut(v, id)),
        _ => None,
    }
}

/// Every statement list of a JSON tree: its owner's ID, its name, and the
/// IDs of its statements.
fn statement_lists(value: &Value, out: &mut Vec<(String, String, Vec<String>)>) {
    match value {
        Value::Object(map) => {
            if let (Some(Value::String(id)), Some(Value::Object(lists))) =
                (map.get("id"), map.get("statements"))
            {
                for (name, list) in lists {
                    let items = list.as_array().into_iter().flatten().map(id_of).collect();
                    out.push((id.clone(), name.clone(), items));
                }
            }
            map.values().for_each(|v| statement_lists(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| statement_lists(v, out)),
        _ => {}
    }
}

#[test]
fn the_place_of_a_new_block_is_known_before_it_is_analysed() {
    // What a block dropped or pasted into a statement list will see is known
    // from the analysis before the block itself is analysed: at the start of
    // the list it is the owner's answer for that list, after a statement it
    // is that statement's `scope_after` (W4-2 re-targets a dropped preset
    // with it). Checked by inserting a statement at every position of every
    // list of every example and analysing again.
    let mut checked = 0;
    for path in project_files("examples") {
        let name = file_name(&path);
        let text = std::fs::read_to_string(&path).unwrap();
        let original: Value = serde_json::from_str(&text).unwrap();
        let before = analyze(&resolved(text.as_bytes(), &name));
        let mut lists = Vec::new();
        statement_lists(&original, &mut lists);
        for (owner, list, items) in lists {
            for at in 0..=items.len() {
                let expected = match at.checked_sub(1).and_then(|i| items.get(i)) {
                    None => before.scope_at(&block(&owner), Some(&list)),
                    Some(previous) => before.scope_after(&block(previous)),
                };
                let mut edited = original.clone();
                let mut inserted = marker();
                inserted["id"] = json!("new_block");
                find_mut(&mut edited, &owner).unwrap()["statements"][&list]
                    .as_array_mut()
                    .unwrap()
                    .insert(at, inserted);
                let after = analyze(&resolved(edited.to_string().as_bytes(), &name));
                assert_eq!(
                    after.scope_at(&block("new_block"), None),
                    expected,
                    "{name}: {owner} {list} at {at}"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 50, "only {checked} places");
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
    /// A loose stack on the first module's canvas (parked); none when empty.
    parked: Vec<Gen>,
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
        prop::collection::vec(gen_statement(), 0..3),
    )
        .prop_map(|(main, functions, parked)| GenProgram {
            main,
            functions,
            parked,
        })
}

/// How the analyser treats a block, and so what the scope query answers
/// there.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reach {
    /// It is analysed.
    Reached,
    /// It is a disabled statement (with this block ID) in a list that is
    /// analysed: not analysed itself, but the scope query answers for its
    /// position, as for an enabled block there.
    DisabledHere(String),
    /// It is inside the disabled block with this ID (a statement in a list
    /// that is analysed, or a disabled function): it answers like that block.
    Like(String),
    /// It is parked (in a loose stack, or nested in one): it answers with
    /// the parked scope.
    Parked,
}

impl Reach {
    /// The reach of the blocks inside a block with this reach.
    fn inside(&self) -> Self {
        match self {
            Self::Reached => Self::Reached,
            Self::DisabledHere(id) | Self::Like(id) => Self::Like(id.clone()),
            Self::Parked => Self::Parked,
        }
    }

    /// The reach of the block `id` marked disabled, given its reach
    /// otherwise.
    fn disabled(&self, id: &str) -> Self {
        match self {
            Self::Reached | Self::DisabledHere(_) => Self::DisabledHere(id.to_owned()),
            Self::Like(_) | Self::Parked => self.clone(),
        }
    }
}

/// A probe: a block whose reference the test points at each candidate.
#[derive(Debug)]
struct Probe {
    block: String,
    /// The symbol ID it refers to in the rendered text, unique to it.
    placeholder: String,
    call: bool,
    /// How the analyser treats it.
    reach: Reach,
    /// The variable whose starting value it is in, if any.
    initialises: Option<String>,
}

/// The ID of the statement that ends `main`'s body in a rendered program: a
/// `print 1` that declares nothing, so what is visible at it is the parked
/// scope.
const END_OF_MAIN: &str = "end_of_main";

#[derive(Default)]
struct Renderer {
    next: usize,
    variables: Vec<String>,
    functions: Vec<String>,
    probes: Vec<Probe>,
    /// The disabled statements in lists that are analysed.
    disabled_statements: Vec<String>,
    /// The disabled functions, with their module's ID.
    disabled_functions: Vec<(String, String)>,
}

impl Renderer {
    fn fresh(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}{}", self.next)
    }

    /// Records a probe with the block ID `block` (a fresh one when `None`).
    fn probe(
        &mut self,
        block: Option<String>,
        call: bool,
        reach: Reach,
        initialises: Option<String>,
    ) -> (String, String) {
        let block = block.unwrap_or_else(|| self.fresh(if call { "pc" } else { "pv" }));
        let placeholder = self.fresh("q");
        self.probes.push(Probe {
            block: block.clone(),
            placeholder: placeholder.clone(),
            call,
            reach,
            initialises,
        });
        (block, placeholder)
    }

    /// A `var.get` probe inside a block with this reach.
    fn var_probe(&mut self, reach: &Reach, initialises: Option<String>) -> Value {
        let (id, target) = self.probe(None, false, reach.inside(), initialises);
        json!({"block": {"id": id, "type": "var.get", "v": 1, "fields": {"VAR": {"ref": target}}}})
    }

    /// A statement list of a block with this reach.
    fn list(&mut self, list: &[Gen], reach: &Reach) -> Vec<Value> {
        let inner = reach.inside();
        list.iter().map(|g| self.statement(g, &inner)).collect()
    }

    /// A statement with this reach (before its own `disabled` flag).
    fn statement(&mut self, statement: &Gen, reach: &Reach) -> Value {
        let id = self.fresh("k");
        self.statement_as(id, statement, reach)
    }

    /// A statement with the block ID `id`.
    fn statement_as(&mut self, id: String, statement: &Gen, reach: &Reach) -> Value {
        match statement {
            Gen::Declare { name, probe_value } => {
                let symbol = self.fresh("s");
                self.variables.push(symbol.clone());
                let mut block = json!({"id": id, "type": "var.declare", "v": 1,
                    "fields": {"TYPE": "int", "NAME": {"sym": symbol, "name": NAMES[*name]}}});
                if *probe_value {
                    block["inputs"] = json!({"VALUE": self.var_probe(reach, Some(symbol))});
                }
                block
            }
            Gen::Probe => {
                let item = self.var_probe(reach, None);
                json!({"id": id, "type": "io.print", "v": 1, "extra": {"itemCount": 1},
                       "inputs": {"ITEM0": item}})
            }
            Gen::CallProbe => {
                let (id, target) = self.probe(Some(id), true, reach.clone(), None);
                json!({"id": id, "type": "func.call_stmt", "v": 1, "fields": {"FUNC": {"ref": target}},
                       "extra": {"argCount": 0}})
            }
            Gen::If { body, else_body } => {
                let mut statements = serde_json::Map::new();
                statements.insert(String::from("DO0"), Value::Array(self.list(body, reach)));
                if let Some(other) = else_body {
                    statements.insert(String::from("ELSE"), Value::Array(self.list(other, reach)));
                }
                json!({"id": id, "type": "control.if", "v": 1,
                       "extra": {"elseIfCount": 0, "hasElse": else_body.is_some()},
                       "inputs": {"COND0": {"expr": [{"kw": "true"}]}}, "statements": statements})
            }
            Gen::While(body) => {
                let body = self.list(body, reach);
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
                    self.var_probe(reach, None)
                } else {
                    json!({"expr": [{"num": "0"}]})
                };
                let body = self.list(body, reach);
                json!({"id": id, "type": "control.for_range", "v": 1,
                       "fields": {"VAR": {"sym": symbol, "name": NAMES[*name]}, "DIRECTION": "to"},
                       "inputs": {"FROM": from, "TO": {"expr": [{"num": "3"}]}},
                       "statements": {"BODY": body}})
            }
            Gen::Disabled(inner) => {
                let reach = reach.disabled(&id);
                if reach == Reach::DisabledHere(id.clone()) && !self.disabled_statements.contains(&id) {
                    self.disabled_statements.push(id.clone());
                }
                let mut block = self.statement_as(id, inner, &reach);
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
        let reach = if function.disabled {
            let module = if function.other_module { "mod_1" } else { "mod_0" };
            self.disabled_functions.push((id.clone(), String::from(module)));
            Reach::DisabledHere(id.clone())
        } else {
            Reach::Reached
        };
        let body = self.list(&function.body, &reach);
        json!({"id": id, "type": "func.define", "v": 1, "disabled": function.disabled,
               "fields": {"NAME": {"sym": symbol, "name": NAMES[function.name]}, "RETURNS": "void"},
               "extra": {"params": params}, "statements": {"BODY": body}})
    }

    /// The project text and the probes and candidates in it.
    fn render(program: &GenProgram) -> (String, Self) {
        let mut renderer = Self::default();
        let mut body = renderer.list(&program.main, &Reach::Reached);
        body.push(
            json!({"id": END_OF_MAIN, "type": "io.print", "v": 1, "extra": {"itemCount": 1},
                         "inputs": {"ITEM0": {"expr": [{"num": "1"}]}}}),
        );
        let mut modules = [
            vec![json!({"id": "main", "type": "program.main", "v": 1,
                                       "statements": {"BODY": body}})],
            Vec::new(),
        ];
        let mut parked = renderer.list(&program.parked, &Reach::Parked).into_iter();
        if let Some(mut head) = parked.next() {
            head["x"] = json!(600);
            head["y"] = json!(40);
            let stack: Vec<Value> = parked.collect();
            if !stack.is_empty() {
                head["stack"] = Value::Array(stack);
            }
            modules[0].push(head);
        }
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

/// The project text with the block `id` enabled.
fn enable(text: &str, id: &str) -> String {
    fn walk(value: &mut Value, id: &str) {
        match value {
            Value::Object(map) => {
                if map.get("id").and_then(Value::as_str) == Some(id) && map.contains_key("type") {
                    map.remove("disabled");
                }
                map.values_mut().for_each(|v| walk(v, id));
            }
            Value::Array(items) => items.iter_mut().for_each(|v| walk(v, id)),
            _ => {}
        }
    }
    let mut value: Value = serde_json::from_str(text).expect("generated document");
    walk(&mut value, id);
    value.to_string()
}

/// Points each probe at each candidate and compares the analyser's verdict
/// with what the scope query offered there. Blocks the analyser does not
/// lower are compared with the place they answer for.
fn check_program(program: &GenProgram) -> Result<(), TestCaseError> {
    let (text, renderer) = Renderer::render(program);
    let base = analyse_text(&text);
    // The parked scope is what is visible at the end of main's body.
    let parked = base.symbols_in_scope(&block(END_OF_MAIN), None);
    prop_assert_eq!(&parked, &base.parked_scope(&ModuleId::new("mod_0").unwrap()));
    // A disabled statement answers like the same block enabled.
    for id in &renderer.disabled_statements {
        let enabled = analyse_text(&enable(&text, id));
        prop_assert_eq!(
            base.symbols_in_scope(&block(id), None),
            enabled.symbols_in_scope(&block(id), None),
            "{} answers like an enabled block",
            id
        );
    }
    // A disabled function answers with its module's functions.
    for (id, module) in &renderer.disabled_functions {
        let functions: Vec<SymbolInfo> = base
            .symbol_infos()
            .into_iter()
            .filter(|s| matches!(s.kind, SymbolInfoKind::Function { .. }) && s.module.as_str() == module)
            .collect();
        prop_assert_eq!(base.symbols_in_scope(&block(id), None), functions, "{}", id);
    }
    for probe in &renderer.probes {
        let here = base.symbols_in_scope(&block(&probe.block), None);
        match &probe.reach {
            Reach::Reached => {}
            Reach::Parked => {
                prop_assert_eq!(&here, &parked, "{} is parked", &probe.block);
                continue;
            }
            Reach::Like(ancestor) => {
                let there = base.symbols_in_scope(&block(ancestor), None);
                prop_assert_eq!(&here, &there, "{} answers like {}", &probe.block, ancestor);
                continue;
            }
            Reach::DisabledHere(_) => {
                // The analyser does not check its reference, but the query
                // answers as for the same block enabled (a call statement
                // declares nothing, so enabling it changes nothing there).
                let enabled = analyse_text(&enable(&text, &probe.block));
                let there = enabled.symbols_in_scope(&block(&probe.block), None);
                prop_assert_eq!(&here, &there, "{} answers like an enabled block", &probe.block);
                continue;
            }
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
    // and a function in another module; probes in disabled blocks, in a
    // disabled function and in a parked stack.
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
            Gen::Disabled(Box::new(Gen::CallProbe)),
            Gen::Disabled(Box::new(Gen::While(vec![Gen::Probe, Gen::CallProbe]))),
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
            GenFunction {
                name: 0,
                params: vec![0],
                body: vec![Gen::Probe],
                disabled: true,
                other_module: false,
            },
        ],
        parked: vec![
            Gen::Probe,
            Gen::Declare {
                name: 1,
                probe_value: true,
            },
            Gen::If {
                body: vec![Gen::CallProbe],
                else_body: None,
            },
        ],
    };
    let (_, renderer) = Renderer::render(&program);
    assert_eq!(renderer.probes.len(), 14);
    let reaches: BTreeSet<String> = renderer
        .probes
        .iter()
        .map(|probe| format!("{:?}", probe.reach).chars().take(5).collect())
        .collect();
    assert_eq!(reaches.len(), 4, "every kind of reach is probed: {reaches:?}");
    assert_eq!(renderer.disabled_statements.len(), 3);
    assert_eq!(renderer.disabled_functions.len(), 1);
    check_program(&program).expect("the query matches the analyser");
}
