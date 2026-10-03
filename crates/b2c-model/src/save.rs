//! Canonical serialisation and hashing (spec §5.2, §5.11).
//!
//! CONTRACT (implemented in milestone M1):
//! * `to_canonical_json`: UTF-8 JSON, 2-space indentation, `\n` line endings,
//!   one trailing newline; struct keys in declaration order, map keys sorted,
//!   top-level blocks of each module sorted by ID. Saving an unchanged document
//!   gives byte-identical output, and `load(to_canonical_json(d)) == d`.
//! * `content_hash`: SHA-256 of the canonical serialisation with layout-only
//!   data removed (block `x`/`y`/`collapsed`, comment `pinned`, workspace
//!   `frames`/`notes`/`viewport`), so moving blocks never changes the hash.
//!
//! The writer is hand-written instead of going through `serde_json` so that
//! the output cannot change when a dependency enables `serde_json`'s
//! `preserve_order` feature (free-form maps are sorted here explicitly), and
//! so that the hash can leave out layout keys without copying the document.
//! It follows the `serde` attributes of [`crate::document`] exactly (which
//! keys are skipped when empty or default); a test checks that its output is
//! byte-identical to `serde_json::to_string_pretty`.

use std::fmt::Write as _;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::document::{
    Block, BuildConfiguration, BuildSettings, DefineValue, Document, FieldValue, Input, Module, Project,
    Token, Workspace,
};

/// Serialises a document canonically.
pub fn to_canonical_json(document: &Document) -> String {
    let mut writer = Writer::new(false);
    writer.document(document);
    writer.finish()
}

/// SHA-256 of the document's semantic content.
pub fn content_hash(document: &Document) -> [u8; 32] {
    let mut writer = Writer::new(true);
    writer.document(document);
    Sha256::digest(writer.finish().as_bytes()).into()
}

/// A pretty-printing JSON writer in the style of `serde_json`'s
/// `PrettyFormatter` (2 spaces, `": "`, empty containers as `[]` / `{}`).
struct Writer {
    out: String,
    /// One entry per open container: whether it has no entries yet.
    open: Vec<bool>,
    /// Leave out layout-only keys (for the content hash).
    semantic: bool,
}

impl Writer {
    fn new(semantic: bool) -> Self {
        Self {
            out: String::new(),
            open: Vec::new(),
            semantic,
        }
    }

    fn finish(mut self) -> String {
        self.out.push('\n');
        self.out
    }

    fn indent(&mut self) {
        for _ in 0..self.open.len() {
            self.out.push_str("  ");
        }
    }

    /// Starts a new entry in the innermost container.
    fn entry(&mut self) {
        if let Some(first) = self.open.last_mut() {
            let was_first = std::mem::replace(first, false);
            self.out.push_str(if was_first { "\n" } else { ",\n" });
            self.indent();
        }
    }

    fn begin(&mut self, bracket: char) {
        self.out.push(bracket);
        self.open.push(true);
    }

    fn end(&mut self, bracket: char) {
        let empty = self.open.pop().unwrap_or(true);
        if !empty {
            self.out.push('\n');
            self.indent();
        }
        self.out.push(bracket);
    }

    fn key(&mut self, key: &str) {
        self.entry();
        self.string(key);
        self.out.push_str(": ");
    }

    /// Writes a JSON string with `serde_json`'s escaping: `"` and `\`, the
    /// short escapes for backspace, form feed, newline, carriage return and
    /// tab, `\u00XX` for other C0 controls; everything else as UTF-8.
    fn string(&mut self, text: &str) {
        self.out.push('"');
        for c in text.chars() {
            match c {
                '"' => self.out.push_str("\\\""),
                '\\' => self.out.push_str("\\\\"),
                '\u{8}' => self.out.push_str("\\b"),
                '\u{c}' => self.out.push_str("\\f"),
                '\n' => self.out.push_str("\\n"),
                '\r' => self.out.push_str("\\r"),
                '\t' => self.out.push_str("\\t"),
                c if c < ' ' => {
                    let _ = write!(self.out, "\\u{:04x}", u32::from(c));
                }
                c => self.out.push(c),
            }
        }
        self.out.push('"');
    }

    fn display(&mut self, value: impl std::fmt::Display) {
        let _ = write!(self.out, "{value}");
    }

    fn f64(&mut self, value: f64) {
        match serde_json::Number::from_f64(value) {
            Some(number) => self.display(number),
            None => self.out.push_str("null"),
        }
    }

    /// A unit enum variant, by its `serde` name.
    fn variant<T: Serialize>(&mut self, value: &T) {
        match serde_json::to_value(value) {
            Ok(serde_json::Value::String(name)) => self.string(&name),
            _ => self.out.push_str("null"),
        }
    }

    fn field_str(&mut self, key: &str, value: &str) {
        self.key(key);
        self.string(value);
    }

    fn field_display(&mut self, key: &str, value: impl std::fmt::Display) {
        self.key(key);
        self.display(value);
    }

    fn field_variant<T: Serialize>(&mut self, key: &str, value: &T) {
        self.key(key);
        self.variant(value);
    }

    fn string_list(&mut self, key: &str, items: &[String]) {
        self.key(key);
        self.begin('[');
        for item in items {
            self.entry();
            self.string(item);
        }
        self.end(']');
    }

    /// Free-form JSON with object keys sorted.
    fn value(&mut self, value: &serde_json::Value) {
        match value {
            serde_json::Value::Null => self.out.push_str("null"),
            serde_json::Value::Bool(b) => self.display(b),
            serde_json::Value::Number(n) => self.display(n),
            serde_json::Value::String(s) => self.string(s),
            serde_json::Value::Array(items) => {
                self.begin('[');
                for item in items {
                    self.entry();
                    self.value(item);
                }
                self.end(']');
            }
            serde_json::Value::Object(map) => {
                let mut entries: Vec<(&String, &serde_json::Value)> = map.iter().collect();
                entries.sort_by(|a, b| a.0.cmp(b.0));
                self.begin('{');
                for (key, item) in entries {
                    self.key(key);
                    self.value(item);
                }
                self.end('}');
            }
        }
    }

    // -----------------------------------------------------------------------
    // The document, following the serde attributes of `crate::document`.
    // -----------------------------------------------------------------------

    fn document(&mut self, document: &Document) {
        self.begin('{');
        self.field_str("format", &document.format);
        self.field_display("formatVersion", document.format_version);
        self.key("generator");
        self.begin('{');
        self.field_str("app", &document.generator.app);
        self.field_str("catalog", &document.generator.catalog);
        self.end('}');
        self.key("project");
        self.project(&document.project);
        self.key("modules");
        self.begin('[');
        for module in &document.modules {
            self.entry();
            self.module(module);
        }
        self.end(']');
        if let Some(ext) = &document.ext {
            self.key("x-ext");
            self.value(ext);
        }
        self.end('}');
    }

    fn project(&mut self, project: &Project) {
        self.begin('{');
        self.field_str("id", project.id.as_str());
        self.field_str("name", &project.name);
        if !project.description.is_empty() {
            self.field_str("description", &project.description);
        }
        self.key("language");
        self.begin('{');
        self.field_variant("standard", &project.language.standard);
        if project.language.gnu_extensions {
            self.field_display("gnuExtensions", true);
        }
        self.end('}');
        let options = &project.options;
        self.key("options");
        self.begin('{');
        self.field_display("showAdvanced", options.show_advanced);
        self.field_display("manualMemory", options.manual_memory);
        self.field_display("preferPlainStd", options.prefer_plain_std);
        self.field_variant("formattingStyle", &options.formatting_style);
        self.field_display("checkedIndexing", options.checked_indexing);
        self.end('}');
        self.key("build");
        self.build(&project.build);
        self.key("run");
        self.begin('{');
        if !project.run.args.is_empty() {
            self.string_list("args", &project.run.args);
        }
        self.field_variant("workingDirectory", &project.run.working_directory);
        self.end('}');
        self.end('}');
    }

    fn build(&mut self, build: &BuildSettings) {
        self.begin('{');
        self.key("configurations");
        self.begin('{');
        self.key("debug");
        self.configuration(&build.configurations.debug);
        self.key("release");
        self.configuration(&build.configurations.release);
        self.end('}');
        if !build.defines.is_empty() {
            self.key("defines");
            self.begin('[');
            for define in &build.defines {
                self.entry();
                self.begin('{');
                self.field_str("name", &define.name);
                self.key("value");
                self.begin('{');
                match &define.value {
                    DefineValue::Int(n) => self.field_display("int", n),
                    DefineValue::Bool(b) => self.field_display("bool", b),
                    DefineValue::String(s) => self.field_str("string", s),
                }
                self.end('}');
                self.end('}');
            }
            self.end(']');
        }
        if !build.libraries.is_empty() {
            self.string_list("libraries", &build.libraries);
        }
        if !build.packs.is_empty() {
            self.key("packs");
            self.begin('[');
            for pack in &build.packs {
                self.entry();
                self.begin('{');
                self.field_str("id", &pack.id);
                self.field_str("version", &pack.version);
                self.end('}');
            }
            self.end(']');
        }
        self.end('}');
    }

    fn configuration(&mut self, configuration: &BuildConfiguration) {
        self.begin('{');
        self.field_variant("optimization", &configuration.optimization);
        self.field_display("debugInfo", configuration.debug_info);
        self.key("sanitizers");
        self.begin('[');
        for sanitizer in &configuration.sanitizers {
            self.entry();
            self.variant(sanitizer);
        }
        self.end(']');
        self.field_variant("warnings", &configuration.warnings);
        if configuration.warnings_as_errors {
            self.field_display("warningsAsErrors", true);
        }
        self.field_display("hardening", configuration.hardening);
        self.end('}');
    }

    fn module(&mut self, module: &Module) {
        self.begin('{');
        self.field_str("id", module.id.as_str());
        self.field_str("name", &module.name);
        self.key("workspace");
        self.workspace(&module.workspace);
        self.end('}');
    }

    fn workspace(&mut self, workspace: &Workspace) {
        self.begin('{');
        self.key("blocks");
        self.begin('[');
        let mut blocks: Vec<&Block> = workspace.blocks.iter().collect();
        blocks.sort_by(|a, b| a.id.cmp(&b.id));
        for block in blocks {
            self.entry();
            self.block(block);
        }
        self.end(']');
        if !self.semantic {
            if !workspace.frames.is_empty() {
                self.key("frames");
                self.begin('[');
                for frame in &workspace.frames {
                    self.entry();
                    self.begin('{');
                    self.field_str("id", frame.id.as_str());
                    self.field_str("title", &frame.title);
                    self.field_display("x", frame.x);
                    self.field_display("y", frame.y);
                    self.field_display("w", frame.w);
                    self.field_display("h", frame.h);
                    self.field_variant("color", &frame.color);
                    if frame.emit_banner {
                        self.field_display("emitBanner", true);
                    }
                    self.end('}');
                }
                self.end(']');
            }
            if !workspace.notes.is_empty() {
                self.key("notes");
                self.begin('[');
                for note in &workspace.notes {
                    self.entry();
                    self.begin('{');
                    self.field_str("id", note.id.as_str());
                    self.field_str("text", &note.text);
                    self.field_display("x", note.x);
                    self.field_display("y", note.y);
                    self.end('}');
                }
                self.end(']');
            }
            if let Some(viewport) = &workspace.viewport {
                self.key("viewport");
                self.begin('{');
                self.field_display("x", viewport.x);
                self.field_display("y", viewport.y);
                self.key("scale");
                self.f64(viewport.scale);
                self.end('}');
            }
        }
        self.end('}');
    }

    fn block(&mut self, block: &Block) {
        self.begin('{');
        self.field_str("id", block.id.as_str());
        self.field_str("type", &block.block_type);
        self.field_display("v", block.v);
        if !self.semantic {
            if let Some(x) = block.x {
                self.field_display("x", x);
            }
            if let Some(y) = block.y {
                self.field_display("y", y);
            }
            if block.collapsed {
                self.field_display("collapsed", true);
            }
        }
        if block.disabled {
            self.field_display("disabled", true);
        }
        if let Some(comment) = &block.comment {
            self.key("comment");
            self.begin('{');
            self.field_str("text", &comment.text);
            if comment.pinned && !self.semantic {
                self.field_display("pinned", true);
            }
            self.end('}');
        }
        if !block.extra.is_empty() {
            self.key("extra");
            self.begin('{');
            for (key, value) in &block.extra {
                self.key(key);
                self.value(value);
            }
            self.end('}');
        }
        if !block.fields.is_empty() {
            self.key("fields");
            self.begin('{');
            for (key, value) in &block.fields {
                self.key(key);
                self.field_value(value);
            }
            self.end('}');
        }
        if !block.inputs.is_empty() {
            self.key("inputs");
            self.begin('{');
            for (key, input) in &block.inputs {
                self.key(key);
                self.input(input);
            }
            self.end('}');
        }
        if !block.statements.is_empty() {
            self.key("statements");
            self.begin('{');
            for (key, list) in &block.statements {
                self.key(key);
                self.begin('[');
                for child in list {
                    self.entry();
                    self.block(child);
                }
                self.end(']');
            }
            self.end('}');
        }
        self.end('}');
    }

    fn field_value(&mut self, value: &FieldValue) {
        match value {
            FieldValue::Bool(b) => self.display(b),
            FieldValue::Text(text) => self.string(text),
            FieldValue::Decl(decl) => {
                self.begin('{');
                self.field_str("sym", decl.sym.as_str());
                self.field_str("name", &decl.name);
                self.end('}');
            }
            FieldValue::Ref(reference) => {
                self.begin('{');
                self.field_str("ref", reference.target.as_str());
                self.end('}');
            }
        }
    }

    fn input(&mut self, input: &Input) {
        self.begin('{');
        match input {
            Input::Block(nested) => {
                self.key("block");
                self.block(&nested.block);
            }
            Input::Expr(expr) => {
                self.key("expr");
                self.begin('[');
                for token in &expr.expr {
                    self.entry();
                    self.token(token);
                }
                self.end(']');
                if expr.draft {
                    self.field_display("draft", true);
                }
            }
        }
        self.end('}');
    }

    fn token(&mut self, token: &Token) {
        let (kind, text) = match token {
            Token::Num(text) => ("num", text.as_str()),
            Token::Str(text) => ("str", text.as_str()),
            Token::Chr(text) => ("chr", text.as_str()),
            Token::Ref(sym) => ("ref", sym.as_str()),
            Token::Op(text) => ("op", text.as_str()),
            Token::Kw(text) => ("kw", text.as_str()),
            Token::Text(text) => ("text", text.as_str()),
        };
        self.begin('{');
        self.field_str(kind, text);
        self.end('}');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_escaped_like_serde_json() {
        let samples = [
            "plain",
            "quote \" and backslash \\",
            "\u{0}\u{1}\u{8}\t\n\u{b}\u{c}\r\u{1f}\u{7f}",
            "héllo ✓ 😀 \u{2028}\u{202e}",
            "",
        ];
        for sample in samples {
            let mut writer = Writer::new(false);
            writer.string(sample);
            assert_eq!(writer.out, serde_json::to_string(sample).unwrap(), "{sample:?}");
        }
    }

    #[test]
    fn free_form_values_are_sorted_and_pretty() {
        let value: serde_json::Value =
            serde_json::from_str(r#"{"b": [1, -2, 3.5, true, null, {}, []], "a": {"z": "x", "y": 1e300}}"#)
                .unwrap();
        let mut writer = Writer::new(false);
        writer.value(&value);
        assert_eq!(writer.out, serde_json::to_string_pretty(&value).unwrap());
        assert!(writer.out.find("\"a\"") < writer.out.find("\"b\""));
    }

    #[test]
    fn floats() {
        let mut writer = Writer::new(false);
        writer.f64(1.0);
        writer.out.push(' ');
        writer.f64(0.1);
        writer.out.push(' ');
        writer.f64(f64::NAN);
        assert_eq!(writer.out, "1.0 0.1 null");
    }
}
