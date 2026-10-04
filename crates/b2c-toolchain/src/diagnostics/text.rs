//! The plain-text parser: GCC's `-fdiagnostics-plain-output` and the
//! linker's messages. Hand-written, allocation-bounded, never panics.

use super::{
    CompilerMessage, MAX_INCLUDE_DEPTH, MAX_INPUT_BYTES, MessageOrigin, MessageSeverity, ParsedOutput,
    SourcePos, clean_message, split_option,
};

/// Severity markers, longest first so `fatal error` wins over `error`.
const MARKERS: &[(&str, MessageSeverity)] = &[
    (": internal compiler error: ", MessageSeverity::InternalError),
    (": sorry, unimplemented: ", MessageSeverity::Error),
    (": fatal error: ", MessageSeverity::Fatal),
    (": error: ", MessageSeverity::Error),
    (": warning: ", MessageSeverity::Warning),
    (": note: ", MessageSeverity::Note),
];

/// Linker programs, by file name without `.exe`.
const LINKERS: &[&str] = &[
    "ld", "ld.bfd", "ld.gold", "ld.lld", "ld64.lld", "lld", "mold", "collect2",
];

/// Parses compiler and linker text output.
///
/// Recognised lines:
///
/// * `file:line:col: severity: message [-Woption]` (also without column or
///   line, and `tool: severity: message` for `g++`, `cc1plus` and
///   `collect2`);
/// * `note:` lines, which become children of the message before them;
/// * `file: In function 'f':` (and member function, constructor, lambda,
///   `At global scope:`), which set the function of following messages;
/// * `file: In instantiation of '…':` and `file:line:col:   required from
///   here` lines, which become context children of the next message;
/// * `In file included from a:1,` / `from b:2:` chains;
/// * linker lines: `ld: obj.o: in function 'main':`, `file:(.text+0x1d):
///   undefined reference to 'f(int)'`, `ld: cannot find -lfoo`, and
///   `collect2: error: ld returned 1 exit status`.
///
/// Anything else (source snippets, `compilation terminated.`) is ignored.
pub fn parse_text(text: &str) -> ParsedOutput {
    let mut parser = Parser::default();
    let mut end = text.len().min(MAX_INPUT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    parser.out.truncated = end < text.len();
    for line in text.get(..end).unwrap_or_default().lines() {
        parser.line(line.strip_suffix('\r').unwrap_or(line));
    }
    parser.finish()
}

#[derive(Default)]
struct Parser {
    out: ParsedOutput,
    /// The last top-level message (notes attach to it).
    current: Option<CompilerMessage>,
    /// `In file included from` chain for the next message.
    includes: Vec<SourcePos>,
    /// Whether the include chain is still being continued with `from`.
    in_include_chain: bool,
    /// Function context from `file: In function 'f':`, with its file.
    function: Option<(String, String)>,
    /// Context lines waiting for the message they describe.
    context: Vec<CompilerMessage>,
    /// Function named by the linker's `in function 'f':`.
    linker_function: Option<String>,
    /// The last note ended with `:` (`template argument
    /// deduction/substitution failed:`), so the next error explains it and
    /// belongs to the current message, as in GCC's JSON and SARIF output.
    explaining: bool,
}

impl Parser {
    fn line(&mut self, line: &str) {
        if line.trim().is_empty() {
            return;
        }
        if self.include_line(line) {
            return;
        }
        self.in_include_chain = false;
        if self.linker_line(line) {
            return;
        }
        if self.diagnostic_line(line) {
            return;
        }
        // Context lines ("In function", "In file included from") are
        // recorded; anything else (snippets, carets, "compilation
        // terminated.") is ignored.
        self.context_line(line);
    }

    fn finish(mut self) -> ParsedOutput {
        self.flush();
        if let Some(last) = self.out.messages.last_mut() {
            for note in std::mem::take(&mut self.context) {
                last.push_child(note);
            }
        }
        self.out
    }

    /// Moves the current message to the output.
    fn flush(&mut self) {
        if let Some(message) = self.current.take() {
            self.out.push(message);
        }
    }

    /// `In file included from a.hpp:3:5,` and `                 from b.cpp:1:`.
    fn include_line(&mut self, line: &str) -> bool {
        let rest = if let Some(rest) = line.strip_prefix("In file included from ") {
            self.includes.clear();
            self.in_include_chain = true;
            rest
        } else if self.in_include_chain
            && line.starts_with(' ')
            && let Some(rest) = line.trim_start().strip_prefix("from ")
        {
            rest
        } else {
            return false;
        };
        let rest = rest.trim_end();
        let rest = rest.strip_suffix([',', ':']).unwrap_or(rest);
        if let Some(pos) = parse_pos(rest)
            && self.includes.len() < MAX_INCLUDE_DEPTH
        {
            self.includes.push(pos);
        }
        true
    }

    /// Linker output (see [`parse_text`]).
    fn linker_line(&mut self, line: &str) -> bool {
        let rest = match line.split_once(": ") {
            Some((tool, rest)) if is_linker(tool) => rest,
            _ if has_section_reference(line) => line,
            _ => return false,
        };
        // `obj.o: in function 'main':` sets the function for what follows.
        if let Some(function) = linker_function_context(rest) {
            self.linker_function = Some(function);
            return true;
        }
        let (severity, text, location) = if let Some(text) = rest.strip_prefix("error: ") {
            (MessageSeverity::Error, text, None)
        } else if let Some(text) = rest.strip_prefix("warning: ") {
            (MessageSeverity::Warning, text, None)
        } else if let Some((place, text)) = split_section_reference(rest) {
            match text.strip_prefix("warning: ") {
                Some(text) => (MessageSeverity::Warning, text, linker_pos(place)),
                None => (MessageSeverity::Error, text, linker_pos(place)),
            }
        } else {
            (MessageSeverity::Error, rest, None)
        };
        self.push_linker(severity, text, location)
    }

    fn push_linker(&mut self, severity: MessageSeverity, text: &str, location: Option<SourcePos>) -> bool {
        self.flush();
        let mut message = CompilerMessage::new(MessageOrigin::Linker, severity, text);
        message.symbol = quoted_symbol(&message.message);
        message.location = location;
        message.function.clone_from(&self.linker_function);
        self.out.push(message);
        true
    }

    /// `file:line:col: severity: message`, `tool: severity: message` and
    /// `file:line:col:   required from here`.
    fn diagnostic_line(&mut self, line: &str) -> bool {
        if let Some((pos, rest)) = split_location(line) {
            for &(marker, severity) in MARKERS {
                // The marker without its leading ':' (the location ate it).
                if let Some(text) = rest.strip_prefix(marker.get(1..).unwrap_or(marker)) {
                    self.push_diagnostic(MessageOrigin::Compiler, severity, Some(pos), text);
                    return true;
                }
            }
            if rest.starts_with("  ") {
                // A context line of a template instantiation.
                let mut note = CompilerMessage::new(MessageOrigin::Compiler, MessageSeverity::Note, rest);
                note.location = Some(pos);
                self.context.push(note);
                return true;
            }
            return false;
        }
        let Some((index, marker, severity)) = MARKERS
            .iter()
            .filter_map(|&(marker, severity)| line.find(marker).map(|index| (index, marker, severity)))
            .min_by_key(|&(index, ..)| index)
        else {
            return false;
        };
        let tool = line.get(..index).unwrap_or_default();
        let text = line.get(index + marker.len()..).unwrap_or_default();
        if is_linker(tool) {
            return self.push_linker(severity, text, None);
        }
        let origin = if is_driver(tool) {
            MessageOrigin::Driver
        } else {
            MessageOrigin::Compiler
        };
        self.push_diagnostic(origin, severity, None, text);
        true
    }

    fn push_diagnostic(
        &mut self,
        origin: MessageOrigin,
        severity: MessageSeverity,
        location: Option<SourcePos>,
        text: &str,
    ) {
        let (text, option) = split_option(text);
        let mut message = CompilerMessage::new(origin, severity, text);
        message.option = option;
        message.included_from = std::mem::take(&mut self.includes);
        if let (Some(pos), Some((file, function))) = (&location, &self.function)
            && pos.file == *file
        {
            message.function = Some(function.clone());
        }
        message.location = location;
        for note in std::mem::take(&mut self.context) {
            message.push_child(note);
        }
        let is_note = severity == MessageSeverity::Note;
        let nested = !is_note && self.explaining && self.current.is_some();
        self.explaining = is_note && message.message.ends_with(':');
        match self.current.as_mut() {
            Some(current) if is_note || nested => {
                if !current.push_child(message) {
                    self.out.truncated = true;
                }
            }
            _ if is_note => self.out.push(message),
            _ => {
                self.flush();
                self.current = Some(message);
            }
        }
    }

    /// `file: In function 'f':`, `file: At global scope:` and
    /// `file: In instantiation of '…':`.
    fn context_line(&mut self, line: &str) -> bool {
        let Some((file, rest)) = split_file_prefix(line) else {
            return false;
        };
        let Some(text) = rest.strip_suffix(':') else {
            return false;
        };
        if text == "At global scope" {
            self.function = None;
            return true;
        }
        if !text.starts_with("In ") {
            return false;
        }
        if is_function_context(text) {
            let name = quoted(text).unwrap_or(text);
            self.function = Some((file.to_owned(), clean_message(name)));
        } else {
            // "In instantiation of …", "In substitution of …": context for
            // the next message.
            let mut note = CompilerMessage::new(MessageOrigin::Compiler, MessageSeverity::Note, text);
            note.function = self.function.as_ref().map(|(_, function)| function.clone());
            self.context.push(note);
        }
        true
    }
}

/// Whether a context line names the enclosing function: `In function`,
/// `In member function`, `In copy constructor`, `In lambda function`, …
/// (but not `In instantiation of` or `In substitution of`).
fn is_function_context(text: &str) -> bool {
    let head = text.split(['\'', '\u{2018}']).next().unwrap_or(text);
    text == "In lambda function"
        || ["function ", "constructor ", "destructor "]
            .iter()
            .any(|kind| head.ends_with(kind))
}

/// Splits `file:line[:col]:` off the start of a line. Returns the position
/// and the rest after the last `:` (starting with a space for real
/// diagnostics). A Windows drive prefix (`C:\` or `C:/`) is not taken for a
/// separator.
fn split_location(line: &str) -> Option<(SourcePos, &str)> {
    let skip = usize::from(is_drive_prefix(line)) * 2;
    let mut search = skip;
    while let Some(offset) = line.get(search..)?.find(':') {
        let colon = search + offset;
        if let Some((number, after)) = digits_then_colon(line, colon + 1)
            && colon > 0
        {
            let file = line.get(..colon)?;
            let (column, rest) = match digits_then_colon(line, after) {
                Some((column, after_column)) => (Some(column), line.get(after_column..)?),
                None => (None, line.get(after..)?),
            };
            if number == 0 || file.starts_with(char::is_whitespace) {
                return None;
            }
            return Some((
                SourcePos {
                    file: file.to_owned(),
                    line: number,
                    column,
                },
                rest,
            ));
        }
        search = colon + 1;
    }
    None
}

/// Reads ASCII digits starting at `start`, which must be followed by `:`.
/// Returns the number and the index after the `:`.
fn digits_then_colon(line: &str, start: usize) -> Option<(u32, usize)> {
    let rest = line.get(start..)?;
    let length = rest.bytes().take_while(u8::is_ascii_digit).count();
    if length == 0 || length > 9 || rest.as_bytes().get(length) != Some(&b':') {
        return None;
    }
    let number = rest.get(..length)?.parse().ok()?;
    Some((number, start + length + 1))
}

/// Parses a whole `file:line[:col]` string.
fn parse_pos(text: &str) -> Option<SourcePos> {
    let (head, last) = text.rsplit_once(':')?;
    let last_number: u32 = parse_number(last)?;
    if let Some((file, middle)) = head.rsplit_once(':')
        && let Some(line) = parse_number(middle)
        && !file.is_empty()
        && !(file.len() == 1 && is_drive_prefix(text))
    {
        return Some(SourcePos {
            file: file.to_owned(),
            line,
            column: Some(last_number),
        });
    }
    if head.is_empty() || (head.len() == 1 && is_drive_prefix(text)) {
        return None;
    }
    Some(SourcePos {
        file: head.to_owned(),
        line: last_number,
        column: None,
    })
}

fn parse_number(text: &str) -> Option<u32> {
    if text.is_empty() || text.len() > 9 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok().filter(|&n| n > 0)
}

/// `C:\…` or `C:/…`.
fn is_drive_prefix(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 3
        && bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(1) == Some(&b':')
        && matches!(bytes.get(2), Some(b'\\' | b'/'))
}

/// Splits `file: rest` for context lines; the file must not be empty.
fn split_file_prefix(line: &str) -> Option<(&str, &str)> {
    let skip = usize::from(is_drive_prefix(line)) * 2;
    let index = skip + line.get(skip..)?.find(": ")?;
    let file = line.get(..index)?;
    if file.is_empty() || file.starts_with(' ') {
        return None;
    }
    Some((file, line.get(index + 2..)?))
}

/// The text between the first opening quote and the last closing quote.
fn quoted(text: &str) -> Option<&str> {
    let start = text.find(['\'', '\u{2018}', '`'])?;
    let open_len = text.get(start..)?.chars().next()?.len_utf8();
    let end = text.rfind(['\'', '\u{2019}'])?;
    if end <= start {
        return None;
    }
    text.get(start + open_len..end)
}

/// The symbol in `undefined reference to 'f(int)'` and similar.
fn quoted_symbol(message: &str) -> Option<String> {
    let lead = [
        "undefined reference to ",
        "multiple definition of ",
        "undefined symbol: ",
    ];
    let rest = lead
        .iter()
        .find_map(|lead| message.find(lead).map(|i| message.get(i + lead.len()..)))??;
    let rest = rest.split("; ").next().unwrap_or(rest);
    match quoted(rest) {
        Some(symbol) if !symbol.is_empty() => Some(symbol.to_owned()),
        _ => None,
    }
}

/// `obj.o: in function 'main':` gives `main`.
fn linker_function_context(rest: &str) -> Option<String> {
    let rest = rest.strip_suffix(':')?;
    let index = rest.find("in function ")?;
    let before = rest.get(..index)?;
    if !(before.is_empty() || before.ends_with(": ")) {
        return None;
    }
    quoted(rest.get(index..)?).map(clean_message)
}

/// Whether `tool` (text before the first `: `) names a linker.
fn is_linker(tool: &str) -> bool {
    let name = program_name(tool);
    LINKERS.contains(&name) || name.ends_with("-ld") || name.ends_with("-ld.bfd")
}

/// Whether `tool` names the compiler driver (`g++`, `gcc`, `x86_64-…-g++`).
fn is_driver(tool: &str) -> bool {
    let name = program_name(tool);
    ["g++", "gcc", "c++", "cc"].iter().any(|driver| {
        name == *driver
            || name.ends_with(&format!("-{driver}"))
            || name
                .strip_prefix(driver)
                .is_some_and(|version| version.starts_with('-'))
    })
}

/// The file name of a program path, without `.exe`, lower-cased on the
/// `.exe` comparison only.
fn program_name(tool: &str) -> &str {
    let name = tool.rsplit(['/', '\\']).next().unwrap_or(tool);
    name.strip_suffix(".exe")
        .or_else(|| name.strip_suffix(".EXE"))
        .unwrap_or(name)
}

/// Whether the line contains a linker section reference like
/// `:(.text+0x1d): `.
fn has_section_reference(line: &str) -> bool {
    split_section_reference(line).is_some()
}

/// Splits `place:(.section+0x1d): text` into `place` and `text`.
fn split_section_reference(text: &str) -> Option<(&str, &str)> {
    let open = text.find(":(.")?;
    let close = open + text.get(open..)?.find("): ")?;
    let section = text.get(open + 2..close)?;
    if section.contains(char::is_whitespace) {
        return None;
    }
    Some((text.get(..open)?, text.get(close + 3..)?))
}

/// The source position in a linker place: `main.cpp:5` (with debug
/// information) or `C:\x\cc.o:main.cpp:5`; plain `main.cpp` or `cc.o` gives
/// none.
fn linker_pos(place: &str) -> Option<SourcePos> {
    let (head, line) = place.rsplit_once(':')?;
    let line = parse_number(line)?;
    // `C:\tmp\cc.o:main.cpp` keeps only the part after the object file.
    let file = match head.rsplit_once(':') {
        Some((before, file)) if !(before.len() == 1 && is_drive_prefix(head)) => file,
        _ => head,
    };
    if file.is_empty() {
        return None;
    }
    Some(SourcePos {
        file: file.to_owned(),
        line,
        column: None,
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn one(text: &str) -> CompilerMessage {
        let parsed = parse_text(text);
        assert_eq!(parsed.messages.len(), 1, "{parsed:#?}");
        parsed.messages.into_iter().next().unwrap()
    }

    #[test]
    fn locations_split() {
        let (pos, rest) = split_location("src/a.cpp:13:15: error: x").unwrap();
        assert_eq!(
            (pos.file.as_str(), pos.line, pos.column, rest),
            ("src/a.cpp", 13, Some(15), " error: x")
        );
        let (pos, rest) = split_location(r"C:\b2c\gen\main.cpp:7:3: warning: y").unwrap();
        assert_eq!(
            (pos.file.as_str(), pos.line, pos.column, rest),
            (r"C:\b2c\gen\main.cpp", 7, Some(3), " warning: y")
        );
        let (pos, rest) = split_location("a.cpp:7: note: z").unwrap();
        assert_eq!((pos.line, pos.column, rest), (7, None, " note: z"));
        assert!(split_location("cc1plus: error: x").is_none());
        assert!(split_location("a.cpp:0:1: error: x").is_none());
        assert!(split_location(":1: error").is_none());
    }

    #[test]
    fn positions_parse() {
        assert_eq!(
            parse_pos("/usr/include/c++/13/iostream:41"),
            Some(SourcePos {
                file: String::from("/usr/include/c++/13/iostream"),
                line: 41,
                column: None
            })
        );
        assert_eq!(
            parse_pos(r"C:\x\a.hpp:3:5"),
            Some(SourcePos {
                file: String::from(r"C:\x\a.hpp"),
                line: 3,
                column: Some(5)
            })
        );
        assert_eq!(
            parse_pos(r"C:\x\a.hpp:3").map(|p| (p.line, p.column)),
            Some((3, None))
        );
        assert_eq!(parse_pos("nothing"), None);
        assert_eq!(parse_pos(":3"), None);
    }

    #[test]
    fn errors_warnings_and_options() {
        let message = one("main.cpp:3:5: error: 'x' was not declared in this scope\n");
        assert_eq!(message.severity, MessageSeverity::Error);
        assert_eq!(message.origin, MessageOrigin::Compiler);
        assert_eq!(message.message, "'x' was not declared in this scope");
        assert_eq!(message.option, None);

        let message = one("main.cpp:11:9: warning: unused variable 'unused' [-Wunused-variable]");
        assert_eq!(message.severity, MessageSeverity::Warning);
        assert_eq!(message.option.as_deref(), Some("-Wunused-variable"));

        let message = one(
            "main.cpp:1:10: fatal error: missing.hpp: No such file or directory\ncompilation terminated.\n",
        );
        assert_eq!(message.severity, MessageSeverity::Fatal);
        assert_eq!(message.message, "missing.hpp: No such file or directory");

        let message = one("main.cpp:2:1: internal compiler error: Segmentation fault");
        assert_eq!(message.severity, MessageSeverity::InternalError);
        let message = one("main.cpp:2:1: sorry, unimplemented: thing");
        assert_eq!(message.severity, MessageSeverity::Error);
    }

    #[test]
    fn notes_attach_to_the_previous_message() {
        let parsed = parse_text(
            "a.cpp:6:13: warning: declaration of 'int total' shadows a parameter [-Wshadow]\n\
             a.cpp:3:16: note: shadowed declaration is here\n\
             a.cpp:9:1: error: second\n",
        );
        assert_eq!(parsed.messages.len(), 2);
        let first = &parsed.messages[0];
        assert_eq!(first.children.len(), 1);
        assert_eq!(first.children[0].message, "shadowed declaration is here");
        assert_eq!(first.children[0].location.as_ref().map(|p| p.line), Some(3));
        assert!(parsed.messages[1].children.is_empty());
    }

    #[test]
    fn errors_explaining_a_note_are_nested() {
        let parsed = parse_text(
            "a.cpp:13:15: error: no match for 'operator<<'\n\
             o.h:801:5: note: candidate: 'template<class T> f(T)'\n\
             o.h:801:5: note:   template argument deduction/substitution failed:\n\
             o.h: In substitution of 'template<class T> f(T) [with T = P]':\n\
             a.cpp:13:18:   required from here\n\
             o.h:801:5: error: template constraint failure\n\
             o.h:801:5: note: constraints not satisfied\n\
             a.cpp:20:1: error: independent\n",
        );
        assert_eq!(parsed.messages.len(), 2, "{parsed:#?}");
        let children = &parsed.messages[0].children;
        assert_eq!(children.len(), 4);
        assert_eq!(children[2].severity, MessageSeverity::Error);
        assert_eq!(children[2].children.len(), 2);
        assert_eq!(parsed.messages[1].message, "independent");
    }

    #[test]
    fn a_note_without_a_message_stands_alone() {
        let message = one("a.cpp:1:1: note: lonely");
        assert_eq!(message.severity, MessageSeverity::Note);
    }

    #[test]
    fn function_context_applies_to_the_same_file() {
        let parsed = parse_text(
            "a.cpp: In member function 'void Game::step()':\n\
             a.cpp:4:2: error: e1\n\
             b.hpp:1:1: error: e2\n\
             a.cpp: At global scope:\n\
             a.cpp:9:1: error: e3\n",
        );
        let functions: Vec<_> = parsed.messages.iter().map(|m| m.function.as_deref()).collect();
        assert_eq!(functions, [Some("void Game::step()"), None, None]);
    }

    #[test]
    fn include_chains_attach_innermost_first() {
        let message = one(
            "In file included from /x/iostream:41,\n                 from src/a.cpp:1:\n\
             src/b.hpp:4:12: error: bad\n",
        );
        let chain: Vec<_> = message
            .included_from
            .iter()
            .map(|p| (p.file.as_str(), p.line))
            .collect();
        assert_eq!(chain, [("/x/iostream", 41), ("src/a.cpp", 1)]);
    }

    #[test]
    fn template_context_becomes_children_of_the_next_message() {
        let message = one(
            "src/t.cpp: In instantiation of 'T total(const std::vector<T>&) [with T = int]':\n\
             src/t.cpp:14:17:   required from here\n\
             src/t.cpp:7:22: error: request for member 'size' in 'value'\n",
        );
        assert_eq!(message.children.len(), 2);
        assert!(
            message.children[0]
                .message
                .starts_with("In instantiation of 'T total")
        );
        assert_eq!(message.children[1].message, "required from here");
        assert_eq!(
            message.children[1].location.as_ref().map(|p| (p.line, p.column)),
            Some((14, Some(17)))
        );
    }

    #[test]
    fn tool_messages() {
        let message = one("g++: fatal error: no input files");
        assert_eq!(message.origin, MessageOrigin::Driver);
        assert_eq!(message.severity, MessageSeverity::Fatal);
        let message = one(r"C:\msys64\ucrt64\bin\g++.exe: error: x.cpp: No such file or directory");
        assert_eq!(message.origin, MessageOrigin::Driver);
        let message = one("cc1plus: warning: command-line option '-Wx' is valid for C but not for C++");
        assert_eq!(message.origin, MessageOrigin::Compiler);
        assert!(message.location.is_none());
        let message = one("collect2: error: ld returned 1 exit status");
        assert_eq!(message.origin, MessageOrigin::Linker);
        assert_eq!(message.severity, MessageSeverity::Error);
    }

    #[test]
    fn linker_undefined_references() {
        let parsed = parse_text(
            "/usr/bin/ld: /tmp/ccdgW1cK.o: in function `main':\n\
             link.cpp:(.text+0x13): undefined reference to `say(char const*)'\n\
             /usr/bin/ld: link.cpp:(.text+0x1d): undefined reference to `twice(int)'\n\
             collect2: error: ld returned 1 exit status\n",
        );
        assert_eq!(parsed.messages.len(), 3, "{parsed:#?}");
        let symbols: Vec<_> = parsed.messages.iter().map(|m| m.symbol.as_deref()).collect();
        assert_eq!(symbols, [Some("say(char const*)"), Some("twice(int)"), None]);
        assert!(parsed.messages.iter().all(|m| m.origin == MessageOrigin::Linker));
        assert_eq!(parsed.messages[0].function.as_deref(), Some("main"));
        assert_eq!(
            parsed.messages[0].message,
            "undefined reference to `say(char const*)'"
        );
    }

    #[test]
    fn linker_lines_with_debug_information_have_lines() {
        let message = one("/usr/bin/ld: main.cpp:5:(.text+0x13): undefined reference to `twice(int)'");
        let pos = message.location.unwrap();
        assert_eq!((pos.file.as_str(), pos.line), ("main.cpp", 5));
        let message = one(
            r"C:/msys64/ucrt64/bin/../lib/gcc/x86_64-w64-mingw32/14.2.0/../../../../x86_64-w64-mingw32/bin/ld.exe: C:\Users\ada\AppData\Local\Temp\ccX.o:main.cpp:(.text+0x1d): undefined reference to `twice(int)'",
        );
        assert_eq!(message.symbol.as_deref(), Some("twice(int)"));
        assert!(message.location.is_none());
        let message = one(r"ld.exe: C:\t\cc.o:main.cpp:12:(.text+0x1d): undefined reference to `f()'");
        let pos = message.location.unwrap();
        assert_eq!((pos.file.as_str(), pos.line), ("main.cpp", 12));
    }

    #[test]
    fn linker_other_messages() {
        let message = one("/usr/bin/ld: cannot find -lsfml-graphics: No such file or directory");
        assert_eq!(message.origin, MessageOrigin::Linker);
        assert_eq!(
            message.message,
            "cannot find -lsfml-graphics: No such file or directory"
        );
        let message = one("/usr/bin/ld: warning: x.o: missing .note.GNU-stack section");
        assert_eq!(message.severity, MessageSeverity::Warning);
        let message = one(
            "/usr/bin/ld: b.o: in function `f()':\nb.cpp:(.text+0x0): multiple definition of `f()'; a.o:a.cpp:(.text+0x0): first defined here",
        );
        assert_eq!(message.symbol.as_deref(), Some("f()"));
        assert_eq!(message.function.as_deref(), Some("f()"));
        let message = one("x86_64-w64-mingw32-ld: cannot find -lfoo");
        assert_eq!(message.origin, MessageOrigin::Linker);
    }

    #[test]
    fn typographic_quotes_and_crlf_are_normalised() {
        let message = one("a.cpp:1:2: error: \u{2018}x\u{2019} was not declared in this scope\r\n");
        assert_eq!(message.message, "'x' was not declared in this scope");
    }

    #[test]
    fn snippets_and_noise_are_ignored() {
        let parsed = parse_text(
            "a.cpp:3:5: error: bad\n    3 |     x = 1;\n      |     ^\ncompilation terminated.\nrandom words\n",
        );
        assert_eq!(parsed.messages.len(), 1);
    }

    #[test]
    fn message_count_is_capped() {
        let text = "a.cpp:1:1: error: e\n".repeat(super::super::MAX_MESSAGES + 5);
        let parsed = parse_text(&text);
        assert_eq!(parsed.messages.len(), super::super::MAX_MESSAGES);
        assert!(parsed.truncated);
    }

    #[test]
    fn note_count_is_capped() {
        let mut text = String::from("a.cpp:1:1: error: e\n");
        text.push_str(&"a.cpp:2:2: note: n\n".repeat(super::super::MAX_CHILDREN + 5));
        let parsed = parse_text(&text);
        assert_eq!(parsed.messages[0].children.len(), super::super::MAX_CHILDREN);
        assert!(parsed.truncated);
    }

    proptest! {
        #[test]
        fn never_panics(text in "\\PC{0,400}") {
            let _ = parse_text(&text);
        }

        #[test]
        fn never_panics_on_structured_noise(
            lines in proptest::collection::vec(
                prop_oneof![
                    "[a-zA-Z:/\\\\. 0-9]{0,20}:[0-9]{0,3}:[0-9]{0,3}: (error|warning|note|fatal error): [ -~]{0,30}",
                    "In file included from [ -~]{0,20}",
                    " +from [ -~]{0,20}",
                    "[ -~]{0,20}: In (function|instantiation of) '[ -~]{0,10}':",
                    "[ -~]{0,20}:\\(\\.text\\+0x[0-9a-f]{1,3}\\): undefined reference to `[ -~]{0,10}'",
                    "/usr/bin/ld: [ -~]{0,30}",
                    "\\PC{0,30}",
                ],
                0..40,
            )
        ) {
            let parsed = parse_text(&lines.join("\n"));
            prop_assert!(parsed.messages.len() <= super::super::MAX_MESSAGES);
        }

        #[test]
        fn locations_round_trip(file in "[a-z][a-z0-9_/]{0,20}\\.cpp", line in 1_u32..100_000, column in 1_u32..1000) {
            let message = one(&format!("{file}:{line}:{column}: error: boom"));
            let pos = message.location.unwrap();
            prop_assert_eq!(pos.file, file);
            prop_assert_eq!(pos.line, line);
            prop_assert_eq!(pos.column, Some(column));
        }
    }
}
