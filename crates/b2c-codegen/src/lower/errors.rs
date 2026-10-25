//! Desugaring W2's error nodes (`docs/plans/m3-contracts/w2.md` §1.7, §2.5):
//! package B.
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
    /// *try … catch*, *throw* and *throw the error again*.
    #[allow(clippy::unused_self, reason = "a stub until package B desugars the nodes")]
    pub(super) fn error_stmt(&mut self, kind: &StmtKind) -> CStmtKind {
        let _ = kind;
        CStmtKind::Error
    }

    /// *message of* (`ExprKind::ErrorMessage`).
    #[allow(clippy::unused_self, reason = "a stub until package B desugars the nodes")]
    pub(super) fn error_expr(&mut self, expr: &Expr) -> CExpr {
        let _ = expr;
        CExpr::error()
    }
}
