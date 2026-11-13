//! User namespaces (w3-modules.md §2.12, package N1): the *namespace*
//! blocks' symbols, their nesting, and the namespace each definition in a
//! mouth is in.
//!
//! * **One namespace, many blocks.** Every *namespace* block declares its
//!   own symbol, and the blocks of one name in one enclosing namespace, in
//!   one module or in several, are one C++ namespace. The first of them (in
//!   module order, then by block ID, outer before inner) stands for it: a
//!   nested namespace's `parent`, and the namespace [`Namespaces::member_of`]
//!   gives a definition, are that first block's symbol.
//! * **Nesting** is at most [`MAX_NAMESPACE_DEPTH`] deep; the first block
//!   past it is `B2C-E0219` (the blocks inside it are not reported again).
//! * **Names.** A namespace's name is checked as other names at namespace
//!   scope (`B2C-E0220`: not `std`, nothing starting with `b2c`), and may not
//!   be the name of a definition (a function, struct, enum or global) of any
//!   module in the same enclosing namespace, shared or not (`B2C-E0210`,
//!   program-wide, §2.1): which one `geo::x` meant would then depend on
//!   what each file sees.
//!
//! What a definition's namespace means for lookup, spellings and code
//! generation (`Item::namespace`, `E0210` and `E0213` per namespace) is
//! package N2's.

use std::collections::BTreeMap;

use b2c_ir::diag::Part;
use b2c_ir::ids::{BlockId, SymbolId};
use b2c_ir::sast::{Symbol, SymbolKind};
use b2c_ir::types::Type;
use b2c_model::Block;
use b2c_model::limits::MAX_NAMESPACE_DEPTH;

use super::{Lowerer, field};
use crate::access::{self, namespace_members};
use crate::codes;
use crate::collect::{DeclKind, DeclStatus};
use crate::messages::quoted;

/// The program's user namespaces.
#[derive(Debug, Default)]
pub(super) struct Namespaces {
    /// The namespace each definition in an enabled namespace's mouth is in,
    /// by the definition's block: the symbol that stands for the namespace
    /// (see the module docs). Top-level definitions are not listed.
    pub(super) member_of: BTreeMap<BlockId, SymbolId>,
}

/// A namespace declared, for the name checks once all are known.
struct Declared {
    block: BlockId,
    module: usize,
    /// The symbol that stands for its enclosing namespace.
    parent: Option<SymbolId>,
    name: String,
}

impl Lowerer<'_> {
    /// Declares every enabled *namespace* block of the program, outer
    /// before inner, and records the namespace of each definition in a
    /// mouth; then checks the namespaces' names against the definitions'.
    pub(super) fn declare_namespaces(&mut self) {
        let document = self.document;
        let mut standing: BTreeMap<(Option<SymbolId>, String), SymbolId> = BTreeMap::new();
        let mut declared = Vec::new();
        for (index, module) in document.modules.iter().enumerate() {
            self.enter_module(index, &module.id);
            let mut pending: Vec<(&Block, Option<SymbolId>, usize)> = module
                .workspace
                .blocks
                .iter()
                .filter(|block| block.block_type == "program.namespace" && !block.disabled)
                .map(|block| (block, None, 1))
                .collect();
            pending.sort_by(|a, b| b.0.id.cmp(&a.0.id));
            while let Some((block, parent, depth)) = pending.pop() {
                let Some((stands_for, name)) =
                    self.declare_namespace(block, parent.as_ref(), depth, &mut standing)
                else {
                    continue;
                };
                declared.push(Declared {
                    block: block.id.clone(),
                    module: index,
                    parent: parent.clone(),
                    name,
                });
                let mut inner = Vec::new();
                for member in namespace_members(block) {
                    self.namespaces
                        .member_of
                        .insert(member.id.clone(), stands_for.clone());
                    if member.block_type == "program.namespace" && !member.disabled {
                        inner.push((member, Some(stands_for.clone()), depth + 1));
                    }
                }
                // Depth first, in block ID order.
                inner.sort_by(|a, b| b.0.id.cmp(&a.0.id));
                pending.extend(inner);
            }
        }
        self.check_namespace_names(&declared);
    }

    /// Declares one *namespace* block nested `depth` deep in `parent`, and
    /// returns the symbol that stands for its namespace with the block's
    /// name; `None` when it has no name or its symbol ID is taken
    /// (reported).
    fn declare_namespace(
        &mut self,
        block: &Block,
        parent: Option<&SymbolId>,
        depth: usize,
        standing: &mut BTreeMap<(Option<SymbolId>, String), SymbolId>,
    ) -> Option<(SymbolId, String)> {
        let name_loc = self.loc(&block.id, field("NAME"));
        if depth == MAX_NAMESPACE_DEPTH + 1 {
            let message = format!(
                "This namespace is inside {MAX_NAMESPACE_DEPTH} other namespaces, which is as deep as \
                 namespaces go. Move it, or the namespaces in it, out of one of the namespaces around it."
            );
            self.diags.error(
                codes::NAMESPACE_TOO_DEEP,
                self.loc(&block.id, Part::Whole),
                message,
            );
        }
        let Ok(decl) = access::decl_field(block, "NAME") else {
            self.incomplete(name_loc, "This namespace needs a name.");
            return None;
        };
        let ident = self.ident(&decl.name, &name_loc, true);
        let key = (parent.cloned(), ident.as_str().to_owned());
        let stands_for = standing.entry(key).or_insert_with(|| decl.sym.clone()).clone();
        let symbol = Symbol {
            name: ident,
            kind: SymbolKind::Namespace {
                parent: parent.cloned(),
            },
            ty: Type::Void,
            origin: self.origin(&block.id, field("NAME")),
        };
        self.insert_symbol(&decl.sym, &decl.name, symbol, name_loc)
            .then(|| (stands_for, decl.name.clone()))
    }

    /// `B2C-E0210` for a namespace named like a definition of any module in
    /// the same enclosing namespace, shared or not (w3-modules.md §2.1).
    fn check_namespace_names(&mut self, declared: &[Declared]) {
        let mut definitions: BTreeMap<(Option<&SymbolId>, &str), (&SymbolId, DeclKind, usize)> =
            BTreeMap::new();
        for (sym, decl) in &self.decls {
            let what = matches!(
                decl.kind,
                DeclKind::Function | DeclKind::Struct | DeclKind::Enum | DeclKind::Global
            );
            if !what || decl.status != DeclStatus::Active {
                continue;
            }
            let namespace = decl
                .location
                .block
                .as_ref()
                .and_then(|block| self.namespaces.member_of.get(block));
            definitions
                .entry((namespace, decl.name.as_str()))
                .or_insert((sym, decl.kind, decl.module));
        }
        let clashes: Vec<(usize, BlockId, String, SymbolId, DeclKind, usize)> = declared
            .iter()
            .filter_map(|namespace| {
                let (sym, kind, module) =
                    definitions.get(&(namespace.parent.as_ref(), namespace.name.as_str()))?;
                Some((
                    namespace.module,
                    namespace.block.clone(),
                    namespace.name.clone(),
                    (*sym).clone(),
                    *kind,
                    *module,
                ))
            })
            .collect();
        for (module, block, name, sym, kind, defined_in) in clashes {
            let id = self.document.modules[module].id.clone();
            self.enter_module(module, &id);
            let what = match kind {
                DeclKind::Function => "function",
                DeclKind::Struct => "struct",
                DeclKind::Enum => "enum",
                _ => "global variable",
            };
            let place = if defined_in == module {
                String::new()
            } else {
                format!(" of module {}", quoted(&self.document.modules[defined_in].name))
            };
            let message = format!(
                "There is already a {what} called {}{place} in the same place, and C++ would not know which \
                 of the two the name means. Rename the namespace or the {what}.",
                quoted(&name)
            );
            let location = self.loc(&block, field("NAME"));
            self.error_with_decl(
                codes::DUPLICATE_NAME,
                location,
                message,
                &sym,
                &format!("the {what}"),
            );
        }
    }
}

/// The top-level blocks of a module and the definitions in its enabled
/// namespaces' mouths, at any depth, by block ID (`sorted_blocks` uses
/// it). A disabled namespace's definitions are left out with it.
pub(super) fn with_namespace_members(top_level: Vec<&Block>) -> Vec<&Block> {
    let mut blocks = Vec::with_capacity(top_level.len());
    let mut pending = top_level;
    while let Some(block) = pending.pop() {
        if block.block_type == "program.namespace" && !block.disabled {
            pending.extend(namespace_members(block));
        }
        blocks.push(block);
    }
    blocks.sort_by(|a, b| a.id.cmp(&b.id));
    blocks
}

#[cfg(test)]
mod tests {
    use crate::lower::sorted_blocks;

    #[test]
    fn sorted_blocks_reach_into_enabled_mouths() {
        let module: b2c_model::Module = serde_json::from_value(serde_json::json!({
            "id": "m", "name": "main", "workspace": {"blocks": [
                {"id": "b2", "type": "program.namespace", "v": 1, "statements": {"DEFINITIONS": [
                    {"id": "b4", "type": "func.define", "v": 1},
                    {"id": "b1", "type": "program.namespace", "v": 1, "statements": {"DEFINITIONS": [
                        {"id": "b6", "type": "type.enum", "v": 1},
                        {"id": "b7", "type": "control.break", "v": 1}
                    ]}}
                ]}},
                {"id": "b3", "type": "program.namespace", "v": 1, "disabled": true, "statements": {
                    "DEFINITIONS": [{"id": "b5", "type": "func.define", "v": 1}]
                }}
            ]}
        }))
        .expect("module");
        let ids: Vec<&str> = sorted_blocks(&module).iter().map(|b| b.id.as_str()).collect();
        // Not the statement in a mouth, nor what a disabled namespace holds.
        assert_eq!(ids, ["b1", "b2", "b3", "b4", "b6"]);
    }
}
