//! Analysis: lowers a resolved project document to the Semantic AST and checks
//! names, scopes, types and flow (`docs/spec/06-compiler-pipeline.md` §6.4–6.6).
//!
//! CONTRACT (implemented in milestone M1):
//! * Input: a document that `b2c_catalog::resolve` has checked and completed
//!   (absent inputs filled with catalog defaults). Lowering is keyed by block
//!   type ID (`catalog/core/*.toml`); unknown types are skipped silently
//!   because the resolve stage already reported them.
//! * Output: a `Program` (SAST) that is always produced, plus diagnostics
//!   (`DiagSource::Analyser`, codes `B2C-E02xx`/`E03xx`/`E04xx`/`W05xx`/`I05xx`).
//!   Expressions that have errors get `Type::Error`. Never panics on any input.
//! * Expression slots are parsed by a precedence-climbing parser over the
//!   token list with depth limit `b2c_model::limits::MAX_EXPR_DEPTH`.
//! * Deterministic: the same document always gives the same program and the
//!   same diagnostics in the same order.

use b2c_ir::Diagnostic;
use b2c_ir::sast::Program;
use b2c_model::Document;

/// The result of analysing a document.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    /// The program; complete as far as possible even when there are errors.
    pub program: Program,
    /// Every problem found, in a deterministic order.
    pub diagnostics: Vec<Diagnostic>,
}

/// Analyses a resolved document.
pub fn analyze(document: &Document) -> Analysis {
    let _ = document;
    todo!("implemented in milestone M1")
}
