//! The catalog schema: declarative block definitions in TOML (spec §3.11.1).
//!
//! The same files drive the editor (block shapes, fields, toolbox) and the
//! compiler (validation of project files before analysis).
//!
//! From milestone M3 a definition may also declare its type signature
//! (ADR-0017 §5, `docs/plans/m3-contracts/types.md` §4): type variables with
//! their classes (`where`), type patterns on its output, fields and value
//! inputs (`type`), `assign` inputs, `members` fields, the class of types a
//! parameter row accepts (`type_class`) and the headers a dropdown option
//! needs. Patterns are written in C++ syntax and parsed when the file loads,
//! so a pattern that does not parse makes its file invalid.
//!
//! Milestone M3's second wave (`docs/plans/m3-contracts/w2.md` §2.1) adds
//! the row extras `fields` (a struct's fields) and `values` (an enum's
//! values) beside `params`, a repeat counted by the rows of a row extra,
//! repeated fields (`repeat` on a dropdown, checkbox, type or `symbol_decl`
//! field), a type field filled only from the module (`types = []`) and four
//! categories.

use std::collections::BTreeMap;
use std::fmt;

use b2c_ir::sig::TypePattern;
use b2c_ir::types::TypeTerm;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};

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
    /// The type of the value the block gives, a type pattern (types.md
    /// §4.2): required for the `reporter` and `predicate` shapes and
    /// forbidden for the others (a predicate gives `bool`). M2's values keep
    /// their meaning: `any` and `number` leave the type to the analyser,
    /// `symbol` is the type of the symbol in the block's one `symbol_ref`
    /// field and `field:NAME` the type chosen in the type field `NAME`.
    /// `catalog.json` writes it as its text (`"std::string"`).
    #[serde(default, with = "output_pattern")]
    pub output: Option<TypePattern>,
    /// `where`: each type variable of the block's patterns and its class
    /// (`where = { C = "indexable" }`); `typeVars` in `catalog.json`.
    #[serde(
        default,
        rename(deserialize = "where", serialize = "typeVars"),
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub type_vars: BTreeMap<String, TypeClass>,
    /// Label templates; `%NAME` marks where a field or input goes.
    pub label: Label,
    /// How the compiler lowers the block (`builtin` for core blocks).
    pub lowering: Lowering,
    /// The standard headers every form of the block's code needs, each from
    /// the header table (types.md §5): emitted with the code. A header only
    /// some forms need is left to the code generator, which adds it with
    /// the form; a dropdown option's own headers are the field's `headers`.
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
    /// `namespace_member = true` on a definition: besides the top level, it
    /// fits a namespace's mouth (a statement list marked `definitions`),
    /// with the definitions before and after it there (w3-modules.md §2.12).
    /// In the editor it has a previous and a next connection, which only a
    /// mouth and the definitions in one use. `namespaceMember` in
    /// `catalog.json`, written only when set.
    #[serde(
        default,
        rename(deserialize = "namespace_member", serialize = "namespaceMember"),
        skip_serializing_if = "is_false"
    )]
    pub namespace_member: bool,
    /// `advanced = true`: the editor offers the block only with *Show
    /// advanced blocks* (w4-appearance-expert.md §2.4, w4-editor-tools.md
    /// §6.1). Written only when set.
    #[serde(default, skip_serializing_if = "is_false")]
    pub advanced: bool,
    /// `requires`: the standard or toolchain capability every form of the
    /// block needs, and whether the code generator has an older form
    /// (`requires = ["c++20", "stdFormat"]`, [`Requires`]). Written only
    /// when present.
    #[serde(
        default,
        serialize_with = "serialize_requires",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub requires: Vec<String>,
}

/// What a block, or one value of a field, needs beyond the basic editor
/// (w4-editor-tools.md §6.1), written in a catalog file as a list of words:
/// the oldest standard that has it (`"c++20"`), a toolchain capability
/// (`"stdFormat"`) and `"fallback"` when the code generator writes an older
/// form (`requires = ["c++20", "fallback"]`). The editor's gating service
/// hides what the project's standard or the toolchain cannot compile, unless
/// `fallback` says the generator has that older form, and the analyser
/// reports `B2C-E0703` for a block used with an older standard and no
/// fallback. `catalog.json` writes it as an object (`{"standard": "c++20",
/// "fallback": true}`).
///
/// The definitions keep the words, which the catalog reads with the reader
/// it has for `headers`, and [`Requires::parse`] reads them: a table, or a
/// map of these, cost the WebAssembly core some 20 KB.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Requires {
    /// The oldest standard that has it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub standard: Option<b2c_ir::sast::CppStandard>,
    /// A toolchain capability it needs, probed at discovery (07 §7.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<Capability>,
    /// The code generator writes an older form where the standard or the
    /// capability is missing, so the editor always offers it.
    #[serde(skip_serializing_if = "is_false")]
    pub fallback: bool,
}

impl Requires {
    /// Reads a `requires` list: each word a standard, a capability or
    /// `fallback`, each kind at most once.
    ///
    /// # Errors
    ///
    /// A word that is none of these, or a kind said twice, with a message
    /// naming the word.
    pub fn parse(words: &[String]) -> Result<Requires, String> {
        use b2c_ir::sast::CppStandard;
        let mut requires = Requires::default();
        for word in words {
            let standard = match word.as_str() {
                "c++17" => Some(CppStandard::Cpp17),
                "c++20" => Some(CppStandard::Cpp20),
                "c++23" => Some(CppStandard::Cpp23),
                "c++26" => Some(CppStandard::Cpp26),
                _ => None,
            };
            match word.as_str() {
                _ if standard.is_some() && requires.standard.is_none() => requires.standard = standard,
                "stdFormat" if requires.capability.is_none() => {
                    requires.capability = Some(Capability::StdFormat);
                }
                "fallback" if !requires.fallback => requires.fallback = true,
                _ => {
                    return Err(format!(
                        "{word:?} is not a standard, a capability or \"fallback\", or repeats one"
                    ));
                }
            }
        }
        Ok(requires)
    }

    /// Whether the editor hides it where the requirement is not met: a
    /// requirement with a fallback never is.
    pub fn refuses(words: &[String]) -> bool {
        Requires::parse(words).is_ok_and(|requires| !requires.fallback)
    }
}

/// A probed toolchain capability a block can need (07 §7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum Capability {
    /// `std::format` compiles and links (GCC 13 and later).
    #[serde(rename = "stdFormat")]
    StdFormat,
}

/// Writes a block's `requires` words as a [`Requires`] object.
fn serialize_requires<S: Serializer>(words: &[String], serializer: S) -> Result<S::Ok, S::Error> {
    Requires::parse(words)
        .map_err(serde::ser::Error::custom)?
        .serialize(serializer)
}

/// Writes a field's `requires` as [`Requires`] objects by value.
fn serialize_requires_by_value<S: Serializer>(
    by_value: &BTreeMap<String, Vec<String>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let parsed: Result<BTreeMap<&str, Requires>, String> = by_value
        .iter()
        .map(|(value, words)| Ok((value.as_str(), Requires::parse(words)?)))
        .collect();
    parsed.map_err(serde::ser::Error::custom)?.serialize(serializer)
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
    /// Collections (lists, maps, sets, …).
    Collections,
    /// Types: structs and enums.
    Types,
    /// Files.
    Files,
    /// Errors.
    Errors,
}

impl Category {
    /// Every category, in toolbox order (spec §3.7).
    pub const ALL: [Self; 13] = [
        Self::Program,
        Self::Variables,
        Self::Math,
        Self::Logic,
        Self::Text,
        Self::Control,
        Self::Loops,
        Self::Io,
        Self::Functions,
        Self::Collections,
        Self::Types,
        Self::Files,
        Self::Errors,
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
            Self::Collections => "collections",
            Self::Types => "types",
            Self::Files => "files",
            Self::Errors => "errors",
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
    /// Top-level definition (function, type, …). One marked
    /// [`BlockDef::namespace_member`] also fits a namespace's mouth.
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
    /// The types a `type` field always offers, its menu's first choices, in
    /// C++ syntax (`int`, `std::string`, `auto`, `std::vector<int>`).
    /// `catalog.json` writes each as a type term (types.md §3.1).
    #[serde(default, serialize_with = "serialize_types")]
    pub types: Vec<String>,
    /// Default value (dropdown value, checkbox, text, type name or header).
    /// Fields without a default are required.
    #[serde(default)]
    pub default: Option<FieldDefault>,
    /// `type`: the field's type pattern, on `type`, `symbol_ref` and
    /// `symbol_decl` fields. A type field also accepts every type the
    /// pattern matches within its variables' classes; a reference binds the
    /// pattern's variables from the symbol's type; a name declares its
    /// symbol with the pattern's type.
    #[serde(
        default,
        rename = "type",
        with = "part_pattern",
        skip_serializing_if = "Option::is_none"
    )]
    pub pattern: Option<TypePattern>,
    /// `members = "S"` on a `symbol_ref` field: the field lists the members of
    /// the user type bound to the variable `S` (a struct's fields, an enum's
    /// values) instead of the symbols in scope, and binds nothing.
    /// `catalog.json` lists these per block (`members`).
    #[serde(default, skip_serializing)]
    pub members: Option<String>,
    /// `headers` on a dropdown: the standard headers each option's code
    /// needs besides the block's, by option value
    /// (`headers = { sqrt = ["<cmath>"] }`). `catalog.json` lists these per
    /// block (`optionHeaders`).
    #[serde(default, skip_serializing)]
    pub headers: BTreeMap<String, Vec<String>>,
    /// Repetition controlled by a count or the rows of a row extra, as on an
    /// input: parts `NAME0`, `NAME1`, …, each with the field's default
    /// (w2.md §2.1). Only dropdown, checkbox, type and `symbol_decl` fields
    /// repeat, and a repeated field has no pattern of its own except
    /// `field:F` on a repeated `symbol_decl`, where `F` is a repeated type
    /// field with the same repeat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<Repeat>,
    /// `omit_default = true`: the editor writes the field into a project
    /// only while it holds something other than its default, so a block that
    /// leaves it at the default reads as it did before the field existed
    /// (w3-modules.md §6 W3M-D11). A missing field reads as its default
    /// anyway (03 §3.11.3). Every `SHARED` field has it. `omitDefault` in
    /// `catalog.json`, written only when set.
    #[serde(
        default,
        rename(deserialize = "omit_default", serialize = "omitDefault"),
        skip_serializing_if = "is_false"
    )]
    pub omit_default: bool,
    /// `advanced` on a dropdown or a checkbox: the values the editor offers
    /// only with *Show advanced blocks* (`advanced = ["and", "or"]`; a
    /// checkbox's values are `"true"` and `"false"`). A block that already
    /// holds one keeps it. Written only when not empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advanced: Vec<String>,
    /// `requires` on a dropdown or a checkbox: what each listed value needs,
    /// as a block's [`Requires`] (`requires = { starts = ["c++20",
    /// "fallback"] }`). Written only when not empty.
    #[serde(
        default,
        serialize_with = "serialize_requires_by_value",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub requires: BTreeMap<String, Vec<String>>,
    /// `menu` on a checkbox: the field is set from the block's context menu,
    /// with these words, instead of being drawn on the block (*Fast
    /// unchecked access*, w4-editor-tools.md §7). Such a field is shown once,
    /// has a default and `omit_default`, a `mark`, and appears in neither
    /// label. Written only when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menu: Option<String>,
    /// `mark`: the words a block shows where a `menu` field sits while it
    /// holds something other than its default (`unchecked`). Written only
    /// when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<String>,
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
    /// A type: one of `types`, or a type the field's pattern allows.
    Type,
    /// Declares a symbol: `{"sym", "name"}`.
    SymbolDecl,
    /// References a symbol: `{"ref"}`.
    SymbolRef,
    /// A standard header from the header table, such as `<vector>`, stored
    /// as its text (*use header*, types.md §5.4).
    Header,
    /// A namespace (*use namespace*, w3-modules.md §2.9), stored as text: a
    /// standard namespace as its name after `::` (`::std`), which no symbol
    /// ID can be, or a user namespace's symbol ID. Which namespaces exist is
    /// the analyser's to check.
    Namespace,
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
    /// An input whose pattern is not one fixed type has none (types.md §4.3).
    #[serde(default)]
    pub default: Vec<toml::Value>,
    /// `type`: the type pattern the input's value must fit, for each part of
    /// a repeated input.
    #[serde(
        default,
        rename = "type",
        with = "part_pattern",
        skip_serializing_if = "Option::is_none"
    )]
    pub pattern: Option<TypePattern>,
    /// `assign = true`: the value is assigned, not used to initialise, so a
    /// `char` also fits a `std::string`, as C++ allows; such an input never
    /// binds a type variable.
    #[serde(default, skip_serializing_if = "is_false")]
    pub assign: bool,
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
    /// `definitions = true`: the list is a namespace's mouth, which holds
    /// the definitions marked [`BlockDef::namespace_member`] and no
    /// statement (w3-modules.md §2.12). Written only when set.
    #[serde(default, skip_serializing_if = "is_false")]
    pub definitions: bool,
}

/// Repetition: parts `NAME0` … `NAME{count + plus - 1}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repeat {
    /// The `extra` key holding the count: a `count`, or a row extra
    /// (`params`, `fields` or `values`), whose rows are counted.
    pub count: String,
    /// Added to the count (e.g. 1 for `if` + `elseIfCount` branches).
    #[serde(default)]
    pub plus: u32,
}

/// Type classes for an input's `check`, a signature's `where` and a
/// parameter row's `type_class`: defined in `b2c-ir` with the type rules
/// (`docs/plans/m3-contracts/types.md` §1.3).
pub use b2c_ir::types::TypeClass;

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
    /// Maximum (for `count`; for a row extra, the maximum number of rows).
    pub max: u32,
    /// Default (for `count` and `flag`).
    #[serde(default)]
    pub default: Option<FieldDefault>,
    /// The types a `params` or `fields` row always offers, in C++ syntax, as
    /// a type field's `types`.
    #[serde(default, serialize_with = "serialize_types")]
    pub types: Vec<String>,
    /// `type_class` (for `params` and `fields`): the class of types a row
    /// also accepts besides `types`; `typeClass` in `catalog.json`.
    #[serde(
        default,
        rename(deserialize = "type_class", serialize = "typeClass"),
        skip_serializing_if = "Option::is_none"
    )]
    pub type_class: Option<TypeClass>,
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
    /// `{"sym": SymbolId, "name": String, "type": type term, "mode": "copy"|"editable"|"read_only"}`.
    Params,
    /// A struct's field rows: an array of
    /// `{"sym": SymbolId, "name": String, "type": type term}`.
    Fields,
    /// An enum's value rows: an array of `{"sym": SymbolId, "name": String}`.
    Values,
}

impl ExtraKind {
    /// Whether the extra holds rows, each declaring a symbol: `params`,
    /// `fields` or `values`. A row extra must have its kind's name.
    pub const fn is_rows(self) -> bool {
        matches!(self, Self::Params | Self::Fields | Self::Values)
    }

    /// The name a row extra must have (`params`, `fields`, `values`).
    pub const fn row_name(self) -> Option<&'static str> {
        match self {
            Self::Params => Some("params"),
            Self::Fields => Some("fields"),
            Self::Values => Some("values"),
            Self::Count | Self::Flag => None,
        }
    }

    /// The keys of a row of this kind, in the order the rows are written.
    pub const fn row_keys(self) -> &'static [&'static str] {
        match self {
            Self::Params => &["sym", "name", "type", "mode"],
            Self::Fields => &["sym", "name", "type"],
            Self::Values => &["sym", "name"],
            Self::Count | Self::Flag => &[],
        }
    }

    /// Whether a row of this kind has a `type`, and so `types` and a
    /// `type_class`.
    pub const fn has_row_types(self) -> bool {
        matches!(self, Self::Params | Self::Fields)
    }
}

/// `skip_serializing_if` for flags that are off by default.
#[allow(clippy::trivially_copy_pass_by_ref, reason = "the signature serde calls")]
fn is_false(flag: &bool) -> bool {
    !*flag
}

/// A type pattern as `catalog.json` writes it on a part (types.md §4.5):
/// JSON tagged by `kind`, such as `{"kind": "var", "name": "C"}`,
/// `{"kind": "fn", "fn": "elem", "of": …}`, `{"kind": "type", "type": "int"}`
/// (a fixed type as its term) or `{"kind": "param", "field": "FUNC"}`.
pub fn pattern_json(pattern: &TypePattern) -> Value {
    match pattern {
        TypePattern::Any => json!({"kind": "any"}),
        TypePattern::Number => json!({"kind": "number"}),
        TypePattern::Type(ty) => {
            let term = ty.to_term().and_then(|term| serde_json::to_value(term).ok());
            json!({"kind": "type", "type": term.unwrap_or(Value::Null)})
        }
        TypePattern::Var(var) => json!({"kind": "var", "name": var.as_str()}),
        TypePattern::Lib(ty, args) => json!({
            "kind": "lib",
            "name": ty.cpp_name(),
            "args": args.iter().map(pattern_json).collect::<Vec<_>>(),
        }),
        TypePattern::Fn(function, arg) => {
            json!({"kind": "fn", "fn": function.name(), "of": pattern_json(arg)})
        }
        TypePattern::Field(field) => json!({"kind": "field", "field": field}),
        TypePattern::Symbol(field) => json!({"kind": "symbol", "field": field}),
        TypePattern::Param(field) => json!({"kind": "param", "field": field}),
        TypePattern::Member(field) => json!({"kind": "member", "field": field}),
        TypePattern::Result => json!({"kind": "result"}),
    }
}

/// An entry of a `types` list as `catalog.json` writes it: its type term
/// (`"int"`, `"auto"`, `{"std::vector": "int"}`). An entry that is not a
/// type, which the catalog's checks refuse, is written as it is.
pub fn type_json(entry: &str) -> Value {
    let term = match TypePattern::parse(entry) {
        Ok(TypePattern::Type(ty)) => ty.to_term(),
        _ => TypeTerm::name(entry).ok(),
    };
    term.and_then(|term| serde_json::to_value(term).ok())
        .unwrap_or_else(|| Value::String(entry.to_owned()))
}

/// Writes a `types` list as type terms ([`type_json`]).
fn serialize_types<S: Serializer>(types: &[String], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(types.iter().map(|entry| type_json(entry)))
}

/// Reads a pattern from its C++ syntax (types.md §4.2).
fn parse_pattern<'de, D: Deserializer<'de>>(deserializer: D) -> Result<TypePattern, D::Error> {
    let text = String::deserialize(deserializer)?;
    TypePattern::parse(&text)
        .map_err(|error| serde::de::Error::custom(format!("{text:?} is not a type pattern: {error}")))
}

/// A block's `output`: read as a pattern, written as its text.
mod output_pattern {
    use b2c_ir::sig::TypePattern;
    use serde::{Deserializer, Serializer};

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<TypePattern>, D::Error> {
        super::parse_pattern(deserializer).map(Some)
    }

    #[allow(clippy::ref_option, reason = "the signature serde calls")]
    pub(super) fn serialize<S: Serializer>(
        pattern: &Option<TypePattern>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match pattern {
            Some(pattern) => serializer.collect_str(pattern),
            None => serializer.serialize_none(),
        }
    }
}

/// A part's `type`: read as a pattern, written as tagged JSON
/// ([`pattern_json`]).
mod part_pattern {
    use b2c_ir::sig::TypePattern;
    use serde::{Deserializer, Serialize, Serializer};

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<TypePattern>, D::Error> {
        super::parse_pattern(deserializer).map(Some)
    }

    #[allow(clippy::ref_option, reason = "the signature serde calls")]
    pub(super) fn serialize<S: Serializer>(
        pattern: &Option<TypePattern>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        pattern.as_ref().map(super::pattern_json).serialize(serializer)
    }
}
