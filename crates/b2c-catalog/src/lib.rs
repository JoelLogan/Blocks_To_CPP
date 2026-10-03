//! The block catalog: definitions of every block type (spec §3.11) and the
//! "resolve" pipeline stage that checks a project against them (spec §6.3).
//!
//! CONTRACT (implemented in milestone M1):
//! * [`core_catalog`] parses the TOML files under `catalog/core/` (embedded
//!   with `include_str!`), validates them (unique IDs, field/input/extra
//!   definitions consistent, dropdown defaults among the options, repeat and
//!   `when` keys refer to declared extras, default tokens well-formed) and
//!   caches the result. A broken built-in catalog is a bug: tests must load it.
//! * [`resolve`] checks every block of a document against the catalog and
//!   returns a copy of the document with absent inputs filled from catalog
//!   defaults, plus diagnostics (`DiagSource::Catalog`, codes `B2C-E06xx`):
//!   unknown block type or newer version; wrong shape for the position
//!   (top-level vs statement vs value input); unknown, missing or ill-typed
//!   fields (dropdown value not an option, type not allowed, decl/ref shape);
//!   inputs and statement inputs not matching the definition after expanding
//!   `repeat` counts and `when` flags; `extra` keys unknown, missing (when no
//!   default) or out of range; `params` rows well-formed.

pub mod schema;

use b2c_ir::Diagnostic;
use b2c_model::Document;

pub use schema::*;

/// The catalog version (semver of the `catalog/` directory).
pub const CATALOG_VERSION: &str = "1.0.0";

/// A loaded, validated catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    /// Catalog version.
    pub version: String,
    /// Block definitions by ID.
    pub blocks: std::collections::BTreeMap<String, BlockDef>,
}

/// The built-in core catalog.
pub fn core_catalog() -> &'static Catalog {
    todo!("implemented in milestone M1")
}

/// Checks a document against the catalog and fills absent inputs from defaults.
pub fn resolve(document: &Document, catalog: &Catalog) -> (Document, Vec<Diagnostic>) {
    let _ = (document, catalog);
    todo!("implemented in milestone M1")
}
