//! The pipeline's shared shapes (`docs/spec/06-compiler-pipeline.md` §6.5, §6.6
//! and §6.9): generated files, source maps, static types and the editor's symbol
//! records.
//!
//! Like [`crate::diag`], these are copies of the `b2c-ir` types whose JSON is
//! byte-identical to the shared types' JSON, so the WebAssembly core (which sends
//! the shared types themselves), the CLI and the backend all produce the shapes
//! that `packages/ipc-types` declares for the frontend (spec 02 §2.5.1). The pure
//! compiler crates stay free of TypeScript generation; these copies carry it
//! instead. Every key is a single lower-case word or already camelCase in the
//! shared type, so the copies follow the IPC convention without changing a byte.
//!
//! IDs are plain strings here, as in [`crate::diag::Location`]: they come from the
//! pipeline, which has validated them. Each type converts from its shared
//! counterpart with [`From`].

use serde::{Deserialize, Serialize};

use crate::diag::Part;
use crate::macros::string_enum;

string_enum! {
    /// The kind of a generated file.
    pub enum FileKind {
        /// A translation unit (`.cpp`), compiled on its own.
        Source = "source",
        /// A header (`.hpp`), included by sources.
        Header = "header",
    }
}

/// One generated file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct GeneratedFile {
    /// The path relative to the generated-sources folder, `/`-separated, built
    /// only from validated module names (for example `main.cpp`).
    pub path: String,
    /// Source or header.
    pub kind: FileKind,
    /// The contents: UTF-8 text with `\n` line endings, ending with one newline.
    pub contents: String,
}

/// Maps generated text back to the blocks that produced it (spec 06 §6.9).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SourceMap {
    /// The format version (currently 1).
    pub version: u32,
    /// The generated files with their mapped ranges.
    pub files: Vec<FileMap>,
}

/// The mapped ranges of one generated file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct FileMap {
    /// The path, as in [`GeneratedFile::path`].
    pub path: String,
    /// The ranges, sorted by start position.
    pub ranges: Vec<MappedRange>,
}

/// A position in a generated file: 1-based line, 1-based column counted in UTF-8
/// bytes (GCC's column unit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Position {
    /// The 1-based line.
    pub line: u32,
    /// The 1-based column, in UTF-8 bytes.
    pub column: u32,
}

/// A range of generated text and the block part that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MappedRange {
    /// The first position (inclusive).
    pub start: Position,
    /// The last position (exclusive).
    pub end: Position,
    /// The module of the block.
    pub module: String,
    /// The block.
    pub block: String,
    /// Which part of the block.
    pub part: Part,
}

string_enum! {
    /// A static type (spec 06 §6.6). `string` is `std::string`; `error` is the
    /// type of an expression that already has an error and is compatible with
    /// everything.
    pub enum StaticType {
        /// `void` (function results only).
        Void = "void",
        /// `bool`.
        Bool = "bool",
        /// `char`.
        Char = "char",
        /// `int`.
        Int = "int",
        /// `double`.
        Double = "double",
        /// `std::string`.
        String = "string",
        /// The type of an expression that already has an error.
        Error = "error",
    }
}

string_enum! {
    /// How an argument is passed to a parameter (spec 03 §3.7.7).
    pub enum PassMode {
        /// By value (`T`).
        Copy = "copy",
        /// By reference, editable (`T&`).
        Editable = "editable",
        /// By reference, read-only (`const T&`).
        ReadOnly = "read_only",
    }
}

/// A symbol as the editor sees it (spec 06 §6.5): enough to fill a dropdown,
/// label a getter block or pick the type of a reporter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SymbolInfo {
    /// The symbol's ID (what blocks store).
    pub id: String,
    /// The name the user gave it, as written in its declaring block.
    pub name: String,
    /// What it is, with the details of that kind, as a `kind` key next to the
    /// other keys.
    #[serde(flatten)]
    pub kind: SymbolInfoKind,
    /// Its type; for a function, the type it gives back.
    #[serde(rename = "type")]
    pub ty: StaticType,
    /// The module that declares it.
    pub module: String,
    /// The block that declares it (`var.declare`, `control.for_range`, or the
    /// `func.define` of a function and of its parameters).
    pub decl_block: String,
}

/// The kind of a [`SymbolInfo`], with the details of that kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SymbolInfoKind {
    /// A local variable.
    Variable {
        /// Whether it was declared `const`.
        is_const: bool,
    },
    /// A function parameter.
    Parameter {
        /// How the argument is passed.
        mode: PassMode,
    },
    /// The counter of a `for` loop.
    LoopVariable,
    /// A user function.
    Function {
        /// Its parameters' symbol IDs, in order.
        params: Vec<String>,
        /// The type it gives back (`void` for nothing).
        returns: StaticType,
    },
}

impl From<b2c_ir::source_map::FileKind> for FileKind {
    fn from(kind: b2c_ir::source_map::FileKind) -> Self {
        match kind {
            b2c_ir::source_map::FileKind::Source => Self::Source,
            b2c_ir::source_map::FileKind::Header => Self::Header,
        }
    }
}

impl From<&b2c_ir::source_map::GeneratedFile> for GeneratedFile {
    fn from(file: &b2c_ir::source_map::GeneratedFile) -> Self {
        Self {
            path: file.path.clone(),
            kind: file.kind.into(),
            contents: file.contents.clone(),
        }
    }
}

impl From<&b2c_ir::source_map::SourceMap> for SourceMap {
    fn from(map: &b2c_ir::source_map::SourceMap) -> Self {
        Self {
            version: map.version,
            files: map.files.iter().map(FileMap::from).collect(),
        }
    }
}

impl From<&b2c_ir::source_map::FileMap> for FileMap {
    fn from(file: &b2c_ir::source_map::FileMap) -> Self {
        Self {
            path: file.path.clone(),
            ranges: file.ranges.iter().map(MappedRange::from).collect(),
        }
    }
}

impl From<b2c_ir::source_map::Position> for Position {
    fn from(position: b2c_ir::source_map::Position) -> Self {
        Self {
            line: position.line,
            column: position.column,
        }
    }
}

impl From<&b2c_ir::source_map::MappedRange> for MappedRange {
    fn from(range: &b2c_ir::source_map::MappedRange) -> Self {
        Self {
            start: range.start.into(),
            end: range.end.into(),
            module: range.module.as_str().to_owned(),
            block: range.block.as_str().to_owned(),
            part: Part::from(&range.part),
        }
    }
}

impl From<&b2c_ir::Type> for StaticType {
    fn from(ty: &b2c_ir::Type) -> Self {
        match ty {
            b2c_ir::Type::Void => Self::Void,
            b2c_ir::Type::Bool => Self::Bool,
            b2c_ir::Type::Char => Self::Char,
            b2c_ir::Type::Int => Self::Int,
            b2c_ir::Type::Double => Self::Double,
            b2c_ir::Type::String => Self::String,
            b2c_ir::Type::Error => Self::Error,
        }
    }
}

impl From<b2c_ir::sast::PassMode> for PassMode {
    fn from(mode: b2c_ir::sast::PassMode) -> Self {
        match mode {
            b2c_ir::sast::PassMode::Copy => Self::Copy,
            b2c_ir::sast::PassMode::Editable => Self::Editable,
            b2c_ir::sast::PassMode::ReadOnly => Self::ReadOnly,
        }
    }
}

impl From<&b2c_ir::SymbolInfoKind> for SymbolInfoKind {
    fn from(kind: &b2c_ir::SymbolInfoKind) -> Self {
        match kind {
            b2c_ir::SymbolInfoKind::Variable { is_const } => Self::Variable { is_const: *is_const },
            b2c_ir::SymbolInfoKind::Parameter { mode } => Self::Parameter { mode: (*mode).into() },
            b2c_ir::SymbolInfoKind::LoopVariable => Self::LoopVariable,
            b2c_ir::SymbolInfoKind::Function { params, returns } => Self::Function {
                params: params.iter().map(|p| p.as_str().to_owned()).collect(),
                returns: returns.into(),
            },
        }
    }
}

impl From<&b2c_ir::SymbolInfo> for SymbolInfo {
    fn from(info: &b2c_ir::SymbolInfo) -> Self {
        Self {
            id: info.id.as_str().to_owned(),
            name: info.name.clone(),
            kind: SymbolInfoKind::from(&info.kind),
            ty: StaticType::from(&info.ty),
            module: info.module.as_str().to_owned(),
            decl_block: info.decl_block.as_str().to_owned(),
        }
    }
}
