//! Loading, validating and saving `.b2c` project files
//! (`docs/spec/05-project-format.md`).
//!
//! * [`document`]: the typed Block Document Model.
//! * [`load()`]: parse untrusted bytes with every limit and rule of §5.6.
//! * [`to_canonical_json`]: deterministic serialisation (§5.2).
//! * [`content_hash`]: SHA-256 of the semantic content (§5.11).
//! * [`load_clipboard`] and [`to_canonical_clipboard_json`]: the clipboard
//!   format (§5.12), validated exactly like a project file.
//! * [`remap_ids`] and [`SeededIds`]: fresh block and symbol IDs for pasted
//!   and duplicated blocks.
//! * [`security_hash`] and [`security_summary`]: what workspace trust looks
//!   at (spec §8.3).
//! * [`limits`]: the validation limits.
//!
//! Pure functions only: no I/O, no clock, no randomness; compiles for
//! `wasm32-unknown-unknown`.

mod clipboard;
mod codes;
mod decode;
pub mod document;
mod ids;
mod json;
pub mod limits;
mod load;
mod migrate;
mod save;
mod security;
mod text_rules;
mod walk;

pub use clipboard::{
    CLIPBOARD_FORMAT_TAG, CLIPBOARD_FORMAT_VERSION, Clipboard, ClipboardRef, RefKind, load_clipboard,
    to_canonical_clipboard_json,
};
pub use document::*;
pub use ids::{
    IdKind, IdSource, MAX_ID_ATTEMPTS, RemapError, SeededIds, outside_refs, remap_ids, rewrite_refs, used_ids,
};
pub use load::{LoadError, load};
pub use save::{content_hash, to_canonical_json};
pub use security::{SecuritySummary, hex, security_hash, security_summary};
