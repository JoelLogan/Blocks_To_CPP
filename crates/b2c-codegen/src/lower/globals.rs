//! Desugaring W2's globals and *flip* (`docs/plans/m3-contracts/w2.md` §1.9,
//! §2.7): package C2.
//!
//! P0 routes the nodes here (the dispatch in `mod.rs`, `stmt.rs` and
//! `expr.rs`). A global is a `cast::CGlobal`, printed with the module's
//! declarations in the declaration graph's order; *flip* is `done = !done;`.
//! The forms they share are in `forms.rs` (P0's), the C++ nodes in
//! `cast.rs`, the helpers in `helpers.rs`.

use b2c_ir::sast::{GlobalDef, Item, StmtKind, SymbolKind};

use super::{Lowerer, name_expr};
use crate::cast::{AssignOp, CExpr, CGlobal, CStmtKind, Init, Qualifier, UnOp};

impl Lowerer<'_> {
    /// *flip* (`StmtKind::Flip`): `done = !done;`.
    pub(super) fn global_stmt(&mut self, kind: &StmtKind) -> CStmtKind {
        let StmtKind::Flip { target } = kind else {
            return CStmtKind::Error;
        };
        let Some(symbol) = self.variable(target) else {
            return CStmtKind::Error;
        };
        CStmtKind::Assign {
            target: symbol.name.clone(),
            op: AssignOp::Set,
            value: CExpr::unary(UnOp::Not, name_expr(symbol.name.clone())),
        }
    }

    /// A global variable or constant (`ItemKind::Global`, w2.md §2.7):
    /// `int count = 0;`, `constexpr double kGravity = 9.81;`, `const
    /// std::string kName = "Ann";`, or `int count{};` without a starting
    /// value. It is printed where the declaration graph puts it, with the
    /// module's other declarations.
    pub(super) fn global_item(&mut self, def: &GlobalDef, item: &Item) -> Option<CGlobal> {
        let symbol = self.symbol(&def.symbol)?;
        let SymbolKind::Global { is_const } = symbol.kind else {
            return None;
        };
        self.start_item();
        let ty = self.ctype(&symbol.ty);
        self.note_type(&ty);
        let init = match &def.init {
            Some(value) => Init::Value(self.expr(value)),
            None => Init::Braces,
        };
        self.finish_item(&item.headers);
        let qualifier = match (is_const, def.constexpr) {
            (_, true) => Qualifier::Constexpr,
            (true, false) => Qualifier::Const,
            (false, false) => Qualifier::None,
        };
        Some(CGlobal {
            qualifier,
            ty,
            name: symbol.name.clone(),
            init,
            origin: item.origin.clone(),
            comment: item.comment.clone(),
        })
    }
}
