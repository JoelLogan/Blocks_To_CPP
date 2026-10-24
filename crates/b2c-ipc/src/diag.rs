//! The diagnostic DTO (`docs/spec/06-compiler-pipeline.md` §6.12).
//!
//! A copy of [`b2c_ir::Diagnostic`] whose JSON is byte-identical to the shared
//! type's, which is also the CLI's `--format json` shape. Every key and value of
//! that shape is a single lower-case word, so camelCase and `snake_case` agree and
//! the DTO follows the IPC convention without changing a byte. The pure compiler
//! crates stay free of TypeScript generation; this copy carries it instead.

use serde::{Deserialize, Serialize};

use crate::macros::string_enum;

string_enum! {
    /// How serious a diagnostic is.
    pub enum Severity {
        /// Informational; never blocks a build.
        Info = "info",
        /// Probably a mistake; does not block a build.
        Warning = "warning",
        /// Blocks building and running.
        Error = "error",
    }
}

string_enum! {
    /// Which stage produced a diagnostic.
    pub enum DiagSource {
        /// Loading and validating the project file.
        Loader = "loader",
        /// Checking blocks against the catalog.
        Catalog = "catalog",
        /// Names, types and lints.
        Analyser = "analyser",
        /// Generating C++.
        Generator = "generator",
        /// Finding or probing the compiler.
        Toolchain = "toolchain",
        /// The C++ compiler.
        Compiler = "compiler",
        /// The linker.
        Linker = "linker",
        /// The running program.
        Runtime = "runtime",
    }
}

/// The part of a block a diagnostic points at.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Part {
    /// The whole block.
    Whole,
    /// A named field, for example `NAME`.
    Field {
        /// The field's name in the catalog.
        name: String,
    },
    /// A named value input, for example `COND0`.
    Input {
        /// The input's name in the catalog.
        name: String,
    },
    /// A token range inside an expression slot.
    Tokens {
        /// The input name of the expression slot.
        input: String,
        /// The index of the first token.
        start: u32,
        /// The index one past the last token.
        end: u32,
    },
}

/// Where a diagnostic points.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Location {
    /// The module's ID, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub module: Option<String>,
    /// The block's ID, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub block: Option<String>,
    /// The part of the block.
    pub part: Part,
}

/// A secondary location with its own message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Related {
    /// Where.
    pub location: Location,
    /// What it says about that place.
    pub message: String,
}

/// A problem found while loading, analysing, generating, compiling or running.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    /// The stable code, for example `B2C-E0201`.
    pub code: String,
    /// How serious it is.
    pub severity: Severity,
    /// The friendly, plain-language message.
    pub message: String,
    /// The main location.
    pub primary: Location,
    /// Other relevant locations; omitted from the JSON when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<Related>,
    /// Which stage produced it.
    pub source: DiagSource,
    /// The original compiler or linker text, when the source is not ours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub raw: Option<String>,
}

impl From<b2c_ir::Severity> for Severity {
    fn from(severity: b2c_ir::Severity) -> Self {
        match severity {
            b2c_ir::Severity::Info => Self::Info,
            b2c_ir::Severity::Warning => Self::Warning,
            b2c_ir::Severity::Error => Self::Error,
        }
    }
}

impl From<b2c_ir::DiagSource> for DiagSource {
    fn from(source: b2c_ir::DiagSource) -> Self {
        match source {
            b2c_ir::DiagSource::Loader => Self::Loader,
            b2c_ir::DiagSource::Catalog => Self::Catalog,
            b2c_ir::DiagSource::Analyser => Self::Analyser,
            b2c_ir::DiagSource::Generator => Self::Generator,
            b2c_ir::DiagSource::Toolchain => Self::Toolchain,
            b2c_ir::DiagSource::Compiler => Self::Compiler,
            b2c_ir::DiagSource::Linker => Self::Linker,
            b2c_ir::DiagSource::Runtime => Self::Runtime,
        }
    }
}

impl From<&b2c_ir::Part> for Part {
    fn from(part: &b2c_ir::Part) -> Self {
        match part {
            b2c_ir::Part::Whole => Self::Whole,
            b2c_ir::Part::Field { name } => Self::Field { name: name.clone() },
            b2c_ir::Part::Input { name } => Self::Input { name: name.clone() },
            b2c_ir::Part::Tokens { input, start, end } => Self::Tokens {
                input: input.clone(),
                start: *start,
                end: *end,
            },
        }
    }
}

impl From<&b2c_ir::Location> for Location {
    fn from(location: &b2c_ir::Location) -> Self {
        Self {
            module: location.module.as_ref().map(|m| m.as_str().to_owned()),
            block: location.block.as_ref().map(|b| b.as_str().to_owned()),
            part: Part::from(&location.part),
        }
    }
}

impl From<&b2c_ir::diag::Related> for Related {
    fn from(related: &b2c_ir::diag::Related) -> Self {
        Self {
            location: Location::from(&related.location),
            message: related.message.clone(),
        }
    }
}

impl From<&b2c_ir::Diagnostic> for Diagnostic {
    fn from(diagnostic: &b2c_ir::Diagnostic) -> Self {
        Self {
            code: diagnostic.code.0.clone(),
            severity: diagnostic.severity.into(),
            message: diagnostic.message.clone(),
            primary: Location::from(&diagnostic.primary),
            related: diagnostic.related.iter().map(Related::from).collect(),
            source: diagnostic.source.into(),
            raw: diagnostic.raw.clone(),
        }
    }
}

/// Converts a list of shared diagnostics.
pub fn convert_all(diagnostics: &[b2c_ir::Diagnostic]) -> Vec<Diagnostic> {
    diagnostics.iter().map(Diagnostic::from).collect()
}
