//! Lowering expressions: reporter blocks, expression slots, operators and
//! calls. Blocks and slots produce the same nodes through the same typing
//! functions, so later stages cannot tell them apart (spec §6.4).

use b2c_ir::diag::{Location, Part};
use b2c_ir::ids::{BlockId, SymbolId};
use b2c_ir::sast::{BinaryOp, Expr, ExprKind, Origin, PassMode, UnaryOp};
use b2c_ir::text::{CharLit, NumError, NumLit, NumType, StrLit};
use b2c_ir::types::Type;
use b2c_model::{Block, ExprInput, Input, Token};

use super::convert::Subject;
use super::{FuncSig, Lowerer, ParamSig, field};
use crate::access;
use crate::codes;
use crate::messages::{a_type, quoted, shown};
use crate::parser::{self, ParseError, ParseErrorKind, Syntax, SyntaxKind};
use crate::typing::{OpClass, arithmetic_result, conditional_result, op_class, op_symbol, op_verb};

/// A placeholder for an expression that has an error (already reported).
pub(super) fn error_expr(origin: Origin) -> Expr {
    Expr {
        kind: ExprKind::Int(NumLit::int(0)),
        ty: Type::Error,
        origin,
    }
}

/// `-2147483648`, the smallest `int`, when `magnitude` is the literal
/// `2147483648` (in any integer notation). C++ has no negative literals, and
/// `2147483648` alone does not fit in an `int`, so the value is written
/// `-2147483647 - 1`, as `INT_MIN` is.
fn int_min(magnitude: &str, origin: &Origin) -> Option<Expr> {
    let value = NumLit::parse(magnitude, NumType::LongLong).ok()?;
    if value.as_str() != "2147483648" {
        return None;
    }
    let literal = |value: i32| Expr {
        kind: ExprKind::Int(NumLit::int(value)),
        ty: Type::Int,
        origin: origin.clone(),
    };
    let negated = Expr {
        kind: ExprKind::Unary {
            op: UnaryOp::Neg,
            operand: Box::new(literal(i32::MAX)),
        },
        ty: Type::Int,
        origin: origin.clone(),
    };
    Some(Expr {
        kind: ExprKind::Binary {
            op: BinaryOp::Sub,
            lhs: Box::new(negated),
            rhs: Box::new(literal(1)),
        },
        ty: Type::Int,
        origin: origin.clone(),
    })
}

/// The value of an input, before deciding whether it was required.
enum InputValue {
    Value(Expr),
    Absent,
    DisabledBlock,
}

/// A friendly name for a value input, for messages.
fn input_label(name: &str) -> String {
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit());
    let index: usize = name
        .get(base.len()..)
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(0);
    match base {
        "VALUE" => String::from("a value"),
        "COND" if index > 0 => format!("the condition of 'else if' number {index}"),
        "COND" => String::from("a condition"),
        "A" => String::from("its first value"),
        "B" => String::from("its second value"),
        "BY" => String::from("the amount to change by"),
        "TIMES" => String::from("the number of times"),
        "FROM" => String::from("the start of the loop"),
        "TO" => String::from("the end of the loop"),
        "STEP" => String::from("the step of the loop"),
        "LOW" => String::from("the lowest number"),
        "HIGH" => String::from("the highest number"),
        "THEN" => String::from("the value to use when the condition is true"),
        "ELSE" => String::from("the value to use when the condition is false"),
        "ITEM" => format!("item {}", index + 1),
        "ARG" => format!("input {}", index + 1),
        "CODE" => String::from("an exit code"),
        "PROMPT" => String::from("a question"),
        _ => format!("a value for {}", quoted(name)),
    }
}

/// Where a slot is, for origins and messages.
struct Slot<'s> {
    block: &'s BlockId,
    input: &'s str,
    tokens: &'s [Token],
}

impl Lowerer<'_> {
    // --- Inputs ----------------------------------------------------------------

    fn input_value(&mut self, block: &Block, name: &str) -> InputValue {
        match access::input(block, name) {
            None => InputValue::Absent,
            Some(Input::Block(nested)) if nested.block.disabled => InputValue::DisabledBlock,
            Some(Input::Block(nested)) => InputValue::Value(self.value_block(&nested.block)),
            Some(Input::Expr(slot)) if slot.expr.is_empty() => InputValue::Absent,
            Some(Input::Expr(slot)) => InputValue::Value(self.slot(&block.id, name, slot)),
        }
    }

    /// The value of a required input; reports and returns a placeholder when
    /// it is missing.
    pub(super) fn required_input(&mut self, block: &Block, name: &str) -> Expr {
        let part = Part::Input {
            name: name.to_owned(),
        };
        let message = match self.input_value(block, name) {
            InputValue::Value(value) => return value,
            InputValue::Absent => format!("This block needs {}.", input_label(name)),
            InputValue::DisabledBlock => {
                format!(
                    "The block in {} is disabled, but a value is needed there.",
                    input_label(name)
                )
            }
        };
        let location = self.loc(&block.id, part.clone());
        self.incomplete(location, message);
        error_expr(self.origin(&block.id, part))
    }

    /// The value of an optional input (`None` when absent or disabled).
    pub(super) fn optional_input(&mut self, block: &Block, name: &str) -> Option<Expr> {
        match self.input_value(block, name) {
            InputValue::Value(value) => Some(value),
            InputValue::Absent | InputValue::DisabledBlock => None,
        }
    }

    // --- Reporter blocks -----------------------------------------------------

    /// Lowers a reporter block. Unknown types and statement blocks in a value
    /// input give a placeholder: the catalog check reports them.
    pub(super) fn value_block(&mut self, block: &Block) -> Expr {
        let origin = self.origin(&block.id, Part::Whole);
        if !self.enter(block) {
            return error_expr(origin);
        }
        let expr = match block.block_type.as_str() {
            "var.get" => self.var_get(block, origin),
            "math.number" => self.number_block(block, origin),
            "math.arithmetic" | "math.compare" => self.operator_block(block, origin),
            "math.random_int" => self.random_int(block, origin),
            "math.convert" => self.convert(block, origin),
            "logic.boolean" => {
                let value = self.choice(block, "VALUE", true, &[("true", true), ("false", false)]);
                Expr {
                    kind: ExprKind::Bool(value),
                    ty: Type::Bool,
                    origin,
                }
            }
            "logic.operation" => self.logic_chain(block, origin),
            "logic.not" => {
                let operand = self.required_input(block, "A");
                self.unary(UnaryOp::Not, operand, origin)
            }
            "logic.ternary" => {
                let cond = self.required_input(block, "COND");
                let then_value = self.required_input(block, "THEN");
                let else_value = self.required_input(block, "ELSE");
                self.conditional(cond, then_value, else_value, origin)
            }
            "text.literal" => self.text_block(block, origin, false),
            "text.char" => self.text_block(block, origin, true),
            "text.join" => {
                let count = access::extra_count(block, "itemCount", 2, 2);
                let items = (0..count)
                    .map(|i| self.required_input(block, &format!("ITEM{i}")))
                    .collect();
                Expr {
                    kind: ExprKind::Join(items),
                    ty: Type::String,
                    origin,
                }
            }
            "func.call" => self
                .call_block(block, false)
                .unwrap_or_else(|| error_expr(origin)),
            _ => error_expr(origin),
        };
        self.leave();
        expr
    }

    fn var_get(&mut self, block: &Block, origin: Origin) -> Expr {
        let location = self.loc(&block.id, field("VAR"));
        if let Ok(sym) = access::ref_field(block, "VAR") {
            self.variable(sym, origin, &location)
        } else {
            self.incomplete(location, "Choose a variable in this block.");
            error_expr(origin)
        }
    }

    /// A reference to a variable as a value.
    fn variable(&mut self, sym: &SymbolId, origin: Origin, location: &Location) -> Expr {
        match self.read_variable(sym, location) {
            Some(ty) => Expr {
                kind: ExprKind::Var(sym.clone()),
                ty,
                origin,
            },
            None => error_expr(origin),
        }
    }

    fn number_block(&mut self, block: &Block, origin: Origin) -> Expr {
        let location = self.loc(&block.id, field("VALUE"));
        let Ok(text) = access::text_field(block, "VALUE", Some("0")) else {
            self.incomplete(location, "The number in this block is damaged. Type it again.");
            return error_expr(origin);
        };
        let text = text.trim();
        let (negative, digits) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text.strip_prefix('+').unwrap_or(text)),
        };
        if negative && let Some(min) = int_min(digits, &origin) {
            return min;
        }
        let literal = self.number_literal(digits, origin.clone(), location);
        if negative && literal.ty != Type::Error {
            let ty = literal.ty.clone();
            Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Neg,
                    operand: Box::new(literal),
                },
                ty,
                origin,
            }
        } else {
            literal
        }
    }

    /// A numeric literal: `int` unless it has a decimal point or exponent.
    fn number_literal(&mut self, text: &str, origin: Origin, location: Location) -> Expr {
        let hex = text.starts_with("0x") || text.starts_with("0X");
        let is_double = !hex && text.contains(['.', 'e', 'E']);
        let num_type = if is_double { NumType::Double } else { NumType::Int };
        match NumLit::parse(text, num_type) {
            Ok(literal) if is_double => Expr {
                kind: ExprKind::Float(literal),
                ty: Type::Double,
                origin,
            },
            Ok(literal) => Expr {
                kind: ExprKind::Int(literal),
                ty: Type::Int,
                origin,
            },
            Err(NumError::Syntax(_)) => {
                let message = if text.is_empty() {
                    String::from("A number is needed here, such as 42 or 3.14.")
                } else {
                    format!(
                        "{} is not a number. Write whole numbers like 42 and decimal numbers like 3.14.",
                        quoted(text)
                    )
                };
                self.diags.error(codes::BAD_NUMBER, location, message);
                error_expr(origin)
            }
            Err(NumError::OutOfRange { .. }) => {
                let message = if is_double {
                    format!("{} is too large for a decimal number.", quoted(text))
                } else if text.chars().all(|c| c.is_ascii_digit()) {
                    format!(
                        "{} is too large for a whole number (the largest is 2147483647). Write it as a decimal \
                         number, such as {}.0, if you need it.",
                        quoted(text),
                        shown(text)
                    )
                } else {
                    format!(
                        "{} is too large for a whole number (the largest is 2147483647).",
                        quoted(text)
                    )
                };
                self.diags.error(codes::LITERAL_OVERFLOW, location, message);
                error_expr(origin)
            }
        }
    }

    fn text_block(&mut self, block: &Block, origin: Origin, is_char: bool) -> Expr {
        let location = self.loc(&block.id, field("VALUE"));
        let default = if is_char { "a" } else { "" };
        let Ok(text) = access::text_field(block, "VALUE", Some(default)) else {
            self.incomplete(location, "The text in this block is damaged. Type it again.");
            return error_expr(origin);
        };
        if is_char {
            self.char_literal(text, origin, location)
        } else {
            self.str_literal(text, origin, location)
        }
    }

    fn str_literal(&mut self, text: &str, origin: Origin, location: Location) -> Expr {
        match StrLit::new(text) {
            Ok(literal) => Expr {
                kind: ExprKind::Str(literal),
                ty: Type::String,
                origin,
            },
            Err(error) => {
                self.diags.error(
                    codes::BAD_LITERAL,
                    location,
                    format!("This text can't be used: {error}."),
                );
                error_expr(origin)
            }
        }
    }

    fn char_literal(&mut self, text: &str, origin: Origin, location: Location) -> Expr {
        match CharLit::new(text) {
            Ok(literal) => Expr {
                kind: ExprKind::Char(literal),
                ty: Type::Char,
                origin,
            },
            Err(error) => {
                self.diags.error(
                    codes::BAD_LITERAL,
                    location,
                    format!("This character can't be used: {error}."),
                );
                error_expr(origin)
            }
        }
    }

    /// `math.arithmetic` and `math.compare`.
    fn operator_block(&mut self, block: &Block, origin: Origin) -> Expr {
        let op = if block.block_type == "math.arithmetic" {
            let options = [
                ("add", BinaryOp::Add),
                ("sub", BinaryOp::Sub),
                ("mul", BinaryOp::Mul),
                ("div", BinaryOp::Div),
                ("mod", BinaryOp::Mod),
            ];
            self.choice(block, "OP", BinaryOp::Add, &options)
        } else {
            let options = [
                ("lt", BinaryOp::Lt),
                ("le", BinaryOp::Le),
                ("gt", BinaryOp::Gt),
                ("ge", BinaryOp::Ge),
                ("eq", BinaryOp::Eq),
                ("ne", BinaryOp::Ne),
            ];
            self.choice(block, "OP", BinaryOp::Lt, &options)
        };
        let lhs = self.required_input(block, "A");
        let rhs = self.required_input(block, "B");
        self.binary(op, lhs, rhs, origin)
    }

    fn random_int(&mut self, block: &Block, origin: Origin) -> Expr {
        let low = self.required_input(block, "LOW");
        let low = self.integer(low, &Subject::RandomLow);
        let high = self.required_input(block, "HIGH");
        let high = self.integer(high, &Subject::RandomHigh);
        Expr {
            kind: ExprKind::RandomInt {
                low: Box::new(low),
                high: Box::new(high),
            },
            ty: Type::Int,
            origin,
        }
    }

    fn convert(&mut self, block: &Block, origin: Origin) -> Expr {
        let to = self.choice(
            block,
            "TO",
            Type::Int,
            &[("int", Type::Int), ("double", Type::Double)],
        );
        let value = self.required_input(block, "VALUE");
        if value.ty == Type::String {
            let message = format!(
                "Only numbers can be converted to {}, but this is text.",
                a_type(&to)
            );
            self.diags
                .error(codes::CONVERSION, value.origin.location(), message);
        } else if value.ty == Type::Bool {
            let message = format!(
                "This converts a true/false value to {}: true becomes 1 and false becomes 0. Is that what you \
                 meant?",
                a_type(&to)
            );
            self.diags
                .warning(codes::BOOL_NUMBER, value.origin.location(), message);
        } else if to == Type::Double {
            self.warn_integer_division(&value);
        }
        Expr {
            kind: ExprKind::Convert {
                to: to.clone(),
                value: Box::new(value),
            },
            ty: to,
            origin,
        }
    }

    /// `logic.operation`: a left-associative chain `a && b && c`.
    fn logic_chain(&mut self, block: &Block, origin: Origin) -> Expr {
        let op = self.choice(
            block,
            "OP",
            BinaryOp::And,
            &[("and", BinaryOp::And), ("or", BinaryOp::Or)],
        );
        let count = access::extra_count(block, "itemCount", 2, 2);
        let mut chain: Option<Expr> = None;
        for i in 0..count {
            let item = self.required_input(block, &format!("ITEM{i}"));
            chain = Some(match chain {
                None => item,
                Some(lhs) => self.binary(op, lhs, item, origin.clone()),
            });
        }
        chain.unwrap_or_else(|| error_expr(origin))
    }

    // --- Calls ---------------------------------------------------------------

    /// `func.call` / `func.call_stmt`; `None` when the block names no function.
    pub(super) fn call_block(&mut self, block: &Block, as_statement: bool) -> Option<Expr> {
        let location = self.loc(&block.id, field("FUNC"));
        let Ok(callee) = access::ref_field(block, "FUNC") else {
            self.incomplete(location, "Choose a function in this block.");
            return None;
        };
        let sig = self.resolve_function(callee, &location);
        let count = access::extra_count(block, "argCount", 0, 0);
        let args = (0..count)
            .map(|i| self.required_input(block, &format!("ARG{i}")))
            .collect();
        let origin = self.origin(&block.id, Part::Whole);
        Some(self.finish_call(callee, sig, args, origin, as_statement))
    }

    /// Checks a call's arguments against the signature and builds the node.
    fn finish_call(
        &mut self,
        callee: &SymbolId,
        sig: Option<FuncSig>,
        args: Vec<Expr>,
        origin: Origin,
        as_statement: bool,
    ) -> Expr {
        let Some(sig) = sig else {
            return Expr {
                kind: ExprKind::Call {
                    function: callee.clone(),
                    args,
                },
                ty: Type::Error,
                origin,
            };
        };
        if args.len() != sig.params.len() {
            let expected = match sig.params.len() {
                0 => String::from("no values"),
                1 => String::from("1 value"),
                n => format!("{n} values"),
            };
            let given = match args.len() {
                1 => String::from("1 is"),
                n => format!("{n} are"),
            };
            let message = format!("{} needs {expected}, but {given} given.", quoted(&sig.name));
            self.diags
                .error(codes::ARGUMENT_COUNT, origin.location(), message);
        }
        for (arg, param) in args.iter().zip(&sig.params) {
            self.check_argument(arg, param, &sig.name);
        }
        let ty = if !as_statement && sig.ret == Type::Void {
            let message = format!(
                "{} gives back nothing, so it can't be used as a value. Use the statement version of the block \
                 to just run it.",
                quoted(&sig.name)
            );
            self.diags.error(codes::NO_VALUE, origin.location(), message);
            Type::Error
        } else {
            sig.ret.clone()
        };
        Expr {
            kind: ExprKind::Call {
                function: callee.clone(),
                args,
            },
            ty,
            origin,
        }
    }

    fn check_argument(&mut self, arg: &Expr, param: &ParamSig, function: &str) {
        if param.mode != PassMode::Editable {
            let subject = Subject::Argument {
                function: function.to_owned(),
                param: param.name.clone(),
            };
            self.check_conversion(arg, &param.ty, &subject);
            return;
        }
        if arg.ty == Type::Error || param.ty == Type::Error {
            return;
        }
        let subject = format!(
            "The {} input of {} is editable",
            quoted(&param.name),
            quoted(function)
        );
        let message = match &arg.kind {
            ExprKind::Var(sym) => {
                if self.not_changeable(sym).is_some() {
                    format!(
                        "{subject} (the function may change it), but {} can't be changed.",
                        self.name_of(sym)
                    )
                } else if arg.ty != param.ty {
                    format!(
                        "{subject}, so it needs a variable that holds {}, but {} holds {}.",
                        a_type(&param.ty),
                        self.name_of(sym),
                        a_type(&arg.ty)
                    )
                } else {
                    return;
                }
            }
            _ => format!(
                "{subject} (the function may change it), so it needs a variable, not a value or calculation."
            ),
        };
        self.diags
            .error(codes::EDITABLE_ARGUMENT, arg.origin.location(), message);
    }

    // --- Expression slots ------------------------------------------------------

    /// Parses and lowers an expression slot.
    fn slot(&mut self, block: &BlockId, input: &str, slot: &ExprInput) -> Expr {
        let ctx = Slot {
            block,
            input,
            tokens: &slot.expr,
        };
        match parser::parse(&slot.expr, slot.draft) {
            Ok(tree) => self.syntax(&tree, &ctx),
            Err(error) => {
                self.report_parse_error(&error, &ctx);
                error_expr(self.slot_origin(&ctx, 0, ctx.tokens.len()))
            }
        }
    }

    fn slot_origin(&self, ctx: &Slot<'_>, start: usize, end: usize) -> Origin {
        let to_u32 = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let part = Part::Tokens {
            input: ctx.input.to_owned(),
            start: to_u32(start),
            end: to_u32(end),
        };
        self.origin(ctx.block, part)
    }

    fn syntax(&mut self, tree: &Syntax<'_>, ctx: &Slot<'_>) -> Expr {
        let origin = self.slot_origin(ctx, tree.start, tree.end);
        match &tree.kind {
            SyntaxKind::Number(text) => {
                let location = origin.location();
                self.number_literal(text, origin, location)
            }
            SyntaxKind::Str(text) => {
                let location = origin.location();
                self.str_literal(text, origin, location)
            }
            SyntaxKind::Char(text) => {
                let location = origin.location();
                self.char_literal(text, origin, location)
            }
            SyntaxKind::Bool(value) => Expr {
                kind: ExprKind::Bool(*value),
                ty: Type::Bool,
                origin,
            },
            SyntaxKind::Ref(sym) => {
                let location = origin.location();
                self.variable(sym, origin, &location)
            }
            SyntaxKind::Call { callee, args } => {
                let location = self.slot_origin(ctx, tree.start, tree.start + 1).location();
                let sig = self.resolve_function(callee, &location);
                let args = args.iter().map(|arg| self.syntax(arg, ctx)).collect();
                self.finish_call(callee, sig, args, origin, false)
            }
            SyntaxKind::Unary { op, operand } => {
                if let (UnaryOp::Neg, SyntaxKind::Number(text)) = (op, &operand.kind)
                    && let Some(min) = int_min(text, &origin)
                {
                    return min;
                }
                let operand = self.syntax(operand, ctx);
                self.unary(*op, operand, origin)
            }
            SyntaxKind::Binary { op, lhs, rhs } => {
                let lhs = self.syntax(lhs, ctx);
                let rhs = self.syntax(rhs, ctx);
                self.binary(*op, lhs, rhs, origin)
            }
            SyntaxKind::Conditional {
                cond,
                then_value,
                else_value,
            } => {
                let cond = self.syntax(cond, ctx);
                let then_value = self.syntax(then_value, ctx);
                let else_value = self.syntax(else_value, ctx);
                self.conditional(cond, then_value, else_value, origin)
            }
        }
    }

    /// Describes a token for a message.
    fn describe(&self, token: Option<&Token>) -> String {
        match token {
            Some(Token::Num(text) | Token::Op(text) | Token::Kw(text) | Token::Text(text)) => {
                format!("`{}`", shown(text))
            }
            Some(Token::Str(text)) => format!("the text \"{}\"", shown(text)),
            Some(Token::Chr(text)) => format!("the character '{}'", shown(text)),
            Some(Token::Ref(sym)) => self.name_of(sym),
            None => String::from("the end"),
        }
    }

    fn report_parse_error(&mut self, error: &ParseError, ctx: &Slot<'_>) {
        use ParseErrorKind as K;
        let token = self.describe(ctx.tokens.get(error.start));
        let (code, message) = match error.kind {
            K::TooManyTokens => (
                codes::SLOT_TOO_LONG,
                format!(
                    "This expression is too long ({} parts; the limit is {}). Split it up using variables.",
                    ctx.tokens.len(),
                    b2c_model::limits::MAX_EXPR_TOKENS
                ),
            ),
            K::TooDeep => (
                codes::SLOT_TOO_DEEP,
                format!(
                    "This expression is too complex: it has more than {} levels of operators, brackets or \
                     calls inside each other (in a chain such as a + b + c, each operator counts as a level). \
                     Split it up using variables.",
                    b2c_model::limits::MAX_EXPR_DEPTH
                ),
            ),
            K::Draft => (
                codes::SLOT_DRAFT,
                String::from("This expression isn't finished yet. Complete it, or remove it."),
            ),
            K::TextToken => (
                codes::SLOT_DRAFT,
                format!(
                    "{token} isn't understood here. Use a variable or function that exists, put text in quotes, \
                     or finish typing the expression."
                ),
            ),
            K::Incomplete => (
                codes::SLOT_SYNTAX,
                format!("This expression is incomplete: something is missing after {token}."),
            ),
            K::ExpectedValue => (codes::SLOT_SYNTAX, format!("A value is missing before {token}.")),
            K::MissingOperator => (
                codes::SLOT_SYNTAX,
                format!("{token} can't follow the value before it. Is an operator such as + or == missing?"),
            ),
            K::UnclosedParen => (
                codes::SLOT_SYNTAX,
                String::from("This `(` is never closed. Add a matching `)`."),
            ),
            K::UnmatchedClose => (codes::SLOT_SYNTAX, String::from("This `)` has no matching `(`.")),
            K::MissingColon => (
                codes::SLOT_SYNTAX,
                String::from(
                    "This `?` needs a matching `:`, as in: condition ? value if true : value if false.",
                ),
            ),
            K::StrayColon => (codes::SLOT_SYNTAX, String::from("This `:` has no matching `?`.")),
            K::StrayComma => (
                codes::SLOT_SYNTAX,
                String::from("A `,` can only separate the values given to a function, as in f(a, b)."),
            ),
            K::NotCallable => (
                codes::SLOT_SYNTAX,
                String::from(
                    "Only functions can be called with ( ). Is an operator missing before this `(`?",
                ),
            ),
            K::UnsupportedOp => (
                codes::SLOT_SYNTAX,
                unsupported_op_message(ctx.tokens.get(error.start)),
            ),
            K::UnsupportedKeyword => (
                codes::SLOT_SYNTAX,
                format!(
                    "{token} can't be used in an expression; the only keywords allowed are `true` and `false`."
                ),
            ),
        };
        let location = self.slot_origin(ctx, error.start, error.end).location();
        self.diags.error(code, location, message);
    }

    // --- Operators -------------------------------------------------------------

    /// A unary operator with its type.
    fn unary(&mut self, op: UnaryOp, operand: Expr, origin: Origin) -> Expr {
        let location = origin.location();
        let ty = match (op, &operand.ty) {
            (_, Type::Error) => Type::Error,
            (UnaryOp::Neg | UnaryOp::Plus, Type::String) => {
                self.diags.error(
                    codes::BAD_OPERANDS,
                    location,
                    "Text can't be used with + or - signs: they only work with numbers.",
                );
                Type::Error
            }
            (UnaryOp::Neg | UnaryOp::Plus, Type::Bool) => {
                self.diags.warning(
                    codes::BOOL_NUMBER,
                    location,
                    "A true/false value is used as a number here (1 or 0).",
                );
                Type::Int
            }
            (UnaryOp::Neg | UnaryOp::Plus, Type::Double) => Type::Double,
            (UnaryOp::Neg | UnaryOp::Plus, _) => Type::Int,
            (UnaryOp::Not, Type::String) => {
                self.diags.error(
                    codes::BAD_OPERANDS,
                    location,
                    "'not' needs a true/false value, but this is text.",
                );
                Type::Error
            }
            (UnaryOp::Not, Type::Int | Type::Double | Type::Char) => {
                let message = format!(
                    "'not' needs a true/false value, but this is {}: 0 counts as false and anything else as \
                     true. Use a comparison such as 'x = 0' to make this clear.",
                    a_type(&operand.ty)
                );
                self.diags.warning(codes::BOOL_NUMBER, location, message);
                Type::Bool
            }
            (UnaryOp::Not, _) => Type::Bool,
        };
        Expr {
            kind: ExprKind::Unary {
                op,
                operand: Box::new(operand),
            },
            ty,
            origin,
        }
    }

    /// A binary operator with its type.
    pub(super) fn binary(&mut self, op: BinaryOp, lhs: Expr, rhs: Expr, origin: Origin) -> Expr {
        let ty = self.binary_type(op, &lhs, &rhs, &origin);
        Expr {
            kind: ExprKind::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            ty,
            origin,
        }
    }

    fn binary_type(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr, origin: &Origin) -> Type {
        let (l, r) = (&lhs.ty, &rhs.ty);
        if *l == Type::Error || *r == Type::Error {
            return Type::Error;
        }
        let location = origin.location();
        let text = *l == Type::String || *r == Type::String;
        match op_class(op) {
            OpClass::Arithmetic if text => {
                if op == BinaryOp::Add {
                    let message = "Text can't be added with +. Use the 'join' block to put text and other values \
                                   together.";
                    self.diags.error(codes::TEXT_PLUS, location, message);
                } else {
                    let message = format!(
                        "Can't {} text: {} only works with numbers.",
                        op_verb(op),
                        op_symbol(op)
                    );
                    self.diags.error(codes::BAD_OPERANDS, location, message);
                }
                Type::Error
            }
            OpClass::Arithmetic => {
                self.warn_bool_arithmetic(l, r, &location);
                let ty = arithmetic_result(l, r);
                if ty == Type::Double {
                    self.warn_integer_division(lhs);
                    self.warn_integer_division(rhs);
                }
                ty
            }
            OpClass::Remainder if text => {
                self.diags.error(
                    codes::BAD_OPERANDS,
                    location,
                    "'mod' only works with whole numbers, not text.",
                );
                Type::Error
            }
            OpClass::Remainder if *l == Type::Double || *r == Type::Double => {
                let message = "'mod' only works with whole numbers, but this uses a decimal number. Use the \
                               'convert' block to make it a whole number first.";
                self.diags.error(codes::MOD_DECIMAL, location, message);
                Type::Error
            }
            OpClass::Remainder => {
                self.warn_bool_arithmetic(l, r, &location);
                Type::Int
            }
            OpClass::Ordering | OpClass::Equality => self.comparison_type(op, l, r, location),
            OpClass::Logical => self.logical_type(lhs, rhs),
        }
    }

    fn warn_bool_arithmetic(&mut self, l: &Type, r: &Type, location: &Location) {
        if *l == Type::Bool || *r == Type::Bool {
            self.diags.warning(
                codes::BOOL_NUMBER,
                location.clone(),
                "A true/false value is used as a number here (true is 1, false is 0). Is that what you meant?",
            );
        }
    }

    fn comparison_type(&mut self, op: BinaryOp, l: &Type, r: &Type, location: Location) -> Type {
        match (l, r) {
            (Type::String, Type::String) => Type::Bool,
            (Type::String, other) | (other, Type::String) => {
                let message = format!("Can't compare text with {}.", a_type(other));
                self.diags.error(codes::BAD_OPERANDS, location, message);
                Type::Error
            }
            (Type::Bool, Type::Bool) if op_class(op) == OpClass::Equality => Type::Bool,
            (Type::Bool, _) | (_, Type::Bool) => {
                let message = if op_class(op) == OpClass::Equality {
                    "This compares a true/false value with a number (true is 1, false is 0). Is that what you \
                     meant?"
                } else {
                    "This puts true/false values in order with < or >, which treats them as 1 and 0. Is that what \
                     you meant?"
                };
                self.diags.warning(codes::BOOL_NUMBER, location, message);
                Type::Bool
            }
            _ => {
                if op_class(op) == OpClass::Equality && (*l == Type::Double || *r == Type::Double) {
                    self.diags.warning(
                        codes::FLOAT_EQUALITY,
                        location,
                        "Checking decimal numbers for exact equality is unreliable because of rounding (0.1 + 0.2 \
                         is not exactly 0.3). Check whether their difference is very small instead.",
                    );
                }
                Type::Bool
            }
        }
    }

    fn logical_type(&mut self, lhs: &Expr, rhs: &Expr) -> Type {
        let mut ok = true;
        for side in [lhs, rhs] {
            let location = side.origin.location();
            match side.ty {
                Type::String => {
                    let message = "'and' and 'or' need true/false values, but this is text.";
                    self.diags.error(codes::BAD_OPERANDS, location, message);
                    ok = false;
                }
                Type::Int | Type::Double | Type::Char => {
                    let message = format!(
                        "'and' and 'or' need true/false values, but this is {}: 0 counts as false and anything \
                         else as true. Use a comparison such as 'x ≠ 0' to make this clear.",
                        a_type(&side.ty)
                    );
                    self.diags.warning(codes::BOOL_NUMBER, location, message);
                }
                _ => {}
            }
        }
        if ok { Type::Bool } else { Type::Error }
    }

    /// `cond ? then_value : else_value` with its type.
    fn conditional(&mut self, cond: Expr, then_value: Expr, else_value: Expr, origin: Origin) -> Expr {
        let cond = self.condition(cond);
        let ty = if let Ok((ty, suspicious)) = conditional_result(&then_value.ty, &else_value.ty) {
            if suspicious {
                self.diags.warning(
                    codes::BOOL_NUMBER,
                    origin.location(),
                    "One choice is a true/false value and the other a number, so the true/false value \
                     becomes 1 or 0. Is that what you meant?",
                );
            }
            ty
        } else {
            let message = format!(
                "Both choices must be the same kind of value, but one is {} and the other is {}.",
                a_type(&then_value.ty),
                a_type(&else_value.ty)
            );
            self.diags.error(codes::BRANCH_TYPES, origin.location(), message);
            Type::Error
        };
        let kind = ExprKind::Conditional {
            cond: Box::new(cond),
            then_value: Box::new(then_value),
            else_value: Box::new(else_value),
        };
        Expr { kind, ty, origin }
    }
}

/// A specific message for an operator that slots do not accept.
fn unsupported_op_message(token: Option<&Token>) -> String {
    let op = match token {
        Some(Token::Op(op)) => op.as_str(),
        _ => "",
    };
    let shown_op = format!("`{}`", shown(op));
    match op {
        "=" => String::from(
            "`=` changes a variable, which an expression can't do. Use a 'set' block to change a variable, or \
             `==` to compare.",
        ),
        "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "<<=" | ">>=" | "++" | "--" => format!(
            "{shown_op} changes a variable, which an expression can't do. Use a 'change' block instead."
        ),
        "<<" | ">>" | "&" | "|" | "^" | "~" => {
            format!("The bit operator {shown_op} can't be used in expressions yet.")
        }
        ";" | "{" | "}" => {
            format!("{shown_op} can't be used in an expression: an expression is a single value.")
        }
        "[" | "]" | "." | "->" | "::" => format!("{shown_op} can't be used in expressions yet."),
        _ => format!("{shown_op} isn't an operator that can be used here."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(input_label("VALUE"), "a value");
        assert_eq!(input_label("COND0"), "a condition");
        assert_eq!(input_label("COND2"), "the condition of 'else if' number 2");
        assert_eq!(input_label("ITEM0"), "item 1");
        assert_eq!(input_label("ARG3"), "input 4");
        assert_eq!(input_label("WEIRD"), "a value for `WEIRD`");
        for name in [
            "A", "B", "BY", "TIMES", "FROM", "TO", "STEP", "LOW", "HIGH", "THEN", "ELSE", "CODE", "PROMPT",
        ] {
            assert!(!input_label(name).contains('`'), "{name}");
        }
    }

    #[test]
    fn unsupported_operators() {
        let msg = |op: &str| unsupported_op_message(Some(&Token::Op(op.to_owned())));
        assert!(msg("=").contains("'set' block"));
        assert!(msg("++").contains("'change' block"));
        assert!(msg("<<").contains("bit operator"));
        assert!(msg(";").contains("single value"));
        assert!(msg(".").contains("yet"));
        assert!(msg("@").contains("isn't an operator"));
        assert!(unsupported_op_message(None).contains("isn't an operator"));
    }
}
