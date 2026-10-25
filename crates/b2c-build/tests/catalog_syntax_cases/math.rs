//! The catalog snapshot cases of `catalog/core/math.toml`.

use super::{Blocks, Case, only_printed};

/// Each block printed.
pub(super) fn cases(b: &Blocks) -> Vec<Case> {
    [
        "math.number",
        "math.arithmetic",
        "math.compare",
        "math.random_int",
        "math.convert",
    ]
    .into_iter()
    .map(|kind| only_printed(b, kind))
    .collect()
}
