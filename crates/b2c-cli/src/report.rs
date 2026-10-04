//! Rendering diagnostics for people (`text`) and tools (`json`).
//!
//! Everything written to the terminal passes through [`terminal_safe`] first:
//! project files are untrusted, and a diagnostic may quote their text, so
//! control characters must never reach the terminal raw (they could move the
//! cursor, rewrite earlier output or change the window title).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use b2c_ir::{Diagnostic, Location, Part, Severity};
use b2c_model::{Block, Document, Input};
use serde::Serialize;

/// Version of the `--format json` output. Bump on any incompatible change.
pub(crate) const JSON_FORMAT_VERSION: u32 = 1;

/// Replaces control characters (C0 except newline and tab, DEL, C1) and
/// invisible formatting characters (bidi controls, zero-width characters)
/// with visible `\u{…}` escapes.
pub(crate) fn terminal_safe(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let unsafe_char = (c.is_control() && c != '\n' && c != '\t') || is_invisible_format(c);
        if unsafe_char {
            let _ = write!(out, "\\u{{{:04X}}}", u32::from(c));
        } else {
            out.push(c);
        }
    }
    out
}

/// [`terminal_safe`] for JSON text: each unsafe character (outside the JSON
/// structure they can only occur inside strings) becomes a `\uXXXX` escape,
/// so the text still means the same JSON value.
pub(crate) fn json_terminal_safe(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        if (c.is_control() && c != '\n' && c != '\t') || is_invisible_format(c) {
            let mut units = [0_u16; 2];
            for unit in c.encode_utf16(&mut units) {
                let _ = write!(out, "\\u{unit:04x}");
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Characters that change how text is displayed without being visible
/// (bidirectional controls, zero-width characters, word joiner, BOM, tags).
fn is_invisible_format(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{061C}' | '\u{180E}'
        | '\u{200B}'..='\u{200F}'
        | '\u{2028}'..='\u{202E}'
        | '\u{2060}'..='\u{206F}'
        | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}'
        | '\u{E0000}'..='\u{E007F}')
}

/// Block IDs to block type IDs, for friendlier locations.
pub(crate) struct BlockIndex {
    types: BTreeMap<String, String>,
}

impl BlockIndex {
    /// Indexes every block in the document (nested ones included).
    pub(crate) fn new(document: Option<&Document>) -> Self {
        let mut types = BTreeMap::new();
        if let Some(document) = document {
            for module in &document.modules {
                let mut stack: Vec<&Block> = module.workspace.blocks.iter().collect();
                while let Some(block) = stack.pop() {
                    types.insert(block.id.as_str().to_owned(), block.block_type.clone());
                    for input in block.inputs.values() {
                        if let Input::Block(child) = input {
                            stack.push(&child.block);
                        }
                    }
                    for list in block.statements.values() {
                        stack.extend(list.iter());
                    }
                }
            }
        }
        Self { types }
    }

    fn block_type(&self, id: &str) -> Option<&str> {
        self.types.get(id).map(String::as_str)
    }
}

fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}

fn describe_location(location: &Location, index: &BlockIndex) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(module) = &location.module {
        parts.push(format!("module {}", module.as_str()));
    }
    if let Some(block) = &location.block {
        match index.block_type(block.as_str()) {
            Some(block_type) => parts.push(format!("block {} ({block_type})", block.as_str())),
            None => parts.push(format!("block {}", block.as_str())),
        }
    }
    match &location.part {
        Part::Whole => {}
        Part::Field { name } => parts.push(format!("field {name}")),
        Part::Input { name } => parts.push(format!("input {name}")),
        Part::Tokens { input, start, end } => parts.push(format!("input {input}, tokens {start}..{end}")),
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// Renders diagnostics as plain text, one block per diagnostic, followed by
/// a one-line summary when there are any.
pub(crate) fn render_text(file: &str, diagnostics: &[Diagnostic], index: &BlockIndex) -> String {
    let mut out = String::new();
    for diagnostic in diagnostics {
        let _ = writeln!(
            out,
            "{}[{}]: {}",
            severity_label(diagnostic.severity),
            diagnostic.code.0,
            diagnostic.message
        );
        match describe_location(&diagnostic.primary, index) {
            Some(location) => {
                let _ = writeln!(out, "  --> {file}: {location}");
            }
            None => {
                let _ = writeln!(out, "  --> {file}");
            }
        }
        for related in &diagnostic.related {
            let location =
                describe_location(&related.location, index).unwrap_or_else(|| String::from("project"));
            let _ = writeln!(out, "  note: {} ({location})", related.message);
        }
        if let Some(raw) = &diagnostic.raw {
            for line in raw.lines() {
                let _ = writeln!(out, "  | {line}");
            }
        }
    }
    let count = |severity| diagnostics.iter().filter(|d| d.severity == severity).count();
    let (errors, warnings) = (count(Severity::Error), count(Severity::Warning));
    if errors + warnings > 0 {
        let _ = writeln!(out, "{}", summary(errors, warnings));
    }
    terminal_safe(&out)
}

fn summary(errors: usize, warnings: usize) -> String {
    let plural = |n: usize, word: &str| {
        if n == 1 {
            format!("1 {word}")
        } else {
            format!("{n} {word}s")
        }
    };
    match (errors, warnings) {
        (0, w) => plural(w, "warning"),
        (e, 0) => plural(e, "error"),
        (e, w) => format!("{} and {}", plural(e, "error"), plural(w, "warning")),
    }
}

#[derive(Serialize)]
struct JsonReport<'a> {
    version: u32,
    file: &'a str,
    ok: bool,
    diagnostics: &'a [Diagnostic],
}

/// Renders diagnostics as one JSON object:
/// `{"version":1,"file":…,"ok":…,"diagnostics":[…]}` plus a newline.
pub(crate) fn render_json(file: &str, diagnostics: &[Diagnostic]) -> String {
    let report = JsonReport {
        version: JSON_FORMAT_VERSION,
        file,
        ok: !b2c_ir::has_errors(diagnostics),
        diagnostics,
    };
    // Serialising plain data structures cannot fail; fall back to an empty
    // object rather than panicking if it ever did.
    let mut json = serde_json::to_string(&report).unwrap_or_else(|_| String::from("{}"));
    json.push('\n');
    json
}

#[derive(Serialize)]
struct ToolchainsJson<'a> {
    version: u32,
    toolchains: &'a [b2c_build::ToolchainReport],
}

/// Renders `b2c toolchains --format json`:
/// `{"version":1,"toolchains":[…]}` plus a newline.
pub(crate) fn render_toolchains_json(toolchains: &[b2c_build::ToolchainReport]) -> String {
    let report = ToolchainsJson {
        version: JSON_FORMAT_VERSION,
        toolchains,
    };
    let mut json = serde_json::to_string(&report).unwrap_or_else(|_| String::from("{}"));
    json.push('\n');
    json
}

/// Renders `b2c toolchains` as text: one line per compiler, followed by its
/// problems.
pub(crate) fn render_toolchains_text(toolchains: &[b2c_build::ToolchainReport]) -> String {
    let mut out = String::new();
    if toolchains.is_empty() {
        out.push_str(
            "No g++ was found. Install GCC 11 or newer (on Windows, MSYS2 UCRT64 or WinLibs) and make sure \
             g++ is on PATH, or pass --toolchain <path to g++>.\n",
        );
    }
    for toolchain in toolchains {
        let mut details = Vec::new();
        if let Some(version) = &toolchain.version {
            details.push(format!("GCC {version}"));
        }
        if let Some(target) = &toolchain.target {
            details.push(target.clone());
        }
        if !toolchain.standards.is_empty() {
            details.push(toolchain.standards.join(", "));
        }
        let state = if toolchain.usable { "ready" } else { "not usable" };
        let _ = writeln!(
            out,
            "{}  {}  ({state})",
            toolchain.path.display(),
            details.join("  ")
        );
        for problem in &toolchain.problems {
            let _ = writeln!(
                out,
                "  {}[{}]: {}",
                severity_label(problem.severity),
                problem.code.0,
                problem.message
            );
        }
    }
    terminal_safe(&out)
}

#[cfg(test)]
mod tests {
    use b2c_ir::{BlockId, DiagSource, ModuleId};

    use super::*;

    #[test]
    fn control_and_bidi_characters_are_escaped() {
        assert_eq!(terminal_safe("a\u{1b}[2Jb"), "a\\u{001B}[2Jb");
        assert_eq!(terminal_safe("x\u{202E}y\u{200B}z"), "x\\u{202E}y\\u{200B}z");
        assert_eq!(terminal_safe("\u{9b}31m"), "\\u{009B}31m");
        assert_eq!(terminal_safe("line\nnext\ttab é ✓"), "line\nnext\ttab é ✓");
        assert_eq!(terminal_safe("cr\rlf"), "cr\\u{000D}lf");
    }

    #[test]
    fn json_keeps_its_meaning_but_cannot_drive_the_terminal() {
        let value = serde_json::json!({"name": "Hi\u{9b}2J\u{202E}\u{E0041}\u{2028}ok", "n": 1});
        let json = serde_json::to_string_pretty(&value).unwrap();
        let safe = json_terminal_safe(&json);
        assert!(safe.chars().all(|c| c == '\n' || !c.is_control()), "{safe}");
        assert!(
            safe.contains("Hi\\u009b2J\\u202e\\udb40\\udc41\\u2028ok"),
            "{safe}"
        );
        assert_eq!(serde_json::from_str::<serde_json::Value>(&safe).unwrap(), value);
        assert_eq!(
            json_terminal_safe("{\n  \"a\": \"é ✓\"\n}"),
            "{\n  \"a\": \"é ✓\"\n}"
        );
    }

    #[test]
    fn text_report_names_the_block() {
        let module = ModuleId::new("mod_main").unwrap();
        let block = BlockId::new("b007").unwrap();
        let diagnostic = Diagnostic::error(
            "B2C-E0201",
            DiagSource::Analyser,
            Location::block(Some(module), block).with_part(Part::Field {
                name: String::from("VAR"),
            }),
            "There is no variable called \u{1b}x.",
        );
        let text = render_text("demo.b2c", &[diagnostic], &BlockIndex::new(None));
        assert_eq!(
            text,
            "error[B2C-E0201]: There is no variable called \\u{001B}x.\n  --> demo.b2c: module mod_main, block b007, field VAR\n1 error\n"
        );
    }

    #[test]
    fn json_report_is_versioned() {
        let json = render_json("demo.b2c", &[]);
        assert_eq!(
            json,
            "{\"version\":1,\"file\":\"demo.b2c\",\"ok\":true,\"diagnostics\":[]}\n"
        );
    }

    #[test]
    fn summaries_are_pluralised() {
        assert_eq!(summary(1, 0), "1 error");
        assert_eq!(summary(0, 2), "2 warnings");
        assert_eq!(summary(2, 1), "2 errors and 1 warning");
    }
}
