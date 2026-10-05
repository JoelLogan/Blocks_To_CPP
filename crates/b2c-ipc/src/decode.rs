//! Decoding untrusted input from the webview (`docs/spec/08-security.md` §8.8:
//! "Every command re-validates its input").
//!
//! Command handlers receive their `request` argument as a [`serde_json::Value`] and
//! pass it to [`decode`], so every failure becomes a typed
//! [`IpcError::InvalidRequest`] instead of Tauri's untyped argument error. Project
//! documents arrive as JSON text and go through [`parse_document`], which checks the
//! size before parsing a single byte.

use b2c_model::{Document, LoadError};
use serde::Deserialize;
use serde_json::Value;

use crate::commands::IpcRequest;
use crate::diag::convert_all;
use crate::error::{InvalidReason, IpcError};
use crate::ids::MALFORMED_ID;
use crate::limits::MAX_DOCUMENT_BYTES;
use crate::schema;

/// The loader's code for "made by a newer version" (`B2C-E0108`).
const NEWER_FORMAT_CODE: &str = "B2C-E0108";

/// Decodes and validates the `request` argument of `T`'s command.
///
/// Three steps, each of which can only reject:
///
/// 1. the structural check against [`IpcRequest::schema`], which reports the exact
///    field (unknown and missing keys, wrong JSON types, enum values, ID formats,
///    ranges and string sizes);
/// 2. serde, into the typed request (a failure here means the schema and the type
///    disagree, which a test rules out; it is still reported, without a field);
/// 3. [`IpcRequest::validate`], for what needs the typed value (base64 contents,
///    range rechecks).
///
/// # Errors
/// Returns [`IpcError::InvalidRequest`] or [`IpcError::PayloadTooLarge`]; nothing
/// has been changed when it does.
pub fn decode<T: IpcRequest>(value: Value) -> Result<T, IpcError> {
    schema::check(&value, T::schema())?;
    let request = serde_json::from_value::<T>(value).map_err(|error| serde_error(&error))?;
    request.validate()?;
    Ok(request)
}

/// Maps a serde error to an [`IpcError::InvalidRequest`] by its kind. The message
/// itself is never returned (it may quote request text).
fn serde_error(error: &serde_json::Error) -> IpcError {
    let text = error.to_string();
    let reason = if text.starts_with("unknown field") {
        InvalidReason::UnknownField
    } else if text.starts_with("missing field") {
        InvalidReason::MissingField
    } else if text.starts_with("unknown variant") {
        InvalidReason::BadEnum
    } else if text.starts_with(MALFORMED_ID) {
        InvalidReason::BadId
    } else if text.starts_with("invalid value") {
        InvalidReason::OutOfRange
    } else {
        InvalidReason::Malformed
    };
    IpcError::invalid(reason, None)
}

/// Parses a project document received over IPC.
///
/// A document longer than [`MAX_DOCUMENT_BYTES`] is rejected before any parsing.
/// Otherwise it goes through [`b2c_model::load`], the strict loader that applies
/// every limit and rule of `docs/spec/05-project-format.md` §5.6 and the
/// migrations of §5.7.
///
/// # Errors
/// * [`IpcError::PayloadTooLarge`] for a document over the size limit;
/// * [`IpcError::NewerFormat`] for a project made by a newer version
///   (`B2C-E0108`);
/// * [`IpcError::InvalidDocument`] with the loader's diagnostics otherwise.
pub fn parse_document(text: &str) -> Result<Document, IpcError> {
    parse_document_with(text, b2c_model::load)
}

/// [`parse_document`] with the loader passed in, so a test can prove that an
/// oversized document never reaches it.
pub(crate) fn parse_document_with(
    text: &str,
    load: impl FnOnce(&[u8]) -> Result<Document, LoadError>,
) -> Result<Document, IpcError> {
    if text.len() > MAX_DOCUMENT_BYTES {
        return Err(IpcError::too_large(MAX_DOCUMENT_BYTES));
    }
    load(text.as_bytes()).map_err(|error| load_error(text, &error))
}

fn load_error(text: &str, error: &LoadError) -> IpcError {
    if error.diagnostics.iter().any(|d| d.code.0 == NEWER_FORMAT_CODE) {
        IpcError::NewerFormat {
            needs: saved_by(text),
        }
    } else {
        IpcError::InvalidDocument {
            diagnostics: convert_all(&error.diagnostics),
        }
    }
}

/// The header keys [`saved_by`] reads; serde skips everything else.
#[derive(Deserialize)]
struct Header {
    #[serde(default)]
    generator: Option<GeneratorHeader>,
}

#[derive(Deserialize)]
struct GeneratorHeader {
    #[serde(default)]
    app: Option<Value>,
}

/// The app version that saved the document (`generator.app`), when it looks like
/// a version: 1 to 32 ASCII letters, digits, `.`, `-` or `+`, the same rule the
/// loader uses for its message. Only called after the loader accepted the JSON
/// syntax (it reported `B2C-E0108`, which comes after parsing).
fn saved_by(text: &str) -> Option<String> {
    let header = serde_json::from_str::<Header>(text).ok()?;
    let Some(Value::String(app)) = header.generator?.app else {
        return None;
    };
    let plausible = (1..=32).contains(&app.len())
        && app
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'));
    plausible.then_some(app)
}

/// The standard base64 alphabet (RFC 4648 §4).
const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn sextet(byte: u8) -> Option<u32> {
    let value = match byte {
        b'A'..=b'Z' => byte - b'A',
        b'a'..=b'z' => byte - b'a' + 26,
        b'0'..=b'9' => byte - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    };
    Some(u32::from(value))
}

/// Decodes standard base64 with padding (RFC 4648 §4), strictly: the length is a
/// multiple of 4, padding appears only at the end, there is no whitespace, and the
/// unused bits before the padding are zero, so every byte string has exactly one
/// accepted spelling.
///
/// # Errors
/// Returns [`IpcError::InvalidRequest`] with reason `badEncoding` (and no field)
/// for anything else.
pub fn decode_base64(text: &str) -> Result<Vec<u8>, IpcError> {
    let bad = || IpcError::invalid(InvalidReason::BadEncoding, None);
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err(bad());
    }
    let quads = bytes.len() / 4;
    let mut out = Vec::with_capacity(quads * 3);
    for (index, quad) in bytes.chunks_exact(4).enumerate() {
        let &[a, b, c, d] = quad else {
            return Err(bad());
        };
        let last = index + 1 == quads;
        let padding = match (c, d) {
            (b'=', b'=') if last => 2,
            (_, b'=') if last => 1,
            _ => 0,
        };
        let a = sextet(a).ok_or_else(bad)?;
        let b = sextet(b).ok_or_else(bad)?;
        let c = if padding == 2 {
            0
        } else {
            sextet(c).ok_or_else(bad)?
        };
        let d = if padding >= 1 {
            0
        } else {
            sextet(d).ok_or_else(bad)?
        };
        let word = (a << 18) | (b << 12) | (c << 6) | d;
        let [_, first, second, third] = word.to_be_bytes();
        match padding {
            2 if b & 0x0f != 0 => return Err(bad()),
            1 if c & 0x03 != 0 => return Err(bad()),
            2 => out.push(first),
            1 => out.extend_from_slice(&[first, second]),
            _ => out.extend_from_slice(&[first, second, third]),
        }
    }
    Ok(out)
}

/// Encodes bytes as standard base64 with padding: the inverse of
/// [`decode_base64`].
pub fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let symbol = |value: u32| char::from(BASE64.get((value & 0x3f) as usize).copied().unwrap_or(b'A'));
    for chunk in bytes.chunks(3) {
        let [a, b, c] = match *chunk {
            [a] => [a, 0, 0],
            [a, b] => [a, b, 0],
            [a, b, c, ..] => [a, b, c],
            [] => continue,
        };
        let word = u32::from_be_bytes([0, a, b, c]);
        out.push(symbol(word >> 18));
        out.push(symbol(word >> 12));
        out.push(if chunk.len() > 1 { symbol(word >> 6) } else { '=' });
        out.push(if chunk.len() > 2 { symbol(word) } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn oversized_documents_are_never_parsed() {
        let text = format!("{{{}", " ".repeat(MAX_DOCUMENT_BYTES));
        assert_eq!(text.len(), MAX_DOCUMENT_BYTES + 1);
        let called = Cell::new(false);
        let start = Instant::now();
        let result = parse_document_with(&text, |bytes| {
            called.set(true);
            b2c_model::load(bytes)
        });
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(!called.get(), "the loader must not see an oversized document");
        assert_eq!(
            result.unwrap_err(),
            IpcError::PayloadTooLarge { limit: 33_554_432 }
        );
        // Through the public function too.
        assert_eq!(
            parse_document(&text).unwrap_err(),
            IpcError::PayloadTooLarge { limit: 33_554_432 }
        );
    }

    #[test]
    fn documents_at_the_limit_reach_the_loader() {
        let text = " ".repeat(MAX_DOCUMENT_BYTES);
        let called = Cell::new(false);
        let error = parse_document_with(&text, |bytes| {
            called.set(true);
            b2c_model::load(bytes)
        })
        .unwrap_err();
        assert!(called.get());
        assert!(matches!(error, IpcError::InvalidDocument { .. }));
    }

    #[test]
    fn newer_formats_name_the_version_they_need() {
        let error = parse_document(
            r#"{"format": "blocks2cpp/project", "formatVersion": 7, "generator": {"app": "3.1.0", "catalog": "2.0.0"}, "anything": ["goes"]}"#,
        )
        .unwrap_err();
        assert_eq!(
            error,
            IpcError::NewerFormat {
                needs: Some("3.1.0".to_owned())
            }
        );
        for generator in [
            r#""generator": {"app": "<b>evil</b>"}"#,
            r#""generator": {"app": 3}"#,
            r#""generator": "3.1.0""#,
            r#""other": 1"#,
            &format!(r#""generator": {{"app": "{}"}}"#, "9".repeat(33)),
        ] {
            let text = format!(r#"{{"format": "blocks2cpp/project", "formatVersion": 2, {generator}}}"#);
            assert_eq!(
                parse_document(&text).unwrap_err(),
                IpcError::NewerFormat { needs: None },
                "{generator}"
            );
        }
    }

    #[test]
    fn invalid_documents_carry_the_loader_diagnostics() {
        let IpcError::InvalidDocument { diagnostics } = parse_document(r#"{"a": 1, "a": 2}"#).unwrap_err()
        else {
            panic!("expected invalidDocument");
        };
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "B2C-E0105");
        assert!(matches!(
            parse_document("").unwrap_err(),
            IpcError::InvalidDocument { .. }
        ));
    }

    #[test]
    fn base64_round_trips() {
        for len in 0_usize..=40 {
            let bytes: Vec<u8> = (0..len)
                .map(|i| u8::try_from((i * 37 + 11) % 256).unwrap())
                .collect();
            let text = encode_base64(&bytes);
            assert_eq!(text.len(), len.div_ceil(3) * 4);
            assert_eq!(decode_base64(&text).unwrap(), bytes, "{text}");
        }
        assert_eq!(encode_base64(b"Man"), "TWFu");
        assert_eq!(encode_base64(b"Ma"), "TWE=");
        assert_eq!(encode_base64(b"M"), "TQ==");
        assert_eq!(encode_base64(&[0xfb, 0xff]), "+/8=");
        assert_eq!(decode_base64("").unwrap(), b"");
    }

    #[test]
    fn base64_is_strict() {
        let bad = IpcError::invalid(InvalidReason::BadEncoding, None);
        for text in [
            "TWF", "TWFuT", "TW=u", "T===", "====", "TQ=", "TQ==TQ==", "TW Fu", "TWFu\n", "TWF-", "TWF_",
            "TR==", "TWF=", "TWFuTW=u", "é===",
        ] {
            assert_eq!(decode_base64(text).unwrap_err(), bad, "{text:?}");
        }
    }

    #[test]
    fn serde_errors_keep_their_kind_but_not_their_text() {
        #[derive(Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        #[allow(dead_code)]
        struct Probe {
            small: u8,
            choice: crate::dto::BuildConfig,
            handle: crate::ids::Handle,
        }
        let handle = crate::ids::Handle::example();
        let cases = [
            (
                r#"{"small": 1, "choice": "debug", "handle": "H", "zz": 1}"#,
                InvalidReason::UnknownField,
            ),
            (r#"{"small": 1, "choice": "debug"}"#, InvalidReason::MissingField),
            (
                r#"{"small": 1, "choice": "fast", "handle": "H"}"#,
                InvalidReason::BadEnum,
            ),
            (
                r#"{"small": 1, "choice": "debug", "handle": "x"}"#,
                InvalidReason::BadId,
            ),
            (
                r#"{"small": 300, "choice": "debug", "handle": "H"}"#,
                InvalidReason::OutOfRange,
            ),
            (
                r#"{"small": "1", "choice": "debug", "handle": "H"}"#,
                InvalidReason::Malformed,
            ),
        ];
        for (text, reason) in cases {
            let text = text.replace("\"H\"", &format!("\"{handle}\""));
            let value: Value = serde_json::from_str(&text).unwrap();
            let error = serde_json::from_value::<Probe>(value).unwrap_err();
            assert_eq!(serde_error(&error), IpcError::invalid(reason, None), "{text}");
        }
    }
}
