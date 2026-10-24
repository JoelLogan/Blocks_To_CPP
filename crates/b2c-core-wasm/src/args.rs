//! Strict parsing of the small JSON arguments of the clipboard exports: the
//! list of block IDs to copy, the paste target and the ID seed.
//!
//! These come from the editor, not from a file, but the facade still treats
//! them as untrusted: every text is bounded before it is parsed, unknown and
//! repeated keys are refused, IDs must follow the ID rules (05 §5.4), and an
//! error message never repeats input that did not pass those rules.

use std::collections::BTreeSet;

use b2c_ir::{BlockId, ModuleId};
use serde::Deserialize;

use crate::error::FacadeError;

/// The longest block ID list accepted, in bytes: room for
/// [`b2c_model::limits::MAX_BLOCKS`] IDs of the longest kind, quoted and
/// separated by commas.
pub const MAX_BLOCK_IDS_BYTES: usize = b2c_model::limits::MAX_BLOCKS * (b2c_ir::ids::MAX_ID_LEN + 3) + 2;

/// The longest paste target accepted, in bytes. A real target is well under
/// 200 bytes.
pub const MAX_TARGET_BYTES: usize = 1024;

/// The longest input name accepted in a paste target or a scope query.
/// Catalog input names are short upper-case words (`BODY`, `DO0`, `VALUE`).
pub const MAX_INPUT_NAME_LEN: usize = 64;

/// The length of the ID seed in hex digits (256 bits).
pub const SEED_HEX_LEN: usize = 64;

fn invalid(message: impl Into<String>) -> FacadeError {
    FacadeError::InvalidArguments(message.into())
}

/// Parses the block ID list of `clipboard_make`: a JSON array of distinct
/// block IDs, at most [`b2c_model::limits::MAX_BLOCKS`] of them.
///
/// # Errors
/// [`FacadeError::InvalidArguments`] when the text is too long, is not a
/// JSON array of strings, holds a text that is not a block ID, or repeats an
/// ID.
pub fn block_ids(text: &str) -> Result<Vec<BlockId>, FacadeError> {
    const EXPECTED: &str = "the block IDs should be a JSON list of distinct block IDs";
    if text.len() > MAX_BLOCK_IDS_BYTES {
        return Err(invalid(format!(
            "{EXPECTED}, but the list is longer than {MAX_BLOCK_IDS_BYTES} bytes"
        )));
    }
    let raw: Vec<String> = serde_json::from_str(text).map_err(|error| {
        invalid(format!(
            "{EXPECTED} (stopped at line {}, column {})",
            error.line(),
            error.column()
        ))
    })?;
    if raw.len() > b2c_model::limits::MAX_BLOCKS {
        return Err(invalid(format!(
            "{EXPECTED}, but it has more than {} entries",
            b2c_model::limits::MAX_BLOCKS
        )));
    }
    let mut seen = BTreeSet::new();
    let mut ids = Vec::with_capacity(raw.len());
    for (index, text) in raw.iter().enumerate() {
        let id = BlockId::new(text)
            .map_err(|_| invalid(format!("{EXPECTED}; entry {index} is not a block ID")))?;
        if !seen.insert(id.clone()) {
            return Err(invalid(format!("{EXPECTED}; the block {id} is listed twice")));
        }
        ids.push(id);
    }
    Ok(ids)
}

/// Where pasted blocks go (see [`crate::Session::paste_prepare`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteTarget {
    /// The module whose canvas or blocks receive the paste.
    pub module: ModuleId,
    /// The block the paste is relative to; `None` for the module's canvas.
    pub block: Option<BlockId>,
    /// With a block: one of its statement or value inputs, or `None` for
    /// "directly after the block".
    pub input: Option<String>,
}

/// The wire form, `{"module": "…", "block": "…" | null, "input": "…" | null}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTarget {
    module: String,
    #[serde(default)]
    block: Option<String>,
    #[serde(default)]
    input: Option<String>,
}

/// Parses a paste target.
///
/// # Errors
/// [`FacadeError::InvalidArguments`] when the text is longer than
/// [`MAX_TARGET_BYTES`], is not an object with only the keys `module`,
/// `block` and `input` (each at most once), when `module` or `block` is not
/// an ID, when `input` is given without a block, or when `input` is not an
/// input name (1 to [`MAX_INPUT_NAME_LEN`] characters from `A`–`Z`, `a`–`z`,
/// `0`–`9` and `_`).
pub fn paste_target(text: &str) -> Result<PasteTarget, FacadeError> {
    const EXPECTED: &str = "the paste target should be {\"module\": <module ID>, \"block\": <block ID> or null, \"input\": <input name> or null}";
    if text.len() > MAX_TARGET_BYTES {
        return Err(invalid(format!(
            "{EXPECTED}, but it is longer than {MAX_TARGET_BYTES} bytes"
        )));
    }
    let wire: WireTarget = serde_json::from_str(text).map_err(|error| {
        invalid(format!(
            "{EXPECTED} (stopped at line {}, column {})",
            error.line(),
            error.column()
        ))
    })?;
    let module =
        ModuleId::new(&wire.module).map_err(|_| invalid(format!("{EXPECTED}; module is not an ID")))?;
    let block = wire
        .block
        .as_deref()
        .map(BlockId::new)
        .transpose()
        .map_err(|_| invalid(format!("{EXPECTED}; block is not an ID")))?;
    let input = wire.input;
    if let Some(name) = &input {
        if block.is_none() {
            return Err(invalid(format!("{EXPECTED}; an input needs a block")));
        }
        if !is_input_name(name) {
            return Err(invalid(format!("{EXPECTED}; input is not an input name")));
        }
    }
    Ok(PasteTarget { module, block, input })
}

/// Whether a text can be an input name: 1 to [`MAX_INPUT_NAME_LEN`]
/// characters from `A`–`Z`, `a`–`z`, `0`–`9` and `_`.
pub fn is_input_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_INPUT_NAME_LEN
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Parses the ID seed: exactly [`SEED_HEX_LEN`] hex digits (either case),
/// 256 bits that the editor takes from `crypto.getRandomValues`.
///
/// # Errors
/// [`FacadeError::InvalidArguments`] for any other text.
pub fn seed(text: &str) -> Result<[u8; 32], FacadeError> {
    let refuse = || {
        invalid(format!(
            "the seed should be {SEED_HEX_LEN} hex digits (256 random bits)"
        ))
    };
    if text.len() != SEED_HEX_LEN {
        return Err(refuse());
    }
    let mut seed = [0u8; 32];
    for (byte, pair) in seed.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        let (Some(high), Some(low)) = (
            pair.first().and_then(|&b| hex_value(b)),
            pair.get(1).and_then(|&b| hex_value(b)),
        ) else {
            return Err(refuse());
        };
        *byte = (high << 4) | low;
    }
    Ok(seed)
}

/// The value of one hex digit.
fn hex_value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // tests fail by panicking
mod tests {
    use super::*;

    fn refused<T: std::fmt::Debug>(result: Result<T, FacadeError>) -> String {
        match result {
            Err(FacadeError::InvalidArguments(message)) => message,
            other => panic!("expected invalid arguments, got {other:?}"),
        }
    }

    #[test]
    fn block_id_lists() {
        assert_eq!(block_ids("[]").unwrap(), Vec::<BlockId>::new());
        let ids = block_ids(r#"["b1", "blk_X9"]"#).unwrap();
        assert_eq!(
            ids.iter().map(BlockId::as_str).collect::<Vec<_>>(),
            ["b1", "blk_X9"]
        );
        for text in [
            "",
            "{}",
            "null",
            r#"["b1", 2]"#,
            r#"["b-1"]"#,
            r#"[""]"#,
            r#"["b1", "b1"]"#,
            r#"["b1"] x"#,
            &format!("[\"{}\"]", "a".repeat(33)),
        ] {
            let message = refused(block_ids(text));
            assert!(
                message.starts_with("the block IDs should be"),
                "{text}: {message}"
            );
        }
        assert!(refused(block_ids(r#"["b1", "b1"]"#)).contains("b1 is listed twice"));
        // Invalid text is never repeated.
        assert!(!refused(block_ids(r#"["<script>"]"#)).contains("script"));
    }

    #[test]
    fn block_id_lists_are_bounded() {
        let long = format!("[{}]", " ".repeat(MAX_BLOCK_IDS_BYTES));
        assert!(refused(block_ids(&long)).contains("longer than"));
        let too_many = format!("[{}]", vec!["\"b\""; b2c_model::limits::MAX_BLOCKS + 1].join(","));
        assert!(too_many.len() <= MAX_BLOCK_IDS_BYTES);
        assert!(refused(block_ids(&too_many)).contains("more than 100000 entries"));
    }

    #[test]
    fn paste_targets() {
        let target = paste_target(r#"{"module": "mod_main", "block": "b005", "input": "BODY"}"#).unwrap();
        assert_eq!(target.module.as_str(), "mod_main");
        assert_eq!(target.block.as_ref().map(BlockId::as_str), Some("b005"));
        assert_eq!(target.input.as_deref(), Some("BODY"));
        let canvas = paste_target(r#"{"module": "m", "block": null, "input": null}"#).unwrap();
        assert_eq!(canvas.block, None);
        assert_eq!(canvas.input, None);
        let short = paste_target(r#"{"module": "m"}"#).unwrap();
        assert_eq!(short, canvas);
        for text in [
            "",
            "[]",
            "{}",
            r#"{"module": 1}"#,
            r#"{"module": "m-1"}"#,
            r#"{"module": "m", "block": "b x"}"#,
            r#"{"module": "m", "input": "BODY"}"#,
            r#"{"module": "m", "block": "b", "input": ""}"#,
            r#"{"module": "m", "block": "b", "input": "BO DY"}"#,
            r#"{"module": "m", "module": "n"}"#,
            r#"{"module": "m", "extra": true}"#,
            r#"{"module": "m", "__proto__": {}}"#,
        ] {
            let message = refused(paste_target(text));
            assert!(
                message.starts_with("the paste target should be"),
                "{text}: {message}"
            );
        }
        let long = format!(r#"{{"module": "m"{}}}"#, " ".repeat(MAX_TARGET_BYTES));
        assert!(refused(paste_target(&long)).contains("longer than 1024 bytes"));
        let name = "A".repeat(MAX_INPUT_NAME_LEN + 1);
        assert!(
            refused(paste_target(&format!(
                r#"{{"module": "m", "block": "b", "input": "{name}"}}"#
            )))
            .contains("not an input name")
        );
    }

    #[test]
    fn input_names() {
        assert!(is_input_name("BODY"));
        assert!(is_input_name("DO0"));
        assert!(is_input_name(&"A".repeat(MAX_INPUT_NAME_LEN)));
        assert!(!is_input_name(""));
        assert!(!is_input_name(&"A".repeat(MAX_INPUT_NAME_LEN + 1)));
        assert!(!is_input_name("A-B"));
        assert!(!is_input_name("É"));
    }

    #[test]
    fn seeds() {
        let text = "00ff10Ab".repeat(8);
        let seed = seed(&text).unwrap();
        assert_eq!(seed[..4], [0x00, 0xff, 0x10, 0xab]);
        assert_eq!(seed[28..], [0x00, 0xff, 0x10, 0xab]);
        for bad in [
            String::new(),
            "0".repeat(63),
            "0".repeat(65),
            format!("{}g", "0".repeat(63)),
            format!("{}é", "0".repeat(62)),
            " ".repeat(64),
        ] {
            assert!(refused(super::seed(&bad)).contains("64 hex digits"), "{bad:?}");
        }
    }
}
