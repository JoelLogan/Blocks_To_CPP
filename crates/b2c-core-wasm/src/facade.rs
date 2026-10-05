//! The typed, natively testable functions behind the exports.
//!
//! [`preview_document`] re-composes the front half of the pipeline exactly
//! as `b2c_build::run_frontend` does (this crate may not depend on
//! `b2c-build`): `b2c_model::load`, `b2c_catalog::resolve` against the core
//! catalog, `b2c_lang::analyze`, then `b2c_codegen::generate_with_report`
//! with the same [`CodegenOptions`] (the workspace version as the app
//! version, the "do not edit" banner on, support helpers inline), so an
//! error-free project previews byte for byte as it builds. Two conformance
//! tests guard that: the generated `main.cpp` of every example equals its
//! golden file (`tests/golden/`, written by the build's tests), and the
//! backend compares preview and build output at both indent widths.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use b2c_codegen::{CodegenOptions, HelperPlacement};
use b2c_ir::source_map::{GeneratedFile, SourceMap};
use b2c_ir::{BlockId, DiagSource, Diagnostic, Location, Type, has_errors};
use b2c_model::{Document, LoadError};
use serde::{Serialize, Serializer};

use crate::options::PreviewOptions;

/// The app version written into generated file headers: the workspace
/// version, the same value `b2c-build` uses.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The source-map format version `b2c-codegen` writes.
pub const SOURCE_MAP_VERSION: u32 = 1;

/// The generator needed error placeholders for a program that no earlier
/// stage found a problem in (docs/reference/diagnostics/generator.md). The
/// same code and message as `b2c_build::GENERATOR_INCOMPLETE`.
pub const GENERATOR_INCOMPLETE: &str = "B2C-E0701";

const GENERATOR_INCOMPLETE_MESSAGE: &str = "Part of this program could not be turned into C++, so it was not \
     built. This looks like a bug in Blocks2Cpp; please report it with the project file.";

/// What [`version_info`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    /// The app version ([`APP_VERSION`]).
    pub app: &'static str,
    /// The catalog version (`b2c_catalog::CATALOG_VERSION`).
    pub catalog: &'static str,
    /// The project format version this core reads and writes.
    pub format_version: u32,
    /// The source-map format version ([`SOURCE_MAP_VERSION`]).
    pub source_map_version: u32,
}

/// The versions this core implements.
pub fn version_info() -> VersionInfo {
    VersionInfo {
        app: APP_VERSION,
        catalog: b2c_catalog::CATALOG_VERSION,
        format_version: b2c_model::CURRENT_FORMAT_VERSION,
        source_map_version: SOURCE_MAP_VERSION,
    }
}

/// Loads untrusted project bytes: [`b2c_model::load`], with every limit and
/// rule of 05 §5.6 and the format migrations. The document is not resolved
/// against the catalog.
///
/// # Errors
/// Every problem found, as `B2C-E01xx` loader diagnostics.
pub fn load_document(bytes: &[u8]) -> Result<Document, LoadError> {
    b2c_model::load(bytes)
}

/// A document's canonical serialisation and content hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canonical {
    /// The canonical file text (05 §5.2), exactly what a save writes.
    pub text: String,
    /// The content hash (05 §5.11) as 64 lower-case hex digits.
    pub hash: String,
}

/// Loads a document given as JSON text and serialises it canonically.
///
/// # Errors
/// The loader's problems when the text is not a valid project.
pub fn canonical_document(document_json: &str) -> Result<Canonical, LoadError> {
    let document = b2c_model::load(document_json.as_bytes())?;
    Ok(Canonical {
        text: b2c_model::to_canonical_json(&document),
        hash: hex(&b2c_model::content_hash(&document)),
    })
}

/// The first stage that reported an error, or [`Stage::Generate`] when none
/// did: the stage at which a build (`b2c_build::run_frontend`) would stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// The text is not a valid project; nothing else ran and there are no
    /// files.
    Load,
    /// The catalog rejected some blocks.
    Resolve,
    /// The analyser reported errors.
    Analyze,
    /// No earlier stage reported an error (generation itself may still have
    /// needed placeholders, reported as [`GENERATOR_INCOMPLETE`]).
    Generate,
}

/// The element type of [`Preview::symbols`] until the scope query arrives
/// (milestone M2, wave 2): it has no values, so the list is always empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoSymbol {}

impl Serialize for NoSymbol {
    fn serialize<S: Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        match *self {}
    }
}

/// Everything the live preview shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// The first stage that reported an error (see [`Stage`]).
    pub stage: Stage,
    /// Every diagnostic of every stage, in pipeline order.
    pub diagnostics: Vec<Diagnostic>,
    /// The generated files; empty only when loading failed.
    pub files: Vec<GeneratedFile>,
    /// The map from generated text back to blocks; absent only when loading
    /// failed.
    pub source_map: Option<SourceMap>,
    /// Whether a build would accept the generated code: no diagnostic is an
    /// error and the generator needed no placeholders.
    pub buildable: bool,
    /// How many error placeholders (`0 /* error */`, …) the files contain.
    pub placeholders: usize,
    /// The content hash of the loaded document (the `hash` of
    /// [`canonical_document`]); absent only when loading failed.
    pub content_hash: Option<String>,
    /// Static types of value blocks. Always empty until the scope query
    /// arrives (milestone M2, wave 2).
    pub block_types: BTreeMap<BlockId, Type>,
    /// The program's symbols. Always empty until the scope query arrives
    /// (milestone M2, wave 2).
    pub symbols: Vec<NoSymbol>,
}

impl Preview {
    fn load_failed(diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            stage: Stage::Load,
            diagnostics,
            files: Vec::new(),
            source_map: None,
            buildable: false,
            placeholders: 0,
            content_hash: None,
            block_types: BTreeMap::new(),
            symbols: Vec::new(),
        }
    }
}

/// Runs load → resolve → analyse → generate for the live preview.
///
/// The composition and the [`CodegenOptions`] are those of a build
/// (`b2c_build::run_frontend`), with the indent width from `options`. The
/// one difference: a build stops at the first stage that reports an error,
/// while the preview, once the document has loaded, carries on through
/// catalog and analyser errors and always generates best-effort C++ (with
/// `/* error */` placeholders where something is wrong), so the editor can
/// show as much code as possible (06 §6.1). [`Preview::stage`] says where a
/// build would have stopped, and [`Preview::buildable`] whether it would
/// build. [`GENERATOR_INCOMPLETE`] is added exactly when a build would add
/// it: no earlier stage reported an error and the generator still needed
/// placeholders.
///
/// Only a load failure gives no files. Never panics: the stages it calls
/// turn every problem in the document into diagnostics.
pub fn preview_document(document_json: &str, options: &PreviewOptions) -> Preview {
    let loaded = match b2c_model::load(document_json.as_bytes()) {
        Ok(document) => document,
        Err(error) => return Preview::load_failed(error.diagnostics),
    };
    let content_hash = hex(&b2c_model::content_hash(&loaded));

    let (document, mut diagnostics) = b2c_catalog::resolve(&loaded, b2c_catalog::core_catalog());
    let mut first_failure = has_errors(&diagnostics).then_some(Stage::Resolve);

    let analysis = b2c_lang::analyze(&document);
    diagnostics.extend(analysis.diagnostics);
    if first_failure.is_none() && has_errors(&diagnostics) {
        first_failure = Some(Stage::Analyze);
    }

    let codegen_options = CodegenOptions {
        project_name: document.project.name.clone(),
        app_version: String::from(APP_VERSION),
        do_not_edit_banner: true,
        indent_width: options.indent_width.spaces(),
        helper_placement: HelperPlacement::Inline,
    };
    let generation = b2c_codegen::generate_with_report(&analysis.program, &codegen_options);
    let stage = first_failure.unwrap_or_else(|| {
        if generation.placeholders > 0 {
            diagnostics.push(Diagnostic::error(
                GENERATOR_INCOMPLETE,
                DiagSource::Generator,
                Location::project(),
                GENERATOR_INCOMPLETE_MESSAGE,
            ));
        }
        Stage::Generate
    });
    let buildable = !has_errors(&diagnostics) && generation.placeholders == 0;

    Preview {
        stage,
        diagnostics,
        files: generation.project.files,
        source_map: Some(generation.project.source_map),
        buildable,
        placeholders: generation.placeholders,
        content_hash: Some(content_hash),
        block_types: BTreeMap::new(),
        symbols: Vec::new(),
    }
}

/// Lower-case hex digits of a byte string.
pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        // Writing to a String cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO: &str = include_str!("../../../examples/hello_world.b2c");

    #[test]
    fn hex_is_lower_case_and_two_digits_per_byte() {
        assert_eq!(hex(&[]), "");
        assert_eq!(hex(&[0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
    }

    #[test]
    fn version_matches_the_crates() {
        let info = version_info();
        assert_eq!(info.app, "0.1.0");
        assert_eq!(info.catalog, b2c_catalog::CATALOG_VERSION);
        assert_eq!(info.format_version, 1);
        assert_eq!(info.source_map_version, 1);
    }

    #[test]
    fn source_map_version_matches_codegen() {
        let preview = preview_document(HELLO, &PreviewOptions::default());
        assert_eq!(
            preview.source_map.map(|map| map.version),
            Some(SOURCE_MAP_VERSION)
        );
    }

    #[test]
    fn hello_world_previews_cleanly() {
        let preview = preview_document(HELLO, &PreviewOptions::default());
        assert_eq!(preview.stage, Stage::Generate);
        assert!(preview.diagnostics.is_empty(), "{:?}", preview.diagnostics);
        assert!(preview.buildable);
        assert_eq!(preview.placeholders, 0);
        assert_eq!(preview.files.len(), 1);
        assert!(preview.block_types.is_empty());
        assert!(preview.symbols.is_empty());
        let canonical = canonical_document(HELLO).unwrap();
        assert_eq!(preview.content_hash, Some(canonical.hash));
        assert_eq!(canonical.text, HELLO);
    }

    #[test]
    fn load_failure_gives_no_files() {
        let preview = preview_document("{", &PreviewOptions::default());
        assert_eq!(preview.stage, Stage::Load);
        assert!(preview.files.is_empty());
        assert!(preview.source_map.is_none());
        assert!(preview.content_hash.is_none());
        assert!(!preview.buildable);
        assert_eq!(preview.diagnostics.len(), 1);
        assert!(load_document(b"{").is_err());
        assert!(canonical_document("{").is_err());
    }

    #[test]
    fn indent_width_changes_only_indentation() {
        let four = preview_document(HELLO, &PreviewOptions::default());
        let two = preview_document(
            HELLO,
            &PreviewOptions {
                indent_width: crate::IndentWidth::Two,
            },
        );
        let four_text = &four.files.first().unwrap().contents;
        let two_text = &two.files.first().unwrap().contents;
        assert!(four_text.contains("\n    std::cout"), "{four_text}");
        assert!(two_text.contains("\n  std::cout"), "{two_text}");
        assert!(!two_text.contains("\n    std::cout"), "{two_text}");
        assert_eq!(four.content_hash, two.content_hash);
    }
}
