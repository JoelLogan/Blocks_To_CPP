//! Building blocks for friendly diagnostic messages, and the sink that
//! collects diagnostics.
//!
//! User text (names, literal values) is never copied into a message as is:
//! [`quoted`] truncates it and replaces control and invisible characters with
//! visible placeholders, so a message can never hide or reorder text in the UI.

use std::fmt::Write as _;

use b2c_ir::diag::{DiagSource, Diagnostic, Location};
use b2c_ir::text::is_invisible;
use b2c_ir::types::Type;

/// Longest user text shown in a message, in characters.
const MAX_SHOWN_CHARS: usize = 40;

/// User text made safe for a message, without quotes.
pub(crate) fn shown(text: &str) -> String {
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        if i == MAX_SHOWN_CHARS {
            out.push('…');
            break;
        }
        if c.is_control() || is_invisible(c) || c == '`' {
            let _ = write!(out, "<U+{:04X}>", u32::from(c));
        } else {
            out.push(c);
        }
    }
    out
}

/// A user name for a message, in backticks (e.g. `` `score` ``).
pub(crate) fn quoted(name: &str) -> String {
    if name.is_empty() {
        String::from("(unnamed)")
    } else {
        format!("`{}`", shown(name))
    }
}

/// A type with its article, for messages: "a whole number", "text", …
pub(crate) fn a_type(ty: &Type) -> &'static str {
    match ty {
        Type::Void => "nothing",
        Type::Bool => "a true/false value",
        Type::Char => "a character",
        Type::Int => "a whole number",
        Type::Double => "a decimal number",
        Type::String => "text",
        Type::Error => "an unknown value",
    }
}

/// Collects diagnostics in the order they are found.
#[derive(Debug, Default)]
pub(crate) struct Diags {
    /// The diagnostics so far.
    pub(crate) list: Vec<Diagnostic>,
}

impl Diags {
    /// Reports an error.
    pub(crate) fn error(&mut self, code: &str, location: Location, message: impl Into<String>) {
        self.list
            .push(Diagnostic::error(code, DiagSource::Analyser, location, message));
    }

    /// Reports a warning.
    pub(crate) fn warning(&mut self, code: &str, location: Location, message: impl Into<String>) {
        self.list
            .push(Diagnostic::warning(code, DiagSource::Analyser, location, message));
    }

    /// Reports an informational diagnostic.
    pub(crate) fn info(&mut self, code: &str, location: Location, message: impl Into<String>) {
        self.list
            .push(Diagnostic::info(code, DiagSource::Analyser, location, message));
    }

    /// Reports an already built diagnostic.
    pub(crate) fn push(&mut self, diagnostic: Diagnostic) {
        self.list.push(diagnostic);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_safe() {
        assert_eq!(quoted("score"), "`score`");
        assert_eq!(quoted(""), "(unnamed)");
        assert_eq!(shown("a\u{202E}b"), "a<U+202E>b");
        assert_eq!(shown("a\nb`c"), "a<U+000A>b<U+0060>c");
        let long = "x".repeat(100);
        let s = shown(&long);
        assert_eq!(s.chars().count(), MAX_SHOWN_CHARS + 1);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn articles() {
        assert_eq!(a_type(&Type::Int), "a whole number");
        assert_eq!(a_type(&Type::String), "text");
        assert_eq!(a_type(&Type::Void), "nothing");
        assert_eq!(a_type(&Type::Error), "an unknown value");
        assert_eq!(a_type(&Type::Bool), "a true/false value");
        assert_eq!(a_type(&Type::Char), "a character");
        assert_eq!(a_type(&Type::Double), "a decimal number");
    }
}
