//! The catalog snapshot cases of `catalog/core/functions.toml`, with
//! package C2's default values of parameters (w2.md §2.8).

use serde_json::json;

use super::{Blocks, Case, F, nested, reference, tokens};

/// *define*, the two calls and *return*.
pub(super) fn cases(b: &Blocks) -> Vec<Case> {
    let return_zero = b.block(
        "func.return",
        &json!({ "inputs": { "VALUE": tokens(&json!([{ "num": "0" }])) } }),
    );
    let number = |text: &str| tokens(&json!([{ "num": text }]));
    let text = |value: &str| tokens(&json!([{ "str": value }]));
    // `f(name, times = 2)`, printing `name` `times` times: a read-only text
    // and an int with a default value, which only the declaration shows.
    let with_defaults = b.define_f(&json!({
        "extra": { "params": [
            { "sym": "s_name", "name": "name", "type": "std::string", "mode": "read_only" },
            { "sym": "s_times", "name": "times", "type": "int", "mode": "copy" },
            { "sym": "s_end", "name": "end", "type": "std::string", "mode": "read_only" },
        ] },
        "inputs": { "DEFAULT1": number("2"), "DEFAULT2": text("!") },
        "statements": { "BODY": [b.block("control.repeat", &json!({
            "inputs": { "TIMES": tokens(&json!([reference("s_times")])) },
            "statements": { "BODY": [b.block("io.print", &json!({
                "extra": { "itemCount": 2 },
                "inputs": { "ITEM0": tokens(&json!([reference("s_name")])),
                            "ITEM1": tokens(&json!([reference("s_end")])) },
            }))] },
        }))] },
    }));
    let call_f = |args: &[serde_json::Value]| {
        let inputs: serde_json::Map<String, serde_json::Value> = args
            .iter()
            .enumerate()
            .map(|(index, arg)| (format!("ARG{index}"), arg.clone()))
            .collect();
        b.block(
            "func.call_stmt",
            &json!({ "fields": { "FUNC": reference(F) }, "extra": { "argCount": args.len() }, "inputs": inputs }),
        )
    };
    vec![
        Case::new(
            "func.define",
            "the void function f, next to an empty main",
            vec![b.main(&[]), b.define_f(&json!({}))],
        ),
        Case::variant(
            "func.define",
            "defaults",
            "the void function f(name, times = 2, end = \"!\"), printing name and end times times; main \
             calls it with one, two and three arguments",
            vec![
                b.main(&[
                    call_f(&[text("Ann")]),
                    call_f(&[text("Bob"), number("1")]),
                    call_f(&[text("Cy"), number("3"), text("?")]),
                ]),
                with_defaults,
            ],
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
