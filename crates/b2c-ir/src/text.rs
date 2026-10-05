//! Typed text leaves: the only way user-provided text reaches generated C++.
//!
//! Every value here is validated when it is constructed ("parse, don't
//! validate") and encoded for C++ by a dedicated function when it is emitted.
//! The rules are specified in `docs/spec/08-security.md` §8.4:
//!
//! * [`Ident`]: identifier rules (§8.4.1).
//! * [`StrLit`] / [`CharLit`]: string and character literal encoding (§8.4.2).
//! * [`Comment`]: comment encoding that cannot hide code (§8.4.3).
//! * [`NumLit`]: numeric literal grammar and range checks (§8.4.4).
//!
//! There is deliberately no `From<String>` for any of these types.

use std::fmt::{self, Write as _};

use serde::{Deserialize, Serialize};

use crate::reserved_names::{GLOBAL_NAMES, MACRO_NAMES};

/// Maximum identifier length in characters.
pub const MAX_IDENT_LEN: usize = 64;
/// Maximum length of string-literal and comment text, in bytes.
pub const MAX_TEXT_LEN: usize = 64 * 1024;

// ---------------------------------------------------------------------------
// Identifiers
// ---------------------------------------------------------------------------

/// Why a name is not a valid C++ identifier for user code.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdentError {
    /// The name is empty.
    #[error("a name cannot be empty")]
    Empty,
    /// The name is longer than [`MAX_IDENT_LEN`].
    #[error("a name can be at most {MAX_IDENT_LEN} characters long")]
    TooLong,
    /// The name does not start with an ASCII letter.
    #[error("a name must start with a letter (A–Z or a–z)")]
    BadStart,
    /// The name contains a character other than ASCII letters, digits or `_`.
    #[error("a name can only contain letters, digits and underscores (found {0:?})")]
    BadChar(char),
    /// The name contains `__`, which C++ reserves.
    #[error("a name cannot contain two underscores in a row (C++ reserves those names)")]
    DoubleUnderscore,
    /// The name is a C++ keyword or alternative token.
    #[error("`{0}` is a C++ keyword")]
    Keyword(String),
    /// The name is a macro defined by the standard library or the compiler.
    #[error("`{0}` is already used by the C++ standard library (as a macro)")]
    Macro(String),
    /// The name is reserved by Blocks2Cpp's generated code.
    #[error("`{0}` is reserved by Blocks2Cpp")]
    Reserved(String),
    /// The name clashes with a C library name in the global namespace.
    #[error("`{0}` is already the name of a C library function or type; choose another name")]
    GlobalClash(String),
}

/// A validated C++ identifier for user code (spec §8.4.1).
///
/// The only constructors are [`Ident::new`] (user names) and
/// [`Ident::generated`] (names created by the code generator).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Ident(Box<str>);

/// C++ keywords (up to C++26), alternative tokens and contextual keywords.
const KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "and",
    "and_eq",
    "asm",
    "auto",
    "bitand",
    "bitor",
    "bool",
    "break",
    "case",
    "catch",
    "char",
    "char16_t",
    "char32_t",
    "char8_t",
    "class",
    "co_await",
    "co_return",
    "co_yield",
    "compl",
    "concept",
    "const",
    "const_cast",
    "consteval",
    "constexpr",
    "constinit",
    "continue",
    "contract_assert",
    "decltype",
    "default",
    "delete",
    "do",
    "double",
    "dynamic_cast",
    "else",
    "enum",
    "explicit",
    "export",
    "extern",
    "false",
    "final",
    "float",
    "for",
    "friend",
    "goto",
    "if",
    "import",
    "inline",
    "int",
    "long",
    "module",
    "mutable",
    "namespace",
    "new",
    "noexcept",
    "not",
    "not_eq",
    "nullptr",
    "operator",
    "or",
    "or_eq",
    "override",
    "private",
    "protected",
    "public",
    "register",
    "reinterpret_cast",
    "requires",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "static_assert",
    "static_cast",
    "struct",
    "switch",
    "template",
    "this",
    "thread_local",
    "throw",
    "true",
    "try",
    "typedef",
    "typeid",
    "typename",
    "union",
    "unsigned",
    "using",
    "virtual",
    "void",
    "volatile",
    "wchar_t",
    "while",
    "xor",
    "xor_eq",
];

/// Names the generator itself uses at global or `std` scope.
const GENERATOR_RESERVED: &[&str] = &["main", "std"];

/// Prefix reserved for names created by the generator (case-insensitive).
const GENERATED_PREFIX: &str = "b2c";

impl Ident {
    /// Validates a user-chosen name for use at block or function scope.
    ///
    /// Names at namespace scope (functions, global variables, types) must also
    /// pass [`Ident::check_namespace_scope`].
    ///
    /// # Errors
    /// Returns the first rule the name breaks.
    pub fn new(name: &str) -> Result<Self, IdentError> {
        check_shape(name)?;
        if name.len() >= GENERATED_PREFIX.len()
            && name.as_bytes()[..GENERATED_PREFIX.len()].eq_ignore_ascii_case(GENERATED_PREFIX.as_bytes())
        {
            return Err(IdentError::Reserved(name.to_owned()));
        }
        if GENERATOR_RESERVED.contains(&name) {
            return Err(IdentError::Reserved(name.to_owned()));
        }
        Ok(Self(name.into()))
    }

    /// Validates a name that the code generator creates (temporaries, helper
    /// names). Such names must start with `b2c` so they can never collide with
    /// user names, which are not allowed to.
    ///
    /// # Errors
    /// Returns an error if the name is malformed or lacks the `b2c` prefix.
    pub fn generated(name: &str) -> Result<Self, IdentError> {
        check_shape(name)?;
        if !name.starts_with(GENERATED_PREFIX) {
            return Err(IdentError::Reserved(name.to_owned()));
        }
        Ok(Self(name.into()))
    }

    /// The name of `main`, which only the generator may emit.
    pub fn main() -> Self {
        Self("main".into())
    }

    /// Checks the extra rule for names declared at namespace scope (functions,
    /// global variables, types): they must not clash with a name the standard
    /// headers declare in the global namespace, such as `abs` or `time`.
    ///
    /// # Errors
    /// Returns [`IdentError::GlobalClash`] if the name clashes.
    pub fn check_namespace_scope(&self) -> Result<(), IdentError> {
        if GLOBAL_NAMES.binary_search(&&*self.0).is_ok() {
            return Err(IdentError::GlobalClash(self.0.to_string()));
        }
        Ok(())
    }

    /// The identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Ident {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Ident {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        if name.starts_with(GENERATED_PREFIX) {
            Self::generated(&name).map_err(serde::de::Error::custom)
        } else if name == "main" {
            Ok(Self::main())
        } else {
            Self::new(&name).map_err(serde::de::Error::custom)
        }
    }
}

/// Rules shared by user and generated identifiers.
fn check_shape(name: &str) -> Result<(), IdentError> {
    let mut chars = name.chars();
    let first = chars.next().ok_or(IdentError::Empty)?;
    if name.len() > MAX_IDENT_LEN {
        return Err(IdentError::TooLong);
    }
    if !first.is_ascii_alphabetic() {
        return Err(IdentError::BadStart);
    }
    if let Some(bad) = chars.find(|c| !(c.is_ascii_alphanumeric() || *c == '_')) {
        return Err(IdentError::BadChar(bad));
    }
    if name.contains("__") {
        return Err(IdentError::DoubleUnderscore);
    }
    if KEYWORDS.binary_search(&name).is_ok() {
        return Err(IdentError::Keyword(name.to_owned()));
    }
    if MACRO_NAMES.binary_search(&name).is_ok() {
        return Err(IdentError::Macro(name.to_owned()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// String and character literals
// ---------------------------------------------------------------------------

/// Why text cannot be used in a string or character literal.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LiteralError {
    /// The text contains a NUL character.
    #[error("text cannot contain the NUL character")]
    Nul,
    /// The text is longer than [`MAX_TEXT_LEN`].
    #[error("text can be at most {MAX_TEXT_LEN} bytes long")]
    TooLong,
    /// A character literal must hold exactly one character.
    #[error("a character value must be exactly one character")]
    NotOneChar,
    /// A `char` value must be ASCII.
    #[error("a character value must be a plain ASCII character; use text for {0:?}")]
    NotAscii(char),
}

/// Text for a C++ narrow string literal (spec §8.4.2).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct StrLit(Box<str>);

impl StrLit {
    /// Validates text for a string literal.
    ///
    /// # Errors
    /// Returns an error if the text contains NUL or is too long.
    pub fn new(text: &str) -> Result<Self, LiteralError> {
        if text.len() > MAX_TEXT_LEN {
            return Err(LiteralError::TooLong);
        }
        if text.contains('\0') {
            return Err(LiteralError::Nul);
        }
        Ok(Self(text.into()))
    }

    /// The literal's value (not encoded).
    pub fn value(&self) -> &str {
        &self.0
    }

    /// Encodes the value as a C++ string literal, including the quotes.
    pub fn to_cpp(&self) -> String {
        let mut out = String::with_capacity(self.0.len() + 2);
        out.push('"');
        let mut previous = None;
        for c in self.0.chars() {
            encode_literal_char(c, '"', previous, &mut out);
            previous = Some(c);
        }
        out.push('"');
        out
    }
}

impl<'de> Deserialize<'de> for StrLit {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// A C++ `char` literal holding one ASCII character (spec §8.4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct CharLit(char);

impl CharLit {
    /// Validates a character value.
    ///
    /// # Errors
    /// Returns an error unless the text is exactly one non-NUL ASCII character.
    pub fn new(text: &str) -> Result<Self, LiteralError> {
        let mut chars = text.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return Err(LiteralError::NotOneChar);
        };
        if c == '\0' {
            return Err(LiteralError::Nul);
        }
        if !c.is_ascii() {
            return Err(LiteralError::NotAscii(c));
        }
        Ok(Self(c))
    }

    /// The character value.
    pub fn value(self) -> char {
        self.0
    }

    /// Encodes the value as a C++ character literal, including the quotes.
    pub fn to_cpp(self) -> String {
        let mut out = String::from("'");
        encode_literal_char(self.0, '\'', None, &mut out);
        out.push('\'');
        out
    }
}

impl<'de> Deserialize<'de> for CharLit {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Code points that are escaped as universal character names in literals and
/// replaced by placeholders in comments: C1 controls, every format character
/// (Unicode category Cf, which includes the bidi controls used by "Trojan
/// Source" attacks), line and paragraph separators, and noncharacters.
/// Generated from Unicode 14 plus the Cf additions of Unicode 15–16.
#[allow(clippy::unreadable_literal)] // code points, as written in the Unicode tables
const INVISIBLE_RANGES: &[(u32, u32)] = &[
    (0x0080, 0x009F),
    (0x00AD, 0x00AD),
    (0x0600, 0x0605),
    (0x061C, 0x061C),
    (0x06DD, 0x06DD),
    (0x070F, 0x070F),
    (0x0890, 0x0891),
    (0x08E2, 0x08E2),
    (0x180E, 0x180E),
    (0x200B, 0x200F),
    (0x2028, 0x202E),
    (0x2060, 0x2064),
    (0x2066, 0x206F),
    (0xFDD0, 0xFDEF),
    (0xFEFF, 0xFEFF),
    (0xFFF9, 0xFFFB),
    (0xFFFE, 0xFFFF),
    (0x110BD, 0x110BD),
    (0x110CD, 0x110CD),
    (0x13430, 0x1343F),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0x1FFFE, 0x1FFFF),
    (0x2FFFE, 0x2FFFF),
    (0x3FFFE, 0x3FFFF),
    (0x4FFFE, 0x4FFFF),
    (0x5FFFE, 0x5FFFF),
    (0x6FFFE, 0x6FFFF),
    (0x7FFFE, 0x7FFFF),
    (0x8FFFE, 0x8FFFF),
    (0x9FFFE, 0x9FFFF),
    (0xAFFFE, 0xAFFFF),
    (0xBFFFE, 0xBFFFF),
    (0xCFFFE, 0xCFFFF),
    (0xDFFFE, 0xDFFFF),
    (0xE0001, 0xE0001),
    (0xE0020, 0xE007F),
    (0xEFFFE, 0xEFFFF),
    (0xFFFFE, 0xFFFFF),
    (0x10FFFE, 0x10FFFF),
];

/// Whether a character is invisible or reorders text: a C1 control, a format
/// character (Unicode category Cf, including the bidi controls used by
/// "Trojan Source" attacks), a line or paragraph separator, or a
/// noncharacter. These are the characters [`StrLit`] and [`CharLit`] escape
/// as universal character names and [`Comment`] replaces by placeholders.
pub fn is_invisible(c: char) -> bool {
    let cp = u32::from(c);
    if cp < 0x80 {
        return false;
    }
    INVISIBLE_RANGES
        .binary_search_by(|&(lo, hi)| {
            if hi < cp {
                std::cmp::Ordering::Less
            } else if lo > cp {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Appends the C++ encoding of one literal character (spec §8.4.2 table).
///
/// `previous` is the character before it in the same literal. A `?` that
/// follows another `?` is escaped, so the output never contains `??` and
/// cannot form a trigraph, while ordinary prompts such as `"Age? "` stay
/// readable.
fn encode_literal_char(c: char, quote: char, previous: Option<char>, out: &mut String) {
    match c {
        '\\' => out.push_str("\\\\"),
        '?' if previous == Some('?') => out.push_str("\\?"),
        '\n' => out.push_str("\\n"),
        '\t' => out.push_str("\\t"),
        '\r' => out.push_str("\\r"),
        c if c == quote => {
            out.push('\\');
            out.push(c);
        }
        // Remaining C0 controls and DEL: 3-digit octal escapes stop after three
        // digits, unlike greedy `\x` escapes.
        c if (c as u32) < 0x20 || c == '\u{7f}' => {
            let _ = write!(out, "\\{:03o}", c as u32);
        }
        c if is_invisible(c) => {
            let cp = c as u32;
            if cp <= 0xFFFF {
                let _ = write!(out, "\\u{cp:04X}");
            } else {
                let _ = write!(out, "\\U{cp:08X}");
            }
        }
        c => out.push(c),
    }
}

// ---------------------------------------------------------------------------
// Comments
// ---------------------------------------------------------------------------

/// Text for a C++ comment (spec §8.4.3).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Comment(Box<str>);

impl Comment {
    /// Validates comment text (any characters are allowed; they are made safe
    /// when encoded).
    ///
    /// # Errors
    /// Returns an error if the text is too long.
    pub fn new(text: &str) -> Result<Self, LiteralError> {
        if text.len() > MAX_TEXT_LEN {
            return Err(LiteralError::TooLong);
        }
        Ok(Self(text.into()))
    }

    /// The comment text (not encoded).
    pub fn text(&self) -> &str {
        &self.0
    }

    /// Encodes the comment as `//` lines (without indentation or newlines).
    ///
    /// * Every line terminator (CR, LF, CRLF, NEL, LS, PS, VT, FF) starts a new
    ///   line, so text can never continue outside the comment.
    /// * Controls and invisible characters become visible placeholders such as
    ///   `<U+202E>`.
    /// * Trailing whitespace is trimmed, and a line that would end in `\` gets
    ///   the sentinel ` //`, because a trailing backslash (even followed by
    ///   spaces) splices the next line into the comment.
    pub fn to_cpp_lines(&self) -> Vec<String> {
        let normalized = self.0.replace("\r\n", "\n");
        normalized
            .split(['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}', '\u{0B}', '\u{0C}'])
            .map(|line| {
                let mut text = String::with_capacity(line.len());
                for c in line.chars() {
                    if c == '\t' {
                        text.push(c);
                    } else if c.is_control() || is_invisible(c) {
                        let _ = write!(text, "<U+{:04X}>", c as u32);
                    } else {
                        text.push(c);
                    }
                }
                let trimmed = text.trim_end();
                let mut encoded = if trimmed.is_empty() {
                    String::from("//")
                } else {
                    format!("// {trimmed}")
                };
                // A trailing `\` would splice the next line into the comment;
                // a trailing `??/` would too if trigraphs were enabled, and
                // GCC's -Wtrigraphs (part of -Wall) warns about it.
                if encoded.ends_with('\\') || encoded.ends_with("??/") {
                    encoded.push_str(" //");
                }
                encoded
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Numeric literals
// ---------------------------------------------------------------------------

/// The C++ type a numeric literal is checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumType {
    /// `int` (32-bit two's complement on every supported target).
    Int,
    /// `long long` (64-bit).
    LongLong,
    /// `double` (IEEE 754 binary64).
    Double,
}

/// Why text is not a valid numeric literal.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NumError {
    /// Not a number in C++ literal syntax.
    #[error("`{0}` is not a number")]
    Syntax(String),
    /// Too large (or too small) for the target type.
    #[error("`{text}` does not fit in {ty}")]
    OutOfRange {
        /// The literal as written.
        text: String,
        /// The target type's C++ name.
        ty: &'static str,
    },
}

/// A validated numeric literal, printed in a normalised form (spec §8.4.4).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct NumLit {
    repr: Box<str>,
    ty: NumType,
}

impl NumType {
    /// The C++ spelling of the type.
    pub fn cpp_name(self) -> &'static str {
        match self {
            Self::Int => "int",
            Self::LongLong => "long long",
            Self::Double => "double",
        }
    }
}

impl NumLit {
    /// Parses a literal written by the user and checks it fits `ty`.
    ///
    /// Integers: decimal (`42`), hexadecimal (`0x2A`) or binary (`0b101010`),
    /// with optional `'` digit separators. Floating point: decimal with a
    /// fraction and/or exponent (`3.14`, `.5`, `1e-3`). A leading sign is not
    /// part of a literal (unary minus is an operator). Integers are accepted for
    /// `double` too.
    ///
    /// # Errors
    /// Returns [`NumError::Syntax`] or [`NumError::OutOfRange`].
    pub fn parse(text: &str, ty: NumType) -> Result<Self, NumError> {
        let syntax = || NumError::Syntax(text.to_owned());
        if text.is_empty() || text.len() > 400 {
            return Err(syntax());
        }
        match ty {
            NumType::Int | NumType::LongLong => {
                let value = parse_integer(text).ok_or_else(syntax)?;
                let max = if ty == NumType::Int {
                    i128::from(i32::MAX)
                } else {
                    i128::from(i64::MAX)
                };
                if value > max {
                    return Err(NumError::OutOfRange {
                        text: text.to_owned(),
                        ty: ty.cpp_name(),
                    });
                }
                Ok(Self {
                    repr: value.to_string().into(),
                    ty,
                })
            }
            NumType::Double => {
                if let Some(value) = parse_integer(text) {
                    // Exact up to 2^53; larger integers round, as in C++.
                    #[allow(clippy::cast_precision_loss)]
                    return Self::from_f64(value as f64).ok_or_else(|| NumError::OutOfRange {
                        text: text.to_owned(),
                        ty: ty.cpp_name(),
                    });
                }
                let cleaned = strip_separators(text, 10).ok_or_else(syntax)?;
                if !is_decimal_float(&cleaned) {
                    return Err(syntax());
                }
                let value: f64 = cleaned.parse().map_err(|_| syntax())?;
                Self::from_f64(value).ok_or_else(|| NumError::OutOfRange {
                    text: text.to_owned(),
                    ty: ty.cpp_name(),
                })
            }
        }
    }

    /// A literal for a known integer value (used by the generator, e.g. `1`).
    pub fn int(value: i32) -> Self {
        if value < 0 {
            // Negative literals do not exist in C++; callers negate with unary minus.
            Self {
                repr: i64::from(value).unsigned_abs().to_string().into(),
                ty: NumType::Int,
            }
        } else {
            Self {
                repr: value.to_string().into(),
                ty: NumType::Int,
            }
        }
    }

    /// A `double` literal for a finite, non-negative value.
    pub fn from_f64(value: f64) -> Option<Self> {
        if !value.is_finite() || value.is_sign_negative() {
            return None;
        }
        // Rust's `{:?}` prints the shortest text that round-trips and always
        // includes a `.` or an exponent, which is valid C++ double syntax.
        Some(Self {
            repr: format!("{value:?}").into(),
            ty: NumType::Double,
        })
    }

    /// The literal as C++ source text.
    pub fn as_str(&self) -> &str {
        &self.repr
    }

    /// The type the literal was checked against.
    pub fn num_type(&self) -> NumType {
        self.ty
    }
}

/// Removes C++14 digit separators, rejecting misplaced ones. As in C++, a `'`
/// must stand between two digits of `radix`: not first or last, not next to
/// another `'`, a base prefix (`0x'1`), a decimal point (`1.'5`) or an
/// exponent (`1e'5`), all of which GCC rejects.
fn strip_separators(text: &str, radix: u32) -> Option<String> {
    let bytes = text.as_bytes();
    let is_digit = |index: Option<usize>| {
        index
            .and_then(|index| bytes.get(index))
            .is_some_and(|&b| char::from(b).is_digit(radix))
    };
    for (index, &b) in bytes.iter().enumerate() {
        if b == b'\'' && !(is_digit(index.checked_sub(1)) && is_digit(Some(index + 1))) {
            return None;
        }
    }
    Some(text.replace('\'', ""))
}

/// Parses a non-negative integer literal; `None` if it is not one.
fn parse_integer(text: &str) -> Option<i128> {
    let (digits, radix) = if let Some(rest) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        (rest, 16)
    } else if let Some(rest) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
        (rest, 2)
    } else {
        (text, 10)
    };
    let digits = strip_separators(digits, radix)?;
    // A leading zero would make C++ read the number as octal.
    if radix == 10 && digits.len() > 1 && digits.starts_with('0') {
        return None;
    }
    if digits.is_empty() || digits.len() > 128 || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    i128::from_str_radix(&digits, radix).ok()
}

/// Checks C++ decimal floating-point syntax (without suffix or sign).
fn is_decimal_float(text: &str) -> bool {
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(pos) => (&text[..pos], Some(&text[pos + 1..])),
        None => (text, None),
    };
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (mantissa, None),
    };
    let all_digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    if !all_digits(int_part) || !frac_part.is_none_or(all_digits) {
        return false;
    }
    if int_part.is_empty() && frac_part.is_none_or(str::is_empty) {
        return false;
    }
    if frac_part.is_none() && exponent.is_none() {
        return false;
    }
    match exponent {
        None => true,
        Some(exp) => {
            let digits = exp.strip_prefix(['+', '-']).unwrap_or(exp);
            !digits.is_empty() && all_digits(digits)
        }
    }
}

// ---------------------------------------------------------------------------
// Raw C++
// ---------------------------------------------------------------------------

/// Where unchecked C++ text came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawProvenance {
    /// A Raw C++ block written by the user (trust-gated, spec §3.10).
    RawBlock,
    /// A template from a library pack (validated at pack load, spec §3.11.2).
    CatalogTemplate,
}

/// Unchecked C++ text. Only Raw C++ blocks and catalog templates create it,
/// and it carries its provenance so the UI can mark it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct RawCode {
    text: Box<str>,
    provenance: RawProvenance,
}

impl RawCode {
    /// Wraps text from a Raw C++ block or a validated catalog template.
    ///
    /// # Errors
    /// Returns an error if the text is too long or contains NUL.
    pub fn new(text: &str, provenance: RawProvenance) -> Result<Self, LiteralError> {
        if text.len() > 4 * MAX_TEXT_LEN {
            return Err(LiteralError::TooLong);
        }
        if text.contains('\0') {
            return Err(LiteralError::Nul);
        }
        Ok(Self {
            text: text.into(),
            provenance,
        })
    }

    /// The raw text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Where the text came from.
    pub fn provenance(&self) -> RawProvenance {
        self.provenance
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn keyword_table_is_sorted() {
        assert!(KEYWORDS.windows(2).all(|w| w[0] < w[1]));
        assert!(MACRO_NAMES.windows(2).all(|w| w[0] < w[1]));
        assert!(GLOBAL_NAMES.windows(2).all(|w| w[0] < w[1]));
        assert!(INVISIBLE_RANGES.windows(2).all(|w| w[0].1 < w[1].0));
    }

    #[test]
    fn identifiers() {
        for good in ["score", "x", "Player", "max", "total_2", "y1", "abs", "x9_"] {
            assert!(Ident::new(good).is_ok(), "{good}");
        }
        let cases: &[(&str, IdentError)] = &[
            ("", IdentError::Empty),
            ("_x", IdentError::BadStart),
            ("9lives", IdentError::BadStart),
            ("a-b", IdentError::BadChar('-')),
            ("naïve", IdentError::BadChar('ï')),
            ("a__b", IdentError::DoubleUnderscore),
            ("class", IdentError::Keyword("class".into())),
            ("and", IdentError::Keyword("and".into())),
            ("final", IdentError::Keyword("final".into())),
            ("linux", IdentError::Macro("linux".into())),
            ("errno", IdentError::Macro("errno".into())),
            ("NULL", IdentError::Macro("NULL".into())),
            ("main", IdentError::Reserved("main".into())),
            ("std", IdentError::Reserved("std".into())),
            ("b2cTemp", IdentError::Reserved("b2cTemp".into())),
            ("B2C_X", IdentError::Reserved("B2C_X".into())),
        ];
        for (name, err) in cases {
            assert_eq!(Ident::new(name).as_ref().err(), Some(err), "{name}");
        }
        assert_eq!(Ident::new(&"a".repeat(65)), Err(IdentError::TooLong));
        assert!(Ident::new(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn namespace_scope_names() {
        for clash in ["abs", "time", "y1", "index", "exit", "printf"] {
            let ident = Ident::new(clash).unwrap();
            assert_eq!(
                ident.check_namespace_scope(),
                Err(IdentError::GlobalClash(clash.into()))
            );
        }
        assert!(Ident::new("greet").unwrap().check_namespace_scope().is_ok());
    }

    #[test]
    fn generated_identifiers() {
        assert!(Ident::generated("b2c_count").is_ok());
        assert!(Ident::generated("count").is_err());
        assert!(Ident::generated("b2c__x").is_err());
    }

    #[test]
    fn string_literals() {
        let enc = |s: &str| StrLit::new(s).unwrap().to_cpp();
        assert_eq!(enc("Hello, world!"), r#""Hello, world!""#);
        assert_eq!(enc(r#"say "hi" \ ok"#), r#""say \"hi\" \\ ok""#);
        assert_eq!(enc("a\nb\tc\rd"), r#""a\nb\tc\rd""#);
        assert_eq!(enc("??/"), r#""?\?/""#);
        assert_eq!(enc("???="), r#""?\?\?=""#);
        assert_eq!(enc("Age? "), r#""Age? ""#);
        assert_eq!(enc("? ?"), r#""? ?""#);
        assert_eq!(enc("\u{1}BC"), r#""\001BC""#);
        assert_eq!(enc("\u{7f}"), r#""\177""#);
        assert_eq!(enc("héllo ✓"), "\"héllo ✓\"");
        assert_eq!(enc("a\u{202E}b"), r#""a\u202Eb""#);
        assert_eq!(enc("\u{85}"), r#""\u0085""#);
        assert_eq!(enc("\u{E0041}"), r#""\U000E0041""#);
        assert_eq!(enc("it's"), r#""it's""#);
        assert_eq!(
            enc(r#""); std::system("x"); //"#),
            r#""\"); std::system(\"x\"); //""#
        );
        assert_eq!(StrLit::new("a\0b"), Err(LiteralError::Nul));
    }

    #[test]
    fn char_literals() {
        assert_eq!(CharLit::new("a").unwrap().to_cpp(), "'a'");
        assert_eq!(CharLit::new("'").unwrap().to_cpp(), r"'\''");
        assert_eq!(CharLit::new("\"").unwrap().to_cpp(), "'\"'");
        assert_eq!(CharLit::new("\n").unwrap().to_cpp(), r"'\n'");
        assert_eq!(CharLit::new("?").unwrap().to_cpp(), "'?'");
        assert_eq!(CharLit::new("é"), Err(LiteralError::NotAscii('é')));
        assert_eq!(CharLit::new("ab"), Err(LiteralError::NotOneChar));
        assert_eq!(CharLit::new(""), Err(LiteralError::NotOneChar));
        assert_eq!(CharLit::new("\0"), Err(LiteralError::Nul));
    }

    #[test]
    fn comments() {
        let lines = |s: &str| Comment::new(s).unwrap().to_cpp_lines();
        assert_eq!(lines("Check the guess"), vec!["// Check the guess"]);
        assert_eq!(lines("a\nb\r\nc\rd"), vec!["// a", "// b", "// c", "// d"]);
        assert_eq!(lines("x\u{2028}y\u{85}z"), vec!["// x", "// y", "// z"]);
        assert_eq!(lines("path C:\\temp\\  "), vec!["// path C:\\temp\\ //"]);
        assert_eq!(lines("bidi \u{202E}evil"), vec!["// bidi <U+202E>evil"]);
        assert_eq!(lines(""), vec!["//"]);
        assert_eq!(lines("*/ ok /*"), vec!["// */ ok /*"]);
        assert_eq!(lines("what??/"), vec!["// what??/ //"]);
    }

    #[test]
    fn numbers() {
        let p = |s: &str, t| NumLit::parse(s, t).map(|n| n.as_str().to_owned());
        assert_eq!(p("42", NumType::Int).as_deref(), Ok("42"));
        assert_eq!(p("0x2A", NumType::Int).as_deref(), Ok("42"));
        assert_eq!(p("0b101010", NumType::Int).as_deref(), Ok("42"));
        assert_eq!(p("1'000'000", NumType::Int).as_deref(), Ok("1000000"));
        assert_eq!(p("2147483647", NumType::Int).as_deref(), Ok("2147483647"));
        assert!(matches!(
            p("2147483648", NumType::Int),
            Err(NumError::OutOfRange { .. })
        ));
        assert_eq!(p("2147483648", NumType::LongLong).as_deref(), Ok("2147483648"));
        for bad in [
            "", "-1", "1.5", "0x", "08", "1''0", "'1", "1e5", "abc", "1u", "0x1G",
        ] {
            assert!(matches!(p(bad, NumType::Int), Err(NumError::Syntax(_))), "{bad}");
        }
        assert_eq!(p("3.14", NumType::Double).as_deref(), Ok("3.14"));
        assert_eq!(p(".5", NumType::Double).as_deref(), Ok("0.5"));
        assert_eq!(p("5.", NumType::Double).as_deref(), Ok("5.0"));
        assert_eq!(p("3", NumType::Double).as_deref(), Ok("3.0"));
        assert_eq!(p("1e-3", NumType::Double).as_deref(), Ok("0.001"));
        assert_eq!(p("2.5E+2", NumType::Double).as_deref(), Ok("250.0"));
        for bad in [".", "e5", "1e", "1.2.3", "1e+", "-2.0", "1.0f", "inf", "nan"] {
            assert!(
                matches!(p(bad, NumType::Double), Err(NumError::Syntax(_))),
                "{bad}"
            );
        }
        assert!(matches!(
            p("1e400", NumType::Double),
            Err(NumError::OutOfRange { .. })
        ));
        assert_eq!(NumLit::int(1).as_str(), "1");
        assert!(NumLit::from_f64(f64::NAN).is_none());
    }

    /// The accessors give back exactly what was validated, and the type names
    /// are the C++ spellings that error messages use.
    #[test]
    fn accessors_return_the_validated_values() {
        let ident = Ident::new("score").unwrap();
        assert_eq!(ident.as_str(), "score");
        assert_eq!(ident.to_string(), "score");
        assert_eq!(format!("[{ident}]"), "[score]");
        assert_eq!(Ident::main().as_str(), "main");
        assert_eq!(StrLit::new("say \"hi\"\n").unwrap().value(), "say \"hi\"\n");
        assert_eq!(CharLit::new("x").unwrap().value(), 'x');
        assert_eq!(Comment::new("a\r\nb */").unwrap().text(), "a\r\nb */");
        let raw = RawCode::new("std::cout << 1;", RawProvenance::CatalogTemplate).unwrap();
        assert_eq!(raw.text(), "std::cout << 1;");
        assert_eq!(raw.provenance(), RawProvenance::CatalogTemplate);

        assert_eq!(NumType::Int.cpp_name(), "int");
        assert_eq!(NumType::LongLong.cpp_name(), "long long");
        assert_eq!(NumType::Double.cpp_name(), "double");
        assert_eq!(
            NumLit::parse("7", NumType::LongLong).unwrap().num_type(),
            NumType::LongLong
        );
        let error = |text: &str, ty| NumLit::parse(text, ty).unwrap_err().to_string();
        assert_eq!(
            error("2147483648", NumType::Int),
            "`2147483648` does not fit in int"
        );
        assert_eq!(
            error("9223372036854775808", NumType::LongLong),
            "`9223372036854775808` does not fit in long long"
        );
        assert_eq!(error("1e400", NumType::Double), "`1e400` does not fit in double");
    }

    /// Deserialising applies the same rules as the constructors: `main` and
    /// `b2c…` names only in their generator forms, nothing else that
    /// [`Ident::new`] refuses.
    #[test]
    fn identifiers_deserialize_with_the_constructor_rules() {
        let load = |name: &str| serde_json::from_value::<Ident>(serde_json::Value::from(name));
        assert_eq!(load("score").unwrap(), Ident::new("score").unwrap());
        assert_eq!(load("main").unwrap(), Ident::main());
        assert_eq!(load("b2c_tmp1").unwrap(), Ident::generated("b2c_tmp1").unwrap());
        for bad in [
            "", "class", "9lives", "a__b", "NULL", "std", "B2C_X", "b2c__x", "naïve",
        ] {
            assert!(load(bad).is_err(), "{bad:?}");
        }
        let ident = Ident::new("total_2").unwrap();
        assert_eq!(serde_json::to_value(&ident).unwrap(), "total_2");

        let lit = |text: &str| serde_json::from_value::<StrLit>(serde_json::Value::from(text));
        assert_eq!(lit("Hi").unwrap().value(), "Hi");
        assert!(lit("a\0b").is_err());
        let ch = |text: &str| serde_json::from_value::<CharLit>(serde_json::Value::from(text));
        assert_eq!(ch("?").unwrap().value(), '?');
        assert!(ch("ab").is_err());
    }

    /// The text limits of 05 §5.6: 64 KiB for string and comment text,
    /// 256 KiB for Raw C++, both inclusive.
    #[test]
    fn text_length_limits_are_exact() {
        const TEXT: usize = 64 * 1024;
        const RAW: usize = 256 * 1024;
        let at = "a".repeat(TEXT);
        let over = "a".repeat(TEXT + 1);
        assert_eq!(StrLit::new(&at).unwrap().value().len(), TEXT);
        assert_eq!(StrLit::new(&over), Err(LiteralError::TooLong));
        assert_eq!(Comment::new(&at).unwrap().text().len(), TEXT);
        assert_eq!(Comment::new(&over), Err(LiteralError::TooLong));
        // A multi-byte character counts by its bytes.
        let wide_over = format!("{}é", "a".repeat(TEXT - 1));
        assert_eq!(StrLit::new(&wide_over), Err(LiteralError::TooLong));

        let raw = |len: usize| RawCode::new(&"a".repeat(len), RawProvenance::RawBlock);
        assert_eq!(raw(RAW).unwrap().text().len(), RAW);
        assert_eq!(raw(RAW + 1), Err(LiteralError::TooLong));
        assert_eq!(
            RawCode::new("a\0b", RawProvenance::RawBlock),
            Err(LiteralError::Nul)
        );
    }

    /// Every range of [`INVISIBLE_RANGES`] is found from its first to its last
    /// code point, and the code points just outside it are not (they are
    /// ordinary characters: no two ranges touch).
    #[test]
    fn invisible_ranges_are_found_at_both_ends() {
        let char_at = |cp: u32| char::from_u32(cp);
        for &(lo, hi) in INVISIBLE_RANGES {
            for cp in [lo, hi] {
                let c = char_at(cp).unwrap();
                assert!(is_invisible(c), "U+{cp:04X}");
            }
            for cp in [lo - 1, hi + 1] {
                if let Some(c) = char_at(cp) {
                    assert!(!is_invisible(c), "U+{cp:04X}");
                }
            }
        }
        // Some of the ends spelled out: the first C1 control, the soft hyphen,
        // the Arabic letter mark, the zero-width space, the right-to-left
        // override (Trojan Source) and the word joiner.
        for c in [
            '\u{80}',
            '\u{9F}',
            '\u{AD}',
            '\u{61C}',
            '\u{200B}',
            '\u{202E}',
            '\u{2060}',
            '\u{10FFFF}',
        ] {
            assert!(is_invisible(c), "{c:?}");
        }
        for c in [
            '\0', 'a', '\u{7F}', '\u{A0}', '\u{AC}', '\u{AE}', '\u{2065}', 'é', '✓',
        ] {
            assert!(!is_invisible(c), "{c:?}");
        }
    }

    /// Integer and literal-length limits, at and one past each bound.
    #[test]
    fn numeric_limits_are_exact() {
        let p = |s: &str, t| NumLit::parse(s, t).map(|n| n.as_str().to_owned());
        let out_of_range = |s: &str, t| matches!(NumLit::parse(s, t), Err(NumError::OutOfRange { .. }));
        let syntax = |s: &str, t| matches!(NumLit::parse(s, t), Err(NumError::Syntax(_)));

        assert_eq!(p("0", NumType::Int).as_deref(), Ok("0"));
        assert_eq!(p("0", NumType::Double).as_deref(), Ok("0.0"));
        assert_eq!(p("0x7FFFFFFF", NumType::Int).as_deref(), Ok("2147483647"));
        assert!(out_of_range("0x80000000", NumType::Int));
        assert_eq!(
            p("9223372036854775807", NumType::LongLong).as_deref(),
            Ok("9223372036854775807")
        );
        assert!(out_of_range("9223372036854775808", NumType::LongLong));

        // At most 128 digits after the base prefix (leading zeros count).
        let binary = |digits: usize| format!("0b{}1", "0".repeat(digits - 1));
        assert_eq!(p(&binary(128), NumType::Int).as_deref(), Ok("1"));
        assert!(syntax(&binary(129), NumType::Int));
        let hex = |digits: usize| format!("0x{}F", "0".repeat(digits - 1));
        assert_eq!(p(&hex(128), NumType::LongLong).as_deref(), Ok("15"));
        assert!(syntax(&hex(129), NumType::LongLong));

        // At most 400 bytes of literal text.
        let zeros = |len: usize| format!("0.{}", "0".repeat(len - 2));
        assert_eq!(p(&zeros(400), NumType::Double).as_deref(), Ok("0.0"));
        assert!(syntax(&zeros(401), NumType::Double));
    }

    /// Literals the generator asks for: a negative value gives the literal of
    /// its magnitude (the caller adds the unary minus).
    #[test]
    fn generator_integer_literals() {
        for (value, text) in [
            (0, "0"),
            (1, "1"),
            (42, "42"),
            (-1, "1"),
            (-42, "42"),
            (i32::MAX, "2147483647"),
            (i32::MIN, "2147483648"),
        ] {
            let lit = NumLit::int(value);
            assert_eq!(lit.as_str(), text, "{value}");
            assert_eq!(lit.num_type(), NumType::Int);
        }
    }

    /// The decimal floating-point grammar on its own: Rust's `f64` parser
    /// would refuse some of these anyway, but the grammar must not rely on it.
    #[test]
    fn decimal_float_grammar() {
        for good in ["1.5", "1.", ".5", "1e5", "1E+5", "1.5e-10", "0.0"] {
            assert!(is_decimal_float(good), "{good}");
        }
        for bad in [
            "", ".", "1", "e5", "1e", "1e+", "1e-", "1ex", "1e5x", "1.e", "1.5x", "x.5", "1.2.3", "+1.5",
        ] {
            assert!(!is_decimal_float(bad), "{bad}");
        }
    }

    /// A digit separator must stand between two digits, as in C++ (found by
    /// the `encoders` fuzz target: `.'02` was accepted as `0.02`).
    #[test]
    fn digit_separators_stand_between_digits() {
        let p = |s: &str, t| NumLit::parse(s, t).map(|n| n.as_str().to_owned());
        for (good, ty, value) in [
            ("1'000", NumType::Int, "1000"),
            ("0x1'e", NumType::Int, "30"),
            ("0b1'0", NumType::Int, "2"),
            ("1'0", NumType::Double, "10.0"),
            ("1'000.5", NumType::Double, "1000.5"),
            ("1.0'5", NumType::Double, "1.05"),
            (".5'5", NumType::Double, "0.55"),
            ("1e1'0", NumType::Double, "10000000000.0"),
        ] {
            assert_eq!(p(good, ty).as_deref(), Ok(value), "{good}");
        }
        for bad in ["0x'1", "0X'F", "0b'1", "0'x1", "0x1'", "1'a", "0'1"] {
            assert!(matches!(p(bad, NumType::Int), Err(NumError::Syntax(_))), "{bad}");
        }
        for bad in [
            ".'02", "1'.5", "1.'5", "1'e5", "1e'5", "1e+'5", "1.5'", "0x'1", "1'.",
        ] {
            assert!(
                matches!(p(bad, NumType::Double), Err(NumError::Syntax(_))),
                "{bad}"
            );
        }
    }

    proptest! {
        #[test]
        fn string_literal_has_no_raw_controls_or_unescaped_quotes(s in "\\PC*|[\\x01-\\x7f]*") {
            if let Ok(lit) = StrLit::new(&s) {
                let enc = lit.to_cpp();
                let inner = &enc[1..enc.len() - 1];
                prop_assert!(!inner.chars().any(|c| c.is_control() || is_invisible(c)));
                // Every quote inside is escaped: removing escape pairs leaves no `"`.
                let mut chars = inner.chars();
                while let Some(c) = chars.next() {
                    if c == '\\' { chars.next(); } else { prop_assert_ne!(c, '"'); }
                }
            }
        }

        #[test]
        fn comment_lines_never_end_in_backslash(s in "\\PC*|[\\x00-\\x7f]*") {
            for line in Comment::new(&s).unwrap().to_cpp_lines() {
                prop_assert!(line.starts_with("//"));
                prop_assert!(!line.trim_end().ends_with('\\'));
                prop_assert!(!line.trim_end().ends_with("??/"));
                prop_assert!(!line.chars().any(|c| (c.is_control() && c != '\t') || is_invisible(c)));
            }
        }

        #[test]
        fn valid_identifiers_match_the_shape(s in "[A-Za-z_][A-Za-z0-9_]{0,70}") {
            if let Ok(id) = Ident::new(&s) {
                prop_assert!(id.as_str().len() <= MAX_IDENT_LEN);
                prop_assert!(!id.as_str().contains("__"));
                prop_assert!(id.as_str().as_bytes()[0].is_ascii_alphabetic());
            }
        }

        #[test]
        fn integers_round_trip(v in 0i32..=i32::MAX) {
            let lit = NumLit::parse(&v.to_string(), NumType::Int).unwrap();
            prop_assert_eq!(lit.as_str(), v.to_string());
        }

        #[test]
        #[allow(clippy::float_cmp)] // exact round-trip is the property under test
        fn doubles_round_trip(v in 0.0f64..1e300) {
            let lit = NumLit::from_f64(v).unwrap();
            prop_assert_eq!(lit.as_str().parse::<f64>().unwrap(), v);
            prop_assert!(lit.as_str().contains('.') || lit.as_str().contains('e'));
        }
    }
}
