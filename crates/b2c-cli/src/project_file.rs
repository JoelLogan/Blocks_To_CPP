//! Reading project files from disk with a size bound.

use std::fs::File;
use std::io::Read as _;
use std::path::Path;

use b2c_model::limits::MAX_FILE_BYTES;

use crate::report::terminal_safe;

/// A path as shown to the user (control characters escaped).
pub(crate) fn shown(path: &Path) -> String {
    terminal_safe(&path.display().to_string())
}

/// Reads a project file.
///
/// Reads at most one byte more than the project size limit, so a huge or
/// endless file (such as a device) cannot exhaust memory; the loader then
/// reports the oversized input as a normal diagnostic.
///
/// # Errors
/// Returns a message for the user when the file cannot be opened or read, or
/// is not a regular file.
pub(crate) fn read(path: &Path) -> Result<Vec<u8>, String> {
    let describe = |error: std::io::Error| format!("cannot read {}: {error}", shown(path));
    let file = File::open(path).map_err(describe)?;
    let metadata = file.metadata().map_err(describe)?;
    if !metadata.is_file() {
        return Err(format!("cannot read {}: not a regular file", shown(path)));
    }
    let limit = u64::try_from(MAX_FILE_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes).map_err(describe)?;
    Ok(bytes)
}
