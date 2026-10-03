//! Names, scopes and symbols (spec §3.6, §6.5, §8.4.1).

// Test code: unwrap/expect/panic and printing to stderr are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod common;

use b2c_ir::diag::{Part, Severity};
use b2c_ir::sast::{ExprKind, ItemKind, StmtKind};
use b2c_ir::types::Type;
use b2c_lang::analyze;
use common::*;
use serde_json::json;

#[test]
fn variable_visible_after_declaration_including_nested_blocks() {
    let a = run_main(vec![
        declare("int", "x", Some(num("1"))),
        if_then(e("x > 0"), vec![repeat(e("x"), vec![print(vec![get("x")])])]),
        print(vec![e("x + 1")]),
    ]);
    assert_clean(&a);
}

#[test]
fn reference_to_missing_symbol() {
    let a = run_main(vec![print(vec![get("ghost")])]);
    let d = only(&a, "B2C-E0201");
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(
        d.primary.part,
        Part::Field {
            name: String::from("VAR")
        }
    );
    assert!(d.message.contains("doesn't exist"), "{}", d.message);

    let a = run_main(vec![print(vec![e("1 + ghost")])]);
    let d = only(&a, "B2C-E0201");
    assert_eq!(
        d.primary.part,
        Part::Tokens {
            input: String::from("ITEM0"),
            start: 2,
            end: 3
        }
    );
}

#[test]
fn used_before_declaration() {
    let a = run_main(vec![print(vec![get("x")]), declare("int", "x", Some(num("1")))]);
    let d = only(&a, "B2C-E0202");
    assert!(
        d.message.contains("`x` is used before it is created"),
        "{}",
        d.message
    );
    assert_eq!(d.related.len(), 1);
    assert_eq!(
        d.related[0].location.part,
        Part::Field {
            name: String::from("NAME")
        }
    );

    // Also from a nested block before the declaration.
    let a = run_main(vec![
        if_then(e("true"), vec![set("x", num("2"))]),
        declare("int", "x", Some(num("1"))),
    ]);
    only(&a, "B2C-E0202");
}

#[test]
fn out_of_scope() {
    // Declared inside an if, used after it.
    let a = run_main(vec![
        if_then(e("true"), vec![declare("int", "x", Some(num("1")))]),
        print(vec![get("x")]),
    ]);
    let d = only(&a, "B2C-E0203");
    assert!(d.message.contains("can't be used here"), "{}", d.message);

    // Declared in one branch, used in another.
    let a = run_main(vec![if_else(
        vec![
            (e("true"), vec![declare("int", "x", Some(num("1")))]),
            (e("false"), vec![print(vec![get("x")])]),
        ],
        None,
    )]);
    only(&a, "B2C-E0203");

    // Main's variable used in a function.
    let a = run(vec![
        main(vec![declare("int", "x", Some(num("1")))]),
        func("f", "void", &[], vec![print(vec![get("x")])]),
    ]);
    only(&a, "B2C-E0203");
}

#[test]
fn parameters_and_counters_out_of_scope() {
    let a = run(vec![
        main(vec![print(vec![get("p")])]),
        func("f", "void", &[("p", "int", "copy")], vec![print(vec![get("p")])]),
    ]);
    let d = only(&a, "B2C-E0203");
    assert!(d.message.contains("input of the function `f`"), "{}", d.message);

    let a = run_main(vec![
        for_range("i", num("0"), "to", num("3"), None, vec![]),
        print(vec![get("i")]),
    ]);
    let d = only(&a, "B2C-E0203");
    assert!(d.message.contains("counter of a 'for' loop"), "{}", d.message);

    // The counter is not available in the loop's own header.
    let a = run_main(vec![for_range("i", num("0"), "to", e("i + 1"), None, vec![])]);
    let d = only(&a, "B2C-E0203");
    assert!(
        d.message.contains("loop's own start, end or step"),
        "{}",
        d.message
    );
}

#[test]
fn disabled_and_detached_declarations() {
    let mut off = declare("int", "x", Some(num("1")));
    off["disabled"] = json!(true);
    let a = run_main(vec![off, print(vec![get("x")])]);
    let d = only(&a, "B2C-E0204");
    assert!(d.message.contains("disabled block"), "{}", d.message);

    // Inside a disabled container.
    let mut container = if_then(e("true"), vec![declare("int", "y", Some(num("1")))]);
    container["disabled"] = json!(true);
    let a = run_main(vec![container, print(vec![get("y")])]);
    only(&a, "B2C-E0204");

    // A loose top-level declaration.
    let a = run(vec![
        main(vec![print(vec![get("z")])]),
        declare("int", "z", Some(num("1"))),
    ]);
    let d = only(&a, "B2C-E0204");
    assert!(d.message.contains("isn't attached"), "{}", d.message);

    // A call to a disabled function.
    let mut f = func("f", "void", &[], vec![]);
    f["disabled"] = json!(true);
    let a = run(vec![main(vec![call_stmt("f", vec![])]), f]);
    only(&a, "B2C-E0204");
}

#[test]
fn functions_are_order_independent_and_recursive() {
    let a = run(vec![
        func(
            "a_first",
            "int",
            &[("n", "int", "copy")],
            vec![
                if_then(e("n <= 0"), vec![ret(Some(num("0")))]),
                ret(Some(e("b_second(n - 1) + a_first(n - 1)"))),
            ],
        ),
        func(
            "b_second",
            "int",
            &[("m", "int", "copy")],
            vec![ret(Some(e("a_first(m)")))],
        ),
        main(vec![print(vec![e("a_first(3)")])]),
    ]);
    assert_clean(&a);
}

#[test]
fn function_in_another_module() {
    let d = doc_modules(vec![
        ("main", vec![main(vec![call_stmt("helper", vec![])])]),
        ("util", vec![func("helper", "void", &[], vec![])]),
    ]);
    let a = analyze(&d);
    let d = only(&a, "B2C-E0206");
    assert!(d.message.contains("`util`"), "{}", d.message);
}

#[test]
fn wrong_kind_of_symbol() {
    // A function used as a value.
    let a = run(vec![
        main(vec![print(vec![e("f + 1")])]),
        func("f", "int", &[], vec![ret(Some(num("1")))]),
    ]);
    let d = only(&a, "B2C-E0207");
    assert!(d.message.contains("is a function, not a value"), "{}", d.message);

    // A variable called.
    let a = run_main(vec![declare("int", "x", Some(num("1"))), print(vec![e("x(2)")])]);
    let d = only(&a, "B2C-E0207");
    assert!(d.message.contains("can't be called"), "{}", d.message);
    let a = run_main(vec![declare("int", "x", Some(num("1"))), call_stmt("x", vec![])]);
    only(&a, "B2C-E0207");

    // A function changed.
    let a = run(vec![
        main(vec![set("f", num("1"))]),
        func("f", "void", &[], vec![]),
    ]);
    let d = only(&a, "B2C-E0207");
    assert!(
        d.message.contains("only variables can be changed"),
        "{}",
        d.message
    );
}

#[test]
fn duplicate_names_in_one_scope() {
    let a = run_main(vec![
        declare("int", "x", Some(num("1"))),
        json!({"id": "dup", "type": "var.declare", "v": 1,
               "fields": {"TYPE": "int", "NAME": {"sym": "sym_x2", "name": "x"}},
               "inputs": {"VALUE": {"expr": [{"num": "2"}]}}}),
    ]);
    let d = only(&a, "B2C-E0210");
    assert_eq!(d.primary.block.as_ref().map(b2c_ir::BlockId::as_str), Some("dup"));
    assert_eq!(d.related.len(), 1);

    // A function body shares the scope of the parameters (C++ rejects this).
    let a = run(vec![
        main(vec![]),
        func(
            "f",
            "void",
            &[("n", "int", "copy")],
            vec![json!({"id": "dup", "type": "var.declare", "v": 1,
            "fields": {"TYPE": "int", "NAME": {"sym": "sym_n2", "name": "n"}}})],
        ),
    ]);
    only(&a, "B2C-E0210");

    // Likewise a for body and its counter.
    let a = run_main(vec![for_range(
        "i",
        num("0"),
        "to",
        num("3"),
        None,
        vec![json!({"id": "dup",
        "type": "var.declare", "v": 1, "fields": {"TYPE": "int", "NAME": {"sym": "sym_i2", "name": "i"}}})],
    )]);
    only(&a, "B2C-E0210");

    // Two parameters with the same name.
    let a = run(vec![
        main(vec![]),
        json!({"id": "f", "type": "func.define", "v": 1,
               "fields": {"NAME": {"sym": "sym_f", "name": "f"}, "RETURNS": "void"},
               "extra": {"params": [{"sym": "p1", "name": "a", "type": "int", "mode": "copy"},
                                    {"sym": "p2", "name": "a", "type": "int", "mode": "copy"}]}}),
    ]);
    let d = only(&a, "B2C-E0210");
    assert_eq!(
        d.primary.part,
        Part::Field {
            name: String::from("params[1]")
        }
    );
}

#[test]
fn shadowing_is_a_warning() {
    let a = run_main(vec![
        declare("int", "x", Some(num("1"))),
        if_then(e("true"), vec![declare("int", "x2", Some(num("2")))]),
        if_then(
            e("true"),
            vec![json!({"id": "inner", "type": "var.declare", "v": 1,
            "fields": {"TYPE": "int", "NAME": {"sym": "sym_inner", "name": "x"}},
            "inputs": {"VALUE": {"expr": [{"num": "2"}]}}})],
        ),
        for_range("x3", num("0"), "to", num("1"), None, vec![]),
    ]);
    let d = only(&a, "B2C-W0501");
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(
        d.primary.block.as_ref().map(b2c_ir::BlockId::as_str),
        Some("inner")
    );

    // A loop counter hiding an outer variable.
    let a = run_main(vec![
        declare("int", "i", Some(num("1"))),
        json!({"id": "loop", "type": "control.for_range", "v": 1,
               "fields": {"VAR": {"sym": "sym_i2", "name": "i"}},
               "inputs": {"FROM": {"expr": [{"num": "0"}]}, "TO": {"expr": [{"num": "3"}]}}}),
    ]);
    only(&a, "B2C-W0501");

    // A variable with the name of a function.
    let a = run(vec![
        main(vec![json!({"id": "var_f", "type": "var.declare", "v": 1,
                         "fields": {"TYPE": "int", "NAME": {"sym": "sym_var_f", "name": "f"}}})]),
        func("f", "void", &[], vec![]),
    ]);
    let d = only(&a, "B2C-W0501");
    assert!(d.message.contains("same name as a function"), "{}", d.message);
}

#[test]
fn hidden_outer_variable_is_an_error() {
    // Inner `x` hides the outer one, but the block refers to the outer by ID.
    let a = run_main(vec![
        declare("int", "x", Some(num("1"))),
        if_then(
            e("true"),
            vec![
                json!({"id": "inner", "type": "var.declare", "v": 1,
                   "fields": {"TYPE": "int", "NAME": {"sym": "sym_inner", "name": "x"}},
                   "inputs": {"VALUE": {"expr": [{"num": "2"}]}}}),
                print(vec![get("x")]),
            ],
        ),
    ]);
    assert_codes(&a, &["B2C-W0501", "B2C-E0205"]);

    // `create int x = x` where the value means the outer x: C++ sees the new one.
    let a = run_main(vec![
        declare("int", "x", Some(num("1"))),
        if_then(
            e("true"),
            vec![json!({"id": "inner", "type": "var.declare", "v": 1,
            "fields": {"TYPE": "int", "NAME": {"sym": "sym_inner", "name": "x"}},
            "inputs": {"VALUE": {"expr": [{"ref": "sym_x"}, {"op": "+"}, {"num": "1"}]}}})],
        ),
    ]);
    assert_codes(&a, &["B2C-W0501", "B2C-E0205"]);

    // A loop whose end refers to an outer variable with the counter's name.
    let a = run_main(vec![
        declare("int", "n", Some(num("3"))),
        json!({"id": "loop", "type": "control.for_range", "v": 1,
               "fields": {"VAR": {"sym": "sym_n2", "name": "n"}},
               "inputs": {"FROM": {"expr": [{"num": "0"}]}, "TO": {"expr": [{"ref": "sym_n"}]}}}),
    ]);
    assert_codes(&a, &["B2C-E0205", "B2C-W0501"]);

    // A function hidden by a variable.
    let a = run(vec![
        main(vec![
            json!({"id": "var_f", "type": "var.declare", "v": 1,
                   "fields": {"TYPE": "int", "NAME": {"sym": "sym_var_f", "name": "f"}},
                   "inputs": {"VALUE": {"expr": [{"num": "1"}]}}}),
            call_stmt("f", vec![]),
        ]),
        func("f", "void", &[], vec![]),
    ]);
    assert_codes(&a, &["B2C-W0501", "B2C-E0205"]);
}

#[test]
fn own_initialiser() {
    // Typed: allowed (C++ accepts it) but warned by the flow check.
    let a = run_main(vec![declare("int", "x", Some(e("x + 1")))]);
    assert_codes(&a, &["B2C-W0503"]);
    // auto: an error, the type is unknown.
    let a = run_main(vec![declare("auto", "x", Some(e("x + 1")))]);
    assert_codes(&a, &["B2C-E0202"]);
}

#[test]
fn duplicate_functions() {
    let a = run(vec![
        main(vec![]),
        func("f", "void", &[], vec![]),
        json!({"id": "fn_g", "type": "func.define", "v": 1,
               "fields": {"NAME": {"sym": "sym_f2", "name": "f"}, "RETURNS": "int"},
               "statements": {"BODY": [{"id": "r", "type": "func.return", "v": 1,
                   "inputs": {"VALUE": {"expr": [{"num": "1"}]}}}]}}),
    ]);
    let d = only(&a, "B2C-E0211");
    assert_eq!(
        d.primary.block.as_ref().map(b2c_ir::BlockId::as_str),
        Some("fn_g")
    );
    assert!(d.message.contains("overloads"), "{}", d.message);
    assert!(d.message.contains("in this module"), "{}", d.message);
    assert_eq!(d.related.len(), 1, "points at the other function");
}

#[test]
fn duplicate_functions_in_different_modules() {
    // Every function is generated with external linkage, so the same name in
    // two modules would not link.
    let d = doc_modules(vec![
        ("main", vec![main(vec![]), func("helper", "void", &[], vec![])]),
        (
            "util",
            vec![json!({"id": "fn_h2", "type": "func.define", "v": 1,
                        "fields": {"NAME": {"sym": "sym_helper2", "name": "helper"}, "RETURNS": "void"}})],
        ),
    ]);
    let a = analyze(&d);
    let e = only(&a, "B2C-E0211");
    assert_eq!(
        e.primary.block.as_ref().map(b2c_ir::BlockId::as_str),
        Some("fn_h2")
    );
    assert!(e.message.contains("in module `main`"), "{}", e.message);
    // Different names in different modules are fine.
    let d = doc_modules(vec![
        ("main", vec![main(vec![]), func("helper", "void", &[], vec![])]),
        ("util", vec![func("other", "void", &[], vec![])]),
    ]);
    assert_clean(&analyze(&d));
}

#[test]
fn duplicate_symbol_ids() {
    let a = run_main(vec![
        declare("int", "x", Some(num("1"))),
        json!({"id": "dup", "type": "var.declare", "v": 1,
               "fields": {"TYPE": "int", "NAME": {"sym": "sym_x", "name": "other"}}}),
        print(vec![get("x")]),
    ]);
    assert_codes(&a, &["B2C-E0212"]);

    // A parameter that reuses another parameter's ID is reported once; calls
    // still see the right number of inputs.
    let a = run(vec![
        main(vec![
            call_stmt("show", vec![text("a")]),
            call_stmt("twice", vec![text("b")]),
        ]),
        func(
            "show",
            "void",
            &[("t", "std::string", "read_only")],
            vec![print(vec![get("t")])],
        ),
        func(
            "twice",
            "void",
            &[("t", "std::string", "copy")],
            vec![print(vec![get("t")])],
        ),
    ]);
    assert_codes(&a, &["B2C-E0212"]);
    let twice = a
        .program
        .symbols
        .symbols
        .values()
        .find(|s| s.name.as_str() == "twice")
        .expect("twice");
    assert_eq!(
        twice.kind,
        b2c_ir::sast::SymbolKind::Function { params: Vec::new() },
        "the duplicate is left out of the program"
    );
}

#[test]
fn invalid_names() {
    for (name, needle) in [
        ("int", "C++ keyword"),
        ("2fast", "start with a letter"),
        ("my var", "only contain letters"),
        ("a__b", "two underscores"),
        ("b2c_tmp", "reserved by Blocks2Cpp"),
        ("main", "reserved by Blocks2Cpp"),
        ("std", "reserved by Blocks2Cpp"),
        ("NULL", "standard library"),
        ("", "A name is needed"),
    ] {
        let a = run_main(vec![json!({"id": "d", "type": "var.declare", "v": 1,
            "fields": {"TYPE": "int", "NAME": {"sym": "sym_v", "name": name}}})]);
        let d = only(&a, "B2C-E0220");
        assert!(d.message.contains(needle), "{name}: {}", d.message);
        assert_eq!(
            d.primary.part,
            Part::Field {
                name: String::from("NAME")
            }
        );
        // The symbol still exists, with a placeholder name.
        let sym = b2c_ir::ids::SymbolId::new("sym_v").expect("id");
        assert!(
            a.program
                .symbols
                .get(&sym)
                .expect("symbol")
                .name
                .as_str()
                .starts_with("b2c_")
        );
    }
    // A long name is shown truncated and without control characters.
    let long = format!("x\u{202E}{}", "y".repeat(100));
    let a = run_main(vec![json!({"id": "d", "type": "var.declare", "v": 1,
        "fields": {"TYPE": "int", "NAME": {"sym": "sym_v", "name": long}}})]);
    let d = only(&a, "B2C-E0220");
    assert!(d.message.contains("<U+202E>") && d.message.contains('…') && !d.message.contains('\u{202E}'));
}

#[test]
fn function_names_need_namespace_rules() {
    // `abs` is fine for a local variable but not for a function.
    let a = run(vec![main(vec![declare("int", "abs", Some(num("1")))])]);
    assert_clean(&a);
    let a = run(vec![main(vec![]), func("abs", "void", &[], vec![])]);
    let d = only(&a, "B2C-E0220");
    assert!(d.message.contains("C library"), "{}", d.message);
    // Parameters are local.
    let a = run(vec![
        main(vec![]),
        func("f", "void", &[("time", "int", "copy")], vec![]),
    ]);
    assert_clean(&a);
    // A function called like a standard function on text would make calls
    // with text ambiguous (argument-dependent lookup finds `std::stoi`).
    let a = run(vec![
        main(vec![]),
        func(
            "stoi",
            "int",
            &[("t", "std::string", "read_only")],
            vec![ret(Some(num("1")))],
        ),
    ]);
    let d = only(&a, "B2C-E0220");
    assert!(d.message.contains("standard library function"), "{}", d.message);
    // The same name is fine for a variable.
    assert_clean(&run(vec![main(vec![declare("int", "stoi", Some(num("1")))])]));
}

#[test]
fn symbol_table_contents() {
    let a = run(vec![
        main(vec![
            declare("int", "x", Some(num("1"))),
            for_range("i", num("0"), "to", num("2"), None, vec![]),
        ]),
        func(
            "f",
            "double",
            &[("p", "std::string", "read_only")],
            vec![ret(Some(num("1.0")))],
        ),
    ]);
    assert_clean(&a);
    let names: Vec<(&str, &str)> = a
        .program
        .symbols
        .symbols
        .iter()
        .map(|(id, s)| (id.as_str(), s.name.as_str()))
        .collect();
    assert_eq!(
        names,
        [("sym_f", "f"), ("sym_i", "i"), ("sym_p", "p"), ("sym_x", "x")]
    );
    let f = a
        .program
        .symbols
        .get(&b2c_ir::ids::SymbolId::new("sym_f").expect("id"))
        .expect("f");
    assert_eq!(f.ty, Type::Double);
    let p = a
        .program
        .symbols
        .get(&b2c_ir::ids::SymbolId::new("sym_p").expect("id"))
        .expect("p");
    assert_eq!(p.ty, Type::String);
}

#[test]
fn references_resolve_to_var_nodes() {
    let a = run_main(vec![
        declare("int", "x", Some(num("1"))),
        declare("int", "y", Some(e("x"))),
    ]);
    assert_clean(&a);
    let ItemKind::Main(main) = &a.program.modules[0].items[0].kind else {
        panic!()
    };
    let StmtKind::VarDecl(y) = &main.body.stmts[1].kind else {
        panic!()
    };
    let init = y.init.as_ref().expect("init");
    assert!(matches!(&init.kind, ExprKind::Var(s) if s.as_str() == "sym_x"));
    assert_eq!(init.ty, Type::Int);
}
