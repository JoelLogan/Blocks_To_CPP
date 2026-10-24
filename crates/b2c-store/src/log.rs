//! The local log files: a writer that rotates by size
//! (`docs/spec/08-security.md` §8.11, `docs/spec/02-architecture.md` §2.7,
//! `docs/spec/09-quality-and-delivery.md` §9.1).
//!
//! CONTRACT (milestone M2):
//! * [`RotatingFile`] appends to `<base>.log` in its folder. Before a write
//!   that would make that file larger than `max_bytes`, it rotates:
//!   `<base>.<files-1>.log` is deleted, every `<base>.<n>.log` becomes
//!   `<base>.<n+1>.log`, `<base>.log` becomes `<base>.1.log`, and a new
//!   `<base>.log` is started. So there are at most `files` files, each at
//!   most `max_bytes`, and the oldest lines are dropped first. A single
//!   write larger than `max_bytes` is accepted only in part (as
//!   [`io::Write::write`] allows), so `write_all` spreads it over files that
//!   each stay within the limit. Files left over from a larger `files`
//!   setting are deleted when the writer is opened.
//! * The desktop app opens it with [`Dirs::logs`](crate::Dirs::logs),
//!   [`LOG_BASE`], [`LOG_MAX_BYTES`] and [`LOG_FILES`]: `blocks2cpp.log` and
//!   `blocks2cpp.1.log` to `blocks2cpp.4.log`, 5 MiB each (08 §8.11).
//! * The folder is created private ([`ensure_private_dir`], `0700` on Unix)
//!   and every log file is `0600` on Unix; a link or anything other than a
//!   regular file at a log file's path is refused, never written through.
//! * The size of the current file is read from its handle before every
//!   write, so several app instances writing the same log still keep every
//!   file within the limit (when one rotates, the others keep appending to
//!   the renamed file until it is full, then rotate in turn).
//! * The writer knows nothing about `tracing`: the app wraps it in a
//!   `Mutex` as its `MakeWriter` and decides what is written. Logs never
//!   contain project content and show paths only at debug level (08 §8.11);
//!   that is the caller's rule, since this writer cannot see what it writes.
//!   No error from this module contains a path.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::dirs::ensure_private_dir;
use crate::error::StoreError;
use crate::fs_checks::is_link;

/// The base name of the app's log files: `blocks2cpp.log`, `blocks2cpp.1.log`, ...
pub const LOG_BASE: &str = "blocks2cpp";
/// The size at which the app's log rotates (5 MiB).
pub const LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;
/// How many log files the app keeps (the current one and four older ones).
pub const LOG_FILES: usize = 5;
/// The most files a [`RotatingFile`] may keep.
pub const MAX_LOG_FILES: usize = 100;
/// The longest base name, in bytes.
const MAX_BASE_LEN: usize = 64;

/// A log file that rotates by size (see the module documentation). It
/// implements [`io::Write`]; share it between threads behind a `Mutex`.
#[derive(Debug)]
pub struct RotatingFile {
    dir: PathBuf,
    base: String,
    max_bytes: u64,
    files: usize,
    /// The open `<base>.log`; `None` after a failed rotation, until the next
    /// write opens it again.
    file: Option<File>,
}

impl RotatingFile {
    /// Opens the log `<dir>/<base>.log` for appending (creating the folder
    /// privately and the file `0600` when needed), keeping at most `files`
    /// files of at most `max_bytes` each, and deletes `<base>.<n>.log` files
    /// with `n >= files` left over from an earlier setting.
    ///
    /// # Errors
    /// [`io::ErrorKind::InvalidInput`] when `base` is empty, longer than 64
    /// bytes or has characters other than ASCII letters, digits, `-` and
    /// `_`, when `max_bytes` is 0, when `files` is 0 or more than
    /// [`MAX_LOG_FILES`], or when `<base>.log` is a link or not a regular
    /// file; otherwise the error of creating the folder or opening the file.
    pub fn open(dir: &Path, base: &str, max_bytes: u64, files: usize) -> io::Result<Self> {
        let valid_base = !base.is_empty()
            && base.len() <= MAX_BASE_LEN
            && base
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if !valid_base {
            return Err(invalid_input(
                "a log's base name must be 1 to 64 ASCII letters, digits, '-' or '_'",
            ));
        }
        if max_bytes == 0 {
            return Err(invalid_input("a log file must be allowed at least one byte"));
        }
        if files == 0 || files > MAX_LOG_FILES {
            return Err(invalid_input("a log must keep between 1 and 100 files"));
        }
        ensure_private_dir(dir).map_err(into_io)?;
        let mut writer = Self {
            dir: dir.to_path_buf(),
            base: base.to_owned(),
            max_bytes,
            files,
            file: None,
        };
        writer.remove_leftovers();
        writer.file = Some(open_log(&writer.path_of(0))?);
        Ok(writer)
    }

    /// The paths of the log's files, newest first: `<base>.log`, then
    /// `<base>.1.log` to `<base>.<files-1>.log` (whether they exist yet or
    /// not).
    pub fn paths(&self) -> Vec<PathBuf> {
        (0..self.files)
            .map(|generation| self.path_of(generation))
            .collect()
    }

    /// `<base>.log` for generation 0, else `<base>.<generation>.log`.
    fn path_of(&self, generation: usize) -> PathBuf {
        let name = if generation == 0 {
            format!("{}.log", self.base)
        } else {
            format!("{}.{generation}.log", self.base)
        };
        self.dir.join(name)
    }

    /// Deletes `<base>.<n>.log` files (and links) with `n >= files`, best
    /// effort: a file that cannot be deleted is left.
    fn remove_leftovers(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(generation) = name.to_str().and_then(|name| self.generation_of(name)) else {
                continue;
            };
            let removable = entry
                .file_type()
                .is_ok_and(|kind| kind.is_file() || kind.is_symlink());
            if generation >= self.files && removable {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    /// `n` when `name` is `<base>.<n>.log` with `n >= 1` written without
    /// leading zeros.
    fn generation_of(&self, name: &str) -> Option<usize> {
        let digits = name
            .strip_prefix(self.base.as_str())?
            .strip_prefix('.')?
            .strip_suffix(".log")?;
        let canonical = !digits.is_empty()
            && !digits.starts_with('0')
            && digits.len() <= 20
            && digits.bytes().all(|b| b.is_ascii_digit());
        canonical.then(|| digits.parse().ok()).flatten()
    }

    /// The open `<base>.log`, opened again if a rotation failed before.
    fn current(&mut self) -> io::Result<&mut File> {
        let file = match self.file.take() {
            Some(file) => file,
            None => open_log(&self.path_of(0))?,
        };
        Ok(self.file.insert(file))
    }

    /// Shifts the files by one generation, dropping the oldest, and starts
    /// a new `<base>.log`.
    fn rotate(&mut self) -> io::Result<()> {
        // Closed first: Windows cannot rename an open file of ours.
        self.file = None;
        let oldest = self.files - 1;
        remove_if_present(&self.path_of(oldest))?;
        for generation in (0..oldest).rev() {
            rename_if_present(&self.path_of(generation), &self.path_of(generation + 1))?;
        }
        self.file = Some(open_log(&self.path_of(0))?);
        Ok(())
    }
}

impl Write for RotatingFile {
    /// Appends to `<base>.log`, rotating first when `buf` would not fit;
    /// writes only what fits in an empty file when `buf` is larger than the
    /// limit.
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let wanted = u64::try_from(buf.len()).unwrap_or(u64::MAX);
        let mut size = self.current()?.metadata()?.len();
        if size > 0 && size.saturating_add(wanted) > self.max_bytes {
            self.rotate()?;
            size = self.current()?.metadata()?.len();
        }
        let room = self.max_bytes.saturating_sub(size);
        if room == 0 {
            // Only when another app instance filled the new file at once.
            return Err(io::Error::other("the log file is full"));
        }
        let count = usize::try_from(wanted.min(room)).unwrap_or(buf.len());
        self.current()?.write(&buf[..count])
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.file {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

/// Opens a log file for appending: created `0600` on Unix (an existing one
/// is made `0600`), never through a link, and only a regular file.
fn open_log(path: &Path) -> io::Result<File> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link(&metadata) || !metadata.is_file() => return Err(not_a_file()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut options = OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(not_a_file());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o777 != 0o600 {
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
    }
    Ok(file)
}

/// Deletes `path`; a missing file is fine.
fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Renames `from` to `to`; a missing `from` is fine.
fn rename_if_present(from: &Path, to: &Path) -> io::Result<()> {
    match fs::rename(from, to) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn not_a_file() -> io::Error {
    invalid_input("refusing to write a log through a link or to something other than a regular file")
}

/// A store error as an I/O error, without the path it carries (08 §8.11).
fn into_io(error: StoreError) -> io::Error {
    match error {
        StoreError::Io { source, .. } => source,
        StoreError::Link { .. } | StoreError::Invalid(_) | StoreError::PathNotUnicode => {
            io::Error::new(io::ErrorKind::InvalidInput, error.to_string())
        }
        StoreError::Random { .. } => io::Error::other(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn rotates_before_a_write_that_would_not_fit() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("logs");
        let mut log = RotatingFile::open(&logs, "app", 10, 3).unwrap();
        log.write_all(b"aaaa\n").unwrap();
        log.write_all(b"bbbb\n").unwrap();
        // 10 bytes: full, but not over.
        assert_eq!(names(&logs), ["app.log"]);
        log.write_all(b"cccc\n").unwrap();
        assert_eq!(names(&logs), ["app.1.log", "app.log"]);
        assert_eq!(fs::read(logs.join("app.1.log")).unwrap(), b"aaaa\nbbbb\n");
        log.write_all(b"dddddd\n").unwrap();
        log.write_all(b"ee\n").unwrap();
        log.write_all(b"ff\n").unwrap();
        log.flush().unwrap();
        assert_eq!(names(&logs), ["app.1.log", "app.2.log", "app.log"]);
        assert_eq!(fs::read(logs.join("app.log")).unwrap(), b"ff\n");
        assert_eq!(fs::read(logs.join("app.1.log")).unwrap(), b"dddddd\nee\n");
        assert_eq!(fs::read(logs.join("app.2.log")).unwrap(), b"cccc\n");
        assert_eq!(
            log.paths(),
            [
                logs.join("app.log"),
                logs.join("app.1.log"),
                logs.join("app.2.log")
            ]
        );
    }

    #[test]
    fn oversized_writes_are_spread_over_files_within_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = RotatingFile::open(dir.path(), "app", 4, 3).unwrap();
        assert_eq!(log.write(b"0123456789").unwrap(), 4);
        log.write_all(b"0123456789").unwrap();
        assert_eq!(names(dir.path()), ["app.1.log", "app.2.log", "app.log"]);
        assert_eq!(fs::read(dir.path().join("app.log")).unwrap(), b"89");
        assert_eq!(fs::read(dir.path().join("app.1.log")).unwrap(), b"4567");
        assert_eq!(fs::read(dir.path().join("app.2.log")).unwrap(), b"0123");
        assert_eq!(log.write(b"").unwrap(), 0);
    }

    #[test]
    fn a_single_file_is_restarted() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = RotatingFile::open(dir.path(), "one", 6, 1).unwrap();
        log.write_all(b"abc\n").unwrap();
        log.write_all(b"def\n").unwrap();
        assert_eq!(names(dir.path()), ["one.log"]);
        assert_eq!(fs::read(dir.path().join("one.log")).unwrap(), b"def\n");
    }

    #[test]
    fn arguments_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        for base in ["", "a/b", "a.b", "..", "a b", &"x".repeat(65), "é"] {
            let error = RotatingFile::open(dir.path(), base, 10, 2).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{base}");
        }
        for (max_bytes, files) in [(0, 2), (10, 0), (10, MAX_LOG_FILES + 1)] {
            let error = RotatingFile::open(dir.path(), "app", max_bytes, files).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }
        assert!(RotatingFile::open(dir.path(), "a-Z_9", 1, MAX_LOG_FILES).is_ok());
        let error = RotatingFile::open(Path::new("relative"), "app", 10, 2).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(!error.to_string().contains("relative"));
    }

    #[test]
    fn generations_are_recognised_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let log = RotatingFile::open(dir.path(), "app", 10, 2).unwrap();
        assert_eq!(log.generation_of("app.1.log"), Some(1));
        assert_eq!(log.generation_of("app.42.log"), Some(42));
        for name in [
            "app.log",
            "app.0.log",
            "app.01.log",
            "app.x.log",
            "app..log",
            "apps.1.log",
            "app.1.log.bak",
            "app.-1.log",
            "app.99999999999999999999999.log",
        ] {
            assert_eq!(log.generation_of(name), None, "{name}");
        }
    }

    #[test]
    fn error_texts_have_no_paths() {
        let path = Path::new("/home/someone/secret");
        for error in [
            StoreError::link(path),
            StoreError::io("create the folder", path, io::ErrorKind::PermissionDenied.into()),
            StoreError::Invalid("rule"),
        ] {
            let text = format!("{:?}", into_io(error));
            assert!(!text.contains("secret"), "{text}");
        }
    }
}
