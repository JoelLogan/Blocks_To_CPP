//! Shared intermediate representations for Blocks2Cpp.
//!
//! This crate is the contract between the pipeline stages
//! (`docs/spec/06-compiler-pipeline.md`):
//!
//! * [`ids`]: validated IDs of blocks, symbols, modules and projects.
//! * [`diag`]: the diagnostics model every stage reports with.
//! * [`text`]: typed text leaves, the only path from user text to C++, and
//!   the file stems module names become.
//! * [`types`]: static types, type terms, classes and type functions.
//! * [`sig`]: catalog signatures, and the binding of a block's type
//!   variables.
//! * [`headers`]: the closed table of standard headers, and the headers of
//!   an item.
//! * [`sast`]: the Semantic AST that analysis produces and generation consumes.
//! * [`layout`]: which files each module gets.
//! * [`scope_info`]: what the editor learns about a symbol from a scope query.
//! * [`source_map`]: generated files and the map back to blocks.
//!
//! It performs no I/O and compiles for `wasm32-unknown-unknown`.

pub mod diag;
pub mod headers;
pub mod ids;
pub mod layout;
mod reserved_names;
pub mod sast;
pub mod scope_info;
pub mod sig;
pub mod source_map;
pub mod text;
pub mod types;

pub use diag::{DiagCode, DiagSource, Diagnostic, Location, Part, Severity, has_errors};
pub use headers::{Header, ItemHeaders};
pub use ids::{BlockId, IdError, ModuleId, ProjectId, SymbolId};
pub use layout::{Layout, ModuleFiles};
pub use scope_info::{SymbolInfo, SymbolInfoKind};
pub use sig::{
    BlockParts, BlockSig, Fit, PartName, PartType, Resolution, SigTable, TypeEnv, TypePattern, TypeVar,
};
pub use types::{LibType, TermForm, Type, TypeClass, TypeFacts, TypeFn, TypeTerm};
