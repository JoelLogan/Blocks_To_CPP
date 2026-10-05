//! Loader diagnostic codes (`B2C-E01xx`), documented in
//! `docs/reference/diagnostics/loader-and-catalog.md`, and a collector that
//! caps how many diagnostics one file can produce.

use b2c_ir::{DiagSource, Diagnostic, Location};

/// The file is larger than the size limit.
pub(crate) const FILE_TOO_LARGE: &str = "B2C-E0101";
/// The file is not UTF-8 text without a byte order mark.
pub(crate) const NOT_UTF8: &str = "B2C-E0102";
/// The file is not valid JSON.
pub(crate) const JSON_SYNTAX: &str = "B2C-E0103";
/// Lists and objects are nested too deeply.
pub(crate) const TOO_DEEP: &str = "B2C-E0104";
/// The same key appears twice in one object.
pub(crate) const DUPLICATE_KEY: &str = "B2C-E0105";
/// The file holds too many JSON values.
pub(crate) const TOO_MANY_VALUES: &str = "B2C-E0106";
/// The file is not a Blocks2Cpp project.
pub(crate) const NOT_A_PROJECT: &str = "B2C-E0107";
/// The file was made by a newer version of Blocks2Cpp.
pub(crate) const NEWER_FORMAT: &str = "B2C-E0108";
/// The format version is missing, malformed or cannot be upgraded.
pub(crate) const BAD_FORMAT_VERSION: &str = "B2C-E0109";
/// A key that the format does not define.
pub(crate) const UNKNOWN_KEY: &str = "B2C-E0110";
/// A required key is missing.
pub(crate) const MISSING_KEY: &str = "B2C-E0111";
/// A value of the wrong kind, an unknown choice or a number out of range.
pub(crate) const WRONG_VALUE: &str = "B2C-E0112";
/// A malformed ID.
pub(crate) const BAD_ID: &str = "B2C-E0113";
/// Two blocks, frames or notes share an ID.
pub(crate) const DUPLICATE_BLOCK_ID: &str = "B2C-E0114";
/// A symbol ID is declared more than once.
pub(crate) const DUPLICATE_SYMBOL: &str = "B2C-E0115";
/// Two modules share an ID.
pub(crate) const DUPLICATE_MODULE_ID: &str = "B2C-E0116";
/// A module name that cannot be used as a file name.
pub(crate) const BAD_MODULE_NAME: &str = "B2C-E0117";
/// A module named like a Windows device (`con`, `nul`, `com1`, …).
pub(crate) const DEVICE_MODULE_NAME: &str = "B2C-E0118";
/// Two module names differ only in letter case (or not at all).
pub(crate) const MODULE_NAME_CLASH: &str = "B2C-E0119";
/// No modules, or too many.
pub(crate) const MODULE_COUNT: &str = "B2C-E0120";
/// Too many blocks.
pub(crate) const TOO_MANY_BLOCKS: &str = "B2C-E0121";
/// Too many tokens in one expression slot.
pub(crate) const TOO_MANY_TOKENS: &str = "B2C-E0122";
/// Text or a name is too long.
pub(crate) const TOO_LONG: &str = "B2C-E0123";
/// Text contains the NUL character.
pub(crate) const NUL_CHAR: &str = "B2C-E0124";
/// Text contains a control character.
pub(crate) const CONTROL_CHAR: &str = "B2C-E0125";
/// Text contains a bidirectional control character.
pub(crate) const BIDI_CHAR: &str = "B2C-E0126";
/// A `__proto__`, `constructor` or `prototype` key.
pub(crate) const RESERVED_KEY: &str = "B2C-E0127";
/// `x`/`y` on a block that is not on the canvas itself.
pub(crate) const NESTED_POSITION: &str = "B2C-E0128";
/// A coordinate or size out of range.
pub(crate) const BAD_COORDINATE: &str = "B2C-E0129";
/// A viewport zoom out of range.
pub(crate) const BAD_ZOOM: &str = "B2C-E0130";
/// A variadic count or list in `extra` above the limit.
pub(crate) const TOO_MANY_PARTS: &str = "B2C-E0131";
/// A preprocessor define whose name is not a valid identifier.
pub(crate) const BAD_DEFINE_NAME: &str = "B2C-E0132";
/// A malformed library name.
pub(crate) const BAD_LIBRARY_NAME: &str = "B2C-E0133";
/// Two preprocessor defines with the same name.
pub(crate) const DUPLICATE_DEFINE: &str = "B2C-E0134";
/// Too much free-form data in `extra` and `x-ext`.
pub(crate) const TOO_MUCH_EXTRA_DATA: &str = "B2C-E0135";
/// A malformed library pack reference.
pub(crate) const BAD_PACK: &str = "B2C-E0136";
/// The same library pack is listed twice.
pub(crate) const DUPLICATE_PACK: &str = "B2C-E0137";
/// The data is not Blocks2Cpp clipboard data.
pub(crate) const NOT_CLIPBOARD: &str = "B2C-E0138";
/// A `stack` on a block that is not directly on the canvas.
pub(crate) const MISPLACED_STACK: &str = "B2C-E0139";
/// More problems than are listed.
pub(crate) const TOO_MANY_PROBLEMS: &str = "B2C-E0199";

/// Maximum diagnostics listed for one file; the rest are counted.
pub(crate) const MAX_DIAGNOSTICS: usize = 1000;

/// Collects loader diagnostics, keeping at most [`MAX_DIAGNOSTICS`].
#[derive(Debug, Default)]
pub(crate) struct Diags {
    list: Vec<Diagnostic>,
    dropped: usize,
}

impl Diags {
    /// Adds an error.
    pub(crate) fn error(&mut self, code: &str, primary: Location, message: impl Into<String>) {
        self.push(Diagnostic::error(code, DiagSource::Loader, primary, message));
    }

    /// Adds a diagnostic.
    pub(crate) fn push(&mut self, diagnostic: Diagnostic) {
        if self.list.len() < MAX_DIAGNOSTICS {
            self.list.push(diagnostic);
        } else {
            self.dropped += 1;
        }
    }

    /// Counts problems that were found but not described individually.
    pub(crate) fn add_unlisted(&mut self, count: usize) {
        self.dropped += count;
    }

    /// Whether anything was reported.
    pub(crate) fn is_empty(&self) -> bool {
        self.list.is_empty() && self.dropped == 0
    }

    /// The diagnostics, with a final note when some were not listed.
    pub(crate) fn finish(mut self) -> Vec<Diagnostic> {
        if self.dropped > 0 {
            let message = format!(
                "{} more problem(s) were found but are not listed. Fix the problems above and load the file again.",
                self.dropped
            );
            self.list.push(Diagnostic::error(
                TOO_MANY_PROBLEMS,
                DiagSource::Loader,
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
        assert!(diags.is_empty());
        for _ in 0..MAX_DIAGNOSTICS + 5 {
            diags.error(UNKNOWN_KEY, Location::project(), "x");
        }
        diags.add_unlisted(2);
        let diagnostics = diags.finish();
        assert_eq!(diagnostics.len(), MAX_DIAGNOSTICS + 1);
        let note = diagnostics.last().unwrap();
        assert_eq!(note.code.0, TOO_MANY_PROBLEMS);
        assert!(note.message.starts_with("7 more problem(s)"));
    }
}
