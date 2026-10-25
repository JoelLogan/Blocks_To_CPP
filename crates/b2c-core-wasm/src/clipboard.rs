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
//! # User types and their members (clipboard format version 2)
//!
//! Milestone M3 adds structs and enums (types.md §2.4):
//!
//! * a struct or an enum is recorded with the global-namespace qualifier,
//!   as a function is (`::Student`), and binds again by the same rule;
//! * a field or an enumerator is recorded with its owner's qualified name in
//!   front of its own (`::Student::mark`, `::Action::Deposit`), and binds
//!   again through its owner: the owner binds by its name, as a struct or
//!   enum would; when it is the member's own owner (the struct the field
//!   belongs to at the target), the member keeps its ID; when it is another
//!   type, the member binds to that type's member of the same name;
//!   otherwise it is unresolved. Members are found among the visible
//!   symbols, with their owner.
//!
//! The blocks that declare structs and enums come with wave W2; until then
//! no document declares them, so no payload records them, but a payload
//! from elsewhere is bound by these rules.
//!
//! # What is visible at the paste target
//!
//! A paste target is `{module, block, input}` ([`crate::PasteTarget`]):
//!
//! * no block: the module's canvas, where the pasted blocks are parked, so
//!   they see the parked scope (06 §6.5): what is visible at the end of the
//!   module's `main` body, or the module's functions alone without one;
//! * a block and an input: what the scope query gives there (the start of
//!   that statement list, or inside that value input);
//! * a block without an input: directly after the block in its statement
//!   list, which is what is visible at the block plus the variable the block
//!   itself creates, if it is a `var.declare` the analyser accepted (it hides
//!   any other symbol with its name).
//!
//! The scope query answers for every block of the analysed document, so a
//! paste into or after a parked block (loose, in a loose stack, or nested in
//! one) has the parked scope too, and one into or after a block inside a
//! disabled statement has that statement's scope (05 §5.12). A disabled
//! statement in a list the analyser reaches has the scope of its position,
//! like an enabled block there. Only a block nested too deeply has no
//! answer; its place sees the module's functions, which are visible
//! everywhere in it. The analysis is the one kept from the last update when
//! it belongs to the same document (same content hash), otherwise a new one.
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
//! Keeping a renamed original (05 §5.12, 06 §6.14.11) means that copy,
//! rename, paste in one project needs no fix, and agrees with the analyser,
//! which accepts the reference there. Symbol IDs are stable but not unique
//! across projects: projects made from the same example or template share
//! them (`s_guess`), so a block pasted from one into another keeps a
//! reference whose ID the target declares with the same kind, silently,
//! whatever that symbol is called there.
//!
//! An unresolved reference still refers to its original symbol ID, so the
//! analyser reports it once the blocks are in place (`B2C-E0201`, or
//! `B2C-E0203` when that symbol exists in the document but not there). The
//! paste reports one [`NOT_DECLARED`] per unresolved symbol at its first use
//! in the pasted blocks, naming the original.
//!
//! # The shape of the result
//!
//! A copied loose stack is one block with a `stack` (ADR-0011). Pasted on a
//! canvas it stays that way; pasted in or after a block, the stacked blocks
//! follow their head as ordinary blocks, because a block inside another one
//! cannot have a `stack`.

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

/// A symbol declared in a document, as a payload records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Declared {
    /// Its name as written.
    pub(crate) name: String,
    /// What kind of symbol it is.
    pub(crate) kind: RefKind,
    /// For a field or an enumerator, the struct or enum it belongs to.
    pub(crate) owner: Option<SymbolId>,
}

/// Every symbol declared in a document (also in disabled and loose blocks),
/// with its name as written and its kind: `var.declare` and
/// `control.for_range` declare variables and loop counters, `func.define`
/// declares a function and (in `extra.params`) its parameters, as in the
/// analyser's declaration pass. The first declaration of an ID wins. The
/// blocks that declare structs, enums and their members join in wave W2.
pub(crate) fn declarations(document: &Document) -> BTreeMap<SymbolId, Declared> {
    let mut found: BTreeMap<SymbolId, Declared> = BTreeMap::new();
    let mut record = |sym: &SymbolId, name: &str, kind: RefKind| {
        found.entry(sym.clone()).or_insert_with(|| Declared {
            name: name.to_owned(),
            kind,
            owner: None,
        });
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

/// The qualified name recorded for a declared symbol (see the module docs):
/// `None` for a field or an enumerator whose owner is not a declared struct
/// or enum.
pub(crate) fn qualified_name(declared: &BTreeMap<SymbolId, Declared>, sym: &SymbolId) -> Option<String> {
    let symbol = declared.get(sym)?;
    Some(match symbol.kind {
        RefKind::Function | RefKind::Struct | RefKind::Enum => format!("::{}", symbol.name),
        RefKind::Field | RefKind::Enumerator => {
            let owner = declared.get(symbol.owner.as_ref()?)?;
            if owner.kind != owner_kind(symbol.kind) {
                return None;
            }
            format!("::{}::{}", owner.name, symbol.name)
        }
        RefKind::Variable | RefKind::Parameter | RefKind::LoopVariable => symbol.name.clone(),
    })
}

/// The kind of the type a member belongs to: a struct for a field, an enum
/// for an enumerator.
fn owner_kind(member: RefKind) -> RefKind {
    if member == RefKind::Enumerator {
        RefKind::Enum
    } else {
        RefKind::Struct
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
        if let (Some(symbol), Some(name)) = (declared.get(&sym), qualified_name(&declared, &sym))
            && name.chars().count() <= MAX_QUALIFIED_NAME_LEN
        {
            refs.insert(
                sym,
                ClipboardRef {
                    name,
                    kind: symbol.kind,
                },
            );
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
        SymbolInfoKind::Struct { .. } => RefKind::Struct,
        SymbolInfoKind::Enum { .. } => RefKind::Enum,
        SymbolInfoKind::Field { .. } => RefKind::Field,
        SymbolInfoKind::Enumerator { .. } => RefKind::Enumerator,
    }
}

/// Whether a visible symbol is the one a payload's `refs` entry names: the
/// same kind and the same qualified name (see the module docs).
fn names(reference: &ClipboardRef, info: &SymbolInfo) -> bool {
    named(&reference.name, reference.kind, info)
}

/// Whether a visible symbol has the qualified name `name` and the kind
/// `kind`. A field or an enumerator is never matched by name alone: it is
/// bound again through its owner ([`bind_member`]).
fn named(name: &str, kind: RefKind, info: &SymbolInfo) -> bool {
    if ref_kind(info) != kind {
        return false;
    }
    match kind {
        RefKind::Function | RefKind::Struct | RefKind::Enum => {
            name.strip_prefix("::").unwrap_or(name) == info.name
        }
        RefKind::Variable | RefKind::Parameter | RefKind::LoopVariable => name == info.name,
        RefKind::Field | RefKind::Enumerator => false,
    }
}

/// The struct or enum a visible field or enumerator belongs to.
fn member_owner(info: &SymbolInfo) -> Option<&SymbolId> {
    match &info.kind {
        SymbolInfoKind::Field { owner } | SymbolInfoKind::Enumerator { owner } => Some(owner),
        _ => None,
    }
}

/// How a field or an enumerator is bound again.
#[derive(Debug, PartialEq, Eq)]
enum Member {
    /// It keeps its ID.
    Keep,
    /// It binds to this member of another type.
    Rebind(SymbolId),
    /// Nothing matches.
    Unresolved,
}

/// Binds a field or an enumerator again through its owner (types.md §2.4),
/// whose qualified name the payload records in front of the member's own
/// (`::Student::mark`):
///
/// * the owner binds to the visible struct (or enum) with that name, by the
///   rule of the other names: the original member's own owner if it is one
///   of them, otherwise the only one; with none, the original member's own
///   owner when the original member is visible (the type was renamed since
///   the copy);
/// * when that is the original member's own owner, the member keeps its ID;
/// * when it is another type, the member binds to that type's visible
///   member of the same name and kind;
/// * otherwise, and with several, it is unresolved.
fn bind_member(sym: &SymbolId, reference: &ClipboardRef, visible: &[SymbolInfo]) -> Member {
    let Some((owner_name, member)) = reference.name.rsplit_once("::") else {
        return Member::Unresolved;
    };
    let own_owner = visible
        .iter()
        .find(|info| info.id == *sym && ref_kind(info) == reference.kind)
        .and_then(member_owner);
    let owners: Vec<&SymbolId> = visible
        .iter()
        .filter(|info| named(owner_name, owner_kind(reference.kind), info))
        .map(|info| &info.id)
        .collect();
    let owner = match (own_owner, owners.as_slice()) {
        (Some(own), _) if owners.contains(&own) => own,
        (_, [only]) => *only,
        (Some(own), []) => own,
        _ => return Member::Unresolved,
    };
    if own_owner == Some(owner) {
        return Member::Keep;
    }
    let members: Vec<&SymbolInfo> = visible
        .iter()
        .filter(|info| {
            ref_kind(info) == reference.kind && member_owner(info) == Some(owner) && info.name == member
        })
        .collect();
    match members.as_slice() {
        [only] => Member::Rebind(only.id.clone()),
        _ => Member::Unresolved,
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
///   unresolved unless that very symbol is visible;
/// * a field or an enumerator binds through its owner ([`bind_member`]).
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
        if matches!(reference.kind, RefKind::Field | RefKind::Enumerator) {
            match bind_member(sym, reference, visible) {
                Member::Keep => {}
                Member::Rebind(id) => {
                    binding.rebound.insert(sym.clone(), id);
                }
                Member::Unresolved => binding.unresolved.push(Unresolved {
                    sym: sym.clone(),
                    name: Some(reference.name.clone()),
                }),
            }
            continue;
        }
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

/// The blocks to insert in or after a block: each block followed by the
/// blocks of its loose `stack` (ADR-0011), in order, and none of them with a
/// `stack`. A block in a statement list or a value input cannot have one
/// (`B2C-E0139`), and the stacked blocks are what follows the head there.
/// Stacked blocks never have a `stack` of their own (the loader refuses it),
/// so one level is all there is.
pub(crate) fn unstack(blocks: Vec<Block>) -> Vec<Block> {
    let mut out = Vec::with_capacity(blocks.len());
    for mut block in blocks {
        let stack = std::mem::take(&mut block.stack);
        out.push(block);
        out.extend(stack);
    }
    out
}

/// Where each symbol is first referred to in the blocks: the block and the
/// field (also in a type term there), or the token in an expression slot,
/// or the block, for a type term in a row of `extra`.
fn first_references(blocks: &[Block], wanted: &BTreeSet<&SymbolId>) -> BTreeMap<SymbolId, (BlockId, Part)> {
    let mut found: BTreeMap<SymbolId, (BlockId, Part)> = BTreeMap::new();
    tree::walk(blocks, |block| {
        for (field, value) in &block.fields {
            let targets: Vec<&SymbolId> = match value {
                FieldValue::Ref(reference) => vec![&reference.target],
                FieldValue::Type(term) => term.refs().collect(),
                _ => Vec::new(),
            };
            for target in targets {
                if wanted.contains(target) {
                    found
                        .entry(target.clone())
                        .or_insert_with(|| (block.id.clone(), Part::Field { name: field.clone() }));
                }
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
        // The rows of `extra` (the field terms are found above already).
        for sym in block.type_term_refs() {
            if wanted.contains(&sym) {
                found
                    .entry(sym)
                    .or_insert_with(|| (block.id.clone(), Part::Whole));
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
        RefKind::Struct => "struct",
        RefKind::Enum => "enum",
        RefKind::Field => "field",
        RefKind::Enumerator => "enum value",
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
            shared: false,
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

    fn declared(name: &str, kind: RefKind, owner: Option<&str>) -> Declared {
        Declared {
            name: name.to_owned(),
            kind,
            owner: owner.map(sym),
        }
    }

    #[test]
    fn qualified_names() {
        let table: BTreeMap<SymbolId, Declared> = [
            ("v", declared("score", RefKind::Variable, None)),
            ("p", declared("p", RefKind::Parameter, None)),
            ("i", declared("i", RefKind::LoopVariable, None)),
            ("f", declared("area", RefKind::Function, None)),
            ("st", declared("Student", RefKind::Struct, None)),
            ("en", declared("Action", RefKind::Enum, None)),
            ("mark", declared("mark", RefKind::Field, Some("st"))),
            ("dep", declared("Deposit", RefKind::Enumerator, Some("en"))),
            ("lost", declared("x", RefKind::Field, Some("gone"))),
            ("odd", declared("x", RefKind::Field, Some("en"))),
            ("bare", declared("x", RefKind::Enumerator, None)),
        ]
        .into_iter()
        .map(|(id, d)| (sym(id), d))
        .collect();
        let name = |id: &str| qualified_name(&table, &sym(id));
        assert_eq!(name("v").as_deref(), Some("score"));
        assert_eq!(name("p").as_deref(), Some("p"));
        assert_eq!(name("i").as_deref(), Some("i"));
        assert_eq!(name("f").as_deref(), Some("::area"));
        assert_eq!(name("st").as_deref(), Some("::Student"));
        assert_eq!(name("en").as_deref(), Some("::Action"));
        assert_eq!(name("mark").as_deref(), Some("::Student::mark"));
        assert_eq!(name("dep").as_deref(), Some("::Action::Deposit"));
        // A member without a declared owner of the right kind gets no entry.
        assert_eq!(name("lost"), None);
        assert_eq!(name("odd"), None);
        assert_eq!(name("bare"), None);
        assert_eq!(name("unknown"), None);
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

    fn user_type(id: &str, name: &str) -> SymbolInfo {
        info(
            id,
            name,
            SymbolInfoKind::Struct {
                fields: Vec::new(),
                comparable: false,
                printable: false,
            },
        )
    }

    fn user_enum(id: &str, name: &str) -> SymbolInfo {
        info(id, name, SymbolInfoKind::Enum { values: Vec::new() })
    }

    fn field(id: &str, name: &str, owner: &str) -> SymbolInfo {
        info(id, name, SymbolInfoKind::Field { owner: sym(owner) })
    }

    fn enumerator(id: &str, name: &str, owner: &str) -> SymbolInfo {
        info(id, name, SymbolInfoKind::Enumerator { owner: sym(owner) })
    }

    #[test]
    fn structs_and_enums_bind_by_name_like_functions() {
        let outside = BTreeSet::from([sym("s_st"), sym("s_en")]);
        let refs = BTreeMap::from([
            (sym("s_st"), reference("::Student", RefKind::Struct)),
            (sym("s_en"), reference("Action", RefKind::Enum)),
        ]);
        let visible = [
            user_type("t_st", "Student"),
            user_enum("t_en", "Action"),
            // A function of the same name is not a struct.
            function("t_f", "Student"),
        ];
        let binding = rebind(&outside, &refs, &visible);
        assert_eq!(
            binding.rebound,
            BTreeMap::from([(sym("s_st"), sym("t_st")), (sym("s_en"), sym("t_en"))])
        );
        assert!(binding.unresolved.is_empty());
        assert!(!names(
            &reference("::Student", RefKind::Enum),
            &user_type("t", "Student")
        ));
    }

    #[test]
    fn fields_and_enumerators_bind_again_through_their_owner() {
        let refs = BTreeMap::from([
            (sym("s_mark"), reference("::Student::mark", RefKind::Field)),
            (sym("s_dep"), reference("::Action::Deposit", RefKind::Enumerator)),
        ]);
        let outside = BTreeSet::from([sym("s_mark"), sym("s_dep")]);
        // Pasted next to the original: the owner is the member's own, so
        // the members keep their IDs, even renamed since the copy.
        let original = [
            user_type("s_st", "Student"),
            field("s_mark", "grade", "s_st"),
            user_enum("s_en", "Action"),
            enumerator("s_dep", "Deposit", "s_en"),
        ];
        let binding = rebind(&outside, &refs, &original);
        assert!(binding.rebound.is_empty());
        assert!(binding.unresolved.is_empty());
        // The owner was renamed too: no type has the name, but the member's
        // own owner is visible, so it still keeps its ID.
        let renamed = [user_type("s_st", "Pupil"), field("s_mark", "mark", "s_st")];
        let binding = rebind(&BTreeSet::from([sym("s_mark")]), &refs, &renamed);
        assert!(binding.rebound.is_empty() && binding.unresolved.is_empty());
        // In another project: the owner binds to that project's type by name,
        // and the member to that type's member of the same name.
        let other = [
            user_type("t_st", "Student"),
            field("t_name", "name", "t_st"),
            field("t_mark", "mark", "t_st"),
            // A member of the same name of another type does not count.
            user_type("t_other", "Other"),
            field("t_other_mark", "mark", "t_other"),
            user_enum("t_en", "Action"),
            enumerator("t_dep", "Deposit", "t_en"),
            enumerator("t_wd", "Withdraw", "t_en"),
        ];
        let binding = rebind(&outside, &refs, &other);
        assert_eq!(
            binding.rebound,
            BTreeMap::from([(sym("s_dep"), sym("t_dep")), (sym("s_mark"), sym("t_mark"))])
        );
        assert!(binding.unresolved.is_empty());
        // The owner is there, the member is not; or no owner at all; or two.
        let missing = [user_type("t_st", "Student"), field("t_name", "name", "t_st")];
        let twice = [
            user_type("t_a", "Student"),
            user_type("t_b", "Student"),
            field("t_mark", "mark", "t_a"),
        ];
        for visible in [&missing[..], &[], &twice[..]] {
            let binding = rebind(&BTreeSet::from([sym("s_mark")]), &refs, visible);
            assert!(binding.rebound.is_empty());
            assert_eq!(
                binding.unresolved,
                [Unresolved {
                    sym: sym("s_mark"),
                    name: Some(String::from("::Student::mark"))
                }]
            );
        }
        // A field is not an enumerator, and a struct no enum.
        let wrong = [user_enum("t_st", "Student"), enumerator("t_mark", "mark", "t_st")];
        let binding = rebind(&BTreeSet::from([sym("s_mark")]), &refs, &wrong);
        assert_eq!(binding.unresolved.len(), 1);
        // A recorded name without an owner binds nothing.
        let bare = BTreeMap::from([(sym("s_mark"), reference("mark", RefKind::Field))]);
        let binding = rebind(&BTreeSet::from([sym("s_mark")]), &bare, &other);
        assert_eq!(binding.unresolved.len(), 1);
    }

    #[test]
    fn a_member_prefers_its_own_owner_among_types_of_that_name() {
        let refs = BTreeMap::from([(sym("s_mark"), reference("::Student::mark", RefKind::Field))]);
        let visible = [
            user_type("s_st", "Student"),
            user_type("t_st", "Student"),
            field("s_mark", "mark", "s_st"),
            field("t_mark", "mark", "t_st"),
        ];
        let binding = rebind(&BTreeSet::from([sym("s_mark")]), &refs, &visible);
        assert!(binding.rebound.is_empty() && binding.unresolved.is_empty());
        assert_eq!(member_owner(&visible[2]), Some(&sym("s_st")));
        assert_eq!(member_owner(&visible[0]), None);
    }

    #[test]
    fn unresolved_references_in_type_terms_point_at_their_block() {
        let term =
            b2c_ir::types::TypeTerm::app("std::vector", vec![b2c_ir::types::TypeTerm::Ref(sym("s_st"))])
                .unwrap();
        let mut typed = block_with_id("b_typed");
        typed.fields.insert("TYPE".into(), FieldValue::Type(term));
        let mut row = block_with_id("b_row");
        row.extra.insert(
            "params".into(),
            serde_json::json!([{"sym": "s_n", "name": "n", "type": {"ref": "s_en"}, "mode": "copy"}]),
        );
        let wanted_ids = [sym("s_st"), sym("s_en")];
        let wanted: BTreeSet<&SymbolId> = wanted_ids.iter().collect();
        let found = first_references(&[typed, row], &wanted);
        assert_eq!(
            found[&sym("s_st")],
            (
                BlockId::new("b_typed").unwrap(),
                Part::Field { name: "TYPE".into() }
            )
        );
        assert_eq!(found[&sym("s_en")], (BlockId::new("b_row").unwrap(), Part::Whole));
    }

    fn block_with_id(id: &str) -> Block {
        Block {
            id: BlockId::new(id).unwrap(),
            block_type: "t".into(),
            v: 1,
            x: None,
            y: None,
            collapsed: false,
            disabled: false,
            comment: None,
            extra: BTreeMap::new(),
            fields: BTreeMap::new(),
            inputs: BTreeMap::new(),
            statements: BTreeMap::new(),
            stack: Vec::new(),
        }
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
