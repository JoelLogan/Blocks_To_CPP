//! Type rules for the M1 types (spec §3.5.3, §6.6), as pure functions.
//!
//! The rules approximate C++: an error is reported only when g++ would reject
//! the code (or the generated code would certainly not mean what the blocks
//! say); legal but suspicious code gets a warning. [`Type::Error`] is
//! compatible with everything so that one mistake is reported once.

use b2c_ir::sast::Expr;
use b2c_ir::sast::{BinaryOp, ExprKind};
use b2c_ir::types::Type;

/// How a value of one type converts to another when it initialises a
/// variable, is passed as an argument or is returned (spec §3.5.3).
///
/// This is the analyser's own rule for those sites, published so that the
/// editor's connection checker refuses exactly what the analyser reports as
/// an error: only [`Conversion::Invalid`] is an error (`B2C-E0301`, or
/// `B2C-E0305` for a value that is `void`); [`Conversion::Narrowing`]
/// (`B2C-W0518`) and [`Conversion::BoolNumber`] (`B2C-W0519`) are warnings;
/// the others are silent.
///
/// Other sites differ. `set` (assignment) uses this rule but also accepts a
/// `char` for a `std::string` variable, as C++ does (`s = 'a';` compiles,
/// `std::string s = 'a';` does not), although [`conversion`] calls that
/// [`Conversion::Invalid`]. `change by` has rules of its own, and an input
/// with a catalog check class (such as the `text` prompt of `ask`) follows
/// that class, not this rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Conversion {
    /// The same type, or [`Type::Error`] on either side: the error is already
    /// reported where that value came from, so nothing more is said.
    Same,
    /// A safe implicit conversion: `int` → `double`, `char` → `int` or
    /// `double`.
    Widening,
    /// An implicit conversion that can lose information: `double` → `int` or
    /// `char`, `int` → `char`. Legal, but warned about.
    Narrowing,
    /// Between `bool` and a number (`int`, `double` or `char`), either way.
    /// Legal, but suspicious, so warned about.
    BoolNumber,
    /// Not possible: text to or from any other type, and `void` (a missing
    /// value) to or from any other type.
    Invalid,
}

impl Conversion {
    /// Every conversion, from the most to the least permissive.
    pub const ALL: [Self; 5] = [
        Self::Same,
        Self::Widening,
        Self::Narrowing,
        Self::BoolNumber,
        Self::Invalid,
    ];

    /// The name the editor uses for it in JSON: `same`, `widening`,
    /// `narrowing`, `boolNumber` or `invalid`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Same => "same",
            Self::Widening => "widening",
            Self::Narrowing => "narrowing",
            Self::BoolNumber => "boolNumber",
            Self::Invalid => "invalid",
        }
    }

    /// Whether the analyser reports this conversion as an error.
    pub const fn is_error(self) -> bool {
        matches!(self, Self::Invalid)
    }
}

/// The conversion from a value of type `from` to a place of type `to`.
///
/// Numbers are `int`, `double` and `char`. In order:
///
/// 1. [`Type::Error`] on either side, or the same type on both: `Same`.
/// 2. `bool` to a number, or a number to `bool`: `BoolNumber`.
/// 3. `int` or `char` to `double`, `char` to `int`: `Widening`.
/// 4. `double` to `int` or `char`, `int` to `char`: `Narrowing`.
/// 5. Anything else (text with any other type, `void` with any other type):
///    `Invalid`.
///
/// ```
/// use b2c_ir::Type;
/// use b2c_lang::{Conversion, conversion};
///
/// assert_eq!(conversion(&Type::Int, &Type::Double), Conversion::Widening);
/// assert_eq!(conversion(&Type::Double, &Type::Int), Conversion::Narrowing);
/// assert_eq!(conversion(&Type::String, &Type::Int), Conversion::Invalid);
/// assert_eq!(conversion(&Type::Error, &Type::String), Conversion::Same);
/// // Not for initialising; `set` accepts it anyway (see [`Conversion`]).
/// assert_eq!(conversion(&Type::Char, &Type::String), Conversion::Invalid);
/// ```
pub fn conversion(from: &Type, to: &Type) -> Conversion {
    use Type as T;
    match (from, to) {
        (T::Error, _) | (_, T::Error) => Conversion::Same,
        (a, b) if a == b => Conversion::Same,
        (T::Bool, T::Int | T::Double | T::Char) | (T::Int | T::Double | T::Char, T::Bool) => {
            Conversion::BoolNumber
        }
        (T::Int | T::Char, T::Double) | (T::Char, T::Int) => Conversion::Widening,
        (T::Double, T::Int | T::Char) | (T::Int, T::Char) => Conversion::Narrowing,
        _ => Conversion::Invalid,
    }
}

/// Whether a type is an integer type in C++ terms (`bool` included).
pub(crate) fn is_integral(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Char | Type::Bool)
}

/// Whether a variable of this type is a scalar that holds garbage-free but
/// meaningless zero when declared without a value.
pub(crate) fn is_scalar(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Double | Type::Char | Type::Bool)
}

/// The type of an arithmetic result after the usual promotions.
pub(crate) fn arithmetic_result(lhs: &Type, rhs: &Type) -> Type {
    if *lhs == Type::Double || *rhs == Type::Double {
        Type::Double
    } else {
        Type::Int
    }
}

/// Whether an expression divides two integers (so the result is truncated).
pub(crate) fn is_integer_division(expr: &Expr) -> bool {
    matches!(&expr.kind, ExprKind::Binary { op: BinaryOp::Div, lhs, rhs } if is_integral(&lhs.ty) && is_integral(&rhs.ty))
}

/// Classes of binary operators that share their typing rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpClass {
    /// `+ - * /`.
    Arithmetic,
    /// `%`.
    Remainder,
    /// `< <= > >=`.
    Ordering,
    /// `== !=`.
    Equality,
    /// `&& ||`.
    Logical,
}

/// The class of a binary operator.
pub(crate) fn op_class(op: BinaryOp) -> OpClass {
    match op {
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => OpClass::Arithmetic,
        BinaryOp::Mod => OpClass::Remainder,
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => OpClass::Ordering,
        BinaryOp::Eq | BinaryOp::Ne => OpClass::Equality,
        BinaryOp::And | BinaryOp::Or => OpClass::Logical,
    }
}

/// How an operator reads in a message ("add", "compare", …).
pub(crate) fn op_verb(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "add",
        BinaryOp::Sub => "subtract",
        BinaryOp::Mul => "multiply",
        BinaryOp::Div => "divide",
        BinaryOp::Mod => "take the remainder of",
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge | BinaryOp::Eq | BinaryOp::Ne => "compare",
        BinaryOp::And | BinaryOp::Or => "combine",
    }
}

/// The C++ spelling of an operator, for messages.
pub(crate) fn op_symbol(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Mod => "%",
        BinaryOp::Lt => "<",
        BinaryOp::Le => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::Ge => ">=",
        BinaryOp::Eq => "==",
        BinaryOp::Ne => "!=",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
    }
}

/// The common type of the two values of a conditional, if they have one.
/// `Ok((ty, suspicious))`: `suspicious` is true when a `bool` is mixed with a number.
pub(crate) fn conditional_result(a: &Type, b: &Type) -> Result<(Type, bool), ()> {
    use Type as T;
    match (a, b) {
        (T::Error, _) | (_, T::Error) => Ok((T::Error, false)),
        (x, y) if x == y && *x != T::Void => Ok((x.clone(), false)),
        (T::Int | T::Double | T::Char, T::Int | T::Double | T::Char) => Ok((arithmetic_result(a, b), false)),
        (T::Bool, T::Int | T::Double | T::Char) | (T::Int | T::Double | T::Char, T::Bool) => {
            Ok((arithmetic_result(a, b), true))
        }
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use b2c_ir::ids::{BlockId, ModuleId};
    use b2c_ir::sast::Origin;
    use b2c_ir::text::NumLit;

    const ALL: [Type; 7] = [
        Type::Void,
        Type::Bool,
        Type::Char,
        Type::Int,
        Type::Double,
        Type::String,
        Type::Error,
    ];

    #[test]
    fn conversions() {
        use Conversion as C;
        let cases = [
            (Type::Int, Type::Int, C::Same),
            (Type::Int, Type::Double, C::Widening),
            (Type::Char, Type::Double, C::Widening),
            (Type::Char, Type::Int, C::Widening),
            (Type::Double, Type::Int, C::Narrowing),
            (Type::Double, Type::Char, C::Narrowing),
            (Type::Int, Type::Char, C::Narrowing),
            (Type::Bool, Type::Int, C::BoolNumber),
            (Type::Double, Type::Bool, C::BoolNumber),
            (Type::String, Type::Int, C::Invalid),
            (Type::Int, Type::String, C::Invalid),
            (Type::Char, Type::String, C::Invalid),
            (Type::Bool, Type::String, C::Invalid),
            (Type::Void, Type::Int, C::Invalid),
            (Type::Error, Type::String, C::Same),
            (Type::String, Type::Error, C::Same),
        ];
        for (from, to, expected) in cases {
            assert_eq!(conversion(&from, &to), expected, "{from:?} -> {to:?}");
        }
        for t in &ALL {
            assert_eq!(conversion(t, t), C::Same);
            assert_eq!(conversion(t, &Type::Error), C::Same);
        }
    }

    #[test]
    fn conditionals() {
        assert_eq!(conditional_result(&Type::Int, &Type::Int), Ok((Type::Int, false)));
        assert_eq!(
            conditional_result(&Type::Int, &Type::Double),
            Ok((Type::Double, false))
        );
        assert_eq!(
            conditional_result(&Type::Char, &Type::Char),
            Ok((Type::Char, false))
        );
        assert_eq!(
            conditional_result(&Type::Char, &Type::Int),
            Ok((Type::Int, false))
        );
        assert_eq!(conditional_result(&Type::Bool, &Type::Int), Ok((Type::Int, true)));
        assert_eq!(
            conditional_result(&Type::String, &Type::String),
            Ok((Type::String, false))
        );
        assert_eq!(conditional_result(&Type::String, &Type::Char), Err(()));
        assert_eq!(conditional_result(&Type::Void, &Type::Void), Err(()));
        assert_eq!(
            conditional_result(&Type::Error, &Type::String),
            Ok((Type::Error, false))
        );
    }

    #[test]
    fn operators() {
        assert_eq!(op_class(BinaryOp::Mod), OpClass::Remainder);
        assert_eq!(op_class(BinaryOp::Ge), OpClass::Ordering);
        assert_eq!(op_class(BinaryOp::Ne), OpClass::Equality);
        assert_eq!(op_class(BinaryOp::Or), OpClass::Logical);
        assert_eq!(op_symbol(BinaryOp::And), "&&");
        assert_eq!(op_verb(BinaryOp::Sub), "subtract");
        assert!(is_integral(&Type::Bool) && !is_integral(&Type::Double));
        assert!(is_scalar(&Type::Char) && !is_scalar(&Type::String));
        assert_eq!(arithmetic_result(&Type::Char, &Type::Char), Type::Int);
    }

    #[test]
    fn integer_division() {
        let origin = Origin::whole(ModuleId::new("m").expect("id"), BlockId::new("b").expect("id"));
        let int = |ty: Type| Expr {
            kind: ExprKind::Int(NumLit::int(1)),
            ty,
            origin: origin.clone(),
        };
        let div = |l: Type, r: Type, op: BinaryOp| Expr {
            kind: ExprKind::Binary {
                op,
                lhs: Box::new(int(l)),
                rhs: Box::new(int(r)),
            },
            ty: Type::Int,
            origin: origin.clone(),
        };
        assert!(is_integer_division(&div(Type::Int, Type::Int, BinaryOp::Div)));
        assert!(is_integer_division(&div(Type::Char, Type::Int, BinaryOp::Div)));
        assert!(!is_integer_division(&div(Type::Double, Type::Int, BinaryOp::Div)));
        assert!(!is_integer_division(&div(Type::Int, Type::Int, BinaryOp::Mul)));
        assert!(!is_integer_division(&int(Type::Int)));
    }
}
