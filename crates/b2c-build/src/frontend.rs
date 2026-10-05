//! The pure front half of the pipeline: project bytes to generated C++
//! (`docs/spec/06-compiler-pipeline.md` §6.1, stages ①–⑦).
//!
//! Nothing here touches the file system or spawns a process, so the result
//! depends only on the input bytes and the options.
//!
//! The editor's live preview (`b2c-core-wasm`) composes the same stages with
//! the same [`CodegenOptions`], so the code view, the source map and what is
//! compiled are the same text (01 P3, 06 §6.1 invariant 4). Every option
//! here that changes the generated code is part of the build folder's
//! options hash (07 §7.5.1).

use b2c_codegen::{CodegenOptions, HelperPlacement};
use b2c_ir::sast::Program;
use b2c_ir::source_map::GeneratedProject;
use b2c_ir::{DiagSource, Diagnostic, Location};
use b2c_model::Document;

/// The generator needed error placeholders for a program the analyser
/// accepted (docs/reference/diagnostics/generator.md).
pub const GENERATOR_INCOMPLETE: &str = "B2C-E0701";

/// The indent widths the code style allows, in spaces (04 §4.3; tabs and
/// brace styles come later).
pub const INDENT_WIDTHS: [u8; 2] = [2, 4];

/// The default indent width, and the one the command-line tool always uses
/// (07 §7.9).
pub const DEFAULT_INDENT_WIDTH: u8 = 4;

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
    /// Spaces per indentation level: 2 or 4 ([`INDENT_WIDTHS`]; the app's
    /// code style setting). Any other value is treated as
    /// [`DEFAULT_INDENT_WIDTH`]; see [`FrontendOptions::indent`].
    pub indent_width: u8,
}

impl FrontendOptions {
    /// The indent width actually used: [`Self::indent_width`] when it is one
    /// of [`INDENT_WIDTHS`], otherwise [`DEFAULT_INDENT_WIDTH`].
    pub fn indent(&self) -> u8 {
        if INDENT_WIDTHS.contains(&self.indent_width) {
            self.indent_width
        } else {
            DEFAULT_INDENT_WIDTH
        }
    }
}

impl Default for FrontendOptions {
    fn default() -> Self {
        Self {
            do_not_edit_banner: true,
            helper_placement: HelperPlacement::Inline,
            indent_width: DEFAULT_INDENT_WIDTH,
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
    /// The content hash of the loaded document (`b2c_model::content_hash`,
    /// 05 §5.11), before catalog defaults were filled in: the `projectHash`
    /// of a build. Absent when loading failed.
    pub project_hash: Option<[u8; 32]>,
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
                project_hash: None,
                program: None,
                generated: None,
                diagnostics: error.diagnostics,
            };
        }
    };
    let project_hash = Some(b2c_model::content_hash(&loaded));

    let (document, mut diagnostics) = b2c_catalog::resolve(&loaded, b2c_catalog::core_catalog());
    if b2c_ir::has_errors(&diagnostics) {
        return Frontend {
            stage: Stage::Resolve,
            document: Some(document),
            project_hash,
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
            project_hash,
            program: Some(analysis.program),
            generated: None,
            diagnostics,
        };
    }

    let codegen_options = CodegenOptions {
        project_name: document.project.name.clone(),
        app_version: String::from(env!("CARGO_PKG_VERSION")),
        do_not_edit_banner: options.do_not_edit_banner,
        indent_width: options.indent(),
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
        project_hash,
        program: Some(analysis.program),
        generated,
        diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO: &[u8] = include_bytes!("../../../examples/hello_world.b2c");

    fn main_cpp(frontend: &Frontend) -> &str {
        let generated = frontend.generated.as_ref().expect("generated");
        &generated
            .files
            .iter()
            .find(|file| file.path == "main.cpp")
            .expect("main.cpp")
            .contents
    }

    #[test]
    fn the_indent_width_reaches_the_generator() {
        let four = run_frontend(HELLO, &FrontendOptions::default());
        let two = run_frontend(
            HELLO,
            &FrontendOptions {
                indent_width: 2,
                ..FrontendOptions::default()
            },
        );
        assert!(main_cpp(&four).contains("\n    "), "{}", main_cpp(&four));
        assert!(!main_cpp(&two).contains("\n    "), "{}", main_cpp(&two));
        assert!(main_cpp(&two).contains("\n  "), "{}", main_cpp(&two));
        // The source maps follow the text.
        assert_ne!(
            four.generated.as_ref().map(|g| &g.source_map),
            two.generated.as_ref().map(|g| &g.source_map)
        );
        // The content hash is of the project, not of the generated code.
        assert_eq!(four.project_hash, two.project_hash);
    }

    #[test]
    fn unsupported_indent_widths_fall_back_to_four() {
        for width in [0, 1, 3, 5, 8, u8::MAX] {
            let options = FrontendOptions {
                indent_width: width,
                ..FrontendOptions::default()
            };
            assert_eq!(options.indent(), DEFAULT_INDENT_WIDTH, "{width}");
        }
        for width in INDENT_WIDTHS {
            let options = FrontendOptions {
                indent_width: width,
                ..FrontendOptions::default()
            };
            assert_eq!(options.indent(), width);
        }
        let odd = run_frontend(
            HELLO,
            &FrontendOptions {
                indent_width: 3,
                ..FrontendOptions::default()
            },
        );
        assert_eq!(
            main_cpp(&odd),
            main_cpp(&run_frontend(HELLO, &FrontendOptions::default()))
        );
    }

    #[test]
    fn the_project_hash_is_the_content_hash_of_the_loaded_document() {
        let frontend = run_frontend(HELLO, &FrontendOptions::default());
        let loaded = b2c_model::load(HELLO).expect("hello world loads");
        assert_eq!(frontend.project_hash, Some(b2c_model::content_hash(&loaded)));
        let broken = run_frontend(b"{", &FrontendOptions::default());
        assert_eq!(broken.stage, Stage::Load);
        assert_eq!(broken.project_hash, None);
    }
}
