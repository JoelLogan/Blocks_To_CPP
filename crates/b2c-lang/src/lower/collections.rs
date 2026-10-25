//! Lowering the collection blocks of W2 (`docs/plans/m3-contracts/w2.md`
//! §1.2): package A.
//!
//! P0 routes the blocks here (the dispatch in `mod.rs`, `stmt.rs` and
//! `expr.rs`) and leaves their rules to the package: until it fills this
//! module in, a statement lowers to nothing and a value to an error
//! placeholder, as a block type the analyser does not know does, so no
//! program changes. The catalog has none of these blocks before the
//! package adds them.

use b2c_ir::sast::{Expr, Origin, StmtKind};
use b2c_model::Block;

use super::Lowerer;
use super::expr::error_expr;

impl Lowerer<'_> {
    /// A collection statement: *add*, *set item*, *for each*, *for each key
    /// value*, *clear*, *remove*, *sort*.
    #[allow(clippy::unused_self, reason = "a stub until package A lowers the blocks")]
    pub(super) fn collection_statement(&mut self, block: &Block) -> Option<StmtKind> {
        let _ = block;
        None
    }

    /// A collection value: *item of*, *length of*, *is empty*, *list of*,
    /// *contains*, *next item of*.
    #[allow(clippy::unused_self, reason = "a stub until package A lowers the blocks")]
    pub(super) fn collection_value(&mut self, block: &Block, origin: Origin) -> Expr {
        let _ = block;
        error_expr(origin)
    }
}
