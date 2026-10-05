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
//! | [`symbols_in_scope`] | `[SymbolInfo, …]`: what a block may refer to, from the last preview's analysis (06 §6.5) |
//! | [`conversion_table`] | `[{from, to, conversion}, …]`: the analyser's conversion rule for every pair of static types (06 §6.6) |
//! | [`clipboard_make`] | `{ok, payload?, text?, diagnostics}`: the clipboard payload of copied blocks and their C++ (05 §5.12) |
//! | [`paste_prepare`] | `{ok, blocks?, unresolved, diagnostics}`: a payload's blocks with fresh IDs and re-bound references |
//!
//! Diagnostics, generated files, source maps and symbols keep the `b2c-ir`
//! serde shapes, the same as `b2c check --format json`
//! (docs/reference/cli.md) and `@blocks2cpp/ipc-types`. A call with
//! arguments that are not valid for the export itself (not a problem in the
//! project or a payload, which is always reported as diagnostics) returns
//! `{"error": {"kind", "message"}}` instead; see [`FacadeError`].
//!
//! # State
//!
//! [`preview`] keeps the analysis of the document it previewed, and
//! [`symbols_in_scope`] answers from it, so opening a dropdown never re-runs
//! the pipeline. The editor asks only after its own preview has finished, so
//! the answer is about the document it shows. The clipboard exports use the
//! kept analysis when it belongs to the document they are given, and
//! analyse that document otherwise. Each WebAssembly instance has one such
//! [`Session`]; a fresh instance starts without one.
//!
//! # Rules
//!
//! * **Pure:** no I/O, clock, randomness or threads, like the crates it
//!   wraps; the project bytes and clipboard payloads are untrusted and go
//!   through [`b2c_model::load`] and [`b2c_model::load_clipboard`] with every
//!   limit of 05 §5.6 before anything else. Fresh IDs for pasted blocks come
//!   from a seed the editor takes from `crypto.getRandomValues`.
//! * **Never panics:** every result is handled and nothing is indexed
//!   unchecked. Should the module still trap (a bug), the frontend drops the
//!   instance and starts a new one (`resetCore` in `@blocks2cpp/b2c-core-wasm`).
//! * **No hand-written `unsafe`:** the crate denies `unsafe_code` (instead
//!   of the workspace's `forbid`) only because the `#[wasm_bindgen]`
//!   expansion contains `unsafe` glue.
//!
//! The typed functions behind the exports live in [`facade`], [`Session`]
//! and [`scope`].

pub mod args;
pub mod clipboard;
mod cut;
pub mod error;
pub mod facade;
pub mod options;
pub mod scope;
mod session;
mod tree;
mod wire;

use std::cell::RefCell;

use wasm_bindgen::prelude::wasm_bindgen;

pub use args::PasteTarget;
pub use clipboard::{NOT_DECLARED, Unresolved};
pub use error::{FacadeError, Failure};
pub use facade::{
    APP_VERSION, Canonical, GENERATOR_INCOMPLETE, Preview, SOURCE_MAP_VERSION, Stage, VersionInfo,
    canonical_document, load_document, preview_document, version_info,
};
pub use options::{IndentWidth, MAX_OPTIONS_BYTES, PreviewOptions};
pub use scope::{ConversionRow, STATIC_TYPES};
pub use session::{ClipboardMade, Pasted, Session};

thread_local! {
    /// The session of this instance (WebAssembly has one thread; natively,
    /// each thread has its own).
    static SESSION: RefCell<Session> = RefCell::new(Session::new());
}

/// Runs `call` with this thread's session, or returns the internal-error
/// envelope when the session cannot be reached (it is in use by a call that
/// is still running, which the single-threaded editor never does, or the
/// thread is shutting down).
fn with_session(call: impl FnOnce(&mut Session) -> String) -> String {
    let unavailable = || {
        wire::error(&FacadeError::Internal(String::from(
            "the compiler core's state is in use by another call",
        )))
    };
    SESSION
        .try_with(|cell| match cell.try_borrow_mut() {
            Ok(mut session) => call(&mut session),
            Err(_) => unavailable(),
        })
        .unwrap_or_else(|_| unavailable())
}

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
/// preview, with the options `{"indentWidth": 2 | 4}`, and keeps the
/// analysis for [`symbols_in_scope`] and the clipboard exports.
///
/// Unlike a build, the preview carries on through catalog and analyser
/// errors, so the editor always shows as much C++ as it can; see
/// [`preview_document`] for the exact rules. `symbols` lists every symbol of
/// the program and `blockTypes` the static type of each value block. A
/// document that does not load clears the kept analysis. Invalid options
/// give `{"error": {"kind": "invalidOptions", …}}` and change nothing.
#[wasm_bindgen]
pub fn preview(document_json: &str, options_json: &str) -> String {
    match PreviewOptions::from_json(options_json) {
        Ok(options) => with_session(|session| wire::preview(&session.preview(document_json, &options))),
        Err(error) => wire::error(&error),
    }
}

/// The symbols a block may refer to, from the last preview's analysis: a
/// JSON list of `SymbolInfo` (06 §6.5), sorted by name, then ID.
///
/// With `input` absent, or the name of a value input, it is what is visible
/// at the block; with the name of one of the block's statement inputs, what
/// is visible at the start of that list (including a loop's counter and a
/// function's parameters). The list is empty (`[]`) when there has been no
/// successful preview, and for a block the previewed document does not have
/// or the analyser does not reach. See
/// [`b2c_lang::Analysis::symbols_in_scope`] for the exact rules.
#[wasm_bindgen]
#[allow(
    clippy::needless_pass_by_value,
    reason = "wasm-bindgen passes an optional string by value"
)]
pub fn symbols_in_scope(block_id: &str, input: Option<String>) -> String {
    with_session(|session| wire::symbols(&session.symbols_in_scope(block_id, input.as_deref())))
}

/// The analyser's conversion rule ([`b2c_lang::conversion`]) for every
/// pair of static types: a JSON list of
/// `{"from": <type>, "to": <type>, "conversion": "same" | "widening" |
/// "narrowing" | "boolNumber" | "invalid"}`, 49 entries. Only `invalid` is
/// an analyser error; the editor's connection checker refuses exactly those.
#[wasm_bindgen]
pub fn conversion_table() -> String {
    wire::conversion_table(&scope::conversion_table())
}

/// Copies blocks of a document (05 §5.12).
///
/// `block_ids_json` is a JSON list of distinct block IDs of the document,
/// in copy order. Returns `{"ok": true, "payload": "<canonical clipboard
/// JSON>", "text": "<their C++>", "diagnostics": []}`, where `text` is
/// absent when none of the blocks produce code. A document that does not
/// load gives `{"ok": false, "diagnostics": [...]}`; a malformed list or an
/// ID that is not a block of the document gives
/// `{"error": {"kind": "invalidArguments", …}}`. See
/// [`Session::clipboard_make`].
#[wasm_bindgen]
pub fn clipboard_make(document_json: &str, block_ids_json: &str) -> String {
    match args::block_ids(block_ids_json) {
        Ok(ids) => with_session(|session| wire::clipboard_made(&session.clipboard_make(document_json, &ids))),
        Err(error) => wire::error(&error),
    }
}

/// Prepares a clipboard payload for pasting (05 §5.12).
///
/// `target_json` is `{"module": <module ID>, "block": <block ID> | null,
/// "input": <input name> | null}`: the module's canvas (`block` null), the
/// start of a block's statement list or one of its value inputs (`input`),
/// or the place directly after a block (`input` null). `seed_hex` is 64 hex
/// digits of fresh randomness (`crypto.getRandomValues`) from which the new
/// IDs are derived.
///
/// Returns `{"ok": true, "blocks": [...], "unresolved": [{"sym", "name"}],
/// "diagnostics": [...]}`: the blocks to insert, with fresh IDs (none used
/// in the document) and references re-bound by qualified name and kind
/// among the symbols visible at the target; references that find nothing
/// are listed in `unresolved` with a `B2C-E0201` each. A payload or document
/// that does not load gives `{"ok": false, "unresolved": [], "diagnostics":
/// [...]}` with the loader's codes; a malformed target or seed, or a target
/// the document does not have, gives
/// `{"error": {"kind": "invalidArguments", …}}`. See
/// [`Session::paste_prepare`].
#[wasm_bindgen]
pub fn paste_prepare(clipboard_text: &str, document_json: &str, target_json: &str, seed_hex: &str) -> String {
    let arguments = args::paste_target(target_json).and_then(|target| Ok((target, args::seed(seed_hex)?)));
    match arguments {
        Ok((target, seed)) => with_session(|session| {
            wire::pasted(&session.paste_prepare(clipboard_text, document_json, &target, seed))
        }),
        Err(error) => wire::error(&error),
    }
}
