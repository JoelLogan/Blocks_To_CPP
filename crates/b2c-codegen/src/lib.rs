//! Code generation: the Semantic AST to readable, deterministic C++ with
//! source maps (`docs/spec/06-compiler-pipeline.md` §6.7–6.11).
//!
//! CONTRACT (implemented in milestone M1):
//! * `generate` never panics on any `Program`. It is meant for programs whose
//!   analysis reported no errors; for others it still produces best-effort
//!   output (e.g. `0 /* error */` for expressions of `Type::Error`).
//! * All user text reaches the output only through `b2c_ir::text` encoders;
//!   output is re-printed from the AST, never spliced from input strings.
//! * Deterministic: identical input and options give byte-identical output.
//! * Output files: `<module>.cpp` per module; support helpers inline in the
//!   single-module case (spec §3.9). Every file ends with one newline and has
//!   no trailing whitespace.

use b2c_ir::sast::Program;
use b2c_ir::source_map::GeneratedProject;

/// Where support helpers (`b2c::random_int`, `b2c::ask`, …) are emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HelperPlacement {
    /// In a delimited section at the top of the file that uses them.
    #[default]
    Inline,
    /// In a shared `b2c_support.hpp` header.
    Header,
}

/// Options for code generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenOptions {
    /// Project name, shown in the file header comment.
    pub project_name: String,
    /// App version, shown in the file header comment.
    pub app_version: String,
    /// Add the "Edit the blocks, not this file" line (omitted for export).
    pub do_not_edit_banner: bool,
    /// Spaces per indentation level.
    pub indent_width: u8,
    /// Where support helpers go.
    pub helper_placement: HelperPlacement,
}

impl Default for CodegenOptions {
    fn default() -> Self {
        Self {
            project_name: String::from("Untitled"),
            app_version: String::from(env!("CARGO_PKG_VERSION")),
            do_not_edit_banner: true,
            indent_width: 4,
            helper_placement: HelperPlacement::Inline,
        }
    }
}

/// Generates C++ files and the source map for a program.
pub fn generate(program: &Program, options: &CodegenOptions) -> GeneratedProject {
    let _ = (program, options);
    todo!("implemented in milestone M1")
}
