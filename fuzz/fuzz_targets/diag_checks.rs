//! The oracle shared by the diagnostics-parser targets (`diag_sarif`,
//! `diag_json`, `diag_text`, `diag_output`): what must hold for every
//! [`ParsedOutput`], whatever the compiler printed
//! (docs/spec/07-toolchain-build-run.md §7.5.3, docs/spec/08-security.md
//! §8.13). The compiler's output quotes the user's code, so it is untrusted.
//!
//! * At most [`MAX_MESSAGES`] top-level messages, at most [`MAX_CHILDREN`]
//!   notes under any message and at most [`MAX_INCLUDE_DEPTH`] entries in an
//!   include chain.
//! * Message, function and symbol text is cleaned: at most
//!   [`MAX_MESSAGE_CHARS`] characters, or exactly one more when the text was
//!   cut and ends in `…`; no control character other than tab; typographic
//!   quotes (`‘’`) normalised to `'`.
//! * Line numbers are 1-based.
//! * A symbol is only ever reported for a linker message.
//!
//! The tree is walked with an explicit stack, so deeply nested notes cannot
//! overflow the fuzzer's stack.

// Each target uses only part of this module.
#![allow(dead_code)]

use b2c_toolchain::diagnostics::{
    CompilerMessage, MAX_CHILDREN, MAX_INCLUDE_DEPTH, MAX_MESSAGE_CHARS, MAX_MESSAGES, MessageOrigin,
    ParsedOutput, SourcePos,
};

/// Splits standard error from the SARIF file in a `diag_output` input; also
/// in `fuzz/diag_output.dict`.
pub const SARIF_SEPARATOR: &[u8] = b"\0sarif\0";

/// Checks every bound and cleaning rule of the module documentation.
pub fn check_output(parsed: &ParsedOutput) {
    assert!(
        parsed.messages.len() <= MAX_MESSAGES,
        "{} top-level messages, more than {MAX_MESSAGES}",
        parsed.messages.len()
    );
    let mut stack: Vec<&CompilerMessage> = parsed.messages.iter().collect();
    while let Some(message) = stack.pop() {
        check_message(message);
        stack.extend(&message.children);
    }
}

/// Checks that a `truncated` flag is backed by a full list: the parsers set
/// it only when they drop a message because [`MAX_MESSAGES`] are kept, or a
/// note because a message has [`MAX_CHILDREN`] (inputs over
/// `MAX_INPUT_BYTES` never reach the fuzzer). Not for `parse_output` with a
/// SARIF file, which also reports what was dropped from the compiler's text
/// messages it then discards.
pub fn check_truncation_is_real(parsed: &ParsedOutput) {
    if !parsed.truncated || parsed.messages.len() == MAX_MESSAGES {
        return;
    }
    let mut stack: Vec<&CompilerMessage> = parsed.messages.iter().collect();
    while let Some(message) = stack.pop() {
        if message.children.len() == MAX_CHILDREN {
            return;
        }
        stack.extend(&message.children);
    }
    panic!(
        "truncated, but no list is full ({} messages)",
        parsed.messages.len()
    );
}

/// Checks that every message, at any depth, came from `origin`.
pub fn check_origin(parsed: &ParsedOutput, origin: MessageOrigin) {
    let mut stack: Vec<&CompilerMessage> = parsed.messages.iter().collect();
    while let Some(message) = stack.pop() {
        assert_eq!(message.origin, origin, "unexpected origin: {message:?}");
        stack.extend(&message.children);
    }
}

fn check_message(message: &CompilerMessage) {
    check_text("message", &message.message);
    if let Some(function) = &message.function {
        check_text("function", function);
    }
    if let Some(symbol) = &message.symbol {
        check_text("symbol", symbol);
        assert_eq!(
            message.origin,
            MessageOrigin::Linker,
            "a symbol on a non-linker message: {message:?}"
        );
    }
    assert!(
        message.children.len() <= MAX_CHILDREN,
        "{} notes, more than {MAX_CHILDREN}",
        message.children.len()
    );
    assert!(
        message.included_from.len() <= MAX_INCLUDE_DEPTH,
        "an include chain of {}, longer than {MAX_INCLUDE_DEPTH}",
        message.included_from.len()
    );
    message
        .location
        .iter()
        .chain(&message.included_from)
        .for_each(check_pos);
}

fn check_pos(pos: &SourcePos) {
    assert!(pos.line >= 1, "line 0 in {pos:?}");
}

fn check_text(what: &str, text: &str) {
    let count = text.chars().count();
    assert!(
        count <= MAX_MESSAGE_CHARS || (count == MAX_MESSAGE_CHARS + 1 && text.ends_with('…')),
        "{what} has {count} characters, more than {MAX_MESSAGE_CHARS} plus an ellipsis"
    );
    assert!(
        !text.chars().any(|c| c.is_control() && c != '\t'),
        "{what} keeps a control character: {text:?}"
    );
    assert!(
        !text.contains(['\u{2018}', '\u{2019}']),
        "{what} keeps a typographic quote: {text:?}"
    );
}

/// Splits a `diag_output` input after its mode byte into standard error and
/// the SARIF file. With `with_sarif`, standard error is everything before the
/// first [`SARIF_SEPARATOR`] and the SARIF file everything after it (an empty
/// file when there is no separator); without it, the whole input is standard
/// error and there is no SARIF file.
pub fn split_output_input(rest: &[u8], with_sarif: bool) -> (&[u8], Option<&[u8]>) {
    if !with_sarif {
        return (rest, None);
    }
    match rest
        .windows(SARIF_SEPARATOR.len())
        .position(|window| window == SARIF_SEPARATOR)
    {
        Some(at) => (
            rest.get(..at).unwrap_or_default(),
            Some(rest.get(at + SARIF_SEPARATOR.len()..).unwrap_or_default()),
        ),
        None => (rest, Some(&[])),
    }
}
