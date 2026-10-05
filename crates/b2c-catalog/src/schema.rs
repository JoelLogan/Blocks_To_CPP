//! The catalog schema: declarative block definitions in TOML (spec §3.11.1).
//!
//! The same files drive the editor (block shapes, fields, toolbox) and the
//! compiler (validation of project files before analysis).

use std::fmt;
use std::str::FromStr;

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
    /// The type of the value the block gives: required for the `reporter`
    /// and `predicate` shapes and forbidden for the others (a predicate
    /// gives `bool`). The editor's connection checker uses it; the analyser
    /// does the real type checking.
    #[serde(default)]
    pub output: Option<OutputType>,
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

/// Toolbox categories (spec §3.7), declared (and ordered) in the order the
/// toolbox shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

impl Category {
    /// Every category, in toolbox order (spec §3.7).
    pub const ALL: [Self; 9] = [
        Self::Program,
        Self::Variables,
        Self::Math,
        Self::Logic,
        Self::Text,
        Self::Control,
        Self::Loops,
        Self::Io,
        Self::Functions,
    ];

    /// The category's ID as written in catalog files, e.g. `io`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Program => "program",
            Self::Variables => "variables",
            Self::Math => "math",
            Self::Logic => "logic",
            Self::Text => "text",
            Self::Control => "control",
            Self::Loops => "loops",
            Self::Io => "io",
            Self::Functions => "functions",
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
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

/// The static type of the value a `reporter` or `predicate` block gives
/// (spec §3.11.1), written in catalog files as one of `any`, `bool`, `int`,
/// `double`, `number`, `char`, `string`, `symbol` or `field:NAME`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum OutputType {
    /// Depends on the inputs (for example `logic.ternary`).
    Any,
    /// `bool`.
    Bool,
    /// `int`.
    Int,
    /// `double`.
    Double,
    /// `int` or `double`, depending on the values (for example a number
    /// literal, or arithmetic).
    Number,
    /// `char`.
    Char,
    /// `std::string` (written `string`).
    StdString,
    /// The type of the symbol that the block's one `symbol_ref` field refers
    /// to: a variable's type, or a function's return type. The editor takes
    /// it from the live analysis.
    Symbol,
    /// The type chosen in the block's `type` field of this name (for
    /// example `field:TO` for `math.convert`).
    Field(String),
}

impl OutputType {
    /// The prefix of [`OutputType::Field`] in catalog files.
    pub const FIELD_PREFIX: &'static str = "field:";
}

impl fmt::Display for OutputType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Any => "any",
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Double => "double",
            Self::Number => "number",
            Self::Char => "char",
            Self::StdString => "string",
            Self::Symbol => "symbol",
            Self::Field(field) => return write!(f, "{}{field}", Self::FIELD_PREFIX),
        };
        f.write_str(name)
    }
}

/// Text that is not an [`OutputType`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "{0:?} is not an output type (use any, bool, int, double, number, char, string, symbol, or field:NAME with an UPPER_CASE field name)"
)]
pub struct OutputTypeError(pub String);

impl FromStr for OutputType {
    type Err = OutputTypeError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Ok(match text {
            "any" => Self::Any,
            "bool" => Self::Bool,
            "int" => Self::Int,
            "double" => Self::Double,
            "number" => Self::Number,
            "char" => Self::Char,
            "string" => Self::StdString,
            "symbol" => Self::Symbol,
            _ => match text.strip_prefix(Self::FIELD_PREFIX) {
                Some(field) if is_part_name(field) => Self::Field(field.to_owned()),
                _ => return Err(OutputTypeError(text.to_owned())),
            },
        })
    }
}

impl TryFrom<String> for OutputType {
    type Error = OutputTypeError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl From<OutputType> for String {
    fn from(output: OutputType) -> Self {
        output.to_string()
    }
}

/// Field, input and statement names: `^[A-Z][A-Z0-9_]*$`.
pub(crate) fn is_part_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_uppercase())
        && bytes.all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
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
