//! SARIF 2.1 diagnostics as GCC 13+ writes them.
//!
//! Only the parts Blocks2Cpp needs are modelled; everything else (artifact
//! contents, which GCC fills with whole source files, rules, invocations) is
//! skipped without being stored.

use serde::Deserialize;

use super::{
    CompilerMessage, MAX_INPUT_BYTES, MessageOrigin, MessageSeverity, ParseError, ParsedOutput, SourcePos,
    clean_message, normalise_option,
};

#[derive(Deserialize)]
struct Log {
    /// `2.1.0` from GCC; required, so other JSON is not mistaken for SARIF.
    version: String,
    #[serde(default)]
    runs: Vec<Run>,
}

#[derive(Deserialize)]
struct Run {
    #[serde(default)]
    results: Vec<SarifResult>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SarifResult {
    #[serde(default)]
    rule_id: Option<String>,
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    message: Message,
    #[serde(default)]
    locations: Vec<Location>,
    #[serde(default)]
    related_locations: Vec<Location>,
}

#[derive(Deserialize, Default)]
struct Message {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_field_names)] // field names mirror the SARIF 2.1 schema
struct Location {
    #[serde(default)]
    physical_location: Option<PhysicalLocation>,
    #[serde(default)]
    logical_locations: Vec<LogicalLocation>,
    #[serde(default)]
    message: Option<Message>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PhysicalLocation {
    #[serde(default)]
    artifact_location: Option<ArtifactLocation>,
    #[serde(default)]
    region: Option<Region>,
}

#[derive(Deserialize)]
struct ArtifactLocation {
    #[serde(default)]
    uri: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Region {
    #[serde(default)]
    start_line: Option<u64>,
    #[serde(default)]
    start_column: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LogicalLocation {
    #[serde(default)]
    fully_qualified_name: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    kind: Option<String>,
}

/// Parses a SARIF 2.1 log written by GCC.
///
/// Each result becomes a message: `level` gives the severity (`ruleId`
/// `fatal error` and `error` without a level are errors), a `ruleId` that
/// starts with `-` is the option, the first location is the position (with
/// the enclosing function from its logical location) and each related
/// location becomes a note.
///
/// ```
/// use b2c_toolchain::diagnostics::{MessageSeverity, parse_sarif};
///
/// let sarif = br#"{"version": "2.1.0", "runs": [{"results": [{
///   "ruleId": "-Wunused-variable", "level": "warning",
///   "message": {"text": "unused variable 'x'"},
///   "locations": [{"physicalLocation": {"artifactLocation": {"uri": "main.cpp"},
///                  "region": {"startLine": 4, "startColumn": 9}}}]}]}]}"#;
/// let parsed = parse_sarif(sarif)?;
/// let message = &parsed.messages[0];
/// assert_eq!(message.severity, MessageSeverity::Warning);
/// assert_eq!(message.option.as_deref(), Some("-Wunused-variable"));
/// assert_eq!(message.location.as_ref().unwrap().column, Some(9));
/// # Ok::<(), b2c_toolchain::diagnostics::ParseError>(())
/// ```
///
/// # Errors
/// [`ParseError`] if the input is not a SARIF log or is larger than
/// [`MAX_INPUT_BYTES`].
pub fn parse_sarif(bytes: &[u8]) -> Result<ParsedOutput, ParseError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(ParseError::TooLarge);
    }
    let log: Log = serde_json::from_slice(bytes).map_err(|error| ParseError::Malformed {
        format: "SARIF",
        reason: error.to_string(),
    })?;
    if !log.version.starts_with("2.") {
        return Err(ParseError::Malformed {
            format: "SARIF",
            reason: format!("unsupported SARIF version {:?}", clean_message(&log.version)),
        });
    }
    let mut out = ParsedOutput::default();
    for result in log.runs.into_iter().flat_map(|run| run.results) {
        let message = convert(result, &mut out.truncated);
        out.push(message);
    }
    Ok(out)
}

fn convert(result: SarifResult, truncated: &mut bool) -> CompilerMessage {
    let rule = result.rule_id.unwrap_or_default();
    let severity = match result.level.as_deref() {
        Some("error") => MessageSeverity::Error,
        Some("warning") => MessageSeverity::Warning,
        Some("note" | "none") => MessageSeverity::Note,
        _ => MessageSeverity::from_gcc(&rule).unwrap_or(MessageSeverity::Warning),
    };
    let severity = match rule.as_str() {
        "fatal error" => MessageSeverity::Fatal,
        "internal compiler error" => MessageSeverity::InternalError,
        _ => severity,
    };
    let mut message = CompilerMessage::new(
        MessageOrigin::Compiler,
        severity,
        result.message.text.as_deref().unwrap_or_default(),
    );
    if rule.starts_with('-') {
        message.option = Some(normalise_option(&rule));
    }
    if let Some(location) = result.locations.into_iter().next() {
        message.function = function_name(&location.logical_locations);
        message.location = location.physical_location.and_then(position);
    }
    for related in result.related_locations {
        let text = related.message.and_then(|m| m.text).unwrap_or_default();
        let mut note = CompilerMessage::new(MessageOrigin::Compiler, MessageSeverity::Note, &text);
        note.location = related.physical_location.and_then(position);
        if !message.push_child(note) {
            *truncated = true;
            break;
        }
    }
    message
}

fn function_name(locations: &[LogicalLocation]) -> Option<String> {
    let location = locations
        .iter()
        .find(|l| l.kind.as_deref() == Some("function"))
        .or_else(|| locations.first())?;
    location
        .fully_qualified_name
        .as_deref()
        .or(location.name.as_deref())
        .map(clean_message)
}

fn position(physical: PhysicalLocation) -> Option<SourcePos> {
    let uri = physical.artifact_location?.uri?;
    let region = physical.region?;
    let line = u32::try_from(region.start_line?).ok().filter(|&line| line > 0)?;
    let file = uri_to_path(&uri);
    if file.is_empty() {
        return None;
    }
    Some(SourcePos {
        file,
        line,
        column: region
            .start_column
            .and_then(|column| u32::try_from(column).ok())
            .filter(|&column| column > 0),
    })
}

/// Turns a SARIF URI reference into a path as the compiler would print it:
/// `file://` (and the extra `/` before a Windows drive) removed and `%XX`
/// escapes decoded. Invalid escapes are kept as they are.
fn uri_to_path(uri: &str) -> String {
    let path = match uri.strip_prefix("file://") {
        Some(rest) => {
            let rest = rest.strip_prefix("localhost").unwrap_or(rest);
            // `file:///C:/x` → `C:/x`.
            match rest.strip_prefix('/') {
                Some(after) if after.as_bytes().get(1) == Some(&b':') => after,
                _ => rest,
            }
        }
        None => uri,
    };
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%'
            && let Some(high) = bytes.get(index + 1).and_then(|&b| hex(b))
            && let Some(low) = bytes.get(index + 2).and_then(|&b| hex(b))
        {
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(byte);
            index += 1;
        }
    }
    clean_message(&String::from_utf8_lossy(&decoded))
}

fn hex(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .and_then(|digit| u8::try_from(digit).ok())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn uris_decode() {
        assert_eq!(uri_to_path("src/main.cpp"), "src/main.cpp");
        assert_eq!(uri_to_path("my%20project/main.cpp"), "my project/main.cpp");
        assert_eq!(uri_to_path("file:///home/ada/a.cpp"), "/home/ada/a.cpp");
        assert_eq!(uri_to_path("file:///C:/Users/ada/a.cpp"), "C:/Users/ada/a.cpp");
        assert_eq!(uri_to_path("bad%zzescape%2"), "bad%zzescape%2");
        assert_eq!(uri_to_path("%C3%A9t%C3%A9.cpp"), "été.cpp");
    }

    #[test]
    fn fatal_errors_without_level_are_fatal() {
        let sarif = br#"{"version": "2.1.0", "runs": [{"results": [{"ruleId": "fatal error",
            "message": {"text": "missing.hpp: No such file or directory"}}]}]}"#;
        let parsed = parse_sarif(sarif).unwrap();
        assert_eq!(parsed.messages[0].severity, MessageSeverity::Fatal);
        assert!(parsed.messages[0].location.is_none());
    }

    #[test]
    fn werror_rules_are_errors_with_normalised_options() {
        let sarif = br#"{"version": "2.1.0", "runs": [{"results": [{"ruleId": "-Werror=unused-variable", "level": "error",
            "message": {"text": "unused variable 'x'"}}]}]}"#;
        let message = &parse_sarif(sarif).unwrap().messages[0];
        assert_eq!(message.severity, MessageSeverity::Error);
        assert_eq!(message.option.as_deref(), Some("-Wunused-variable"));
    }

    #[test]
    fn empty_and_malformed_logs() {
        assert!(
            parse_sarif(br#"{"version": "2.1.0", "runs": []}"#)
                .unwrap()
                .messages
                .is_empty()
        );
        assert!(parse_sarif(br#"{"runs": []}"#).is_err());
        assert!(parse_sarif(br#"{"version": "1.0", "runs": []}"#).is_err());
        assert!(parse_sarif(b"").is_err());
        assert!(parse_sarif(b"[]").is_err());
        assert!(parse_sarif(br#"{"version": "2.1.0", "runs": [{"results": [{"locations": 5}]}]}"#).is_err());
    }

    proptest! {
        #[test]
        fn never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
            let _ = parse_sarif(&bytes);
        }

        #[test]
        fn never_panics_on_sarif_like_input(text in "[\\[\\]{}\":,a-zA-Z0-9 %-]{0,300}") {
            let _ = parse_sarif(text.as_bytes());
            let _ = uri_to_path(&text);
        }
    }
}
