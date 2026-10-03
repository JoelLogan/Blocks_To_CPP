//! The token writer: the only code in this crate that appends text to the
//! output (spec §6.8.2).
//!
//! Its public methods accept either a typed text leaf from [`b2c_ir::text`]
//! (each written through its encoder) or a `&'static str` chosen by the
//! generator. No method takes a runtime `&str`, so user text cannot reach the
//! output without going through an encoder. The raw writer is private.
//!
//! The printer also tracks the current line and column (1-based, columns in
//! UTF-8 bytes, as GCC reports them) and records source-map ranges.

use b2c_ir::sast::Origin;
use b2c_ir::source_map::{MappedRange, Position};
use b2c_ir::text::{CharLit, Comment, Ident, NumLit, StrLit};

/// Builds one generated file.
#[derive(Debug)]
pub(crate) struct Printer {
    out: String,
    line: u32,
    column: u32,
    level: usize,
    indent_width: usize,
    at_line_start: bool,
    /// Column (1-based) where the next line starts instead of the
    /// indentation, for continuation lines.
    align_next: Option<u32>,
    ranges: Vec<MappedRange>,
}

/// Converts a length to a column delta, saturating (files never get near 4 GiB).
fn to_u32(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

impl Printer {
    /// An empty file with the given indentation width.
    pub(crate) fn new(indent_width: u8) -> Self {
        Self {
            out: String::new(),
            line: 1,
            column: 1,
            level: 0,
            indent_width: usize::from(indent_width),
            at_line_start: true,
            align_next: None,
            ranges: Vec::new(),
        }
    }

    /// Appends text that contains no line break. Indentation is written lazily
    /// before the first text on a line, so empty lines stay empty.
    fn write_raw(&mut self, text: &str) {
        for (index, segment) in text.split('\n').enumerate() {
            if index > 0 {
                // Callers never pass line breaks; handle them anyway so the
                // line count stays right.
                self.newline();
            }
            if segment.is_empty() {
                continue;
            }
            if self.at_line_start {
                let indent = self.line_indent();
                self.align_next = None;
                self.out.extend(std::iter::repeat_n(' ', indent));
                self.column = self.column.saturating_add(to_u32(indent));
                self.at_line_start = false;
            }
            self.out.push_str(segment);
            self.column = self.column.saturating_add(to_u32(segment.len()));
        }
    }

    /// The number of spaces the current line starts with.
    fn line_indent(&self) -> usize {
        match self.align_next {
            Some(column) => column.saturating_sub(1) as usize,
            None => self.level.saturating_mul(self.indent_width),
        }
    }

    /// Writes text chosen by the generator (keywords, punctuation, standard
    /// names, support-helper code).
    pub(crate) fn fixed(&mut self, text: &'static str) {
        self.write_raw(text);
    }

    /// Writes an identifier.
    pub(crate) fn ident(&mut self, ident: &Ident) {
        self.write_raw(ident.as_str());
    }

    /// Writes a string literal through its encoder.
    pub(crate) fn str_lit(&mut self, lit: &StrLit) {
        self.write_raw(&lit.to_cpp());
    }

    /// Writes a character literal through its encoder.
    pub(crate) fn char_lit(&mut self, lit: CharLit) {
        self.write_raw(&lit.to_cpp());
    }

    /// Writes a numeric literal in its normalised form.
    pub(crate) fn num(&mut self, lit: &NumLit) {
        self.write_raw(lit.as_str());
    }

    /// Writes a comment as `//` lines at the current indentation, each followed
    /// by a line break. The encoder guards lines that end in `\` or `??/`.
    pub(crate) fn comment(&mut self, comment: &Comment) {
        for line in comment.to_cpp_lines() {
            self.write_raw(&line);
            self.newline();
        }
    }

    /// Writes generator-chosen code made of several lines. Each leading tab of
    /// a line stands for one indentation level (relative to the current one),
    /// so the code follows the configured indentation width.
    pub(crate) fn fixed_lines(&mut self, text: &'static str) {
        let base = self.level;
        for line in text.lines() {
            let body = line.trim_start_matches('\t');
            self.level = base + (line.len() - body.len());
            self.fixed(body);
            self.newline();
        }
        self.level = base;
    }

    /// Ends the current line.
    pub(crate) fn newline(&mut self) {
        self.out.push('\n');
        self.line = self.line.saturating_add(1);
        self.column = 1;
        self.at_line_start = true;
        self.align_next = None;
    }

    /// Ends the current line; the next one continues it, starting at `column`.
    pub(crate) fn newline_aligned(&mut self, column: u32) {
        self.newline();
        self.align_next = Some(column);
    }

    /// Starts a new section: ends the current line if needed, then makes sure
    /// exactly one empty line separates it from earlier output.
    pub(crate) fn section(&mut self) {
        if !self.at_line_start {
            self.newline();
        }
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.newline();
        }
    }

    /// Increases the indentation level.
    pub(crate) fn indent(&mut self) {
        self.level += 1;
    }

    /// Decreases the indentation level.
    pub(crate) fn dedent(&mut self) {
        self.level = self.level.saturating_sub(1);
    }

    /// The position where the next text will start.
    pub(crate) fn position(&self) -> Position {
        let column = if self.at_line_start {
            to_u32(self.line_indent()).saturating_add(1)
        } else {
            self.column
        };
        Position {
            line: self.line,
            column,
        }
    }

    /// Records that the text from `start` to the current position came from
    /// `origin`.
    pub(crate) fn map(&mut self, start: Position, origin: &Origin) {
        let end = Position {
            line: self.line,
            column: self.column,
        };
        self.ranges.push(MappedRange {
            start,
            end: end.max(start),
            module: origin.module.clone(),
            block: origin.block.clone(),
            part: origin.part.clone(),
        });
    }

    /// Finishes the file: exactly one trailing line break, and the ranges sorted
    /// by start position (enclosing ranges before the ranges they contain).
    pub(crate) fn finish(mut self) -> (String, Vec<MappedRange>) {
        if !self.at_line_start {
            self.newline();
        }
        while self.out.ends_with("\n\n") {
            self.out.pop();
        }
        if self.out.is_empty() {
            self.out.push('\n');
        }
        self.ranges
            .sort_by(|a, b| a.start.cmp(&b.start).then_with(|| b.end.cmp(&a.end)));
        (self.out, self.ranges)
    }
}

#[cfg(test)]
mod tests {
    use b2c_ir::diag::Part;
    use b2c_ir::ids::{BlockId, ModuleId};

    use super::*;

    fn origin(block: &str) -> Origin {
        Origin::whole(ModuleId::new("mod_main").unwrap(), BlockId::new(block).unwrap())
    }

    #[test]
    fn tracks_lines_and_byte_columns() {
        let mut p = Printer::new(4);
        p.fixed("int x;");
        p.newline();
        p.indent();
        let start = p.position();
        assert_eq!(start, Position { line: 2, column: 5 });
        p.str_lit(&StrLit::new("é").unwrap());
        p.map(start, &origin("b1"));
        let (text, ranges) = p.finish();
        assert_eq!(text, "int x;\n    \"é\"\n");
        // `"é"` is 4 bytes: columns 5..9.
        assert_eq!(ranges[0].end, Position { line: 2, column: 9 });
        assert_eq!(ranges[0].part, Part::Whole);
    }

    #[test]
    fn empty_lines_have_no_indentation_and_sections_are_single_spaced() {
        let mut p = Printer::new(2);
        p.indent();
        p.fixed("a");
        p.newline();
        p.newline();
        p.section();
        p.fixed("b");
        p.section();
        p.section();
        p.fixed("c");
        let (text, _) = p.finish();
        assert_eq!(text, "  a\n\n  b\n\n  c\n");
    }

    #[test]
    fn fixed_lines_use_the_indent_width() {
        let mut p = Printer::new(2);
        p.fixed_lines("f() {\n\treturn;\n\n}\n");
        let (text, _) = p.finish();
        assert_eq!(text, "f() {\n  return;\n\n}\n");
    }

    #[test]
    fn comments_are_encoded_and_guarded() {
        let mut p = Printer::new(4);
        p.indent();
        p.comment(&Comment::new("one\ntwo \\\nwhat??/").unwrap());
        let (text, _) = p.finish();
        assert_eq!(text, "    // one\n    // two \\ //\n    // what??/ //\n");
    }

    #[test]
    fn aligned_continuation_lines() {
        let mut p = Printer::new(4);
        p.indent();
        p.fixed("std::cout");
        p.newline_aligned(15);
        assert_eq!(p.position(), Position { line: 2, column: 15 });
        p.fixed("<< x;");
        p.newline();
        p.fixed("y;");
        let (text, _) = p.finish();
        assert_eq!(text, "    std::cout\n              << x;\n    y;\n");
    }

    #[test]
    fn ranges_are_sorted_outer_first() {
        let mut p = Printer::new(4);
        let outer = p.position();
        p.fixed("f(");
        let inner = p.position();
        p.fixed("x");
        p.map(inner, &origin("inner"));
        p.fixed(");");
        p.map(outer, &origin("outer"));
        let (_, ranges) = p.finish();
        let blocks: Vec<_> = ranges.iter().map(|r| r.block.as_str()).collect();
        assert_eq!(blocks, ["outer", "inner"]);
    }
}
