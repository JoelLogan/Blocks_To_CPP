//! Desugaring W2's collection nodes (`docs/plans/m3-contracts/w2.md` §1.2):
//! package A.
//!
//! P0 routes the nodes here (the dispatch in `mod.rs`, `stmt.rs` and
//! `expr.rs`) and leaves their C++ to the package: until it fills this module
//! in, a node prints as an error placeholder (`/* error */`). No analysis
//! makes these nodes before the package's blocks exist, so no program
//! changes. The forms they share are in `forms.rs` (P0's), the C++ nodes in
//! `cast.rs`, the helpers in `helpers.rs`.

use b2c_ir::sast::{Expr, StmtKind};

use super::Lowerer;
use crate::cast::{CExpr, CStmtKind};

impl Lowerer<'_> {
    /// *for each* (`StmtKind::RangeFor`) and the changes to a collection
    /// (`StmtKind::CollectionOp`).
    #[allow(clippy::unused_self, reason = "a stub until package A desugars the nodes")]
    pub(super) fn collection_stmt(&mut self, kind: &StmtKind) -> CStmtKind {
        let _ = kind;
        CStmtKind::Error
    }

    /// *item of*, *length of*, *is empty*, *contains*, *next item of*, *list
    /// of*.
    #[allow(clippy::unused_self, reason = "a stub until package A desugars the nodes")]
    pub(super) fn collection_expr(&mut self, expr: &Expr) -> CExpr {
        let _ = expr;
        CExpr::error()
    }
}
