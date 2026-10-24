//! Preview options: the machine's code-style settings that change the
//! generated text (spec gap decision "Code style in M2": indent width 2 or 4
//! only; tabs and brace styles come later).
//!
//! The options arrive as JSON text from the editor and are parsed strictly:
//! exactly one key, `indentWidth`, with the integer 2 or 4. Unknown keys,
//! duplicate keys, other values and oversized input are refused.

use serde::Deserialize;

use crate::error::FacadeError;

/// The longest options text accepted, in bytes. The real options are about
/// twenty bytes; the limit only keeps a broken caller from making the
/// facade parse something large.
pub const MAX_OPTIONS_BYTES: usize = 1024;

/// Spaces per indentation level in generated C++.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndentWidth {
    /// Two spaces.
    Two,
    /// Four spaces (the default code style, and what the CLI always uses).
    #[default]
    Four,
}

impl IndentWidth {
    /// The number of spaces.
    pub fn spaces(self) -> u8 {
        match self {
            Self::Two => 2,
            Self::Four => 4,
        }
    }

    /// The width for a number of spaces, if it is supported.
    pub fn from_spaces(spaces: u64) -> Option<Self> {
        match spaces {
            2 => Some(Self::Two),
            4 => Some(Self::Four),
            _ => None,
        }
    }
}

/// Options for [`crate::preview_document`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreviewOptions {
    /// Spaces per indentation level.
    pub indent_width: IndentWidth,
}

/// The wire form, `{"indentWidth": 2 | 4}`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireOptions {
    indent_width: u64,
}

const EXPECTED: &str = "expected {\"indentWidth\": 2} or {\"indentWidth\": 4}";

impl PreviewOptions {
    /// Parses the options JSON sent by the editor.
    ///
    /// # Errors
    /// [`FacadeError::InvalidOptions`] when the text is longer than
    /// [`MAX_OPTIONS_BYTES`], is not a JSON object with exactly the key
    /// `indentWidth` (no unknown or repeated keys), or the width is not the
    /// integer 2 or 4. The message describes what was expected and where
    /// parsing stopped, without repeating the input.
    pub fn from_json(text: &str) -> Result<Self, FacadeError> {
        if text.len() > MAX_OPTIONS_BYTES {
            return Err(FacadeError::InvalidOptions(format!(
                "{EXPECTED}, but the options are longer than {MAX_OPTIONS_BYTES} bytes"
            )));
        }
        let wire: WireOptions = serde_json::from_str(text).map_err(|error| {
            FacadeError::InvalidOptions(format!(
                "{EXPECTED} (stopped at line {}, column {})",
                error.line(),
                error.column()
            ))
        })?;
        let indent_width = IndentWidth::from_spaces(wire.indent_width)
            .ok_or_else(|| FacadeError::InvalidOptions(format!("{EXPECTED}; indentWidth must be 2 or 4")))?;
        Ok(Self { indent_width })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid(text: &str) -> String {
        match PreviewOptions::from_json(text) {
            Err(FacadeError::InvalidOptions(message)) => message,
            other => panic!("{text:?} should be refused, got {other:?}"),
        }
    }

    #[test]
    fn accepts_two_and_four() {
        let two = PreviewOptions::from_json(r#"{"indentWidth": 2}"#).unwrap();
        assert_eq!(two.indent_width, IndentWidth::Two);
        assert_eq!(two.indent_width.spaces(), 2);
        let four = PreviewOptions::from_json(" {\"indentWidth\":4}\n").unwrap();
        assert_eq!(four.indent_width, IndentWidth::Four);
        assert_eq!(four.indent_width.spaces(), 4);
        assert_eq!(PreviewOptions::default().indent_width, IndentWidth::Four);
    }

    #[test]
    fn refuses_everything_else() {
        for text in [
            "",
            "null",
            "[]",
            "{}",
            r#"{"indentWidth": 3}"#,
            r#"{"indentWidth": 0}"#,
            r#"{"indentWidth": -4}"#,
            r#"{"indentWidth": 4.0}"#,
            r#"{"indentWidth": "4"}"#,
            r#"{"indentWidth": 18446744073709551620}"#,
            r#"{"indentWidth": 4, "indentWidth": 2}"#,
            r#"{"indentWidth": 4, "tabs": true}"#,
            r#"{"indentWidth": 4, "__proto__": {}}"#,
            r#"{"indentWidth": 4} trailing"#,
            r#"{"indent_width": 4}"#,
        ] {
            let message = invalid(text);
            assert!(message.starts_with("expected"), "{text:?}: {message}");
        }
    }

    #[test]
    fn never_repeats_the_input() {
        let message = invalid("{\"\u{202e}<script>\": 4}");
        assert!(!message.contains("script"), "{message}");
        assert!(!message.contains('\u{202e}'), "{message}");
    }

    #[test]
    fn refuses_oversized_options_before_parsing() {
        let text = format!(r#"{{"indentWidth": 4{}}}"#, " ".repeat(MAX_OPTIONS_BYTES));
        assert!(invalid(&text).contains("longer than 1024 bytes"));
        let at_limit = format!(r#"{{"indentWidth": 4{}}}"#, " ".repeat(MAX_OPTIONS_BYTES - 18));
        assert_eq!(at_limit.len(), MAX_OPTIONS_BYTES);
        assert!(PreviewOptions::from_json(&at_limit).is_ok());
    }

    #[test]
    fn widths_from_spaces() {
        assert_eq!(IndentWidth::from_spaces(2), Some(IndentWidth::Two));
        assert_eq!(IndentWidth::from_spaces(4), Some(IndentWidth::Four));
        for spaces in [0, 1, 3, 8, u64::MAX] {
            assert_eq!(IndentWidth::from_spaces(spaces), None);
        }
    }
}
