//! Desugaring of expressions.

use b2c_ir::sast::{BinaryOp, Expr, ExprKind, UnaryOp};
use b2c_ir::text::StrLit;
use b2c_ir::types::Type;

use super::scan::MAX_DEPTH;
use super::{Lowerer, bool_token, int_literal};
use crate::cast::{BinOp, CExpr, CExprKind, CType, Callee, UnOp};
use crate::helpers::Helper;

/// The C++ unary operator for a SAST operator.
fn un_op(op: UnaryOp) -> UnOp {
    match op {
        UnaryOp::Neg => UnOp::Neg,
        UnaryOp::Plus => UnOp::Plus,
        UnaryOp::Not => UnOp::Not,
    }
}

/// The C++ binary operator for a SAST operator.
fn bin_op(op: BinaryOp) -> BinOp {
    match op {
        BinaryOp::Add => BinOp::Add,
        BinaryOp::Sub => BinOp::Sub,
        BinaryOp::Mul => BinOp::Mul,
        BinaryOp::Div => BinOp::Div,
        BinaryOp::Mod => BinOp::Mod,
        BinaryOp::Lt => BinOp::Lt,
        BinaryOp::Le => BinOp::Le,
        BinaryOp::Gt => BinOp::Gt,
        BinaryOp::Ge => BinOp::Ge,
        BinaryOp::Eq => BinOp::Eq,
        BinaryOp::Ne => BinOp::Ne,
        BinaryOp::And => BinOp::And,
        BinaryOp::Or => BinOp::Or,
    }
}

/// Whether a SAST operator is a comparison.
fn is_comparison(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge | BinaryOp::Eq | BinaryOp::Ne
    )
}

/// The comparison that is true exactly when `op` is false, if the operand
/// types allow it. `==`/`!=` invert for every built-in type. The ordering
/// comparisons do not invert for `double`, because `!(a < b)` and `a >= b`
/// differ when a value is NaN.
fn inverse_comparison(op: BinaryOp, lhs: &Type, rhs: &Type) -> Option<BinaryOp> {
    let ordered = |ty: &Type| matches!(ty, Type::Int | Type::Char | Type::Bool | Type::String);
    let comparable = |ty: &Type| ordered(ty) || *ty == Type::Double;
    let (inverse, needs_order) = match op {
        BinaryOp::Eq => (BinaryOp::Ne, false),
        BinaryOp::Ne => (BinaryOp::Eq, false),
        BinaryOp::Lt => (BinaryOp::Ge, true),
        BinaryOp::Le => (BinaryOp::Gt, true),
        BinaryOp::Gt => (BinaryOp::Le, true),
        BinaryOp::Ge => (BinaryOp::Lt, true),
        _ => return None,
    };
    let ok = if needs_order {
        ordered(lhs) && ordered(rhs)
    } else {
        comparable(lhs) && comparable(rhs)
    };
    ok.then_some(inverse)
}

/// Whether the C++ generated for a `std::string`-typed SAST expression is a
/// `std::string` object (as opposed to a `const char*` such as a literal).
fn is_std_string(expr: &Expr) -> bool {
    is_std_string_at(expr, 0)
}

/// [`is_std_string`] with a recursion bound (answering "no" when exceeded,
/// which only adds a harmless `std::string(…)`).
fn is_std_string_at(expr: &Expr, depth: usize) -> bool {
    if expr.ty != Type::String || depth > MAX_DEPTH {
        return false;
    }
    match &expr.kind {
        ExprKind::Var(_) | ExprKind::Call { .. } | ExprKind::Join(_) | ExprKind::Binary { .. } => true,
        ExprKind::Conditional {
            then_value,
            else_value,
            ..
        } => is_std_string_at(then_value, depth + 1) || is_std_string_at(else_value, depth + 1),
        _ => false,
    }
}

/// Moves the origins of `from` onto a replacement expression.
fn replace_kind(from: CExpr, kind: CExprKind) -> CExpr {
    CExpr {
        kind,
        origins: from.origins,
    }
}

/// A desugared `bool` as text: `b ? "true" : "false"`, or just `"true"` /
/// `"false"` for a literal.
fn bool_text(expr: &Expr, lowered: CExpr) -> CExpr {
    match (&expr.kind, &lowered.kind) {
        (ExprKind::Bool(true), CExprKind::Fixed(_)) => replace_kind(lowered, CExprKind::Fixed("\"true\"")),
        (ExprKind::Bool(false), CExprKind::Fixed(_)) => replace_kind(lowered, CExprKind::Fixed("\"false\"")),
        _ => CExpr::conditional(lowered, CExpr::fixed("\"true\""), CExpr::fixed("\"false\"")),
    }
}

impl Lowerer<'_> {
    /// Desugars an expression and records its origin.
    pub(super) fn expr(&mut self, expr: &Expr) -> CExpr {
        if self.depth >= MAX_DEPTH || expr.ty == Type::Error {
            return CExpr::error().with_origin(expr.origin.clone());
        }
        self.depth += 1;
        let lowered = self.expr_kind(expr);
        self.depth -= 1;
        lowered.with_origin(expr.origin.clone())
    }

    /// Desugars an expression without recording its origin.
    fn expr_kind(&mut self, expr: &Expr) -> CExpr {
        match &expr.kind {
            ExprKind::Int(lit) | ExprKind::Float(lit) => CExpr::new(CExprKind::Num(lit.clone())),
            ExprKind::Bool(value) => CExpr::fixed(bool_token(*value)),
            ExprKind::Str(lit) => CExpr::new(CExprKind::Str(lit.clone())),
            ExprKind::Char(lit) => CExpr::new(CExprKind::Char(*lit)),
            ExprKind::Var(id) => match self.variable(id) {
                Some(symbol) => CExpr::new(CExprKind::Name(symbol.name.clone())),
                None => CExpr::error(),
            },
            ExprKind::Unary { op, operand } => CExpr::unary(un_op(*op), self.expr(operand)),
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, &expr.ty),
            ExprKind::Conditional {
                cond,
                then_value,
                else_value,
            } => {
                let cond = self.expr(cond);
                let then_value = self.expr(then_value);
                let else_value = self.expr(else_value);
                CExpr::conditional(cond, then_value, else_value)
            }
            ExprKind::Call { function, args } => match self.function_symbol(function) {
                Some(symbol) => {
                    // Using a returned `std::string` (e.g. printing it) needs
                    // `<string>` even if this module never names the type.
                    self.note_type(CType::result(&symbol.ty));
                    let args = args.iter().map(|arg| self.expr(arg)).collect();
                    CExpr::new(CExprKind::Call {
                        callee: Callee::User(symbol.name.clone()),
                        args,
                    })
                }
                None => CExpr::error(),
            },
            ExprKind::Join(items) => self.join(items),
            ExprKind::RandomInt { low, high } => {
                self.use_helper(Helper::RandomInt);
                let low = self.expr(low);
                let high = self.expr(high);
                CExpr::call_fixed("b2c::random_int", vec![low, high])
            }
            ExprKind::Convert { to, value } => match CType::value(to) {
                ty @ (CType::Int | CType::Double | CType::Char | CType::Bool) => {
                    CExpr::new(CExprKind::Cast {
                        ty,
                        value: Box::new(self.expr(value)),
                    })
                }
                _ => CExpr::error(),
            },
        }
    }

    /// Desugars a binary operation. When text is joined with `+` or compared
    /// and neither side is a `std::string` object (e.g. two literals), the left
    /// side becomes one, so C++ does not add or compare pointers.
    fn binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr, ty: &Type) -> CExpr {
        let mut left = self.expr(lhs);
        let right = self.expr(rhs);
        let text_op = (op == BinaryOp::Add && *ty == Type::String)
            || (is_comparison(op) && lhs.ty == Type::String && rhs.ty == Type::String);
        if text_op && !is_std_string(lhs) && !is_std_string(rhs) {
            left = self.std_string_of(lhs, left);
        }
        CExpr::binary(bin_op(op), left, right)
    }

    /// The condition of `repeat until <cond>`: `!(cond)`, simplified where
    /// that is exact: an inverted comparison (`a != b` for `!(a == b)`), the
    /// operand of a `not`, or the opposite literal.
    pub(super) fn negated_condition(&mut self, cond: &Expr) -> CExpr {
        if cond.ty != Type::Error && self.depth < MAX_DEPTH {
            match &cond.kind {
                ExprKind::Binary { op, lhs, rhs } => {
                    if let Some(inverse) = inverse_comparison(*op, &lhs.ty, &rhs.ty) {
                        return self
                            .binary(inverse, lhs, rhs, &cond.ty)
                            .with_origin(cond.origin.clone());
                    }
                }
                ExprKind::Unary {
                    op: UnaryOp::Not,
                    operand,
                } => {
                    return self.expr(operand).with_origin(cond.origin.clone());
                }
                ExprKind::Bool(value) => {
                    return CExpr::fixed(bool_token(!value)).with_origin(cond.origin.clone());
                }
                _ => {}
            }
        }
        CExpr::unary(UnOp::Not, self.expr(cond))
    }

    /// Converts a desugared expression to a `std::string` object:
    /// `std::string(text)`, `std::string(1, c)`, `std::to_string(n)` or
    /// `std::string(b ? "true" : "false")`.
    pub(super) fn std_string_of(&mut self, expr: &Expr, lowered: CExpr) -> CExpr {
        self.include("<string>");
        match expr.ty {
            Type::String if is_std_string(expr) => lowered,
            Type::String => CExpr::call_fixed("std::string", vec![lowered]),
            Type::Char => CExpr::call_fixed("std::string", vec![int_literal(1), lowered]),
            Type::Int | Type::Double => CExpr::call_fixed("std::to_string", vec![lowered]),
            Type::Bool => {
                let text = bool_text(expr, lowered);
                CExpr::call_fixed("std::string", vec![text])
            }
            Type::Void | Type::Error => lowered,
        }
    }

    /// `join (a) (b) …` as `std::string` concatenation (spec §3.7.4).
    fn join(&mut self, items: &[Expr]) -> CExpr {
        self.include("<string>");
        let Some((first, rest)) = items.split_first() else {
            return CExpr::call_fixed("std::string", Vec::new());
        };
        let lowered = self.expr(first);
        let mut joined = self.std_string_of(first, lowered);
        for item in rest {
            let lowered = self.expr(item);
            let part = match item.ty {
                Type::Char => CExpr::call_fixed("std::string", vec![int_literal(1), lowered]),
                Type::Int | Type::Double => CExpr::call_fixed("std::to_string", vec![lowered]),
                Type::Bool => bool_text(item, lowered),
                Type::String | Type::Void | Type::Error => lowered,
            };
            joined = CExpr::binary(BinOp::Add, joined, part);
        }
        joined
    }

    /// A value to print with `<<`: `bool` values print as `true`/`false`.
    pub(super) fn print_operand(&mut self, expr: &Expr) -> CExpr {
        let lowered = self.expr(expr);
        if expr.ty == Type::Bool {
            bool_text(expr, lowered)
        } else {
            lowered
        }
    }

    /// The prompt argument of `b2c::ask<T>` / `b2c::ask_line`, which take a
    /// `const std::string&`. A character literal becomes a one-character
    /// string literal; other non-text values are converted.
    pub(super) fn prompt_arg(&mut self, prompt: &Expr) -> CExpr {
        let lowered = self.expr(prompt);
        match (&prompt.ty, &prompt.kind) {
            (Type::String, _) => lowered,
            (Type::Char, ExprKind::Char(c)) if lowered.kind != CExprKind::Error => {
                match StrLit::new(c.value().encode_utf8(&mut [0; 4])) {
                    Ok(text) => replace_kind(lowered, CExprKind::Str(text)),
                    Err(_) => self.std_string_of(prompt, lowered),
                }
            }
            (Type::Bool, _) => bool_text(prompt, lowered),
            _ => self.std_string_of(prompt, lowered),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_inversion_respects_nan() {
        assert_eq!(
            inverse_comparison(BinaryOp::Eq, &Type::Double, &Type::Int),
            Some(BinaryOp::Ne)
        );
        assert_eq!(inverse_comparison(BinaryOp::Lt, &Type::Double, &Type::Int), None);
        assert_eq!(
            inverse_comparison(BinaryOp::Lt, &Type::Int, &Type::Char),
            Some(BinaryOp::Ge)
        );
        assert_eq!(
            inverse_comparison(BinaryOp::Ge, &Type::String, &Type::String),
            Some(BinaryOp::Lt)
        );
        assert_eq!(
            inverse_comparison(BinaryOp::Le, &Type::Int, &Type::Int),
            Some(BinaryOp::Gt)
        );
        assert_eq!(
            inverse_comparison(BinaryOp::Gt, &Type::Int, &Type::Int),
            Some(BinaryOp::Le)
        );
        assert_eq!(inverse_comparison(BinaryOp::Eq, &Type::Error, &Type::Int), None);
        assert_eq!(inverse_comparison(BinaryOp::And, &Type::Bool, &Type::Bool), None);
    }
}
