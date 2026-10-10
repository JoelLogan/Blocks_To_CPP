//! Snapshots of the SAST (as JSON) and of diagnostics for small programs.

// Test code: unwrap/expect/panic and printing to stderr are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod common;

use b2c_lang::{Analysis, analyze};
use common::*;
use serde_json::json;

fn snapshot(a: &Analysis) -> String {
    let program = serde_json::to_string_pretty(&a.program).expect("serialise");
    format!("{program}\n--- diagnostics ---\n{}\n", render_all(a))
}

/// The guessing game of spec §3.13.1, built from blocks and typed slots.
fn guessing_game() -> Vec<serde_json::Value> {
    vec![main(vec![
        declare("int", "secret", Some(random_int(num("1"), num("100")))),
        declare("int", "guess", Some(num("0"))),
        print(vec![text("Guess a number from 1 to 100!")]),
        while_loop(
            "until",
            e("guess == secret"),
            vec![
                ask("guess", Some(text("Your guess: "))),
                if_else(
                    vec![
                        (e("guess < secret"), vec![print(vec![text("Too low!")])]),
                        (e("guess > secret"), vec![print(vec![text("Too high!")])]),
                    ],
                    Some(vec![print(vec![text("Correct!")])]),
                ),
            ],
        ),
    ])]
}

#[test]
fn guessing_game_sast() {
    reset_ids();
    let a = run(guessing_game());
    assert_clean(&a);
    insta::assert_snapshot!(snapshot(&a));
}

#[test]
fn functions_sast() {
    reset_ids();
    let mut area = func(
        "area",
        "double",
        &[("w", "double", "copy"), ("h", "double", "copy")],
        vec![ret(Some(e("w * h")))],
    );
    area["comment"] = json!({"text": "Area of a rectangle"});
    let a = run(vec![
        main(vec![
            declare("double", "a", Some(call("area", vec![num("2.5"), e("4")]))),
            declare("int", "count", Some(num("0"))),
            call_stmt("bump", vec![get("count")]),
            for_range(
                "i",
                num("1"),
                "through",
                num("3"),
                None,
                vec![print(vec![join(vec![text("Round "), get("i")])])],
            ),
            print(vec![get("a"), get("count")]),
        ]),
        area,
        func(
            "bump",
            "void",
            &[("n", "int", "editable")],
            vec![change("n", num("1"))],
        ),
    ]);
    assert_clean(&a);
    insta::assert_snapshot!(snapshot(&a));
}

#[test]
fn diagnostics_tour() {
    reset_ids();
    let a = run(vec![
        main(vec![
            declare("int", "x", Some(text("five"))),
            declare("std::string", "name", Some(e("\"Ada\" + 1"))),
            print(vec![get("missing")]),
            print(vec![e("y * 2")]),
            declare("int", "y", Some(num("3000000000"))),
            if_then(e("x = 1"), vec![]),
            declare("double", "avg", Some(e("x / 2"))),
            if_then(e("avg == 0.5"), vec![brk()]),
            set("x", num("2.5")),
            call_stmt("half", vec![get("x"), num("1")]),
            forever(vec![print(vec![text("tick")])]),
            print(vec![text("never")]),
        ]),
        func(
            "half",
            "int",
            &[("v", "int", "copy")],
            vec![if_then(e("v > 0"), vec![ret(Some(e("v / 2")))])],
        ),
    ]);
    insta::assert_snapshot!(render_all(&a));
}

/// Numbers typed with a sign, each saved as one `num` token as the editor
/// saves them (spec §3.4), through the analyser and the generator: a
/// countdown that changes its counter by -1, the smallest `int`, a negative
/// decimal and a subtraction of a negative number.
#[test]
fn typed_negative_numbers_cpp() {
    reset_ids();
    let a = run(vec![main(vec![
        declare("int", "n", Some(typed_num("3"))),
        repeat(
            typed_num("3"),
            vec![print(vec![get("n")]), change("n", typed_num("-1"))],
        ),
        print(vec![text("Liftoff!")]),
        declare("int", "lowest", Some(typed_num("-2147483648"))),
        declare("double", "below", Some(typed_num("-0.5"))),
        declare(
            "int",
            "back",
            Some(json!({"expr": [{"ref": sym("n")}, {"op": "-"}, {"num": "-1"}]})),
        ),
    ])]);
    assert_clean(&a);
    let project = b2c_codegen::generate(&a.program, &b2c_codegen::CodegenOptions::default());
    let cpp = &project.files[0].contents;
    let main = cpp
        .get(cpp.find("int main()").expect("a main function")..)
        .unwrap_or_default();
    insta::assert_snapshot!(main);
}

#[test]
fn analysis_is_deterministic() {
    let blocks = guessing_game();
    let first = analyze(&doc(blocks.clone()));
    let second = analyze(&doc(blocks));
    assert_eq!(first, second);
    assert_eq!(snapshot(&first), snapshot(&second));
}
