//! GCC's JSON diagnostics (`-fdiagnostics-format=json`, GCC 10–14).
//!
//! GCC prints one JSON array per compiler run on standard error, at the
//! end; the driver's and linker's messages around it stay plain text.

use serde::Deserialize;

use super::{
    CompilerMessage, MAX_INPUT_BYTES, MessageOrigin, MessageSeverity, ParseError, ParsedOutput, SourcePos,
    clean_message, lossy, normalise_option, parse_text,
};

#[derive(Deserialize)]
struct JsonDiagnostic {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    option: Option<String>,
    #[serde(default)]
    locations: Vec<JsonLocation>,
    #[serde(default)]
    children: Vec<JsonDiagnostic>,
}

#[derive(Deserialize)]
struct JsonLocation {
    #[serde(default)]
    caret: Option<JsonPoint>,
}

#[derive(Deserialize)]
struct JsonPoint {
    #[serde(default)]
    file: String,
    #[serde(default)]
    line: u64,
    #[serde(default)]
    column: Option<u64>,
}

/// Parses one GCC JSON diagnostics array.
///
/// ```
/// use b2c_toolchain::diagnostics::{MessageSeverity, parse_gcc_json};
///
/// let json = br#"[{"kind": "error", "message": "'x' was not declared in this scope",
///   "children": [], "locations": [{"caret": {"file": "main.cpp", "line": 3, "column": 5}}]}]"#;
/// let parsed = parse_gcc_json(json)?;
/// assert_eq!(parsed.messages[0].severity, MessageSeverity::Error);
/// assert_eq!(parsed.messages[0].location.as_ref().unwrap().line, 3);
/// # Ok::<(), b2c_toolchain::diagnostics::ParseError>(())
/// ```
///
/// # Errors
/// [`ParseError`] if the input is not a JSON array of GCC diagnostics, is
/// nested more deeply than `serde_json` allows, or is larger than
/// [`MAX_INPUT_BYTES`].
pub fn parse_gcc_json(bytes: &[u8]) -> Result<ParsedOutput, ParseError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(ParseError::TooLarge);
    }
    let diagnostics: Vec<JsonDiagnostic> =
        serde_json::from_slice(bytes).map_err(|error| ParseError::Malformed {
            format: "JSON",
            reason: error.to_string(),
        })?;
    let mut out = ParsedOutput::default();
    for diagnostic in diagnostics {
        let (message, truncated) = convert(diagnostic);
        out.truncated |= truncated;
        out.push(message);
    }
    Ok(out)
}

/// Parses standard error of a JSON-format run: each line that starts a JSON
/// array is parsed as GCC JSON, everything else as text (driver and linker
/// messages). A line that looks like JSON but does not parse is read as
/// text too.
pub(crate) fn parse_json_stderr(stderr: &[u8]) -> ParsedOutput {
    let text = lossy(stderr);
    let mut out = ParsedOutput {
        truncated: stderr.len() > MAX_INPUT_BYTES,
        ..ParsedOutput::default()
    };
    let mut plain = String::new();
    let mut rest = text.as_str();
    while !rest.is_empty() {
        let (line, after) = match rest.find('\n') {
            Some(end) => (
                rest.get(..end).unwrap_or_default(),
                rest.get(end + 1..).unwrap_or_default(),
            ),
            None => (rest, ""),
        };
        if line.trim_start().starts_with('[') {
            // The array may span several lines; let serde find its end.
            let start = rest.len() - rest.trim_start().len();
            let mut stream = serde_json::Deserializer::from_str(rest.get(start..).unwrap_or_default())
                .into_iter::<Vec<JsonDiagnostic>>();
            if let Some(Ok(diagnostics)) = stream.next() {
                let consumed = start + stream.byte_offset();
                for diagnostic in diagnostics {
                    let (message, truncated) = convert(diagnostic);
                    out.truncated |= truncated;
                    out.push(message);
                }
                rest = rest.get(consumed..).unwrap_or_default();
                continue;
            }
        }
        plain.push_str(line);
        plain.push('\n');
        rest = after;
    }
    let parsed = parse_text(&plain);
    out.truncated |= parsed.truncated;
    for message in parsed.messages {
        out.push(message);
    }
    out
}

/// Converts one diagnostic and its children. Returns whether children were
/// dropped.
fn convert(diagnostic: JsonDiagnostic) -> (CompilerMessage, bool) {
    let severity = severity(&diagnostic.kind);
    let mut message = CompilerMessage::new(MessageOrigin::Compiler, severity, &diagnostic.message);
    message.option = diagnostic.option.as_deref().map(normalise_option);
    message.location = diagnostic
        .locations
        .into_iter()
        .find_map(|location| location.caret)
        .and_then(|point| position(&point));
    let mut truncated = false;
    for child in diagnostic.children {
        let (child, child_truncated) = convert(child);
        truncated |= child_truncated;
        if !message.push_child(child) {
            truncated = true;
            break;
        }
    }
    (message, truncated)
}

fn severity(kind: &str) -> MessageSeverity {
    MessageSeverity::from_gcc(kind).unwrap_or(if kind.contains("error") {
        MessageSeverity::Error
    } else if kind.contains("warning") {
        MessageSeverity::Warning
    } else {
        MessageSeverity::Note
    })
}

fn position(point: &JsonPoint) -> Option<SourcePos> {
    let line = u32::try_from(point.line).ok().filter(|&line| line > 0)?;
    if point.file.is_empty() {
        return None;
    }
    Some(SourcePos {
        file: clean_message(&point.file),
        line,
        column: point
            .column
            .and_then(|column| u32::try_from(column).ok())
            .filter(|&column| column > 0),
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn children_become_notes() {
        let json = br#"[{"kind": "warning", "message": "declaration of 'int total' shadows a parameter",
            "option": "-Wshadow", "children": [{"kind": "note", "message": "shadowed declaration is here",
            "locations": [{"caret": {"file": "a.cpp", "line": 3, "column": 16}}]}],
            "locations": [{"caret": {"file": "a.cpp", "line": 6, "column": 13}}]}]"#;
        let parsed = parse_gcc_json(json).unwrap();
        let message = &parsed.messages[0];
        assert_eq!(message.option.as_deref(), Some("-Wshadow"));
        assert_eq!(message.children[0].severity, MessageSeverity::Note);
        assert_eq!(message.children[0].location.as_ref().map(|p| p.line), Some(3));
    }

    #[test]
    fn mixed_stderr_keeps_linker_text() {
        let stderr = b"[]\n/usr/bin/ld: x.o: in function `main':\nx.cpp:(.text+0x1d): undefined reference to `f()'\ncollect2: error: ld returned 1 exit status\n";
        let parsed = parse_json_stderr(stderr);
        assert_eq!(parsed.messages.len(), 2);
        assert_eq!(parsed.messages[0].symbol.as_deref(), Some("f()"));
    }

    #[test]
    fn malformed_json_is_an_error_or_text() {
        assert!(parse_gcc_json(b"[{").is_err());
        assert!(parse_gcc_json(b"{}").is_err());
        let parsed = parse_json_stderr(b"[not json\na.cpp:1:1: error: e\n");
        assert_eq!(parsed.messages.len(), 1);
    }

    #[test]
    fn deep_nesting_is_rejected_not_overflowed() {
        let mut json = String::from("[");
        for _ in 0..10_000 {
            json.push_str(r#"{"kind":"note","message":"m","children":["#);
        }
        assert!(parse_gcc_json(json.as_bytes()).is_err());
        let _ = parse_json_stderr(json.as_bytes());
    }

    #[test]
    fn bad_positions_are_dropped() {
        let json = br#"[{"kind": "error", "message": "m", "locations": [{"caret": {"file": "", "line": 3}}]},
            {"kind": "error", "message": "m", "locations": [{"caret": {"file": "a", "line": 0}}]},
            {"kind": "odd error kind", "message": "m"}]"#;
        let parsed = parse_gcc_json(json).unwrap();
        assert!(parsed.messages.iter().all(|m| m.location.is_none()));
        assert_eq!(parsed.messages[2].severity, MessageSeverity::Error);
    }

    proptest! {
        #[test]
        fn never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
            let _ = parse_gcc_json(&bytes);
            let _ = parse_json_stderr(&bytes);
        }

        #[test]
        fn never_panics_on_json_like_input(text in "[\\[\\]{}\":,a-z0-9 \\n-]{0,300}") {
            let _ = parse_gcc_json(text.as_bytes());
            let _ = parse_json_stderr(text.as_bytes());
        }
    }
}
