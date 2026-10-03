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
    /// diagnostics to blocks. Ranges are nested, so the innermost one is the
    /// shortest range that contains the position.
    pub fn lookup(&self, path: &str, position: Position) -> Option<&MappedRange> {
        let file = self.files.iter().find(|f| f.path == path)?;
        file.ranges
            .iter()
            .filter(|r| r.start <= position && position < r.end)
            .min_by_key(|r| {
                (
                    r.end.line - r.start.line,
                    if r.end.line == r.start.line { r.end.column.saturating_sub(r.start.column) } else { 0 },
                )
            })
    }

    /// Finds the innermost range on a line when only the line is known (e.g.
    /// linker or runtime stack traces).
    pub fn lookup_line(&self, path: &str, line: u32) -> Option<&MappedRange> {
        let file = self.files.iter().find(|f| f.path == path)?;
        file.ranges
            .iter()
            .filter(|r| r.start.line <= line && line <= r.end.line)
            .min_by_key(|r| (r.end.line - r.start.line, r.end.column.saturating_sub(r.start.column)))
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
                ranges: vec![range(3, 1, 10, 2, "outer"), range(5, 5, 5, 30, "inner"), range(5, 9, 5, 14, "tiny")],
            }],
        };
        let at = |line, column| map.lookup("main.cpp", Position { line, column }).map(|r| r.block.as_str());
        assert_eq!(at(5, 10), Some("tiny"));
        assert_eq!(at(5, 20), Some("inner"));
        assert_eq!(at(7, 1), Some("outer"));
        assert_eq!(at(11, 1), None);
        assert_eq!(map.lookup("other.cpp", Position { line: 5, column: 10 }), None);
        assert_eq!(map.lookup_line("main.cpp", 5).map(|r| r.block.as_str()), Some("tiny"));
    }
}
