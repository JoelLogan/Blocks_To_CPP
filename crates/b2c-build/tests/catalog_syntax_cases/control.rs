//! The catalog snapshot cases of `catalog/core/control.toml`.

use serde_json::json;

use super::{Blocks, Case, I, declaration, only_step};

/// The loops and *if*; *leave loop* and *skip to next round* in a loop.
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
    vec![
        only_step(b, "control.if"),
        only_step(b, "control.while"),
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
