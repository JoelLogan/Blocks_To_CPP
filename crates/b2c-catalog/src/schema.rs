//! The catalog schema: declarative block definitions in TOML (spec §3.11.1).
//!
//! The same files drive the editor (block shapes, fields, toolbox) and the
//! compiler (validation of project files before analysis).

use serde::{Deserialize, Serialize};

/// One catalog TOML file: a list of `[[block]]` tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogFile {
    /// Block definitions.
    #[serde(default)]
    pub block: Vec<BlockDef>,
}

/// A block type definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockDef {
    /// Stable, namespaced ID (`category.name`); never reused.
    pub id: String,
    /// Bumped on breaking changes; older versions need a migration.
    pub version: u32,
    /// Toolbox category.
    pub category: Category,
    /// Visual and structural shape.
    pub shape: Shape,
    /// Label templates; `%NAME` marks where a field or input goes.
    pub label: Label,
    /// How the compiler lowers the block (`builtin` for core blocks).
    pub lowering: Lowering,
    /// Headers the generated code needs (informational; the generator decides).
    #[serde(default)]
    pub headers: Vec<String>,
    /// One-line help text (tooltip).
    pub help: String,
    /// Fields.
    #[serde(default)]
    pub field: Vec<FieldDef>,
    /// Value inputs.
    #[serde(default)]
    pub input: Vec<InputDef>,
    /// Statement inputs.
    #[serde(default)]
    pub statement: Vec<StatementDef>,
    /// Mutator state keys.
    #[serde(default)]
    pub extra: Vec<ExtraDef>,
}

/// Toolbox categories (spec §3.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Program structure.
    Program,
    /// Variables.
    Variables,
    /// Math.
    Math,
    /// Logic.
    Logic,
    /// Text.
    Text,
    /// Control.
    Control,
    /// Loops.
    Loops,
    /// Input / Output.
    Io,
    /// Functions.
    Functions,
}

/// Block shapes (spec §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// Top-level entry point (`when program starts`).
    Hat,
    /// Top-level definition (function, type, …).
    Definition,
    /// A statement (puzzle notch top and bottom); may have statement inputs.
    Statement,
    /// An expression producing a value (round).
    Reporter,
    /// An expression producing `bool` (hexagonal).
    Predicate,
}

impl Shape {
    /// Whether blocks of this shape sit at the top level of a workspace.
    pub fn is_top_level(self) -> bool {
        matches!(self, Self::Hat | Self::Definition)
    }

    /// Whether blocks of this shape fit a value input.
    pub fn is_value(self) -> bool {
        matches!(self, Self::Reporter | Self::Predicate)
    }
}

/// Label templates for the two display modes (spec §3.5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Label {
    /// Friendly mode, e.g. `print %ITEM`.
    pub friendly: String,
    /// C++ mode, e.g. `std::cout << %ITEM`.
    pub cpp: String,
}

/// How a block is lowered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lowering {
    /// Hand-written lowering in b2c-lang, keyed by block ID.
    Builtin,
}

/// A field definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldDef {
    /// Field name (upper case), e.g. `NAME`.
    pub name: String,
    /// Kind of value.
    pub kind: FieldKind,
    /// Dropdown options as `[label, value]` pairs.
    #[serde(default)]
    pub options: Vec<[String; 2]>,
    /// Allowed type names for `type` fields (e.g. `int`, `std::string`, `auto`).
    #[serde(default)]
    pub types: Vec<String>,
    /// Default value (dropdown value, checkbox, text or type name). Fields
    /// without a default are required.
    #[serde(default)]
    pub default: Option<FieldDefault>,
}

/// A field's default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FieldDefault {
    /// Checkbox default.
    Bool(bool),
    /// Text, number text, dropdown value or type name.
    Text(String),
}

/// Kinds of fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    /// One of `options` (stored as the option value).
    Dropdown,
    /// `true` / `false`.
    Checkbox,
    /// Free text (string literal content).
    Text,
    /// Numeric literal text.
    Number,
    /// A type name from `types`.
    Type,
    /// Declares a symbol: `{"sym", "name"}`.
    SymbolDecl,
    /// References a symbol: `{"ref"}`.
    SymbolRef,
}

/// A value-input definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputDef {
    /// Input name (upper case). Repeated inputs are numbered: `ITEM0`, `ITEM1`, …
    pub name: String,
    /// Type class the editor's connection checker uses; the analyser does the
    /// real type checking.
    pub check: TypeClass,
    /// The input may be left empty (no catalog default either).
    #[serde(default)]
    pub optional: bool,
    /// Repetition controlled by a count in `extra`.
    #[serde(default)]
    pub repeat: Option<Repeat>,
    /// Default expression tokens used when the input is absent (a "shadow").
    /// Same JSON-style token objects as project files, e.g. `[{ num = "0" }]`.
    #[serde(default)]
    pub default: Vec<toml::Value>,
}

/// A statement-input definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatementDef {
    /// Input name (upper case), e.g. `BODY`, `DO0`, `ELSE`.
    pub name: String,
    /// Repetition controlled by a count in `extra`.
    #[serde(default)]
    pub repeat: Option<Repeat>,
    /// Present only when this `extra` flag is true (e.g. `ELSE` when `hasElse`).
    #[serde(default)]
    pub when: Option<String>,
}

/// Repetition: inputs `NAME0` … `NAME{count + plus - 1}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repeat {
    /// The `extra` key holding the count.
    pub count: String,
    /// Added to the count (e.g. 1 for `if` + `elseIfCount` branches).
    #[serde(default)]
    pub plus: u32,
}

/// Type classes for inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeClass {
    /// Any non-void value.
    Any,
    /// `int`, `double` or `char`.
    Number,
    /// `int` (or `char`).
    Integer,
    /// `bool`.
    Bool,
    /// `std::string` or `char`.
    Text,
}

/// A mutator-state key definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtraDef {
    /// Key name (camelCase), e.g. `itemCount`.
    pub name: String,
    /// Kind of value.
    pub kind: ExtraKind,
    /// Minimum (for `count`).
    #[serde(default)]
    pub min: u32,
    /// Maximum (for `count`; for `params`, the maximum number of rows).
    pub max: u32,
    /// Default (for `count` and `flag`).
    #[serde(default)]
    pub default: Option<FieldDefault>,
    /// Allowed parameter types (for `params`).
    #[serde(default)]
    pub types: Vec<String>,
}

/// Kinds of mutator state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtraKind {
    /// A non-negative integer.
    Count,
    /// A boolean.
    Flag,
    /// Function parameter rows: an array of
    /// `{"sym": SymbolId, "name": String, "type": type name, "mode": "copy"|"editable"|"read_only"}`.
    Params,
}
