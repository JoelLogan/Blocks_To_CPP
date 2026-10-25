//! The forms of types.md §5.2 that collection blocks lower to: an item by
//! its position or key, a size, and a range-`for` with or without a
//! structured binding. Each records the headers it needs.
//!
//! The SAST has no node for them before the collection blocks of milestone
//! M3's second wave, whose lowering calls these.
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "called by the lowering of W2's collection blocks")
)]

use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{Block, Expr, ExprKind};
use b2c_ir::text::Ident;
use b2c_ir::types::{LibType, Type};

use super::Lowerer;
use crate::cast::{CExpr, CStmtKind, CType, RangeBinding};

/// Whether a type's items are found by an `int` position (types.md §1.4,
/// `index`): a vector, a deque or a string. A map's items are found by key.
fn indexed_by_position(ty: &Type) -> bool {
    match ty {
        Type::String => true,
        Type::Lib(app) => matches!(app.ty(), LibType::Vector | LibType::Deque),
        _ => false,
    }
}

impl Lowerer<'_> {
    /// The object of a library member call or of a range-`for`, recording
    /// its type's headers (types.md §5.2: the code uses the value through
    /// its type). A text literal, which C++ reads as an array of characters,
    /// becomes a `std::string` object first.
    pub(super) fn library_object(&mut self, value: &Expr) -> CExpr {
        let lowered = self.expr(value);
        self.note_value_type(&value.ty);
        if value.ty == Type::String {
            self.std_string_of(value, lowered)
        } else {
            lowered
        }
    }

    /// `c.at(i)` (*item of*, types.md §1.5 and §5.2). The index of a vector,
    /// deque or string is cast to `std::size_t`, so the code compiles clean
    /// under `-Wsign-conversion` (the *strict* level); an integer literal,
    /// which is never negative, stays as it is. A map's key is passed as it
    /// is.
    pub(super) fn element_at(&mut self, container: &Expr, index: &Expr) -> CExpr {
        let object = self.library_object(container);
        let lowered = self.expr(index);
        let cast = indexed_by_position(&container.ty) && !matches!(index.kind, ExprKind::Int(_));
        let index = if cast {
            let size = CType::Size;
            self.note_type(&size);
            CExpr::cast(size, lowered)
        } else {
            lowered
        };
        CExpr::method(object, "at", vec![index])
    }

    /// `static_cast<int>(c.size())` (*length of*, types.md §1.5 and §5.2):
    /// sizes are `int` in M3, and the cast keeps `-Wsign-compare` and
    /// `-Wsign-conversion` quiet where the size meets an `int`.
    pub(super) fn size_of(&mut self, container: &Expr) -> CExpr {
        let object = self.library_object(container);
        CExpr::cast(CType::Int, CExpr::method(object, "size", Vec::new()))
    }

    /// `for (const auto& x : c) { … }` (*for each*), or for a map's key and
    /// value `for (const auto& [k, v] : m) { … }` (C++17; types.md §1.5 and
    /// §5.2). `vars` are the loop's one or two variables, which the body
    /// sees.
    pub(super) fn range_for(&mut self, vars: &[SymbolId], range: &Expr, body: &Block) -> CStmtKind {
        let names: Option<Vec<Ident>> = vars
            .iter()
            .map(|id| self.variable(id).map(|symbol| symbol.name.clone()))
            .collect();
        let binding = match names.as_deref() {
            Some([one]) => RangeBinding::One(one.clone()),
            Some([key, value]) => RangeBinding::Pair(key.clone(), value.clone()),
            _ => return CStmtKind::Error,
        };
        let range = self.library_object(range);
        let body = self.scoped_block(body, names.unwrap_or_default());
        CStmtKind::RangeFor { binding, range, body }
    }
}
