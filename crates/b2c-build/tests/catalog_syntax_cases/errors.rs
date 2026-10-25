//! The catalog snapshot cases of `catalog/core/errors.toml` (W2, package B).

use serde_json::{Value, json};

use super::{Blocks, Case, ONLY_STEP, declaration, nested, reference};

/// The symbol of the first catch part's error (`e`).
const E: &str = "s_e";

/// A catch part's variable, named as the toolbox names it.
fn caught(index: usize) -> (String, Value) {
    (format!("NAME{index}"), declaration(&format!("{E}{index}"), "e"))
}

/// *try … catch*, with one part per error type of `errors`, each part's
/// statements from `parts` (or none), and `body` in the `try` part.
fn try_block(b: &Blocks, errors: &[&str], body: &[Value], parts: &[Vec<Value>]) -> Value {
    let mut fields = serde_json::Map::new();
    let mut statements = serde_json::Map::new();
    statements.insert(String::from("TRY"), json!(body));
    for (index, error) in errors.iter().enumerate() {
        fields.insert(format!("ERROR{index}"), json!(error));
        let (name, decl) = caught(index);
        fields.insert(name, decl);
        if let Some(part) = parts.get(index) {
            statements.insert(format!("CATCH{index}"), json!(part));
        }
    }
    b.block(
        "error.try",
        &json!({ "extra": { "catchCount": errors.len() }, "fields": fields, "statements": statements }),
    )
}

/// *try … catch* in its forms; *throw* of each error it can throw;
/// *message of* and *throw the error again* in a catch part.
pub(super) fn cases(b: &Blocks) -> Vec<Case> {
    let message_of = |index: usize| {
        b.block(
            "error.message",
            &json!({ "fields": { "ERROR": reference(&format!("{E}{index}")) } }),
        )
    };
    let throw = |error: &str| b.block("error.throw", &json!({ "fields": { "ERROR": error } }));
    vec![
        Case::new(
            "error.try",
            "the only step of main, with one empty catch part for any error (e) and an empty try part",
            vec![b.main(&[try_block(b, &["std::exception"], &[], &[])])],
        ),
        Case::variant(
            "error.try",
            "three-parts",
            "the only step of main: its try part throws an invalid argument error, and its three catch \
             parts (invalid argument, out of range, runtime error) each print the message of their \
             error e",
            vec![b.main(&[try_block(
                b,
                &["std::invalid_argument", "std::out_of_range", "std::runtime_error"],
                &[throw("std::invalid_argument")],
                &[
                    vec![b.print(&nested(&message_of(0)))],
                    vec![b.print(&nested(&message_of(1)))],
                    vec![b.print(&nested(&message_of(2)))],
                ],
            )])],
        ),
        Case::new("error.throw", ONLY_STEP, vec![b.main(&[b.plain("error.throw")])]),
        Case::variant(
            "error.throw",
            "out-of-range",
            "throws an out-of-range error, the only step of main",
            vec![b.main(&[throw("std::out_of_range")])],
        ),
        Case::new(
            "error.message",
            "printed in the catch part of a try block, the only step of main, whose catch part catches \
             any error as e",
            vec![b.main(&[try_block(
                b,
                &["std::exception"],
                &[],
                &[vec![b.print(&nested(&message_of(0)))]],
            )])],
        ),
        Case::new(
            "error.rethrow",
            "the catch part of a try block, the only step of main, whose catch part catches any error as e",
            vec![b.main(&[try_block(
                b,
                &["std::exception"],
                &[],
                &[vec![b.plain("error.rethrow")]],
            )])],
        ),
    ]
}
