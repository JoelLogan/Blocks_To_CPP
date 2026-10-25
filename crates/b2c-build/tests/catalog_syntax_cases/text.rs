//! The catalog snapshot cases of `catalog/core/text.toml`.

use super::{Blocks, Case, nested, only_printed};

/// Each block printed, and a text literal that becomes a `std::string`.
pub(super) fn cases(b: &Blocks) -> Vec<Case> {
    let mut cases: Vec<Case> = ["text.literal", "text.char", "text.join"]
        .into_iter()
        .map(|kind| only_printed(b, kind))
        .collect();
    cases.push(Case::variant(
        "text.literal",
        "as-string",
        "the value of create x, a text, before a print of x, in main",
        vec![b.main(&[b.create_text_x(&nested(&b.plain("text.literal"))), b.print_x()])],
    ));
    cases
}
