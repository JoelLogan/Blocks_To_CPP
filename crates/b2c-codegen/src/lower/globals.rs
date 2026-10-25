//! Desugaring W2's globals and *flip* (`docs/plans/m3-contracts/w2.md` §1.9,
//! §2.7): package C2.
//!
//! P0 routes the nodes here (the dispatch in `mod.rs`, `stmt.rs` and
//! `expr.rs`) and leaves their C++ to the package: until it fills this module
//! in, a node prints as an error placeholder (`/* error */`). No analysis
//! makes these nodes before the package's blocks exist, so no program
//! changes. The forms they share are in `forms.rs` (P0's), the C++ nodes in
//! `cast.rs`, the helpers in `helpers.rs`.

use b2c_ir::sast::{GlobalDef, Item, StmtKind};

use super::Lowerer;
use crate::cast::CStmtKind;

impl Lowerer<'_> {
    /// *flip* (`StmtKind::Flip`).
    #[allow(clippy::unused_self, reason = "a stub until package C2 desugars the nodes")]
    pub(super) fn global_stmt(&mut self, kind: &StmtKind) -> CStmtKind {
        let _ = kind;
        CStmtKind::Error
    }

    /// A global variable or constant (`ItemKind::Global`, w2.md §2.7):
    /// `cast::CGlobal`, printed by `emit` before the functions. Package C2
    /// fills this in; until then the item prints nothing, and no analysis
    /// makes one.
    #[allow(clippy::unused_self, reason = "a stub until package C2 lowers the items")]
    pub(super) fn global_item(&mut self, def: &GlobalDef, item: &Item) {
        let _ = (def, item);
    }
}
