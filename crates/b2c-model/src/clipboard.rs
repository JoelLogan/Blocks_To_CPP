//! The clipboard format (spec §5.12): blocks copied in the editor, put on
//! the clipboard as `application/x-blocks2cpp+json`.
//!
//! ```json
//! { "format": "blocks2cpp/clipboard", "formatVersion": 1, "catalog": "1.0.0",
//!   "blocks": [ … ],
//!   "refs": { "<symbol id>": { "name": "geo::area", "kind": "function" } } }
//! ```
//!
//! Clipboard data comes from anywhere (another app, a web page, a
//! hand-written file), so it is as untrusted as a project file. Pasting runs
//! the **same validator and limits as file loading** (spec §5.6):
//! [`load_clipboard`] uses the same strict JSON parser, size and depth
//! limits, text rules, block decoder and `B2C-E01xx` codes as
//! [`crate::load()`]. Only the header differs: data that is not clipboard data
//! is `B2C-E0138`, and a newer `formatVersion` is `B2C-E0108`.
//!
//! The copied blocks are the payload's top-level blocks: like blocks on a
//! canvas they may have `x`/`y` and a loose `stack` (ADR-0011), so copying a
//! stack gives one block with a `stack`. Block and symbol IDs must be unique
//! within the payload. Pasted blocks get fresh IDs with
//! [`crate::remap_ids`] before they join a document.

use std::collections::BTreeMap;

use b2c_ir::SymbolId;
use serde::{Deserialize, Serialize};

use crate::codes::{self, Diags};
use crate::decode::{Decoder, Origin};
use crate::document::{Block, FORMAT_TAG};
use crate::json::Json;
use crate::load::{LoadError, check_bytes, parse};
use crate::save::Writer;
use crate::text_rules::quote;

/// The value of a clipboard payload's `"format"` key.
pub const CLIPBOARD_FORMAT_TAG: &str = "blocks2cpp/clipboard";

/// The clipboard format version this build reads and writes.
pub const CLIPBOARD_FORMAT_VERSION: u32 = 1;

/// A validated clipboard payload (spec §5.12). `format` and
/// `formatVersion` are not stored: they are always
/// [`CLIPBOARD_FORMAT_TAG`] and [`CLIPBOARD_FORMAT_VERSION`].
#[derive(Debug, Clone, PartialEq)]
pub struct Clipboard {
    /// Version of the block catalog the blocks were copied with, e.g.
    /// `1.0.0`.
    pub catalog: String,
    /// The copied blocks, in the order they were copied. Each may carry
    /// `x`/`y` and a loose `stack`, like a block on a canvas.
    pub blocks: Vec<Block>,
    /// The symbols the blocks refer to but do not declare, so that a paste
    /// can bind them again by qualified name and kind (spec §6.14.11).
    pub refs: BTreeMap<SymbolId, ClipboardRef>,
}

/// What a clipboard payload records about a symbol that its blocks use but
/// do not declare.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipboardRef {
    /// The symbol's qualified name, e.g. `geo::area`, `::count` or a bare
    /// `count` in the main module (spec §5.12). Text rules apply; at most
    /// [`crate::limits::MAX_QUALIFIED_NAME_LEN`] characters.
    pub name: String,
    /// What kind of symbol it is. A paste binds only to a symbol of the
    /// same kind.
    pub kind: RefKind,
}

/// The kind of a symbol named in a clipboard payload's `refs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefKind {
    /// A variable (`"variable"`).
    Variable,
    /// A function parameter (`"parameter"`).
    Parameter,
    /// A loop counter (`"loopVariable"`).
    LoopVariable,
    /// A function (`"function"`).
    Function,
}

/// Parses and validates an untrusted clipboard payload.
///
/// The steps and limits are those of [`crate::load()`]: size, UTF-8 and byte
/// order mark; the strict JSON parser (syntax, depth, value count, duplicate
/// keys); the header; then every block and `refs` rule, all problems
/// reported at once.
///
/// # Errors
/// Returns every problem found when the input is not a valid clipboard
/// payload, with loader codes (`B2C-E01xx`).
pub fn load_clipboard(bytes: &[u8]) -> Result<Clipboard, LoadError> {
    let text = check_bytes(bytes, Origin::Clipboard)?;
    let root = parse(text, Origin::Clipboard)?;
    check_header(&root)?;
    let (clipboard, diags) = Decoder::new(Diags::default(), Origin::Clipboard).run_clipboard(&root);
    match clipboard {
        Some(clipboard) if diags.is_empty() => Ok(clipboard),
        _ => Err(LoadError::from_diags(diags)),
    }
}

/// Serialises a clipboard payload canonically, in the style of
/// [`crate::to_canonical_json`]: 2-space indentation, `\n` line endings, one
/// trailing newline, keys in the order `format`, `formatVersion`,
/// `catalog`, `blocks`, `refs` (always written, `{}` when empty), map keys
/// sorted. The blocks keep their order. For a valid payload,
/// `load_clipboard(to_canonical_clipboard_json(c)) == c`.
pub fn to_canonical_clipboard_json(clipboard: &Clipboard) -> String {
    let mut writer = Writer::new(false);
    writer.begin('{');
    writer.field_str("format", CLIPBOARD_FORMAT_TAG);
    writer.field_display("formatVersion", CLIPBOARD_FORMAT_VERSION);
    writer.field_str("catalog", &clipboard.catalog);
    writer.key("blocks");
    writer.begin('[');
    for block in &clipboard.blocks {
        writer.entry();
        writer.block(block);
    }
    writer.end(']');
    writer.key("refs");
    writer.begin('{');
    for (sym, reference) in &clipboard.refs {
        writer.key(sym.as_str());
        writer.begin('{');
        writer.field_str("name", &reference.name);
        writer.field_variant("kind", &reference.kind);
        writer.end('}');
    }
    writer.end('}');
    writer.end('}');
    writer.finish()
}

/// The header: `format` must be [`CLIPBOARD_FORMAT_TAG`] (else
/// `B2C-E0138`), and `formatVersion` a whole number: newer is `B2C-E0108`,
/// anything else but [`CLIPBOARD_FORMAT_VERSION`] is `B2C-E0109`. There is
/// one clipboard format version so far, so there are no migrations.
fn check_header(root: &Json) -> Result<(), LoadError> {
    let not_clipboard = |detail: String| LoadError::single(codes::NOT_CLIPBOARD, detail);
    let Json::Object(_) = root else {
        return Err(not_clipboard(format!(
            "The pasted data is not Blocks2Cpp blocks: it should hold a JSON object, but it holds {}.",
            root.kind()
        )));
    };
    match root.get("format") {
        Some(Json::String(format)) if &**format == CLIPBOARD_FORMAT_TAG => {}
        Some(Json::String(format)) if &**format == FORMAT_TAG => {
            return Err(not_clipboard(String::from(
                "The pasted data is a whole Blocks2Cpp project, not copied blocks. Open it as a project instead.",
            )));
        }
        Some(Json::String(format)) => {
            return Err(not_clipboard(format!(
                "The pasted data is not Blocks2Cpp blocks: its \"format\" is {}, not \"{CLIPBOARD_FORMAT_TAG}\".",
                quote(format)
            )));
        }
        Some(other) => {
            return Err(not_clipboard(format!(
                "The pasted data is not Blocks2Cpp blocks: its \"format\" should be \"{CLIPBOARD_FORMAT_TAG}\", but it is {}.",
                other.kind()
            )));
        }
        None => {
            return Err(not_clipboard(String::from(
                "The pasted data is not Blocks2Cpp blocks: it has no \"format\" key.",
            )));
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
                "The pasted data has no valid \"formatVersion\" (a whole number such as 1), so it cannot be pasted.",
            ),
        ));
    };
    if version > u64::from(CLIPBOARD_FORMAT_VERSION) {
        return Err(LoadError::single(
            codes::NEWER_FORMAT,
            format!(
                "These blocks were copied from a newer version of Blocks2Cpp (they use clipboard format {version}, and this version reads format {CLIPBOARD_FORMAT_VERSION}). Update Blocks2Cpp to paste them."
            ),
        ));
    }
    if version < u64::from(CLIPBOARD_FORMAT_VERSION) {
        return Err(LoadError::single(
            codes::BAD_FORMAT_VERSION,
            format!(
                "The pasted data uses clipboard format {version}, which this version of Blocks2Cpp cannot read."
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes_of(bytes: &[u8]) -> Vec<String> {
        load_clipboard(bytes)
            .unwrap_err()
            .diagnostics
            .into_iter()
            .map(|d| d.code.0)
            .collect()
    }

    fn message_of(bytes: &[u8]) -> String {
        let error = load_clipboard(bytes).unwrap_err();
        assert_eq!(error.diagnostics.len(), 1, "{:?}", error.diagnostics);
        error.diagnostics[0].message.clone()
    }

    #[test]
    fn the_smallest_payload_loads() {
        let clipboard = load_clipboard(
            br#"{"format": "blocks2cpp/clipboard", "formatVersion": 1, "catalog": "1.0.0", "blocks": [], "refs": {}}"#,
        )
        .unwrap();
        assert_eq!(clipboard.catalog, "1.0.0");
        assert!(clipboard.blocks.is_empty());
        assert!(clipboard.refs.is_empty());
        assert_eq!(
            to_canonical_clipboard_json(&clipboard),
            "{\n  \"format\": \"blocks2cpp/clipboard\",\n  \"formatVersion\": 1,\n  \"catalog\": \"1.0.0\",\n  \"blocks\": [],\n  \"refs\": {}\n}\n"
        );
    }

    #[test]
    fn bytes_and_json_errors_name_the_pasted_data() {
        assert_eq!(
            message_of(b"{\"a\": \"\xff\"}"),
            "The pasted data is not valid UTF-8 text (line 1, column 8)."
        );
        assert_eq!(
            message_of("\u{feff}{}".as_bytes()),
            "The pasted data starts with an invisible byte order mark (BOM)."
        );
        assert_eq!(
            message_of(b"{oops}"),
            "The pasted data is not valid JSON: expected a key in double quotes, but found 'o' (line 1, column 2)."
        );
        assert!(
            message_of(&[b'['; 129])
                .starts_with("Lists and objects in the pasted data are nested more than 128 levels deep")
        );
        let huge = vec![b' '; crate::limits::MAX_FILE_BYTES + 1];
        assert_eq!(
            message_of(&huge),
            "The pasted data is larger than 33554432 bytes (32 MiB), the most a paste can be."
        );
    }

    #[test]
    fn header_errors() {
        assert_eq!(codes_of(b"[]"), [codes::NOT_CLIPBOARD]);
        assert_eq!(codes_of(b"{}"), [codes::NOT_CLIPBOARD]);
        assert_eq!(codes_of(br#"{"format": 1}"#), [codes::NOT_CLIPBOARD]);
        assert_eq!(
            message_of(br#"{"format": "blockly/workspace"}"#),
            "The pasted data is not Blocks2Cpp blocks: its \"format\" is \"blockly/workspace\", not \"blocks2cpp/clipboard\"."
        );
        assert_eq!(
            message_of(br#"{"format": "blocks2cpp/project", "formatVersion": 1}"#),
            "The pasted data is a whole Blocks2Cpp project, not copied blocks. Open it as a project instead."
        );
        let tag = br#"{"format": "blocks2cpp/clipboard""#;
        for version in [
            "",
            ", \"formatVersion\": \"1\"",
            ", \"formatVersion\": -1",
            ", \"formatVersion\": 1.5",
            ", \"formatVersion\": 0",
        ] {
            let text = [&tag[..], version.as_bytes(), b"}"].concat();
            assert_eq!(codes_of(&text), [codes::BAD_FORMAT_VERSION], "{version}");
        }
    }

    #[test]
    fn newer_versions_are_refused_clearly() {
        for version in ["2", "4294967296"] {
            let text = format!(
                r#"{{"format": "blocks2cpp/clipboard", "formatVersion": {version}, "future": true}}"#
            );
            assert_eq!(
                message_of(text.as_bytes()),
                format!(
                    "These blocks were copied from a newer version of Blocks2Cpp (they use clipboard format {version}, and this version reads format 1). Update Blocks2Cpp to paste them."
                )
            );
            assert_eq!(codes_of(text.as_bytes()), [codes::NEWER_FORMAT]);
        }
    }

    #[test]
    fn unknown_and_missing_keys() {
        let error = load_clipboard(br#"{"format": "blocks2cpp/clipboard", "formatVersion": 1, "x-ext": {}}"#)
            .unwrap_err();
        let messages: Vec<&str> = error.diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "The pasted data has an unknown key \"x-ext\". Remove it or check its spelling.",
                "The pasted data is missing \"catalog\".",
                "The pasted data is missing \"blocks\".",
                "The pasted data is missing \"refs\".",
            ]
        );
    }

    #[test]
    fn refs_are_checked() {
        let payload = |refs: &str| {
            format!(
                r#"{{"format": "blocks2cpp/clipboard", "formatVersion": 1, "catalog": "1.0.0", "blocks": [], "refs": {refs}}}"#
            )
        };
        let clipboard = load_clipboard(
            payload(r#"{"sym_b": {"name": "geo::area", "kind": "function"}, "sym_a": {"name": "i", "kind": "loopVariable"}}"#)
                .as_bytes(),
        )
        .unwrap();
        let names: Vec<(&str, &str, RefKind)> = clipboard
            .refs
            .iter()
            .map(|(sym, r)| (sym.as_str(), r.name.as_str(), r.kind))
            .collect();
        assert_eq!(
            names,
            [
                ("sym_a", "i", RefKind::LoopVariable),
                ("sym_b", "geo::area", RefKind::Function)
            ]
        );
        let cases = [
            ("[]", vec![codes::WRONG_VALUE]),
            (
                r#"{"bad id": {"name": "x", "kind": "variable"}}"#,
                vec![codes::BAD_ID],
            ),
            (
                r#"{"__proto__": {"name": "x", "kind": "variable"}}"#,
                vec![codes::RESERVED_KEY],
            ),
            (
                r#"{"s": {"name": "x", "kind": "macro"}}"#,
                vec![codes::WRONG_VALUE],
            ),
            (r#"{"s": {"name": "x"}}"#, vec![codes::MISSING_KEY]),
            (
                r#"{"s": {"name": "x", "kind": "variable", "type": "int"}}"#,
                vec![codes::UNKNOWN_KEY],
            ),
            (
                r#"{"s": {"name": "a\u202eb", "kind": "variable"}}"#,
                vec![codes::BIDI_CHAR],
            ),
            (
                r#"{"s": {"name": 5, "kind": "variable"}}"#,
                vec![codes::WRONG_VALUE],
            ),
            (r#"{"s": "x"}"#, vec![codes::WRONG_VALUE]),
        ];
        for (refs, expected) in cases {
            assert_eq!(codes_of(payload(refs).as_bytes()), expected, "{refs}");
        }
        let long = "n".repeat(crate::limits::MAX_QUALIFIED_NAME_LEN);
        let fits = format!(r#"{{"s": {{"name": "{long}", "kind": "variable"}}}}"#);
        assert!(load_clipboard(payload(&fits).as_bytes()).is_ok());
        let too_long = format!(r#"{{"s": {{"name": "{long}x", "kind": "variable"}}}}"#);
        assert_eq!(
            message_of(payload(&too_long).as_bytes()),
            "\"refs.s.name\" is 1025 characters long, but a qualified name can be at most 1024."
        );
    }
}
