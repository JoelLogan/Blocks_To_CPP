//! The catalog snapshot cases of `catalog/core/program.toml`, with package
//! C2's arguments of `main` (w2.md §1.9).

use serde_json::json;

use super::{Blocks, Case, declaration, nested, only_step, reference};

/// `program.main` alone, `program.exit` in `main` and in a function.
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
    ]
}
