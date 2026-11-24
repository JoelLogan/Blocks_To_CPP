//! Lowering the text, input and output, and math blocks of W2
//! (`docs/plans/m3-contracts/w2.md` §1.3, §1.4): package B.
//!
//! Each block's value inputs are typed by its catalog signature (ADR-0018):
//! they are lowered through [`Lowerer::typed_input`], typed by
//! [`Lowerer::type_parts`] and reported through [`Lowerer::report_fit`],
//! which names each part by its [`Subject::Phrase`] phrase. A `number`
//! output is the arithmetic type of the block's inputs, as *add* gives it:
//! `char` and `bool` become `int` (w2.md §1.4).

use b2c_ir::diag::Part;
use b2c_ir::sast::{Expr, ExprKind, MathOp, Origin, StmtKind, TextOp};
use b2c_ir::types::{LibType, Type, TypeClass};
use b2c_model::Block;

use super::Lowerer;
use super::convert::Subject;
use super::expr::error_expr;
use super::user_types::Output;
use crate::codes;
use crate::messages::quoted;
use crate::typing::arithmetic_result;

/// The functions of *%FN of %VALUE* (`math.fn`), by option.
const MATH_FUNCTIONS: [(&str, MathOp); 14] = [
    ("floor", MathOp::Floor),
    ("ceil", MathOp::Ceil),
    ("round", MathOp::Round),
    ("sqrt", MathOp::Sqrt),
    ("cbrt", MathOp::Cbrt),
    ("exp", MathOp::Exp),
    ("log", MathOp::Log),
    ("log10", MathOp::Log10),
    ("sin", MathOp::Sin),
    ("cos", MathOp::Cos),
    ("tan", MathOp::Tan),
    ("asin", MathOp::Asin),
    ("acos", MathOp::Acos),
    ("atan", MathOp::Atan),
];

/// The functions of *%FN of %A and %B* (`math.fn_two`), by option.
const MATH_FUNCTIONS2: [(&str, MathOp); 3] = [
    ("pow", MathOp::Pow),
    ("atan2", MathOp::Atan2),
    ("hypot", MathOp::Hypot),
];

/// A part of a block for its messages; its value is not stored.
fn part(phrase: &str) -> Subject {
    Subject::Phrase {
        text: phrase.to_owned(),
        stores: false,
    }
}

/// The arithmetic type of a `number` output with these operands (w2.md
/// §1.4), as the editor's `numberType` gives it: `double` when one of them
/// is, `int` otherwise (`char` and `bool` promote), and unknown when one of
/// them is unknown or not a number (reported at its input).
fn number_type(values: &[Expr]) -> Type {
    let numeric = |ty: &Type| matches!(ty, Type::Int | Type::Double | Type::Char | Type::Bool);
    if !values.iter().all(|value| numeric(&value.ty)) {
        return Type::Error;
    }
    values
        .iter()
        .fold(Type::Int, |ty, value| arithmetic_result(&ty, &value.ty))
}

impl Lowerer<'_> {
    /// *show numbers with … decimal places*.
    pub(super) fn text_statement(&mut self, block: &Block) -> Option<StmtKind> {
        match block.block_type.as_str() {
            "io.decimals" => {
                let [digits] = self.typed_values(block, [("DIGITS", part("The number of decimal places"))]);
                Some(StmtKind::Decimals { digits })
            }
            _ => None,
        }
    }

    /// A text value: *length of*, *letter of*, *as text*, …; or a math value
    /// ([`Self::math_value`]).
    pub(super) fn text_value(&mut self, block: &Block, origin: Origin) -> Expr {
        match block.block_type.as_str() {
            "text.length" => self.text_length(block, origin),
            "text.letter" => {
                let [index, text] = self.typed_values(
                    block,
                    [
                        ("INDEX", part("The position in 'letter of'")),
                        ("TEXT", part("'letter of'")),
                    ],
                );
                let kind = ExprKind::Letter {
                    text: Box::new(text),
                    index: Box::new(index),
                };
                Self::value(kind, Type::Char, origin)
            }
            "text.case" => {
                let options = [("lower", TextOp::ToLower), ("upper", TextOp::ToUpper)];
                let op = self.choice(block, "CASE", TextOp::ToLower, &options);
                let phrase = if op == TextOp::ToLower {
                    "'lowercase'"
                } else {
                    "'uppercase'"
                };
                let [text] = self.typed_values(block, [("TEXT", part(phrase))]);
                Self::text_call(op, vec![text], Type::String, origin)
            }
            "text.split" => {
                let args = self.typed_values(
                    block,
                    [
                        ("TEXT", part("The text to split")),
                        ("SEPARATOR", part("The separator")),
                    ],
                );
                let ty = Type::lib(LibType::Vector, vec![Type::String]).unwrap_or(Type::Error);
                Self::text_call(TextOp::Split, args.into(), ty, origin)
            }
            "text.to_number" => {
                let to = self.choice(
                    block,
                    "TO",
                    Type::Int,
                    &[("int", Type::Int), ("double", Type::Double)],
                );
                let [text] = self.typed_values(block, [("TEXT", part("'as number'"))]);
                let kind = ExprKind::ToNumber {
                    to: to.clone(),
                    text: Box::new(text),
                };
                Self::value(kind, to, origin)
            }
            "text.as_text" => self.as_text(block, origin),
            "text.find" => {
                let options = [
                    ("contains", TextOp::Contains),
                    ("starts", TextOp::StartsWith),
                    ("ends", TextOp::EndsWith),
                ];
                let op = self.choice(block, "OP", TextOp::Contains, &options);
                let args = self.typed_values(
                    block,
                    [
                        ("TEXT", part("The text to look in")),
                        ("PART", part("The text to look for")),
                    ],
                );
                Self::text_call(op, args.into(), Type::Bool, origin)
            }
            "text.part" => {
                let args = self.typed_values(
                    block,
                    [
                        ("TEXT", part("'part of'")),
                        ("START", part("The start of the part")),
                        ("LENGTH", part("The length of the part")),
                    ],
                );
                Self::text_call(TextOp::Part, args.into(), Type::String, origin)
            }
            "text.trim" => {
                let [text] = self.typed_values(block, [("TEXT", part("'trim'"))]);
                Self::text_call(TextOp::Trim, vec![text], Type::String, origin)
            }
            "text.format" => self.format_text(block, origin),
            "text.replace" => {
                let [old, new, text] = self.typed_values(
                    block,
                    [
                        ("OLD", part("The text to replace")),
                        ("NEW", part("The text to put in its place")),
                        ("TEXT", part("The text to change")),
                    ],
                );
                // `b2c::replace_all(text, old, new)`: the text first, as C++
                // takes it.
                Self::text_call(TextOp::ReplaceAll, vec![text, old, new], Type::String, origin)
            }
            _ => self.math_value(block, origin),
        }
    }

    /// A math value: *round*, *absolute value*, *larger*, *power*, *kept
    /// between*, *random decimal*.
    fn math_value(&mut self, block: &Block, origin: Origin) -> Expr {
        match block.block_type.as_str() {
            "math.fn" => {
                let op = self.choice(block, "FN", MathOp::Floor, &MATH_FUNCTIONS);
                let [value] = self.typed_values(block, [("VALUE", part("The number"))]);
                Self::math_call(op, vec![value], Type::Double, origin)
            }
            "math.abs" => {
                let [value] = self.typed_values(block, [("VALUE", part("The number"))]);
                let ty = number_type(std::slice::from_ref(&value));
                Self::math_call(MathOp::Abs, vec![value], ty, origin)
            }
            "math.minmax" => {
                let op = self.choice(
                    block,
                    "FN",
                    MathOp::Max,
                    &[("max", MathOp::Max), ("min", MathOp::Min)],
                );
                let args = self.typed_values(
                    block,
                    [("A", part("The first number")), ("B", part("The second number"))],
                );
                let ty = number_type(&args);
                Self::math_call(op, args.into(), ty, origin)
            }
            "math.fn_two" => {
                let op = self.choice(block, "FN", MathOp::Pow, &MATH_FUNCTIONS2);
                let args = self.typed_values(
                    block,
                    [("A", part("The first number")), ("B", part("The second number"))],
                );
                Self::math_call(op, args.into(), Type::Double, origin)
            }
            "math.clamp" => {
                let args = self.typed_values(
                    block,
                    [
                        ("VALUE", part("The number")),
                        ("LOW", part("The lowest value")),
                        ("HIGH", part("The highest value")),
                    ],
                );
                let ty = number_type(&args);
                Self::math_call(MathOp::Clamp, args.into(), ty, origin)
            }
            "math.random_real" => {
                let [low, high] = self.typed_values(
                    block,
                    [("LOW", Subject::RandomLow), ("HIGH", Subject::RandomHigh)],
                );
                let kind = ExprKind::RandomReal {
                    low: Box::new(low),
                    high: Box::new(high),
                };
                Self::value(kind, Type::Double, origin)
            }
            _ => error_expr(origin),
        }
    }

    /// *length of*: a character is refused (`B2C-E0301`), since it has no
    /// length of its own; text is counted.
    fn text_length(&mut self, block: &Block, origin: Origin) -> Expr {
        let [text] = self.typed_values_with(block, [("TEXT", part("'length of'"))], |lowerer, value| {
            if value.ty != Type::Char {
                return false;
            }
            if lowerer.checking() {
                lowerer.diags.error(
                    codes::CONVERSION,
                    value.origin.location(),
                    "'length of' needs text, but this is a character. Use a text value here.",
                );
            }
            true
        });
        Self::value(ExprKind::TextLength(Box::new(text)), Type::Int, origin)
    }

    /// *as text*: the types *join* takes, written as *join* writes them
    /// (w2.md §2.10), refused as *join* refuses them (`B2C-E0314`); a struct
    /// is refused too, also a printable one: C++ cannot add it to text, so
    /// its fields are joined instead.
    fn as_text(&mut self, block: &Block, origin: Origin) -> Expr {
        let [value] = self.typed_values(block, [("VALUE", part("'as text'"))]);
        let is_struct = value.ty.is_in(TypeClass::Struct, &self.symbols) == Some(true);
        if is_struct && self.checking() {
            let message = format!(
                "{} can't be turned into text in one go. Join its fields instead.",
                self.struct_name(&value.ty)
            );
            self.diags
                .error(codes::NOT_PRINTABLE, value.origin.location(), message);
        } else if !is_struct {
            self.check_printable(&value, Output::Joined);
        }
        Self::value(ExprKind::AsText(Box::new(value)), Type::String, origin)
    }

    /// A struct's name for a message, capitalised for the start of a
    /// sentence: `` `Student` ``, or "A struct" when it has none.
    fn struct_name(&self, ty: &Type) -> String {
        match ty {
            Type::Named(id) => self
                .symbols
                .get(id)
                .map_or_else(|| String::from("A struct"), |symbol| quoted(symbol.name.as_str())),
            _ => String::from("A struct"),
        }
    }

    /// Lowers, types and reports the value inputs `parts` of a block, in
    /// catalog order, each named by its subject in messages. A missing
    /// input is reported as missing and becomes a placeholder.
    pub(super) fn typed_values<const N: usize>(
        &mut self,
        block: &Block,
        parts: [(&str, Subject); N],
    ) -> [Expr; N] {
        self.typed_values_with(block, parts, |_, _| false)
    }

    /// [`Self::typed_values`], where `own_rule` may report a value itself
    /// first: when it returns `true`, the value's fit is not reported again.
    pub(super) fn typed_values_with<const N: usize>(
        &mut self,
        block: &Block,
        parts: [(&str, Subject); N],
        mut own_rule: impl FnMut(&mut Self, &Expr) -> bool,
    ) -> [Expr; N] {
        // The block's own signature, also when it has no pattern: its inputs'
        // classes are checked all the same.
        let sigs = self.sigs;
        let sig = sigs
            .get(&block.block_type)
            .unwrap_or_else(|| self.block_sig(&block.block_type));
        let mut typed = self.typed(sig);
        for (name, _) in &parts {
            self.typed_input(block, &mut typed, name, true);
        }
        let typing = self.type_parts(block, &mut typed, None);
        parts.map(|(name, subject)| {
            let value = typed.take(name).unwrap_or_else(|| {
                error_expr(self.origin(
                    &block.id,
                    Part::Input {
                        name: name.to_owned(),
                    },
                ))
            });
            if !own_rule(self, &value) {
                self.report_fit(&typed, &typing, name, &value, &subject);
            }
            value
        })
    }

    /// An expression node.
    fn value(kind: ExprKind, ty: Type, origin: Origin) -> Expr {
        Expr { kind, ty, origin }
    }

    /// A text operation; unknown when one of its operands is.
    fn text_call(op: TextOp, args: Vec<Expr>, ty: Type, origin: Origin) -> Expr {
        let ty = if args.iter().any(|arg| arg.ty == Type::Error) {
            Type::Error
        } else {
            ty
        };
        Self::value(ExprKind::TextCall { op, args }, ty, origin)
    }

    /// A function of `<cmath>`, `<cstdlib>` or `<algorithm>`; unknown when
    /// one of its operands is.
    fn math_call(op: MathOp, args: Vec<Expr>, ty: Type, origin: Origin) -> Expr {
        let ty = if args.iter().any(|arg| arg.ty == Type::Error) {
            Type::Error
        } else {
            ty
        };
        Self::value(ExprKind::MathCall { op, args }, ty, origin)
    }
}

// --- format … with (w4-editor-tools.md §7) ------------------------------------

/// What one place of a *format* pattern (`{}`, `{:.2f}`) asks of its value:
/// a precision, and a presentation letter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FormatPlace {
    /// `.N`: a number of decimal places, or of characters of a text.
    precision: bool,
    /// The presentation letter (`f`, `d`, `x`, `s`, …).
    kind: Option<char>,
}

impl FormatPlace {
    /// Whether a value of `ty` (one `std::format` takes) fits the place.
    fn fits(self, ty: &Type) -> bool {
        let (precision, kinds) = match ty {
            Type::Double => (true, "eEfFgG"),
            Type::String => (true, "s"),
            Type::Int => (false, "dxXobBc"),
            Type::Char => (false, "cdxXobB"),
            Type::Bool => (false, "sdxXobB"),
            _ => return true,
        };
        (precision || !self.precision) && self.kind.is_none_or(|kind| kinds.contains(kind))
    }
}

/// The places of a *format … with* pattern, in order, or why `std::format`
/// would not take it: `{{` and `}}` are a brace each, and a place is `{}` or
/// `{:spec}`, the spec `[[fill]align][width][.precision][letter]` (the
/// subset the block offers; numbered places and the `+`, `#` flags are not).
pub(crate) fn format_places(pattern: &str) -> Result<Vec<FormatPlace>, String> {
    let mut places = Vec::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
            }
            '{' => {
                let mut inner = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some('{') | None => {
                            return Err(String::from(
                                "a '{' has no '}' after it (write '{{' for a brace of its own)",
                            ));
                        }
                        Some(c) => inner.push(c),
                    }
                }
                places.push(format_place(&inner)?);
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
            }
            '}' => {
                return Err(String::from(
                    "a '}' has no '{' before it (write '}}' for a brace of its own)",
                ));
            }
            _ => {}
        }
    }
    Ok(places)
}

/// One place's spec (what is between its braces).
fn format_place(inner: &str) -> Result<FormatPlace, String> {
    let unknown =
        || format!("'{{{inner}}}' is not a place 'format' knows: write {{}}, or for example {{:.2f}}");
    let spec: Vec<char> = match inner.strip_prefix(':') {
        Some(spec) => spec.chars().collect(),
        None if inner.is_empty() => Vec::new(),
        None => return Err(unknown()),
    };
    let is_align = |c: &char| matches!(c, '<' | '>' | '^');
    let mut at = if spec.get(1).is_some_and(is_align) {
        2
    } else {
        usize::from(spec.first().is_some_and(is_align))
    };
    while spec.get(at).is_some_and(char::is_ascii_digit) {
        at += 1;
    }
    let precision = spec.get(at) == Some(&'.');
    if precision {
        at += 1;
        let digits = at;
        while spec.get(at).is_some_and(char::is_ascii_digit) {
            at += 1;
        }
        if at == digits {
            return Err(unknown());
        }
    }
    let kind = spec.get(at).copied();
    at += usize::from(kind.is_some());
    if at != spec.len() || kind.is_some_and(|kind| !"sdxXobBceEfFgG".contains(kind)) {
        return Err(unknown());
    }
    Ok(FormatPlace { precision, kind })
}

impl Lowerer<'_> {
    /// *format (pattern) with (a) (b) …*: `std::format("pattern", a, b)`.
    /// The pattern is a text field, which `std::format` needs fixed when the
    /// program is built; its places must be as many as the values and fit
    /// them, and each value is a number, a character, a true/false value or
    /// text.
    fn format_text(&mut self, block: &Block, origin: Origin) -> Expr {
        let location = self.loc(&block.id, super::field("FORMAT"));
        let count = crate::access::extra_count(block, "itemCount", 1, 2);
        let values: Vec<Expr> = (0..count)
            .map(|i| self.required_input(block, &format!("ITEM{i}")))
            .collect();
        let Ok(text) = crate::access::text_field(block, "FORMAT", Some("")) else {
            self.incomplete(
                location,
                "The pattern of this 'format' block is damaged. Type it again.",
            );
            return error_expr(origin);
        };
        let pattern = self.str_literal(text, origin.clone(), location.clone());
        if !self.checking() {
            let mut args = vec![pattern];
            args.extend(values);
            return Self::text_call(TextOp::Format, args, Type::String, origin);
        }
        for value in &values {
            let formattable = matches!(
                value.ty,
                Type::Bool | Type::Char | Type::Int | Type::Double | Type::String | Type::Error
            );
            if !formattable {
                let message = format!(
                    "'format' can put in whole numbers, decimal numbers, characters, true/false values and \
                     text, not {}. Use 'as text' on it first, or 'join'.",
                    self.type_name(&value.ty)
                );
                self.diags
                    .error(codes::NOT_PRINTABLE, value.origin.location(), message);
            }
        }
        match format_places(text) {
            Err(problem) => {
                let message = format!("This pattern can't be used by 'format': {problem}.");
                self.diags.error(codes::BAD_LITERAL, location, message);
            }
            Ok(places) if places.len() != values.len() => {
                let message = format!(
                    "The pattern has {} for values, but the block gives it {}. Add or remove values with ⊕ \
                     and ⊖, or change the pattern.",
                    counted(places.len(), "place"),
                    counted(values.len(), "value")
                );
                self.diags.error(codes::ARGUMENT_COUNT, location, message);
            }
            Ok(places) => {
                for (number, (place, value)) in places.iter().zip(&values).enumerate() {
                    if !place.fits(&value.ty) {
                        let message = format!(
                            "Place {} of the pattern does not fit its value, {}: a number of decimal places \
                             or a letter such as 'f' needs a value of its kind.",
                            number + 1,
                            self.type_name(&value.ty)
                        );
                        self.diags.error(codes::BAD_LITERAL, location.clone(), message);
                    }
                }
            }
        }
        let mut args = vec![pattern];
        args.extend(values);
        Self::text_call(TextOp::Format, args, Type::String, origin)
    }
}

/// `1 place`, `2 places`.
fn counted(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}
