//! Copy and paste through the validated clipboard format (05 §5.12):
//! making a payload from selected blocks, and preparing a payload's blocks
//! for insertion into a document.
//!
//! # Qualified names in milestone M2
//!
//! A payload's `refs` record the qualified name and kind of each symbol the
//! copied blocks use but do not declare, so a paste can bind them again by
//! name (06 §6.14.11). Milestone M2 has no namespaces and no shared names,
//! so the rule is:
//!
//! * a variable, parameter or loop counter is recorded by its plain name
//!   (`score`): locals are never qualified;
//! * a function is recorded with the global-namespace qualifier (`::area`),
//!   since every M2 function is a non-shared function of its module's global
//!   namespace. Pasting binds it to the target module's own function of that
//!   name (functions of other modules are not visible, `B2C-E0206`); a
//!   recorded plain `area` is read the same way.
//!
//! A name qualified with a namespace (`geo::area`, from a later version)
//! never matches in M2 and stays unresolved.
//!
//! # What is visible at the paste target
//!
//! A paste target is `{module, block, input}` ([`crate::PasteTarget`]):
//!
//! * no block: the module's canvas, where only the module's functions are
//!   visible;
//! * a block and an input: what the scope query gives there (the start of
//!   that statement list, or inside that value input);
//! * a block without an input: directly after the block in its statement
//!   list, which is what is visible at the block plus the variable the block
//!   itself creates, if it is a `var.declare` the analyser accepted (it hides
//!   any other symbol with its name).
//!
//! A block the analyser does not reach (disabled, loose on the canvas, or in
//! a loose stack) has no scope of its own, so its place sees what the canvas
//! sees. The analysis is the one kept from the last preview when it belongs
//! to the same document (same content hash), otherwise a new one.
//!
//! # How references are bound
//!
//! For each reference of the pasted blocks to a symbol they do not declare:
//!
//! * if the payload names it (`refs`), it binds to the visible symbol with
//!   that qualified name and kind: itself, if it is one of them (a paste next
//!   to the original), otherwise the only one. With none, it keeps its
//!   original symbol if that is visible and of that kind (renamed since the
//!   copy); with none or several otherwise, it is unresolved;
//! * if the payload does not name it, it stays as it is and is unresolved
//!   unless that very symbol is visible.
//!
//! An unresolved reference still refers to its original symbol ID, so the
//! analyser reports it once the blocks are in place (`B2C-E0201`, or
//! `B2C-E0203` when that symbol exists in the document but not there). The
//! paste reports one [`NOT_DECLARED`] per unresolved symbol at its first use
//! in the pasted blocks, naming the original.

use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::diag::Part;
use b2c_ir::{BlockId, DiagSource, Diagnostic, Location, ModuleId, SymbolId, SymbolInfo, SymbolInfoKind};
use b2c_model::limits::MAX_QUALIFIED_NAME_LEN;
use b2c_model::{Block, Clipboard, ClipboardRef, Document, FieldValue, Input, RefKind, Token};
use serde::Serialize;

use crate::error::FacadeError;
use crate::tree;

/// The diagnostic code of a pasted reference that found nothing to bind to:
/// the analyser's "not declared" (docs/reference/diagnostics/analyser.md).
pub const NOT_DECLARED: &str = "B2C-E0201";

/// The longest name quoted in a paste message, in characters; longer names
/// are shortened with `…`.
const MAX_QUOTED_NAME_CHARS: usize = 64;

/// Finds the blocks to copy, in the given order, as copies without a canvas
/// position. A block nested in another listed block is copied once, as part
/// of that block. A top-level block keeps its loose `stack` (ADR-0011).
///
/// # Errors
/// [`FacadeError::InvalidArguments`] when an ID is not a block of the
/// document.
pub(crate) fn select(document: &Document, ids: &[BlockId]) -> Result<Vec<Block>, FacadeError> {
    let wanted: BTreeSet<&BlockId> = ids.iter().collect();
    let mut chosen: BTreeMap<&BlockId, &Block> = BTreeMap::new();
    let mut covered: BTreeSet<&BlockId> = BTreeSet::new();
    let roots = document
        .modules
        .iter()
        .flat_map(|module| &module.workspace.blocks);
    tree::walk_pruned(roots, |block| {
        if !wanted.contains(&block.id) {
            return true;
        }
        chosen.insert(&block.id, block);
        // Everything inside is copied with it: mark the listed IDs below it
        // as found, without copying them again.
        tree::walk(children_of(block), |inner| {
            if wanted.contains(&inner.id) {
                covered.insert(&inner.id);
            }
        });
        false
    });
    let mut blocks = Vec::with_capacity(chosen.len());
    for id in ids {
        if let Some(block) = chosen.get(id) {
            let mut copy = (*block).clone();
            copy.x = None;
            copy.y = None;
            blocks.push(copy);
        } else if !covered.contains(id) {
            return Err(FacadeError::InvalidArguments(format!(
                "the block {id} is not in the document"
            )));
        }
    }
    Ok(blocks)
}

/// The direct children of a block (its nested reporters, its statement
/// lists and its stack).
fn children_of(block: &Block) -> Vec<&Block> {
    let mut children: Vec<&Block> = block
        .inputs
        .values()
        .filter_map(|input| match input {
            Input::Block(nested) => Some(&*nested.block),
            Input::Expr(_) => None,
        })
        .collect();
    children.extend(block.statements.values().flatten());
    children.extend(&block.stack);
    children
}

/// Every symbol declared in a document (also in disabled and loose blocks),
/// with its name as written and its kind: `var.declare` and
/// `control.for_range` declare variables and loop counters, `func.define`
/// declares a function and (in `extra.params`) its parameters, as in the
/// analyser's declaration pass. The first declaration of an ID wins.
pub(crate) fn declarations(document: &Document) -> BTreeMap<SymbolId, (String, RefKind)> {
    let mut found: BTreeMap<SymbolId, (String, RefKind)> = BTreeMap::new();
    let mut record = |sym: &SymbolId, name: &str, kind: RefKind| {
        found
            .entry(sym.clone())
            .or_insert_with(|| (name.to_owned(), kind));
    };
    let roots = document
        .modules
        .iter()
        .flat_map(|module| &module.workspace.blocks);
    tree::walk(roots, |block| {
        let decl = |field: &str| match block.fields.get(field) {
            Some(FieldValue::Decl(decl)) => Some(decl),
            _ => None,
        };
        match block.block_type.as_str() {
            "var.declare" => {
                if let Some(decl) = decl("NAME") {
                    record(&decl.sym, &decl.name, RefKind::Variable);
                }
            }
            "control.for_range" => {
                if let Some(decl) = decl("VAR") {
                    record(&decl.sym, &decl.name, RefKind::LoopVariable);
                }
            }
            "func.define" => {
                if let Some(decl) = decl("NAME") {
                    record(&decl.sym, &decl.name, RefKind::Function);
                }
                let rows = block.extra.get("params").and_then(serde_json::Value::as_array);
                for row in rows.into_iter().flatten() {
                    let sym = row.get("sym").and_then(serde_json::Value::as_str);
                    let name = row.get("name").and_then(serde_json::Value::as_str);
                    if let (Some(sym), Some(name)) = (sym.and_then(|s| SymbolId::new(s).ok()), name) {
                        record(&sym, name, RefKind::Parameter);
                    }
                }
            }
            _ => {}
        }
    });
    found
}

/// The qualified name recorded for a symbol (see the module docs).
pub(crate) fn qualified_name(name: &str, kind: RefKind) -> String {
    match kind {
        RefKind::Function => format!("::{name}"),
        RefKind::Variable | RefKind::Parameter | RefKind::LoopVariable => name.to_owned(),
    }
}

/// The clipboard payload for copied blocks: the blocks, and a `refs` entry
/// for each symbol they use but do not declare whose declaration is in the
/// document. A reference whose declaration is gone, or whose qualified name
/// would be longer than [`MAX_QUALIFIED_NAME_LEN`] characters (never a valid
/// identifier), gets no entry; a paste leaves it as it is.
pub(crate) fn payload(document: &Document, blocks: Vec<Block>) -> Clipboard {
    let declared = declarations(document);
    let mut refs = BTreeMap::new();
    for sym in b2c_model::outside_refs(&blocks) {
        if let Some((name, kind)) = declared.get(&sym) {
            let name = qualified_name(name, *kind);
            if name.chars().count() <= MAX_QUALIFIED_NAME_LEN {
                refs.insert(sym, ClipboardRef { name, kind: *kind });
            }
        }
    }
    Clipboard {
        catalog: String::from(b2c_catalog::CATALOG_VERSION),
        blocks,
        refs,
    }
}

/// A pasted reference that found nothing to bind to: it stays a reference
/// to its original symbol ID, and the analyser reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unresolved {
    /// The symbol ID the pasted blocks still refer to.
    pub sym: SymbolId,
    /// The qualified name the payload recorded for it, or `None` when the
    /// payload did not name it.
    pub name: Option<String>,
}

/// What [`rebind`] decided for each outside reference.
#[derive(Debug, Default)]
pub(crate) struct Binding {
    /// References to bind to another symbol: old ID → new ID.
    pub(crate) rebound: BTreeMap<SymbolId, SymbolId>,
    /// References that stay as they are and are not visible at the target.
    pub(crate) unresolved: Vec<Unresolved>,
}

/// The kind of a visible symbol, as a payload names it.
fn ref_kind(info: &SymbolInfo) -> RefKind {
    match info.kind {
        SymbolInfoKind::Variable { .. } => RefKind::Variable,
        SymbolInfoKind::Parameter { .. } => RefKind::Parameter,
        SymbolInfoKind::LoopVariable => RefKind::LoopVariable,
        SymbolInfoKind::Function { .. } => RefKind::Function,
    }
}

/// Whether a visible symbol is the one a payload's `refs` entry names: the
/// same kind and the same qualified name (see the module docs).
fn names(reference: &ClipboardRef, info: &SymbolInfo) -> bool {
    if ref_kind(info) != reference.kind {
        return false;
    }
    match reference.kind {
        RefKind::Function => reference.name.strip_prefix("::").unwrap_or(&reference.name) == info.name,
        RefKind::Variable | RefKind::Parameter | RefKind::LoopVariable => reference.name == info.name,
    }
}

/// Decides how each reference of pasted blocks to a symbol they do not
/// declare is bound at the target, given the symbols `visible` there:
///
/// * a reference the payload names (`refs`) binds to the visible symbol
///   with that qualified name and kind: itself, if it is one of them (a
///   paste next to the original), otherwise the only one. With none, it
///   keeps its original symbol if that is visible and of that kind (renamed
///   since the copy); otherwise, and with several, it is unresolved;
/// * a reference the payload does not name stays as it is, and is
///   unresolved unless that very symbol is visible.
pub(crate) fn rebind(
    outside: &BTreeSet<SymbolId>,
    refs: &BTreeMap<SymbolId, ClipboardRef>,
    visible: &[SymbolInfo],
) -> Binding {
    let mut binding = Binding::default();
    for sym in outside {
        let itself = visible.iter().find(|info| info.id == *sym);
        let Some(reference) = refs.get(sym) else {
            if itself.is_none() {
                binding.unresolved.push(Unresolved {
                    sym: sym.clone(),
                    name: None,
                });
            }
            continue;
        };
        let named: Vec<&SymbolInfo> = visible.iter().filter(|info| names(reference, info)).collect();
        if named.iter().any(|info| info.id == *sym) {
            continue;
        }
        match named.as_slice() {
            [only] => {
                binding.rebound.insert(sym.clone(), only.id.clone());
            }
            [] if itself.is_some_and(|info| ref_kind(info) == reference.kind) => {}
            _ => binding.unresolved.push(Unresolved {
                sym: sym.clone(),
                name: Some(reference.name.clone()),
            }),
        }
    }
    binding
}

/// Where each symbol is first referred to in the blocks: the block and the
/// field, or the token in an expression slot.
fn first_references(blocks: &[Block], wanted: &BTreeSet<&SymbolId>) -> BTreeMap<SymbolId, (BlockId, Part)> {
    let mut found: BTreeMap<SymbolId, (BlockId, Part)> = BTreeMap::new();
    tree::walk(blocks, |block| {
        for (field, value) in &block.fields {
            if let FieldValue::Ref(reference) = value
                && wanted.contains(&reference.target)
            {
                found
                    .entry(reference.target.clone())
                    .or_insert_with(|| (block.id.clone(), Part::Field { name: field.clone() }));
            }
        }
        for (input, value) in &block.inputs {
            let Input::Expr(expr) = value else {
                continue;
            };
            for (index, token) in expr.expr.iter().enumerate() {
                if let Token::Ref(sym) = token
                    && wanted.contains(sym)
                {
                    let start = u32::try_from(index).unwrap_or(u32::MAX);
                    found.entry(sym.clone()).or_insert_with(|| {
                        (
                            block.id.clone(),
                            Part::Tokens {
                                input: input.clone(),
                                start,
                                end: start.saturating_add(1),
                            },
                        )
                    });
                }
            }
        }
    });
    found
}

/// A name for a message: shortened to [`MAX_QUOTED_NAME_CHARS`]
/// characters. Payload names have passed the loader's text rules, so they
/// hold no control, bidirectional or invisible characters.
fn quoted(name: &str) -> String {
    let mut chars = name.chars();
    let mut shown: String = chars.by_ref().take(MAX_QUOTED_NAME_CHARS).collect();
    if chars.next().is_some() {
        shown.push('…');
    }
    format!("`{shown}`")
}

/// The kind of symbol in words, for messages.
fn kind_words(kind: RefKind) -> &'static str {
    match kind {
        RefKind::Variable => "variable",
        RefKind::Parameter => "parameter",
        RefKind::LoopVariable => "loop counter",
        RefKind::Function => "function",
    }
}

/// One `B2C-E0201` per unresolved reference, at its first use in the pasted
/// blocks, naming the original symbol (05 §5.12).
pub(crate) fn unresolved_diagnostics(
    blocks: &[Block],
    unresolved: &[Unresolved],
    refs: &BTreeMap<SymbolId, ClipboardRef>,
    module: &ModuleId,
) -> Vec<Diagnostic> {
    let wanted: BTreeSet<&SymbolId> = unresolved.iter().map(|u| &u.sym).collect();
    let places = first_references(blocks, &wanted);
    unresolved
        .iter()
        .map(|item| {
            let location = match places.get(&item.sym) {
                Some((block, part)) => Location {
                    module: Some(module.clone()),
                    block: Some(block.clone()),
                    part: part.clone(),
                },
                None => Location {
                    module: Some(module.clone()),
                    block: None,
                    part: Part::Whole,
                },
            };
            let message = match (refs.get(&item.sym), &item.name) {
                (Some(reference), Some(name)) => format!(
                    "The pasted blocks use the {} {}, which doesn't exist here. Choose another one, or create {} first.",
                    kind_words(reference.kind),
                    quoted(name),
                    quoted(name.strip_prefix("::").unwrap_or(name)),
                ),
                _ => String::from(
                    "The pasted blocks use a variable or function that doesn't exist here. Choose another one, or create it first.",
                ),
            };
            Diagnostic::error(NOT_DECLARED, DiagSource::Analyser, location, message)
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // tests fail by panicking
mod tests {
    use b2c_ir::Type;
    use b2c_ir::sast::PassMode;

    use super::*;

    fn sym(id: &str) -> SymbolId {
        SymbolId::new(id).unwrap()
    }

    fn info(id: &str, name: &str, kind: SymbolInfoKind) -> SymbolInfo {
        SymbolInfo {
            id: sym(id),
            name: name.to_owned(),
            kind,
            ty: Type::Int,
            module: ModuleId::new("m").unwrap(),
            decl_block: BlockId::new("b").unwrap(),
        }
    }

    fn variable(id: &str, name: &str) -> SymbolInfo {
        info(id, name, SymbolInfoKind::Variable { is_const: false })
    }

    fn function(id: &str, name: &str) -> SymbolInfo {
        info(
            id,
            name,
            SymbolInfoKind::Function {
                params: Vec::new(),
                returns: Type::Int,
            },
        )
    }

    fn reference(name: &str, kind: RefKind) -> ClipboardRef {
        ClipboardRef {
            name: name.to_owned(),
            kind,
        }
    }

    #[test]
    fn qualified_names() {
        assert_eq!(qualified_name("score", RefKind::Variable), "score");
        assert_eq!(qualified_name("p", RefKind::Parameter), "p");
        assert_eq!(qualified_name("i", RefKind::LoopVariable), "i");
        assert_eq!(qualified_name("area", RefKind::Function), "::area");
        assert!(names(
            &reference("::area", RefKind::Function),
            &function("f", "area")
        ));
        assert!(names(
            &reference("area", RefKind::Function),
            &function("f", "area")
        ));
        assert!(!names(
            &reference("geo::area", RefKind::Function),
            &function("f", "area")
        ));
        assert!(!names(
            &reference("::score", RefKind::Variable),
            &variable("v", "score")
        ));
        assert!(!names(
            &reference("area", RefKind::Variable),
            &function("f", "area")
        ));
        let param = info("p", "count", SymbolInfoKind::Parameter { mode: PassMode::Copy });
        assert!(names(&reference("count", RefKind::Parameter), &param));
        assert!(!names(&reference("count", RefKind::Variable), &param));
        let counter = info("i", "i", SymbolInfoKind::LoopVariable);
        assert!(names(&reference("i", RefKind::LoopVariable), &counter));
    }

    #[test]
    fn rebinding_rules() {
        let outside: BTreeSet<SymbolId> = ["s_same", "s_moved", "s_gone", "s_twice", "s_unnamed", "s_seen"]
            .into_iter()
            .map(sym)
            .collect();
        let refs: BTreeMap<SymbolId, ClipboardRef> = [
            ("s_same", reference("x", RefKind::Variable)),
            ("s_moved", reference("::area", RefKind::Function)),
            ("s_gone", reference("missing", RefKind::Variable)),
            ("s_twice", reference("::dup", RefKind::Function)),
        ]
        .into_iter()
        .map(|(id, r)| (sym(id), r))
        .collect();
        let visible = [
            variable("s_same", "x"),
            function("t_area", "area"),
            function("t_dup1", "dup"),
            function("t_dup2", "dup"),
            variable("s_seen", "seen"),
            variable("t_missing", "missing_not"),
        ];
        let binding = rebind(&outside, &refs, &visible);
        assert_eq!(binding.rebound, BTreeMap::from([(sym("s_moved"), sym("t_area"))]));
        assert_eq!(
            binding.unresolved,
            [
                Unresolved {
                    sym: sym("s_gone"),
                    name: Some(String::from("missing"))
                },
                Unresolved {
                    sym: sym("s_twice"),
                    name: Some(String::from("::dup"))
                },
                Unresolved {
                    sym: sym("s_unnamed"),
                    name: None
                },
            ]
        );
    }

    #[test]
    fn a_renamed_original_is_kept_unless_another_symbol_has_the_name() {
        let outside = BTreeSet::from([sym("s_x")]);
        let refs = BTreeMap::from([(sym("s_x"), reference("x", RefKind::Variable))]);
        // Renamed since the copy, and nothing else is called `x`: it stays.
        let binding = rebind(&outside, &refs, &[variable("s_x", "renamed")]);
        assert!(binding.rebound.is_empty());
        assert!(binding.unresolved.is_empty());
        // The name decides when another visible symbol has it.
        let visible = [variable("s_x", "renamed"), variable("t_x", "x")];
        let binding = rebind(&outside, &refs, &visible);
        assert_eq!(binding.rebound, BTreeMap::from([(sym("s_x"), sym("t_x"))]));
        assert!(binding.unresolved.is_empty());
        // Visible, but not of the recorded kind: unresolved.
        let binding = rebind(&outside, &refs, &[function("s_x", "x")]);
        assert_eq!(binding.unresolved.len(), 1);
    }

    #[test]
    fn messages_quote_and_shorten_names() {
        assert_eq!(quoted("score"), "`score`");
        let long = "n".repeat(100);
        let shown = quoted(&long);
        assert_eq!(shown.chars().count(), MAX_QUOTED_NAME_CHARS + 3);
        assert!(shown.ends_with("…`"));
    }
}
