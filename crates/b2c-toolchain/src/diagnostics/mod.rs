//! Parsing what g++ and the linker report (spec §7.5.3, step 1).
//!
//! Three formats are understood, matching the format ladder that probing
//! picks ([`crate::probe::DiagnosticsFormat`]):
//!
//! * **SARIF 2.1** ([`parse_sarif`]): the file GCC 13+ writes with
//!   `-fdiagnostics-format=sarif-file` or (GCC 15+)
//!   `-fdiagnostics-add-output=sarif:…`;
//! * **GCC JSON** ([`parse_gcc_json`]): the array GCC 10–14 prints on
//!   standard error with `-fdiagnostics-format=json`;
//! * **plain text** ([`parse_text`]): `file:line:col: severity: message
//!   [-Woption]` with `note:` lines, `In function …` and `In file included
//!   from …` context, template `required from here` lines, and the linker's
//!   messages (`undefined reference to …`, `collect2: error: …`).
//!
//! [`parse_output`] combines them for one compiler run. Every parser is
//! strict about sizes ([`MAX_INPUT_BYTES`], [`MAX_MESSAGES`],
//! [`MAX_CHILDREN`], [`MAX_MESSAGE_CHARS`]) and never panics, whatever the
//! input: the compiler's output is treated as untrusted (it quotes the
//! user's code).
//!
//! ```
//! use b2c_toolchain::diagnostics::{MessageSeverity, parse_text};
//!
//! let parsed = parse_text(
//!     "main.cpp: In function 'int main()':\n\
//!      main.cpp:5:9: warning: unused variable 'x' [-Wunused-variable]\n",
//! );
//! let message = &parsed.messages[0];
//! assert_eq!(message.severity, MessageSeverity::Warning);
//! assert_eq!(message.message, "unused variable 'x'");
//! assert_eq!(message.option.as_deref(), Some("-Wunused-variable"));
//! assert_eq!(message.function.as_deref(), Some("int main()"));
//! let location = message.location.as_ref().unwrap();
//! assert_eq!((location.file.as_str(), location.line, location.column), ("main.cpp", 5, Some(9)));
//! ```

mod json;
mod sarif;
mod text;

use serde::{Deserialize, Serialize};

pub use json::parse_gcc_json;
pub use sarif::parse_sarif;
pub use text::parse_text;

use crate::probe::DiagnosticsFormat;

/// Largest input any parser looks at (64 MiB); longer input is cut off and
/// the result flagged as truncated.
pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
/// Most top-level messages kept from one run.
pub const MAX_MESSAGES: usize = 2000;
/// Most notes kept under one message.
pub const MAX_CHILDREN: usize = 200;
/// Longest message text kept, in characters (longer text ends in `…`).
pub const MAX_MESSAGE_CHARS: usize = 4096;
/// Longest include chain kept for one message.
pub const MAX_INCLUDE_DEPTH: usize = 64;

/// Which program reported a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageOrigin {
    /// The compiler proper (`cc1plus`): almost everything.
    Compiler,
    /// The `g++` driver itself (`g++: fatal error: …`).
    Driver,
    /// The linker (`ld`, `collect2`).
    Linker,
}

/// How serious a compiler message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSeverity {
    /// `fatal error`: compilation stopped.
    Fatal,
    /// `error` (also `sorry, unimplemented`).
    Error,
    /// `warning`.
    Warning,
    /// `note`, or a context line such as `required from here`.
    Note,
    /// `internal compiler error`: a bug in GCC.
    InternalError,
}

impl MessageSeverity {
    /// Whether the message makes the build fail.
    pub fn is_error(self) -> bool {
        matches!(self, Self::Fatal | Self::Error | Self::InternalError)
    }

    /// Reads a GCC severity word (`error`, `fatal error`, …).
    pub(crate) fn from_gcc(word: &str) -> Option<Self> {
        Some(match word {
            "error" | "sorry, unimplemented" => Self::Error,
            "fatal error" => Self::Fatal,
            "warning" => Self::Warning,
            "note" => Self::Note,
            "internal compiler error" | "ice" => Self::InternalError,
            _ => return None,
        })
    }
}

/// A position in a source file, as the compiler reported it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourcePos {
    /// The file exactly as the compiler printed it (relative to its working
    /// directory, or absolute); SARIF URIs are percent-decoded and a
    /// `file://` prefix is removed.
    pub file: String,
    /// 1-based line.
    pub line: u32,
    /// 1-based column, when reported.
    pub column: Option<u32>,
}

/// One message from the compiler or linker, with its notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerMessage {
    /// Which program reported it.
    pub origin: MessageOrigin,
    /// How serious it is.
    pub severity: MessageSeverity,
    /// The message text, without location, severity or option. Typographic
    /// quotes (`‘’`) are normalised to `'`.
    pub message: String,
    /// Where it points, if anywhere.
    pub location: Option<SourcePos>,
    /// The option that controls it, e.g. `-Wunused-variable`. `-Werror=x`
    /// is normalised to `-Wx`.
    pub option: Option<String>,
    /// The enclosing function, when the compiler said (`In function 'int
    /// main()'`) or the linker said (`in function 'main'`).
    pub function: Option<String>,
    /// The symbol a linker message is about (`undefined reference to
    /// 'twice(int)'` gives `twice(int)`).
    pub symbol: Option<String>,
    /// `In file included from` chain, innermost first.
    pub included_from: Vec<SourcePos>,
    /// Notes and context (`required from here`, candidates, …).
    pub children: Vec<CompilerMessage>,
}

impl CompilerMessage {
    /// A message with no location, notes or context.
    pub fn new(origin: MessageOrigin, severity: MessageSeverity, message: &str) -> Self {
        Self {
            origin,
            severity,
            message: clean_message(message),
            location: None,
            option: None,
            function: None,
            symbol: None,
            included_from: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Adds a note, respecting [`MAX_CHILDREN`]. Returns `false` if it was
    /// dropped.
    pub(crate) fn push_child(&mut self, child: Self) -> bool {
        if self.children.len() >= MAX_CHILDREN {
            return false;
        }
        self.children.push(child);
        true
    }
}

/// The messages from one compiler run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedOutput {
    /// The messages, in the order reported.
    pub messages: Vec<CompilerMessage>,
    /// Whether anything was left out because of a size limit.
    pub truncated: bool,
}

impl ParsedOutput {
    /// Whether any message is an error.
    pub fn has_errors(&self) -> bool {
        self.messages.iter().any(|m| m.severity.is_error())
    }

    /// Adds a top-level message, respecting [`MAX_MESSAGES`].
    pub(crate) fn push(&mut self, message: CompilerMessage) {
        if self.messages.len() >= MAX_MESSAGES {
            self.truncated = true;
        } else {
            self.messages.push(message);
        }
    }

    fn extend(&mut self, other: Self) {
        self.truncated |= other.truncated;
        for message in other.messages {
            self.push(message);
        }
    }
}

/// Why structured diagnostics could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// The input is not valid for the format.
    #[error("the compiler's {format} diagnostics could not be read: {reason}")]
    Malformed {
        /// `SARIF` or `JSON`.
        format: &'static str,
        /// What was wrong.
        reason: String,
    },
    /// The input is larger than [`MAX_INPUT_BYTES`].
    #[error("the compiler's diagnostics are larger than {MAX_INPUT_BYTES} bytes")]
    TooLarge,
}

/// Parses everything one compiler run produced: its standard error (text,
/// or GCC JSON mixed with text) and, for the SARIF formats, the SARIF file
/// (`None` if it was not written).
///
/// When the SARIF file parses, compiler messages come from it and only the
/// driver's and linker's lines are taken from standard error (GCC 15 also
/// prints the compiler's messages there as text). When it is missing or
/// malformed, standard error is parsed as text instead.
pub fn parse_output(format: DiagnosticsFormat, stderr: &[u8], sarif: Option<&[u8]>) -> ParsedOutput {
    let mut out = ParsedOutput::default();
    match format {
        DiagnosticsFormat::AddOutputSarif | DiagnosticsFormat::SarifFile => match sarif.map(parse_sarif) {
            Some(Ok(parsed)) => {
                out.extend(parsed);
                let text = parse_text(&lossy(stderr));
                out.truncated |= text.truncated;
                for message in text.messages {
                    if message.origin != MessageOrigin::Compiler {
                        out.push(message);
                    }
                }
            }
            Some(Err(_)) | None => out.extend(parse_text(&lossy(stderr))),
        },
        DiagnosticsFormat::Json => out.extend(json::parse_json_stderr(stderr)),
        DiagnosticsFormat::Plain => out.extend(parse_text(&lossy(stderr))),
    }
    out
}

/// Decodes compiler output, replacing invalid UTF-8 and cutting it at
/// [`MAX_INPUT_BYTES`].
pub(crate) fn lossy(bytes: &[u8]) -> String {
    let limited = bytes.get(..MAX_INPUT_BYTES).unwrap_or(bytes);
    String::from_utf8_lossy(limited).into_owned()
}

/// Normalises message text: typographic quotes become `'`, control
/// characters other than tab become spaces, surrounding whitespace is
/// trimmed and the length is capped at [`MAX_MESSAGE_CHARS`].
pub(crate) fn clean_message(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(MAX_MESSAGE_CHARS * 4));
    for (count, c) in text.trim().chars().enumerate() {
        if count >= MAX_MESSAGE_CHARS {
            out.push('…');
            break;
        }
        out.push(match c {
            '\u{2018}' | '\u{2019}' => '\'',
            '\t' => '\t',
            c if c.is_control() => ' ',
            c => c,
        });
    }
    out
}

/// Splits a trailing `[-Woption]` off a message. Returns the message and the
/// normalised option.
pub(crate) fn split_option(message: &str) -> (&str, Option<String>) {
    let trimmed = message.trim_end();
    if let Some(body) = trimmed.strip_suffix(']')
        && let Some((text, option)) = body.rsplit_once(" [")
        && option.starts_with('-')
        && !option.contains(char::is_whitespace)
    {
        return (text, Some(normalise_option(option)));
    }
    (trimmed, None)
}

/// `-Werror=unused-variable` becomes `-Wunused-variable`.
pub(crate) fn normalise_option(option: &str) -> String {
    match option.strip_prefix("-Werror=") {
        Some(rest) => format!("-W{rest}"),
        None => option.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_are_split_and_normalised() {
        assert_eq!(
            split_option("unused variable 'x' [-Wunused-variable]"),
            ("unused variable 'x'", Some(String::from("-Wunused-variable")))
        );
        assert_eq!(
            split_option("unused variable 'x' [-Werror=unused-variable]"),
            ("unused variable 'x'", Some(String::from("-Wunused-variable")))
        );
        assert_eq!(
            split_option("x [enabled by default]"),
            ("x [enabled by default]", None)
        );
        assert_eq!(split_option("a[i]"), ("a[i]", None));
        assert_eq!(split_option(""), ("", None));
    }

    #[test]
    fn messages_are_cleaned() {
        assert_eq!(
            clean_message("  \u{2018}x\u{2019} is\u{1b}[31m bad "),
            "'x' is [31m bad"
        );
        let long = "x".repeat(MAX_MESSAGE_CHARS + 10);
        let cleaned = clean_message(&long);
        assert_eq!(cleaned.chars().count(), MAX_MESSAGE_CHARS + 1);
        assert!(cleaned.ends_with('…'));
    }
}
