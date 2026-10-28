//! The catalog snapshot cases of `catalog/core/program.toml`, with package
//! C2's arguments of `main` (w2.md §1.9) and package S's *use header* and
//! *check at compile time* (§1.6).

use serde_json::{Value, json};

use super::{Blocks, Case, X, declaration, nested, only_step, reference, tokens};

/// *use header* with `header`, on the canvas.
fn use_header(b: &Blocks, header: &str) -> Value {
    b.block(
        "program.use_header",
        &json!({ "x": 400, "y": 40, "fields": { "HEADER": header } }),
    )
}

/// `program.main` alone, `program.exit` in `main` and in a function; *use
/// header* beside an empty `main`, and naming a header a block of the program
/// includes already; *check at compile time* of `true`, and of a constant.
pub(super) fn cases(b: &Blocks) -> Vec<Case> {
    vec![
        Case::new("program.main", "alone", vec![b.main(&[])]),
        Case::variant(
            "program.main",
            "args",
            "with arguments, named args, printing how many there are",
            vec![b.block(
                "program.main",
                &json!({
                    "x": 40, "y": 40,
                    "fields": { "ARGS": true, "ARGV": declaration("s_args", "args") },
                    "statements": { "BODY": [b.print(&nested(&b.block(
                        "coll.length",
                        &json!({ "fields": { "LIST": reference("s_args") } }),
                    )))] },
                }),
            )],
        ),
        only_step(b, "program.exit"),
        Case::variant(
            "program.exit",
            "in-function",
            "the body of the void function f (std::exit), next to an empty main",
            vec![b.main(&[]), b.function("void", &[b.plain("program.exit")])],
        ),
        Case::new(
            "program.use_header",
            "next to an empty main, as the toolbox gives it (<cmath>)",
            vec![
                b.main(&[]),
                b.block("program.use_header", &json!({ "x": 400, "y": 40 })),
            ],
        ),
        Case::variant(
            "program.use_header",
            "merged",
            "<iostream> and <cmath> next to a main that prints 2.5 rounded (math.fn as the toolbox gives it): \
             each header is included once",
            vec![
                use_header(b, "<iostream>"),
                b.main(&[b.print(&nested(&b.plain("math.fn")))]),
                use_header(b, "<cmath>"),
            ],
        ),
        only_step(b, "program.check"),
        Case::variant(
            "program.check",
            "constant",
            "after the constant x (an int, 3), checking that x × 2 = 6 with a quoted message",
            vec![b.main(&[
                {
                    let mut constant = b.create_x(&json!({ "CONST": true }));
                    constant["inputs"] = json!({ "VALUE": tokens(&json!([{ "num": "3" }])) });
                    constant
                },
                b.block(
                    "program.check",
                    &json!({
                        "fields": { "MESSAGE": "x is \"three\"" },
                        "inputs": { "COND": tokens(&json!([
                            reference(X), { "op": "*" }, { "num": "2" }, { "op": "==" }, { "num": "6" }
                        ])) },
                    }),
                ),
            ])],
        ),
    ]
}
