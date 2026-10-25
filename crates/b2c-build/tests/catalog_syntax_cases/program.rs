//! The catalog snapshot cases of `catalog/core/program.toml`.

use super::{Blocks, Case, only_step};

/// `program.main` alone, `program.exit` in `main` and in a function.
pub(super) fn cases(b: &Blocks) -> Vec<Case> {
    vec![
        Case::new("program.main", "alone", vec![b.main(&[])]),
        only_step(b, "program.exit"),
        Case::variant(
            "program.exit",
            "in-function",
            "the body of the void function f (std::exit), next to an empty main",
            vec![b.main(&[]), b.function("void", &[b.plain("program.exit")])],
        ),
    ]
}
