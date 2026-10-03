//! Analysis: lowers a resolved project document to the Semantic AST and checks
//! names, scopes, types and flow (`docs/spec/06-compiler-pipeline.md` §6.4–6.6).
//!
//! * Input: a document that `b2c_catalog::resolve` has checked and completed
//!   (absent inputs filled with catalog defaults). Lowering is keyed by block
//!   type ID (`catalog/core/*.toml`); unknown types are skipped silently
//!   because the resolve stage already reported them. Anything else the
//!   analyser needs but cannot find (a missing name, value or setting) is
//!   reported as `B2C-E0430`, so the function never panics on any input.
//! * Output: a [`Program`] (SAST) that is always produced, plus diagnostics
//!   (`DiagSource::Analyser`, codes `B2C-E02xx` names, `E03xx` types, `E04xx`
//!   structure and flow, `W05xx`/`I05xx` lints; every code is documented in
//!   `docs/reference/diagnostics/analyser.md`). Expressions that have errors
//!   get `Type::Error`, and anything involving `Type::Error` is not reported
//!   again, so one mistake gives one message.
//! * Expression slots are parsed by a precedence-climbing parser over the
//!   token list, with the depth limit `b2c_model::limits::MAX_EXPR_DEPTH` and
//!   the token limit `MAX_EXPR_TOKENS`.
//! * Deterministic: the same document always gives the same program and the
//!   same diagnostics in the same order. Top-level blocks are processed by
//!   block ID, so their order in the file does not matter.
//!
//! The analysis runs in three passes:
//!
//! 1. **Collect** every declaration, including those in disabled or
//!    unattached blocks, so that unresolved references can be explained.
//! 2. **Lower** blocks to the SAST in program order, with C++ block scoping:
//!    references are bound to symbols and every expression is typed.
//! 3. **Flow** checks on each finished item: missing `return`, unreachable
//!    blocks, variables used before they get a value, endless loops. An item
//!    that nests too deeply for the later stages is reported instead.

mod access;
mod codes;
mod collect;
mod flow;
mod lower;
mod messages;
mod nesting;
mod parser;
mod scope;
mod typing;

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
///
/// Never panics: problems in the document become diagnostics, and the program
/// contains everything that could be lowered.
pub fn analyze(document: &Document) -> Analysis {
    lower::run(document)
}
