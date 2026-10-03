//! The expression-slot parser (spec §3.4, milestone M1 subset).
//!
//! A slot holds a flat token list. This module turns it into a [`Syntax`]
//! tree with precedence climbing; it knows nothing about symbols or types (the
//! lowering resolves and checks the tree). The grammar, from lowest to highest
//! precedence:
//!
//! ```ebnf
//! expr           = logical_or [ "?" expr ":" expr ] ;
//! logical_or     = logical_and { ( "||" | "or" ) logical_and } ;
//! logical_and    = equality { ( "&&" | "and" ) equality } ;
//! equality       = relational { ( "==" | "!=" ) relational } ;
//! relational     = additive { ( "<" | "<=" | ">" | ">=" ) additive } ;
//! additive       = multiplicative { ( "+" | "-" ) multiplicative } ;
//! multiplicative = unary { ( "*" | "/" | "%" ) unary } ;
//! unary          = ( "!" | "not" | "-" | "+" ) unary | primary ;
//! primary        = num | str | chr | "true" | "false" | ref [ call_args ] | "(" expr ")" ;
//! call_args      = "(" [ expr { "," expr } ] ")" ;
//! ```
//!
//! Binary operators are left-associative and the conditional is
//! right-associative, as in C++. Two limits keep every later stage safe:
//!
//! * Nesting (parentheses, unary operators, call arguments and conditional
//!   branches) is limited to [`MAX_EXPR_DEPTH`] levels, so the parser's own
//!   recursion is bounded no matter what the token list holds.
//! * The resulting tree may have at most [`MAX_EXPR_DEPTH`] levels of
//!   operators and calls. A flat chain such as `a + b + c + …` is parsed
//!   without recursion, but it becomes a left-leaning tree one level deeper
//!   per operator, and the stages that walk the tree recursively (type
//!   checking, flow checks, code generation) must stay far from their own
//!   limits.
//!
//! A slot has at most [`MAX_EXPR_TOKENS`] tokens.

use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{BinaryOp, UnaryOp};
use b2c_model::Token;
use b2c_model::limits::{MAX_EXPR_DEPTH, MAX_EXPR_TOKENS};

/// A parsed expression with the token range it covers (`start..end`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Syntax<'t> {
    /// What it is.
    pub(crate) kind: SyntaxKind<'t>,
    /// Index of its first token.
    pub(crate) start: usize,
    /// Index one past its last token.
    pub(crate) end: usize,
    /// Number of operator levels in the tree: 0 for a literal or name, one
    /// more than the tallest operand for an operator or call. Parentheses do
    /// not add a level.
    pub(crate) levels: usize,
}

impl<'t> Syntax<'t> {
    /// A single-token node.
    fn leaf(kind: SyntaxKind<'t>, at: usize) -> Self {
        Self {
            kind,
            start: at,
            end: at + 1,
            levels: 0,
        }
    }

    /// An operator or call one level above its tallest operand
    /// (`operand_levels`), or a [`ParseErrorKind::TooDeep`] error at token
    /// `at` when that makes more than [`MAX_EXPR_DEPTH`] levels.
    fn node(
        kind: SyntaxKind<'t>,
        (start, end): (usize, usize),
        operand_levels: usize,
        at: usize,
    ) -> Result<Self, ParseError> {
        let levels = operand_levels + 1;
        if levels > MAX_EXPR_DEPTH {
            return Err(Parser::error(ParseErrorKind::TooDeep, at));
        }
        Ok(Self {
            kind,
            start,
            end,
            levels,
        })
    }
}

/// Kinds of parsed expressions.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SyntaxKind<'t> {
    /// Numeric literal text, without a sign.
    Number(&'t str),
    /// String literal value.
    Str(&'t str),
    /// Character literal value.
    Char(&'t str),
    /// `true` or `false`.
    Bool(bool),
    /// A reference to a symbol, not followed by `(`.
    Ref(&'t SymbolId),
    /// A call `f(a, b)`; the callee is the first token of the node.
    Call {
        /// The called symbol.
        callee: &'t SymbolId,
        /// Arguments in order.
        args: Vec<Syntax<'t>>,
    },
    /// A prefix operator.
    Unary {
        /// Operator.
        op: UnaryOp,
        /// Operand.
        operand: Box<Syntax<'t>>,
    },
    /// A binary operator.
    Binary {
        /// Operator.
        op: BinaryOp,
        /// Left operand.
        lhs: Box<Syntax<'t>>,
        /// Right operand.
        rhs: Box<Syntax<'t>>,
    },
    /// `cond ? then_value : else_value`.
    Conditional {
        /// Condition.
        cond: Box<Syntax<'t>>,
        /// Value when true.
        then_value: Box<Syntax<'t>>,
        /// Value when false.
        else_value: Box<Syntax<'t>>,
    },
}

/// Why a slot does not parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParseErrorKind {
    /// More than [`MAX_EXPR_TOKENS`] tokens.
    TooManyTokens,
    /// Nested more than [`MAX_EXPR_DEPTH`] levels, or more than
    /// [`MAX_EXPR_DEPTH`] levels of operators and calls.
    TooDeep,
    /// The slot is marked as an unfinished draft.
    Draft,
    /// The slot holds unparsed text.
    TextToken,
    /// The expression ends right after an operator.
    Incomplete,
    /// An operator or punctuation where a value is expected.
    ExpectedValue,
    /// A value directly after another value.
    MissingOperator,
    /// A `(` without its `)`.
    UnclosedParen,
    /// A `)` without its `(`.
    UnmatchedClose,
    /// A `?` without its `:`.
    MissingColon,
    /// A `:` without a `?`.
    StrayColon,
    /// A `,` outside a call's argument list.
    StrayComma,
    /// `(` after a value that is not a function reference.
    NotCallable,
    /// An operator that slots do not support (yet).
    UnsupportedOp,
    /// A keyword other than `true` or `false`.
    UnsupportedKeyword,
}

/// A parse error and the token range it points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParseError {
    /// What is wrong.
    pub(crate) kind: ParseErrorKind,
    /// First token of the problem.
    pub(crate) start: usize,
    /// One past the last token of the problem.
    pub(crate) end: usize,
}

/// Parses a slot's tokens. An empty list is [`ParseErrorKind::Incomplete`]
/// with an empty range (callers report empty slots as missing values).
pub(crate) fn parse(tokens: &[Token], draft: bool) -> Result<Syntax<'_>, ParseError> {
    let whole = |kind| ParseError {
        kind,
        start: 0,
        end: tokens.len(),
    };
    if tokens.is_empty() {
        return Err(whole(ParseErrorKind::Incomplete));
    }
    if tokens.len() > MAX_EXPR_TOKENS {
        return Err(whole(ParseErrorKind::TooManyTokens));
    }
    if let Some(i) = tokens.iter().position(|t| matches!(t, Token::Text(_))) {
        return Err(ParseError {
            kind: ParseErrorKind::TextToken,
            start: i,
            end: i + 1,
        });
    }
    if draft {
        return Err(whole(ParseErrorKind::Draft));
    }
    let mut parser = Parser { tokens, pos: 0 };
    let expr = parser.expr(0)?;
    if parser.pos < tokens.len() {
        return Err(parser.unexpected(Expected::End));
    }
    Ok(expr)
}

/// Binary operators with their precedence (higher binds tighter).
fn binary_op(op: &str) -> Option<(BinaryOp, u8)> {
    Some(match op {
        "||" | "or" => (BinaryOp::Or, 1),
        "&&" | "and" => (BinaryOp::And, 2),
        "==" => (BinaryOp::Eq, 3),
        "!=" => (BinaryOp::Ne, 3),
        "<" => (BinaryOp::Lt, 4),
        "<=" => (BinaryOp::Le, 4),
        ">" => (BinaryOp::Gt, 4),
        ">=" => (BinaryOp::Ge, 4),
        "+" => (BinaryOp::Add, 5),
        "-" => (BinaryOp::Sub, 5),
        "*" => (BinaryOp::Mul, 6),
        "/" => (BinaryOp::Div, 6),
        "%" => (BinaryOp::Mod, 6),
        _ => return None,
    })
}

/// Every operator and punctuation token the M1 grammar uses.
pub(crate) fn is_supported_op(op: &str) -> bool {
    binary_op(op).is_some() || matches!(op, "!" | "not" | "(" | ")" | "," | "?" | ":")
}

/// What the parser expected when it found something else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expected {
    /// A value (literal, name, `(` or prefix operator).
    Value,
    /// The `)` closing the `(` at this index.
    Close(usize),
    /// The `:` of the `?` at this index.
    Colon(usize),
    /// `,` or `)` in the argument list opened at this index.
    ArgSeparator(usize),
    /// The end of the slot.
    End,
}

struct Parser<'t> {
    tokens: &'t [Token],
    pos: usize,
}

impl<'t> Parser<'t> {
    fn peek(&self) -> Option<&'t Token> {
        self.tokens.get(self.pos)
    }

    fn peek_op(&self) -> Option<&'t str> {
        match self.peek() {
            Some(Token::Op(op)) => Some(op.as_str()),
            _ => None,
        }
    }

    fn error(kind: ParseErrorKind, at: usize) -> ParseError {
        ParseError {
            kind,
            start: at,
            end: at + 1,
        }
    }

    /// `expr = logical_or [ "?" expr ":" expr ]`.
    fn expr(&mut self, depth: usize) -> Result<Syntax<'t>, ParseError> {
        let cond = self.binary(1, depth)?;
        if self.peek_op() != Some("?") {
            return Ok(cond);
        }
        let question = self.pos;
        self.pos += 1;
        let then_value = self.nested_expr(depth)?;
        if self.peek_op() != Some(":") {
            return Err(self.unexpected(Expected::Colon(question)));
        }
        self.pos += 1;
        let else_value = self.nested_expr(depth)?;
        let span = (cond.start, else_value.end);
        let levels = cond.levels.max(then_value.levels).max(else_value.levels);
        let kind = SyntaxKind::Conditional {
            cond: Box::new(cond),
            then_value: Box::new(then_value),
            else_value: Box::new(else_value),
        };
        Syntax::node(kind, span, levels, question)
    }

    /// An expression one nesting level deeper.
    fn nested_expr(&mut self, depth: usize) -> Result<Syntax<'t>, ParseError> {
        if depth >= MAX_EXPR_DEPTH {
            return Err(Self::error(
                ParseErrorKind::TooDeep,
                self.pos.min(self.tokens.len().saturating_sub(1)),
            ));
        }
        self.expr(depth + 1)
    }

    /// Precedence climbing over the binary operators with precedence `min` or more.
    fn binary(&mut self, min: u8, depth: usize) -> Result<Syntax<'t>, ParseError> {
        let mut lhs = self.unary(depth)?;
        while let Some((op, prec)) = self.peek_op().and_then(binary_op) {
            if prec < min {
                break;
            }
            let at = self.pos;
            self.pos += 1;
            let rhs = self.binary(prec + 1, depth)?;
            let span = (lhs.start, rhs.end);
            let levels = lhs.levels.max(rhs.levels);
            let kind = SyntaxKind::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
            lhs = Syntax::node(kind, span, levels, at)?;
        }
        Ok(lhs)
    }

    /// `unary = ( "!" | "not" | "-" | "+" ) unary | primary`.
    fn unary(&mut self, depth: usize) -> Result<Syntax<'t>, ParseError> {
        let op = match self.peek_op() {
            Some("!" | "not") => UnaryOp::Not,
            Some("-") => UnaryOp::Neg,
            Some("+") => UnaryOp::Plus,
            _ => return self.primary(depth),
        };
        let start = self.pos;
        if depth >= MAX_EXPR_DEPTH {
            return Err(Self::error(ParseErrorKind::TooDeep, start));
        }
        self.pos += 1;
        let operand = self.unary(depth + 1)?;
        let span = (start, operand.end);
        let levels = operand.levels;
        let kind = SyntaxKind::Unary {
            op,
            operand: Box::new(operand),
        };
        Syntax::node(kind, span, levels, start)
    }

    fn primary(&mut self, depth: usize) -> Result<Syntax<'t>, ParseError> {
        let at = self.pos;
        let Some(token) = self.peek() else {
            return Err(self.unexpected(Expected::Value));
        };
        let kind = match token {
            Token::Num(text) => SyntaxKind::Number(text),
            Token::Str(text) => SyntaxKind::Str(text),
            Token::Chr(text) => SyntaxKind::Char(text),
            Token::Kw(kw) if kw == "true" => SyntaxKind::Bool(true),
            Token::Kw(kw) if kw == "false" => SyntaxKind::Bool(false),
            Token::Kw(_) => return Err(Self::error(ParseErrorKind::UnsupportedKeyword, at)),
            Token::Ref(sym) => {
                if self.tokens.get(at + 1) == Some(&Token::Op(String::from("("))) {
                    return self.call(sym, depth);
                }
                SyntaxKind::Ref(sym)
            }
            Token::Op(op) if op == "(" => return self.parenthesized(depth),
            Token::Op(_) | Token::Text(_) => return Err(self.unexpected(Expected::Value)),
        };
        self.pos += 1;
        Ok(Syntax::leaf(kind, at))
    }

    fn parenthesized(&mut self, depth: usize) -> Result<Syntax<'t>, ParseError> {
        let open = self.pos;
        self.pos += 1;
        let mut inner = self.nested_expr(depth)?;
        if self.peek_op() != Some(")") {
            return Err(self.unexpected(Expected::Close(open)));
        }
        self.pos += 1;
        inner.start = open;
        inner.end = self.pos;
        Ok(inner)
    }

    fn call(&mut self, callee: &'t SymbolId, depth: usize) -> Result<Syntax<'t>, ParseError> {
        let start = self.pos;
        let open = start + 1;
        self.pos = open + 1;
        let mut args = Vec::new();
        if self.peek_op() == Some(")") {
            self.pos += 1;
        } else {
            loop {
                args.push(self.nested_expr(depth)?);
                match self.peek_op() {
                    Some(",") => self.pos += 1,
                    Some(")") => {
                        self.pos += 1;
                        break;
                    }
                    _ => return Err(self.unexpected(Expected::ArgSeparator(open))),
                }
            }
        }
        let levels = args.iter().map(|arg| arg.levels).max().unwrap_or(0);
        Syntax::node(
            SyntaxKind::Call { callee, args },
            (start, self.pos),
            levels,
            start,
        )
    }

    /// Explains why the token at the current position does not fit.
    fn unexpected(&self, expected: Expected) -> ParseError {
        use ParseErrorKind as K;
        let at = self.pos;
        let Some(token) = self.peek() else {
            // Ran out of tokens.
            return match expected {
                Expected::Close(open) | Expected::ArgSeparator(open) => Self::error(K::UnclosedParen, open),
                Expected::Colon(question) => Self::error(K::MissingColon, question),
                Expected::Value | Expected::End => {
                    Self::error(K::Incomplete, self.tokens.len().saturating_sub(1))
                }
            };
        };
        let kind = match token {
            Token::Op(op) if !is_supported_op(op) => K::UnsupportedOp,
            Token::Op(op) => match (op.as_str(), expected) {
                (_, Expected::Value) => K::ExpectedValue,
                ("(", _) => K::NotCallable,
                (")", Expected::End) => K::UnmatchedClose,
                (")" | ",", Expected::Colon(question)) => return Self::error(K::MissingColon, question),
                (":", _) => K::StrayColon,
                (",", _) => K::StrayComma,
                _ => K::MissingOperator,
            },
            Token::Kw(kw) if kw != "true" && kw != "false" => K::UnsupportedKeyword,
            Token::Text(_) => K::TextToken,
            _ if expected == Expected::Value => K::ExpectedValue,
            _ => K::MissingOperator,
        };
        Self::error(kind, at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Tokenises a compact test notation: identifiers are refs (`x` → `sym_x`),
    /// `$word` is a text token, `true`/`false`/`nullptr` are keywords, `and`,
    /// `or`, `not` are operators.
    fn toks(src: &str) -> Vec<Token> {
        let mut out = Vec::new();
        let chars: Vec<char> = src.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c.is_whitespace() {
                i += 1;
            } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.') {
                    i += 1;
                }
                out.push(Token::Num(chars[start..i].iter().collect()));
            } else if c.is_ascii_alphabetic() || c == '$' {
                let start = i;
                i += 1;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                out.push(match word.as_str() {
                    "true" | "false" | "nullptr" => Token::Kw(word),
                    "and" | "or" | "not" => Token::Op(word),
                    w if w.starts_with('$') => Token::Text(w[1..].to_owned()),
                    w => Token::Ref(SymbolId::new(&format!("sym_{w}")).expect("id")),
                });
            } else if c == '"' {
                let start = i + 1;
                i = start;
                while chars[i] != '"' {
                    i += 1;
                }
                out.push(Token::Str(chars[start..i].iter().collect()));
                i += 1;
            } else if c == '\'' {
                out.push(Token::Chr(chars[i + 1].to_string()));
                i += 3;
            } else {
                let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
                if [
                    "==", "!=", "<=", ">=", "&&", "||", "<<", ">>", "++", "+=", "->", "::",
                ]
                .contains(&two.as_str())
                {
                    out.push(Token::Op(two));
                    i += 2;
                } else {
                    out.push(Token::Op(c.to_string()));
                    i += 1;
                }
            }
        }
        out
    }

    /// Renders a tree with full parentheses, for checking structure.
    fn show(s: &Syntax<'_>) -> String {
        match &s.kind {
            SyntaxKind::Number(t) => (*t).to_owned(),
            SyntaxKind::Str(t) => format!("{t:?}"),
            SyntaxKind::Char(t) => format!("'{t}'"),
            SyntaxKind::Bool(b) => b.to_string(),
            SyntaxKind::Ref(sym) => sym.as_str().trim_start_matches("sym_").to_owned(),
            SyntaxKind::Call { callee, args } => format!(
                "{}({})",
                callee.as_str().trim_start_matches("sym_"),
                args.iter().map(show).collect::<Vec<_>>().join(", ")
            ),
            SyntaxKind::Unary { op, operand } => format!("({op:?} {})", show(operand)),
            SyntaxKind::Binary { op, lhs, rhs } => format!("({} {op:?} {})", show(lhs), show(rhs)),
            SyntaxKind::Conditional {
                cond,
                then_value,
                else_value,
            } => {
                format!("({} ? {} : {})", show(cond), show(then_value), show(else_value))
            }
        }
    }

    fn ok(src: &str) -> String {
        let tokens = toks(src);
        let tree = parse(&tokens, false).unwrap_or_else(|e| panic!("{src}: {e:?}"));
        assert_eq!((tree.start, tree.end), (0, tokens.len()), "{src}: span");
        show(&tree)
    }

    fn err(src: &str) -> (ParseErrorKind, usize, usize) {
        let tokens = toks(src);
        let e = parse(&tokens, false).expect_err(src);
        (e.kind, e.start, e.end)
    }

    #[test]
    fn precedence() {
        assert_eq!(ok("a + b * c"), "(a Add (b Mul c))");
        assert_eq!(ok("a * b + c"), "((a Mul b) Add c)");
        assert_eq!(ok("a - b - c"), "((a Sub b) Sub c)");
        assert_eq!(ok("a / b % c"), "((a Div b) Mod c)");
        assert_eq!(ok("a + b < c * d"), "((a Add b) Lt (c Mul d))");
        assert_eq!(ok("a < b == c > d"), "((a Lt b) Eq (c Gt d))");
        assert_eq!(ok("a == b != c"), "((a Eq b) Ne c)");
        assert_eq!(ok("a || b && c"), "(a Or (b And c))");
        assert_eq!(ok("a && b || c && d"), "((a And b) Or (c And d))");
        assert_eq!(ok("a or b and not c"), "(a Or (b And (Not c)))");
        assert_eq!(ok("a <= b && c >= d"), "((a Le b) And (c Ge d))");
    }

    #[test]
    fn unary_and_parentheses() {
        assert_eq!(ok("-a * b"), "((Neg a) Mul b)");
        assert_eq!(ok("- - a"), "(Neg (Neg a))");
        assert_eq!(ok("+a"), "(Plus a)");
        assert_eq!(ok("!a == b"), "((Not a) Eq b)");
        assert_eq!(ok("(a + b) * c"), "((a Add b) Mul c)");
        assert_eq!(ok("a - (b - c)"), "(a Sub (b Sub c))");
        assert_eq!(ok("((1))"), "1");
    }

    #[test]
    fn conditional_is_right_associative() {
        assert_eq!(ok("a ? b : c"), "(a ? b : c)");
        assert_eq!(ok("a ? b : c ? d : e"), "(a ? b : (c ? d : e))");
        assert_eq!(ok("a ? b ? c : d : e"), "(a ? (b ? c : d) : e)");
        assert_eq!(ok("a || b ? c + 1 : d"), "((a Or b) ? (c Add 1) : d)");
    }

    #[test]
    fn literals_and_calls() {
        assert_eq!(ok("1.5 + 2"), "(1.5 Add 2)");
        assert_eq!(ok("\"hi\" == s"), "(\"hi\" Eq s)");
        assert_eq!(ok("'x'"), "'x'");
        assert_eq!(ok("true && false"), "(true And false)");
        assert_eq!(ok("f()"), "f()");
        assert_eq!(ok("f(a, b + 1, g(c))"), "f(a, (b Add 1), g(c))");
        assert_eq!(ok("f(a) * 2"), "(f(a) Mul 2)");
    }

    #[test]
    fn spans_cover_tokens() {
        let tokens = toks("x + (y * 2)");
        let tree = parse(&tokens, false).expect("parses");
        let SyntaxKind::Binary { lhs, rhs, .. } = &tree.kind else {
            panic!("binary")
        };
        assert_eq!((lhs.start, lhs.end), (0, 1));
        assert_eq!((rhs.start, rhs.end), (2, 7));
        let tokens = toks("f(a, b)");
        let tree = parse(&tokens, false).expect("parses");
        let SyntaxKind::Call { args, .. } = &tree.kind else {
            panic!("call")
        };
        assert_eq!((args[1].start, args[1].end), (4, 5));
        assert_eq!((tree.start, tree.end), (0, 6));
    }

    #[test]
    fn errors() {
        use ParseErrorKind as K;
        assert_eq!(err("a +"), (K::Incomplete, 1, 2));
        assert_eq!(err("a b"), (K::MissingOperator, 1, 2));
        assert_eq!(err("a 1"), (K::MissingOperator, 1, 2));
        assert_eq!(err("(a + b"), (K::UnclosedParen, 0, 1));
        assert_eq!(err("(a b)"), (K::MissingOperator, 2, 3));
        assert_eq!(err("a + b)"), (K::UnmatchedClose, 3, 4));
        assert_eq!(err("a ? b"), (K::MissingColon, 1, 2));
        assert_eq!(err("a ? b , c"), (K::MissingColon, 1, 2));
        assert_eq!(err("(a ? b)"), (K::MissingColon, 2, 3));
        assert_eq!(err("(a !)"), (K::MissingOperator, 2, 3));
        assert_eq!(err("a ! b"), (K::MissingOperator, 1, 2));
        assert_eq!(err("a : b"), (K::StrayColon, 1, 2));
        assert_eq!(err("a , b"), (K::StrayComma, 1, 2));
        assert_eq!(err("f(a b)"), (K::MissingOperator, 3, 4));
        assert_eq!(err("f(a ; b)"), (K::UnsupportedOp, 3, 4));
        assert_eq!(err("f(a :"), (K::StrayColon, 3, 4));
        assert_eq!(err("f(a"), (K::UnclosedParen, 1, 2));
        assert_eq!(err("f("), (K::Incomplete, 1, 2));
        assert_eq!(err("f(,)"), (K::ExpectedValue, 2, 3));
        assert_eq!(err("5(3)"), (K::NotCallable, 1, 2));
        assert_eq!(err("(a)(b)"), (K::NotCallable, 3, 4));
        assert_eq!(err("* a"), (K::ExpectedValue, 0, 1));
        assert_eq!(err(")"), (K::ExpectedValue, 0, 1));
        assert_eq!(err("a << b"), (K::UnsupportedOp, 1, 2));
        assert_eq!(err("a = b"), (K::UnsupportedOp, 1, 2));
        assert_eq!(err("a++"), (K::UnsupportedOp, 1, 2));
        assert_eq!(err("~a"), (K::UnsupportedOp, 0, 1));
        assert_eq!(err("nullptr"), (K::UnsupportedKeyword, 0, 1));
        assert_eq!(err("a == nullptr"), (K::UnsupportedKeyword, 2, 3));
        assert_eq!(err("a nullptr"), (K::UnsupportedKeyword, 1, 2));
        assert_eq!(err("a ? b : "), (K::Incomplete, 3, 4));
        assert_eq!(err("a ? (b : c)"), (K::StrayColon, 4, 5));
        assert_eq!(err("a ? b ? c : d"), (K::MissingColon, 1, 2));
    }

    #[test]
    fn text_draft_and_limits() {
        let tokens = toks("a + $foo");
        assert_eq!(
            parse(&tokens, false).map_err(|e| (e.kind, e.start, e.end)),
            Err((ParseErrorKind::TextToken, 2, 3))
        );
        let tokens = toks("a +");
        assert_eq!(
            parse(&tokens, true).map_err(|e| (e.kind, e.start, e.end)),
            Err((ParseErrorKind::Draft, 0, 2))
        );
        let tokens = toks("a");
        assert!(parse(&tokens, true).is_err());

        let many = vec![Token::Num(String::from("1")); MAX_EXPR_TOKENS + 1];
        assert_eq!(
            parse(&many, false).map_err(|e| e.kind),
            Err(ParseErrorKind::TooManyTokens)
        );

        let deep_ok = format!("{}1{}", "(".repeat(MAX_EXPR_DEPTH), ")".repeat(MAX_EXPR_DEPTH));
        assert_eq!(ok(&deep_ok), "1");
        let too_deep = format!(
            "{}1{}",
            "(".repeat(MAX_EXPR_DEPTH + 1),
            ")".repeat(MAX_EXPR_DEPTH + 1)
        );
        assert_eq!(
            parse(&toks(&too_deep), false).map_err(|e| e.kind),
            Err(ParseErrorKind::TooDeep)
        );
        let negs = format!("{}1", "-".repeat(MAX_EXPR_DEPTH + 1));
        assert_eq!(
            parse(&toks(&negs), false).map_err(|e| e.kind),
            Err(ParseErrorKind::TooDeep)
        );
        let negs = format!("{}1", "- ".repeat(MAX_EXPR_DEPTH));
        assert!(parse(&toks(&negs), false).is_ok());

        // A flat chain is parsed without recursion, but each operator is a
        // level of the tree: 64 operators are fine, 65 are not.
        let chain = vec!["1"; MAX_EXPR_DEPTH + 1].join(" + ");
        assert_eq!(parse(&toks(&chain), false).map(|t| t.levels), Ok(MAX_EXPR_DEPTH));
        let chain = vec!["1"; MAX_EXPR_DEPTH + 2].join(" + ");
        let tokens = toks(&chain);
        let error = parse(&tokens, false).expect_err("65 operators");
        assert_eq!(error.kind, ParseErrorKind::TooDeep);
        assert_eq!(
            (error.start, error.end),
            (2 * MAX_EXPR_DEPTH + 1, 2 * MAX_EXPR_DEPTH + 2)
        );
        // The same holds for calls and conditionals.
        let calls = format!("{}1{}", "f(".repeat(MAX_EXPR_DEPTH), ")".repeat(MAX_EXPR_DEPTH));
        assert_eq!(parse(&toks(&calls), false).map(|t| t.levels), Ok(MAX_EXPR_DEPTH));
        let mixed = format!(
            "{}1{}",
            "f(1 + ".repeat(MAX_EXPR_DEPTH / 2),
            ")".repeat(MAX_EXPR_DEPTH / 2)
        );
        assert_eq!(parse(&toks(&mixed), false).map(|t| t.levels), Ok(MAX_EXPR_DEPTH));
        let mixed = format!(
            "{}1 * 2{}",
            "f(1 + ".repeat(MAX_EXPR_DEPTH / 2),
            ")".repeat(MAX_EXPR_DEPTH / 2)
        );
        assert_eq!(
            parse(&toks(&mixed), false).map_err(|e| e.kind),
            Err(ParseErrorKind::TooDeep)
        );
        let ternaries = vec!["a ? 1 :"; MAX_EXPR_DEPTH + 1].join(" ") + " 2";
        assert_eq!(
            parse(&toks(&ternaries), false).map_err(|e| e.kind),
            Err(ParseErrorKind::TooDeep)
        );
        let ternaries = vec!["a ? 1 :"; MAX_EXPR_DEPTH].join(" ") + " 2";
        assert_eq!(
            parse(&toks(&ternaries), false).map(|t| t.levels),
            Ok(MAX_EXPR_DEPTH)
        );
    }

    fn any_token() -> impl Strategy<Value = Token> {
        const OPS: &[&str] = &[
            "+", "-", "*", "/", "%", "==", "!=", "<", "<=", ">", ">=", "&&", "||", "!", "and", "or", "not",
            "(", ")", ",", "?", ":", "<<", "=", "[", ".", ";",
        ];
        prop_oneof![
            4 => prop::sample::select(OPS).prop_map(|o| Token::Op(o.to_owned())),
            2 => "[0-9.ex]{1,4}".prop_map(Token::Num),
            1 => ".{0,3}".prop_map(Token::Str),
            1 => ".{0,2}".prop_map(Token::Chr),
            2 => "[a-c]".prop_map(|s| Token::Ref(SymbolId::new(&s).expect("id"))),
            1 => prop::sample::select(&["true", "false", "this"][..]).prop_map(|k| Token::Kw(k.to_owned())),
            1 => "[a-z]{0,3}".prop_map(Token::Text),
        ]
    }

    proptest! {
        #[test]
        fn never_panics_and_spans_are_in_range(tokens in prop::collection::vec(any_token(), 1..40), draft: bool) {
            match parse(&tokens, draft) {
                Ok(tree) => {
                    prop_assert!(!draft);
                    prop_assert_eq!((tree.start, tree.end), (0, tokens.len()));
                }
                Err(e) => {
                    prop_assert!(e.start < e.end && e.end <= tokens.len(), "{:?}", e);
                }
            }
        }

        #[test]
        fn deep_nesting_is_rejected_not_overflowed(depth in 0usize..400, open in "[(!-]") {
            let mut tokens: Vec<Token> = (0..depth).map(|_| Token::Op(open.clone())).collect();
            tokens.push(Token::Num(String::from("1")));
            if open == "(" {
                tokens.extend((0..depth).map(|_| Token::Op(String::from(")"))));
            }
            let result = parse(&tokens, false);
            if depth <= MAX_EXPR_DEPTH && tokens.len() <= MAX_EXPR_TOKENS {
                prop_assert!(result.is_ok());
            } else {
                prop_assert!(result.is_err());
            }
        }
    }
}
