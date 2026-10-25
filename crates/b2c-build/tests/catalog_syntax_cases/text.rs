//! The catalog snapshot cases of `catalog/core/text.toml`.

use serde_json::{Value, json};

use super::{Blocks, Case, X, declaration, nested, only_printed, reference, tokens};

/// Each block printed, and a text literal that becomes a `std::string`;
/// W2's blocks (package B): *split* as a list's value, and the forms each
/// option or input changes.
pub(super) fn cases(b: &Blocks) -> Vec<Case> {
    let mut cases: Vec<Case> = [
        "text.literal",
        "text.char",
        "text.join",
        "text.length",
        "text.letter",
        "text.case",
        "text.to_number",
        "text.as_text",
        "text.find",
        "text.part",
        "text.trim",
        "text.replace",
    ]
    .into_iter()
    .map(|kind| only_printed(b, kind))
    .collect();
    cases.push(Case::variant(
        "text.literal",
        "as-string",
        "the value of create x, a text, before a print of x, in main",
        vec![b.main(&[b.create_text_x(&nested(&b.plain("text.literal"))), b.print_x()])],
    ));
    cases.push(Case::variant(
        "text.letter",
        "variable-index",
        "the letter at the int x of the text literal, printed after create x in main",
        vec![b.main(&[
            b.create_x(&json!({})),
            b.print(&nested(&b.block(
                "text.letter",
                &json!({ "inputs": { "INDEX": tokens(&json!([reference(X)])) } }),
            ))),
        ])],
    ));
    cases.push(Case::variant(
        "text.case",
        "upper",
        "uppercase, the only item of a print, the only step of main",
        printed_with(b, "text.case", &json!({ "fields": { "CASE": "upper" } })),
    ));
    cases.push(Case::variant(
        "text.to_number",
        "double",
        "as a decimal number, the only item of a print, the only step of main",
        printed_with(b, "text.to_number", &json!({ "fields": { "TO": "double" } })),
    ));
    cases.extend(more_cases(b));
    cases
}

/// The forms of W2's other text blocks: *starts with* and *ends with*
/// (helpers in C++17), a part from a variable, *as text* of each type, and
/// *split* as a list's value.
fn more_cases(b: &Blocks) -> Vec<Case> {
    let mut cases = Vec::new();
    for (variant, op) in [("starts", "starts"), ("ends", "ends")] {
        cases.push(Case::variant(
            "text.find",
            variant,
            "the only item of a print, the only step of main (C++17: the support helper)",
            printed_with(b, "text.find", &json!({ "fields": { "OP": op } })),
        ));
    }
    cases.push(Case::variant(
        "text.part",
        "variable-start",
        "from the int x, printed after create x in main",
        vec![b.main(&[
            b.create_x(&json!({})),
            b.print(&nested(&b.block(
                "text.part",
                &json!({ "inputs": { "START": tokens(&json!([reference(X)])) } }),
            ))),
        ])],
    ));
    for (variant, value) in [
        ("double", json!([{ "num": "2.5" }])),
        ("char", json!([{ "chr": "c" }])),
        ("bool", json!([{ "kw": "true" }])),
        ("text", json!([{ "str": "hi" }])),
    ] {
        cases.push(Case::variant(
            "text.as_text",
            variant,
            "the only item of a print, the only step of main",
            printed_with(
                b,
                "text.as_text",
                &json!({ "inputs": { "VALUE": tokens(&value) } }),
            ),
        ));
    }
    cases.push(
        Case::new(
            "text.split",
            "the value of create parts, a list of text, the only step of main (format version 2)",
            vec![b.main(&[b.block(
                "var.declare",
                &json!({
                    "fields": {
                        "TYPE": { "std::vector": "std::string" },
                        "NAME": declaration("s_parts", "parts"),
                    },
                    "inputs": { "VALUE": nested(&b.plain("text.split")) },
                }),
            )])],
        )
        .with_format_version(2),
    );
    cases
}

/// `main` with only a print of the `kind` block, with `parts` merged into
/// what the toolbox gives.
fn printed_with(b: &Blocks, kind: &str, parts: &Value) -> Vec<Value> {
    vec![b.main(&[b.print(&nested(&b.block(kind, parts)))])]
}
