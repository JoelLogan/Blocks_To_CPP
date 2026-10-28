//! The catalog snapshot cases of `catalog/core/files.toml` (W2, package D,
//! and package S's *open file*, *read line*, *write* and *file exists*).

use serde_json::{Value, json};

use super::{Blocks, Case, X, declaration, nested, only_printed, reference, tokens};

/// The symbol of the line of the outer file loop (`line`).
const LINE: &str = "s_line";

/// The symbol of the line of the inner file loop (`word`).
const WORD: &str = "s_word";

/// The symbol of the stream *open file* declares (`file`).
const FILE: &str = "s_file";

/// *open file* `path` for `mode` as `file`, with its *if it can't be
/// opened* part when `failed` is given (the toolbox's preset has the part).
fn open(b: &Blocks, mode: &str, path: &str, failed: Option<&[Value]>) -> Value {
    let mut statements = json!({});
    if let Some(failed) = failed {
        statements["FAILED"] = json!(failed);
    }
    b.block(
        "file.open",
        &json!({
            "extra": { "hasFailed": failed.is_some() },
            "fields": { "FILE": declaration(FILE, "file"), "MODE": mode },
            "inputs": { "PATH": tokens(&json!([{ "str": path }])) },
            "statements": statements,
        }),
    )
}

/// *read line from file into x*.
fn read_line(b: &Blocks) -> Value {
    b.block(
        "file.read_line",
        &json!({ "fields": { "FILE": reference(FILE), "LINE": reference(X) } }),
    )
}

/// *write* these items to `file`, with a new line or without.
fn write(b: &Blocks, items: &[Value], newline: bool) -> Value {
    let inputs: serde_json::Map<String, Value> = items
        .iter()
        .enumerate()
        .map(|(index, item)| (format!("ITEM{index}"), item.clone()))
        .collect();
    b.block(
        "file.write",
        &json!({
            "extra": { "itemCount": items.len() },
            "fields": { "FILE": reference(FILE), "NEWLINE": newline },
            "inputs": inputs,
        }),
    )
}

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
/// differ; then [`stream_cases`].
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
    let mut cases = vec![
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
    ];
    cases.extend(stream_cases(b));
    cases
}

/// *open file* for each mode; *read line* printed and as a loop's
/// condition; *write* with a new line and without; *file exists* printed
/// (package S).
fn stream_cases(b: &Blocks) -> Vec<Case> {
    let empty: &[Value] = &[];
    vec![
        Case::new(
            "file.open",
            "the only step of main, opening data.txt for reading as file, with the toolbox's empty part for a \
             file that can't be opened",
            vec![b.main(&[open(b, "read", "data.txt", Some(empty))])],
        ),
        Case::variant(
            "file.open",
            "write",
            "the only step of main, opening out.txt for writing as file; its part for a file that can't be \
             opened prints to the error stream",
            vec![b.main(&[open(
                b,
                "write",
                "out.txt",
                Some(&[b.block(
                    "io.print",
                    &json!({
                        "fields": { "STREAM": "err" },
                        "inputs": { "ITEM0": tokens(&json!([{ "str": "Cannot open out.txt" }])) },
                    }),
                )]),
            )])],
        ),
        Case::variant(
            "file.open",
            "append",
            "the only step of main, opening log.txt for adding to its end as file, without a part for a file \
             that can't be opened",
            vec![b.main(&[open(b, "append", "log.txt", None)])],
        ),
        Case::new(
            "file.read_line",
            "printed after the text x (empty) and opening data.txt for reading as file",
            vec![b.main(&[
                b.create_text_x(&tokens(&json!([{ "str": "" }]))),
                open(b, "read", "data.txt", None),
                b.print(&nested(&read_line(b))),
            ])],
        ),
        Case::variant(
            "file.read_line",
            "loop",
            "the condition of a while loop that prints x, after the text x (empty) and opening data.txt for \
             reading as file",
            vec![b.main(&[
                b.create_text_x(&tokens(&json!([{ "str": "" }]))),
                open(b, "read", "data.txt", None),
                b.block(
                    "control.while",
                    &json!({
                        "inputs": { "COND": nested(&read_line(b)) },
                        "statements": { "BODY": [b.print_x()] },
                    }),
                ),
            ])],
        ),
        Case::new(
            "file.write",
            "after opening out.txt for writing as file, writing the toolbox's text",
            vec![b.main(&[
                open(b, "write", "out.txt", None),
                b.block("file.write", &json!({ "fields": { "FILE": reference(FILE) } })),
            ])],
        ),
        Case::variant(
            "file.write",
            "items",
            "after opening out.txt for writing as file, writing a text, a number and a true/false value without \
             a new line",
            vec![b.main(&[
                open(b, "write", "out.txt", None),
                write(
                    b,
                    &[
                        tokens(&json!([{ "str": "count: " }])),
                        tokens(&json!([{ "num": "3" }])),
                        tokens(&json!([{ "kw": "true" }])),
                    ],
                    false,
                ),
            ])],
        ),
        only_printed(b, "file.exists"),
    ]
}
