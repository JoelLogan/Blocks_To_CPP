//! Canonical serialisation and hashing (spec §5.2, §5.11).
//!
//! CONTRACT (implemented in milestone M1):
//! * `to_canonical_json`: UTF-8 JSON, 2-space indentation, `\n` line endings,
//!   one trailing newline; struct keys in declaration order, map keys sorted,
//!   top-level blocks of each module sorted by ID. Saving an unchanged document
//!   gives byte-identical output, and `load(to_canonical_json(d)) == d`.
//! * `content_hash`: SHA-256 of the canonical serialisation with layout-only
//!   data removed (block `x`/`y`/`collapsed`, comment `pinned`, workspace
//!   `frames`/`notes`/`viewport`), so moving blocks never changes the hash.

use crate::Document;

/// Serialises a document canonically.
pub fn to_canonical_json(document: &Document) -> String {
    let _ = document;
    todo!("implemented in milestone M1")
}

/// SHA-256 of the document's semantic content.
pub fn content_hash(document: &Document) -> [u8; 32] {
    let _ = document;
    todo!("implemented in milestone M1")
}
