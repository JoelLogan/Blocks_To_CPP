//! Desugaring W2's file nodes (`docs/plans/m3-contracts/w2.md` §1.8, §2.4),
//! *use header* and *check at compile time* (§1.6): package D. The hidden
//! stream name of *for each line* is chosen as *repeat*'s counter is
//! (`scan::fresh_name`, w2.md §2.12).
//!
//! P0 routes the nodes here (the dispatch in `mod.rs`, `stmt.rs` and
//! `expr.rs`) and leaves their C++ to the package: until it fills this module
//! in, a node prints as an error placeholder (`/* error */`). No analysis
//! makes these nodes before the package's blocks exist, so no program
//! changes. The forms they share are in `forms.rs` (P0's), the C++ nodes in
//! `cast.rs`, the helpers in `helpers.rs`.

use b2c_ir::headers::Header;
use b2c_ir::sast::{Expr, Item, StmtKind};

use super::Lowerer;
use crate::cast::{CExpr, CStmtKind};

impl Lowerer<'_> {
    /// *for each line in file*, *open file*, *write … to file* and *check at
    /// compile time*.
    #[allow(clippy::unused_self, reason = "a stub until package D desugars the nodes")]
    pub(super) fn file_stmt(&mut self, kind: &StmtKind) -> CStmtKind {
        let _ = kind;
        CStmtKind::Error
    }

    /// *read line from … into* and *file … exists*.
    #[allow(clippy::unused_self, reason = "a stub until package D desugars the nodes")]
    pub(super) fn file_expr(&mut self, expr: &Expr) -> CExpr {
        let _ = expr;
        CExpr::error()
    }

    /// *use header* (`ItemKind::UseHeader`): the header joins the module's
    /// includes, once (w2.md §1.6). Package D may extend this.
    pub(super) fn use_header(&mut self, header: Header, item: &Item) {
        let _ = item;
        self.usage.includes.insert(header);
    }
}
