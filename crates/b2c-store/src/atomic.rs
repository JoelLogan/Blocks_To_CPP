//! Atomic, durable file writes (05 §5.10, 08 §8.6, 01 N10).
//!
//! Every file the app writes for itself or for the user (projects,
//! settings, the recent list, trust, toolchains, recovery snapshots, build
//! manifests) goes through [`write_atomic`]:
//!
//! 1. a temporary file is created **in the same folder** with a random name,
//!    exclusively (`O_EXCL`, so an existing file or link is never opened)
//!    and owner-only (`0600` on Unix), by the `tempfile` crate;
//! 2. the bytes are written, flushed and `fsync`ed;
//! 3. the temporary file is renamed over the target by
//!    [`b2c_process::os::atomic_replace`]: `MoveFileExW(REPLACE_EXISTING |
//!    WRITE_THROUGH)` on Windows, `rename(2)` and a directory `fsync` on Unix.
//!
//! A crash at any point leaves the old file or the new one, never a mix, and
//! the temporary file is removed on every error. With
//! [`Backup::KeepPrevious`] the previous version is first copied to
//! `<name>.bak` the same way (its own temporary file, then a rename), so a
//! link planted at `<name>.bak` is replaced, never followed.

use std::fs::{self, File, Metadata, Permissions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use crate::error::StoreError;
use crate::fs_checks::{is_link, same_file};

/// Whether [`write_atomic`] keeps the previous version of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backup {
    /// Just replace the file.
    None,
    /// Copy the current file (if there is one) to `<name>.bak` first, one
    /// generation (see [`backup_path`]).
    KeepPrevious,
}

/// The backup of `target` that [`Backup::KeepPrevious`] writes: the same
/// folder and file name with `.bak` appended (`game.b2c` → `game.b2c.bak`).
///
/// # Errors
/// [`StoreError::Invalid`] when `target` has no file name.
pub fn backup_path(target: &Path) -> Result<PathBuf, StoreError> {
    let name = target
        .file_name()
        .ok_or(StoreError::Invalid("the file to write has no file name"))?;
    let mut backup = name.to_os_string();
    backup.push(".bak");
    Ok(target.with_file_name(backup))
}

/// Replaces `target` with `bytes` atomically and durably (see the module
/// documentation), optionally keeping the previous version.
///
/// The folder must exist. The target must be a regular file or not exist: a
/// symbolic link or junction (on Windows, any name-surrogate reparse point),
/// or a folder, at `target` is refused and left alone. (Other reparse
/// points, such as the placeholders of a cloud-synced folder, are ordinary
/// files here.) A replaced file keeps its permission bits on Unix (a project
/// the user made group-readable stays so); a new file is `0600`.
///
/// # Errors
/// [`StoreError::Invalid`] when `target` has no file name or parent folder;
/// [`StoreError::Link`] when `target` is not a regular file; and
/// [`StoreError::Io`] when any step fails. On error `target` is unchanged
/// (except, on Unix, when only the final directory sync failed) and no
/// temporary file is left behind; with [`Backup::KeepPrevious`] the backup
/// may already have been updated.
pub fn write_atomic(target: &Path, bytes: &[u8], backup: Backup) -> Result<(), StoreError> {
    write_atomic_with_hook(target, bytes, backup, &mut |_| Ok(()))
}

/// [`write_atomic`] with a hook called after the temporary file is complete
/// and synced, right before the rename; it gets the temporary file's path.
/// An error from the hook aborts the write as if the rename had failed, so
/// tests can inject a fault (or a crash) at the worst moment.
pub(crate) fn write_atomic_with_hook(
    target: &Path,
    bytes: &[u8],
    backup: Backup,
    before_rename: &mut dyn FnMut(&Path) -> io::Result<()>,
) -> Result<(), StoreError> {
    let dir = parent_dir(target)?;
    let existing = inspect_target(target)?;
    if let (Backup::KeepPrevious, Some(metadata)) = (backup, &existing) {
        copy_to_backup(target, metadata, &backup_path(target)?, dir)?;
    }
    let permissions = existing.as_ref().and_then(kept_permissions);
    let temp = write_temp(dir, target, permissions, &mut |file| file.write_all(bytes))?;
    before_rename(&temp).map_err(|source| StoreError::io("write the file", target, source))?;
    replace(temp, target)
}

/// The folder of `target`.
fn parent_dir(target: &Path) -> Result<&Path, StoreError> {
    if target.file_name().is_none() {
        return Err(StoreError::Invalid("the file to write has no file name"));
    }
    match target.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => Ok(parent),
        _ => Err(StoreError::Invalid("the file to write has no folder")),
    }
}

/// The current target, if any: it must be a regular file (not followed if
/// it is a link).
fn inspect_target(target: &Path) -> Result<Option<Metadata>, StoreError> {
    match fs::symlink_metadata(target) {
        Ok(metadata) if is_link(&metadata) || !metadata.is_file() => Err(StoreError::link(target)),
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(StoreError::io("inspect the file", target, source)),
    }
}

/// The permissions a file replacing one with `metadata` gets: the same
/// permission bits on Unix; elsewhere the temporary file's defaults.
#[allow(clippy::unnecessary_wraps)] // `None` on Windows
fn kept_permissions(metadata: &Metadata) -> Option<Permissions> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        Some(Permissions::from_mode(metadata.permissions().mode() & 0o777))
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

/// Copies the current `target` (described by `metadata`) to `backup`
/// through a temporary file and a rename.
fn copy_to_backup(target: &Path, metadata: &Metadata, backup: &Path, dir: &Path) -> Result<(), StoreError> {
    // A folder (or similar) at the backup path is left alone; a link there
    // is simply replaced by the rename.
    if let Ok(existing) = fs::symlink_metadata(backup)
        && !is_link(&existing)
        && !existing.is_file()
    {
        return Err(StoreError::link(backup));
    }
    let mut source = File::open(target).map_err(|source| StoreError::io("read the file", target, source))?;
    let opened = source
        .metadata()
        .map_err(|source| StoreError::io("read the file", target, source))?;
    // The file opened must be the one inspected, not something swapped in.
    if !opened.is_file() || !same_file(metadata, &opened) {
        return Err(StoreError::link(target));
    }
    let temp = write_temp(dir, backup, kept_permissions(metadata), &mut |file| {
        io::copy(&mut source, file).map(|_| ())
    })?;
    replace(temp, backup)
}

/// Creates a temporary file in `dir`, fills it with `write_contents`, sets
/// `permissions`, flushes and syncs it, and closes it. The file is removed
/// again when the returned path is dropped.
fn write_temp(
    dir: &Path,
    target: &Path,
    permissions: Option<Permissions>,
    write_contents: &mut dyn FnMut(&mut File) -> io::Result<()>,
) -> Result<tempfile::TempPath, StoreError> {
    let temp = tempfile::Builder::new()
        .prefix(".b2c-")
        .suffix(".tmp")
        .tempfile_in(dir)
        .map_err(|source| StoreError::io("create a temporary file", target, source))?;
    let (mut file, path) = temp.into_parts();
    let write = |file: &mut File| -> io::Result<()> {
        write_contents(file)?;
        file.flush()?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.sync_all()
    };
    let written = write(&mut file);
    // Closed before the rename (Windows) and before any clean-up.
    drop(file);
    // On error `path` is dropped on return, which removes the temporary file.
    written.map_err(|source| StoreError::io("write the file", target, source))?;
    Ok(path)
}

/// Renames the complete temporary file over `target`.
fn replace(temp: tempfile::TempPath, target: &Path) -> Result<(), StoreError> {
    match b2c_process::os::atomic_replace(&temp, target) {
        Ok(()) => {
            // The temporary file now is the target: only stop the clean-up.
            let _ = temp.keep();
            Ok(())
        }
        // Dropping `temp` removes the temporary file (if it is still there).
        Err(source) => Err(StoreError::io("replace the file", target, source)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names in a folder, sorted.
    fn listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn writes_new_and_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("settings.json");
        write_atomic(&target, b"one", Backup::None).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"one");
        write_atomic(&target, b"two", Backup::None).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"two");
        assert_eq!(listing(dir.path()), ["settings.json"]);
    }

    #[test]
    fn keeps_one_previous_version() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("game.b2c");
        // No previous version: no backup.
        write_atomic(&target, b"v1", Backup::KeepPrevious).unwrap();
        assert_eq!(listing(dir.path()), ["game.b2c"]);
        write_atomic(&target, b"v2", Backup::KeepPrevious).unwrap();
        write_atomic(&target, b"v3", Backup::KeepPrevious).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"v3");
        assert_eq!(fs::read(dir.path().join("game.b2c.bak")).unwrap(), b"v2");
        assert_eq!(listing(dir.path()), ["game.b2c", "game.b2c.bak"]);
        assert_eq!(backup_path(&target).unwrap(), dir.path().join("game.b2c.bak"));
    }

    #[test]
    fn a_fault_before_the_rename_keeps_the_old_file_and_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("game.b2c");
        fs::write(&target, b"old").unwrap();
        let mut seen = None;
        let result = write_atomic_with_hook(&target, b"new", Backup::None, &mut |temp| {
            // The temporary file is complete at this point.
            assert_eq!(fs::read(temp).unwrap(), b"new");
            assert_eq!(temp.parent(), Some(dir.path()));
            seen = Some(temp.to_path_buf());
            Err(io::Error::other("simulated crash"))
        });
        assert!(matches!(result, Err(StoreError::Io { .. })));
        assert_eq!(fs::read(&target).unwrap(), b"old");
        assert!(!seen.unwrap().exists());
        assert_eq!(listing(dir.path()), ["game.b2c"]);
    }

    #[test]
    fn a_failed_rename_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("game.b2c");
        fs::write(&target, b"old").unwrap();
        // Removing the folder's entry from under the temporary file makes
        // the rename itself fail.
        let result = write_atomic_with_hook(&target, b"new", Backup::None, &mut |temp| fs::remove_file(temp));
        assert!(matches!(result, Err(StoreError::Io { .. })));
        assert_eq!(fs::read(&target).unwrap(), b"old");
        assert_eq!(listing(dir.path()), ["game.b2c"]);
    }

    #[test]
    fn folders_and_bad_targets_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("folder");
        fs::create_dir(&folder).unwrap();
        assert!(matches!(
            write_atomic(&folder, b"x", Backup::None),
            Err(StoreError::Link { .. })
        ));
        assert!(matches!(
            write_atomic(Path::new("relative-without-folder"), b"x", Backup::None),
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            write_atomic(Path::new("/"), b"x", Backup::None),
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            write_atomic(&dir.path().join("missing").join("file"), b"x", Backup::None),
            Err(StoreError::Io { .. })
        ));
        // A folder at the backup path is left alone and the save refused.
        let target = dir.path().join("game.b2c");
        fs::write(&target, b"old").unwrap();
        fs::create_dir(dir.path().join("game.b2c.bak")).unwrap();
        assert!(matches!(
            write_atomic(&target, b"new", Backup::KeepPrevious),
            Err(StoreError::Link { .. })
        ));
        assert_eq!(fs::read(&target).unwrap(), b"old");
    }

    #[cfg(unix)]
    #[test]
    fn links_at_the_target_are_refused_and_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim");
        fs::write(&victim, b"keep me").unwrap();
        let target = dir.path().join("settings.json");
        std::os::unix::fs::symlink(&victim, &target).unwrap();
        assert!(matches!(
            write_atomic(&target, b"x", Backup::None),
            Err(StoreError::Link { .. })
        ));
        assert_eq!(fs::read(&victim).unwrap(), b"keep me");
        assert!(fs::symlink_metadata(&target).unwrap().file_type().is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_planted_at_the_backup_is_replaced_and_its_target_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim");
        fs::write(&victim, b"keep me").unwrap();
        let target = dir.path().join("game.b2c");
        fs::write(&target, b"old").unwrap();
        let backup = dir.path().join("game.b2c.bak");
        std::os::unix::fs::symlink(&victim, &backup).unwrap();
        write_atomic(&target, b"new", Backup::KeepPrevious).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert!(!fs::symlink_metadata(&backup).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(&backup).unwrap(), b"old");
        assert_eq!(fs::read(&victim).unwrap(), b"keep me");
    }

    #[cfg(unix)]
    #[test]
    fn new_files_are_owner_only_and_replaced_files_keep_their_mode() {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("trust.json");
        write_atomic(&target, b"{}", Backup::None).unwrap();
        assert_eq!(mode(&target), 0o600);
        let project = dir.path().join("shared.b2c");
        fs::write(&project, b"old").unwrap();
        fs::set_permissions(&project, Permissions::from_mode(0o640)).unwrap();
        write_atomic(&project, b"new", Backup::KeepPrevious).unwrap();
        assert_eq!(mode(&project), 0o640);
        assert_eq!(mode(&dir.path().join("shared.b2c.bak")), 0o640);
    }

    #[cfg(unix)]
    #[test]
    fn the_folder_is_synced() {
        let dir = tempfile::tempdir().unwrap();
        let before = b2c_process::os::directory_syncs();
        write_atomic(&dir.path().join("file"), b"x", Backup::None).unwrap();
        assert!(b2c_process::os::directory_syncs() > before);
    }
}
