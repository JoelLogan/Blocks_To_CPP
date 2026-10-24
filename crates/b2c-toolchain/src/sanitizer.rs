//! Finding sanitizer reports in a running program's output
//! (`docs/spec/07-toolchain-build-run.md` §7.6.4, "Sanitizer reports in M2").
//!
//! Debug builds link AddressSanitizer and UndefinedBehaviorSanitizer where the
//! toolchain has them (§7.4.3). When one of them stops the program, the program
//! merely exits with code 1, so the exit status alone would say *"Finished with
//! exit code 1"*. The [`Detector`] watches the output stream for the first
//! report, so the exit message can say *"Crashed: heap-buffer-overflow
//! (AddressSanitizer)"* instead:
//!
//! * AddressSanitizer: a line `==<pid>==ERROR: AddressSanitizer: <kind> …`;
//!   the kind is the bug type AddressSanitizer names (`heap-buffer-overflow`,
//!   `stack-overflow`, `SEGV` as `segv`, …).
//! * UndefinedBehaviorSanitizer: a line `<file>:<line>:<column>: runtime
//!   error: <message>` (or `<unknown>: runtime error: …`, or a line that starts
//!   with `runtime error:`); the kind is derived from the message
//!   (`signed-integer-overflow`, `division-by-zero`, …, else
//!   `undefined-behavior`).
//!
//! The scanner is pure and bounded, because the program controls its output:
//!
//! * it is fed raw bytes in chunks of any size, as they arrive, and the result
//!   does not depend on how the stream was split;
//! * terminal escape sequences (sanitizers colour their reports when standard
//!   error is a terminal, as it is in the IDE's console) and carriage returns
//!   are skipped, and only the first [`MAX_LINE_BYTES`] bytes of each line are
//!   kept, so its state never exceeds 4 KiB however long a line is;
//! * the kind it reports always matches `[a-z0-9-]{1,64}`;
//! * the first report wins: after it, further output is ignored.
//!
//! A program can of course print text that looks like a report; the result
//! only changes how its own exit is described. LeakSanitizer reports
//! (`ERROR: LeakSanitizer`) are not matched in M2. Mapping a report to blocks
//! through its stack frames arrives in M5.
//!
//! ```
//! use b2c_toolchain::sanitizer::{Detector, Tool};
//!
//! let mut detector = Detector::new();
//! detector.feed(b"guess: 42\r\n==4158==ERROR: AddressSanitizer: heap-buf");
//! detector.feed(b"fer-overflow on address 0x502000000020\r\n");
//! let report = detector.report().expect("a report");
//! assert_eq!(report.tool, Tool::Address);
//! assert_eq!(report.kind, "heap-buffer-overflow");
//! assert_eq!(report.summary(), "Crashed: heap-buffer-overflow (AddressSanitizer)");
//! ```

use std::fmt;

/// The most bytes of one output line the detector keeps (4 KiB). The rest of
/// a longer line is skipped, so a report whose marker lies beyond it is not
/// found.
pub const MAX_LINE_BYTES: usize = 4096;

/// The longest [`SanitizerReport::kind`].
pub const MAX_KIND_LEN: usize = 64;

/// The kind reported when AddressSanitizer names a bug type that does not fit
/// the kind format.
const ASAN_FALLBACK: &str = "unknown-crash";

/// The kind reported for an UndefinedBehaviorSanitizer message that matches no
/// known check.
const UBSAN_FALLBACK: &str = "undefined-behavior";

/// The escape character that starts a terminal control sequence.
const ESC: u8 = 0x1B;

/// The sanitizer that reported a problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    /// AddressSanitizer (`-fsanitize=address`).
    Address,
    /// UndefinedBehaviorSanitizer (`-fsanitize=undefined`).
    Undefined,
}

impl Tool {
    /// The sanitizer's own name, as it prints it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Address => "AddressSanitizer",
            Self::Undefined => "UndefinedBehaviorSanitizer",
        }
    }
}

impl fmt::Display for Tool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The first sanitizer report found in a program's output.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SanitizerReport {
    /// Which sanitizer reported.
    pub tool: Tool,
    /// What it found, for example `heap-buffer-overflow` or
    /// `signed-integer-overflow`: 1 to [`MAX_KIND_LEN`] characters from
    /// `a`–`z`, `0`–`9` and `-` (the [`Detector`] never produces anything
    /// else).
    pub kind: String,
}

impl SanitizerReport {
    /// The exit message for a program the report stopped, for example
    /// *"Crashed: heap-buffer-overflow (AddressSanitizer)"*.
    pub fn summary(&self) -> String {
        format!("Crashed: {} ({})", self.kind, self.tool.name())
    }
}

/// Where the scanner is inside a terminal escape sequence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Escape {
    /// Plain text.
    #[default]
    None,
    /// Just after `ESC`.
    Esc,
    /// In a control sequence (`ESC [` … final byte).
    Csi,
    /// In a string sequence (`ESC ]`, `ESC P`, `ESC X`, `ESC ^`, `ESC _`),
    /// which ends with `BEL` or `ESC \`.
    Str,
    /// Just after an `ESC` inside a string sequence.
    StrEsc,
}

/// Watches a program's output for the first sanitizer report (see the
/// [module documentation](self)).
#[derive(Debug, Clone, Default)]
pub struct Detector {
    /// The text of the current line so far, without escape sequences and
    /// control characters, at most [`MAX_LINE_BYTES`] bytes.
    line: Vec<u8>,
    escape: Escape,
    found: Option<SanitizerReport>,
}

impl Detector {
    /// A detector that has seen no output yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Scans the next piece of output. Lines may be split across calls
    /// anywhere, even inside an escape sequence or a UTF-8 character. Does
    /// nothing once a report has been found.
    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.found.is_some() {
                return;
            }
            self.step(byte);
        }
    }

    /// The first report in the output so far. A last line without a line
    /// break counts as complete, so call this once the output has ended
    /// (before that, a report still being printed may show only the start
    /// of its kind).
    pub fn report(&self) -> Option<SanitizerReport> {
        self.found.clone().or_else(|| match_line(&self.line))
    }

    /// How many bytes of the current line are kept (at most
    /// [`MAX_LINE_BYTES`]), for tests and fuzzing of the memory bound.
    pub fn pending_line_len(&self) -> usize {
        self.line.len()
    }

    fn step(&mut self, byte: u8) {
        if byte == b'\n' {
            // A line break ends any escape sequence: a sequence the program
            // never finished must not hide the following lines.
            self.escape = Escape::None;
            self.end_line();
            return;
        }
        self.escape = match self.escape {
            Escape::None => match byte {
                ESC => Escape::Esc,
                // Tabs are text; carriage returns and other control
                // characters are not.
                0x00..=0x08 | 0x0A..=0x1F | 0x7F => Escape::None,
                _ => {
                    self.push(byte);
                    Escape::None
                }
            },
            Escape::Esc => after_esc(byte),
            Escape::Csi => match byte {
                // The final byte ends the sequence; CAN and SUB cancel it.
                0x40..=0x7E | 0x18 | 0x1A => Escape::None,
                ESC => Escape::Esc,
                _ => Escape::Csi,
            },
            Escape::Str => match byte {
                0x07 => Escape::None,
                ESC => Escape::StrEsc,
                _ => Escape::Str,
            },
            Escape::StrEsc => {
                if byte == b'\\' {
                    Escape::None
                } else {
                    // Not a string terminator: this ESC starts a new sequence.
                    after_esc(byte)
                }
            }
        };
    }

    fn push(&mut self, byte: u8) {
        if self.line.len() < MAX_LINE_BYTES {
            self.line.push(byte);
        }
    }

    fn end_line(&mut self) {
        self.found = match_line(&self.line);
        if self.found.is_some() {
            // Nothing else is scanned; give the buffer back.
            self.line = Vec::new();
        } else {
            self.line.clear();
        }
    }
}

/// The state after `ESC` followed by `byte`.
fn after_esc(byte: u8) -> Escape {
    match byte {
        b'[' => Escape::Csi,
        b']' | b'P' | b'X' | b'^' | b'_' => Escape::Str,
        ESC => Escape::Esc,
        // A two-byte sequence (`ESC 7`, `ESC c`, …) ends here.
        _ => Escape::None,
    }
}

/// The report on one line of text (escape sequences already removed), if any.
fn match_line(line: &[u8]) -> Option<SanitizerReport> {
    match_address(line).or_else(|| match_undefined(line))
}

/// `==<pid>==ERROR: AddressSanitizer: <kind> …` at the start of the line.
fn match_address(line: &[u8]) -> Option<SanitizerReport> {
    let rest = line.strip_prefix(b"==")?;
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || digits > 10 {
        return None;
    }
    let rest = rest[digits..].strip_prefix(b"==ERROR: AddressSanitizer")?;
    let kind = if let Some(words) = rest.strip_prefix(b": ") {
        address_kind(words)
    } else if rest.starts_with(b" failed to allocate") {
        // The allocator could not map memory: there is no colon form.
        String::from("out-of-memory")
    } else {
        return None;
    };
    Some(SanitizerReport {
        tool: Tool::Address,
        kind,
    })
}

/// The kind from the words after `AddressSanitizer: `.
fn address_kind(words: &[u8]) -> String {
    let mut words = words
        .split(u8::is_ascii_whitespace)
        .filter(|word| !word.is_empty());
    let first = words.next().unwrap_or_default();
    let fixed = match first {
        // "attempting double-free on 0x…", "attempting free on address which
        // was not malloc()-ed"
        b"attempting" => Some(match words.next().unwrap_or_default() {
            b"double-free" => "double-free",
            b"free" => "bad-free",
            _ => ASAN_FALLBACK,
        }),
        // "requested allocation size 0x… exceeds maximum supported size"
        b"requested" => Some("allocation-size-too-big"),
        // "allocator is out of memory trying to allocate", "out of memory: …"
        b"allocator" | b"out" => Some("out-of-memory"),
        _ => None,
    };
    if let Some(kind) = fixed {
        return kind.to_owned();
    }
    let trimmed = first
        .strip_suffix(b":")
        .or_else(|| first.strip_suffix(b","))
        .unwrap_or(first);
    let lower = trimmed.to_ascii_lowercase();
    if is_kind(&lower) {
        // `is_kind` accepted only ASCII.
        String::from_utf8(lower).unwrap_or_else(|_| ASAN_FALLBACK.to_owned())
    } else {
        ASAN_FALLBACK.to_owned()
    }
}

/// Whether `text` matches `[a-z0-9-]{1,64}`.
fn is_kind(text: &[u8]) -> bool {
    (1..=MAX_KIND_LEN).contains(&text.len())
        && text
            .iter()
            .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The marker of an UndefinedBehaviorSanitizer report.
const RUNTIME_ERROR: &[u8] = b"runtime error: ";

/// `<location>: runtime error: <message>`, or a line that starts with
/// `runtime error: `.
fn match_undefined(line: &[u8]) -> Option<SanitizerReport> {
    let message = if let Some(message) = line.strip_prefix(RUNTIME_ERROR) {
        message
    } else {
        let at = find(line, b": runtime error: ")?;
        if !is_location(&line[..at]) {
            return None;
        }
        &line[at + 2 + RUNTIME_ERROR.len()..]
    };
    Some(SanitizerReport {
        tool: Tool::Undefined,
        kind: undefined_kind(message).to_owned(),
    })
}

/// The first position of `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// Whether `text` is a location as UndefinedBehaviorSanitizer prints it at the
/// start of a line: `<file>:<line>` or `<file>:<line>:<column>`, `<unknown>`,
/// or a module location in brackets (`(<module>+0x…)`).
fn is_location(text: &[u8]) -> bool {
    if text.first().is_none_or(u8::is_ascii_whitespace) {
        return false;
    }
    if text == b"<unknown>" || (text.len() > 2 && text.starts_with(b"(") && text.ends_with(b")")) {
        return true;
    }
    let mut rest = text;
    let mut numbers = 0;
    while numbers < 2 {
        let Some(colon) = rest.iter().rposition(|&b| b == b':') else {
            break;
        };
        let number = &rest[colon + 1..];
        if number.is_empty() || !number.iter().all(u8::is_ascii_digit) {
            break;
        }
        rest = &rest[..colon];
        numbers += 1;
    }
    numbers > 0 && !rest.is_empty()
}

/// The kind for an UndefinedBehaviorSanitizer message: the first entry of
/// this table whose text the message contains (the wording of the GCC and
/// LLVM runtimes), named after the check that reports it.
const UNDEFINED_KINDS: &[(&str, &str)] = &[
    ("signed integer overflow", "signed-integer-overflow"),
    ("negation of ", "signed-integer-overflow"),
    ("division of ", "signed-integer-overflow"),
    ("division by zero", "division-by-zero"),
    ("shift exponent", "shift-exponent"),
    ("left shift of", "shift-base"),
    ("null pointer passed as argument", "nonnull-attribute"),
    ("null pointer returned from function", "returns-nonnull-attribute"),
    ("misaligned address", "misaligned-address"),
    ("pointer index expression", "pointer-overflow"),
    ("applying non-zero offset", "pointer-overflow"),
    ("applying zero offset", "pointer-overflow"),
    ("addition of unsigned offset", "pointer-overflow"),
    ("subtraction of unsigned offset", "pointer-overflow"),
    ("null pointer", "null-pointer-use"),
    ("out of bounds for type", "index-out-of-bounds"),
    ("variable length array bound", "vla-bound"),
    ("end of a value-returning function", "missing-return"),
    ("unreachable program point", "unreachable"),
    ("outside the range of representable values", "float-cast-overflow"),
    ("not a valid value for type 'bool'", "invalid-bool-value"),
    ("not a valid value for type", "invalid-enum-value"),
    ("passing zero to", "invalid-builtin-use"),
    ("insufficient space for an object", "object-size"),
    ("does not point to an object of type", "dynamic-type-mismatch"),
    ("implicit conversion", "implicit-conversion"),
];

fn undefined_kind(message: &[u8]) -> &'static str {
    UNDEFINED_KINDS
        .iter()
        .find(|(needle, _)| find(message, needle.as_bytes()).is_some())
        .map_or(UBSAN_FALLBACK, |&(_, kind)| kind)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn scan(output: &[u8]) -> Option<SanitizerReport> {
        let mut detector = Detector::new();
        detector.feed(output);
        detector.report()
    }

    // Compared with what `Detector::report` returns.
    #[allow(clippy::unnecessary_wraps)]
    fn address(kind: &str) -> Option<SanitizerReport> {
        Some(SanitizerReport {
            tool: Tool::Address,
            kind: kind.to_owned(),
        })
    }

    #[allow(clippy::unnecessary_wraps)]
    fn undefined(kind: &str) -> Option<SanitizerReport> {
        Some(SanitizerReport {
            tool: Tool::Undefined,
            kind: kind.to_owned(),
        })
    }

    /// AddressSanitizer's report as g++ 13's runtime prints it to a terminal
    /// (coloured, `\r\n` line ends).
    const ASAN_COLOURED: &[u8] = b"=================================================================\r\n\
        \x1b[1m\x1b[31m==4158==ERROR: AddressSanitizer: heap-buffer-overflow on address 0x502000000020 at pc 0x55d3e93ec351 bp 0x7ffe356dd330 sp 0x7ffe356dd320\r\n\
        \x1b[1m\x1b[0m\x1b[1m\x1b[34mWRITE of size 4 at 0x502000000020 thread T0\x1b[1m\x1b[0m\r\n";

    /// UndefinedBehaviorSanitizer's report as g++ 13's runtime prints it to a
    /// terminal.
    const UBSAN_COLOURED: &[u8] = b"\x1b[1mub.cpp:3:49:\x1b[1m\x1b[31m runtime error: \x1b[1m\x1b[0m\x1b[1msigned integer overflow: 2147483647 + 1 cannot be represented in type 'int'\x1b[1m\x1b[0m\r\n\
        \x20   #0 0x5627b4757229 in main /tmp/ub.cpp:3\r\n";

    #[test]
    fn real_reports_are_found_with_or_without_colour() {
        assert_eq!(scan(ASAN_COLOURED), address("heap-buffer-overflow"));
        assert_eq!(scan(UBSAN_COLOURED), undefined("signed-integer-overflow"));
        assert_eq!(
            scan(b"==12==ERROR: AddressSanitizer: stack-overflow on address 0x7ffd\n"),
            address("stack-overflow")
        );
        assert_eq!(
            scan(b"main.cpp:10:5: runtime error: division by zero\n"),
            undefined("division-by-zero")
        );
    }

    #[test]
    fn address_kinds_are_normalised() {
        let cases: &[(&str, &str)] = &[
            ("heap-use-after-free on address 0x1", "heap-use-after-free"),
            ("SEGV on unknown address 0x000000000000 (pc 0x1)", "segv"),
            ("FPE on unknown address", "fpe"),
            ("attempting double-free on 0x602 in thread T0:", "double-free"),
            (
                "attempting free on address which was not malloc()-ed: 0x1",
                "bad-free",
            ),
            (
                "attempting to call malloc_usable_size() for pointer",
                "unknown-crash",
            ),
            (
                "requested allocation size 0xfff exceeds maximum supported size",
                "allocation-size-too-big",
            ),
            (
                "allocator is out of memory trying to allocate 0x10 bytes",
                "out-of-memory",
            ),
            ("out of memory: allocator is trying to allocate", "out-of-memory"),
            ("odr-violation (0x1):", "odr-violation"),
            (
                "alloc-dealloc-mismatch (operator new [] vs operator delete)",
                "alloc-dealloc-mismatch",
            ),
            ("new-delete-type-mismatch on 0x1", "new-delete-type-mismatch"),
            ("BUS on unknown address", "bus"),
            ("unknown-crash on address", "unknown-crash"),
            ("weird_kind on address", "unknown-crash"),
            ("<script> on address", "unknown-crash"),
            ("", "unknown-crash"),
        ];
        for &(words, kind) in cases {
            let line = format!("==1==ERROR: AddressSanitizer: {words}\n");
            assert_eq!(scan(line.as_bytes()), address(kind), "{words:?}");
        }
        let long = format!("==1==ERROR: AddressSanitizer: {} x\n", "a".repeat(65));
        assert_eq!(scan(long.as_bytes()), address("unknown-crash"));
        let longest = format!("==1==ERROR: AddressSanitizer: {} x\n", "a".repeat(64));
        assert_eq!(scan(longest.as_bytes()), address(&"a".repeat(64)));
        assert_eq!(
            scan(b"==7==ERROR: AddressSanitizer failed to allocate 0x10 (16) bytes of LargeMmapAllocator\n"),
            address("out-of-memory")
        );
    }

    #[test]
    fn undefined_kinds_follow_the_table() {
        let cases: &[(&str, &str)] = &[
            (
                "signed integer overflow: 1 + 2147483647 cannot be represented",
                "signed-integer-overflow",
            ),
            (
                "negation of -2147483648 cannot be represented in type 'int'",
                "signed-integer-overflow",
            ),
            (
                "division of -2147483648 by -1 cannot be represented in type 'int'",
                "signed-integer-overflow",
            ),
            ("division by zero", "division-by-zero"),
            (
                "shift exponent 40 is too large for 32-bit type 'int'",
                "shift-exponent",
            ),
            ("left shift of negative value -1", "shift-base"),
            (
                "null pointer passed as argument 1, which is declared to never be null",
                "nonnull-attribute",
            ),
            (
                "null pointer returned from function declared to never return null",
                "returns-nonnull-attribute",
            ),
            (
                "load of misaligned address 0x1 for type 'int'",
                "misaligned-address",
            ),
            ("load of null pointer of type 'int'", "null-pointer-use"),
            (
                "member call on null pointer of type 'struct A'",
                "null-pointer-use",
            ),
            ("applying zero offset to null pointer", "pointer-overflow"),
            (
                "pointer index expression with base 0x1 overflowed",
                "pointer-overflow",
            ),
            ("index 10 out of bounds for type 'int [5]'", "index-out-of-bounds"),
            (
                "variable length array bound evaluates to non-positive value 0",
                "vla-bound",
            ),
            (
                "execution reached the end of a value-returning function without returning a value",
                "missing-return",
            ),
            ("execution reached an unreachable program point", "unreachable"),
            (
                "1e+10 is outside the range of representable values of type 'int'",
                "float-cast-overflow",
            ),
            (
                "load of value 7, which is not a valid value for type 'bool'",
                "invalid-bool-value",
            ),
            (
                "load of value 9, which is not a valid value for type 'Colour'",
                "invalid-enum-value",
            ),
            (
                "passing zero to __builtin_ctz(), which is not a valid argument",
                "invalid-builtin-use",
            ),
            (
                "member access within address 0x1 with insufficient space for an object of type 'A'",
                "object-size",
            ),
            (
                "downcast of address 0x1 which does not point to an object of type 'B'",
                "dynamic-type-mismatch",
            ),
            (
                "implicit conversion from type 'int' of value -1 changed the value",
                "implicit-conversion",
            ),
            ("something new", "undefined-behavior"),
        ];
        for &(message, kind) in cases {
            let line = format!("prog.cpp:4:12: runtime error: {message}\r\n");
            assert_eq!(scan(line.as_bytes()), undefined(kind), "{message:?}");
        }
    }

    #[test]
    fn undefined_locations_are_checked() {
        for location in [
            "a.cpp:3",
            "a.cpp:3:7",
            "/home/ada/my project/main.cpp:12:5",
            "<unknown>",
            "(prog+0x1234)",
        ] {
            let line = format!("{location}: runtime error: division by zero\n");
            assert_eq!(
                scan(line.as_bytes()),
                undefined("division-by-zero"),
                "{location:?}"
            );
        }
        assert_eq!(
            scan(b"runtime error: division by zero\n"),
            undefined("division-by-zero")
        );
        for line in [
            "Error: runtime error: bad input\n",
            ":3: runtime error: x\n",
            "a.cpp:: runtime error: x\n",
            "a.cpp:x1: runtime error: x\n",
            "()\x3a runtime error: x\n",
            "my runtime error: x\n",
            "  a.cpp:3:4: runtime error: indented\n",
        ] {
            assert_eq!(scan(line.as_bytes()), None, "{line:?}");
        }
    }

    #[test]
    fn markers_must_start_the_line() {
        for line in [
            "x==1==ERROR: AddressSanitizer: heap-buffer-overflow\n",
            " ==1==ERROR: AddressSanitizer: heap-buffer-overflow\n",
            "====ERROR: AddressSanitizer: heap-buffer-overflow\n",
            "==12345678901==ERROR: AddressSanitizer: heap-buffer-overflow\n",
            "==1==ERROR: LeakSanitizer: detected memory leaks\n",
            "==1==WARNING: AddressSanitizer failed to allocate 0x10 bytes\n",
            "==1==ERROR: AddressSanitizerX: heap-buffer-overflow\n",
            "SUMMARY: AddressSanitizer: heap-buffer-overflow main.cpp:2 in main\n",
        ] {
            assert_eq!(scan(line.as_bytes()), None, "{line:?}");
        }
    }

    #[test]
    fn the_first_report_wins() {
        let output = b"==1==ERROR: AddressSanitizer: stack-overflow on address 1\n\
            a.cpp:1:1: runtime error: division by zero\n";
        assert_eq!(scan(output), address("stack-overflow"));
        let output = b"a.cpp:1:1: runtime error: division by zero\n\
            ==1==ERROR: AddressSanitizer: stack-overflow on address 1\n";
        assert_eq!(scan(output), undefined("division-by-zero"));
    }

    #[test]
    fn a_last_line_without_a_break_counts() {
        let mut detector = Detector::new();
        detector.feed(b"output\n==9==ERROR: AddressSanitizer: heap-buf");
        // The unfinished line is read as it stands.
        assert_eq!(detector.report(), address("heap-buf"));
        detector.feed(b"fer-overflow on 0x1");
        assert_eq!(detector.report(), address("heap-buffer-overflow"));
        detector.feed(b"\r\nmore output");
        assert_eq!(detector.report(), address("heap-buffer-overflow"));
        assert_eq!(
            scan(b"a.cpp:2:3: runtime error: division by zero"),
            undefined("division-by-zero")
        );
        assert_eq!(scan(b""), None);
    }

    #[test]
    fn escape_sequences_and_controls_are_skipped() {
        // OSC with BEL and with ST, a two-byte escape, CAN cancelling a CSI,
        // backspace and bell characters.
        let output = b"\x1b]0;title\x07\x1b]8;;http://x\x1b\\=\x1b7=1\x1b[1;3\x18==ERROR: Addr\x08essSan\x07itizer: heap-use-after-free\n";
        assert_eq!(scan(output), address("heap-use-after-free"));
        // An escape sequence the program never finished ends at the line break.
        let output = b"\x1b]unterminated title\n==1==ERROR: AddressSanitizer: segv\n";
        assert_eq!(scan(output), address("segv"));
        let output = b"\x1b[unterminated\n==1==ERROR: AddressSanitizer: segv\n";
        assert_eq!(scan(output), address("segv"));
        // ESC inside a string sequence that is not ST starts a new sequence.
        let output = b"\x1b]x\x1b[31m==1==ERROR: AddressSanitizer: fpe\n";
        assert_eq!(scan(output), address("fpe"));
    }

    #[test]
    fn long_lines_keep_only_their_start() {
        let mut detector = Detector::new();
        detector.feed(&vec![b'x'; 10 * MAX_LINE_BYTES]);
        assert_eq!(detector.pending_line_len(), MAX_LINE_BYTES);
        detector.feed(b"==1==ERROR: AddressSanitizer: segv\n");
        assert_eq!(
            detector.report(),
            None,
            "the marker is not at the start of the line"
        );
        assert_eq!(detector.pending_line_len(), 0);

        // A location so long that the marker is past the kept part.
        let line = format!(
            "{}.cpp:1:1: runtime error: division by zero\n",
            "d".repeat(MAX_LINE_BYTES)
        );
        assert_eq!(scan(line.as_bytes()), None);
        let line = format!(
            "{}.cpp:1:1: runtime error: division by zero\n",
            "d".repeat(MAX_LINE_BYTES - 64)
        );
        assert_eq!(scan(line.as_bytes()), undefined("division-by-zero"));
    }

    #[test]
    fn nothing_is_scanned_after_a_report() {
        let mut detector = Detector::new();
        detector.feed(b"==1==ERROR: AddressSanitizer: segv\npartial line");
        assert_eq!(detector.pending_line_len(), 0);
        assert_eq!(detector.report(), address("segv"));
        assert_eq!(Tool::Undefined.to_string(), "UndefinedBehaviorSanitizer");
        assert_eq!(
            undefined("shift-base").map(|r| r.summary()).as_deref(),
            Some("Crashed: shift-base (UndefinedBehaviorSanitizer)")
        );
    }

    /// Splits `bytes` at the given (sorted, deduplicated) positions.
    fn chunks(bytes: &[u8], mut cuts: Vec<usize>) -> Vec<&[u8]> {
        cuts.retain(|&cut| cut <= bytes.len());
        cuts.sort_unstable();
        cuts.dedup();
        let mut pieces = Vec::new();
        let mut start = 0;
        for cut in cuts {
            pieces.push(&bytes[start..cut]);
            start = cut;
        }
        pieces.push(&bytes[start..]);
        pieces
    }

    /// Output that is likely to contain reports, escapes and long lines.
    fn output() -> impl Strategy<Value = Vec<u8>> {
        let piece = prop_oneof![
            Just(b"==1==ERROR: AddressSanitizer: ".to_vec()),
            Just(b"a.cpp:1:2: runtime error: ".to_vec()),
            Just(b"division by zero".to_vec()),
            Just(b"heap-buffer-overflow ".to_vec()),
            Just(b"\x1b[1m\x1b[31m".to_vec()),
            Just(b"\x1b]0;t\x07".to_vec()),
            Just(b"\r\n".to_vec()),
            Just(vec![b'z'; MAX_LINE_BYTES]),
            proptest::collection::vec(any::<u8>(), 0..32),
        ];
        proptest::collection::vec(piece, 0..12).prop_map(|pieces| pieces.concat())
    }

    proptest! {
        #[test]
        fn chunking_never_changes_the_result(
            bytes in output(),
            cuts in proptest::collection::vec(0usize..20_000, 0..8),
        ) {
            let whole = scan(&bytes);
            let mut detector = Detector::new();
            for piece in chunks(&bytes, cuts) {
                detector.feed(piece);
                prop_assert!(detector.pending_line_len() <= MAX_LINE_BYTES);
            }
            prop_assert_eq!(detector.report(), whole);
        }

        #[test]
        fn kinds_always_have_the_format(bytes in output()) {
            if let Some(report) = scan(&bytes) {
                prop_assert!(is_kind(report.kind.as_bytes()), "{:?}", report.kind);
            }
        }

        #[test]
        fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
            let mut detector = Detector::new();
            detector.feed(&bytes);
            prop_assert!(detector.pending_line_len() <= MAX_LINE_BYTES);
            if let Some(report) = detector.report() {
                prop_assert!(is_kind(report.kind.as_bytes()));
            }
        }
    }
}
