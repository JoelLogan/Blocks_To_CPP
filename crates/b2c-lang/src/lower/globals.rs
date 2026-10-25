//! Lowering the globals and *flip* of W2 (`docs/plans/m3-contracts/w2.md`
//! §1.9, §2.7): package C2.
//!
//! P0 routes the blocks here (the dispatch in `mod.rs`, `stmt.rs` and
//! `expr.rs`) and leaves their rules to the package: until it fills this
//! module in, a statement lowers to nothing and a value to an error
//! placeholder, as a block type the analyser does not know does, so no
//! program changes. The catalog has none of these blocks before the
//! package adds them.

use b2c_ir::sast::{Item, StmtKind};
use b2c_model::Block;

use super::Lowerer;

impl Lowerer<'_> {
    /// A global variable or constant (`var.global`).
    #[allow(clippy::unused_self, reason = "a stub until package C2 lowers the blocks")]
    pub(super) fn global(&mut self, block: &Block) -> Option<Item> {
        let _ = block;
        None
    }

    /// *flip*.
    #[allow(clippy::unused_self, reason = "a stub until package C2 lowers the blocks")]
    pub(super) fn global_statement(&mut self, block: &Block) -> Option<StmtKind> {
        let _ = block;
        None
    }
}
