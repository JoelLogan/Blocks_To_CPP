//! Reading and saving `.b2c` project files on disk (05 §5.6, §5.10;
//! 08 §8.6). Parsing and validation are `b2c_model`'s job; this module only
//! moves bytes safely.

use std::path::{Path, PathBuf};

use b2c_model::limits::MAX_FILE_BYTES;
use sha2::{Digest as _, Sha256};

use crate::atomic::{Backup, write_atomic};
use crate::error::{ReadError, StoreError};
use crate::ids::hex_lower;
use crate::read::read_bounded;

/// The largest project file, in bytes (05 §5.6): 32 MiB.
pub const MAX_PROJECT_BYTES: u64 = MAX_FILE_BYTES as u64;

/// Reads a project file of at most [`MAX_PROJECT_BYTES`], reading at most
/// one byte more (see [`read_bounded`]).
///
/// # Errors
/// As [`read_bounded`]; [`ReadError::TooLarge`] for a larger file.
pub fn read_project(path: &Path) -> Result<Vec<u8>, ReadError> {
    read_bounded(path, MAX_PROJECT_BYTES)
}

/// Saves a project's canonical bytes (`b2c_model::to_canonical_json`)
/// atomically, keeping the previous version as `<name>.b2c.bak`
/// (05 §5.10; see [`write_atomic`]).
///
/// # Errors
/// [`StoreError::Invalid`] when `canonical` is larger than
/// [`MAX_PROJECT_BYTES`] (it could never be opened again); otherwise as
/// [`write_atomic`].
pub fn save_project(path: &Path, canonical: &[u8]) -> Result<(), StoreError> {
    if canonical.len() > MAX_FILE_BYTES {
        return Err(StoreError::Invalid(
            "the project is larger than 32 MiB, the most a project file can be",
        ));
    }
    write_atomic(path, canonical, Backup::KeepPrevious)
}

/// The SHA-256 of `bytes`: the baseline for external-change detection.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// The SHA-256 of `bytes` as 64 lower-case hexadecimal digits (the `hash`
/// of a save response).
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex_lower(&sha256(bytes))
}

/// The canonical form of an existing path: absolute, with every link,
/// `.` and `..` resolved. Projects are bound, trusted and listed under this
/// form only, so two spellings of one file are one project.
///
/// On Windows the `\\?\` prefix that canonicalisation adds is removed when
/// the plain form means exactly the same file (a drive or UNC path shorter
/// than `MAX_PATH`, with no device names and no trailing dots or spaces), so
/// paths shown to the user look normal.
///
/// # Errors
/// [`StoreError::Io`] when the path does not exist or cannot be resolved.
pub fn canonical_path(path: &Path) -> Result<PathBuf, StoreError> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|source| StoreError::io("find the full path of the file", path, source))?;
    #[cfg(windows)]
    if let Some(plain) = canonical.to_str().and_then(simplify_verbatim) {
        return Ok(PathBuf::from(plain));
    }
    Ok(canonical)
}

/// The plain form of a Windows verbatim path (`\\?\C:\x` → `C:\x`,
/// `\\?\UNC\server\share\x` → `\\server\share\x`), or `None` when the
/// verbatim form must be kept because the plain one could mean something
/// else or fail.
#[cfg_attr(not(windows), allow(dead_code))]
fn simplify_verbatim(path: &str) -> Option<String> {
    /// `MAX_PATH` less the terminating NUL.
    const MAX_PLAIN_LEN: usize = 259;
    let (plain, rest) = if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        (format!(r"\\{rest}"), rest)
    } else {
        let rest = path.strip_prefix(r"\\?\")?;
        let bytes = rest.as_bytes();
        let drive =
            bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
        if !drive {
            return None;
        }
        (rest.to_owned(), &rest[3..])
    };
    if plain.encode_utf16().count() > MAX_PLAIN_LEN {
        return None;
    }
    let components_ok = rest
        .split('\\')
        .filter(|component| !component.is_empty())
        .all(plain_component_ok);
    components_ok.then_some(plain)
}

/// Whether a path component means the same thing with and without the
/// verbatim prefix.
#[cfg_attr(not(windows), allow(dead_code))]
fn plain_component_ok(component: &str) -> bool {
    if component == "." || component == ".." || component.ends_with('.') || component.ends_with(' ') {
        return false;
    }
    if component
        .chars()
        .any(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '|' | '?' | '*'))
    {
        return false;
    }
    // Device names stay devices with any extension: `nul.txt`, `COM1.b2c`.
    let stem = component
        .split('.')
        .next()
        .unwrap_or(component)
        .trim_end_matches(' ');
    let upper = stem.to_ascii_uppercase();
    let device = matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        upper.strip_prefix(prefix).is_some_and(|number| {
            matches!(
                number,
                "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "\u{b9}" | "\u{b2}" | "\u{b3}"
            )
        })
    });
    !device
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn hashes_are_lower_case_hex() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(sha256(b"abc")[0], 0xba);
        assert_eq!(sha256_hex(b"abc").len(), 64);
    }

    #[test]
    fn saves_keep_a_backup_and_reads_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.b2c");
        save_project(&path, b"{\"v\":1}\n").unwrap();
        save_project(&path, b"{\"v\":2}\n").unwrap();
        assert_eq!(read_project(&path).unwrap(), b"{\"v\":2}\n");
        assert_eq!(fs::read(dir.path().join("game.b2c.bak")).unwrap(), b"{\"v\":1}\n");
    }

    #[test]
    fn oversized_projects_are_refused_both_ways() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.b2c");
        let huge = vec![b' '; MAX_FILE_BYTES + 1];
        assert!(matches!(save_project(&path, &huge), Err(StoreError::Invalid(_))));
        assert!(!path.exists());
        // Sparse files: the size is what matters.
        fs::File::create(&path)
            .unwrap()
            .set_len(MAX_PROJECT_BYTES + 1)
            .unwrap();
        assert!(matches!(
            read_project(&path),
            Err(ReadError::TooLarge { limit }) if limit == MAX_PROJECT_BYTES
        ));
        fs::File::create(&path)
            .unwrap()
            .set_len(MAX_PROJECT_BYTES)
            .unwrap();
        assert_eq!(read_project(&path).unwrap().len(), MAX_FILE_BYTES);
    }

    #[test]
    fn canonical_paths_resolve_dots_and_links() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.b2c");
        fs::write(&path, b"x").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        let dotted = dir.path().join("sub").join("..").join("game.b2c");
        let canonical = canonical_path(&path).unwrap();
        assert!(canonical.is_absolute());
        assert_eq!(canonical_path(&dotted).unwrap(), canonical);
        #[cfg(unix)]
        {
            let link = dir.path().join("link.b2c");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert_eq!(canonical_path(&link).unwrap(), canonical);
        }
        assert!(matches!(
            canonical_path(&dir.path().join("missing.b2c")),
            Err(StoreError::Io { .. })
        ));
    }

    #[test]
    fn verbatim_windows_paths_are_simplified_only_when_safe() {
        assert_eq!(
            simplify_verbatim(r"\\?\C:\Users\Ada\game.b2c").as_deref(),
            Some(r"C:\Users\Ada\game.b2c")
        );
        assert_eq!(simplify_verbatim(r"\\?\d:\").as_deref(), Some(r"d:\"));
        assert_eq!(
            simplify_verbatim(r"\\?\UNC\server\share\game.b2c").as_deref(),
            Some(r"\\server\share\game.b2c")
        );
        for kept in [
            r"C:\already\plain",
            r"\\?\Volume{0b1c}\x",
            r"\\?\C:\dir.\game.b2c",
            r"\\?\C:\dir \game.b2c",
            r"\\?\C:\con\game.b2c",
            r"\\?\C:\x\NUL.txt",
            r"\\?\C:\x\com1.b2c",
            r"\\?\C:\x\a:b",
            r"\\?\C:\x\a*b",
        ] {
            assert_eq!(simplify_verbatim(kept), None, "{kept}");
        }
        let superscript = format!(r"\\?\C:\x\LPT{}", '\u{b9}');
        assert_eq!(simplify_verbatim(&superscript), None);
        let long = format!(r"\\?\C:\{}", "a".repeat(300));
        assert_eq!(simplify_verbatim(&long), None);
        // Names that only look like devices are fine.
        assert!(simplify_verbatim(r"\\?\C:\x\console\com10\lpt.txt\nul1").is_some());
    }
}
