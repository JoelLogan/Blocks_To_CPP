//! Lowering the error blocks of W2 (`docs/plans/m3-contracts/w2.md` §1.7,
//! §2.5): package B. A catch part is lowered through `Lowerer::in_catch`,
//! and *throw the error again* reports through `Lowerer::check_rethrow`.
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
    /// *try … catch*, *throw*, *throw the error again*.
    #[allow(clippy::unused_self, reason = "a stub until package B lowers the blocks")]
    pub(super) fn error_statement(&mut self, block: &Block) -> Option<StmtKind> {
        let _ = block;
        None
    }

    /// *message of*.
    #[allow(clippy::unused_self, reason = "a stub until package B lowers the blocks")]
    pub(super) fn error_value(&mut self, block: &Block, origin: Origin) -> Expr {
        let _ = block;
        error_expr(origin)
    }
}
