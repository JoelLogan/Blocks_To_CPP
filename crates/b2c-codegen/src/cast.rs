//! The C++ AST (CAST): a syntax-level tree produced by desugaring
//! (`lower`) and printed by `emit` (spec §6.8.1).
//!
//! Every leaf that can carry user text is one of the typed text leaves of
//! [`b2c_ir::text`]. Text chosen by the generator itself is a `&'static str`,
//! so there is no way to put an arbitrary runtime string into the tree.
//!
//! Parentheses are explicit [`CExprKind::Paren`] nodes. The smart constructors
//! ([`CExpr::binary`], [`CExpr::unary`], [`CExpr::conditional`]) insert them
//! where C++ precedence or associativity requires, plus a few clarity
//! parentheses that also keep `-Wall` quiet (mixed `&&`/`||`, a comparison
//! inside a comparison, `!x` on the left of a comparison).

use b2c_ir::sast::Origin;
use b2c_ir::text::{CharLit, Comment, Ident, NumLit, StrLit};
use b2c_ir::types::Type;

/// A C++ type as written in a declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CType {
    /// `void`.
    Void,
    /// `bool`.
    Bool,
    /// `char`.
    Char,
    /// `int`.
    Int,
    /// `double`.
    Double,
    /// `std::string`.
    String,
    /// `auto`.
    Auto,
    /// A type that could not be determined (best-effort output after errors).
    Error,
}

impl CType {
    /// The C++ type for a value of a static type. `Void` and `Error` have no
    /// value type and map to [`CType::Error`].
    pub(crate) fn value(ty: &Type) -> Self {
        match ty {
            Type::Bool => Self::Bool,
            Type::Char => Self::Char,
            Type::Int => Self::Int,
            Type::Double => Self::Double,
            Type::String => Self::String,
            Type::Void | Type::Error => Self::Error,
        }
    }

    /// The C++ type for a function result (`void` allowed).
    pub(crate) fn result(ty: &Type) -> Self {
        if *ty == Type::Void {
            Self::Void
        } else {
            Self::value(ty)
        }
    }

    /// The C++ spelling.
    pub(crate) fn spelling(self) -> &'static str {
        match self {
            Self::Void => "void",
            Self::Bool => "bool",
            Self::Char => "char",
            Self::Int => "int",
            Self::Double => "double",
            Self::String => "std::string",
            Self::Auto => "auto",
            Self::Error => "int /* error */",
        }
    }
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnOp {
    /// `-x`
    Neg,
    /// `+x`
    Plus,
    /// `!x`
    Not,
}

impl UnOp {
    /// The operator token.
    pub(crate) fn token(self) -> &'static str {
        match self {
            Self::Neg => "-",
            Self::Plus => "+",
            Self::Not => "!",
        }
    }
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinOp {
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Mod,
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `<<` (stream insertion)
    Shl,
    /// `>>` (stream extraction)
    Shr,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `&&`
    And,
    /// `||`
    Or,
}

impl BinOp {
    /// The operator token.
    pub(crate) fn token(self) -> &'static str {
        match self {
            Self::Mul => "*",
            Self::Div => "/",
            Self::Mod => "%",
            Self::Add => "+",
            Self::Sub => "-",
            Self::Shl => "<<",
            Self::Shr => ">>",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::And => "&&",
            Self::Or => "||",
        }
    }

    /// The operator's precedence.
    pub(crate) fn precedence(self) -> Prec {
        match self {
            Self::Mul | Self::Div | Self::Mod => Prec::Multiplicative,
            Self::Add | Self::Sub => Prec::Additive,
            Self::Shl | Self::Shr => Prec::Shift,
            Self::Lt | Self::Le | Self::Gt | Self::Ge => Prec::Relational,
            Self::Eq | Self::Ne => Prec::Equality,
            Self::And => Prec::LogicalAnd,
            Self::Or => Prec::LogicalOr,
        }
    }

    /// Whether this is a comparison (`<`, `<=`, `>`, `>=`, `==`, `!=`).
    pub(crate) fn is_comparison(self) -> bool {
        matches!(self.precedence(), Prec::Relational | Prec::Equality)
    }
}

/// C++ operator precedence levels used by the generator, loosest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Prec {
    /// `c ? a : b` (right-associative).
    Conditional,
    /// `||`
    LogicalOr,
    /// `&&`
    LogicalAnd,
    /// `==`, `!=`
    Equality,
    /// `<`, `<=`, `>`, `>=`
    Relational,
    /// `<<`, `>>`
    Shift,
    /// `+`, `-`
    Additive,
    /// `*`, `/`, `%`
    Multiplicative,
    /// Prefix `-`, `+`, `!`
    Unary,
    /// Names, literals, calls, casts and parenthesised expressions.
    Primary,
}

/// The function a call names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Callee {
    /// A user function.
    User(Ident),
    /// A standard-library function or support helper chosen by the generator
    /// (e.g. `std::to_string`, `b2c::random_int`).
    Fixed(&'static str),
}

/// An expression with the origins (blocks) it maps back to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CExpr {
    /// The expression.
    pub(crate) kind: CExprKind,
    /// Where it came from; each origin gets a source-map range.
    pub(crate) origins: Vec<Origin>,
}

/// Kinds of C++ expressions.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CExprKind {
    /// A numeric literal.
    Num(NumLit),
    /// A string literal (user text).
    Str(StrLit),
    /// A character literal (user text).
    Char(CharLit),
    /// A primary token chosen by the generator: `true`, `std::cout`, `'\n'`, …
    Fixed(&'static str),
    /// A variable, parameter or function name.
    Name(Ident),
    /// A prefix operator.
    Unary {
        /// Operator.
        op: UnOp,
        /// Operand (already parenthesised if needed).
        operand: Box<CExpr>,
    },
    /// A binary operator.
    Binary {
        /// Operator.
        op: BinOp,
        /// Left operand (already parenthesised if needed).
        lhs: Box<CExpr>,
        /// Right operand (already parenthesised if needed).
        rhs: Box<CExpr>,
    },
    /// `cond ? then_value : else_value`.
    Conditional {
        /// Condition.
        cond: Box<CExpr>,
        /// Value when true.
        then_value: Box<CExpr>,
        /// Value when false.
        else_value: Box<CExpr>,
    },
    /// A function call.
    Call {
        /// The function.
        callee: Callee,
        /// Arguments.
        args: Vec<CExpr>,
    },
    /// `static_cast<ty>(value)`.
    Cast {
        /// Target type.
        ty: CType,
        /// Converted value.
        value: Box<CExpr>,
    },
    /// `(inner)`.
    Paren(Box<CExpr>),
    /// `0 /* error */`: an expression that could not be generated.
    Error,
}

impl CExpr {
    /// An expression without an origin.
    pub(crate) fn new(kind: CExprKind) -> Self {
        Self {
            kind,
            origins: Vec::new(),
        }
    }

    /// A fixed primary token.
    pub(crate) fn fixed(text: &'static str) -> Self {
        Self::new(CExprKind::Fixed(text))
    }

    /// The error placeholder `0 /* error */`.
    pub(crate) fn error() -> Self {
        Self::new(CExprKind::Error)
    }

    /// Adds an origin (kept after the existing ones).
    #[must_use]
    pub(crate) fn with_origin(mut self, origin: Origin) -> Self {
        self.origins.push(origin);
        self
    }

    /// A call of a generator-chosen function.
    pub(crate) fn call_fixed(name: &'static str, args: Vec<Self>) -> Self {
        Self::new(CExprKind::Call {
            callee: Callee::Fixed(name),
            args,
        })
    }

    /// The expression's precedence.
    pub(crate) fn precedence(&self) -> Prec {
        match &self.kind {
            CExprKind::Unary { .. } => Prec::Unary,
            CExprKind::Binary { op, .. } => op.precedence(),
            CExprKind::Conditional { .. } => Prec::Conditional,
            CExprKind::Num(_)
            | CExprKind::Str(_)
            | CExprKind::Char(_)
            | CExprKind::Fixed(_)
            | CExprKind::Name(_)
            | CExprKind::Call { .. }
            | CExprKind::Cast { .. }
            | CExprKind::Paren(_)
            | CExprKind::Error => Prec::Primary,
        }
    }

    /// Wraps the expression in parentheses.
    #[must_use]
    pub(crate) fn paren(self) -> Self {
        Self::new(CExprKind::Paren(Box::new(self)))
    }

    /// Wraps the expression in parentheses if `wrap` is true.
    #[must_use]
    fn paren_if(self, wrap: bool) -> Self {
        if wrap { self.paren() } else { self }
    }

    /// A prefix operator, parenthesising the operand when needed. `-(-x)` and
    /// `+(+x)` keep their parentheses so they never print as `--x` / `++x`.
    pub(crate) fn unary(op: UnOp, operand: Self) -> Self {
        let same_sign = matches!(
            (&op, &operand.kind),
            (UnOp::Neg, CExprKind::Unary { op: UnOp::Neg, .. })
                | (UnOp::Plus, CExprKind::Unary { op: UnOp::Plus, .. })
        );
        let wrap = operand.precedence() < Prec::Unary || same_sign;
        Self::new(CExprKind::Unary {
            op,
            operand: Box::new(operand.paren_if(wrap)),
        })
    }

    /// A binary operator, parenthesising operands where precedence,
    /// associativity or clarity requires.
    pub(crate) fn binary(op: BinOp, lhs: Self, rhs: Self) -> Self {
        let prec = op.precedence();
        let wrap_lhs = lhs.precedence() < prec || needs_clarity_parens(op, &lhs, true);
        // Left-associative: an operand of equal precedence on the right needs
        // parentheses, except for `&&`/`||` chains, which are associative.
        let same_logical = matches!(
            (&op, &rhs.kind),
            (BinOp::And, CExprKind::Binary { op: BinOp::And, .. })
        ) || matches!(
            (&op, &rhs.kind),
            (BinOp::Or, CExprKind::Binary { op: BinOp::Or, .. })
        );
        let wrap_rhs = (rhs.precedence() <= prec && !same_logical) || needs_clarity_parens(op, &rhs, false);
        Self::new(CExprKind::Binary {
            op,
            lhs: Box::new(lhs.paren_if(wrap_lhs)),
            rhs: Box::new(rhs.paren_if(wrap_rhs)),
        })
    }

    /// `cond ? then_value : else_value`. A nested conditional is parenthesised
    /// in the condition and the middle, but not in the last operand, so
    /// `a ? b : c ? d : e` chains read naturally.
    pub(crate) fn conditional(cond: Self, then_value: Self, else_value: Self) -> Self {
        let wrap_cond = cond.precedence() <= Prec::Conditional;
        let wrap_then = then_value.precedence() <= Prec::Conditional;
        Self::new(CExprKind::Conditional {
            cond: Box::new(cond.paren_if(wrap_cond)),
            then_value: Box::new(then_value.paren_if(wrap_then)),
            else_value: Box::new(else_value),
        })
    }
}

/// Clarity parentheses that C++ does not require but GCC's `-Wparentheses`
/// and `-Wlogical-not-parentheses` (in `-Wall`) ask for.
fn needs_clarity_parens(op: BinOp, operand: &CExpr, is_lhs: bool) -> bool {
    match &operand.kind {
        // `a || (b && c)`
        CExprKind::Binary { op: BinOp::And, .. } => op == BinOp::Or,
        // `(a < b) == c`
        CExprKind::Binary { op: inner, .. } if inner.is_comparison() => op.is_comparison(),
        // `(!a) == b`
        CExprKind::Unary { op: UnOp::Not, .. } => is_lhs && op.is_comparison(),
        _ => false,
    }
}

/// A sequence of statements in braces.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct CBlock {
    /// Statements in order.
    pub(crate) stmts: Vec<CStmt>,
}

/// A statement with the origin it maps back to and its comment.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CStmt {
    /// The statement.
    pub(crate) kind: CStmtKind,
    /// The block it came from.
    pub(crate) origin: Option<Origin>,
    /// The block's comment, printed above the statement.
    pub(crate) comment: Option<Comment>,
}

impl CStmt {
    /// A statement without origin or comment.
    pub(crate) fn new(kind: CStmtKind) -> Self {
        Self {
            kind,
            origin: None,
            comment: None,
        }
    }
}

/// Assignment operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssignOp {
    /// `=`
    Set,
    /// `+=`
    Add,
    /// `-=`
    Sub,
    /// `*=`
    Mul,
    /// `/=`
    Div,
    /// `%=`
    Mod,
}

impl AssignOp {
    /// The operator token.
    pub(crate) fn token(self) -> &'static str {
        match self {
            Self::Set => "=",
            Self::Add => "+=",
            Self::Sub => "-=",
            Self::Mul => "*=",
            Self::Div => "/=",
            Self::Mod => "%=",
        }
    }
}

/// `++x` or `--x`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IncOp {
    /// `++`
    Inc,
    /// `--`
    Dec,
}

impl IncOp {
    /// The operator token.
    pub(crate) fn token(self) -> &'static str {
        match self {
            Self::Inc => "++",
            Self::Dec => "--",
        }
    }
}

/// How a declared variable is initialised.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Init {
    /// `= value`
    Value(CExpr),
    /// `{}` (value-initialisation).
    Braces,
}

/// The third clause of a `for` loop.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ForStep {
    /// `++i` / `--i`.
    Inc(IncOp, Ident),
    /// `i += step` / `i -= step`.
    Assign(AssignOp, Ident, CExpr),
}

/// One declarator in a `for` loop's init clause: `name = value`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ForDecl {
    /// Variable name.
    pub(crate) name: Ident,
    /// Initial value.
    pub(crate) value: CExpr,
}

/// Kinds of C++ statements.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CStmtKind {
    /// `[const] T name = value;` or `[const] T name{};`
    Decl {
        /// Whether the variable is `const`.
        is_const: bool,
        /// Its type.
        ty: CType,
        /// Its name.
        name: Ident,
        /// Its initialiser.
        init: Init,
    },
    /// `expr;`
    Expr(CExpr),
    /// `target op value;`
    Assign {
        /// Variable assigned to.
        target: Ident,
        /// Operator.
        op: AssignOp,
        /// Right-hand side.
        value: CExpr,
    },
    /// `++target;` / `--target;`
    Inc {
        /// Variable updated.
        target: Ident,
        /// Operator.
        op: IncOp,
    },
    /// `if (…) { … } else if (…) { … } else { … }`
    If {
        /// Condition and body of the `if` and each `else if`.
        branches: Vec<(CExpr, CBlock)>,
        /// The `else` body.
        else_body: Option<CBlock>,
    },
    /// `while (cond) { … }`
    While {
        /// Condition.
        cond: CExpr,
        /// Body.
        body: CBlock,
    },
    /// `for (T a = x, b = y; cond; step) { … }`
    For {
        /// Type of the declared variables.
        ty: CType,
        /// Declared variables (at least one).
        decls: Vec<ForDecl>,
        /// Loop condition.
        cond: CExpr,
        /// Step clause.
        step: ForStep,
        /// Body.
        body: CBlock,
    },
    /// `break;`
    Break,
    /// `continue;`
    Continue,
    /// `return;` / `return value;`
    Return(Option<CExpr>),
    /// Several statements generated for one block, printed one per line.
    Seq(Vec<CStmt>),
    /// `{ … }`
    Block(CBlock),
    /// `/* error */;`: a statement that could not be generated.
    Error,
}

/// How a parameter is passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParamStyle {
    /// `T x`
    Value,
    /// `T& x`
    Ref,
    /// `const T& x`
    ConstRef,
}

/// A function parameter.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CParam {
    /// Its type.
    pub(crate) ty: CType,
    /// How it is passed.
    pub(crate) style: ParamStyle,
    /// Its name.
    pub(crate) name: Ident,
    /// Where it was declared.
    pub(crate) origin: Origin,
}

/// A function definition (`main` included).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CFunction {
    /// Return type.
    pub(crate) ret: CType,
    /// Name.
    pub(crate) name: Ident,
    /// Parameters.
    pub(crate) params: Vec<CParam>,
    /// Body.
    pub(crate) body: CBlock,
    /// The defining block.
    pub(crate) origin: Origin,
    /// The block's comment.
    pub(crate) comment: Option<Comment>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> CExpr {
        CExpr::new(CExprKind::Name(Ident::new(text).unwrap()))
    }

    fn is_paren(e: &CExpr) -> bool {
        matches!(e.kind, CExprKind::Paren(_))
    }

    fn operands(e: &CExpr) -> (&CExpr, &CExpr) {
        match &e.kind {
            CExprKind::Binary { lhs, rhs, .. } => (lhs, rhs),
            _ => panic!("not binary"),
        }
    }

    #[test]
    fn left_associative_operators_parenthesise_the_right_operand() {
        let e = CExpr::binary(
            BinOp::Sub,
            name("a"),
            CExpr::binary(BinOp::Sub, name("b"), name("c")),
        );
        let (lhs, rhs) = operands(&e);
        assert!(!is_paren(lhs));
        assert!(is_paren(rhs));
        let e = CExpr::binary(
            BinOp::Sub,
            CExpr::binary(BinOp::Sub, name("a"), name("b")),
            name("c"),
        );
        let (lhs, _) = operands(&e);
        assert!(!is_paren(lhs));
    }

    #[test]
    fn logical_chains_and_clarity() {
        let e = CExpr::binary(
            BinOp::And,
            name("a"),
            CExpr::binary(BinOp::And, name("b"), name("c")),
        );
        assert!(!is_paren(operands(&e).1));
        let e = CExpr::binary(
            BinOp::Or,
            name("a"),
            CExpr::binary(BinOp::And, name("b"), name("c")),
        );
        assert!(is_paren(operands(&e).1));
        let e = CExpr::binary(
            BinOp::Eq,
            CExpr::binary(BinOp::Lt, name("a"), name("b")),
            name("c"),
        );
        assert!(is_paren(operands(&e).0));
        let e = CExpr::binary(BinOp::Eq, CExpr::unary(UnOp::Not, name("a")), name("b"));
        assert!(is_paren(operands(&e).0));
    }

    #[test]
    fn unary_operands() {
        let e = CExpr::unary(UnOp::Neg, CExpr::unary(UnOp::Neg, name("a")));
        let CExprKind::Unary { operand, .. } = &e.kind else {
            panic!()
        };
        assert!(is_paren(operand));
        let e = CExpr::unary(UnOp::Neg, CExpr::unary(UnOp::Plus, name("a")));
        let CExprKind::Unary { operand, .. } = &e.kind else {
            panic!()
        };
        assert!(!is_paren(operand));
        let e = CExpr::unary(UnOp::Not, CExpr::binary(BinOp::Lt, name("a"), name("b")));
        let CExprKind::Unary { operand, .. } = &e.kind else {
            panic!()
        };
        assert!(is_paren(operand));
    }

    #[test]
    fn type_spellings() {
        assert_eq!(CType::value(&Type::String).spelling(), "std::string");
        assert_eq!(CType::result(&Type::Void).spelling(), "void");
        assert_eq!(CType::value(&Type::Void), CType::Error);
    }
}
