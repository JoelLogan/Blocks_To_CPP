//! Desugaring W2's collection nodes (`docs/plans/m3-contracts/w2.md` §1.2,
//! types.md §1.5 and §5.2): the changes to a collection, *for each*, and
//! the values of a collection. The forms the text blocks share (`element_at`,
//! `size_of`, `range_for`, `library_object`) are in `forms.rs`, the C++
//! nodes in `cast.rs`, the helpers in `helpers.rs`.
//!
//! A collection a block names in a field is always a variable, so its name
//! may be printed twice (`c.end()` after `std::ranges::find(c, x)`).

use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{Block, CollectionChange, CppStandard, Expr, ExprKind, RangeMode, StmtKind, UnaryOp};
use b2c_ir::types::{LibType, Type, TypeFn};

use super::{Lowerer, name_expr, scan};
use crate::cast::{AssignOp, BinOp, BindingStyle, CExpr, CExprKind, CStmtKind, CType};
use crate::helpers::Helper;

/// The library type of a collection, if it is one.
fn lib_type(ty: &Type) -> Option<LibType> {
    match ty {
        Type::Lib(app) => Some(app.ty()),
        _ => None,
    }
}

/// The value of an integer literal, possibly negated (`-5`), as the
/// analyser types it.
fn int_literal_value(expr: &Expr) -> Option<i64> {
    match &expr.kind {
        ExprKind::Int(literal) => literal.as_str().parse().ok(),
        ExprKind::Unary {
            op: UnaryOp::Neg,
            operand,
        } => int_literal_value(operand).and_then(i64::checked_neg),
        _ => None,
    }
}

/// Whether a brace item of type `item.ty` goes into an element of type
/// `elem` as it is (slots.md §3.1): when the types are the same, and for an
/// integer, character or `bool` literal whose value fits the element on
/// every target: any whole number into `int` or `double`, 0 to 127 into
/// `char`, 0 and 1 into `bool`. Every other item is cast, so g++ never meets
/// a narrowing conversion in braces (an error, or `-Wnarrowing`), also where
/// `char` is unsigned.
fn fits_as_written(item: &Expr, elem: &Type) -> bool {
    if item.ty == *elem || item.ty == Type::Error || *elem == Type::Error {
        return true;
    }
    let value = match &item.kind {
        ExprKind::Char(c) => u32::from(c.value()).into(),
        ExprKind::Bool(b) => i64::from(*b),
        _ => match int_literal_value(item) {
            Some(value) => value,
            None => return false,
        },
    };
    match elem {
        Type::Int | Type::Double => !matches!(item.kind, ExprKind::Char(c) if !c.value().is_ascii()),
        Type::Char => (0..=127).contains(&value),
        Type::Bool => (0..=1).contains(&value) && !matches!(item.kind, ExprKind::Char(_)),
        _ => false,
    }
}

/// Whether a type is a number, character or `bool`: the only element types
/// a brace item is cast to (any other item has its element's type, or the
/// analyser reported it).
fn is_scalar(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Double | Type::Char | Type::Bool)
}

impl Lowerer<'_> {
    /// *for each* (`StmtKind::RangeFor`) and the changes to a collection
    /// (`StmtKind::CollectionOp`).
    pub(super) fn collection_stmt(&mut self, kind: &StmtKind) -> CStmtKind {
        match kind {
            StmtKind::RangeFor {
                vars,
                range,
                mode,
                body,
            } => self.for_each(vars, range, *mode, body),
            StmtKind::CollectionOp { collection, op } => self.collection_op(collection, op),
            _ => CStmtKind::Error,
        }
    }

    /// *item of*, *length of*, *is empty*, *contains*, *next item of*, *list
    /// of*.
    pub(super) fn collection_expr(&mut self, expr: &Expr) -> CExpr {
        match &expr.kind {
            ExprKind::Item { collection, index } => self.element_at(collection, index),
            ExprKind::Size(collection) => self.size_of(collection),
            ExprKind::IsEmpty(collection) => {
                let object = self.library_object(collection);
                CExpr::method(object, "empty", Vec::new())
            }
            ExprKind::Next(collection) => {
                let method = match lib_type(&collection.ty) {
                    Some(LibType::Queue) => "front",
                    Some(LibType::Stack | LibType::PriorityQueue) => "top",
                    _ => return CExpr::error(),
                };
                let object = self.library_object(collection);
                CExpr::method(object, method, Vec::new())
            }
            ExprKind::Contains { collection, value } => self.contains(collection, value),
            ExprKind::ListOf(items) => self.list_of(items, &expr.ty),
            _ => CExpr::error(),
        }
    }

    /// `for (const auto& x : c) { … }`, `for (auto x : c)` (*copy*) or `for
    /// (auto& x : c)` (*editable*), with `[[maybe_unused]]` when the body
    /// never uses the loop's names (w2.md §2.12).
    fn for_each(&mut self, vars: &[SymbolId], range: &Expr, mode: RangeMode, body: &Block) -> CStmtKind {
        let unused = !scan::mentions_any(body, vars);
        let mut lowered = self.range_for(vars, range, body);
        if let CStmtKind::RangeFor {
            style, maybe_unused, ..
        } = &mut lowered
        {
            *style = match mode {
                RangeMode::ReadOnly => BindingStyle::ConstRef,
                RangeMode::Copy => BindingStyle::Copy,
                RangeMode::Editable => BindingStyle::Ref,
            };
            *maybe_unused = unused;
        }
        lowered
    }

    /// A change to a collection variable (w2.md §1.2).
    fn collection_op(&mut self, collection: &SymbolId, op: &CollectionChange) -> CStmtKind {
        let Some(symbol) = self.variable(collection) else {
            return CStmtKind::Error;
        };
        let Some(lib) = lib_type(&symbol.ty) else {
            return CStmtKind::Error;
        };
        let name = symbol.name.clone();
        let ty = symbol.ty.clone();
        // The code uses the collection through its type's members.
        self.note_value_type(&ty);
        let object = name_expr(name.clone());
        let call = {
            let name = name.clone();
            move |method: &'static str, args: Vec<CExpr>| {
                CStmtKind::Expr(CExpr::method(name_expr(name.clone()), method, args))
            }
        };
        match op {
            CollectionChange::Add(value) => {
                let method = match lib {
                    LibType::Vector | LibType::Deque => "push_back",
                    LibType::Set => "insert",
                    LibType::Queue | LibType::Stack | LibType::PriorityQueue => "push",
                    _ => return CStmtKind::Error,
                };
                let value = self.expr(value);
                call(method, vec![value])
            }
            CollectionChange::SetItem { index, value } => {
                let target = if lib == LibType::Map {
                    let key = self.expr(index);
                    CExpr::index(object, key)
                } else {
                    let position = self.position(index);
                    CExpr::method(object, "at", vec![position])
                };
                CStmtKind::AssignTo {
                    target,
                    op: AssignOp::Set,
                    value: self.expr(value),
                }
            }
            CollectionChange::Clear => match lib {
                // The adaptors have no `clear()`: an empty one replaces it.
                LibType::Queue | LibType::Stack | LibType::PriorityQueue => CStmtKind::Assign {
                    target: name,
                    op: AssignOp::Set,
                    value: CExpr::new(CExprKind::Braces(Vec::new())),
                },
                _ => call("clear", Vec::new()),
            },
            CollectionChange::RemoveAt(index) => {
                self.use_helper(Helper::RemoveAt);
                let index = self.expr(index);
                CStmtKind::Expr(CExpr::call_fixed("b2c::remove_at", vec![object, index]))
            }
            CollectionChange::RemoveValue(value) => {
                let value = self.expr(value);
                call("erase", vec![value])
            }
            CollectionChange::RemoveNext => call("pop", Vec::new()),
            CollectionChange::Sort { descending } => self.sort(object, *descending),
        }
    }

    /// A position into a list or deque, as `.at()` takes it: an `int`
    /// cast to `std::size_t`, so the code compiles clean under
    /// `-Wsign-conversion`; an integer literal, never negative, as it is
    /// (types.md §5.2, as `element_at` does).
    fn position(&mut self, index: &Expr) -> CExpr {
        let lowered = self.expr(index);
        if matches!(index.kind, ExprKind::Int(_)) {
            return lowered;
        }
        let size = CType::Size;
        self.note_type(&size);
        CExpr::cast(size, lowered)
    }

    /// `std::ranges::sort(c);` (C++20 and later) or `std::sort(c.begin(), c.end());`
    /// (C++17), largest first with `std::greater`.
    fn sort(&mut self, object: CExpr, descending: bool) -> CStmtKind {
        self.include("<algorithm>");
        let greater = descending.then(|| {
            self.include("<functional>");
            CExpr::fixed(if self.standard >= CppStandard::Cpp20 {
                "std::greater{}"
            } else {
                "std::greater<>{}"
            })
        });
        let call = if self.standard >= CppStandard::Cpp20 {
            let mut args = vec![object];
            args.extend(greater);
            CExpr::call_fixed("std::ranges::sort", args)
        } else {
            let mut args = vec![
                CExpr::method(object.clone(), "begin", Vec::new()),
                CExpr::method(object, "end", Vec::new()),
            ];
            args.extend(greater);
            CExpr::call_fixed("std::sort", args)
        };
        CStmtKind::Expr(call)
    }

    /// *contains* (w2.md §1.2): a set's or map's own lookup, `c.contains(x)`
    /// (C++20) or `c.count(x) != 0`; for a list or deque, `std::ranges::find(c,
    /// x) != c.end()` (C++20) or `std::find(c.begin(), c.end(), x) !=
    /// c.end()`.
    fn contains(&mut self, collection: &Expr, value: &Expr) -> CExpr {
        let lib = lib_type(&collection.ty);
        let object = self.library_object(collection);
        let cpp20 = self.standard >= CppStandard::Cpp20;
        match lib {
            Some(LibType::Set | LibType::Map) => {
                let value = self.expr(value);
                if cpp20 {
                    CExpr::method(object, "contains", vec![value])
                } else {
                    let count = CExpr::method(object, "count", vec![value]);
                    CExpr::binary(BinOp::Ne, count, super::int_literal(0))
                }
            }
            Some(LibType::Vector | LibType::Deque) => {
                self.include("<algorithm>");
                let value = self.sought(value);
                let end = CExpr::method(object.clone(), "end", Vec::new());
                let found = if cpp20 {
                    CExpr::call_fixed("std::ranges::find", vec![object, value])
                } else {
                    let begin = CExpr::method(object, "begin", Vec::new());
                    CExpr::call_fixed("std::find", vec![begin, end.clone(), value])
                };
                CExpr::binary(BinOp::Ne, found, end)
            }
            _ => CExpr::error(),
        }
    }

    /// The value `std::find` looks for. A function template cannot deduce
    /// its type from a brace list, so a *list of* names its type
    /// (`std::vector<int>{1, 2}`).
    fn sought(&mut self, value: &Expr) -> CExpr {
        match &value.kind {
            ExprKind::ListOf(items) if value.ty != Type::Error => {
                let ty = self.ctype(&value.ty);
                self.note_type(&ty);
                let items = self.list_items(items, &value.ty);
                CExpr::new(CExprKind::Init {
                    ty,
                    items: items.into_iter().map(|item| (None, item)).collect(),
                })
                .with_origin(value.origin.clone())
            }
            _ => self.expr(value),
        }
    }

    /// *list of*: `{a, b}`, each item cast to the element type where
    /// slots.md §3.1 says.
    fn list_of(&mut self, items: &[Expr], ty: &Type) -> CExpr {
        self.note_value_type(ty);
        let items = self.list_items(items, ty);
        CExpr::new(CExprKind::Braces(items))
    }

    /// The items of a brace list of type `ty`.
    fn list_items(&mut self, items: &[Expr], ty: &Type) -> Vec<CExpr> {
        let elem = TypeFn::Elem.apply(ty).unwrap_or(Type::Error);
        items
            .iter()
            .map(|item| {
                let lowered = self.expr(item);
                if is_scalar(&elem) && !fits_as_written(item, &elem) {
                    CExpr::cast(self.ctype(&elem), lowered)
                } else {
                    lowered
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use b2c_ir::ids::{BlockId, ModuleId};
    use b2c_ir::sast::Origin;
    use b2c_ir::text::{CharLit, NumLit, NumType};

    use super::*;

    fn lit(kind: ExprKind, ty: Type) -> Expr {
        Expr {
            kind,
            ty,
            origin: Origin::whole(ModuleId::new("m").unwrap(), BlockId::new("b").unwrap()),
        }
    }

    fn int(value: i32) -> Expr {
        let literal = lit(ExprKind::Int(NumLit::int(value.abs())), Type::Int);
        if value < 0 {
            lit(
                ExprKind::Unary {
                    op: UnaryOp::Neg,
                    operand: Box::new(literal),
                },
                Type::Int,
            )
        } else {
            literal
        }
    }

    #[test]
    fn literals_that_fit_every_target_stay_as_written() {
        let letter = |text: &str| lit(ExprKind::Char(CharLit::new(text).unwrap()), Type::Char);
        let real = lit(
            ExprKind::Float(NumLit::parse("2.0", NumType::Double).unwrap()),
            Type::Double,
        );
        let truth = lit(ExprKind::Bool(true), Type::Bool);
        let cases = [
            (int(5), Type::Int, true),
            (int(-2_000_000), Type::Double, true),
            (int(127), Type::Char, true),
            (int(128), Type::Char, false),
            (int(-5), Type::Char, false),
            (int(0), Type::Bool, true),
            (int(1), Type::Bool, true),
            (int(2), Type::Bool, false),
            (letter("a"), Type::Int, true),
            (letter("a"), Type::Double, true),
            (letter("a"), Type::Bool, false),
            (truth.clone(), Type::Int, true),
            (truth, Type::Char, true),
            (real.clone(), Type::Int, false),
            (real.clone(), Type::Char, false),
            (real, Type::Double, true),
        ];
        for (item, elem, expected) in cases {
            assert_eq!(
                fits_as_written(&item, &elem),
                expected,
                "{:?} into {elem:?}",
                item.kind
            );
        }
        // A variable of another type is always cast.
        let var = lit(ExprKind::Var(SymbolId::new("s").unwrap()), Type::Int);
        assert!(!fits_as_written(&var, &Type::Double));
        assert!(fits_as_written(&var, &Type::Int));
    }
}
