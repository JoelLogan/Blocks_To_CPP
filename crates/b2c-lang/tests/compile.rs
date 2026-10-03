//! Tricky but valid programs: the analyser reports no errors, and g++
//! accepts the C++ generated for them (spec §7.5.3: code generated after a
//! clean analysis must compile). Each case targets a corner where the
//! analyser and C++ could disagree: text literals that are pointers in C++,
//! characters used as numbers, names that the C++ code also uses, `std`
//! functions found by argument-dependent lookup, and so on.
//!
//! Like the other g++ checks, these pass without checking anything when g++
//! is not on `PATH`, unless `B2C_REQUIRE_GXX` is set.

// Test code: unwrap/expect/panic and printing to stderr are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod common;

use b2c_codegen::{CodegenOptions, generate};
use b2c_ir::diag::Severity;
use common::*;
use serde_json::{Value, json};

/// Asserts that a program has no analyser errors and that g++ accepts it.
#[track_caller]
fn compiles(blocks: Vec<Value>) {
    let a = run(blocks);
    assert!(
        !a.diagnostics.iter().any(|d| d.severity == Severity::Error),
        "unexpected errors:\n{}",
        render_all(&a)
    );
    let project = generate(&a.program, &CodegenOptions::default());
    let source = &project.files[0].contents;
    assert!(
        !source.contains("/* error */"),
        "the generator needed error placeholders:\n{source}"
    );
    if !gxx::available() {
        return;
    }
    if let Err(messages) = gxx::syntax_check(source, false) {
        panic!("g++ rejected:\n{messages}\n--- source ---\n{source}");
    }
}

/// `main` with one variable of each type, the given statements, and a
/// `print` that uses every variable.
fn with_vars(statements: Vec<Value>) -> Vec<Value> {
    let mut body = vec![
        declare("int", "i", Some(num("1"))),
        declare("double", "d", Some(num("0.5"))),
        declare("bool", "b", Some(boolean(false))),
        declare("char", "c", Some(chr("a"))),
        declare("std::string", "s", Some(text("x"))),
    ];
    body.extend(statements);
    body.push(print(vec![get("i"), get("d"), get("b"), get("c"), get("s")]));
    vec![main(body)]
}

#[test]
fn text_literals_and_text_values_mix() {
    compiles(with_vars(vec![
        print(vec![e("b ? \"yes\" : \"no\"")]),
        set("s", e("b ? \"x\" : s")),
        set("s", e("b ? s : \"longer\"")),
        declare("auto", "t", Some(e("b ? \"a\" : \"bb\""))),
        print(vec![get("t")]),
        print(vec![e("\"abc\" < \"abd\""), e("\"a\" == s"), e("s != \"q\"")]),
        print(vec![join(vec![e("b ? \"p\" : \"qq\""), get("s")])]),
    ]));
}

#[test]
fn characters_are_numbers_and_can_be_assigned_to_text() {
    compiles(with_vars(vec![
        set("c", e("c + 1")),
        print(vec![e("+c"), e("c < 'z'"), e("c == 65"), e("c % 2")]),
        set("s", e("c")),
        set("s", chr("?")),
        print(vec![join(vec![
            chr("q"),
            get("i"),
            get("b"),
            get("d"),
            e("b ? 'a' : 'b'"),
        ])]),
    ]));
}

#[test]
fn smallest_int() {
    compiles(with_vars(vec![
        set("i", e("-2147483648")),
        set("i", num("-2147483648")),
        set("i", e("-0x80000000")),
    ]));
}

#[test]
fn names_the_generated_code_also_uses() {
    compiles(vec![
        main(vec![
            declare("int", "string", Some(num("1"))),
            declare("int", "cout", Some(num("2"))),
            declare("int", "endl", Some(num("3"))),
            declare("int", "n", Some(num("3"))),
            declare("int", "i", Some(num("3"))),
            declare("int", "size", Some(num("3"))),
            repeat(
                e("n"),
                vec![
                    change("n", num("1")),
                    print(vec![get("string"), get("cout"), get("endl"), get("size")]),
                ],
            ),
            repeat(call("j", vec![]), vec![print(vec![get("i")])]),
            call_stmt("ask", vec![]),
            call_stmt("random_int", vec![]),
        ]),
        func("j", "int", &[], vec![ret(Some(num("2")))]),
        func("ask", "void", &[], vec![]),
        func("random_int", "void", &[], vec![]),
    ]);
}

#[test]
fn functions_named_like_std_templates() {
    // `std::size`, `std::swap` and friends are templates, so a user function
    // with the same name wins for text arguments.
    compiles(vec![
        main(vec![
            declare("std::string", "s", Some(text("a"))),
            declare("std::string", "t", Some(text("b"))),
            call_stmt("swap", vec![get("s"), get("t")]),
            print(vec![call("size", vec![get("s")])]),
        ]),
        func(
            "swap",
            "void",
            &[("a", "std::string", "editable"), ("z", "std::string", "editable")],
            vec![],
        ),
        func(
            "size",
            "int",
            &[("text", "std::string", "read_only")],
            vec![ret(Some(num("1")))],
        ),
    ]);
}

#[test]
fn editable_and_text_parameters() {
    compiles(vec![
        main(vec![
            declare("int", "x", Some(num("1"))),
            declare("std::string", "s", Some(text("a"))),
            call_stmt("two", vec![get("x"), get("x")]),
            call_stmt("show", vec![e("\"lit\"")]),
            call_stmt("show", vec![join(vec![get("s"), num("1")])]),
            call_stmt("show", vec![e("true ? s : \"x\"")]),
            print(vec![
                call("twice", vec![get("s")]),
                call("twice", vec![text("q")]),
            ]),
            print(vec![get("x")]),
        ]),
        func(
            "two",
            "void",
            &[("a", "int", "editable"), ("z", "int", "editable")],
            vec![change("a", num("1")), change("z", num("1"))],
        ),
        func(
            "show",
            "void",
            &[("t", "std::string", "read_only")],
            vec![print(vec![get("t")])],
        ),
        func(
            "twice",
            "std::string",
            &[("u", "std::string", "copy")],
            vec![ret(Some(join(vec![get("u"), get("u")])))],
        ),
    ]);
}

#[test]
fn return_conversions_and_endless_functions() {
    compiles(vec![
        main(vec![
            print(vec![
                call("f", vec![num("1")]),
                call("g", vec![]),
                call("h", vec![]),
                call("k", vec![]),
            ]),
            call_stmt("stop", vec![]),
        ]),
        func(
            "f",
            "char",
            &[("n", "int", "copy")],
            vec![ret(Some(e("'a' + n")))],
        ),
        func("g", "double", &[], vec![ret(Some(num("1")))]),
        func("h", "bool", &[], vec![ret(Some(e("1 < 2")))]),
        func("k", "std::string", &[], vec![ret(Some(text("lit")))]),
        // No `return` needed: every path stops the program or loops forever.
        func(
            "stop",
            "int",
            &[],
            vec![forever(vec![repeat(num("3"), vec![exit(num("2"))])])],
        ),
    ]);
}

#[test]
fn every_way_of_asking() {
    let mut statements = Vec::new();
    for var in ["i", "d", "b", "c", "s"] {
        let mut simple = ask(var, Some(chr("?")));
        simple["fields"]["MODE"] = json!("simple");
        statements.push(simple);
        statements.push(ask(var, None));
        statements.push(ask(var, Some(join(vec![text("v"), get("i")]))));
    }
    compiles(with_vars(statements));
}

#[test]
fn loops_with_unusual_bounds() {
    compiles(vec![main(vec![
        declare("int", "n", Some(num("5"))),
        for_range(
            "k",
            num("0"),
            "to",
            e("n"),
            Some(num("2")),
            vec![set("n", e("n - 1"))],
        ),
        for_range(
            "k2",
            num("10"),
            "down_to",
            chr("A"),
            Some(e("n")),
            vec![print(vec![get("k2")])],
        ),
        for_range(
            "k3",
            e("'a'"),
            "through",
            e("'z'"),
            None,
            vec![print(vec![get("k3")])],
        ),
        repeat(e("-5"), vec![]),
    ])]);
}

#[test]
fn until_conditions_are_inverted_correctly() {
    compiles(with_vars(vec![
        while_loop("until", e("d < 1.0"), vec![set("d", e("d + 1"))]),
        while_loop("until", e("s == \"x\" || not b"), vec![set("b", e("true"))]),
        while_loop("until", e("\"a\" < \"b\""), vec![brk()]),
        while_loop("until", e("false"), vec![brk()]),
    ]));
}

#[test]
fn deepest_accepted_nesting() {
    // The deepest expressions and blocks the analyser accepts still generate
    // complete code.
    let chain = vec!["1"; b2c_model::limits::MAX_EXPR_DEPTH + 1].join(" + ");
    let mut body = vec![print(vec![e(&chain)])];
    for _ in 0..60 {
        body = vec![if_then(e("true"), body)];
    }
    compiles(vec![main(body)]);
}
