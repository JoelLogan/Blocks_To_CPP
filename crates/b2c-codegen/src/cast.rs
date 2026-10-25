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

use std::collections::BTreeSet;

use b2c_ir::headers::Header;
use b2c_ir::sast::{Origin, SymbolKind, SymbolTable};
use b2c_ir::text::{CharLit, Comment, Ident, NumLit, StrLit};
use b2c_ir::types::{LibType, Type};

/// A C++ type as written in a declaration (types.md §5.1).
///
/// It is made of names from closed tables (the keywords, [`LibType`]'s
/// qualified names) and validated [`Ident`]s, so the emitter composes it from
/// `&'static str` text and identifiers and a type never carries other text.
/// Standard names are spelt in full: 06 §6.14.5 shortens them only inside a
/// region of a `using namespace` directive, and no project has one before
/// *Textbook style* and the `use namespace` block arrive.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
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
    /// `std::size_t`: what an `int` index into a vector, deque or string is
    /// cast to (types.md §5.2).
    Size,
    /// A standard-library type with its template arguments:
    /// `std::vector<int>`, `std::map<std::string, int>`, `std::ifstream`.
    Lib(LibType, Vec<CType>),
    /// A user struct or enum, by its qualified name: the identifiers joined
    /// by `::` (`Point`; `geo::Point` once namespaces arrive).
    Named(Vec<Ident>),
    /// `auto`.
    Auto,
    /// A type that could not be determined (best-effort output after errors).
    Error,
}

impl CType {
    /// The C++ type for a value of a static type. `Void` and `Error` have no
    /// value type and map to [`CType::Error`], and so does a user type whose
    /// symbol is missing or is not a struct or an enum, and a library type
    /// with such an argument (best-effort output after errors).
    pub(crate) fn value(ty: &Type, symbols: &SymbolTable) -> Self {
        match ty {
            Type::Bool => Self::Bool,
            Type::Char => Self::Char,
            Type::Int => Self::Int,
            Type::Double => Self::Double,
            Type::String => Self::String,
            Type::Lib(app) => {
                let mut args = Vec::with_capacity(app.args().len());
                for arg in app.args() {
                    match Self::value(arg, symbols) {
                        Self::Error => return Self::Error,
                        arg => args.push(arg),
                    }
                }
                Self::Lib(app.ty(), args)
            }
            Type::Named(id) => match symbols.get(id) {
                Some(symbol)
                    if matches!(symbol.kind, SymbolKind::Struct { .. } | SymbolKind::Enum { .. }) =>
                {
                    Self::Named(vec![symbol.name.clone()])
                }
                _ => Self::Error,
            },
            Type::Void | Type::Error => Self::Error,
        }
    }

    /// The C++ type for a function result (`void` allowed).
    pub(crate) fn result(ty: &Type, symbols: &SymbolTable) -> Self {
        if *ty == Type::Void {
            Self::Void
        } else {
            Self::value(ty, symbols)
        }
    }

    /// The spelling of a type that is one keyword or standard name; `None`
    /// for library and user types, which are composed of several names.
    pub(crate) fn keyword(&self) -> Option<&'static str> {
        Some(match self {
            Self::Void => "void",
            Self::Bool => "bool",
            Self::Char => "char",
            Self::Int => "int",
            Self::Double => "double",
            Self::String => "std::string",
            Self::Size => "std::size_t",
            Self::Auto => "auto",
            Self::Error => "int /* error */",
            Self::Lib(..) | Self::Named(_) => return None,
        })
    }

    /// Adds the standard headers that spelling the type needs (types.md
    /// §5.2): `<string>`, `<cstddef>` for `std::size_t`, and each library
    /// type's header with those of its arguments. User types need none.
    pub(crate) fn collect_headers(&self, headers: &mut BTreeSet<Header>) {
        match self {
            Self::String => headers.extend(Header::standard("<string>")),
            Self::Size => headers.extend(Header::standard("<cstddef>")),
            Self::Lib(ty, args) => {
                headers.insert(ty.header());
                for arg in args {
                    arg.collect_headers(headers);
                }
            }
            Self::Void
            | Self::Bool
            | Self::Char
            | Self::Int
            | Self::Double
            | Self::Named(_)
            | Self::Auto
            | Self::Error => {}
        }
    }

    /// The length in bytes of the type as printed.
    pub(crate) fn width(&self) -> usize {
        match self {
            Self::Lib(ty, args) if args.is_empty() => ty.cpp_name().len(),
            // `name<a, b>`
            Self::Lib(ty, args) => {
                ty.cpp_name().len() + 2 + args.iter().map(Self::width).sum::<usize>() + 2 * (args.len() - 1)
            }
            // `a::b`
            Self::Named(path) => {
                path.iter().map(|part| part.as_str().len()).sum::<usize>() + 2 * path.len().saturating_sub(1)
            }
            other => other.keyword().map_or(0, str::len),
        }
    }

    /// The type as printed, for ordering overloads only (the emitter prints
    /// types through the printer).
    pub(crate) fn sort_key(&self) -> String {
        let mut key = String::new();
        self.write_key(&mut key);
        key
    }

    fn write_key(&self, key: &mut String) {
        match self {
            Self::Lib(ty, args) => {
                key.push_str(ty.cpp_name());
                if !args.is_empty() {
                    key.push('<');
                    for (index, arg) in args.iter().enumerate() {
                        if index > 0 {
                            key.push_str(", ");
                        }
                        arg.write_key(key);
                    }
                    key.push('>');
                }
            }
            Self::Named(path) => {
                for (index, part) in path.iter().enumerate() {
                    if index > 0 {
                        key.push_str("::");
                    }
                    key.push_str(part.as_str());
                }
            }
            other => key.push_str(other.keyword().unwrap_or_default()),
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
    /// `object.name(args)`: a member function of a library type, chosen by
    /// the generator (`at`, `size`; types.md §5.2).
    #[cfg_attr(not(test), allow(dead_code, reason = "made for W2's collection blocks"))]
    Method {
        /// The object (already parenthesised if needed).
        object: Box<CExpr>,
        /// The member function.
        name: &'static str,
        /// Arguments.
        args: Vec<CExpr>,
    },
    /// `object.field`: a field of a user struct.
    #[cfg_attr(not(test), allow(dead_code, reason = "made for W2's struct blocks"))]
    Field {
        /// The object (already parenthesised if needed).
        object: Box<CExpr>,
        /// The field.
        field: Ident,
    },
    /// `Color::Red`: an enumerator, after its enum's qualified name.
    #[cfg_attr(not(test), allow(dead_code, reason = "made for W2's enum blocks"))]
    Enumerator {
        /// The enum's qualified name.
        owner: Vec<Ident>,
        /// The enumerator.
        value: Ident,
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
            | CExprKind::Method { .. }
            | CExprKind::Field { .. }
            | CExprKind::Enumerator { .. }
            | CExprKind::Paren(_)
            | CExprKind::Error => Prec::Primary,
        }
    }

    /// `static_cast<ty>(value)`.
    pub(crate) fn cast(ty: CType, value: Self) -> Self {
        Self::new(CExprKind::Cast {
            ty,
            value: Box::new(value),
        })
    }

    /// `object.name(args)`, parenthesising the object when needed
    /// (`(a + b).size()`).
    #[cfg_attr(not(test), allow(dead_code, reason = "made for W2's collection blocks"))]
    pub(crate) fn method(object: Self, name: &'static str, args: Vec<Self>) -> Self {
        let wrap = object.precedence() < Prec::Primary;
        Self::new(CExprKind::Method {
            object: Box::new(object.paren_if(wrap)),
            name,
            args,
        })
    }

    /// `object.field`, parenthesising the object when needed.
    #[cfg_attr(not(test), allow(dead_code, reason = "made for W2's struct blocks"))]
    pub(crate) fn field(object: Self, field: Ident) -> Self {
        let wrap = object.precedence() < Prec::Primary;
        Self::new(CExprKind::Field {
            object: Box::new(object.paren_if(wrap)),
            field,
        })
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
    /// `for (const auto& x : range) { … }`, or with a structured binding
    /// `for (const auto& [k, v] : range) { … }` (types.md §1.5, §5.2).
    #[cfg_attr(not(test), allow(dead_code, reason = "made for W2's for-each blocks"))]
    RangeFor {
        /// What each item is bound to.
        binding: RangeBinding,
        /// The range: never a literal (a text literal becomes a
        /// `std::string` object first).
        range: CExpr,
        /// Body.
        body: CBlock,
    },
    /// `switch (value) { case label: … }`.
    #[cfg_attr(not(test), allow(dead_code, reason = "made for W2's enums and switch block"))]
    Switch {
        /// The value switched on.
        value: CExpr,
        /// The cases, in order.
        cases: Vec<CCase>,
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

/// What a range-`for` binds each item to: `const auto& x`, or the structured
/// binding `const auto& [k, v]` of a map's key and value.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(test), allow(dead_code, reason = "made for W2's for-each blocks"))]
pub(crate) enum RangeBinding {
    /// One name for the whole item.
    One(Ident),
    /// A key and a value (C++17 structured binding).
    Pair(Ident, Ident),
}

/// One `case label:` of a `switch` with the statements under it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CCase {
    /// The label: a literal or an enumerator.
    pub(crate) label: CExpr,
    /// The statements, printed one level deeper than the label.
    pub(crate) body: Vec<CStmt>,
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

/// A user type's definition with the functions its ticks and uses promise
/// (types.md §1.6, §5.2).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(test), allow(dead_code, reason = "made for W2's struct and enum blocks"))]
pub(crate) enum CTypeDef {
    /// `struct S { … };`
    Struct(CStruct),
    /// `enum class E { … };` and its print support.
    Enum(CEnum),
}

/// `struct S { fields; operators };`
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CStruct {
    /// Its name.
    pub(crate) name: Ident,
    /// Its fields, in order.
    pub(crate) fields: Vec<CField>,
    /// `auto operator<=>(const S&) const = default;`: a comparable struct in
    /// C++20.
    pub(crate) defaulted_compare: bool,
    /// Hidden friends: the six comparisons of a comparable struct in C++17,
    /// then the `operator<<` of a printable struct.
    pub(crate) friends: Vec<CGenerated>,
    /// The defining block.
    pub(crate) origin: Origin,
    /// The block's comment.
    pub(crate) comment: Option<Comment>,
}

/// A field of a struct: `T name{};` or `T name = value;`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CField {
    /// Its type.
    pub(crate) ty: CType,
    /// Its name.
    pub(crate) name: Ident,
    /// Its default member initialiser.
    pub(crate) init: Init,
    /// Where it was declared.
    pub(crate) origin: Origin,
}

/// `enum class E { A, B };` and the free functions that print it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CEnum {
    /// Its name.
    pub(crate) name: Ident,
    /// Its enumerators, in order.
    pub(crate) values: Vec<Ident>,
    /// `operator<<` (the enum is printed or written) and `to_string` (it is
    /// joined), printed after the definition.
    pub(crate) functions: Vec<CGenerated>,
    /// The defining block.
    pub(crate) origin: Origin,
    /// The block's comment.
    pub(crate) comment: Option<Comment>,
}

/// A function the generator writes for a user type `T` (types.md §5.2).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CGenerated {
    /// Which one; it gives the signature.
    pub(crate) kind: Generated,
    /// The parameters' names, one per parameter of [`Generated::params`];
    /// `None` leaves a parameter the body does not use unnamed.
    pub(crate) params: Vec<Option<Ident>>,
    /// The body.
    pub(crate) body: CBlock,
}

/// The functions the generator writes for user types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Generated {
    /// `friend bool operator==(const T& a, const T& b)`, and the other five
    /// comparisons: a hidden friend of a comparable struct in C++17.
    Compare(BinOp),
    /// `friend std::ostream& operator<<(std::ostream& out, const T& value)`:
    /// a hidden friend of a printable struct.
    PrintStruct,
    /// `std::ostream& operator<<(std::ostream& out, T value)`: an enum that
    /// is printed or written.
    PrintEnum,
    /// `std::string to_string(T value)`: an enum that is joined.
    EnumToString,
}

/// How a parameter of a [`Generated`] function is declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GeneratedParam {
    /// `std::ostream& out`.
    Stream,
    /// `const T& value`.
    ConstRef,
    /// `T value` (an enum, which is a scalar).
    Value,
}

impl Generated {
    /// The result type.
    pub(crate) fn ret(self) -> &'static str {
        match self {
            Self::Compare(_) => "bool",
            Self::PrintStruct | Self::PrintEnum => "std::ostream&",
            Self::EnumToString => "std::string",
        }
    }

    /// The function's name after `operator` (`==`, `<<`), or its whole name
    /// (`to_string`).
    pub(crate) fn name(self) -> (Option<&'static str>, &'static str) {
        match self {
            Self::Compare(op) => (Some("operator"), op.token()),
            Self::PrintStruct | Self::PrintEnum => (Some("operator"), "<<"),
            Self::EnumToString => (None, "to_string"),
        }
    }

    /// Whether it is a hidden friend, defined inside its struct.
    pub(crate) fn is_friend(self) -> bool {
        matches!(self, Self::Compare(_) | Self::PrintStruct)
    }

    /// Its parameters, in order.
    pub(crate) fn params(self) -> &'static [GeneratedParam] {
        match self {
            Self::Compare(_) => &[GeneratedParam::ConstRef, GeneratedParam::ConstRef],
            Self::PrintStruct => &[GeneratedParam::Stream, GeneratedParam::ConstRef],
            Self::PrintEnum => &[GeneratedParam::Stream, GeneratedParam::Value],
            Self::EnumToString => &[GeneratedParam::Value],
        }
    }
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
        let symbols = SymbolTable::default();
        assert_eq!(CType::value(&Type::String, &symbols).sort_key(), "std::string");
        assert_eq!(CType::result(&Type::Void, &symbols).sort_key(), "void");
        assert_eq!(CType::value(&Type::Void, &symbols), CType::Error);
    }
}
