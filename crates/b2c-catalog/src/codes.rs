//! Catalog diagnostic codes (`B2C-E06xx`), documented in
//! `docs/reference/diagnostics/loader-and-catalog.md`, and a collector that
//! caps how many diagnostics one document can produce.

use b2c_ir::{DiagSource, Diagnostic, Location};

/// The catalog has no block definitions at all.
pub(crate) const EMPTY_CATALOG: &str = "B2C-E0600";
/// A block type the catalog does not define.
pub(crate) const UNKNOWN_BLOCK: &str = "B2C-E0601";
/// A block saved by a newer catalog version.
pub(crate) const NEWER_BLOCK: &str = "B2C-E0602";
/// A block of an older version that cannot be upgraded.
pub(crate) const OLD_BLOCK: &str = "B2C-E0603";
/// A block in a place its shape does not allow.
pub(crate) const WRONG_PLACE: &str = "B2C-E0604";
/// A field the block type does not have.
pub(crate) const UNKNOWN_FIELD: &str = "B2C-E0605";
/// A required field is missing.
pub(crate) const MISSING_FIELD: &str = "B2C-E0606";
/// A field value that does not fit the field.
pub(crate) const BAD_FIELD: &str = "B2C-E0607";
/// A value input the block type does not have.
pub(crate) const UNKNOWN_INPUT: &str = "B2C-E0608";
/// A required value input is missing.
pub(crate) const MISSING_INPUT: &str = "B2C-E0609";
/// A statement input the block type does not have.
pub(crate) const UNKNOWN_STATEMENT: &str = "B2C-E0610";
/// An `extra` key the block type does not have.
pub(crate) const UNKNOWN_EXTRA: &str = "B2C-E0611";
/// A required `extra` value is missing.
pub(crate) const MISSING_EXTRA: &str = "B2C-E0612";
/// An `extra` value of the wrong kind or out of range.
pub(crate) const BAD_EXTRA: &str = "B2C-E0613";
/// A malformed function parameter row.
pub(crate) const BAD_PARAM: &str = "B2C-E0614";
/// A broken definition in the catalog itself.
pub(crate) const BROKEN_DEFINITION: &str = "B2C-E0620";
/// More problems than are listed.
pub(crate) const TOO_MANY_PROBLEMS: &str = "B2C-E0699";

/// Maximum diagnostics listed for one document; the rest are counted.
pub(crate) const MAX_DIAGNOSTICS: usize = 10_000;

/// Collects catalog diagnostics, keeping at most [`MAX_DIAGNOSTICS`].
#[derive(Debug, Default)]
pub(crate) struct Diags {
    list: Vec<Diagnostic>,
    dropped: usize,
}

impl Diags {
    /// Adds an error.
    pub(crate) fn error(&mut self, code: &str, primary: Location, message: impl Into<String>) {
        if self.list.len() < MAX_DIAGNOSTICS {
            self.list
                .push(Diagnostic::error(code, DiagSource::Catalog, primary, message));
        } else {
            self.dropped += 1;
        }
    }

    /// The diagnostics, with a final note when some were not listed.
    pub(crate) fn finish(mut self) -> Vec<Diagnostic> {
        if self.dropped > 0 {
            let message = format!(
                "{} more problem(s) with blocks were found but are not listed. Fix the problems above first.",
                self.dropped
            );
            self.list.push(Diagnostic::error(
                TOO_MANY_PROBLEMS,
                DiagSource::Catalog,
                Location::project(),
                message,
            ));
        }
        self.list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_are_capped() {
        let mut diags = Diags::default();
        for _ in 0..MAX_DIAGNOSTICS + 3 {
            diags.error(UNKNOWN_FIELD, Location::project(), "x");
        }
        let diagnostics = diags.finish();
        assert_eq!(diagnostics.len(), MAX_DIAGNOSTICS + 1);
        let note = diagnostics.last().unwrap();
        assert_eq!(note.code.0, TOO_MANY_PROBLEMS);
        assert_eq!(note.source, DiagSource::Catalog);
        assert!(note.message.starts_with("3 more problem(s)"));
    }
}
