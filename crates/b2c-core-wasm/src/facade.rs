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
//!
//! The preview also reports what the analysis knows for the editor: every
//! symbol and the static type of each value block. The analysis itself is
//! kept by a [`crate::Session`] for the scope query and the clipboard;
//! [`preview_document`] keeps nothing.

use std::collections::BTreeMap;

use b2c_codegen::{CodegenOptions, HelperPlacement};
use b2c_ir::source_map::{GeneratedFile, SourceMap};
use b2c_ir::{BlockId, DiagSource, Diagnostic, Location, SymbolInfo, Type, has_errors};
use b2c_lang::Analysis;
use b2c_model::{Document, LoadError};
use serde::Serialize;

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
        hash: b2c_model::hex(&b2c_model::content_hash(&document)),
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
    /// The static type of each value block of the program, by block ID
    /// ([`b2c_lang::Analysis::block_types`]): what the connection checker
    /// and the `var.get`/`func.call` output types use. Blocks outside the
    /// program (disabled or loose) are not listed; empty when loading
    /// failed.
    pub block_types: BTreeMap<BlockId, Type>,
    /// Every symbol of the program, sorted by name, then ID
    /// ([`b2c_lang::Analysis::symbol_infos`]); empty when loading failed.
    pub symbols: Vec<SymbolInfo>,
}

impl Preview {
    pub(crate) fn load_failed(diagnostics: Vec<Diagnostic>) -> Self {
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
    match run_pipeline(document_json.as_bytes(), *options) {
        Ok(run) => run.preview,
        Err(error) => Preview::load_failed(error.diagnostics),
    }
}

/// A preview with the analysis behind it, which the [`crate::Session`]
/// keeps for the scope query and the clipboard.
#[derive(Debug, Clone)]
pub(crate) struct PipelineRun {
    /// What the preview shows.
    pub(crate) preview: Preview,
    /// The analysis of the resolved document (its diagnostics are moved
    /// into [`Preview::diagnostics`], so they are empty here).
    pub(crate) analysis: Analysis,
    /// The content hash of the loaded document.
    pub(crate) content_hash: [u8; 32],
    /// The options the files were generated with.
    pub(crate) codegen_options: CodegenOptions,
}

/// The options a build would generate code with for this project, at the
/// given indent width.
pub(crate) fn codegen_options(project_name: &str, options: PreviewOptions) -> CodegenOptions {
    CodegenOptions {
        project_name: project_name.to_owned(),
        app_version: String::from(APP_VERSION),
        do_not_edit_banner: true,
        indent_width: options.indent_width.spaces(),
        helper_placement: HelperPlacement::Inline,
    }
}

/// Resolves and analyses a loaded document (stages ② to ⑥), returning the
/// analysis and the catalog's diagnostics.
pub(crate) fn analyse(loaded: &Document) -> (Document, Vec<Diagnostic>, Analysis) {
    let (document, diagnostics) = b2c_catalog::resolve(loaded, b2c_catalog::core_catalog());
    let analysis = b2c_lang::analyze(&document);
    (document, diagnostics, analysis)
}

/// The whole preview pipeline on untrusted bytes (see [`preview_document`]).
///
/// # Errors
/// The loader's problems when the bytes are not a valid project.
pub(crate) fn run_pipeline(bytes: &[u8], options: PreviewOptions) -> Result<PipelineRun, LoadError> {
    let loaded = b2c_model::load(bytes)?;
    let content_hash = b2c_model::content_hash(&loaded);

    let (document, mut diagnostics, mut analysis) = analyse(&loaded);
    let mut first_failure = has_errors(&diagnostics).then_some(Stage::Resolve);
    diagnostics.append(&mut analysis.diagnostics);
    if first_failure.is_none() && has_errors(&diagnostics) {
        first_failure = Some(Stage::Analyze);
    }

    let codegen_options = codegen_options(&document.project.name, options);
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

    let preview = Preview {
        stage,
        diagnostics,
        files: generation.project.files,
        source_map: Some(generation.project.source_map),
        buildable,
        placeholders: generation.placeholders,
        content_hash: Some(b2c_model::hex(&content_hash)),
        block_types: analysis.block_types(),
        symbols: analysis.symbol_infos(),
    };
    Ok(PipelineRun {
        preview,
        analysis,
        content_hash,
        codegen_options,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO: &str = include_str!("../../../examples/hello_world.b2c");

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
        // hello_world declares nothing and has no value blocks.
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
        assert!(preview.symbols.is_empty());
        assert!(preview.block_types.is_empty());
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
