//! Lowering the struct and enum blocks of W2 (`docs/plans/m3-contracts/w2.md`
//! §1.5, §2.2): package C1. The pre-pass that declares structs and enums
//! before any function is C1's too, in `mod.rs`.
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
    /// A struct or an enum definition (`type.struct`, `type.enum`).
    #[allow(clippy::unused_self, reason = "a stub until package C1 lowers the blocks")]
    pub(super) fn type_definition(&mut self, block: &Block) -> Option<Item> {
        let _ = block;
        None
    }

    /// *set … of … to*.
    #[allow(clippy::unused_self, reason = "a stub until package C1 lowers the blocks")]
    pub(super) fn type_statement(&mut self, block: &Block) -> Option<StmtKind> {
        let _ = block;
        None
    }

    /// *new … with*, *… of …*, an enum value.
    #[allow(clippy::unused_self, reason = "a stub until package C1 lowers the blocks")]
    pub(super) fn type_value(&mut self, block: &Block, origin: Origin) -> Expr {
        let _ = block;
        error_expr(origin)
    }
}
