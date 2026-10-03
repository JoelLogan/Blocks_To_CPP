//! The Block Document Model: typed structure of a `.b2c` project file
//! (`docs/spec/05-project-format.md` §5.3–5.5).
//!
//! These types derive `serde` so tests and tools can build documents directly,
//! but untrusted input must go through [`crate::load`], which adds the limits,
//! duplicate-key rejection and text rules of §5.6 that plain `serde` does not.

use std::collections::BTreeMap;

use b2c_ir::ids::{BlockId, ModuleId, ProjectId, SymbolId};
use b2c_ir::sast::CppStandard;
use serde::{Deserialize, Serialize};

/// The value of the top-level `"format"` key.
pub const FORMAT_TAG: &str = "blocks2cpp/project";

/// The project format version this build reads and writes.
pub const CURRENT_FORMAT_VERSION: u32 = 1;

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

fn default_true() -> bool {
    true
}

/// A whole project file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Document {
    /// Always [`FORMAT_TAG`].
    pub format: String,
    /// File format version (see [`CURRENT_FORMAT_VERSION`]).
    pub format_version: u32,
    /// What wrote the file.
    pub generator: Generator,
    /// Project settings.
    pub project: Project,
    /// Modules (at least one).
    pub modules: Vec<Module>,
    /// Forward-compatible tooling metadata: preserved, never interpreted.
    #[serde(rename = "x-ext", default, skip_serializing_if = "Option::is_none")]
    pub ext: Option<serde_json::Value>,
}

/// Versions of the app and catalog that last saved the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generator {
    /// App version, e.g. `0.1.0`.
    pub app: String,
    /// Catalog version, e.g. `1.0.0`.
    pub catalog: String,
}

/// Project-wide settings. Every option is a closed enum or a validated scalar;
/// there are no free-form compiler flags, paths or commands (ADR-0005).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Project {
    /// Project ID.
    pub id: ProjectId,
    /// Display name.
    pub name: String,
    /// Optional description.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Language settings.
    pub language: Language,
    /// Editor and generation options.
    #[serde(default)]
    pub options: ProjectOptions,
    /// Build settings.
    #[serde(default)]
    pub build: BuildSettings,
    /// Run settings.
    #[serde(default)]
    pub run: RunSettings,
}

/// Language settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Language {
    /// C++ standard.
    pub standard: CppStandard,
    /// Use `gnu++NN` instead of `c++NN`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub gnu_extensions: bool,
}

/// Editor and generation options (spec §5.3).
#[allow(clippy::struct_excessive_bools)] // mirrors independent options in the file format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectOptions {
    /// Show advanced blocks in the toolbox.
    #[serde(default)]
    pub show_advanced: bool,
    /// Allow raw `new`/`delete` blocks.
    #[serde(default)]
    pub manual_memory: bool,
    /// Prefer inline standard C++ over support helpers where possible.
    #[serde(default)]
    pub prefer_plain_std: bool,
    /// How `format`/`join` are generated.
    #[serde(default)]
    pub formatting_style: FormattingStyle,
    /// Bounds-checked indexing (`.at()`) by default.
    #[serde(default = "default_true")]
    pub checked_indexing: bool,
}

impl Default for ProjectOptions {
    fn default() -> Self {
        Self {
            show_advanced: false,
            manual_memory: false,
            prefer_plain_std: false,
            formatting_style: FormattingStyle::Stream,
            checked_indexing: true,
        }
    }
}

/// How text formatting is generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormattingStyle {
    /// `+`, `std::to_string` and streams.
    #[default]
    Stream,
    /// `std::format` (C++20 with GCC 13+).
    Format,
}

/// Build settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildSettings {
    /// Debug and release configurations.
    #[serde(default)]
    pub configurations: Configurations,
    /// Preprocessor defines with typed values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defines: Vec<Define>,
    /// Library names resolved through machine-local library profiles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub libraries: Vec<String>,
    /// Library packs the project uses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packs: Vec<PackRef>,
}

/// The two build configurations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Configurations {
    /// Debug configuration.
    pub debug: BuildConfiguration,
    /// Release configuration.
    pub release: BuildConfiguration,
}

impl Default for Configurations {
    fn default() -> Self {
        Self {
            debug: BuildConfiguration {
                optimization: Optimization::None,
                debug_info: true,
                sanitizers: vec![Sanitizer::Address, Sanitizer::Undefined],
                warnings: WarningLevel::Helpful,
                warnings_as_errors: false,
                hardening: true,
            },
            release: BuildConfiguration {
                optimization: Optimization::Speed,
                debug_info: false,
                sanitizers: Vec::new(),
                warnings: WarningLevel::Helpful,
                warnings_as_errors: false,
                hardening: true,
            },
        }
    }
}

/// One build configuration (spec §7.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildConfiguration {
    /// Optimisation level.
    pub optimization: Optimization,
    /// Emit debug information.
    pub debug_info: bool,
    /// Sanitizers to enable when the toolchain supports them.
    #[serde(default)]
    pub sanitizers: Vec<Sanitizer>,
    /// Warning level.
    pub warnings: WarningLevel,
    /// Treat warnings as errors.
    #[serde(default, skip_serializing_if = "is_false")]
    pub warnings_as_errors: bool,
    /// Hardening flags for the produced program.
    pub hardening: bool,
}

/// Optimisation level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Optimization {
    /// `-O0`.
    None,
    /// `-Og`.
    Debug,
    /// `-O2`.
    Speed,
    /// `-Os`.
    Size,
}

/// A sanitizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sanitizer {
    /// AddressSanitizer.
    Address,
    /// UndefinedBehaviorSanitizer.
    Undefined,
}

/// Warning level (spec §7.4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningLevel {
    /// `-Wall`.
    Minimal,
    /// `-Wall -Wextra -Wpedantic`.
    Helpful,
    /// `helpful` plus extra checks.
    Strict,
}

/// A preprocessor define. The name must be a valid identifier and the value
/// is typed; the backend renders `-DNAME=value` itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Define {
    /// Macro name.
    pub name: String,
    /// Typed value.
    pub value: DefineValue,
}

/// A define's value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DefineValue {
    /// Integer.
    Int(i64),
    /// Boolean (`1` / `0`).
    Bool(bool),
    /// String (emitted as an escaped string literal).
    String(String),
}

/// A library pack reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackRef {
    /// Pack ID.
    pub id: String,
    /// SemVer requirement.
    pub version: String,
}

/// Run settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunSettings {
    /// Command-line arguments (an argv list, never a shell string).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Working directory for the program.
    #[serde(default)]
    pub working_directory: WorkingDirectory,
}

/// Where the program runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingDirectory {
    /// The folder containing the project file.
    #[default]
    Project,
    /// A per-project sandbox folder in the build cache.
    Sandbox,
}

/// One module (becomes `<name>.cpp`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Module {
    /// Module ID.
    pub id: ModuleId,
    /// Module name: `[a-z][a-z0-9_-]{0,63}`, not a Windows device name, unique
    /// case-insensitively (it becomes a file name).
    pub name: String,
    /// The module's canvas.
    pub workspace: Workspace,
}

/// A module's canvas.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    /// Top-level blocks (canonical order: sorted by ID).
    #[serde(default)]
    pub blocks: Vec<Block>,
    /// Grouping frames (layout only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<Frame>,
    /// Sticky notes (layout only, never emitted).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    /// Saved viewport.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<Viewport>,
}

/// A titled frame grouping top-level blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Frame {
    /// Frame ID.
    pub id: BlockId,
    /// Title.
    pub title: String,
    /// Left.
    pub x: i32,
    /// Top.
    pub y: i32,
    /// Width.
    pub w: i32,
    /// Height.
    pub h: i32,
    /// Colour name from the theme palette.
    pub color: FrameColor,
    /// Emit a `// ===== Title =====` banner comment.
    #[serde(default, skip_serializing_if = "is_false")]
    pub emit_banner: bool,
}

/// Frame colours (theme tokens).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameColor {
    /// Neutral grey.
    Grey,
    /// Blue.
    Blue,
    /// Green.
    Green,
    /// Yellow.
    Yellow,
    /// Orange.
    Orange,
    /// Purple.
    Purple,
}

/// A sticky note on the canvas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Note {
    /// Note ID.
    pub id: BlockId,
    /// Text.
    pub text: String,
    /// Left.
    pub x: i32,
    /// Top.
    pub y: i32,
}

/// Saved viewport.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Viewport {
    /// Scroll X.
    pub x: i32,
    /// Scroll Y.
    pub y: i32,
    /// Zoom (0.1–4.0).
    pub scale: f64,
}

/// One block (spec §5.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    /// Block ID, unique in the project.
    pub id: BlockId,
    /// Catalog block type, e.g. `control.if`.
    #[serde(rename = "type")]
    pub block_type: String,
    /// Catalog block version.
    pub v: u32,
    /// Canvas X (top-level blocks only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    /// Canvas Y (top-level blocks only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
    /// Shown collapsed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub collapsed: bool,
    /// Excluded from generation.
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
    /// Comment emitted as `//` lines above the generated code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<BlockComment>,
    /// Mutator state (variadic counts, parameter rows), validated against the
    /// catalog's schema for the block type.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, serde_json::Value>,
    /// Field values.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, FieldValue>,
    /// Value inputs. An absent input means the catalog default.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, Input>,
    /// Statement inputs, as arrays (never linked `next` chains).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub statements: BTreeMap<String, Vec<Block>>,
}

/// A block comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockComment {
    /// Comment text.
    pub text: String,
    /// Whether the bubble is pinned open in the editor.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
}

/// A field value. Numbers are stored as text so they keep their exact form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FieldValue {
    /// Checkbox.
    Bool(bool),
    /// Text, number text, dropdown value or type name.
    Text(String),
    /// Declares a symbol: `{"sym": "sym_x", "name": "score"}`.
    Decl(SymbolDecl),
    /// References a symbol: `{"ref": "sym_x"}`.
    Ref(SymbolRef),
}

/// A symbol declaration in a field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolDecl {
    /// Stable symbol ID.
    pub sym: SymbolId,
    /// User-chosen name (validated as an identifier during analysis).
    pub name: String,
}

/// A symbol reference in a field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolRef {
    /// The referenced symbol.
    #[serde(rename = "ref")]
    pub target: SymbolId,
}

/// A value input: a nested reporter block or an expression slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Input {
    /// `{"block": {…}}`.
    Block(BlockInput),
    /// `{"expr": [tokens], "draft": false}`.
    Expr(ExprInput),
}

/// A nested reporter block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockInput {
    /// The block.
    pub block: Box<Block>,
}

/// An expression slot (spec §3.4): a flat token list in which symbols are
/// referenced by ID, so renames never break expressions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExprInput {
    /// Tokens.
    pub expr: Vec<Token>,
    /// The text is an unfinished draft that does not parse yet.
    #[serde(default, skip_serializing_if = "is_false")]
    pub draft: bool,
}

/// One expression-slot token. Serialised as a single-key object, e.g.
/// `{"num": "42"}` or `{"ref": "sym_7Qk2"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Token {
    /// Numeric literal text (`"42"`, `"3.14"`, `"0x2A"`).
    Num(String),
    /// String literal value (unescaped).
    Str(String),
    /// Character literal value (one character).
    Chr(String),
    /// Reference to a variable, parameter or function.
    Ref(SymbolId),
    /// Operator or punctuation: `+ - * / % == != < <= > >= && || ! and or not ( ) , ? :`.
    Op(String),
    /// Keyword literal: `true` or `false`.
    Kw(String),
    /// Unparsed draft text (only with `draft: true`).
    Text(String),
}
