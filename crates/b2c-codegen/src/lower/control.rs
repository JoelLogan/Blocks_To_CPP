//! Desugaring W2's control nodes (`docs/plans/m3-contracts/w2.md` §1.6, §2.9):
//! package D.
//!
//! P0 routes the nodes here (the dispatch in `mod.rs`, `stmt.rs` and
//! `expr.rs`) and leaves their C++ to the package: until it fills this module
//! in, a node prints as an error placeholder (`/* error */`). No analysis
//! makes these nodes before the package's blocks exist, so no program
//! changes. The forms they share are in `forms.rs` (P0's), the C++ nodes in
//! `cast.rs`, the helpers in `helpers.rs`.

use b2c_ir::sast::StmtKind;

use super::Lowerer;
use crate::cast::CStmtKind;

impl Lowerer<'_> {
    /// *switch on*, *do … then repeat*, *wait* and *go on into the next
    /// case*.
    #[allow(clippy::unused_self, reason = "a stub until package D desugars the nodes")]
    pub(super) fn control_stmt(&mut self, kind: &StmtKind) -> CStmtKind {
        let _ = kind;
        CStmtKind::Error
    }
}
