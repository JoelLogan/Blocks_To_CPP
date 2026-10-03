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

#[test]
fn analysis_is_deterministic() {
    let blocks = guessing_game();
    let first = analyze(&doc(blocks.clone()));
    let second = analyze(&doc(blocks));
    assert_eq!(first, second);
    assert_eq!(snapshot(&first), snapshot(&second));
}
