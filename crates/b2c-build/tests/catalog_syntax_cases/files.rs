//! The catalog snapshot cases of `catalog/core/files.toml` (W2, package D).

use serde_json::{Value, json};

use super::{Blocks, Case, declaration, nested, reference, tokens};

/// The symbol of the line of the outer file loop (`line`).
const LINE: &str = "s_line";

/// The symbol of the line of the inner file loop (`word`).
const WORD: &str = "s_word";

/// *for each line* over `path`, naming its line `name` (symbol `symbol`),
/// with `body`, and its *if it can't be opened* part when `failed` is given
/// (the toolbox's preset has the part).
fn lines(
    b: &Blocks,
    symbol: &str,
    name: &str,
    path: &str,
    body: &[Value],
    failed: Option<&[Value]>,
) -> Value {
    let mut statements = json!({ "BODY": body });
    if let Some(failed) = failed {
        statements["FAILED"] = json!(failed);
    }
    b.block(
        "file.for_each_line",
        &json!({
            "extra": { "hasFailed": failed.is_some() },
            "fields": { "LINE": declaration(symbol, name) },
            "inputs": { "PATH": tokens(&json!([{ "str": path }])) },
            "statements": statements,
        }),
    )
}

/// A print of the variable `symbol`.
fn print_of(b: &Blocks, symbol: &str) -> Value {
    b.print(&tokens(&json!([reference(symbol)])))
}

/// *for each line* as the toolbox gives it, with its part for a file that
/// can't be opened, and without; two loops nested, whose hidden stream names
/// differ.
pub(super) fn cases(b: &Blocks) -> Vec<Case> {
    let error_and_stop = || {
        vec![
            b.block(
                "io.print",
                &json!({
                    "fields": { "STREAM": "err" },
                    "inputs": { "ITEM0": tokens(&json!([{ "str": "Cannot open data.txt" }])) },
                }),
            ),
            b.block(
                "program.exit",
                &json!({ "inputs": { "CODE": tokens(&json!([{ "num": "1" }])) } }),
            ),
        ]
    };
    let length_of = |symbol: &str| {
        nested(&b.block(
            "text.length",
            &json!({ "inputs": { "TEXT": tokens(&json!([reference(symbol)])) } }),
        ))
    };
    vec![
        Case::new(
            "file.for_each_line",
            "the only step of main, reading data.txt and printing each line; its part for a file that \
             can't be opened prints to the error stream and stops the program with exit code 1",
            vec![b.main(&[lines(
                b,
                LINE,
                "line",
                "data.txt",
                &[print_of(b, LINE)],
                Some(&error_and_stop()),
            )])],
        ),
        Case::variant(
            "file.for_each_line",
            "without-failed-part",
            "the only step of main, reading data.txt and printing each line, with no part for a file that \
             can't be opened",
            vec![b.main(&[lines(b, LINE, "line", "data.txt", &[print_of(b, LINE)], None)])],
        ),
        Case::variant(
            "file.for_each_line",
            "nested",
            "the only step of main, reading names.txt line by line; for each line, reading words.txt line \
             by line and printing the length of each word",
            vec![b.main(&[lines(
                b,
                LINE,
                "line",
                "names.txt",
                &[lines(
                    b,
                    WORD,
                    "word",
                    "words.txt",
                    &[b.print(&length_of(WORD))],
                    None,
                )],
                None,
            )])],
        ),
    ]
}
