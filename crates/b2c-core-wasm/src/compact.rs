//! The preview's compact source map (M3-5, docs/plans/m3-contracts/w5-performance.md §4.1):
//! the same ranges as [`SourceMap`] (06 §6.9), in numbers instead of one object per range, so
//! that the editor parses an answer of a few hundred thousand characters in a fraction of the
//! time. Only the preview's JSON has this form: the IR's map, what a build writes and what
//! `cut.rs` reads keep the form of version 1.
//!
//! Version 2:
//!
//! ```json
//! {"version": 2, "modules": ["mod_main", …], "blocks": ["b001", …],
//!  "parts": [{"kind": "whole"}, …],
//!  "files": [{"path": "main.cpp", "module"?: "mod_main",
//!             "ranges": [startLine, startColumn, endLine, endColumn, module, block, part, …]}]}
//! ```
//!
//! Seven numbers per range, in the IR's order; `module`, `block` and `part` are indexes into the
//! lists above, which hold each module ID, block ID and part once, in first use. A file's
//! `module` key is there only when the IR's file has one (ADR-0019).

use std::collections::HashMap;

use b2c_ir::diag::Part;
use b2c_ir::source_map::{FileMap, MappedRange, Position, SourceMap};
use b2c_ir::{BlockId, ModuleId};
use serde::{Deserialize, Serialize, Serializer};

/// The version of the compact form.
pub const PREVIEW_SOURCE_MAP_VERSION: u32 = 2;

/// Numbers per range.
const STRIDE: usize = 7;

#[derive(Serialize, Deserialize)]
struct Compact {
    version: u32,
    modules: Vec<ModuleId>,
    blocks: Vec<BlockId>,
    parts: Vec<Part>,
    files: Vec<CompactFile>,
}

#[derive(Serialize, Deserialize)]
struct CompactFile {
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    module: Option<ModuleId>,
    ranges: Vec<u32>,
}

/// Interns values in first use.
struct Table<T> {
    list: Vec<T>,
    seen: HashMap<T, u32>,
}

impl<T: Clone + Eq + std::hash::Hash> Table<T> {
    fn new() -> Self {
        Self {
            list: Vec::new(),
            seen: HashMap::new(),
        }
    }

    fn of(&mut self, value: &T) -> u32 {
        if let Some(&index) = self.seen.get(value) {
            return index;
        }
        let index = u32::try_from(self.list.len()).unwrap_or(u32::MAX);
        self.list.push(value.clone());
        self.seen.insert(value.clone(), index);
        index
    }
}

fn compact(map: &SourceMap) -> Compact {
    let mut modules = Table::new();
    let mut blocks = Table::new();
    let mut parts: Table<Part> = Table::new();
    let files = map
        .files
        .iter()
        .map(|file| {
            let mut ranges = Vec::with_capacity(file.ranges.len() * STRIDE);
            for range in &file.ranges {
                ranges.extend([
                    range.start.line,
                    range.start.column,
                    range.end.line,
                    range.end.column,
                    modules.of(&range.module),
                    blocks.of(&range.block),
                    parts.of(&range.part),
                ]);
            }
            CompactFile {
                path: file.path.clone(),
                module: file.module.clone(),
                ranges,
            }
        })
        .collect();
    Compact {
        version: PREVIEW_SOURCE_MAP_VERSION,
        modules: modules.list,
        blocks: blocks.list,
        parts: parts.list,
        files,
    }
}

/// Serialises the preview's source map in the compact form (`serialize_with` of
/// [`crate::Preview::source_map`]); `None` stays `null`.
///
/// # Errors
/// The serializer's.
#[allow(
    clippy::ref_option,
    reason = "serde's serialize_with passes the field by reference"
)]
pub(crate) fn serialize<S: Serializer>(map: &Option<SourceMap>, serializer: S) -> Result<S::Ok, S::Error> {
    map.as_ref().map(compact).serialize(serializer)
}

/// Reads the compact form back into the IR's map, for tests and for any reader that wants the
/// form of version 1; `None` for anything that is not a well-formed compact map of this version
/// (another version, a range list that is not a multiple of seven numbers, an index out of
/// range).
pub fn expand(json: &str) -> Option<SourceMap> {
    let compact: Compact = serde_json::from_str(json).ok()?;
    if compact.version != PREVIEW_SOURCE_MAP_VERSION {
        return None;
    }
    let index = |n: u32| usize::try_from(n).ok();
    let mut files = Vec::with_capacity(compact.files.len());
    for file in compact.files {
        if file.ranges.len() % STRIDE != 0 {
            return None;
        }
        let mut ranges = Vec::with_capacity(file.ranges.len() / STRIDE);
        for cells in file.ranges.chunks_exact(STRIDE) {
            let &[
                start_line,
                start_column,
                end_line,
                end_column,
                module,
                block,
                part,
            ] = cells
            else {
                return None;
            };
            ranges.push(MappedRange {
                start: Position {
                    line: start_line,
                    column: start_column,
                },
                end: Position {
                    line: end_line,
                    column: end_column,
                },
                module: compact.modules.get(index(module)?)?.clone(),
                block: compact.blocks.get(index(block)?)?.clone(),
                part: compact.parts.get(index(part)?)?.clone(),
            });
        }
        files.push(FileMap {
            path: file.path,
            module: file.module,
            ranges,
        });
    }
    // The IR's version, which an expanded map is in.
    Some(SourceMap {
        version: crate::facade::SOURCE_MAP_VERSION,
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(line: u32, block: &str, part: Part) -> MappedRange {
        MappedRange {
            start: Position { line, column: 1 },
            end: Position { line, column: 9 },
            module: ModuleId::new("mod_main").expect("id"),
            block: BlockId::new(block).expect("id"),
            part,
        }
    }

    #[test]
    fn a_map_survives_the_round_trip() {
        let map = SourceMap {
            version: 1,
            files: vec![
                FileMap {
                    path: String::from("main.cpp"),
                    module: Some(ModuleId::new("mod_main").expect("id")),
                    ranges: vec![
                        range(1, "b1", Part::Whole),
                        range(
                            2,
                            "b2",
                            Part::Field {
                                name: String::from("NAME"),
                            },
                        ),
                        range(3, "b1", Part::Whole),
                    ],
                },
                FileMap {
                    path: String::from("b2c_support.hpp"),
                    module: None,
                    ranges: Vec::new(),
                },
            ],
        };
        let json = serde_json::to_string(&compact(&map)).expect("json");
        assert!(
            json.starts_with(r#"{"version":2,"modules":["mod_main"],"blocks":["b1","b2"]"#),
            "{json}"
        );
        assert!(
            json.contains(r#""ranges":[1,1,1,9,0,0,0,2,1,2,9,0,1,1,3,1,3,9,0,0,0]"#),
            "{json}"
        );
        assert!(!json.contains(r#""path":"b2c_support.hpp","module""#), "{json}");
        assert_eq!(expand(&json), Some(map));
    }

    #[test]
    fn malformed_maps_expand_to_nothing() {
        for json in [
            "",
            "null",
            r#"{"version":1,"modules":[],"blocks":[],"parts":[],"files":[]}"#,
            r#"{"version":2,"modules":[],"blocks":[],"parts":[],"files":[{"path":"a","ranges":[1,2,3]}]}"#,
            r#"{"version":2,"modules":["m"],"blocks":["b"],"parts":[{"kind":"whole"}],"files":[{"path":"a","ranges":[1,1,1,2,0,0,1]}]}"#,
            r#"{"version":2,"modules":["m"],"blocks":["b"],"parts":[{"kind":"whole"}],"files":[{"path":"a","ranges":[1,1,1,2,0,5,0]}]}"#,
        ] {
            assert_eq!(expand(json), None, "{json}");
        }
    }
}
