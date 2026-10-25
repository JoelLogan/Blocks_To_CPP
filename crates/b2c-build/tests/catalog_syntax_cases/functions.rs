//! The catalog snapshot cases of `catalog/core/functions.toml`.

use serde_json::json;

use super::{Blocks, Case, F, nested, reference, tokens};

/// *define*, the two calls and *return*.
pub(super) fn cases(b: &Blocks) -> Vec<Case> {
    let return_zero = b.block(
        "func.return",
        &json!({ "inputs": { "VALUE": tokens(&json!([{ "num": "0" }])) } }),
    );
    vec![
        Case::new(
            "func.define",
            "the void function f, next to an empty main",
            vec![b.main(&[]), b.define_f(&json!({}))],
        ),
        Case::new(
            "func.call_stmt",
            "the only step of main, calling the empty void function f",
            vec![
                b.main(&[b.block("func.call_stmt", &json!({ "fields": { "FUNC": reference(F) } }))]),
                b.function("void", &[]),
            ],
        ),
        Case::new(
            "func.return",
            "the body of the void function f, next to an empty main",
            vec![b.main(&[]), b.function("void", &[b.plain("func.return")])],
        ),
        Case::new(
            "func.call",
            "calls the function f, which returns the int 0, as the only item of a print in main",
            vec![
                b.main(&[b.print(&nested(
                    &b.block("func.call", &json!({ "fields": { "FUNC": reference(F) } })),
                ))]),
                b.function("int", &[return_zero]),
            ],
        ),
    ]
}
