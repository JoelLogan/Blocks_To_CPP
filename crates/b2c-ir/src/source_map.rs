//! Generated files and source maps (spec §6.9).

use serde::{Deserialize, Serialize};

use crate::diag::Part;
use crate::ids::{BlockId, ModuleId};

/// Output of code generation for a whole project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedProject {
    /// Generated files, in a deterministic order.
    pub files: Vec<GeneratedFile>,
    /// Maps ranges of generated text back to blocks.
    pub source_map: SourceMap,
}

/// Kind of generated file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    /// A translation unit (`.cpp`), compiled on its own.
    Source,
    /// A header (`.hpp`), included by sources.
    Header,
}

/// One generated file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedFile {
    /// Path relative to the generated-sources directory, `/`-separated, built
    /// only from validated module names (e.g. `main.cpp`, `b2c_support.hpp`).
    pub path: String,
    /// Source or header.
    pub kind: FileKind,
    /// File contents (UTF-8, `\n` line endings, ends with one newline).
    pub contents: String,
}

/// Maps generated text ranges to the blocks that produced them.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SourceMap {
    /// Format version (currently 1).
    pub version: u32,
    /// Files with their mapped ranges.
    pub files: Vec<FileMap>,
}

/// Mapped ranges of one generated file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FileMap {
    /// Path as in [`GeneratedFile::path`].
    pub path: String,
    /// Ranges, sorted by start position.
    pub ranges: Vec<MappedRange>,
}

/// A position in a generated file: 1-based line, 1-based column counted in
/// UTF-8 bytes (GCC's column unit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Position {
    /// 1-based line.
    pub line: u32,
    /// 1-based column in UTF-8 bytes.
    pub column: u32,
}

/// A range of generated text and the block part that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MappedRange {
    /// First position (inclusive).
    pub start: Position,
    /// Last position (exclusive).
    pub end: Position,
    /// The module of the block.
    pub module: ModuleId,
    /// The block.
    pub block: BlockId,
    /// Which part of the block.
    pub part: Part,
}

impl SourceMap {
    /// Finds the innermost range containing a position, for mapping compiler
    /// diagnostics to blocks.
    ///
    /// Ranges are properly nested, so the ranges containing a position form a
    /// chain and the innermost one starts last (and, among ranges with the
    /// same start, ends first).
    pub fn lookup(&self, path: &str, position: Position) -> Option<&MappedRange> {
        let file = self.files.iter().find(|f| f.path == path)?;
        file.ranges
            .iter()
            .filter(|r| r.start <= position && position < r.end)
            .max_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)))
    }

    /// Finds the range for a whole line when only the line is known (e.g.
    /// linker errors or runtime stack traces): the outermost range that
    /// starts on that line (usually the statement written there), or, on a
    /// continuation line, the innermost range that covers it.
    pub fn lookup_line(&self, path: &str, line: u32) -> Option<&MappedRange> {
        let file = self.files.iter().find(|f| f.path == path)?;
        let starting_here = file
            .ranges
            .iter()
            .filter(|r| r.start.line == line)
            .min_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
        starting_here.or_else(|| {
            file.ranges
                .iter()
                .filter(|r| r.start.line <= line && line <= r.end.line)
                .max_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(sl: u32, sc: u32, el: u32, ec: u32, block: &str) -> MappedRange {
        MappedRange {
            start: Position { line: sl, column: sc },
            end: Position { line: el, column: ec },
            module: ModuleId::new("mod_main").unwrap(),
            block: BlockId::new(block).unwrap(),
            part: Part::Whole,
        }
    }

    #[test]
    fn lookup_finds_innermost() {
        let map = SourceMap {
            version: 1,
            files: vec![FileMap {
                path: "main.cpp".into(),
                ranges: vec![
                    range(3, 1, 10, 2, "outer"),
                    range(4, 5, 4, 20, "before"),
                    range(5, 5, 5, 30, "inner"),
                    range(5, 9, 5, 14, "tiny"),
                    range(5, 9, 5, 12, "tinier"),
                    range(6, 5, 8, 6, "multi"),
                    range(6, 9, 8, 2, "multi_inner"),
                ],
            }],
        };
        let at = |line, column| {
            map.lookup("main.cpp", Position { line, column })
                .map(|r| r.block.as_str())
        };
        assert_eq!(at(5, 10), Some("tinier"));
        assert_eq!(at(5, 13), Some("tiny"));
        assert_eq!(at(5, 20), Some("inner"));
        assert_eq!(at(6, 6), Some("multi"));
        assert_eq!(at(7, 1), Some("multi_inner"));
        assert_eq!(at(8, 3), Some("multi"));
        assert_eq!(at(9, 1), Some("outer"));
        assert_eq!(at(11, 1), None);
        assert_eq!(map.lookup("other.cpp", Position { line: 5, column: 10 }), None);
        let on_line = |line| map.lookup_line("main.cpp", line).map(|r| r.block.as_str());
        assert_eq!(on_line(5), Some("inner"));
        assert_eq!(on_line(4), Some("before"));
        assert_eq!(on_line(7), Some("multi_inner"));
        assert_eq!(on_line(9), Some("outer"));
        assert_eq!(on_line(11), None);
    }
}
