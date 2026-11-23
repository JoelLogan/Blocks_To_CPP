//! Parked stacks for the editor's menus (M3-4, ADR-0012,
//! docs/plans/m3-contracts/w5-performance.md §5, §10.1).
//!
//! A parked stack is never part of the program, but the editor's menus ask
//! about its blocks. So once every item is lowered, each parked stack is
//! lowered again in a **sandbox**: from its module's parked point (what is
//! visible at the end of `main`'s body), as a statement list of its own, so
//! that a parked `create` is visible below it in its stack and its reporters
//! are typed. The sandbox records positions on the scope index as any body
//! does; everything else it makes is kept apart from the program:
//!
//! * its diagnostics are dropped (the stack keeps its one `B2C-W0505`);
//! * the symbols it declares are moved out of the program's table, into
//!   [`Parked::symbols`], which only the scope query reads;
//! * its statements and reporters go to [`Parked`] for
//!   [`crate::Analysis::block_types`] only (codegen reads the program).
//!
//! A parked list is a top-level block that is not a definition with the
//! blocks stacked below it, or the blocks stacked below a definition. A
//! block in a namespace's mouth that is not a definition is not parked, and
//! stays out of the sandbox.

use b2c_model::Block;

use super::{ItemCtx, Lowerer, is_value_type};
use crate::query::{DEFINITION_TYPES, Parked};
use crate::scope::ListKey;

/// The pseudo-list a parked stack is lowered in.
const PARKED_LIST: &str = "#parked";
/// The pseudo-list holding what the parked point makes visible.
const SEED_LIST: &str = "#seed";

impl Lowerer<'_> {
    /// Lowers every parked stack in the sandbox (see the module docs), module
    /// by module in document order. Called once every `main` end is
    /// recorded, before the blocks the lowering never reached are recorded,
    /// so that the sandbox's positions win.
    pub(super) fn lower_parked(&mut self) -> Parked {
        let document = self.document;
        let mut parked = Parked::default();
        let lists: Vec<(usize, &b2c_model::Module, &Block, Vec<&Block>)> = document
            .modules
            .iter()
            .enumerate()
            .flat_map(|(index, module)| {
                module.workspace.blocks.iter().filter_map(move |top| {
                    let mut list: Vec<&Block> = Vec::with_capacity(top.stack.len() + 1);
                    if !DEFINITION_TYPES.contains(&top.block_type.as_str()) {
                        list.push(top);
                    }
                    list.extend(&top.stack);
                    (!list.is_empty()).then_some((index, module, top, list))
                })
            })
            .collect();
        if lists.is_empty() {
            return parked;
        }
        let program_symbols: std::collections::BTreeSet<b2c_ir::ids::SymbolId> =
            self.symbols.symbols.keys().cloned().collect();
        let saved = std::mem::take(&mut self.diags);
        for (index, module, top, list) in lists {
            self.enter_module(index, &module.id);
            self.begin_item(ItemCtx::Parked);
            let point = self.index.parked_point(index);
            self.scopes.seed(point.node, ListKey::new(&top.id, SEED_LIST));
            self.scopes.push(ListKey::new(&top.id, PARKED_LIST), false);
            for block in list {
                if block.disabled {
                    // As in a body: it answers for its position.
                    self.record_skipped(block);
                } else if is_value_type(&block.block_type) {
                    parked.values.push(self.value_block(block, None));
                } else if let Some(stmt) = self.statement(block) {
                    parked.stmts.push(stmt);
                }
            }
            self.scopes.reset();
        }
        self.diags = saved;
        self.begin_item(ItemCtx::Main);
        // The sandbox's declarations leave the program's table.
        let (kept, moved) = std::mem::take(&mut self.symbols.symbols)
            .into_iter()
            .partition(|(id, _)| program_symbols.contains(id));
        self.symbols.symbols = kept;
        parked.symbols.symbols = moved;
        parked
    }
}
