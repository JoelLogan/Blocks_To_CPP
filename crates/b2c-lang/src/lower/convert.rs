//! Implicit conversions: assignment, initialisation, arguments, `return`,
//! conditions and integer slots (spec §3.5.3, §6.6).

use b2c_ir::sast::{Expr, ExprKind, UnaryOp};
use b2c_ir::types::Type;

use super::Lowerer;
use crate::codes;
use crate::messages::{a_type, quoted};
use crate::typing::{Conversion, conversion, is_integer_division};

/// What receives a value, for messages ("`score` needs a whole number, …").
#[derive(Debug, Clone)]
pub(super) enum Subject {
    /// A variable being created or set.
    Variable(String),
    /// A function argument.
    Argument {
        /// The function's name.
        function: String,
        /// The parameter's name.
        param: String,
    },
    /// The value a function gives back.
    Result(String),
    /// An exit code (`stop program`, or `return` in `main`).
    ExitCode,
    /// A condition (`if`, loops, `?:`).
    Condition,
    /// The count of `repeat`.
    RepeatCount,
    /// The start of a `for` loop.
    LoopStart,
    /// The end of a `for` loop.
    LoopEnd,
    /// The step of a `for` loop.
    LoopStep,
    /// The lower bound of `random integer`.
    RandomLow,
    /// The upper bound of `random integer`.
    RandomHigh,
}

impl Subject {
    /// Whether the value is stored, passed or given back (so a decimal part
    /// is dropped when it becomes a whole number), rather than being a count,
    /// bound or code that should simply be a whole number.
    fn stores_value(&self) -> bool {
        matches!(self, Self::Variable(_) | Self::Argument { .. } | Self::Result(_))
    }

    /// The subject at the start of a sentence.
    fn phrase(&self) -> String {
        match self {
            Self::Variable(name) => quoted(name),
            Self::Argument { function, param } => {
                format!("The {} input of {}", quoted(param), quoted(function))
            }
            Self::Result(function) => format!("The result of {}", quoted(function)),
            Self::ExitCode => String::from("The exit code"),
            Self::Condition => String::from("The condition"),
            Self::RepeatCount => String::from("The number of times to repeat"),
            Self::LoopStart => String::from("The start of the loop"),
            Self::LoopEnd => String::from("The end of the loop"),
            Self::LoopStep => String::from("The step of the loop"),
            Self::RandomLow => String::from("The lowest random number"),
            Self::RandomHigh => String::from("The highest random number"),
        }
    }
}

/// The value of an integer constant (a literal, possibly negated).
pub(super) fn constant_int(expr: &Expr) -> Option<i64> {
    match &expr.kind {
        ExprKind::Int(literal) => literal.as_str().parse().ok(),
        ExprKind::Unary {
            op: UnaryOp::Neg,
            operand,
        } => constant_int(operand).and_then(i64::checked_neg),
        ExprKind::Unary {
            op: UnaryOp::Plus,
            operand,
        } => constant_int(operand),
        _ => None,
    }
}

impl Lowerer<'_> {
    /// Checks that `value` can be used where `target` is expected.
    pub(super) fn check_conversion(&mut self, value: &Expr, target: &Type, subject: &Subject) {
        let location = value.origin.location();
        let phrase = subject.phrase();
        let (want, have) = (a_type(target), a_type(&value.ty));
        match conversion(&value.ty, target) {
            Conversion::Exact | Conversion::Widening => {}
            Conversion::Narrowing => {
                let message = if *target == Type::Char {
                    format!(
                        "{phrase} needs a character, but this is {have}, which may not fit in a character."
                    )
                } else if subject.stores_value() {
                    format!(
                        "{phrase} needs {want}, but this is {have}, so the part after the decimal point will be \
                         dropped. Use the 'convert' block to show that this is intended."
                    )
                } else {
                    format!(
                        "{phrase} should be {want}, but this is {have}. Use the 'convert' block to make it {want}."
                    )
                };
                self.diags.warning(codes::NARROWING, location.clone(), message);
            }
            Conversion::BoolNumber => {
                let message = if *target == Type::Bool {
                    format!(
                        "{phrase} needs a true/false value, but this is {have}: 0 counts as false and anything \
                         else as true. Use a comparison such as 'x ≠ 0' to make this clear."
                    )
                } else {
                    format!(
                        "{phrase} needs {want}, but this is a true/false value, which becomes 1 (true) or 0 (false)."
                    )
                };
                self.diags.warning(codes::BOOL_NUMBER, location.clone(), message);
            }
            Conversion::Invalid if value.ty == Type::Void => {
                let message = format!("{phrase} needs {want}, but this gives back nothing.");
                self.diags.error(codes::NO_VALUE, location.clone(), message);
            }
            Conversion::Invalid => {
                let hint = if *target == Type::String {
                    " Use the 'join' block to turn it into text."
                } else if value.ty == Type::String {
                    " Text is not turned into a number automatically."
                } else {
                    ""
                };
                self.diags.error(
                    codes::CONVERSION,
                    location.clone(),
                    format!("{phrase} needs {want}, but this is {have}.{hint}"),
                );
            }
        }
        if *target == Type::Double {
            self.warn_integer_division(value);
        }
    }

    /// Warns when `value` divides two integers where a decimal is wanted.
    pub(super) fn warn_integer_division(&mut self, value: &Expr) {
        if is_integer_division(value) {
            self.diags.warning(
                codes::INTEGER_DIVISION,
                value.origin.location(),
                "This divides two whole numbers, so the result is a whole number too (7 / 2 gives 3), even \
                 though a decimal number is wanted here. Write one of them as a decimal number (7.0 / 2) to get \
                 the exact result.",
            );
        }
    }

    /// Checks an integer slot (counts, bounds, exit codes) and returns the value.
    pub(super) fn integer(&mut self, value: Expr, subject: &Subject) -> Expr {
        self.check_conversion(&value, &Type::Int, subject);
        value
    }

    /// Checks a condition and returns it.
    pub(super) fn condition(&mut self, value: Expr) -> Expr {
        self.check_conversion(&value, &Type::Bool, &Subject::Condition);
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use b2c_ir::ids::{BlockId, ModuleId};
    use b2c_ir::sast::Origin;
    use b2c_ir::text::NumLit;

    #[test]
    fn constants() {
        let origin = Origin::whole(ModuleId::new("m").expect("id"), BlockId::new("b").expect("id"));
        let lit = Expr {
            kind: ExprKind::Int(NumLit::int(3)),
            ty: Type::Int,
            origin: origin.clone(),
        };
        assert_eq!(constant_int(&lit), Some(3));
        let neg = Expr {
            kind: ExprKind::Unary {
                op: UnaryOp::Neg,
                operand: Box::new(lit.clone()),
            },
            ty: Type::Int,
            origin: origin.clone(),
        };
        assert_eq!(constant_int(&neg), Some(-3));
        let plus = Expr {
            kind: ExprKind::Unary {
                op: UnaryOp::Plus,
                operand: Box::new(neg),
            },
            ty: Type::Int,
            origin: origin.clone(),
        };
        assert_eq!(constant_int(&plus), Some(-3));
        let other = Expr {
            kind: ExprKind::Bool(true),
            ty: Type::Bool,
            origin,
        };
        assert_eq!(constant_int(&other), None);
    }

    #[test]
    fn phrases() {
        assert_eq!(Subject::Variable(String::from("x")).phrase(), "`x`");
        let arg = Subject::Argument {
            function: String::from("f"),
            param: String::from("n"),
        };
        assert_eq!(arg.phrase(), "The `n` input of `f`");
        assert_eq!(Subject::Result(String::from("f")).phrase(), "The result of `f`");
        assert!(arg.stores_value() && Subject::Variable(String::from("x")).stores_value());
        assert!(!Subject::RepeatCount.stores_value() && !Subject::ExitCode.stores_value());
        for s in [
            Subject::ExitCode,
            Subject::Condition,
            Subject::RepeatCount,
            Subject::LoopStart,
            Subject::LoopEnd,
            Subject::LoopStep,
            Subject::RandomLow,
            Subject::RandomHigh,
        ] {
            assert!(s.phrase().starts_with("The "));
        }
    }
}
