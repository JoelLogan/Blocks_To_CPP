//! The catalog snapshot cases of `catalog/core/control.toml`; *switch on*
//! and *do … then repeat* are W2's (package D).

use serde_json::{Value, json};

use super::{Blocks, Case, I, ONLY_STEP, X, declaration, only_step, reference, tokens};

/// An expression input of one whole number.
fn number(value: &str) -> Value {
    tokens(&json!([{ "num": value }]))
}

/// An expression input of one character.
fn letter(value: &str) -> Value {
    tokens(&json!([{ "chr": value }]))
}

/// `x <op> value`, as tokens.
fn compare_x(op: &str, value: &str) -> Value {
    tokens(&json!([reference(X), { "op": op }, { "num": value }]))
}

/// *switch on (x)* with one case per `(label, statements)`, and the default
/// part's statements when `default` is given.
fn switch_on_x(b: &Blocks, cases: Vec<(Value, Vec<Value>)>, default: Option<Vec<Value>>) -> Value {
    let mut inputs = serde_json::Map::new();
    let mut statements = serde_json::Map::new();
    inputs.insert(String::from("VALUE"), tokens(&json!([reference(X)])));
    let count = cases.len();
    for (index, (label, body)) in cases.into_iter().enumerate() {
        inputs.insert(format!("CASE{index}"), label);
        statements.insert(format!("DO{index}"), json!(body));
    }
    let has_default = default.is_some();
    if let Some(body) = default {
        statements.insert(String::from("DEFAULT"), json!(body));
    }
    b.block(
        "control.switch",
        &json!({
            "extra": { "caseCount": count, "hasDefault": has_default },
            "inputs": inputs,
            "statements": statements,
        }),
    )
}

/// The loops and *if*; *leave loop* and *skip to next round* in a loop;
/// *switch on* an `int` and a `char`, with the `break;` each case gets or
/// does not need; *do … then repeat* with *while* and *until*.
#[allow(
    clippy::too_many_lines,
    reason = "one case per block and variant, in catalog order"
)]
pub(super) fn cases(b: &Blocks) -> Vec<Case> {
    let in_forever = |kind: &'static str| {
        let forever = b.block(
            "control.forever",
            &json!({ "statements": { "BODY": [b.plain(kind)] } }),
        );
        Case::new(
            kind,
            "the body of a forever loop, the only step of main",
            vec![b.main(&[forever])],
        )
    };
    let change_x = || b.block("var.change", &json!({ "fields": { "VAR": reference(X) } }));
    vec![
        only_step(b, "control.if"),
        Case::new(
            "control.switch",
            "after creating x (an int), with case 1 printing x, an empty case 2 and a default part \
             printing x",
            vec![b.main(&[
                b.create_x(&json!({})),
                switch_on_x(
                    b,
                    vec![(number("1"), vec![b.print_x()]), (number("2"), Vec::new())],
                    Some(vec![b.print_x()]),
                ),
            ])],
        ),
        Case::variant(
            "control.switch",
            "char",
            "after creating x (a char 'a'), with cases 'a' and 'b' printing x and no default part",
            vec![b.main(&[
                {
                    let mut create = b.create_x(&json!({ "TYPE": "char" }));
                    create["inputs"]["VALUE"] = letter("a");
                    create
                },
                switch_on_x(
                    b,
                    vec![(letter("a"), vec![b.print_x()]), (letter("b"), vec![b.print_x()])],
                    None,
                ),
            ])],
        ),
        Case::variant(
            "control.switch",
            "leaving-cases",
            "in a repeat after creating x (an int): case 0 skips to the next round, case 1 stops the \
             program, case 2 changes x by 1, and the default part is empty",
            vec![b.main(&[
                b.create_x(&json!({})),
                b.block(
                    "control.repeat",
                    &json!({ "statements": { "BODY": [switch_on_x(
                        b,
                        vec![
                            (number("0"), vec![b.plain("control.continue")]),
                            (number("1"), vec![b.plain("program.exit")]),
                            (number("2"), vec![change_x()]),
                        ],
                        Some(Vec::new()),
                    )] } }),
                ),
            ])],
        ),
        Case::new(
            "control.fallthrough",
            "the last block of case 1 of a switch on x (an int) after a print of x, before case 2, which \
             prints x",
            vec![b.main(&[
                b.create_x(&json!({})),
                switch_on_x(
                    b,
                    vec![
                        (number("1"), vec![b.print_x(), b.plain("control.fallthrough")]),
                        (number("2"), vec![b.print_x()]),
                    ],
                    None,
                ),
            ])],
        ),
        only_step(b, "control.wait"),
        only_step(b, "control.while"),
        Case::new(
            "control.do_while",
            ONLY_STEP,
            vec![b.main(&[b.plain("control.do_while")])],
        ),
        Case::variant(
            "control.do_while",
            "until",
            "after creating x (an int), changing x by 1 until x > 3, then printing x",
            vec![b.main(&[
                b.create_x(&json!({})),
                b.block(
                    "control.do_while",
                    &json!({
                        "fields": { "MODE": "until" },
                        "inputs": { "COND": compare_x(">", "3") },
                        "statements": { "BODY": [change_x()] },
                    }),
                ),
                b.print_x(),
            ])],
        ),
        only_step(b, "control.repeat"),
        Case::new(
            "control.for_range",
            "the only step of main, counting with i",
            vec![b.main(&[b.block(
                "control.for_range",
                &json!({ "fields": { "VAR": declaration(I, "i") } }),
            )])],
        ),
        only_step(b, "control.forever"),
        in_forever("control.break"),
        in_forever("control.continue"),
    ]
}
