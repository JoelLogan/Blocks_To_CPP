//! The JSON text the exports return (compact, 06 §6.13).
//!
//! Every function here returns a complete JSON document and never panics.
//! Should encoding fail anyway (a bug: the types here always serialise), the
//! result is the [`FacadeError::Encode`] envelope instead.

use b2c_ir::Diagnostic;
use b2c_model::{Document, LoadError};
use serde::Serialize;

use crate::error::FacadeError;
use crate::facade::{Canonical, Preview, VersionInfo};

/// Encodes a value, or the encode-error envelope when that fails.
fn encode<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|failure| error(&FacadeError::Encode(failure.to_string())))
}

/// `{"error": {"kind": "...", "message": "..."}}`.
pub(crate) fn error(failure: &FacadeError) -> String {
    #[derive(Serialize)]
    struct Envelope<'a> {
        error: Body<'a>,
    }
    #[derive(Serialize)]
    struct Body<'a> {
        kind: &'a str,
        message: &'a str,
    }
    let message = failure.to_string();
    let envelope = Envelope {
        error: Body {
            kind: failure.kind(),
            message: &message,
        },
    };
    // Two plain strings always encode; the literal is a last resort that
    // keeps the promise of returning JSON.
    serde_json::to_string(&envelope).unwrap_or_else(|_| {
        String::from(r#"{"error":{"kind":"encode","message":"the error could not be encoded as JSON"}}"#)
    })
}

/// A failed load: `{"ok": false, "diagnostics": [...]}`.
#[derive(Serialize)]
struct Failure<'a> {
    ok: bool,
    diagnostics: &'a [Diagnostic],
}

fn failure(problem: &LoadError) -> String {
    encode(&Failure {
        ok: false,
        diagnostics: &problem.diagnostics,
    })
}

pub(crate) fn version(info: &VersionInfo) -> String {
    encode(info)
}

/// `{"ok": true, "document": {…}, "diagnostics": []}` with the document in
/// canonical key order, or the failure.
///
/// The document is written by `b2c_model`'s canonical writer (struct keys in
/// declaration order, map keys sorted, top-level blocks by ID) and then
/// compacted, rather than through `serde_json`, so the key order cannot
/// depend on `serde_json` features, and the text is not parsed again.
pub(crate) fn load(result: &Result<Document, LoadError>) -> String {
    match result {
        Ok(document) => {
            let compact = compact_json(&b2c_model::to_canonical_json(document));
            let mut out = String::with_capacity(compact.len().saturating_add(48));
            out.push_str(r#"{"ok":true,"document":"#);
            out.push_str(&compact);
            out.push_str(r#","diagnostics":[]}"#);
            out
        }
        Err(problem) => failure(problem),
    }
}

/// `{"ok": true, "text": "…", "hash": "…", "diagnostics": []}`, or the
/// failure.
pub(crate) fn canonical(result: &Result<Canonical, LoadError>) -> String {
    #[derive(Serialize)]
    struct Success<'a> {
        ok: bool,
        text: &'a str,
        hash: &'a str,
        diagnostics: [Diagnostic; 0],
    }
    match result {
        Ok(canonical) => encode(&Success {
            ok: true,
            text: &canonical.text,
            hash: &canonical.hash,
            diagnostics: [],
        }),
        Err(problem) => failure(problem),
    }
}

pub(crate) fn preview(preview: &Preview) -> String {
    encode(preview)
}

/// Removes the whitespace between JSON tokens (outside strings).
///
/// For JSON text produced by a trusted writer (the canonical writer): it
/// only drops the four JSON whitespace characters outside string literals
/// and keeps everything else, escapes included, byte for byte.
pub(crate) fn compact_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    for c in text.chars() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if !matches!(c, ' ' | '\n' | '\r' | '\t') {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // tests fail by panicking
mod tests {
    use super::*;

    #[test]
    fn compact_keeps_strings_and_escapes() {
        let pretty = "{\n  \"a b\": [\n    1,\n    \"x \\\" y\\\\\",\n    \"\\n \"\n  ],\n  \"c\": {}\n}\n";
        let compact = compact_json(pretty);
        assert_eq!(compact, r#"{"a b":[1,"x \" y\\","\n "],"c":{}}"#);
        let a: serde_json::Value = serde_json::from_str(pretty).unwrap();
        let b: serde_json::Value = serde_json::from_str(&compact).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn compact_keeps_non_ascii_text() {
        assert_eq!(
            compact_json("[ \"é \u{1F600}\" ,\t\"\\u0007\" ]"),
            "[\"é \u{1F600}\",\"\\u0007\"]"
        );
    }

    #[test]
    fn error_envelope() {
        let text = error(&FacadeError::InvalidOptions(String::from("say \"2\"")));
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["error"]["kind"], "invalidOptions");
        assert_eq!(
            value["error"]["message"],
            "the preview options are not valid: say \"2\""
        );
    }

    #[test]
    fn failure_shape() {
        let text = load(&b2c_model::load(b"[]"));
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["ok"], false);
        assert!(value.get("document").is_none());
        assert!(!value["diagnostics"].as_array().unwrap().is_empty());
    }
}
