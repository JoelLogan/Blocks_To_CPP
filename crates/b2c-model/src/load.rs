//! Loading untrusted project bytes (spec §5.6, §5.7, §6.2).
//!
//! CONTRACT (implemented in milestone M1):
//! * Reject input larger than [`crate::limits::MAX_FILE_BYTES`] before parsing.
//! * Parse JSON rejecting duplicate keys anywhere and nesting deeper than
//!   [`crate::limits::MAX_JSON_DEPTH`], then decode into [`crate::Document`]
//!   with unknown keys rejected (except inside `"x-ext"`).
//! * Check `format == FORMAT_TAG`; refuse newer `formatVersion`s with a clear
//!   message; migrate older ones (none exist yet for version 1).
//! * Enforce every limit in [`crate::limits`], unique block/module/symbol IDs,
//!   module-name rules, and the text rules: valid UTF-8, no NUL, no C0 controls
//!   other than tab and newline, no Unicode bidi controls, in every string.
//! * Report problems as `b2c_ir::Diagnostic`s with `DiagSource::Loader` and
//!   codes `B2C-E01xx`, pointing at the block when there is one.
//!
//! The steps, in order (each later step runs only when the earlier ones
//! succeeded, because it would only repeat their complaints):
//!
//! 1. size limit, UTF-8 and byte-order-mark check;
//! 2. JSON parsing ([`crate::json`]): syntax, depth, value count, duplicates;
//! 3. the header: `format` and `formatVersion`, then migrations
//!    ([`crate::migrate`]);
//! 4. decoding with every other rule ([`crate::decode`]), which reports all
//!    problems it finds.

use b2c_ir::{Diagnostic, Location};

use crate::codes::{self, Diags};
use crate::decode::Decoder;
use crate::json::{self, ErrorKind, Json};
use crate::limits::{MAX_FILE_BYTES, MAX_JSON_DEPTH};
use crate::migrate::{self, MigrationError};
use crate::text_rules::quote;
use crate::{CURRENT_FORMAT_VERSION, Document, FORMAT_TAG};

/// Why a project could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the project could not be loaded ({} problem(s))", .diagnostics.len())]
pub struct LoadError {
    /// Every problem found (at least one).
    pub diagnostics: Vec<Diagnostic>,
}

impl LoadError {
    fn from_diags(diags: Diags) -> Self {
        let mut diagnostics = diags.finish();
        if diagnostics.is_empty() {
            // Unreachable by construction; keeps the "at least one" promise.
            diagnostics.push(Diagnostic::error(
                codes::NOT_A_PROJECT,
                b2c_ir::DiagSource::Loader,
                Location::project(),
                "The project could not be loaded.",
            ));
        }
        Self { diagnostics }
    }

    fn single(code: &str, message: String) -> Self {
        let mut diags = Diags::default();
        diags.error(code, Location::project(), message);
        Self::from_diags(diags)
    }
}

/// Parses and validates untrusted project bytes.
///
/// # Errors
/// Returns every problem found when the input is not a valid project.
pub fn load(bytes: &[u8]) -> Result<Document, LoadError> {
    let text = check_bytes(bytes)?;
    let root = parse(text)?;
    let root = check_header(root)?;
    let (document, diags) = Decoder::new(Diags::default()).run(&root);
    match document {
        Some(document) if diags.is_empty() => Ok(document),
        _ => Err(LoadError::from_diags(diags)),
    }
}

/// Step 1: size, UTF-8 and no byte order mark.
fn check_bytes(bytes: &[u8]) -> Result<&str, LoadError> {
    if bytes.len() > MAX_FILE_BYTES {
        // No exact size: callers stop reading one byte past the limit (so a
        // huge file cannot exhaust memory), so `bytes.len()` is usually not
        // the size of the file.
        return Err(LoadError::single(
            codes::FILE_TOO_LARGE,
            format!(
                "The project file is larger than {MAX_FILE_BYTES} bytes (32 MiB), the most a project file can be."
            ),
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|error| {
        let valid = bytes.get(..error.valid_up_to()).unwrap_or_default();
        let prefix = std::str::from_utf8(valid).unwrap_or_default();
        let (line, column) = json::line_columns(prefix, &[prefix.len()])
            .first()
            .copied()
            .unwrap_or((1, 1));
        LoadError::single(
            codes::NOT_UTF8,
            format!(
                "The project file is not valid UTF-8 text (line {line}, column {column}). Save it with the UTF-8 encoding."
            ),
        )
    })?;
    if text.starts_with('\u{feff}') {
        return Err(LoadError::single(
            codes::NOT_UTF8,
            String::from(
                "The project file starts with an invisible byte order mark (BOM). Save it as UTF-8 without a BOM.",
            ),
        ));
    }
    Ok(text)
}

/// Step 2: JSON syntax, depth, size and duplicate keys.
fn parse(text: &str) -> Result<Json, LoadError> {
    let parsed = json::parse(text).map_err(|error| {
        let (line, column) = json::line_columns(text, &[error.offset])
            .first()
            .copied()
            .unwrap_or((1, 1));
        let at = format!("line {line}, column {column}");
        let found = json::describe_at(text, error.offset);
        let (code, message) = match error.kind {
            ErrorKind::TooDeep => (
                codes::TOO_DEEP,
                format!(
                    "Lists and objects in the project file are nested more than {MAX_JSON_DEPTH} levels deep ({at}). Real projects need far fewer levels, so the file was not opened."
                ),
            ),
            ErrorKind::TooManyValues => (
                codes::TOO_MANY_VALUES,
                format!(
                    "The project file holds more than {} values ({at}), far more than any real project, so it was not opened.",
                    json::MAX_JSON_VALUES
                ),
            ),
            kind => (
                codes::JSON_SYNTAX,
                format!("The project file is not valid JSON: {} ({at}).", syntax_problem(kind, &found)),
            ),
        };
        LoadError::single(code, message)
    })?;
    if parsed.duplicates.is_empty() {
        return Ok(parsed.value);
    }
    let offsets: Vec<usize> = parsed.duplicates.iter().map(|d| d.offset).collect();
    let positions = json::line_columns(text, &offsets);
    let mut diags = Diags::default();
    for (duplicate, (line, column)) in parsed.duplicates.iter().zip(positions) {
        diags.error(
            codes::DUPLICATE_KEY,
            Location::project(),
            format!(
                "The key {} appears twice in the same object (line {line}, column {column}). Programs disagree about which value counts, so each key may appear only once.",
                quote(&duplicate.key)
            ),
        );
    }
    diags.add_unlisted(parsed.unrecorded_duplicates);
    Err(LoadError::from_diags(diags))
}

/// What a JSON syntax error means, in plain English.
fn syntax_problem(kind: ErrorKind, found: &str) -> String {
    match kind {
        ErrorKind::Expected(what) => format!("expected {what}, but found {found}"),
        ErrorKind::UnterminatedString => String::from("a piece of text starting here has no closing quote"),
        ErrorKind::ControlInString => format!(
            "text contains a raw control character ({found}); write it as an escape such as \\n or \\t"
        ),
        ErrorKind::InvalidEscape => String::from(
            "text contains an unknown escape; only \\\" \\\\ \\/ \\b \\f \\n \\r \\t and \\uXXXX exist",
        ),
        ErrorKind::InvalidUnicodeEscape => String::from("\\u must be followed by four hexadecimal digits"),
        ErrorKind::LoneSurrogate => {
            String::from("a \\u escape is half of a UTF-16 surrogate pair without its other half")
        }
        ErrorKind::InvalidNumber => String::from("a number is written incorrectly"),
        ErrorKind::NumberOutOfRange => String::from("a number is too large"),
        ErrorKind::TrailingData => format!("there is more after the end of the data (found {found})"),
        ErrorKind::TooDeep | ErrorKind::TooManyValues => String::from("the file is too complex"),
    }
}

/// A short, safe version string from the file (for messages), if it looks
/// like one.
fn saved_by(root: &Json) -> Option<&str> {
    let Some(Json::String(app)) = root.get("generator").and_then(|g| g.get("app")) else {
        return None;
    };
    let plausible = (1..=32).contains(&app.len())
        && app
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'));
    plausible.then_some(&**app)
}

/// Step 3: `format`, `formatVersion` and migrations.
fn check_header(root: Json) -> Result<Json, LoadError> {
    let Json::Object(_) = root else {
        return Err(LoadError::single(
            codes::NOT_A_PROJECT,
            format!(
                "This file is not a Blocks2Cpp project: it should hold a JSON object, but it holds {}.",
                root.kind()
            ),
        ));
    };
    match root.get("format") {
        Some(Json::String(format)) if &**format == FORMAT_TAG => {}
        Some(Json::String(format)) => {
            return Err(LoadError::single(
                codes::NOT_A_PROJECT,
                format!(
                    "This file is not a Blocks2Cpp project: its \"format\" is {}, not \"{FORMAT_TAG}\".",
                    quote(format)
                ),
            ));
        }
        Some(other) => {
            return Err(LoadError::single(
                codes::NOT_A_PROJECT,
                format!(
                    "This file is not a Blocks2Cpp project: its \"format\" should be \"{FORMAT_TAG}\", but it is {}.",
                    other.kind()
                ),
            ));
        }
        None => {
            return Err(LoadError::single(
                codes::NOT_A_PROJECT,
                String::from("This file is not a Blocks2Cpp project: it has no \"format\" key."),
            ));
        }
    }
    let version = match root.get("formatVersion") {
        Some(Json::Number(n)) => n.as_u64(),
        _ => None,
    };
    let Some(version) = version else {
        return Err(LoadError::single(
            codes::BAD_FORMAT_VERSION,
            String::from(
                "The project file has no valid \"formatVersion\" (a whole number such as 1), so it cannot be read.",
            ),
        ));
    };
    // Any whole number above the current version is a newer format, even
    // one beyond 32 bits (spec §5.7).
    if version > u64::from(CURRENT_FORMAT_VERSION) {
        let needs = saved_by(&root)
            .map(|app| format!("needs ≥ {app}; "))
            .unwrap_or_default();
        return Err(LoadError::single(
            codes::NEWER_FORMAT,
            format!(
                "This project was made with a newer version of Blocks2Cpp ({needs}it uses project format {version}, and this version reads format {CURRENT_FORMAT_VERSION}). Update Blocks2Cpp to open it."
            ),
        ));
    }
    // At most `CURRENT_FORMAT_VERSION` now, so it fits.
    let version = u32::try_from(version).unwrap_or(CURRENT_FORMAT_VERSION);
    migrate::upgrade(root, version).map_err(|error| {
        let message = match error {
            MigrationError::NoPath { from } => format!(
                "This project uses project format {from}, which this version of Blocks2Cpp cannot read or upgrade."
            ),
            MigrationError::Failed { from, reason } => format!(
                "This project uses the older project format {from}, and upgrading it failed: {reason}."
            ),
        };
        LoadError::single(codes::BAD_FORMAT_VERSION, message)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes_of(bytes: &[u8]) -> Vec<String> {
        load(bytes)
            .unwrap_err()
            .diagnostics
            .into_iter()
            .map(|d| d.code.0)
            .collect()
    }

    #[test]
    fn size_limit_comes_before_parsing() {
        let mut huge = vec![b' '; MAX_FILE_BYTES + 1];
        huge[0] = b'{';
        assert_eq!(codes_of(&huge), [codes::FILE_TOO_LARGE]);
        // Callers read at most one byte past the limit, so the message must
        // not claim that the truncated input's length is the file's size.
        let error = load(&huge).unwrap_err();
        assert_eq!(
            error.diagnostics[0].message,
            "The project file is larger than 33554432 bytes (32 MiB), the most a project file can be."
        );
    }

    #[test]
    fn encoding_errors() {
        assert_eq!(codes_of(b"{\n \"a\": \"\xff\"}"), [codes::NOT_UTF8]);
        let error = load(b"{\n \"a\": \"\xff\"}").unwrap_err();
        assert!(error.diagnostics[0].message.contains("line 2, column 8"));
        assert_eq!(codes_of("\u{feff}{}".as_bytes()), [codes::NOT_UTF8]);
    }

    #[test]
    fn syntax_errors_have_positions() {
        let error = load(b"{\n  \"format\": \"blocks2cpp/project\",\n  oops\n}").unwrap_err();
        assert_eq!(error.diagnostics.len(), 1);
        let diagnostic = &error.diagnostics[0];
        assert_eq!(diagnostic.code.0, codes::JSON_SYNTAX);
        assert_eq!(
            diagnostic.message,
            "The project file is not valid JSON: expected a key in double quotes, but found 'o' (line 3, column 3)."
        );
        assert_eq!(codes_of(b""), [codes::JSON_SYNTAX]);
        assert_eq!(codes_of(&[b'['; 200]), [codes::TOO_DEEP]);
    }

    #[test]
    fn every_duplicate_key_is_reported() {
        let error = load(br#"{"a": 1, "b": {"c": 1, "c": 2}, "a": 2}"#).unwrap_err();
        let messages: Vec<&str> = error.diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(messages.len(), 2);
        assert!(messages[0].starts_with("The key \"c\" appears twice"));
        assert!(messages[1].contains("(line 1, column 33)"));
    }

    #[test]
    fn header_errors() {
        assert_eq!(codes_of(b"[]"), [codes::NOT_A_PROJECT]);
        assert_eq!(
            codes_of(br#"{"format": "something else"}"#),
            [codes::NOT_A_PROJECT]
        );
        assert_eq!(codes_of(br#"{"format": 1}"#), [codes::NOT_A_PROJECT]);
        assert_eq!(codes_of(br#"{"formatVersion": 1}"#), [codes::NOT_A_PROJECT]);
        let tag = br#"{"format": "blocks2cpp/project""#;
        for version in [
            "",
            ", \"formatVersion\": \"1\"",
            ", \"formatVersion\": -1",
            ", \"formatVersion\": 1.5",
        ] {
            let text = [&tag[..], version.as_bytes(), b"}"].concat();
            assert_eq!(codes_of(&text), [codes::BAD_FORMAT_VERSION], "{version}");
        }
        let text = [&tag[..], b", \"formatVersion\": 0}"].concat();
        assert_eq!(codes_of(&text), [codes::BAD_FORMAT_VERSION]);
    }

    #[test]
    fn newer_versions_are_refused_clearly() {
        let error = load(
            br#"{"format": "blocks2cpp/project", "formatVersion": 7, "generator": {"app": "3.1.0", "catalog": "2.0.0"}, "anything": "goes"}"#,
        )
        .unwrap_err();
        assert_eq!(error.diagnostics.len(), 1);
        assert_eq!(error.diagnostics[0].code.0, codes::NEWER_FORMAT);
        assert_eq!(
            error.diagnostics[0].message,
            "This project was made with a newer version of Blocks2Cpp (needs ≥ 3.1.0; it uses project format 7, and this version reads format 1). Update Blocks2Cpp to open it."
        );
        let error = load(
            br#"{"format": "blocks2cpp/project", "formatVersion": 2, "generator": {"app": "<b>evil</b>"}}"#,
        )
        .unwrap_err();
        assert!(error.diagnostics[0].message.contains("(it uses project format 2"));
        // A whole number beyond 32 bits is a newer format too, not "no valid
        // formatVersion".
        let error = load(br#"{"format": "blocks2cpp/project", "formatVersion": 4294967296}"#).unwrap_err();
        assert_eq!(error.diagnostics[0].code.0, codes::NEWER_FORMAT);
        assert!(
            error.diagnostics[0]
                .message
                .contains("it uses project format 4294967296,")
        );
    }

    #[test]
    fn load_error_display() {
        let error = load(b"[]").unwrap_err();
        assert_eq!(
            error.to_string(),
            "the project could not be loaded (1 problem(s))"
        );
        let empty = LoadError::from_diags(Diags::default());
        assert_eq!(empty.diagnostics.len(), 1);
    }
}
