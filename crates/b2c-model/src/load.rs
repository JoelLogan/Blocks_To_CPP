//! Loading untrusted project bytes (spec §5.6, §5.7, §6.2).
//!
//! CONTRACT (implemented in milestone M1):
//! * Reject input larger than [`crate::limits::MAX_FILE_BYTES`] before parsing.
//! * Parse JSON rejecting duplicate keys anywhere and nesting deeper than
//!   [`crate::limits::MAX_JSON_DEPTH`], then decode into [`crate::Document`]
//!   with unknown keys rejected (except inside `"x-ext"`).
//! * Check `format == FORMAT_TAG`; refuse newer `formatVersion`s with a clear
//!   message; migrate older ones (none exist yet for version 1).
//! * Enforce every limit in [`crate::limits`], unique block/module/symbol IDs,
//!   module-name rules, and the text rules: valid UTF-8, no NUL, no C0 controls
//!   other than tab and newline, no Unicode bidi controls, in every string.
//! * Report problems as `b2c_ir::Diagnostic`s with `DiagSource::Loader` and
//!   codes `B2C-E01xx`, pointing at the block when there is one.

use b2c_ir::Diagnostic;

use crate::Document;

/// Why a project could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the project could not be loaded ({} problem(s))", .diagnostics.len())]
pub struct LoadError {
    /// Every problem found (at least one).
    pub diagnostics: Vec<Diagnostic>,
}

/// Parses and validates untrusted project bytes.
///
/// # Errors
/// Returns every problem found when the input is not a valid project.
pub fn load(bytes: &[u8]) -> Result<Document, LoadError> {
    let _ = bytes;
    todo!("implemented in milestone M1")
}
