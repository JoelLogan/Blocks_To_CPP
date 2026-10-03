//! A strict JSON parser for untrusted project bytes (spec §5.6, §6.2).
//!
//! Why not `serde_json::Value`: the loader needs things `serde_json` does not
//! offer, all decided *while* parsing so that hostile input is cut short:
//!
//! * duplicate keys are found in every object (many parsers silently keep the
//!   last value, which allows ambiguity attacks);
//! * nesting is limited to exactly [`MAX_JSON_DEPTH`] arrays/objects (the
//!   built-in `serde_json` limit is one lower and cannot be configured);
//! * the number of values is limited ([`MAX_JSON_VALUES`]), and objects are
//!   stored as compact slices instead of B-tree maps (a one-key
//!   `serde_json::Map` allocates a ~600-byte node), so a 32 MiB file cannot
//!   blow up into gigabytes of memory;
//! * every error has a byte offset that becomes a line and column.
//!
//! Numbers become `serde_json::Number`s with the same rules as in
//! `serde_json::Value` (unsigned, then signed 64-bit integers, otherwise
//! `f64`), but floats are parsed with correct rounding.

#![deny(clippy::indexing_slicing)]

use serde_json::Number;

use crate::limits::MAX_JSON_DEPTH;

/// Maximum number of JSON values (scalars, arrays and objects) in one input.
///
/// A canonical (indented) project needs well over 8 bytes per value, so a
/// file within [`crate::limits::MAX_FILE_BYTES`] stays far below this; only
/// adversarial input such as `[0,0,0,…]` reaches it. It bounds the memory of
/// the parse tree to roughly 100 MiB.
pub(crate) const MAX_JSON_VALUES: usize = 4 * 1024 * 1024;

/// How many duplicate keys are recorded; the rest are only counted.
const MAX_RECORDED_DUPLICATES: usize = 256;

/// Objects up to this size are checked for duplicate keys pairwise.
const PAIRWISE_LIMIT: usize = 16;

/// A parsed JSON value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Json {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number, with `serde_json`'s meaning.
    Number(Number),
    /// A string (escapes already decoded).
    String(Box<str>),
    /// An array.
    Array(Box<[Json]>),
    /// An object, in file order (keys are unique when parsing succeeded
    /// without duplicates).
    Object(Box<[(Box<str>, Json)]>),
}

impl Json {
    /// A plain-English description of the kind of value, for messages.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "true/false",
            Self::Number(_) => "a number",
            Self::String(_) => "text",
            Self::Array(_) => "a list",
            Self::Object(_) => "an object",
        }
    }

    /// The value of `key` when this is an object that has it.
    pub(crate) fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(entries) => entries.iter().find(|(k, _)| &**k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Sets `key` to `value` in an object, replacing an existing entry or
    /// appending a new one. Does nothing to other kinds of values.
    pub(crate) fn set(&mut self, key: &str, value: Self) {
        if let Self::Object(entries) = self {
            if let Some(slot) = entries.iter_mut().find(|(k, _)| &**k == key) {
                slot.1 = value;
            } else {
                let mut list = std::mem::take(entries).into_vec();
                list.push((key.into(), value));
                *entries = list.into_boxed_slice();
            }
        }
    }

    /// Removes `key` from an object and returns its value.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "for format migrations; none exist for version 1 yet")
    )]
    pub(crate) fn remove(&mut self, key: &str) -> Option<Self> {
        let Self::Object(entries) = self else {
            return None;
        };
        let index = entries.iter().position(|(k, _)| &**k == key)?;
        let mut list = std::mem::take(entries).into_vec();
        let (_, value) = list.remove(index);
        *entries = list.into_boxed_slice();
        Some(value)
    }

    /// Converts to a `serde_json::Value` (recursion is bounded by the depth
    /// limit the parser enforced).
    pub(crate) fn to_value(&self) -> serde_json::Value {
        match self {
            Self::Null => serde_json::Value::Null,
            Self::Bool(b) => serde_json::Value::Bool(*b),
            Self::Number(n) => serde_json::Value::Number(n.clone()),
            Self::String(s) => serde_json::Value::String(s.to_string()),
            Self::Array(items) => serde_json::Value::Array(items.iter().map(Self::to_value).collect()),
            Self::Object(entries) => serde_json::Value::Object(
                entries
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_value()))
                    .collect(),
            ),
        }
    }
}

/// What went wrong while parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErrorKind {
    /// Something else was expected here (the text says what).
    Expected(&'static str),
    /// A string has no closing quote.
    UnterminatedString,
    /// A raw control character (U+0000–U+001F) inside a string.
    ControlInString,
    /// A backslash escape other than `\" \\ \/ \b \f \n \r \t \uXXXX`.
    InvalidEscape,
    /// `\u` not followed by four hexadecimal digits.
    InvalidUnicodeEscape,
    /// A UTF-16 surrogate escape without its partner.
    LoneSurrogate,
    /// A malformed number.
    InvalidNumber,
    /// A number too large to represent.
    NumberOutOfRange,
    /// More content after the end of the top-level value.
    TrailingData,
    /// Arrays/objects nested deeper than [`MAX_JSON_DEPTH`].
    TooDeep,
    /// More than [`MAX_JSON_VALUES`] values.
    TooManyValues,
}

/// A parse error at a byte offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParseError {
    /// What went wrong.
    pub(crate) kind: ErrorKind,
    /// Byte offset in the input.
    pub(crate) offset: usize,
}

/// A key that appears more than once in the same object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Duplicate {
    /// The key.
    pub(crate) key: Box<str>,
    /// Byte offset of the repeated occurrence.
    pub(crate) offset: usize,
}

/// A successful parse.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Parsed {
    /// The value.
    pub(crate) value: Json,
    /// Repeated keys, in file order (at most [`MAX_RECORDED_DUPLICATES`]).
    pub(crate) duplicates: Vec<Duplicate>,
    /// Repeated keys found beyond those recorded.
    pub(crate) unrecorded_duplicates: usize,
}

/// Parses one JSON value that makes up the whole text.
pub(crate) fn parse(text: &str) -> Result<Parsed, ParseError> {
    let mut parser = Parser {
        text,
        bytes: text.as_bytes(),
        pos: 0,
        values: 0,
        duplicates: Vec::new(),
        unrecorded_duplicates: 0,
    };
    parser.skip_whitespace();
    let value = parser.value(0)?;
    parser.skip_whitespace();
    if parser.pos < parser.bytes.len() {
        return Err(parser.error(ErrorKind::TrailingData));
    }
    let mut duplicates = parser.duplicates;
    duplicates.sort_by_key(|d| d.offset);
    Ok(Parsed {
        value,
        duplicates,
        unrecorded_duplicates: parser.unrecorded_duplicates,
    })
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
    values: usize,
    duplicates: Vec<Duplicate>,
    unrecorded_duplicates: usize,
}

impl<'a> Parser<'a> {
    fn error(&self, kind: ErrorKind) -> ParseError {
        ParseError {
            kind,
            offset: self.pos,
        }
    }

    fn error_at(kind: ErrorKind, offset: usize) -> ParseError {
        ParseError { kind, offset }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    /// The text between two offsets that lie on ASCII bytes (or the end).
    fn slice(&self, start: usize, end: usize) -> Result<&'a str, ParseError> {
        self.text
            .get(start..end)
            .ok_or_else(|| Self::error_at(ErrorKind::InvalidNumber, start))
    }

    /// Parses a value; `depth` is the number of arrays/objects around it.
    fn value(&mut self, depth: usize) -> Result<Json, ParseError> {
        self.values += 1;
        if self.values > MAX_JSON_VALUES {
            return Err(self.error(ErrorKind::TooManyValues));
        }
        match self.peek() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => Ok(Json::String(self.string()?)),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => Ok(Json::Number(self.number()?)),
            _ => Err(self.error(ErrorKind::Expected("a value"))),
        }
    }

    fn literal(&mut self, word: &'static str, value: Json) -> Result<Json, ParseError> {
        let end = self.pos + word.len();
        if self.bytes.get(self.pos..end) == Some(word.as_bytes()) {
            self.pos = end;
            Ok(value)
        } else {
            Err(self.error(ErrorKind::Expected("a value")))
        }
    }

    fn enter(&self, level: usize) -> Result<(), ParseError> {
        if level > MAX_JSON_DEPTH {
            Err(self.error(ErrorKind::TooDeep))
        } else {
            Ok(())
        }
    }

    fn array(&mut self, level: usize) -> Result<Json, ParseError> {
        self.enter(level)?;
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Array(Box::default()));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value(level)?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Array(items.into_boxed_slice()));
                }
                _ => return Err(self.error(ErrorKind::Expected("',' or ']'"))),
            }
        }
    }

    fn object(&mut self, level: usize) -> Result<Json, ParseError> {
        self.enter(level)?;
        self.pos += 1;
        let mut entries: Vec<(Box<str>, Json)> = Vec::new();
        let mut key_offsets: Vec<usize> = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Object(Box::default()));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'"') {
                return Err(self.error(ErrorKind::Expected("a key in double quotes")));
            }
            let key_offset = self.pos;
            let key = self.string()?;
            self.skip_whitespace();
            if self.peek() != Some(b':') {
                return Err(self.error(ErrorKind::Expected("':' after the key")));
            }
            self.pos += 1;
            self.skip_whitespace();
            let value = self.value(level)?;
            entries.push((key, value));
            key_offsets.push(key_offset);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(self.error(ErrorKind::Expected("',' or '}'"))),
            }
        }
        self.find_duplicates(&entries, &key_offsets);
        Ok(Json::Object(entries.into_boxed_slice()))
    }

    fn record_duplicate(&mut self, key: &str, offset: usize) {
        if self.duplicates.len() < MAX_RECORDED_DUPLICATES {
            self.duplicates.push(Duplicate {
                key: key.into(),
                offset,
            });
        } else {
            self.unrecorded_duplicates += 1;
        }
    }

    /// Records every key that repeats an earlier key of the same object.
    fn find_duplicates(&mut self, entries: &[(Box<str>, Json)], offsets: &[usize]) {
        let keyed = entries.iter().map(|(k, _)| &**k).zip(offsets.iter().copied());
        if entries.len() <= PAIRWISE_LIMIT {
            let keys: Vec<(&str, usize)> = keyed.collect();
            for (index, (key, offset)) in keys.iter().enumerate() {
                if keys.iter().take(index).any(|(earlier, _)| earlier == key) {
                    self.record_duplicate(key, *offset);
                }
            }
        } else {
            let mut sorted: Vec<(&str, usize)> = keyed.collect();
            // Offsets increase through the object, so equal keys stay in
            // file order and every element after the first is a repeat.
            sorted.sort_unstable();
            let mut repeats: Vec<(&str, usize)> = sorted
                .windows(2)
                .filter_map(|pair| match pair {
                    [(a, _), (b, offset)] if a == b => Some((*b, *offset)),
                    _ => None,
                })
                .collect();
            repeats.sort_by_key(|(_, offset)| *offset);
            for (key, offset) in repeats {
                self.record_duplicate(key, offset);
            }
        }
    }

    fn string(&mut self) -> Result<Box<str>, ParseError> {
        let open = self.pos;
        self.pos += 1;
        let mut out: Option<String> = None;
        loop {
            let run_start = self.pos;
            while let Some(&byte) = self.bytes.get(self.pos) {
                if byte == b'"' || byte == b'\\' || byte < 0x20 {
                    break;
                }
                self.pos += 1;
            }
            let run = self.slice(run_start, self.pos)?;
            match self.peek() {
                None => return Err(Self::error_at(ErrorKind::UnterminatedString, open)),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(match out {
                        None => run.into(),
                        Some(mut text) => {
                            text.push_str(run);
                            text.into_boxed_str()
                        }
                    });
                }
                Some(b'\\') => {
                    let text = out.get_or_insert_with(String::new);
                    text.push_str(run);
                    self.escape(text)?;
                }
                Some(_) => return Err(self.error(ErrorKind::ControlInString)),
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), ParseError> {
        let at = self.pos;
        self.pos += 1;
        let Some(code) = self.peek() else {
            return Err(Self::error_at(ErrorKind::UnterminatedString, at));
        };
        self.pos += 1;
        let c = match code {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => self.unicode_escape(at)?,
            _ => return Err(Self::error_at(ErrorKind::InvalidEscape, at)),
        };
        out.push(c);
        Ok(())
    }

    /// Decodes the rest of a `\uXXXX` escape (and a following low surrogate).
    fn unicode_escape(&mut self, at: usize) -> Result<char, ParseError> {
        let first = self.hex4(at)?;
        let code = match first {
            0xD800..=0xDBFF => {
                if self.bytes.get(self.pos..self.pos + 2) != Some(b"\\u") {
                    return Err(Self::error_at(ErrorKind::LoneSurrogate, at));
                }
                self.pos += 2;
                let second = self.hex4(at)?;
                if !(0xDC00..=0xDFFF).contains(&second) {
                    return Err(Self::error_at(ErrorKind::LoneSurrogate, at));
                }
                0x1_0000 + ((first - 0xD800) << 10) + (second - 0xDC00)
            }
            0xDC00..=0xDFFF => return Err(Self::error_at(ErrorKind::LoneSurrogate, at)),
            _ => first,
        };
        char::from_u32(code).ok_or_else(|| Self::error_at(ErrorKind::LoneSurrogate, at))
    }

    fn hex4(&mut self, at: usize) -> Result<u32, ParseError> {
        let digits = self
            .bytes
            .get(self.pos..self.pos + 4)
            .ok_or_else(|| Self::error_at(ErrorKind::InvalidUnicodeEscape, at))?;
        let mut value = 0;
        for &digit in digits {
            let nibble = char::from(digit)
                .to_digit(16)
                .ok_or_else(|| Self::error_at(ErrorKind::InvalidUnicodeEscape, at))?;
            value = value * 16 + nibble;
        }
        self.pos += 4;
        Ok(value)
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.pos - start
    }

    fn number(&mut self) -> Result<Number, ParseError> {
        let start = self.pos;
        let invalid = || Self::error_at(ErrorKind::InvalidNumber, start);
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(invalid()),
        }
        let mut integer = true;
        if self.peek() == Some(b'.') {
            integer = false;
            self.pos += 1;
            if self.digits() == 0 {
                return Err(invalid());
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            integer = false;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return Err(invalid());
            }
        }
        let token = self.slice(start, self.pos)?;
        if integer {
            if let Ok(value) = token.parse::<u64>() {
                return Ok(Number::from(value));
            }
            // `-0` is the float -0.0 in serde_json; it is handled below.
            if let Ok(value) = token.parse::<i64>()
                && value != 0
            {
                return Ok(Number::from(value));
            }
        }
        // Like serde_json, everything else (fractions, exponents, integers
        // beyond 64 bits) is an f64. Rust's parser rounds correctly, so the
        // shortest form that saving writes reads back as the same value
        // (serde_json's own parser can be one unit in the last place off
        // unless its `float_roundtrip` feature is enabled).
        token
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .ok_or_else(|| Self::error_at(ErrorKind::NumberOutOfRange, start))
    }
}

/// Converts byte offsets to 1-based (line, column) pairs, where the column
/// counts characters. Runs in one pass over the text, however many offsets.
pub(crate) fn line_columns(text: &str, offsets: &[usize]) -> Vec<(usize, usize)> {
    let mut order: Vec<usize> = (0..offsets.len()).collect();
    order.sort_by_key(|&i| offsets.get(i).copied().unwrap_or(0));
    let mut result = vec![(1, 1); offsets.len()];
    let mut chars = text.char_indices().peekable();
    let (mut line, mut column) = (1, 1);
    for index in order {
        let target = offsets.get(index).copied().unwrap_or(0);
        while let Some(&(at, c)) = chars.peek() {
            if at >= target {
                break;
            }
            chars.next();
            if c == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        if let Some(slot) = result.get_mut(index) {
            *slot = (line, column);
        }
    }
    result
}

/// Describes the character at `offset` for a message: `'x'` for visible
/// ASCII, `U+XXXX` for anything else, or "the end of the file".
pub(crate) fn describe_at(text: &str, offset: usize) -> String {
    match text.get(offset..).and_then(|rest| rest.chars().next()) {
        None => String::from("the end of the file"),
        Some(c) if c.is_ascii_graphic() => format!("'{c}'"),
        Some(' ') => String::from("a space"),
        Some(c) => format!("U+{:04X}", u32::from(c)),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing)]

    use super::*;

    fn ok(text: &str) -> Json {
        let parsed = parse(text).unwrap();
        assert!(parsed.duplicates.is_empty(), "{text}");
        parsed.value
    }

    fn err(text: &str) -> ParseError {
        parse(text).unwrap_err()
    }

    #[test]
    fn scalars() {
        assert_eq!(ok("null"), Json::Null);
        assert_eq!(ok(" true "), Json::Bool(true));
        assert_eq!(ok("false"), Json::Bool(false));
        assert_eq!(ok("\"a\""), Json::String("a".into()));
        assert_eq!(ok("0"), Json::Number(0u64.into()));
        assert_eq!(ok("-12"), Json::Number((-12i64).into()));
        assert_eq!(ok("18446744073709551615"), Json::Number(u64::MAX.into()));
        assert_eq!(ok("1.5e2"), Json::Number(Number::from_f64(150.0).unwrap()));
    }

    #[test]
    fn numbers_match_serde_json() {
        for text in [
            "-0",
            "-0.0",
            "0.1",
            "1E5",
            "1e-5",
            "-9223372036854775808",
            "-9223372036854775809",
            "18446744073709551616",
            "123456789012345678901234567890",
            "2.2250738585072014e-308",
            "4.9e-324",
            "1.7976931348623157e308",
        ] {
            let expected: serde_json::Value = serde_json::from_str(text).unwrap();
            assert_eq!(ok(text).to_value(), expected, "{text}");
        }
    }

    #[test]
    fn invalid_numbers() {
        for text in ["01", "-", "1.", ".5", "1e", "1e+", "+1", "0x10", "1.5.2", "--1"] {
            assert!(parse(text).is_err(), "{text}");
        }
        assert_eq!(err("1e400").kind, ErrorKind::NumberOutOfRange);
        assert_eq!(err("-1e400").kind, ErrorKind::NumberOutOfRange);
    }

    #[test]
    fn strings_and_escapes() {
        assert_eq!(
            ok(r#""a\"b\\c\/d\b\f\n\r\t""#),
            Json::String("a\"b\\c/d\u{8}\u{c}\n\r\t".into())
        );
        assert_eq!(ok(r#""é\u0000""#), Json::String("é\0".into()));
        assert_eq!(ok(r#""😀""#), Json::String("😀".into()));
        assert_eq!(ok("\"héllo ✓\""), Json::String("héllo ✓".into()));
        assert_eq!(err(r#""\ud83d""#).kind, ErrorKind::LoneSurrogate);
        assert_eq!(err(r#""\ud83dA""#).kind, ErrorKind::LoneSurrogate);
        assert_eq!(err(r#""\ude00""#).kind, ErrorKind::LoneSurrogate);
        assert_eq!(err(r#""\u12""#).kind, ErrorKind::InvalidUnicodeEscape);
        assert_eq!(err(r#""\u12g4""#).kind, ErrorKind::InvalidUnicodeEscape);
        assert_eq!(err(r#""\x41""#).kind, ErrorKind::InvalidEscape);
        assert_eq!(err("\"a\nb\"").kind, ErrorKind::ControlInString);
        assert_eq!(err("\"a\tb\"").kind, ErrorKind::ControlInString);
        assert_eq!(err("\"abc").kind, ErrorKind::UnterminatedString);
        assert_eq!(err("\"abc\\").kind, ErrorKind::UnterminatedString);
    }

    #[test]
    fn structure_errors() {
        assert_eq!(err("").kind, ErrorKind::Expected("a value"));
        assert_eq!(err("[1,]").kind, ErrorKind::Expected("a value"));
        assert_eq!(err("[1 2]").kind, ErrorKind::Expected("',' or ']'"));
        assert_eq!(err("{\"a\" 1}").kind, ErrorKind::Expected("':' after the key"));
        assert_eq!(
            err("{\"a\":1,}").kind,
            ErrorKind::Expected("a key in double quotes")
        );
        assert_eq!(err("{a:1}").kind, ErrorKind::Expected("a key in double quotes"));
        assert_eq!(err("{\"a\":1 \"b\":2}").kind, ErrorKind::Expected("',' or '}'"));
        assert_eq!(err("{} {}").kind, ErrorKind::TrailingData);
        assert_eq!(err("tru").kind, ErrorKind::Expected("a value"));
        assert_eq!(err("nul").kind, ErrorKind::Expected("a value"));
        assert_eq!(err("\u{feff}{}").kind, ErrorKind::Expected("a value"));
        assert_eq!(err("// comment\n{}").kind, ErrorKind::Expected("a value"));
    }

    #[test]
    fn depth_limit_is_exact() {
        let nested = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
        assert!(parse(&nested(MAX_JSON_DEPTH)).is_ok());
        let error = err(&nested(MAX_JSON_DEPTH + 1));
        assert_eq!(error.kind, ErrorKind::TooDeep);
        assert_eq!(error.offset, MAX_JSON_DEPTH);
        let objects = |n: usize| format!("{}1{}", "{\"a\":".repeat(n), "}".repeat(n));
        assert!(parse(&objects(MAX_JSON_DEPTH)).is_ok());
        assert_eq!(err(&objects(MAX_JSON_DEPTH + 1)).kind, ErrorKind::TooDeep);
        // Far deeper input fails fast instead of overflowing the stack.
        assert_eq!(err(&"[".repeat(1_000_000)).kind, ErrorKind::TooDeep);
    }

    #[test]
    fn value_limit() {
        let mut text = String::from("[");
        text.push_str(&"0,".repeat(MAX_JSON_VALUES - 2));
        text.push_str("0]");
        assert!(parse(&text).is_ok());
        text.insert_str(1, "0,");
        assert_eq!(err(&text).kind, ErrorKind::TooManyValues);
    }

    #[test]
    fn duplicates_small_and_large_objects() {
        let parsed = parse(r#"{"a":1,"b":{"x":1,"x":2},"a":3}"#).unwrap();
        let keys: Vec<&str> = parsed.duplicates.iter().map(|d| &*d.key).collect();
        assert_eq!(keys, ["x", "a"]);
        assert_eq!(parsed.duplicates[1].offset, 25);

        let mut members: Vec<String> = (0..40).map(|i| format!("\"k{i}\":{i}")).collect();
        members.push("\"k7\":0".into());
        members.push("\"k3\":0".into());
        members.push("\"k7\":0".into());
        let parsed = parse(&format!("{{{}}}", members.join(","))).unwrap();
        let keys: Vec<&str> = parsed.duplicates.iter().map(|d| &*d.key).collect();
        assert_eq!(keys, ["k7", "k3", "k7"]);
        // Escaped spellings of the same key are the same key.
        let parsed = parse(r#"{"a":1,"a":2}"#).unwrap();
        assert_eq!(parsed.duplicates.len(), 1);
    }

    #[test]
    fn duplicate_recording_is_capped() {
        let members: Vec<String> = (0..MAX_RECORDED_DUPLICATES + 11)
            .map(|i| format!("\"k{i}\":{{\"a\":1,\"a\":2}}"))
            .collect();
        let parsed = parse(&format!("{{{}}}", members.join(","))).unwrap();
        assert_eq!(parsed.duplicates.len(), MAX_RECORDED_DUPLICATES);
        assert_eq!(parsed.unrecorded_duplicates, 11);
    }

    #[test]
    fn object_editing() {
        let mut value = ok(r#"{"a":1,"b":2}"#);
        value.set("a", Json::Bool(true));
        value.set("c", Json::Null);
        assert_eq!(value.remove("b"), Some(Json::Number(2u64.into())));
        assert_eq!(value.remove("zz"), None);
        assert_eq!(value, ok(r#"{"a":true,"c":null}"#));
        assert_eq!(value.get("c"), Some(&Json::Null));
        let mut scalar = Json::Null;
        scalar.set("a", Json::Null);
        assert_eq!(scalar, Json::Null);
    }

    fn arbitrary_value() -> impl proptest::strategy::Strategy<Value = serde_json::Value> {
        use proptest::prelude::*;
        let leaf = prop_oneof![
            Just(serde_json::Value::Null),
            any::<bool>().prop_map(serde_json::Value::Bool),
            any::<i64>().prop_map(serde_json::Value::from),
            any::<u64>().prop_map(serde_json::Value::from),
            any::<f64>()
                .prop_filter("finite", |x| x.is_finite())
                .prop_map(serde_json::Value::from),
            any::<String>().prop_map(serde_json::Value::String),
        ];
        leaf.prop_recursive(4, 32, 5, |inner| {
            prop_oneof![
                proptest::collection::vec(inner.clone(), 0..5).prop_map(serde_json::Value::Array),
                proptest::collection::btree_map(any::<String>(), inner, 0..5)
                    .prop_map(|map| serde_json::Value::Object(map.into_iter().collect())),
            ]
        })
    }

    proptest::proptest! {
        /// Whatever serde_json writes, this parser reads back identically.
        #[test]
        fn reads_what_serde_json_writes(value in arbitrary_value()) {
            for text in [serde_json::to_string(&value).unwrap(), serde_json::to_string_pretty(&value).unwrap()] {
                let parsed = parse(&text).unwrap();
                proptest::prop_assert!(parsed.duplicates.is_empty());
                proptest::prop_assert_eq!(parsed.value.to_value(), value.clone());
            }
        }

        /// On JSON-like noise, this parser and serde_json agree on what is
        /// valid and on the value (serde_json silently keeps the last of
        /// duplicate keys, which this parser reports instead).
        #[test]
        fn agrees_with_serde_json_on_noise(text in r#"[\[\]{}":,0-9a-z\\ .eE+-]{0,48}"#) {
            match (parse(&text), serde_json::from_str::<serde_json::Value>(&text)) {
                (Ok(parsed), Ok(value)) => {
                    if parsed.duplicates.is_empty() {
                        proptest::prop_assert_eq!(parsed.value.to_value(), value);
                    }
                }
                (Err(_), Err(_)) => {}
                (mine, theirs) => proptest::prop_assert!(false, "{text:?}: {mine:?} vs {theirs:?}"),
            }
        }

        #[test]
        fn never_panics(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..256)) {
            if let Ok(text) = std::str::from_utf8(&bytes) {
                let _ = parse(text);
            }
        }
    }

    #[test]
    fn positions() {
        let text = "{\n  \"é\": [1,\n x]\n}";
        let error = err(text);
        assert_eq!(line_columns(text, &[error.offset]), [(3, 2)]);
        assert_eq!(line_columns(text, &[text.len(), 0, 4]), [(4, 2), (1, 1), (2, 3)]);
        assert_eq!(describe_at(text, error.offset), "'x'");
        assert_eq!(describe_at(text, text.len()), "the end of the file");
        assert_eq!(describe_at("\u{202e}", 0), "U+202E");
        assert_eq!(describe_at(" ", 0), "a space");
    }
}
