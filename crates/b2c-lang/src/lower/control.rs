//! Lowering the control blocks of W2 (`docs/plans/m3-contracts/w2.md` §1.6,
//! §2.9): package D. A switch is lowered through
//! `Lowerer::within(Breakable::Switch, …)` and a loop through
//! `Lowerer::loop_body`; the label checks are in `consts.rs` and the
//! flow rules in `flow.rs` (P0).
//!
//! P0 routes the blocks here (the dispatch in `mod.rs`, `stmt.rs` and
//! `expr.rs`) and leaves their rules to the package: until it fills this
//! module in, a statement lowers to nothing and a value to an error
//! placeholder, as a block type the analyser does not know does, so no
//! program changes. The catalog has none of these blocks before the
//! package adds them.

use b2c_ir::sast::StmtKind;
use b2c_model::Block;

use super::Lowerer;

impl Lowerer<'_> {
    /// *switch on*, *do … then repeat*, *wait*, *go on into the next case*.
    #[allow(clippy::unused_self, reason = "a stub until package D lowers the blocks")]
    pub(super) fn control_statement(&mut self, block: &Block) -> Option<StmtKind> {
        let _ = block;
        None
    }
}
