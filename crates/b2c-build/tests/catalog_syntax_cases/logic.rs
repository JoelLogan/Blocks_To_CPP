//! The catalog snapshot cases of `catalog/core/logic.toml`.

use super::{Blocks, Case, only_printed};

/// Each block printed.
pub(super) fn cases(b: &Blocks) -> Vec<Case> {
    ["logic.boolean", "logic.operation", "logic.not", "logic.ternary"]
        .into_iter()
        .map(|kind| only_printed(b, kind))
        .collect()
}
