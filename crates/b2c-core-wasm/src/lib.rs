//! The compiler core for the editor: a thin `wasm-bindgen` facade over the
//! pure compiler crates (`docs/spec/06-compiler-pipeline.md` §6.13,
//! ADR-0003).
//!
//! The editor runs the same Rust code as the backend and the CLI, compiled
//! to WebAssembly, so the live C++ preview cannot drift from what is built.
//! Every export takes strings or bytes and returns one compact JSON string,
//! and the same functions are callable natively (the tests and native tools
//! use them that way):
//!
//! | Export | Returns |
//! |--------|---------|
//! | [`version`] | `{app, catalog, formatVersion, sourceMapVersion}` |
//! | [`load`] | `{ok, document?, diagnostics}`: the loaded document (migrated, not resolved), in canonical key order |
//! | [`canonical`] | `{ok, text?, hash?, diagnostics}`: the canonical file text (05 §5.2) and the content hash (05 §5.11) |
//! | [`preview`] | `{stage, diagnostics, files, sourceMap, buildable, placeholders, contentHash, blockTypes, symbols}` |
//!
//! Diagnostics, generated files and source maps keep the `b2c-ir` serde
//! shapes, the same as `b2c check --format json` (docs/reference/cli.md).
//! A call with arguments that are not valid for the export itself (not a
//! problem in the project, which is always reported as diagnostics) returns
//! `{"error": {"kind", "message"}}` instead; see [`FacadeError`].
//!
//! # Rules
//!
//! * **Pure:** no I/O, clock, randomness or threads, like the crates it
//!   wraps; the project bytes are untrusted and go through
//!   [`b2c_model::load`] with every limit of 05 §5.6 before anything else.
//! * **Never panics:** every result is handled and nothing is indexed
//!   unchecked. Should the module still trap (a bug), the frontend drops the
//!   instance and starts a new one (`resetCore` in `@blocks2cpp/b2c-core-wasm`).
//! * **No hand-written `unsafe`:** the crate denies `unsafe_code` (instead
//!   of the workspace's `forbid`) only because the `#[wasm_bindgen]`
//!   expansion contains `unsafe` glue.
//!
//! The typed functions behind the exports live in [`facade`].

pub mod error;
pub mod facade;
pub mod options;
mod wire;

use wasm_bindgen::prelude::wasm_bindgen;

pub use error::FacadeError;
pub use facade::{
    APP_VERSION, Canonical, GENERATOR_INCOMPLETE, NoSymbol, Preview, SOURCE_MAP_VERSION, Stage, VersionInfo,
    canonical_document, load_document, preview_document, version_info,
};
pub use options::{IndentWidth, MAX_OPTIONS_BYTES, PreviewOptions};

/// The versions of the app, the catalog, the project format and the source
/// map format this core implements:
/// `{"app": "0.1.0", "catalog": "1.0.0", "formatVersion": 1, "sourceMapVersion": 1}`.
#[wasm_bindgen]
pub fn version() -> String {
    wire::version(&version_info())
}

/// Loads untrusted project bytes (stage ①, 05 §5.6).
///
/// Returns `{"ok": true, "document": {…}, "diagnostics": []}` with the
/// document migrated to the current format (but not resolved against the
/// catalog) in canonical key order, or `{"ok": false, "diagnostics": [...]}`
/// with the `B2C-E01xx` problems.
#[wasm_bindgen]
pub fn load(bytes: &[u8]) -> String {
    wire::load(&load_document(bytes))
}

/// Loads a document (as JSON text) and returns its canonical serialisation
/// and content hash: `{"ok": true, "text": "…", "hash": "<64 hex digits>",
/// "diagnostics": []}`, or `{"ok": false, "diagnostics": [...]}`.
///
/// The text is what a save writes (05 §5.2) and the hash is the dirty-check
/// and build identity (05 §5.11).
#[wasm_bindgen]
pub fn canonical(document_json: &str) -> String {
    wire::canonical(&canonical_document(document_json))
}

/// Runs the whole front half of the pipeline (stages ①–⑦) for the live
/// preview, with the options `{"indentWidth": 2 | 4}`.
///
/// Unlike a build, the preview carries on through catalog and analyser
/// errors, so the editor always shows as much C++ as it can; see
/// [`preview_document`] for the exact rules. Invalid options give
/// `{"error": {"kind": "invalidOptions", …}}`.
#[wasm_bindgen]
pub fn preview(document_json: &str, options_json: &str) -> String {
    match PreviewOptions::from_json(options_json) {
        Ok(options) => wire::preview(&preview_document(document_json, &options)),
        Err(error) => wire::error(&error),
    }
}
