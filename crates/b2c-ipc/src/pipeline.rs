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

use serde::de::{self, Deserializer};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    /// The module the file belongs to (ADR-0019); omitted for
    /// `b2c_support.hpp` and while code generation does not set it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub module: Option<String>,
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
    /// The module of the file, as in [`GeneratedFile::module`]; omitted when
    /// not set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub module: Option<String>,
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

/// A static type as the wire writes it (spec 06 §6.6, types.md §3.1): its
/// type term (`"int"`, `"std::string"`, `{"std::vector": "int"}`,
/// `{"std::map": ["std::string", "int"]}`, `{"ref": "sym_x"}` for a user
/// struct or enum), or `"error"` for the type of an expression that already
/// has an error, which is compatible with everything.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(type = "string | { readonly [name: string]: TypeJson | readonly TypeJson[] }")
)]
pub struct TypeJson(Value);

/// It holds the JSON itself, made from a `b2c_ir::Type` with [`From`], so it
/// is byte-identical to the shared type's JSON by construction.
impl TypeJson {
    /// The JSON.
    pub fn as_json(&self) -> &Value {
        &self.0
    }
}

/// The JSON itself.
impl Serialize for TypeJson {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// Accepts what `b2c_ir::Type` reads, so a value is always a type.
impl<'de> Deserialize<'de> for TypeJson {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        match b2c_ir::Type::deserialize(&value) {
            Ok(_) => Ok(Self(value)),
            Err(_) => Err(de::Error::custom("not a type")),
        }
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
    /// Its type; for a function, the type it gives back; for a struct or an
    /// enum, the type itself, and for an enumerator, its enum.
    #[serde(rename = "type")]
    pub ty: TypeJson,
    /// The module that declares it.
    pub module: String,
    /// The block that declares it (`var.declare`, `control.for_range`, or the
    /// `func.define` of a function and of its parameters).
    pub decl_block: String,
    /// Whether its definition goes into its module's header (ADR-0019);
    /// omitted when false, as it always is in a single-module project.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shared: bool,
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
        returns: TypeJson,
    },
    /// A user struct (milestone M3).
    Struct {
        /// Its fields' symbol IDs, in order.
        fields: Vec<String>,
        /// Whether it is comparable: ticked, and every field's type is
        /// comparable.
        comparable: bool,
        /// Whether it is printable: ticked, and every field's type is
        /// printable.
        printable: bool,
    },
    /// A field of a struct.
    Field {
        /// The struct's symbol ID.
        owner: String,
    },
    /// A user enum (milestone M3).
    Enum {
        /// Its enumerators' symbol IDs, in order.
        values: Vec<String>,
    },
    /// A value of an enum.
    Enumerator {
        /// The enum's symbol ID.
        owner: String,
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
            module: file.module.as_ref().map(|module| module.as_str().to_owned()),
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
            module: file.module.as_ref().map(|module| module.as_str().to_owned()),
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

/// `b2c_ir::Type`'s own JSON: its term form.
impl From<&b2c_ir::Type> for TypeJson {
    fn from(ty: &b2c_ir::Type) -> Self {
        // A type always serialises; the fallback keeps `From` total.
        Self(serde_json::to_value(ty).unwrap_or_else(|_| Value::from(b2c_ir::types::ERROR_JSON)))
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
        let ids = |ids: &[b2c_ir::SymbolId]| ids.iter().map(|id| id.as_str().to_owned()).collect();
        match kind {
            b2c_ir::SymbolInfoKind::Variable { is_const } => Self::Variable { is_const: *is_const },
            b2c_ir::SymbolInfoKind::Parameter { mode } => Self::Parameter { mode: (*mode).into() },
            b2c_ir::SymbolInfoKind::LoopVariable => Self::LoopVariable,
            b2c_ir::SymbolInfoKind::Function { params, returns } => Self::Function {
                params: ids(params),
                returns: returns.into(),
            },
            b2c_ir::SymbolInfoKind::Struct {
                fields,
                comparable,
                printable,
            } => Self::Struct {
                fields: ids(fields),
                comparable: *comparable,
                printable: *printable,
            },
            b2c_ir::SymbolInfoKind::Field { owner } => Self::Field {
                owner: owner.as_str().to_owned(),
            },
            b2c_ir::SymbolInfoKind::Enum { values } => Self::Enum { values: ids(values) },
            b2c_ir::SymbolInfoKind::Enumerator { owner } => Self::Enumerator {
                owner: owner.as_str().to_owned(),
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
            ty: TypeJson::from(&info.ty),
            module: info.module.as_str().to_owned(),
            decl_block: info.decl_block.as_str().to_owned(),
            shared: info.shared,
        }
    }
}
