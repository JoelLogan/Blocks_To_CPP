//! The diagnostics model shared by every stage (spec §6.12).

use serde::{Deserialize, Serialize};

use crate::ids::{BlockId, ModuleId};

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Informational; never blocks a build.
    Info,
    /// Probably a mistake; does not block a build.
    Warning,
    /// Blocks building and running.
    Error,
}

/// Which stage produced a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagSource {
    /// Loading and validating the project file (b2c-model).
    Loader,
    /// Checking blocks against the catalog (b2c-catalog).
    Catalog,
    /// Names, types and lints (b2c-lang).
    Analyser,
    /// Generating C++ (b2c-codegen).
    Generator,
    /// Finding or probing the compiler (b2c-toolchain).
    Toolchain,
    /// The C++ compiler.
    Compiler,
    /// The linker.
    Linker,
    /// The running program.
    Runtime,
}

/// The part of a block a diagnostic points at.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Part {
    /// The whole block.
    Whole,
    /// A named field, e.g. `NAME`.
    Field {
        /// Field name from the catalog.
        name: String,
    },
    /// A named value input, e.g. `COND0`.
    Input {
        /// Input name from the catalog.
        name: String,
    },
    /// A token range inside an expression slot (start inclusive, end exclusive).
    Tokens {
        /// Input name of the expression slot.
        input: String,
        /// Index of the first token.
        start: u32,
        /// Index one past the last token.
        end: u32,
    },
}

/// Where a diagnostic points. All fields are optional because some problems
/// (e.g. an unreadable file) have no block, and some have no module.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Location {
    /// The module, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<ModuleId>,
    /// The block, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<BlockId>,
    /// The part of the block.
    pub part: Part,
}

impl Location {
    /// A location with no module or block (whole project).
    pub fn project() -> Self {
        Self { module: None, block: None, part: Part::Whole }
    }

    /// The whole of a block.
    pub fn block(module: Option<ModuleId>, block: BlockId) -> Self {
        Self { module, block: Some(block), part: Part::Whole }
    }

    /// Returns this location pointing at a different part of the same block.
    #[must_use]
    pub fn with_part(mut self, part: Part) -> Self {
        self.part = part;
        self
    }
}

/// A stable diagnostic code, e.g. `B2C-E0201` (spec §6.12 table).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DiagCode(pub String);

impl DiagCode {
    /// A code from a static string.
    pub fn new(code: &str) -> Self {
        Self(code.to_owned())
    }
}

/// A secondary location with its own message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Related {
    /// Where.
    pub location: Location,
    /// What it says about that place.
    pub message: String,
}

/// A problem found while loading, analysing, generating, compiling or running.
///
/// `message` is plain English for now; it becomes a localisation key plus
/// arguments when the i18n layer lands (spec §4.9), keyed by `code`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Stable code; every code is documented in `docs/reference/diagnostics/`.
    pub code: DiagCode,
    /// Severity.
    pub severity: Severity,
    /// Friendly, plain-language message.
    pub message: String,
    /// Main location.
    pub primary: Location,
    /// Other relevant locations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<Related>,
    /// Which stage produced it.
    pub source: DiagSource,
    /// The original compiler or linker text, when `source` is not ours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

impl Diagnostic {
    /// Creates an error.
    pub fn error(code: &str, source: DiagSource, primary: Location, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, code, source, primary, message)
    }

    /// Creates a warning.
    pub fn warning(code: &str, source: DiagSource, primary: Location, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, code, source, primary, message)
    }

    /// Creates an info diagnostic.
    pub fn info(code: &str, source: DiagSource, primary: Location, message: impl Into<String>) -> Self {
        Self::new(Severity::Info, code, source, primary, message)
    }

    fn new(
        severity: Severity,
        code: &str,
        source: DiagSource,
        primary: Location,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: DiagCode::new(code),
            severity,
            message: message.into(),
            primary,
            related: Vec::new(),
            source,
            raw: None,
        }
    }

    /// Adds a related location.
    #[must_use]
    pub fn with_related(mut self, location: Location, message: impl Into<String>) -> Self {
        self.related.push(Related { location, message: message.into() });
        self
    }
}

/// Whether any diagnostic in the list is an error.
pub fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(|d| d.severity == Severity::Error)
}
