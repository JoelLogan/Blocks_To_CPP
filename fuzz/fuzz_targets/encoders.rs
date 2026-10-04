//! Fuzzes the typed text encoders of `b2c_ir::text`, the only path from user
//! text to generated C++ (docs/spec/08-security.md §8.4). Whatever a
//! validated constructor accepts must encode to C++ that means exactly that
//! value and can neither break out of its context nor hide text:
//!
//! * `StrLit` / `CharLit` (§8.4.2): an independent C++ escape decoder must
//!   read the emitted literal back to exactly the value's bytes; no raw line
//!   break, control, invisible or bidi character, no quote that ends the
//!   literal early, no `??` (trigraph) and only the escapes of the spec's
//!   table. A `char` literal is printable ASCII holding one character.
//! * `Comment` (§8.4.3): one `//` line per source line, each sanitised
//!   exactly as documented, with no line break, control (other than tab),
//!   invisible or bidi character, no trailing whitespace and no trailing `\`
//!   or `??/` that would splice the next line into the comment.
//! * `Ident` (§8.4.1): accepted exactly when the spec's rules hold, checked
//!   against an independent keyword list, the macro names the spec lists and
//!   the reserved-name tables (looked up without the crate's binary search);
//!   every rejection names a rule the name really breaks, and
//!   `check_namespace_scope` agrees with the global-name table.
//! * `NumLit` (§8.4.4): `parse` accepts only the C++ literal grammar (with
//!   digit separators only between digits) in range for the type, rejects
//!   nothing valid that fits, and prints a suffix-free literal of the right
//!   kind that parses back to the same value; `from_f64` and `int` too.
//!
//! Input: one mode byte, then the text. Mode bit 0 reads the text as UTF-8
//! (invalid bytes become U+FFFD) or, when set, as little-endian 32-bit code
//! points, which reaches every plane evenly. Mode bits 1–3, all set, repeat
//! the text to between 8 bytes under and 7 over `MAX_TEXT_LEN` (chosen by
//! `mode >> 4`) to test the length limits. Otherwise the whole text and each
//! of its characters and words are checked.

#![no_main]

use std::collections::HashSet;
use std::fmt::Write as _;
use std::sync::LazyLock;

use b2c_ir::text::{
    CharLit, Comment, Ident, IdentError, LiteralError, MAX_IDENT_LEN, MAX_TEXT_LEN, NumError, NumLit,
    NumType, StrLit, is_invisible,
};
use libfuzzer_sys::fuzz_target;

// The generated reserved-name tables of b2c-ir (private to that crate),
// compiled into this target so the oracle can look names up in hash sets
// instead of trusting the binary search (and the sort order) the crate uses.
#[path = "../../crates/b2c-ir/src/reserved_names.rs"]
mod reserved_names;

use reserved_names::{GLOBAL_NAMES, MACRO_NAMES};

static KEYWORD_SET: LazyLock<HashSet<&str>> = LazyLock::new(|| KEYWORDS.iter().copied().collect());
static MACRO_SET: LazyLock<HashSet<&str>> =
    LazyLock::new(|| MACRO_NAMES.iter().chain(SPEC_MACROS).copied().collect());
static GLOBAL_SET: LazyLock<HashSet<&str>> = LazyLock::new(|| GLOBAL_NAMES.iter().copied().collect());

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let text = if mode & 1 == 0 {
        String::from_utf8_lossy(rest).into_owned()
    } else {
        code_points(rest)
    };
    if mode & STRETCH == STRETCH {
        if let Some(long) = stretch(&text, mode >> 4) {
            check_length_limits(&long);
        }
        return;
    }

    check_str_lit(&text);
    check_comment(&text);
    check_char_lit(&text);
    for c in text.chars().take(MAX_PIECES) {
        check_char_lit(c.encode_utf8(&mut [0; 4]));
    }
    check_ident(&text);
    check_number(&text);
    for word in text
        .split(|c: char| c.is_whitespace() || "\",:;()[]{}=<>".contains(c))
        .filter(|word| !word.is_empty())
        .take(MAX_PIECES)
    {
        check_ident(word);
        check_number(word);
    }
    if let Some(bits) = rest.first_chunk::<8>() {
        check_from_f64(f64::from_le_bytes(*bits));
    }
    if let Some(bits) = rest.first_chunk::<4>() {
        check_int(i32::from_le_bytes(*bits));
    }
});

/// Mode bits that select the length-limit test (one input in eight).
const STRETCH: u8 = 0b1110;

/// How many characters and words of one input are checked on their own.
const MAX_PIECES: usize = 8;

/// Reads the bytes as little-endian 32-bit code points, skipping surrogates.
fn code_points(bytes: &[u8]) -> String {
    bytes
        .chunks_exact(4)
        .filter_map(|chunk| {
            char::from_u32(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) % 0x11_0000)
        })
        .collect()
}

/// Repeats `text` until it is `MAX_TEXT_LEN - 8 + offset` bytes or a little
/// longer (by less than one character), so both sides of the limit are hit.
fn stretch(text: &str, offset: u8) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let target = MAX_TEXT_LEN - 8 + usize::from(offset);
    let mut out = text.repeat(target / text.len() + 1);
    let cut = (target..=out.len())
        .find(|&at| out.is_char_boundary(at))
        .expect("the end is a boundary");
    out.truncate(cut);
    Some(out)
}

/// Text for a panic message: the start of long text, with escapes.
fn shown(text: &str) -> String {
    const LIMIT: usize = 120;
    match text.char_indices().nth(LIMIT) {
        Some((cut, _)) => format!("{:?}… ({} bytes)", &text[..cut], text.len()),
        None => format!("{text:?}"),
    }
}

// ---------------------------------------------------------------------------
// Characters that must never appear raw
// ---------------------------------------------------------------------------

/// The characters spec §8.4.2 names explicitly (bidi controls, zero-width and
/// other invisible format characters, U+0085 and the line and paragraph
/// separators), listed here independently of `is_invisible`'s table.
fn is_spec_listed_hidden(c: char) -> bool {
    matches!(
        c,
        '\u{0085}'
            | '\u{00AD}'
            | '\u{061C}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

/// Characters that end a line in C++ source or in an editor (spec §8.4.3).
fn is_line_break(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// Why a character may not appear raw in generated text, if it may not.
fn hidden_reason(c: char) -> Option<&'static str> {
    if is_line_break(c) {
        Some("line break")
    } else if c.is_control() {
        Some("control character")
    } else if is_spec_listed_hidden(c) {
        Some("bidi or invisible character listed in spec §8.4.2")
    } else if is_invisible(c) {
        Some("invisible character")
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// String and character literals
// ---------------------------------------------------------------------------

fn check_length_limits(text: &str) {
    let fits = text.len() <= MAX_TEXT_LEN;
    match StrLit::new(text) {
        Ok(lit) => {
            assert!(
                fits && !text.contains('\0'),
                "StrLit::new accepted {}",
                shown(text)
            );
            assert_eq!(lit.value(), text, "StrLit::value changed the text");
        }
        Err(LiteralError::TooLong) => assert!(!fits, "StrLit::new said {} is too long", shown(text)),
        Err(LiteralError::Nul) => assert!(text.contains('\0'), "StrLit::new found NUL in {}", shown(text)),
        Err(error) => panic!("StrLit::new rejected {} with {error:?}", shown(text)),
    }
    match Comment::new(text) {
        Ok(comment) => {
            assert!(fits, "Comment::new accepted {} bytes", text.len());
            assert_eq!(comment.text(), text, "Comment::text changed the text");
        }
        Err(LiteralError::TooLong) => assert!(!fits, "Comment::new said {} bytes are too long", text.len()),
        Err(error) => panic!("Comment::new rejected {} with {error:?}", shown(text)),
    }
}

fn check_str_lit(text: &str) {
    let valid = text.len() <= MAX_TEXT_LEN && !text.contains('\0');
    let lit = match StrLit::new(text) {
        Ok(lit) => lit,
        Err(LiteralError::TooLong) if text.len() > MAX_TEXT_LEN => return,
        Err(LiteralError::Nul) if text.contains('\0') => return,
        Err(error) => panic!("StrLit::new rejected {} with {error:?}", shown(text)),
    };
    assert!(valid, "StrLit::new accepted {}", shown(text));
    assert_eq!(lit.value(), text, "StrLit::value changed the text");
    let encoded = lit.to_cpp();
    let decoded = decode_literal(&encoded, '"');
    assert!(
        decoded == text.as_bytes(),
        "the string literal {} for {} means {:?} in C++",
        shown(&encoded),
        shown(text),
        String::from_utf8_lossy(&decoded)
    );
}

fn check_char_lit(text: &str) {
    let mut chars = text.chars();
    let single = match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    };
    let lit = match CharLit::new(text) {
        Ok(lit) => lit,
        Err(LiteralError::NotOneChar) if single.is_none() => return,
        Err(LiteralError::Nul) if single == Some('\0') => return,
        Err(LiteralError::NotAscii(c)) if single == Some(c) && !c.is_ascii() => return,
        Err(error) => panic!("CharLit::new rejected {} with {error:?}", shown(text)),
    };
    let Some(c) = single.filter(|c| *c != '\0' && c.is_ascii()) else {
        panic!("CharLit::new accepted {}", shown(text));
    };
    assert_eq!(lit.value(), c, "CharLit::value changed the character");
    let encoded = lit.to_cpp();
    assert!(
        encoded.bytes().all(|b| (0x20..0x7F).contains(&b)),
        "the char literal {encoded:?} for {c:?} is not printable ASCII"
    );
    let decoded = decode_literal(&encoded, '\'');
    assert!(
        decoded == [c as u8],
        "the char literal {encoded:?} for {c:?} means {decoded:?} in C++ (must be one char)"
    );
}

/// Decodes a C++ narrow string (`quote` = `"`) or character (`'`) literal as
/// a compiler does (UTF-8 source and execution character sets), returning
/// the bytes it denotes. It also checks that the literal uses only the
/// encodings of the spec §8.4.2 table, and panics on anything else.
fn decode_literal(encoded: &str, quote: char) -> Vec<u8> {
    let Some(inner) = encoded
        .strip_prefix(quote)
        .and_then(|rest| rest.strip_suffix(quote))
    else {
        panic!("the literal {} is not enclosed in {quote}", shown(encoded));
    };
    // Spec §8.4.2: the output never contains `??`, so it cannot form a
    // trigraph (`??/` would turn the closing quote into an escape).
    assert!(
        !encoded.contains("??"),
        "the literal {} contains `??`",
        shown(encoded)
    );

    let chars: Vec<char> = inner.chars().collect();
    let mut out = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if c != '\\' {
            assert!(
                c != quote,
                "an unescaped {quote} at character {i} ends the literal {} early",
                shown(encoded)
            );
            if let Some(reason) = hidden_reason(c) {
                panic!(
                    "raw {reason} U+{:04X} in the literal {}",
                    c as u32,
                    shown(encoded)
                );
            }
            out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
            continue;
        }
        let Some(&kind) = chars.get(i) else {
            panic!(
                "the literal {} ends in a backslash that escapes its closing quote",
                shown(encoded)
            );
        };
        i += 1;
        let previous = out.last().copied();
        match kind {
            '\\' => out.push(b'\\'),
            'n' => out.push(b'\n'),
            't' => out.push(b'\t'),
            'r' => out.push(b'\r'),
            '"' | '\'' => {
                assert!(
                    kind == quote,
                    "the literal {} escapes {kind}, which the table leaves as it is",
                    shown(encoded)
                );
                out.push(kind as u8);
            }
            '?' => {
                assert!(
                    previous == Some(b'?'),
                    "the literal {} escapes a `?` that does not follow another `?`",
                    shown(encoded)
                );
                out.push(b'?');
            }
            '0'..='7' => {
                // Up to three octal digits, as C++ reads them.
                let start = i - 1;
                while i < chars.len() && i - start < 3 && chars[i].is_digit(8) {
                    i += 1;
                }
                let digits: String = chars[start..i].iter().collect();
                let value = u32::from_str_radix(&digits, 8).expect("octal digits");
                assert!(
                    digits.len() == 3,
                    "the octal escape \\{digits} in {} is not 3 digits long",
                    shown(encoded)
                );
                assert!(
                    (value < 0x20 && !matches!(value, 0x00 | 0x09 | 0x0A | 0x0D)) || value == 0x7F,
                    "the octal escape \\{digits} in {} is not for a C0 control or DEL",
                    shown(encoded)
                );
                out.push(u8::try_from(value).expect("checked above"));
            }
            'u' | 'U' => {
                let len = if kind == 'u' { 4 } else { 8 };
                let digits: String = chars.get(i..i + len).unwrap_or_default().iter().collect();
                i += len;
                let value = (digits.len() == len && digits.chars().all(|d| d.is_ascii_hexdigit()))
                    .then(|| u32::from_str_radix(&digits, 16).expect("hex digits"));
                let Some(c) = value.and_then(char::from_u32) else {
                    panic!(
                        "\\{kind}{digits} in {} is not a universal character name",
                        shown(encoded)
                    );
                };
                assert!(
                    is_invisible(c),
                    "\\{kind}{digits} in {} escapes a visible character",
                    shown(encoded)
                );
                assert!(
                    (kind == 'U') == (u32::from(c) > 0xFFFF),
                    "\\{kind}{digits} in {} should use \\{}",
                    shown(encoded),
                    if kind == 'u' { 'U' } else { 'u' }
                );
                out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
            }
            other => panic!(
                "the escape \\{other} in {} is not in the spec's table",
                shown(encoded)
            ),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Comments
// ---------------------------------------------------------------------------

fn check_comment(text: &str) {
    let comment = match Comment::new(text) {
        Ok(comment) => comment,
        Err(LiteralError::TooLong) if text.len() > MAX_TEXT_LEN => return,
        Err(error) => panic!("Comment::new rejected {} with {error:?}", shown(text)),
    };
    assert!(
        text.len() <= MAX_TEXT_LEN,
        "Comment::new accepted {} bytes",
        text.len()
    );
    assert_eq!(comment.text(), text, "Comment::text changed the text");
    let lines = comment.to_cpp_lines();
    let sources = source_lines(text);
    assert_eq!(
        lines.len(),
        sources.len(),
        "the comment {} has {} lines but became {lines:?}",
        shown(text),
        sources.len()
    );
    for (line, source) in lines.iter().zip(&sources) {
        check_comment_line(line, text);
        let expected = documented_comment_line(source);
        assert!(
            *line == expected,
            "the comment line {} became {} instead of {}",
            shown(source),
            shown(line),
            shown(&expected)
        );
    }
}

/// Splits text at every line terminator of spec §8.4.3 (`\r\n` counts once).
fn source_lines(text: &str) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' && chars.peek() == Some(&'\n') {
            chars.next();
        }
        if is_line_break(c) {
            lines.push(String::new());
        } else {
            lines.last_mut().expect("never empty").push(c);
        }
    }
    lines
}

/// The encoding spec §8.4.3 describes for one line: controls (except tab,
/// which is harmless and kept) and invisible characters become `<U+XXXX>`,
/// trailing whitespace goes, and a line ending in `\` or `??/` gets ` //`.
fn documented_comment_line(source: &str) -> String {
    let mut visible = String::new();
    for c in source.chars() {
        if c != '\t' && (c.is_control() || is_invisible(c) || is_spec_listed_hidden(c)) {
            write!(visible, "<U+{:04X}>", u32::from(c)).expect("writing to a String");
        } else {
            visible.push(c);
        }
    }
    let visible = visible.trim_end();
    let mut line = if visible.is_empty() {
        String::from("//")
    } else {
        format!("// {visible}")
    };
    if line.ends_with('\\') || line.ends_with("??/") {
        line.push_str(" //");
    }
    line
}

/// The safety properties of one emitted comment line, on their own.
fn check_comment_line(line: &str, text: &str) {
    assert!(
        line.starts_with("//"),
        "the comment line {} of {} does not start with //",
        shown(line),
        shown(text)
    );
    for c in line.chars() {
        if c == '\t' {
            continue;
        }
        if let Some(reason) = hidden_reason(c) {
            panic!(
                "raw {reason} U+{:04X} in the comment line {}",
                u32::from(c),
                shown(line)
            );
        }
    }
    assert!(
        line.trim_end() == line,
        "the comment line {} ends in whitespace",
        shown(line)
    );
    assert!(
        !line.ends_with('\\'),
        "the comment line {} ends in `\\` and splices the next line into the comment",
        shown(line)
    );
    assert!(
        !line.ends_with("??/"),
        "the comment line {} ends in the trigraph `??/` (a `\\` when trigraphs are on)",
        shown(line)
    );
}

// ---------------------------------------------------------------------------
// Identifiers
// ---------------------------------------------------------------------------

/// C++ keywords of every standard up to C++26, the alternative tokens and the
/// contextual keywords spec §8.4.1 rule 3 names, written out independently of
/// b2c-ir's table.
const KEYWORDS: &[&str] = &[
    // [lex.key], C++26
    "alignas",
    "alignof",
    "asm",
    "auto",
    "bool",
    "break",
    "case",
    "catch",
    "char",
    "char8_t",
    "char16_t",
    "char32_t",
    "class",
    "concept",
    "const",
    "consteval",
    "constexpr",
    "constinit",
    "const_cast",
    "continue",
    "contract_assert",
    "co_await",
    "co_return",
    "co_yield",
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
    "float",
    "for",
    "friend",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "mutable",
    "namespace",
    "new",
    "noexcept",
    "nullptr",
    "operator",
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
    // Alternative tokens ([lex.digraph])
    "and",
    "and_eq",
    "bitand",
    "bitor",
    "compl",
    "not",
    "not_eq",
    "or",
    "or_eq",
    "xor",
    "xor_eq",
    // Contextual keywords named by the spec
    "final",
    "override",
    "import",
    "module",
];

/// The macro names spec §8.4.1 rule 4 lists by name.
const SPEC_MACROS: &[&str] = &[
    "assert",
    "errno",
    "NULL",
    "EOF",
    "stdin",
    "stdout",
    "stderr",
    "offsetof",
    "EXIT_SUCCESS",
    "RAND_MAX",
    "INT_MAX",
    "linux",
    "unix",
];

/// Spec §8.4.1 rule 1: `^[A-Za-z][A-Za-z0-9_]{0,63}$`.
fn has_identifier_shape(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=MAX_IDENT_LEN).contains(&bytes.len())
        && bytes[0].is_ascii_alphabetic()
        && bytes.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
}

fn is_keyword(name: &str) -> bool {
    KEYWORD_SET.contains(name)
}

fn is_macro(name: &str) -> bool {
    MACRO_SET.contains(name)
}

/// Spec §8.4.1 rule 5 for user names: `main`, `std`, and the `b2c` prefix in
/// any letter case (which covers `B2C_` and generator temporaries).
fn is_reserved_for_generator(name: &str) -> bool {
    name == "main"
        || name == "std"
        || name
            .as_bytes()
            .get(..3)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"b2c"))
}

#[derive(Clone, Copy, Debug)]
enum IdentKind {
    User,
    Generated,
}

fn check_ident(name: &str) {
    let well_formed =
        has_identifier_shape(name) && !name.contains("__") && !is_keyword(name) && !is_macro(name);
    for kind in [IdentKind::User, IdentKind::Generated] {
        let (result, valid) = match kind {
            IdentKind::User => (Ident::new(name), well_formed && !is_reserved_for_generator(name)),
            IdentKind::Generated => (Ident::generated(name), well_formed && name.starts_with("b2c")),
        };
        match result {
            Ok(ident) => {
                assert!(valid, "{kind:?} identifier {} was accepted", shown(name));
                assert_eq!(ident.as_str(), name, "Ident::as_str changed the name");
                assert_eq!(ident.to_string(), name, "Ident's Display changed the name");
                check_namespace_scope(&ident);
            }
            Err(error) => {
                assert!(
                    !valid,
                    "{kind:?} identifier {} was rejected: {error:?}",
                    shown(name)
                );
                check_ident_error(name, kind, &error);
            }
        }
    }
}

/// The rule a rejection names must be one the name really breaks.
fn check_ident_error(name: &str, kind: IdentKind, error: &IdentError) {
    let first = name.chars().next();
    let true_reason = match error {
        IdentError::Empty => name.is_empty(),
        // In bytes; the same as characters for every name that can be valid.
        IdentError::TooLong => name.len() > MAX_IDENT_LEN,
        IdentError::BadStart => first.is_some_and(|c| !c.is_ascii_alphabetic()),
        IdentError::BadChar(c) => {
            !(c.is_ascii_alphanumeric() || *c == '_') && name.chars().skip(1).any(|other| other == *c)
        }
        IdentError::DoubleUnderscore => name.contains("__"),
        IdentError::Keyword(k) => k == name && is_keyword(name),
        IdentError::Macro(m) => m == name && is_macro(name),
        IdentError::Reserved(r) => {
            r == name
                && match kind {
                    IdentKind::User => is_reserved_for_generator(name),
                    IdentKind::Generated => !name.starts_with("b2c"),
                }
        }
        IdentError::GlobalClash(_) => false,
    };
    assert!(
        true_reason,
        "{kind:?} identifier {} was rejected for a rule it keeps: {error:?}",
        shown(name)
    );
}

fn check_namespace_scope(ident: &Ident) {
    let name = ident.as_str();
    let clashes = GLOBAL_SET.contains(name);
    match ident.check_namespace_scope() {
        Ok(()) => assert!(
            !clashes,
            "{name} clashes with a global C library name but was allowed"
        ),
        Err(IdentError::GlobalClash(reported)) => {
            assert!(
                clashes && reported == name,
                "{name} was reported as the global clash {reported}"
            );
        }
        Err(error) => panic!("check_namespace_scope({name}) failed with {error:?}"),
    }
}

// ---------------------------------------------------------------------------
// Numbers
// ---------------------------------------------------------------------------

/// `NumLit::parse` rejects longer text as a syntax error (a resource limit).
const MAX_NUMBER_TEXT: usize = 400;
/// `NumLit::parse` rejects integers with more digits (a resource limit).
const MAX_INTEGER_DIGITS: usize = 128;

/// A C++ digit-sequence with optional separators (`D ('? D)*`, where every
/// `'` stands between two digits), returned without the separators.
fn digit_sequence(text: &str, radix: u32) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut digits = String::with_capacity(chars.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == '\'' {
            let between_digits = i > 0
                && chars[i - 1].is_digit(radix)
                && chars.get(i + 1).is_some_and(|next| next.is_digit(radix));
            if !between_digits {
                return None;
            }
        } else if c.is_digit(radix) {
            digits.push(c);
        } else {
            return None;
        }
    }
    (!digits.is_empty()).then_some(digits)
}

/// An integer-literal without suffix ([lex.icon]): its radix and digits.
fn cpp_integer(text: &str) -> Option<(u32, String)> {
    if let Some(rest) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        digit_sequence(rest, 16).map(|digits| (16, digits))
    } else if let Some(rest) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
        digit_sequence(rest, 2).map(|digits| (2, digits))
    } else if text.starts_with('0') {
        digit_sequence(text, 8).map(|digits| (8, digits))
    } else {
        digit_sequence(text, 10).map(|digits| (10, digits))
    }
}

/// A decimal floating-literal without suffix ([lex.fcon]), without its
/// separators: `D . D? E?`, `. D E?` or `D E`.
fn cpp_decimal_float(text: &str) -> Option<String> {
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(at) => (&text[..at], Some(&text[at + 1..])),
        None => (text, None),
    };
    let mut cleaned = match mantissa.split_once('.') {
        Some(("", fraction)) => format!(".{}", digit_sequence(fraction, 10)?),
        Some((whole, "")) => format!("{}.", digit_sequence(whole, 10)?),
        Some((whole, fraction)) => {
            format!("{}.{}", digit_sequence(whole, 10)?, digit_sequence(fraction, 10)?)
        }
        None if exponent.is_some() => digit_sequence(mantissa, 10)?,
        None => return None,
    };
    if let Some(exponent) = exponent {
        let (sign, digits) = match exponent.strip_prefix(['+', '-']) {
            Some(digits) => (&exponent[..1], digits),
            None => ("", exponent),
        };
        write!(cleaned, "e{sign}{}", digit_sequence(digits, 10)?).expect("writing to a String");
    }
    Some(cleaned)
}

/// The value of an integer literal's digits; `None` beyond `u128`.
fn integer_value(radix: u32, digits: &str) -> Option<u128> {
    digits.chars().try_fold(0_u128, |value, digit| {
        value
            .checked_mul(u128::from(radix))?
            .checked_add(u128::from(digit.to_digit(radix).expect("a digit")))
    })
}

fn check_number(text: &str) {
    for ty in [NumType::Int, NumType::LongLong, NumType::Double] {
        match ty {
            NumType::Int | NumType::LongLong => check_integer_parse(text, ty),
            NumType::Double => check_double_parse(text),
        }
    }
}

/// A literal `NumLit::parse` may accept as an integer: not octal, except `0`.
fn decimal_hex_or_binary(text: &str) -> Option<(u32, String)> {
    cpp_integer(text).filter(|(radix, digits)| *radix != 8 || digits == "0")
}

/// Whether the resource limits of `NumLit::parse` allow an integer literal.
fn within_limits(text: &str, digits: &str) -> bool {
    text.len() <= MAX_NUMBER_TEXT && digits.len() <= MAX_INTEGER_DIGITS
}

fn check_integer_parse(text: &str, ty: NumType) {
    let max: u128 = if ty == NumType::Int {
        i32::MAX.unsigned_abs().into()
    } else {
        i64::MAX.unsigned_abs().into()
    };
    let literal = decimal_hex_or_binary(text);
    let value = literal
        .as_ref()
        .and_then(|(radix, digits)| integer_value(*radix, digits));
    match NumLit::parse(text, ty) {
        Ok(lit) => {
            let Some((radix, digits)) = &literal else {
                panic!(
                    "NumLit::parse accepted {} as {}: not a C++ integer literal",
                    shown(text),
                    ty.cpp_name()
                );
            };
            let value = value.unwrap_or(u128::MAX);
            assert!(
                value <= max,
                "NumLit::parse accepted {} for {}: out of range",
                shown(text),
                ty.cpp_name()
            );
            assert_eq!(
                lit.as_str(),
                value.to_string(),
                "NumLit::parse({}, {}) printed the wrong value (radix {radix}, digits {digits})",
                shown(text),
                ty.cpp_name()
            );
            assert_eq!(lit.num_type(), ty, "NumLit::parse changed the type");
            check_printed_integer(&lit);
        }
        Err(NumError::OutOfRange {
            text: reported,
            ty: name,
        }) => {
            assert!(
                reported == text && name == ty.cpp_name(),
                "NumLit::parse({}) reported {reported:?} / {name}",
                shown(text)
            );
            assert!(
                literal.is_some() && value.is_none_or(|value| value > max),
                "NumLit::parse said {} does not fit {name}",
                shown(text)
            );
        }
        Err(NumError::Syntax(reported)) => {
            assert!(
                reported == text,
                "NumLit::parse({}) reported {reported:?}",
                shown(text)
            );
            // A valid literal within the parser's limits is accepted or, when
            // too large, reported as out of range.
            if let (Some((_, digits)), Some(value)) = (&literal, value) {
                assert!(
                    !within_limits(text, digits) || i128::try_from(value).is_err(),
                    "NumLit::parse rejected the valid {} literal {} as a syntax error",
                    ty.cpp_name(),
                    shown(text)
                );
            }
        }
    }
}

/// An integer literal as emitted: plain decimal digits without a suffix or a
/// leading zero, which the parser reads back as the same literal.
fn check_printed_integer(lit: &NumLit) {
    let text = lit.as_str();
    assert!(
        !text.is_empty()
            && text.bytes().all(|b| b.is_ascii_digit())
            && (text == "0" || !text.starts_with('0')),
        "the {} literal {text:?} is not plain decimal",
        lit.num_type().cpp_name()
    );
    assert_eq!(
        NumLit::parse(text, lit.num_type()).as_ref(),
        Ok(lit),
        "the printed literal {text:?} does not parse back"
    );
}

fn check_double_parse(text: &str) {
    // The value C++ gives the literal, correctly rounded (Rust's parser and
    // integer casts round to nearest, as GCC does).
    #[allow(clippy::cast_precision_loss)]
    let expected = if let Some((radix, digits)) = decimal_hex_or_binary(text) {
        let value = integer_value(radix, &digits).filter(|value| i128::try_from(*value).is_ok());
        match value {
            Some(value) if within_limits(text, &digits) => Some(value as f64),
            // Past the parser's limits: any rejection is fine.
            _ => None,
        }
    } else if let Some(cleaned) = cpp_decimal_float(text) {
        let value: f64 = cleaned
            .parse()
            .unwrap_or_else(|error| panic!("Rust cannot parse the C++ literal {cleaned:?}: {error}"));
        (text.len() <= MAX_NUMBER_TEXT).then_some(value)
    } else {
        None
    };
    let is_literal = decimal_hex_or_binary(text).is_some() || cpp_decimal_float(text).is_some();
    match NumLit::parse(text, NumType::Double) {
        Ok(lit) => {
            assert!(
                is_literal,
                "NumLit::parse accepted {} as double: not a C++ literal",
                shown(text)
            );
            let Some(expected) = expected else {
                panic!("NumLit::parse accepted {} as double past its limits", shown(text));
            };
            assert!(
                expected.is_finite(),
                "NumLit::parse accepted {} as double: out of range",
                shown(text)
            );
            check_printed_double(&lit, expected, text);
        }
        Err(NumError::OutOfRange { text: reported, ty }) => {
            assert!(
                reported == text && ty == "double",
                "NumLit::parse({}) reported {reported:?} / {ty}",
                shown(text)
            );
            assert!(
                expected.is_some_and(|value| !value.is_finite()),
                "NumLit::parse said {} does not fit double",
                shown(text)
            );
        }
        Err(NumError::Syntax(reported)) => {
            assert!(
                reported == text,
                "NumLit::parse({}) reported {reported:?}",
                shown(text)
            );
            assert!(
                expected.is_none(),
                "NumLit::parse rejected the valid double literal {} as a syntax error",
                shown(text)
            );
        }
    }
}

/// A `double` literal as emitted: a decimal floating literal without sign or
/// suffix (so its type is `double`) whose value is exactly `expected`, and
/// which the parser reads back as the same literal.
fn check_printed_double(lit: &NumLit, expected: f64, source: &str) {
    let text = lit.as_str();
    assert_eq!(
        lit.num_type(),
        NumType::Double,
        "a double literal has type {:?}",
        lit.num_type()
    );
    let Some(cleaned) = cpp_decimal_float(text) else {
        panic!(
            "the double literal {text:?} (from {}) is not a C++ floating literal",
            shown(source)
        );
    };
    let value: f64 = cleaned.parse().expect("a decimal floating literal");
    assert!(
        value.to_bits() == expected.to_bits(),
        "the double literal {text:?} (from {}) is {value:e}, not {expected:e}",
        shown(source)
    );
    assert_eq!(
        NumLit::parse(text, NumType::Double).as_ref(),
        Ok(lit),
        "the printed literal {text:?} does not parse back"
    );
}

fn check_from_f64(value: f64) {
    let lit = NumLit::from_f64(value);
    assert_eq!(
        lit.is_some(),
        value.is_finite() && value.is_sign_positive(),
        "NumLit::from_f64({value:?}) gave {lit:?}"
    );
    let Some(lit) = lit else {
        return;
    };
    check_printed_double(&lit, value, &format!("{value:?}"));
    // The way a user might write the same value parses to the same literal.
    let scientific = format!("{value:e}");
    assert_eq!(
        NumLit::parse(&scientific, NumType::Double).as_ref(),
        Ok(&lit),
        "{scientific:?} does not parse to the literal for {value:?}"
    );
}

fn check_int(value: i32) {
    let lit = NumLit::int(value);
    assert_eq!(
        lit.num_type(),
        NumType::Int,
        "NumLit::int({value}) has type {:?}",
        lit.num_type()
    );
    // C++ has no negative literals: the magnitude, which callers negate.
    assert_eq!(
        lit.as_str(),
        value.unsigned_abs().to_string(),
        "NumLit::int({value})"
    );
    // The magnitude of `i32::MIN` does not fit `int`; the analyser writes that
    // value as `-2147483647 - 1` and never asks for it.
    if value != i32::MIN {
        check_printed_integer(&lit);
    }
}
