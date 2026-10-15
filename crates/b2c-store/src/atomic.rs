//! Atomic, durable file writes (05 §5.10, 08 §8.6, 01 N10).
//!
//! Every file the app writes for itself or for the user (projects,
//! settings, the recent list, trust, toolchains, recovery snapshots, build
//! manifests) goes through [`write_atomic`], or for project files
//! [`write_atomic_keeping_mode`]:
//!
//! 1. a temporary file is created **in the same folder** with a random name,
//!    exclusively (`O_EXCL`, so an existing file or link is never opened)
//!    and owner-only (`0600` on Unix), by the `tempfile` crate; only a new
//!    project file is created instead with the bits any new file of the
//!    user gets (`0666` less the umask, 05 §5.10);
//! 2. the bytes are written and flushed, the file gets its final permission
//!    bits on Unix (`0600`, or for a project the bits of the file it
//!    replaces; a new project keeps the bits it was created with), and it is
//!    `fsync`ed;
//! 3. the temporary file is renamed over the target by
//!    [`b2c_process::os::atomic_replace`]: `MoveFileExW(REPLACE_EXISTING |
//!    WRITE_THROUGH)` on Windows, `rename(2)` and a directory `fsync` on Unix.
//!
//! A crash at any point leaves the old file or the new one, never a mix, and
//! the temporary file is removed on every error. With
//! [`Backup::KeepPrevious`] the previous version is first copied to
//! `<name>.bak` the same way (its own temporary file, then a rename), so a
//! link planted at `<name>.bak` is replaced, never followed. The previous
//! version is opened without blocking and checked through its handle, so a
//! FIFO swapped in at the target is refused instead of hanging the save.

use std::fs::{self, File, Metadata};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use b2c_process::os::open_read_nonblocking;

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

/// The permission bits a written file (and its `.bak`) gets on Unix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModePolicy {
    /// Always `0600` ([`write_atomic`]).
    OwnerOnly,
    /// The bits of the file it replaces; for a new file, the bits any new
    /// file of the user gets ([`write_atomic_keeping_mode`]).
    KeepExisting,
}

/// How the temporary file of one write gets its permission bits on Unix.
/// Elsewhere it keeps the defaults: on Windows, the ACL inherited from its
/// folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileMode {
    /// Exactly these bits (`0o600`, or a replaced file's), set on the
    /// complete file before the rename.
    Exactly(u32),
    /// The bits any new file of the user gets: the file is created with
    /// `0666` and the system takes the umask off (`0644` under the usual
    /// umask `022`), and nothing changes them afterwards.
    UserDefault,
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
/// files here.)
///
/// On Unix the file (and its `.bak`) is always owner-only, `0600`, even when
/// the file it replaces had wider permissions: every machine-local file is
/// `0600` (02 §2.7, 08 §8.6). Project files, whose permissions are the
/// user's choice, are written with [`write_atomic_keeping_mode`] instead.
///
/// # Errors
/// [`StoreError::Invalid`] when `target` has no file name or parent folder;
/// [`StoreError::Link`] when `target` is not a regular file (also when it is
/// swapped for something else while its `.bak` copy is made); and
/// [`StoreError::Io`] when any step fails. On error `target` is unchanged
/// (except, on Unix, when only the final directory sync failed) and no
/// temporary file is left behind; with [`Backup::KeepPrevious`] the backup
/// may already have been updated.
pub fn write_atomic(target: &Path, bytes: &[u8], backup: Backup) -> Result<(), StoreError> {
    write_atomic_with_hook(target, bytes, backup, ModePolicy::OwnerOnly, &mut |_| Ok(()))
}

/// [`write_atomic`] for the user's own files (project files and their
/// `.bak`): on Unix a replaced file keeps its permission bits, so a project
/// the user made group-readable stays so, and a new file gets the bits any
/// new file of the user gets, `0666` less the umask (05 §5.10, 08 §8.6;
/// usually `0644`). Never use it for machine-local files, which must always
/// be `0600`.
///
/// # Errors
/// As [`write_atomic`].
pub fn write_atomic_keeping_mode(target: &Path, bytes: &[u8], backup: Backup) -> Result<(), StoreError> {
    write_atomic_with_hook(target, bytes, backup, ModePolicy::KeepExisting, &mut |_| Ok(()))
}

/// [`write_atomic`] (or [`write_atomic_keeping_mode`], by `policy`) with a
/// hook called after the temporary file is complete and synced, right before
/// the rename; it gets the temporary file's path. An error from the hook
/// aborts the write as if the rename had failed, so tests can inject a fault
/// (or a crash) at the worst moment.
pub(crate) fn write_atomic_with_hook(
    target: &Path,
    bytes: &[u8],
    backup: Backup,
    policy: ModePolicy,
    before_rename: &mut dyn FnMut(&Path) -> io::Result<()>,
) -> Result<(), StoreError> {
    let dir = parent_dir(target)?;
    let existing = inspect_target(target)?;
    if let (Backup::KeepPrevious, Some(metadata)) = (backup, &existing) {
        copy_to_backup(target, metadata, &backup_path(target)?, dir, policy)?;
    }
    let mode = file_mode(policy, existing.as_ref());
    let temp = write_temp(dir, target, mode, &mut |file| file.write_all(bytes))?;
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

/// The mode of a file written under `policy` that replaces the file
/// described by `existing` (`None` for a new file): `0600`, or under
/// [`ModePolicy::KeepExisting`] the replaced file's permission bits, and for
/// a new file the user's default ([`FileMode::UserDefault`]).
fn file_mode(policy: ModePolicy, existing: Option<&Metadata>) -> FileMode {
    match (policy, existing) {
        (ModePolicy::OwnerOnly, _) => FileMode::Exactly(0o600),
        (ModePolicy::KeepExisting, Some(metadata)) => FileMode::Exactly(permission_bits(metadata)),
        (ModePolicy::KeepExisting, None) => FileMode::UserDefault,
    }
}

/// The permission bits of the file `metadata` describes. Only Unix has
/// them; elsewhere this is `0o600`, which nothing applies there.
fn permission_bits(metadata: &Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.permissions().mode() & 0o777
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0o600
    }
}

/// Copies the current `target` (described by `metadata`) to `backup`
/// through a temporary file and a rename.
fn copy_to_backup(
    target: &Path,
    metadata: &Metadata,
    backup: &Path,
    dir: &Path,
    policy: ModePolicy,
) -> Result<(), StoreError> {
    // A folder (or similar) at the backup path is left alone; a link there
    // is simply replaced by the rename.
    if let Ok(existing) = fs::symlink_metadata(backup)
        && !is_link(&existing)
        && !existing.is_file()
    {
        return Err(StoreError::link(backup));
    }
    // Opened without blocking: a FIFO swapped in since `metadata` was taken
    // opens at once (and is refused below) instead of waiting for a writer.
    let mut source =
        open_read_nonblocking(target).map_err(|source| StoreError::io("read the file", target, source))?;
    let opened = source
        .metadata()
        .map_err(|source| StoreError::io("read the file", target, source))?;
    // The file opened must be the one inspected, not something swapped in.
    if !opened.is_file() || !same_file(metadata, &opened) {
        return Err(StoreError::link(target));
    }
    let temp = write_temp(dir, backup, file_mode(policy, Some(metadata)), &mut |file| {
        io::copy(&mut source, file).map(|_| ())
    })?;
    replace(temp, backup)
}

/// Creates a temporary file in `dir` (on Unix with the bits `mode` asks
/// for), fills it with `write_contents`, flushes it, sets its bits, syncs it
/// and closes it. The file is removed again when the returned path is
/// dropped.
fn write_temp(
    dir: &Path,
    target: &Path,
    mode: FileMode,
    write_contents: &mut dyn FnMut(&mut File) -> io::Result<()>,
) -> Result<tempfile::TempPath, StoreError> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(".b2c-").suffix(".tmp");
    #[cfg(unix)]
    if mode == FileMode::UserDefault {
        use std::os::unix::fs::PermissionsExt as _;
        // Created (exclusively) with `0666`, which the system narrows by the
        // umask, as for any new file of the user. Otherwise `tempfile`
        // creates it `0600`.
        builder.permissions(fs::Permissions::from_mode(0o666));
    }
    let temp = builder
        .tempfile_in(dir)
        .map_err(|source| StoreError::io("create a temporary file", target, without_path(&source)))?;
    let (mut file, path) = temp.into_parts();
    let mut write = |file: &mut File| -> io::Result<()> {
        write_contents(file)?;
        file.flush()?;
        #[cfg(unix)]
        if let FileMode::Exactly(bits) = mode {
            use std::os::unix::fs::PermissionsExt as _;
            file.set_permissions(fs::Permissions::from_mode(bits))?;
        }
        #[cfg(not(unix))]
        let _ = mode;
        file.sync_all()
    };
    let written = write(&mut file);
    // Closed before the rename (Windows) and before any clean-up.
    drop(file);
    // On error `path` is dropped on return, which removes the temporary file.
    written.map_err(|source| StoreError::io("write the file", target, source))?;
    Ok(path)
}

/// An error like `error` but without a path in its text. `tempfile` wraps
/// the errors of creating a file in one whose text ends in `at path
/// "/the/folder/.b2c-….tmp"`, and no store error may show a path (08 §8.11;
/// the path stays in [`StoreError::Io`]'s `path` field, for the debug log).
/// The result keeps the OS error code when `error` is a plain OS error, and
/// otherwise (as for `tempfile`'s wrapped errors) only the error kind.
fn without_path(error: &io::Error) -> io::Error {
    error
        .raw_os_error()
        .map_or_else(|| io::Error::from(error.kind()), io::Error::from_raw_os_error)
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
        let result = write_atomic_with_hook(
            &target,
            b"new",
            Backup::None,
            ModePolicy::OwnerOnly,
            &mut |temp| {
                // The temporary file is complete at this point.
                assert_eq!(fs::read(temp).unwrap(), b"new");
                assert_eq!(temp.parent(), Some(dir.path()));
                seen = Some(temp.to_path_buf());
                Err(io::Error::other("simulated crash"))
            },
        );
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
        let result = write_atomic_with_hook(
            &target,
            b"new",
            Backup::None,
            ModePolicy::OwnerOnly,
            &mut |temp| fs::remove_file(temp),
        );
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

    /// `tempfile` adds the temporary file's full path to its errors; store
    /// errors must not show it (08 §8.11), anywhere in the error chain.
    #[test]
    fn errors_never_show_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("secret-project").join("game.b2c");
        let error = write_atomic(&target, b"x", Backup::None).unwrap_err();
        let StoreError::Io { path, source, .. } = &error else {
            panic!("{error:?}");
        };
        assert_eq!(path, &target);
        assert_eq!(source.kind(), io::ErrorKind::NotFound);
        let mut next: Option<&dyn std::error::Error> = Some(&error);
        while let Some(current) = next {
            let text = current.to_string();
            assert!(
                !text.contains("secret-project") && !text.contains(".tmp"),
                "{text}"
            );
            next = current.source();
        }
        // Plain OS errors keep their code.
        let os = io::Error::from_raw_os_error(2);
        assert_eq!(without_path(&os).raw_os_error(), Some(2));
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
    fn machine_local_files_are_always_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("trust.json");
        write_atomic(&target, b"{}", Backup::None).unwrap();
        assert_eq!(mode(&target), 0o600);
        // A file someone made wider is replaced by an owner-only one, and so
        // is its backup.
        fs::set_permissions(&target, fs::Permissions::from_mode(0o666)).unwrap();
        write_atomic(&target, b"[]", Backup::KeepPrevious).unwrap();
        assert_eq!(mode(&target), 0o600);
        assert_eq!(mode(&dir.path().join("trust.json.bak")), 0o600);
    }

    /// The permission bits any new file of this process gets in `dir`:
    /// `0666` less the umask, read from a file created the ordinary way.
    #[cfg(unix)]
    fn new_file_mode(dir: &Path) -> u32 {
        use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
        let probe = dir.join("probe");
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o666)
            .open(&probe)
            .unwrap();
        let mode = file.metadata().unwrap().permissions().mode() & 0o777;
        drop(file);
        fs::remove_file(&probe).unwrap();
        mode
    }

    /// A new project gets what any new file of the user gets, not `0600`
    /// (05 §5.10, 08 §8.6, decision Q13 of 10 §10.3); a saved project keeps
    /// the bits it has, and so does its backup. (Under a umask of `077` the
    /// two are the same, so this test can tell them apart only under a
    /// wider umask such as the usual `022`.)
    #[cfg(unix)]
    #[test]
    fn projects_keep_their_mode_and_new_ones_get_the_users_default() {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let dir = tempfile::tempdir().unwrap();
        let user_default = new_file_mode(dir.path());
        let new = dir.path().join("new.b2c");
        write_atomic_keeping_mode(&new, b"new", Backup::KeepPrevious).unwrap();
        assert_eq!(mode(&new), user_default, "{:o} {user_default:o}", mode(&new));
        assert_eq!(listing(dir.path()), ["new.b2c"]);
        write_atomic_keeping_mode(&new, b"newer", Backup::KeepPrevious).unwrap();
        assert_eq!(mode(&new), user_default);
        assert_eq!(mode(&dir.path().join("new.b2c.bak")), user_default);

        let project = dir.path().join("shared.b2c");
        fs::write(&project, b"old").unwrap();
        fs::set_permissions(&project, fs::Permissions::from_mode(0o640)).unwrap();
        write_atomic_keeping_mode(&project, b"new", Backup::KeepPrevious).unwrap();
        assert_eq!(mode(&project), 0o640);
        assert_eq!(mode(&dir.path().join("shared.b2c.bak")), 0o640);
        // An owner-only project stays owner-only.
        fs::set_permissions(&project, fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic_keeping_mode(&project, b"newer", Backup::KeepPrevious).unwrap();
        assert_eq!(mode(&project), 0o600);
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
