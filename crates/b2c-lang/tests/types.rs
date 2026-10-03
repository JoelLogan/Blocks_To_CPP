//! Type checking (spec §3.5.3, §6.6): errors only when certain, warnings for
//! legal but suspicious code, no cascades from `Type::Error`.

// Test code: unwrap/expect/panic and printing to stderr are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod common;

use b2c_ir::diag::{Part, Severity};
use b2c_ir::sast::{ExprKind, ItemKind, StmtKind};
use b2c_ir::types::Type;
use common::*;
use serde_json::json;

/// Variables of every type, for building expressions.
fn vars() -> Vec<serde_json::Value> {
    vec![
        declare("int", "i", Some(num("1"))),
        declare("double", "d", Some(num("1.5"))),
        declare("char", "c", Some(chr("a"))),
        declare("bool", "b", Some(boolean(true))),
        declare("std::string", "s", Some(text("x"))),
    ]
}

/// Analyses `create <ty> r = <src>` after the standard variables.
fn check(ty: &str, src: &str) -> b2c_lang::Analysis {
    let mut body = vars();
    body.push(declare(ty, "r", Some(e(src))));
    run_main(body)
}

#[track_caller]
fn expect(ty: &str, src: &str, codes: &[&str]) {
    let a = check(ty, src);
    assert_codes(&a, codes);
}

/// The type the analyser gives `src` (via an `auto` declaration).
fn type_of(src: &str) -> Type {
    let a = check("auto", src);
    let ItemKind::Main(main) = &a.program.modules[0].items[0].kind else {
        panic!()
    };
    let StmtKind::VarDecl(decl) = &main.body.stmts[5].kind else {
        panic!()
    };
    decl.init.as_ref().expect("init").ty.clone()
}

#[test]
fn arithmetic_types() {
    assert_eq!(type_of("i + i"), Type::Int);
    assert_eq!(type_of("i * d"), Type::Double);
    assert_eq!(type_of("c + 1"), Type::Int, "char promotes to int");
    assert_eq!(type_of("c + c"), Type::Int);
    assert_eq!(type_of("i % 3"), Type::Int);
    assert_eq!(type_of("-c"), Type::Int);
    assert_eq!(type_of("-d"), Type::Double);
    assert_eq!(type_of("i < d"), Type::Bool);
    assert_eq!(type_of("s == s"), Type::Bool);
    assert_eq!(type_of("b ? i : d"), Type::Double);
    assert_eq!(type_of("b ? c : c"), Type::Char);
    assert_eq!(type_of("b ? s : s"), Type::String);
    assert_eq!(type_of("!b"), Type::Bool);
    assert_eq!(type_of("b && i < 2 || false"), Type::Bool);
}

#[test]
fn text_with_plus_suggests_join() {
    let a = check("std::string", "s + s");
    let d = only(&a, "B2C-E0303");
    assert!(d.message.contains("'join' block"), "{}", d.message);
    expect("std::string", "s + 1", &["B2C-E0303"]);
    expect("std::string", "\"a\" + \"b\"", &["B2C-E0303"]);
    // Blocks too.
    let mut body = vars();
    body.push(declare(
        "std::string",
        "r",
        Some(arith("add", get("s"), text("!"))),
    ));
    assert_codes(&run_main(body), &["B2C-E0303"]);
}

#[test]
fn bad_operands() {
    expect("int", "s - 1", &["B2C-E0302"]);
    expect("int", "s * 2", &["B2C-E0302"]);
    expect("int", "s % 2", &["B2C-E0302"]);
    expect("bool", "s < 1", &["B2C-E0302"]);
    expect("bool", "s == c", &["B2C-E0302"]);
    expect("bool", "s && b", &["B2C-E0302"]);
    expect("bool", "!s", &["B2C-E0302"]);
    expect("int", "-s", &["B2C-E0302"]);
}

#[test]
fn comparisons() {
    expect("bool", "s < s", &[]);
    expect("bool", "s != \"x\"", &[]);
    expect("bool", "c == 'a'", &[]);
    expect("bool", "i < d", &[]);
    expect("bool", "b == b", &[]);
    expect("bool", "b != false", &[]);
    expect("bool", "b == 1", &["B2C-W0519"]);
    expect("bool", "b < b", &["B2C-W0519"]);
    // Chained comparisons compare a bool with a number.
    expect("bool", "1 < i < 3", &["B2C-W0519"]);
}

#[test]
fn equality_on_doubles() {
    let a = check("bool", "d == 0.3");
    let d = only(&a, "B2C-W0511");
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(
        d.primary.part,
        Part::Tokens {
            input: String::from("VALUE"),
            start: 0,
            end: 3
        }
    );
    expect("bool", "d != i", &["B2C-W0511"]);
    expect("bool", "d < 0.3", &[]);
    expect("bool", "i == 3", &[]);
}

#[test]
fn modulo_needs_whole_numbers() {
    expect("int", "d % 2", &["B2C-E0304"]);
    expect("int", "i % 2.0", &["B2C-E0304"]);
    expect("int", "c % 2", &[]);
}

#[test]
fn logical_operators_with_numbers_warn() {
    expect("bool", "i && b", &["B2C-W0519"]);
    expect("bool", "i or d", &["B2C-W0519", "B2C-W0519"]);
    expect("bool", "not i", &["B2C-W0519"]);
    expect("int", "b + 1", &["B2C-W0519"]);
    expect("int", "-b", &["B2C-W0519"]);
}

#[test]
fn conditional_branches() {
    expect("int", "b ? i : c", &[]);
    expect("int", "b ? i : b", &["B2C-W0519"]);
    let a = check("int", "b ? i : s");
    let d = only(&a, "B2C-E0311");
    assert!(
        d.message.contains("a whole number") && d.message.contains("text"),
        "{}",
        d.message
    );
    expect("int", "s ? 1 : 2", &["B2C-E0301"]);
    expect("int", "i ? 1 : 2", &["B2C-W0519"]);
}

#[test]
fn initialisation_and_assignment_conversions() {
    expect("double", "i", &[]);
    expect("int", "c", &[]);
    expect("double", "c", &[]);
    expect("int", "d", &["B2C-W0518"]);
    expect("char", "i", &["B2C-W0518"]);
    expect("char", "d", &["B2C-W0518"]);
    expect("bool", "i", &["B2C-W0519"]);
    expect("int", "b", &["B2C-W0519"]);
    expect("int", "s", &["B2C-E0301"]);
    expect("std::string", "i", &["B2C-E0301"]);
    expect("std::string", "c", &["B2C-E0301"]);
    expect("std::string", "b", &["B2C-E0301"]);
    let a = check("std::string", "i");
    assert!(only(&a, "B2C-E0301").message.contains("join"));
    let a = check("int", "d");
    let d = only(&a, "B2C-W0518");
    assert!(d.message.contains("'convert' block"), "{}", d.message);

    let mut body = vars();
    body.push(set("i", e("d")));
    body.push(set("s", e("i")));
    assert_codes(&run_main(body), &["B2C-W0518", "B2C-E0301"]);

    // A text variable can be *set* to one character (`s = 'a';` is valid
    // C++), although it can't start with one (`std::string s = 'a';` isn't).
    let mut body = vars();
    body.push(set("s", e("c")));
    body.push(set("s", chr("z")));
    body.push(set("s", e("b ? c : 'y'")));
    let a = run_main(body);
    assert_clean(&a);
    if gxx::available() {
        let program = b2c_codegen::generate(&a.program, &b2c_codegen::CodegenOptions::default());
        gxx::syntax_check(&program.files[0].contents, false).unwrap();
    }
    let mut body = vars();
    body.push(set("s", e("b")));
    body.push(set("c", e("s")));
    assert_codes(&run_main(body), &["B2C-E0301", "B2C-E0301"]);
}

#[test]
fn integer_division_into_decimal() {
    let a = check("double", "i / 2");
    let d = only(&a, "B2C-W0510");
    assert!(d.message.contains("7.0 / 2"), "{}", d.message);
    expect("double", "i / 2.0", &[]);
    expect("double", "i / 2 * d", &["B2C-W0510"]);
    expect("int", "i / 2", &[]);
    let mut body = vars();
    body.push(declare("double", "r", Some(convert("double", e("i / 2")))));
    body.push(set("d", e("7 / 2")));
    body.push(update("d", "add", e("i / 3")));
    assert_codes(&run_main(body), &["B2C-W0510", "B2C-W0510", "B2C-W0510"]);
}

#[test]
fn errors_do_not_cascade() {
    // One unknown name: one error, even though it is used in arithmetic,
    // compared, negated and assigned.
    expect(
        "int",
        "-(ghost + 1) * 2 < 3 ? ghost : 1",
        &["B2C-E0201", "B2C-E0201"],
    );
    expect("std::string", "ghost + 1", &["B2C-E0201"]);
    let mut body = vars();
    body.push(set("s", e("ghost")));
    body.push(print(vec![join(vec![e("ghost"), text("x")])]));
    assert_codes(&run_main(body), &["B2C-E0201", "B2C-E0201"]);
    let a = check("int", "-(ghost)");
    let ItemKind::Main(main) = &a.program.modules[0].items[0].kind else {
        panic!()
    };
    let StmtKind::VarDecl(decl) = &main.body.stmts[5].kind else {
        panic!()
    };
    assert_eq!(decl.init.as_ref().expect("init").ty, Type::Error);
}

#[test]
fn auto_takes_the_value_type() {
    assert_eq!(type_of("s"), Type::String);
    let a = run_main(vec![declare("auto", "x", None)]);
    let d = only(&a, "B2C-E0309");
    assert!(d.message.contains("'auto'"), "{}", d.message);
    // An auto variable from an error has the error type: no further reports.
    let a = run_main(vec![
        declare("auto", "x", Some(e("ghost"))),
        set("x", text("a")),
        print(vec![e("x + 1")]),
    ]);
    assert_codes(&a, &["B2C-E0201"]);
}

#[test]
fn const_counters_and_read_only_cannot_change() {
    let a = run_main(vec![
        declare_const("int", "k", num("1")),
        set("k", num("2")),
        change("k", num("1")),
    ]);
    assert_codes(&a, &["B2C-E0308", "B2C-E0308"]);
    assert!(a.diagnostics[0].message.contains("constant"));

    let a = run_main(vec![for_range(
        "i",
        num("0"),
        "to",
        num("3"),
        None,
        vec![set("i", num("5"))],
    )]);
    let d = only(&a, "B2C-E0308");
    assert!(d.message.contains("counter"), "{}", d.message);

    let a = run(vec![
        main(vec![]),
        func(
            "f",
            "void",
            &[
                ("p", "int", "read_only"),
                ("q", "int", "copy"),
                ("r", "int", "editable"),
            ],
            vec![set("p", num("1")), set("q", num("1")), set("r", num("1"))],
        ),
    ]);
    let d = only(&a, "B2C-E0308");
    assert!(d.message.contains("read-only"), "{}", d.message);

    let a = run_main(vec![declare_const("int", "k", num("1")), ask("k", None)]);
    only(&a, "B2C-E0308");
}

#[test]
fn change_and_update_rules() {
    let mut body = vars();
    body.extend([
        change("s", num("1")),
        change("b", num("1")),
        change("i", text("x")),
        change("i", num("1.5")),
        update("d", "mod", num("2")),
        update("i", "mod", num("2.0")),
        change("d", num("1")),
        change("c", num("1")),
        update("i", "mul", boolean(true)),
    ]);
    assert_codes(
        &run_main(body),
        &[
            "B2C-E0302",
            "B2C-W0519",
            "B2C-E0301",
            "B2C-W0518",
            "B2C-E0304",
            "B2C-E0304",
            "B2C-W0519",
        ],
    );
}

#[test]
fn calls_check_arguments() {
    let program = |args: Vec<serde_json::Value>| {
        let mut body = vars();
        body.push(call_stmt("f", args));
        run(vec![
            main(body),
            func(
                "f",
                "void",
                &[("n", "int", "copy"), ("t", "std::string", "read_only")],
                vec![],
            ),
        ])
    };
    assert_clean(&program(vec![get("i"), get("s")]));
    assert_clean(&program(vec![get("c"), text("lit")]));
    assert_codes(&program(vec![get("d"), get("s")]), &["B2C-W0518"]);
    assert_codes(&program(vec![get("s"), get("s")]), &["B2C-E0301"]);
    let a = program(vec![get("i")]);
    let d = only(&a, "B2C-E0306");
    assert!(
        d.message.contains("needs 2 values, but 1 is given"),
        "{}",
        d.message
    );
    let a = program(vec![get("i"), get("s"), get("s")]);
    assert!(only(&a, "B2C-E0306").message.contains("3 are given"));
    let a = program(vec![]);
    assert!(only(&a, "B2C-E0306").message.contains("0 are given"));
}

#[test]
fn editable_arguments_need_matching_variables() {
    let program = |arg: serde_json::Value| {
        let mut body = vars();
        body.push(declare_const("int", "k", num("1")));
        body.push(call_stmt("bump", vec![arg]));
        run(vec![
            main(body),
            func(
                "bump",
                "void",
                &[("n", "int", "editable")],
                vec![change("n", num("1"))],
            ),
        ])
    };
    assert_clean(&program(get("i")));
    assert_clean(&program(e("i")));
    let a = program(num("3"));
    assert!(
        only(&a, "B2C-E0307")
            .message
            .contains("not a value or calculation")
    );
    let a = program(e("i + 1"));
    only(&a, "B2C-E0307");
    let a = program(get("d"));
    assert!(only(&a, "B2C-E0307").message.contains("holds a decimal number"));
    let a = program(get("c"));
    only(&a, "B2C-E0307");
    let a = program(get("k"));
    assert!(only(&a, "B2C-E0307").message.contains("can't be changed"));
}

#[test]
fn void_functions_have_no_value() {
    let a = run(vec![
        main(vec![
            declare("int", "x", Some(call("f", vec![]))),
            print(vec![e("f()")]),
            call_stmt("f", vec![]),
        ]),
        func("f", "void", &[], vec![]),
    ]);
    assert_codes(&a, &["B2C-E0305", "B2C-E0305"]);
    assert!(a.diagnostics[0].message.contains("statement version"));
    // Non-void results may be ignored.
    let a = run(vec![
        main(vec![call_stmt("g", vec![])]),
        func("g", "int", &[], vec![ret(Some(num("1")))]),
    ]);
    assert_clean(&a);
}

#[test]
fn returns() {
    // Return value conversions.
    let a = run(vec![
        main(vec![]),
        func("f", "int", &[], vec![ret(Some(text("x")))]),
    ]);
    let d = only(&a, "B2C-E0301");
    assert!(d.message.starts_with("The result of `f`"), "{}", d.message);
    let a = run(vec![
        main(vec![]),
        func("f", "int", &[], vec![ret(Some(num("1.5")))]),
    ]);
    only(&a, "B2C-W0518");
    // Missing or extra values.
    let a = run(vec![main(vec![]), func("f", "int", &[], vec![ret(None)])]);
    assert_codes(&a, &["B2C-E0403"]);
    let a = run(vec![
        main(vec![]),
        func("f", "void", &[], vec![ret(Some(num("1")))]),
    ]);
    assert_codes(&a, &["B2C-E0404"]);
    let a = run_main(vec![ret(None)]);
    let d = only(&a, "B2C-E0403");
    assert!(d.message.contains("exit code"), "{}", d.message);
    let a = run_main(vec![ret(Some(text("bye")))]);
    only(&a, "B2C-E0301");
}

#[test]
fn integer_slots() {
    let a = run_main(vec![
        declare("double", "d", Some(num("2.5"))),
        repeat(e("d"), vec![]),
        repeat(text("3"), vec![]),
        for_range("i", e("d"), "to", num("3"), Some(e("d")), vec![]),
        declare("int", "r", Some(random_int(e("d"), text("6")))),
        exit(e("d")),
    ]);
    assert_codes(
        &a,
        &[
            "B2C-W0518",
            "B2C-E0301",
            "B2C-W0518",
            "B2C-W0518",
            "B2C-W0518",
            "B2C-E0301",
            "B2C-W0518",
        ],
    );
    assert!(
        a.diagnostics[1]
            .message
            .starts_with("The number of times to repeat")
    );
    // A count or bound should simply be a whole number; nothing is dropped
    // from a variable's value.
    assert_eq!(
        a.diagnostics[0].message,
        "The number of times to repeat should be a whole number, but this is a decimal number. Use the \
         'convert' block to make it a whole number."
    );
    assert!(
        a.diagnostics[2]
            .message
            .starts_with("The start of the loop should be")
    );
    assert!(a.diagnostics[6].message.starts_with("The exit code should be"));
    let a = run_main(vec![declare("int", "n", Some(num("2.5")))]);
    assert!(
        only(&a, "B2C-W0518")
            .message
            .contains("the part after the decimal point will be dropped"),
        "{}",
        render_all(&a)
    );
}

#[test]
fn conditions_need_true_false() {
    let a = run_main(vec![
        declare("int", "n", Some(num("1"))),
        if_then(e("n"), vec![]),
        while_loop("while", text("yes"), vec![brk()]),
    ]);
    assert_codes(&a, &["B2C-W0519", "B2C-E0301"]);
    assert!(
        a.diagnostics[1]
            .message
            .starts_with("The condition needs a true/false value, but this is text")
    );
}

#[test]
fn convert_and_prompts() {
    let a = run_main(vec![
        declare("int", "x", Some(convert("int", text("12")))),
        declare("double", "y", Some(convert("double", boolean(true)))),
        ask("x", Some(num("5"))),
        ask("x", Some(join(vec![text("Value "), num("1"), text(": ")]))),
    ]);
    assert_codes(&a, &["B2C-E0301", "B2C-W0519", "B2C-E0301"]);
    assert!(
        a.diagnostics[1].message.contains("true becomes 1"),
        "{}",
        a.diagnostics[1].message
    );
    assert!(a.diagnostics[2].message.contains("question"));
    // Converting a character gives its code, which is fine.
    assert_clean(&run_main(vec![declare(
        "int",
        "x",
        Some(convert("int", chr("A"))),
    )]));
}

#[test]
fn literals_are_checked() {
    let a = run_main(vec![declare("int", "x", Some(num("3000000000")))]);
    let d = only(&a, "B2C-E0517");
    assert!(d.message.contains("3000000000.0"), "{}", d.message);
    assert_eq!(
        d.primary.part,
        Part::Field {
            name: String::from("VALUE")
        }
    );
    assert_codes(
        &run_main(vec![declare("double", "x", Some(num("1e999")))]),
        &["B2C-E0517"],
    );
    assert_codes(
        &run_main(vec![declare("int", "x", Some(num("0xFFFFFFFFF")))]),
        &["B2C-E0517"],
    );
    assert_codes(
        &run_main(vec![declare("int", "x", Some(e("99999999999")))]),
        &["B2C-E0517"],
    );
    assert_codes(
        &run_main(vec![declare("int", "x", Some(num("1.2.3")))]),
        &["B2C-E0310"],
    );
    assert_codes(
        &run_main(vec![declare("int", "x", Some(num("012")))]),
        &["B2C-E0310"],
    );
    assert_codes(
        &run_main(vec![declare("int", "x", Some(num("")))]),
        &["B2C-E0310"],
    );
    assert_codes(
        &run_main(vec![declare("int", "x", Some(num("--1")))]),
        &["B2C-E0310"],
    );
    assert_codes(
        &run_main(vec![declare("int", "x", Some(e("1x")))]),
        &["B2C-E0310"],
    );
    assert_clean(&run_main(vec![declare("int", "x", Some(num("1'000")))]));
    // The smallest int fits, although its digits alone do not: it becomes
    // `-2147483647 - 1`, as INT_MIN is written.
    for value in [
        num("-2147483648"),
        e("-2147483648"),
        e("-(2'147'483'648)"),
        e("-0x80000000"),
    ] {
        let a = run_main(vec![declare("int", "x", Some(value))]);
        assert_clean(&a);
        let program = b2c_codegen::generate(&a.program, &b2c_codegen::CodegenOptions::default());
        assert!(
            program.files[0].contents.contains("int x = -2147483647 - 1;"),
            "{}",
            program.files[0].contents
        );
    }
    assert_codes(
        &run_main(vec![declare("int", "x", Some(num("-2147483649")))]),
        &["B2C-E0517"],
    );
    assert_codes(
        &run_main(vec![declare("int", "x", Some(e("2147483648")))]),
        &["B2C-E0517"],
    );
    // Characters must be one ASCII character.
    let a = run_main(vec![declare("char", "x", Some(chr("ab")))]);
    assert!(only(&a, "B2C-E0312").message.contains("exactly one character"));
    let a = run_main(vec![declare("char", "x", Some(chr("é")))]);
    assert!(only(&a, "B2C-E0312").message.contains("ASCII"));
    let a = run_main(vec![declare("char", "x", Some(e("'ab'")))]);
    only(&a, "B2C-E0312");
    let a = run_main(vec![declare("std::string", "x", Some(text("a\0b")))]);
    only(&a, "B2C-E0312");
}

#[test]
fn every_value_type_can_be_printed_and_asked() {
    let mut body = vars();
    body.push(print(vec![
        get("i"),
        get("d"),
        get("c"),
        get("b"),
        get("s"),
        e("i < 2"),
    ]));
    for name in ["i", "d", "c", "b", "s"] {
        body.push(ask(name, Some(text("? "))));
    }
    assert_clean(&run_main(body));
}

#[test]
fn errors_from_operators_point_at_the_operator_expression() {
    let a = check("int", "1 + (s - 2)");
    let d = only(&a, "B2C-E0302");
    assert_eq!(
        d.primary.part,
        Part::Tokens {
            input: String::from("VALUE"),
            start: 2,
            end: 7
        }
    );
}

#[test]
fn bool_number_mix_in_blocks() {
    let mut body = vars();
    body.push(declare("bool", "r", Some(logic("and", vec![get("b"), get("i")]))));
    body.push(declare("bool", "r2", Some(not(get("d")))));
    body.push(declare("int", "r3", Some(ternary(get("b"), get("b"), num("2")))));
    assert_codes(&run_main(body), &["B2C-W0519", "B2C-W0519", "B2C-W0519"]);
}

#[test]
fn var_get_of_function_symbol() {
    let a = run(vec![
        main(vec![print(vec![
            json!({"block": {"id": "g", "type": "var.get", "v": 1, "fields": {"VAR": {"ref": "sym_f"}}}}),
        ])]),
        func("f", "int", &[], vec![ret(Some(num("1")))]),
    ]);
    let d = only(&a, "B2C-E0207");
    assert_eq!(
        d.primary.part,
        Part::Field {
            name: String::from("VAR")
        }
    );
    let ItemKind::Main(main) = &a.program.modules[0].items[1].kind else {
        panic!()
    };
    let StmtKind::Print { items, .. } = &main.body.stmts[0].kind else {
        panic!()
    };
    assert!(matches!(items[0].kind, ExprKind::Int(_)) && items[0].ty == Type::Error);
}
