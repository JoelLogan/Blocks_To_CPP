//! The catalog snapshot cases of `catalog/core/io.toml`.

use serde_json::json;

use super::{Blocks, Case, X, only_step, reference, tokens};

/// *print*, *ask* in its three forms, and *show numbers with* (W2, package
/// B).
pub(super) fn cases(b: &Blocks) -> Vec<Case> {
    vec![
        only_step(b, "io.print"),
        only_step(b, "io.decimals"),
        Case::new(
            "io.ask",
            "asks for the int x, after create x and before a print of x, in main",
            b.with_x(b.block("io.ask", &json!({ "fields": { "VAR": reference(X) } }))),
        ),
        Case::variant(
            "io.ask",
            "simple",
            "a simple ask for the int x, after create x and before a print of x, in main",
            b.with_x(b.block(
                "io.ask",
                &json!({ "fields": { "VAR": reference(X), "MODE": "simple" } }),
            )),
        ),
        Case::variant(
            "io.ask",
            "text",
            "asks for the text x, after create x (an empty text) and before a print of x, in main",
            vec![b.main(&[
                b.create_text_x(&tokens(&json!([{ "str": "" }]))),
                b.block("io.ask", &json!({ "fields": { "VAR": reference(X) } })),
                b.print_x(),
            ])],
        ),
    ]
}
