//! What workspace trust looks at (spec §8.3): the security hash stored in a
//! trust record, and the summary the native trust dialog lists.
//!
//! Spec §8.3, *Re-check on outside change*: a trust record stores a hash of
//! the security-relevant content (all Raw C++ text, library requirements,
//! pack references and defines). When that content changes outside the app
//! (found at load), the project returns to Restricted Mode; saves inside the
//! app update the record. Everything else (layout, comments, ordinary
//! blocks) can change without a new trust decision, because the hardened
//! generator turns it into safe C++ on its own.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use b2c_ir::BlockId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::document::{Block, DefineValue, Document, FieldValue};
use crate::save::push_json_string;
use crate::walk;

/// The first bytes hashed by [`security_hash`]: names the hash and its
/// version, so a future change to the input gives different hashes.
const HASH_DOMAIN: &[u8] = b"b2c-trust-v1\n";

/// Block types whose text is Raw C++ start with this.
const RAW_CPP_PREFIX: &str = "raw.";

/// Block types that read or write files start with this.
const FILE_SYSTEM_PREFIX: &str = "file.";

/// SHA-256 of a document's security-relevant content, for trust records
/// (stored as [`hex`] under `rawCodeHashAtGrant`, spec §5.9 and §8.3).
///
/// The hashed bytes are the ASCII text `b2c-trust-v1`, a line feed (0x0A),
/// and then compact JSON (no spaces, `serde_json` string escaping) with the
/// keys in this order:
///
/// * `rawCpp`: `[[blockId, fieldName, text], …]` for every text field of
///   every block whose type starts with `raw.`, in every module, at any
///   depth, in stacks, and disabled ones included; sorted by block ID, then
///   field name (then text);
/// * `libraries`: `project.build.libraries`, sorted;
/// * `packs`: `[{"id": …, "version": …}, …]`, sorted by ID, then version;
/// * `defines`: `project.build.defines` in file order, each as
///   `{"name": …, "value": {"int": n} | {"bool": b} | {"string": s}}`.
///
/// So the hash changes when a define, a pack, a library or Raw C++ text
/// changes, and does not change for layout, comments or other blocks.
pub fn security_hash(document: &Document) -> [u8; 32] {
    let mut raw_cpp: Vec<(&str, &str, &str)> = Vec::new();
    walk::document(document, |block| {
        if is_raw_cpp(block) {
            for (name, value) in &block.fields {
                if let FieldValue::Text(text) = value {
                    raw_cpp.push((block.id.as_str(), name, text));
                }
            }
        }
    });
    raw_cpp.sort_unstable();
    let build = &document.project.build;
    let mut libraries: Vec<&str> = build.libraries.iter().map(String::as_str).collect();
    libraries.sort_unstable();
    let mut packs: Vec<(&str, &str)> = build
        .packs
        .iter()
        .map(|pack| (pack.id.as_str(), pack.version.as_str()))
        .collect();
    packs.sort_unstable();

    let mut json = String::from("{\"rawCpp\":[");
    for (index, (block, field, text)) in raw_cpp.into_iter().enumerate() {
        comma(&mut json, index);
        json.push('[');
        push_json_string(&mut json, block);
        json.push(',');
        push_json_string(&mut json, field);
        json.push(',');
        push_json_string(&mut json, text);
        json.push(']');
    }
    json.push_str("],\"libraries\":[");
    for (index, library) in libraries.into_iter().enumerate() {
        comma(&mut json, index);
        push_json_string(&mut json, library);
    }
    json.push_str("],\"packs\":[");
    for (index, (id, version)) in packs.into_iter().enumerate() {
        comma(&mut json, index);
        json.push_str("{\"id\":");
        push_json_string(&mut json, id);
        json.push_str(",\"version\":");
        push_json_string(&mut json, version);
        json.push('}');
    }
    json.push_str("],\"defines\":[");
    for (index, define) in build.defines.iter().enumerate() {
        comma(&mut json, index);
        json.push_str("{\"name\":");
        push_json_string(&mut json, &define.name);
        json.push_str(",\"value\":{");
        match &define.value {
            DefineValue::Int(n) => {
                let _ = write!(json, "\"int\":{n}");
            }
            DefineValue::Bool(b) => {
                let _ = write!(json, "\"bool\":{b}");
            }
            DefineValue::String(s) => {
                json.push_str("\"string\":");
                push_json_string(&mut json, s);
            }
        }
        json.push_str("}}");
    }
    json.push_str("]}");

    let mut hasher = Sha256::new();
    hasher.update(HASH_DOMAIN);
    hasher.update(json.as_bytes());
    hasher.finalize().into()
}

/// What the native trust dialog lists (spec §8.3), computed by the backend
/// from its own parsed document, never from text the webview sends.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecuritySummary {
    /// Raw C++ blocks (types starting with `raw.`), disabled ones included.
    pub raw_cpp_blocks: Vec<BlockId>,
    /// Library requirements (`project.build.libraries`), sorted, each once.
    pub libraries: Vec<String>,
    /// Blocks that read or write files (types starting with `file.`),
    /// disabled ones included.
    pub file_system_blocks: Vec<BlockId>,
}

/// Lists a document's Raw C++ blocks, library requirements and file-system
/// blocks for the trust dialog (spec §8.3).
///
/// Blocks are listed in canonical order: modules in file order, each
/// module's top-level blocks by ID, then nested blocks parents first (value
/// inputs, statement lists, stacks), at any depth.
pub fn security_summary(document: &Document) -> SecuritySummary {
    let mut summary = SecuritySummary::default();
    walk::document(document, |block| {
        if is_raw_cpp(block) {
            summary.raw_cpp_blocks.push(block.id.clone());
        }
        if block.block_type.starts_with(FILE_SYSTEM_PREFIX) {
            summary.file_system_blocks.push(block.id.clone());
        }
    });
    let libraries: BTreeSet<&String> = document.project.build.libraries.iter().collect();
    summary.libraries = libraries.into_iter().cloned().collect();
    summary
}

/// Lower-case hexadecimal text of `bytes`, two digits per byte.
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn is_raw_cpp(block: &Block) -> bool {
    block.block_type.starts_with(RAW_CPP_PREFIX)
}

/// A comma before every list entry but the first.
fn comma(json: &mut String, index: usize) {
    if index > 0 {
        json.push(',');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_lower_case_with_two_digits_per_byte() {
        assert_eq!(hex(&[]), "");
        assert_eq!(hex(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
        let all: Vec<u8> = (0..=255).collect();
        let text = hex(&all);
        assert_eq!(text.len(), 512);
        assert!(
            text.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
    }
}
