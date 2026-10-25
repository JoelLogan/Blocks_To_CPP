//! Lowering the file blocks of W2 (`docs/plans/m3-contracts/w2.md` §1.8,
//! §2.4), *use header* and *check at compile time* (§1.6): package D.
//!
//! P0 routes the blocks here (the dispatch in `mod.rs`, `stmt.rs` and
//! `expr.rs`) and leaves their rules to the package: until it fills this
//! module in, a statement lowers to nothing and a value to an error
//! placeholder, as a block type the analyser does not know does, so no
//! program changes. The catalog has none of these blocks before the
//! package adds them.

use b2c_ir::sast::{Expr, Item, Origin, StmtKind};
use b2c_model::Block;

use super::Lowerer;
use super::expr::error_expr;

impl Lowerer<'_> {
    /// *use header* (`program.use_header`).
    #[allow(clippy::unused_self, reason = "a stub until package D lowers the blocks")]
    pub(super) fn use_header(&mut self, block: &Block) -> Option<Item> {
        let _ = block;
        None
    }

    /// *for each line in file*, *open file*, *write … to file*, *check at
    /// compile time*.
    #[allow(clippy::unused_self, reason = "a stub until package D lowers the blocks")]
    pub(super) fn file_statement(&mut self, block: &Block) -> Option<StmtKind> {
        let _ = block;
        None
    }

    /// *read line from … into*, *file … exists*.
    #[allow(clippy::unused_self, reason = "a stub until package D lowers the blocks")]
    pub(super) fn file_value(&mut self, block: &Block, origin: Origin) -> Expr {
        let _ = block;
        error_expr(origin)
    }
}
