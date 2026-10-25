//! Lowering the text, input and output, and math blocks of W2
//! (`docs/plans/m3-contracts/w2.md` §1.3, §1.4): package B.
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
    /// *show numbers with … decimal places*.
    #[allow(clippy::unused_self, reason = "a stub until package B lowers the blocks")]
    pub(super) fn text_statement(&mut self, block: &Block) -> Option<StmtKind> {
        let _ = block;
        None
    }

    /// A text or math value: *length of*, *letter of*, *as text*, *round*,
    /// *random decimal*, …
    #[allow(clippy::unused_self, reason = "a stub until package B lowers the blocks")]
    pub(super) fn text_value(&mut self, block: &Block, origin: Origin) -> Expr {
        let _ = block;
        error_expr(origin)
    }
}
