//! The text rules every string in a project file must follow (spec §5.6,
//! §8.4), and safe quoting of untrusted text inside messages.

use std::fmt::Write as _;

/// Unicode bidirectional controls rejected in project text (spec §5.6):
/// U+061C, U+200E, U+200F, U+202A–U+202E and U+2066–U+2069. They can make
/// text look different from what it is ("Trojan Source", CVE-2021-42574).
pub(crate) fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// A C0 control other than tab and newline (NUL excluded: it has its own rule).
fn is_forbidden_control(c: char) -> bool {
    c != '\0' && c != '\t' && c != '\n' && c < ' '
}

/// What is wrong with a piece of text (each rule reports its first offender).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TextProblems {
    /// The text contains U+0000.
    pub(crate) nul: bool,
    /// The first forbidden C0 control character.
    pub(crate) control: Option<char>,
    /// The first bidirectional control character.
    pub(crate) bidi: Option<char>,
}

impl TextProblems {
    /// Whether the text follows every rule.
    #[cfg(test)]
    pub(crate) fn is_clean(self) -> bool {
        self == Self::default()
    }
}

/// Checks text against the rules.
pub(crate) fn check(text: &str) -> TextProblems {
    let mut problems = TextProblems::default();
    // Fast path: only bytes below 0x20 and the lead bytes of the bidi
    // characters (0xD8 for U+061C, 0xE2 for the others) can break a rule.
    if !text.bytes().any(|b| b < 0x20 || b == 0xD8 || b == 0xE2) {
        return problems;
    }
    for c in text.chars() {
        if c == '\0' {
            problems.nul = true;
        } else if is_forbidden_control(c) {
            problems.control.get_or_insert(c);
        } else if is_bidi_control(c) {
            problems.bidi.get_or_insert(c);
        }
    }
    problems
}

/// A short name for a control character with its article ("a carriage
/// return", "an escape character"), for messages.
pub(crate) fn control_name(c: char) -> &'static str {
    match c {
        '\r' => "a carriage return",
        '\u{1b}' => "an escape character",
        '\u{8}' => "a backspace",
        '\u{b}' => "a vertical tab",
        '\u{c}' => "a form feed",
        '\u{7}' => "a bell character",
        _ => "a control character",
    }
}

/// A short name for a bidirectional control, for messages.
pub(crate) fn bidi_name(c: char) -> &'static str {
    match c {
        '\u{061C}' => "Arabic letter mark",
        '\u{200E}' => "left-to-right mark",
        '\u{200F}' => "right-to-left mark",
        '\u{202A}' => "left-to-right embedding",
        '\u{202B}' => "right-to-left embedding",
        '\u{202C}' => "pop directional formatting",
        '\u{202D}' => "left-to-right override",
        '\u{202E}' => "right-to-left override",
        '\u{2066}' => "left-to-right isolate",
        '\u{2067}' => "right-to-left isolate",
        '\u{2068}' => "first strong isolate",
        '\u{2069}' => "pop directional isolate",
        _ => "bidirectional control",
    }
}

/// Characters that must not be shown raw in a message: controls (C0, DEL,
/// C1), bidi controls and every other invisible character the C++ encoders
/// escape ([`b2c_ir::text::is_invisible`]: all format characters, including
/// the invisible "tag" characters U+E0020–U+E007F that can smuggle hidden
/// text, and noncharacters), plus a few more that render as nothing.
fn is_unsafe_to_show(c: char) -> bool {
    c.is_control()
        || is_bidi_control(c)
        || b2c_ir::text::is_invisible(c)
        || matches!(c,
            '\u{00AD}' | '\u{180E}'
            | '\u{200B}'..='\u{200D}'
            | '\u{2028}' | '\u{2029}'
            | '\u{2060}'..='\u{2065}'
            | '\u{206A}'..='\u{206F}'
            | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}')
}

/// Maximum characters of untrusted text quoted in a message.
const QUOTE_CHARS: usize = 40;

/// Quotes untrusted text for a message: wrapped in double quotes, invisible
/// and control characters shown as `\u{XXXX}`, and cut after
/// [`QUOTE_CHARS`] characters.
pub(crate) fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for (index, c) in text.chars().enumerate() {
        if index == QUOTE_CHARS {
            out.push('…');
            break;
        }
        if is_unsafe_to_show(c) {
            let _ = write!(out, "\\u{{{:04X}}}", u32::from(c));
        } else if c == '"' || c == '\\' {
            out.push('\\');
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_text() {
        for text in [
            "",
            "hello",
            "tab\tand\nnewline",
            "héllo ✓ 😀",
            "\u{7f}",
            "\u{85}",
            "\u{2028}",
            "\u{200b}",
        ] {
            assert!(check(text).is_clean(), "{text:?}");
        }
    }

    #[test]
    fn every_forbidden_character_is_found() {
        for byte in 0u8..0x20 {
            let c = char::from(byte);
            let problems = check(&format!("a{c}b"));
            match c {
                '\0' => assert!(problems.nul),
                '\t' | '\n' => assert!(problems.is_clean()),
                _ => assert_eq!(problems.control, Some(c)),
            }
        }
        for code in [
            0x061C, 0x200E, 0x200F, 0x202A, 0x202B, 0x202C, 0x202D, 0x202E, 0x2066, 0x2067, 0x2068, 0x2069,
        ] {
            let c = char::from_u32(code).unwrap();
            assert_eq!(check(&format!("x{c}")).bidi, Some(c));
            assert_ne!(bidi_name(c), "bidirectional control");
        }
        for code in [0x061B, 0x061D, 0x200D, 0x2010, 0x2029, 0x202F, 0x2065, 0x206A] {
            assert!(check(&char::from_u32(code).unwrap().to_string()).is_clean());
        }
    }

    #[test]
    fn several_problems_at_once() {
        let problems = check("\u{202e}a\0b\rc\u{1b}");
        assert!(problems.nul);
        assert_eq!(problems.control, Some('\r'));
        assert_eq!(problems.bidi, Some('\u{202e}'));
        assert_eq!(control_name('\r'), "a carriage return");
        assert_eq!(control_name('\u{1b}'), "an escape character");
        assert_eq!(control_name('\u{1}'), "a control character");
    }

    #[test]
    fn quoting_is_safe() {
        assert_eq!(quote("abc"), "\"abc\"");
        assert_eq!(quote("a\"b\\"), "\"a\\\"b\\\\\"");
        assert_eq!(
            quote("\u{202e}x\u{1b}[2J\n"),
            "\"\\u{202E}x\\u{001B}[2J\\u{000A}\""
        );
        assert_eq!(quote(&"x".repeat(100)), format!("\"{}…\"", "x".repeat(40)));
        assert_eq!(quote("\u{feff}\u{200b}\u{85}"), "\"\\u{FEFF}\\u{200B}\\u{0085}\"");
        // Every format character is escaped, such as the invisible tag
        // characters (which can carry hidden text), U+0600 and U+1D173.
        assert_eq!(
            quote("a\u{e0001}\u{e0041}\u{600}\u{1d173}\u{fffe}b"),
            "\"a\\u{E0001}\\u{E0041}\\u{0600}\\u{1D173}\\u{FFFE}b\""
        );
    }
}
