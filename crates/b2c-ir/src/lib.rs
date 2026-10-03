//! Shared intermediate representations for Blocks2Cpp.
//!
//! This crate is the contract between the pipeline stages
//! (`docs/spec/06-compiler-pipeline.md`):
//!
//! * [`ids`]: validated IDs of blocks, symbols, modules and projects.
//! * [`diag`]: the diagnostics model every stage reports with.
//! * [`text`]: typed text leaves, the only path from user text to C++.
//! * [`types`]: static types.
//! * [`sast`]: the Semantic AST that analysis produces and generation consumes.
//! * [`source_map`]: generated files and the map back to blocks.
//!
//! It performs no I/O and compiles for `wasm32-unknown-unknown`.

pub mod diag;
pub mod ids;
mod reserved_names;
pub mod sast;
pub mod source_map;
pub mod text;
pub mod types;

pub use diag::{DiagCode, DiagSource, Diagnostic, Location, Part, Severity, has_errors};
pub use ids::{BlockId, IdError, ModuleId, ProjectId, SymbolId};
pub use types::Type;
