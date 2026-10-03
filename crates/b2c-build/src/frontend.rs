//! The pure front half of the pipeline: project bytes to generated C++
//! (`docs/spec/06-compiler-pipeline.md` §6.1, stages ①–⑦).
//!
//! Nothing here touches the file system or spawns a process, so the result
//! depends only on the input bytes and the options.

use b2c_codegen::{CodegenOptions, HelperPlacement};
use b2c_ir::sast::Program;
use b2c_ir::source_map::GeneratedProject;
use b2c_ir::{DiagSource, Diagnostic, Location};
use b2c_model::Document;

/// The generator needed error placeholders for a program the analyser
/// accepted (docs/reference/diagnostics/generator.md).
pub const GENERATOR_INCOMPLETE: &str = "B2C-E0701";

/// How far the pipeline got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// The bytes were not a valid project; nothing else ran.
    Load,
    /// The project uses blocks the catalog rejects; analysis did not run.
    Resolve,
    /// Analysis reported errors; no C++ was generated.
    Analyze,
    /// Code generation ran (`generated` is absent if it reported an error).
    Generate,
}

/// Options for the front half of the pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontendOptions {
    /// Add the "Edit the blocks, not this file" banner (off for export).
    pub do_not_edit_banner: bool,
    /// Where support helpers go.
    pub helper_placement: HelperPlacement,
}

impl Default for FrontendOptions {
    fn default() -> Self {
        Self {
            do_not_edit_banner: true,
            helper_placement: HelperPlacement::Inline,
        }
    }
}

/// Everything the front half produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Frontend {
    /// The last stage that ran to completion.
    pub stage: Stage,
    /// The loaded document with catalog defaults filled in (absent when
    /// loading failed).
    pub document: Option<Document>,
    /// The analysed program (absent when loading or resolving failed).
    pub program: Option<Program>,
    /// The generated C++ (present only when no stage reported an error).
    pub generated: Option<GeneratedProject>,
    /// Every diagnostic from every stage that ran, in pipeline order.
    pub diagnostics: Vec<Diagnostic>,
}

impl Frontend {
    /// Whether any stage reported an error.
    pub fn has_errors(&self) -> bool {
        b2c_ir::has_errors(&self.diagnostics)
    }
}

/// Runs load → resolve → analyse → generate on untrusted project bytes.
///
/// Each stage runs only when the previous ones reported no errors, so one
/// mistake is not reported again by every later stage. Generated C++ is
/// produced only for a project without errors (spec §6.1: building requires
/// stages ①–⑤ to report zero errors).
pub fn run_frontend(bytes: &[u8], options: &FrontendOptions) -> Frontend {
    let loaded = match b2c_model::load(bytes) {
        Ok(document) => document,
        Err(error) => {
            return Frontend {
                stage: Stage::Load,
                document: None,
                program: None,
                generated: None,
                diagnostics: error.diagnostics,
            };
        }
    };

    let (document, mut diagnostics) = b2c_catalog::resolve(&loaded, b2c_catalog::core_catalog());
    if b2c_ir::has_errors(&diagnostics) {
        return Frontend {
            stage: Stage::Resolve,
            document: Some(document),
            program: None,
            generated: None,
            diagnostics,
        };
    }

    let analysis = b2c_lang::analyze(&document);
    diagnostics.extend(analysis.diagnostics);
    if b2c_ir::has_errors(&diagnostics) {
        return Frontend {
            stage: Stage::Analyze,
            document: Some(document),
            program: Some(analysis.program),
            generated: None,
            diagnostics,
        };
    }

    let codegen_options = CodegenOptions {
        project_name: document.project.name.clone(),
        app_version: String::from(env!("CARGO_PKG_VERSION")),
        do_not_edit_banner: options.do_not_edit_banner,
        indent_width: 4,
        helper_placement: options.helper_placement,
    };
    let generation = b2c_codegen::generate_with_report(&analysis.program, &codegen_options);
    // Code with placeholders compiles but does not do what the blocks say, so
    // it is never handed on (spec §7.5.3 "generator bug detection").
    let generated = if generation.placeholders == 0 {
        Some(generation.project)
    } else {
        diagnostics.push(Diagnostic::error(
            GENERATOR_INCOMPLETE,
            DiagSource::Generator,
            Location::project(),
            "Part of this program could not be turned into C++, so it was not built. This looks like a bug \
             in Blocks2Cpp; please report it with the project file.",
        ));
        None
    };
    Frontend {
        stage: Stage::Generate,
        document: Some(document),
        program: Some(analysis.program),
        generated,
        diagnostics,
    }
}
