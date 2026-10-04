//! Canonical paths that g++ can use.

use std::io;
use std::path::{Path, PathBuf};

/// Like [`std::fs::canonicalize`] (absolute, symbolic links resolved), but on
/// Windows a verbatim drive path (`\\?\C:\msys64\…`) is turned back into the
/// ordinary form (`C:\msys64\…`) whenever that names the same file.
///
/// GCC's driver finds its own programs (`cc1plus`, `as`, `ld`) relative to
/// the path it was started as and does not understand the `\\?\` prefix, and
/// programs given a verbatim working directory can misbehave too. Verbatim
/// network paths are left alone (discovery refuses network paths anyway).
///
/// # Errors
/// As [`std::fs::canonicalize`].
pub fn canonical(path: &Path) -> io::Result<PathBuf> {
    let verbatim = std::fs::canonicalize(path)?;
    Ok(without_verbatim_prefix(&verbatim)
        .filter(|plain| std::fs::canonicalize(plain).is_ok_and(|again| again == verbatim))
        .unwrap_or(verbatim))
}

/// `\\?\C:\a\b` as `C:\a\b`; `None` for anything else.
#[cfg(windows)]
fn without_verbatim_prefix(path: &Path) -> Option<PathBuf> {
    use std::path::{Component, Prefix};
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return None;
    };
    let Prefix::VerbatimDisk(letter) = prefix.kind() else {
        return None;
    };
    let mut plain = PathBuf::from(format!("{}:\\", char::from(letter)));
    for component in components {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => plain.push(name),
            // A canonical path has no `.`, `..` or second prefix; keep the
            // verbatim form if one appears.
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => return None,
        }
    }
    Some(plain)
}

#[cfg(not(windows))]
fn without_verbatim_prefix(_path: &Path) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_paths_are_absolute_and_plain() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("g++.exe");
        std::fs::write(&file, b"").unwrap();
        let canonical = canonical(&file).unwrap();
        assert!(canonical.is_absolute());
        assert!(
            !canonical.to_string_lossy().starts_with(r"\\?\"),
            "{}",
            canonical.display()
        );
        assert!(canonical.ends_with("g++.exe"));
        assert!(super::canonical(&dir.path().join("missing")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_drive_paths_become_plain() {
        assert_eq!(
            without_verbatim_prefix(Path::new(r"\\?\C:\msys64\ucrt64\bin\g++.exe")),
            Some(PathBuf::from(r"C:\msys64\ucrt64\bin\g++.exe"))
        );
        assert_eq!(
            without_verbatim_prefix(Path::new(r"\\?\UNC\server\share\g++.exe")),
            None
        );
        assert_eq!(without_verbatim_prefix(Path::new(r"C:\plain\g++.exe")), None);
    }
}
