//! Cutting the C++ of some blocks out of generated files through the source
//! map, for the `text/plain` side of a copy (05 §5.12).
//!
//! Only **whole-block** ranges ([`Part::Whole`]) are used, so a cut is
//! always the complete code of a block, never a field or a token range:
//!
//! * The ranges of one block in one file that touch, or follow each other
//!   on consecutive lines, are merged (a block that became several
//!   statements), and the largest merged span is the block's code (a
//!   function's definition, with its comment, wins over its forward
//!   declaration, which a blank line separates from it).
//! * A span that starts and ends a line (only indentation before it, only
//!   the end of the line after it) is a statement or a definition: its whole
//!   lines are taken and the indentation of its first line is removed from
//!   every line, so the text is ready to paste anywhere.
//! * Any other span is an expression, taken exactly.
//!
//! Positions are 1-based lines and 1-based columns in UTF-8 bytes (06 §6.9).
//! A position outside its file, or one that is not on a character boundary,
//! makes the range unusable and it is skipped: the cut never panics and
//! never splits a character.

use b2c_ir::BlockId;
use b2c_ir::diag::Part;
use b2c_ir::source_map::{GeneratedFile, MappedRange, Position, SourceMap};

/// The code of the given blocks, in the given order, each ending with a line
/// break; `None` when none of them produced code (disabled or loose blocks,
/// or blocks the generator could not reach).
pub(crate) fn cut_blocks(
    files: &[GeneratedFile],
    source_map: &SourceMap,
    blocks: &[BlockId],
) -> Option<String> {
    let mut text = String::new();
    for block in blocks {
        if let Some(code) = cut_block(files, source_map, block) {
            text.push_str(&code);
            if !text.ends_with('\n') {
                text.push('\n');
            }
        }
    }
    (!text.is_empty()).then_some(text)
}

/// The code of one block: the largest merged whole-block span over all
/// files (see the module docs).
fn cut_block(files: &[GeneratedFile], source_map: &SourceMap, block: &BlockId) -> Option<String> {
    let mut best: Option<(usize, &str, usize, usize)> = None;
    for file_map in &source_map.files {
        let Some(file) = files.iter().find(|f| f.path == file_map.path) else {
            continue;
        };
        let lines = LineIndex::new(&file.contents);
        let mut spans: Vec<(usize, usize)> = file_map
            .ranges
            .iter()
            .filter(|range| range.block == *block && range.part == Part::Whole)
            .filter_map(|range| lines.span(range))
            .collect();
        spans.sort_unstable();
        for (start, end) in merge(&file.contents, spans) {
            let length = end.saturating_sub(start);
            if best.is_none_or(|(longest, ..)| length > longest) {
                best = Some((length, file.contents.as_str(), start, end));
            }
        }
    }
    let (_, contents, start, end) = best?;
    extract(contents, start, end)
}

/// Merges sorted spans that overlap, touch, or are on consecutive lines
/// with only whitespace between them (statements of one block). A blank
/// line separates: a function's forward declaration and its definition stay
/// apart.
fn merge(contents: &str, spans: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (start, end) in spans {
        if let Some(last) = merged.last_mut() {
            let joined = start <= last.1
                || contents.get(last.1..start).is_some_and(|gap| {
                    gap.chars().all(char::is_whitespace) && gap.matches('\n').count() <= 1
                });
            if joined {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    merged
}

/// The text of a span: whole lines without their common indentation for a
/// span that starts and ends a line, the exact text otherwise.
fn extract(contents: &str, start: usize, end: usize) -> Option<String> {
    let exact = contents.get(start..end)?;
    let line_start = contents.get(..start)?.rfind('\n').map_or(0, |at| at + 1);
    let line_end = contents
        .get(end..)?
        .find('\n')
        .map_or(contents.len(), |at| end + at);
    let before = contents.get(line_start..start)?;
    let after = contents.get(end..line_end)?;
    let starts_line = before.bytes().all(|b| b == b' ');
    let ends_line = after.trim().is_empty();
    if !(starts_line && ends_line) {
        return Some(exact.to_owned());
    }
    let indent = before.len();
    let mut out = String::with_capacity(exact.len().saturating_add(1));
    for line in exact.split('\n') {
        let kept = line.len() - line.trim_start_matches(' ').len();
        out.push_str(line.get(kept.min(indent)..).unwrap_or(line).trim_end());
        out.push('\n');
    }
    Some(out)
}

/// Byte offsets of the starts of a file's lines.
struct LineIndex<'a> {
    contents: &'a str,
    starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    fn new(contents: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            contents
                .bytes()
                .enumerate()
                .filter(|&(_, b)| b == b'\n')
                .map(|(at, _)| at + 1),
        );
        Self { contents, starts }
    }

    /// The byte offset of a position, if it is inside its line (or just
    /// after its last character) and on a character boundary.
    fn offset(&self, position: Position) -> Option<usize> {
        let line = usize::try_from(position.line).ok()?.checked_sub(1)?;
        let column = usize::try_from(position.column).ok()?.checked_sub(1)?;
        let start = *self.starts.get(line)?;
        let line_end = self
            .starts
            .get(line + 1)
            .map_or(self.contents.len(), |next| next.saturating_sub(1));
        let offset = start.checked_add(column)?;
        (offset <= line_end && self.contents.is_char_boundary(offset)).then_some(offset)
    }

    /// The byte span of a range, if both ends are usable and in order.
    fn span(&self, range: &MappedRange) -> Option<(usize, usize)> {
        let start = self.offset(range.start)?;
        let end = self.offset(range.end)?;
        (start <= end).then_some((start, end))
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // tests fail by panicking
mod tests {
    use b2c_ir::ModuleId;
    use b2c_ir::source_map::{FileKind, FileMap};

    use super::*;

    fn block(id: &str) -> BlockId {
        BlockId::new(id).unwrap()
    }

    fn range(block_id: &str, start: (u32, u32), end: (u32, u32), part: Part) -> MappedRange {
        MappedRange {
            start: Position {
                line: start.0,
                column: start.1,
            },
            end: Position {
                line: end.0,
                column: end.1,
            },
            module: ModuleId::new("m").unwrap(),
            block: block(block_id),
            part,
        }
    }

    fn file(contents: &str, ranges: Vec<MappedRange>) -> (Vec<GeneratedFile>, SourceMap) {
        (
            vec![GeneratedFile {
                path: String::from("main.cpp"),
                kind: FileKind::Source,
                contents: contents.to_owned(),
            }],
            SourceMap {
                version: 1,
                files: vec![FileMap {
                    path: String::from("main.cpp"),
                    ranges,
                }],
            },
        )
    }

    const CODE: &str = "int main() {\n    int x = 1 + 2;\n    if (x > 2) {\n        x = 0;\n    }\n}\n";

    #[test]
    fn statements_are_whole_lines_without_their_indentation() {
        let (files, map) = file(
            CODE,
            vec![
                range("decl", (2, 5), (2, 19), Part::Whole),
                range("sum", (2, 13), (2, 18), Part::Whole),
                range("if", (3, 5), (5, 6), Part::Whole),
                range(
                    "if",
                    (3, 9),
                    (3, 14),
                    Part::Input {
                        name: String::from("COND0"),
                    },
                ),
            ],
        );
        assert_eq!(
            cut_blocks(&files, &map, &[block("decl")]).as_deref(),
            Some("int x = 1 + 2;\n")
        );
        assert_eq!(
            cut_blocks(&files, &map, &[block("if")]).as_deref(),
            Some("if (x > 2) {\n    x = 0;\n}\n")
        );
        // An expression is cut exactly.
        assert_eq!(
            cut_blocks(&files, &map, &[block("sum")]).as_deref(),
            Some("1 + 2\n")
        );
        // In the given order, each on its own lines.
        assert_eq!(
            cut_blocks(&files, &map, &[block("if"), block("sum"), block("decl")]).as_deref(),
            Some("if (x > 2) {\n    x = 0;\n}\n1 + 2\nint x = 1 + 2;\n")
        );
    }

    #[test]
    fn blocks_without_code_give_nothing() {
        let (files, map) = file(CODE, vec![range("decl", (2, 5), (2, 19), Part::Whole)]);
        assert_eq!(cut_blocks(&files, &map, &[block("loose")]), None);
        assert_eq!(cut_blocks(&files, &map, &[]), None);
        assert_eq!(
            cut_blocks(&files, &map, &[block("loose"), block("decl")]).as_deref(),
            Some("int x = 1 + 2;\n")
        );
        // Only whole-block ranges count.
        let (files, map) = file(
            CODE,
            vec![range(
                "if",
                (3, 9),
                (3, 14),
                Part::Field {
                    name: String::from("X"),
                },
            )],
        );
        assert_eq!(cut_blocks(&files, &map, &[block("if")]), None);
    }

    #[test]
    fn adjacent_statements_merge_and_the_largest_span_wins() {
        let code = "void f();\n\nint main() {\n    a();\n    b();\n}\n\nvoid f() {\n    c();\n}\n";
        let (files, map) = file(
            code,
            vec![
                range("ask", (4, 5), (4, 9), Part::Whole),
                range("ask", (5, 5), (5, 9), Part::Whole),
                range("fn", (1, 1), (1, 10), Part::Whole),
                range("fn", (8, 1), (10, 2), Part::Whole),
            ],
        );
        assert_eq!(
            cut_blocks(&files, &map, &[block("ask")]).as_deref(),
            Some("a();\nb();\n")
        );
        assert_eq!(
            cut_blocks(&files, &map, &[block("fn")]).as_deref(),
            Some("void f() {\n    c();\n}\n")
        );
        // A blank line keeps a forward declaration apart from the definition
        // right after it.
        let (files, map) = file(
            "void f();\n\nvoid f() {\n    c();\n}\n",
            vec![
                range("fn", (1, 1), (1, 10), Part::Whole),
                range("fn", (3, 1), (5, 2), Part::Whole),
            ],
        );
        assert_eq!(
            cut_blocks(&files, &map, &[block("fn")]).as_deref(),
            Some("void f() {\n    c();\n}\n")
        );
    }

    #[test]
    fn broken_positions_are_skipped_without_panicking() {
        let code = "é = 1;\n";
        for (start, end) in [
            ((0, 1), (1, 3)),
            ((1, 0), (1, 3)),
            ((1, 2), (1, 4)),
            ((1, 1), (1, 99)),
            ((9, 1), (9, 2)),
            ((1, 5), (1, 1)),
            ((u32::MAX, u32::MAX), (u32::MAX, u32::MAX)),
        ] {
            let (files, map) = file(code, vec![range("b", start, end, Part::Whole)]);
            assert_eq!(
                cut_blocks(&files, &map, &[block("b")]),
                None,
                "{start:?}..{end:?}"
            );
        }
        let (files, map) = file(code, vec![range("b", (1, 1), (1, 8), Part::Whole)]);
        assert_eq!(
            cut_blocks(&files, &map, &[block("b")]).as_deref(),
            Some("é = 1;\n")
        );
        // A source map naming a file that does not exist.
        let (files, mut map) = file(code, vec![range("b", (1, 1), (1, 8), Part::Whole)]);
        map.files[0].path = String::from("other.cpp");
        assert_eq!(cut_blocks(&files, &map, &[block("b")]), None);
    }
}
