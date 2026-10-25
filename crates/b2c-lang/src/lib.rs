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
//!   again, so one mistake gives one message. [`analyze_checked`], which the
//!   build and the editor's preview run, does not repeat as `B2C-E0430` what
//!   the catalog check already reported either.
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
//!
//! For the editor, an [`Analysis`] also answers questions about the blocks
//! (spec §6.5): [`Analysis::symbols_in_scope`] lists the symbols a block may
//! refer to, [`Analysis::symbol_infos`] every symbol, and
//! [`Analysis::block_types`] the static type of each value block
//! ([`Analysis::slot_types`] that of each expression slot). Together
//! with [`conversion`], the analyser's own conversion rule (see its type,
//! [`Conversion`], for the sites it covers), they let the editor offer and
//! connect only what the analyser accepts.

mod access;
mod codes;
mod collect;
mod flow;
mod lower;
mod messages;
mod nesting;
mod parser;
mod query;
mod scope;
mod slots;
mod typing;

use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::sast::Program;
use b2c_ir::sig::SigTable;
use b2c_ir::{BlockId, DiagSource, Diagnostic, Part, Severity, Type};
use b2c_model::Document;

pub use slots::{Binder, BlockTyping, InputTyping, SlotFit, type_block};
pub use typing::{Conversion, assignment, check_accepts, conversion};

/// The result of analysing a document.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    /// The program; complete as far as possible even when there are errors.
    pub program: Program,
    /// Every problem found, in a deterministic order.
    pub diagnostics: Vec<Diagnostic>,
    /// What is visible where, for [`Analysis::symbols_in_scope`].
    index: query::ScopeIndex,
    /// The static type of each expression slot lowered, for
    /// [`Analysis::slot_types`].
    slot_types: BTreeMap<BlockId, BTreeMap<String, Type>>,
}

impl Analysis {
    /// The static type of each expression slot that was lowered, by block
    /// ID, then input name (ADR-0018 §3); a block's own type is in
    /// [`Analysis::block_types`]. Empty until the analyser records slot types
    /// (package 3 of `docs/plans/m3-contracts/types.md` §8.2).
    pub fn slot_types(&self) -> BTreeMap<BlockId, BTreeMap<String, Type>> {
        self.slot_types.clone()
    }
}

/// Analyses a document, normally one that `b2c_catalog::resolve` completed,
/// with no catalog signatures: every block keeps its M2 typing rules.
///
/// Never panics, also on a document whose resolution failed (the editor's
/// preview analyses those too): problems in the document become
/// diagnostics, and the program contains everything that could be lowered.
pub fn analyze(document: &Document) -> Analysis {
    analyze_with(document, &SigTable::empty())
}

/// [`analyze`] with the catalog's signatures (`b2c_catalog::Catalog::sigs`),
/// which type the slots of the blocks that have one (ADR-0018). Taking the
/// table keeps this crate independent of `b2c-catalog`. A block the table
/// does not know keeps its M2 typing rules, as every block does until the
/// analyser reads the table (package 3 of types.md §8.2).
pub fn analyze_with(document: &Document, sigs: &SigTable) -> Analysis {
    let _ = sigs;
    lower::run(document)
}

/// [`analyze`] for a document the block catalog has checked
/// (`b2c_catalog::resolve_report`), as the build and the editor's preview
/// run it: what the catalog already reported is not reported again, so one
/// mistake gives one message (06 §6.1).
///
/// `catalog` is the catalog's diagnostics, and `unchecked` the blocks it kept
/// as they were without a check (unknown, made by a newer catalog, too old to
/// upgrade, or with a broken definition). A `B2C-E0430` (*incomplete or
/// damaged block*) is left out when it is about
///
/// * a block in `unchecked`: its fields and inputs follow a definition this
///   catalog does not have, so a choice it does not know or an input it left
///   to a default is not a mistake of its own;
/// * a field or input the catalog reported an error at (an invalid choice,
///   a missing input, a field of the wrong kind);
/// * a function input (`params[…]`) of a block the catalog reported an
///   error on as a whole: the function inputs are kept in the block's
///   `extra` settings, whose problems the catalog reports on the whole block.
///
/// Everything else, the program and the scope query included, is exactly
/// what [`analyze`] gives.
pub fn analyze_checked(
    document: &Document,
    catalog: &[Diagnostic],
    unchecked: &BTreeSet<BlockId>,
) -> Analysis {
    analyze_checked_with(document, catalog, unchecked, &SigTable::empty())
}

/// [`analyze_checked`] with the catalog's signatures, as [`analyze_with`]
/// takes them: what the build and the editor's preview run.
pub fn analyze_checked_with(
    document: &Document,
    catalog: &[Diagnostic],
    unchecked: &BTreeSet<BlockId>,
    sigs: &SigTable,
) -> Analysis {
    let mut analysis = analyze_with(document, sigs);
    let reported: BTreeSet<(&BlockId, PartKey<'_>)> = catalog
        .iter()
        .filter(|d| d.source == DiagSource::Catalog && d.severity == Severity::Error)
        .filter_map(|d| Some((d.primary.block.as_ref()?, PartKey::of(&d.primary.part)?)))
        .collect();
    analysis.diagnostics.retain(|diagnostic| {
        if diagnostic.code.0 != codes::INCOMPLETE_BLOCK {
            return true;
        }
        let Some(block) = diagnostic.primary.block.as_ref() else {
            return true;
        };
        if unchecked.contains(block) {
            return false;
        }
        match PartKey::of(&diagnostic.primary.part) {
            Some(PartKey::Field(name)) if name.starts_with("params[") => {
                !reported.contains(&(block, PartKey::Whole))
                    && !reported.contains(&(block, PartKey::Field(name)))
            }
            Some(part) => !reported.contains(&(block, part)),
            None => true,
        }
    });
    analysis
}

/// The part of a block a diagnostic is about, for [`analyze_checked`]
/// (token ranges are left out: the catalog never reports them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PartKey<'a> {
    /// The whole block.
    Whole,
    /// A field.
    Field(&'a str),
    /// A value input or a statement input.
    Input(&'a str),
}

impl<'a> PartKey<'a> {
    /// The key of a diagnostic's part; `None` for a token range.
    fn of(part: &'a Part) -> Option<Self> {
        match part {
            Part::Whole => Some(Self::Whole),
            Part::Field { name } => Some(Self::Field(name)),
            Part::Input { name } => Some(Self::Input(name)),
            Part::Tokens { .. } => None,
        }
    }
}
