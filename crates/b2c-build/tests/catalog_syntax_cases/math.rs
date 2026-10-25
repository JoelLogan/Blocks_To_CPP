//! The catalog snapshot cases of `catalog/core/math.toml`.

use serde_json::{Value, json};

use super::{Blocks, Case, nested, only_printed, tokens};

/// Each block printed; for W2's blocks (package B), the forms each option
/// changes, and *larger*, *smaller* and *kept between* with operands of
/// mixed types, which the output type converts (w2.md §1.4).
pub(super) fn cases(b: &Blocks) -> Vec<Case> {
    let mut cases: Vec<Case> = [
        "math.number",
        "math.arithmetic",
        "math.compare",
        "math.random_int",
        "math.convert",
        "math.fn",
        "math.abs",
        "math.minmax",
        "math.fn_two",
        "math.clamp",
        "math.random_real",
    ]
    .into_iter()
    .map(|kind| only_printed(b, kind))
    .collect();
    let number = |text: &str| tokens(&json!([{ "num": text }]));
    let letter = |text: &str| tokens(&json!([{ "chr": text }]));
    let printed = |kind: &'static str, variant: &str, placement: &'static str, parts: Value| {
        Case::variant(
            kind,
            variant,
            placement,
            vec![b.main(&[b.print(&nested(&b.block(kind, &parts)))])],
        )
    };
    cases.extend([
        printed(
            "math.fn",
            "sqrt-of-int",
            "the square root of the whole number 9, the only item of a print, the only step of main",
            json!({ "fields": { "FN": "sqrt" }, "inputs": { "VALUE": number("9") } }),
        ),
        printed(
            "math.abs",
            "double",
            "of the decimal number -2.5, the only item of a print, the only step of main",
            json!({ "inputs": { "VALUE": number("-2.5") } }),
        ),
        printed(
            "math.minmax",
            "min-char-double",
            "the smaller of the character 'a' and 2.5, the only item of a print, the only step of main",
            json!({ "fields": { "FN": "min" }, "inputs": { "A": letter("a"), "B": number("2.5") } }),
        ),
        printed(
            "math.minmax",
            "max-bool-int",
            "the larger of true and 2, the only item of a print, the only step of main",
            json!({ "inputs": { "A": tokens(&json!([{ "kw": "true" }])), "B": number("2") } }),
        ),
        printed(
            "math.fn_two",
            "hypot",
            "the hypotenuse of 3 and 4, the only item of a print, the only step of main",
            json!({ "fields": { "FN": "hypot" }, "inputs": { "A": number("3"), "B": number("4") } }),
        ),
        printed(
            "math.clamp",
            "mixed",
            "the character 'z' kept between 0 and 99.5, the only item of a print, the only step of main",
            json!({ "inputs": { "VALUE": letter("z"), "LOW": number("0"), "HIGH": number("99.5") } }),
        ),
    ]);
    cases
}
