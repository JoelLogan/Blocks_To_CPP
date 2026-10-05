//! Size-bounded reads (08 §8.6: "files are read up to limit + 1 bytes, so
//! oversized files are rejected without being fully loaded").

use std::fs::{self, File};
use std::io::Read as _;
use std::path::Path;

use crate::error::ReadError;
use crate::fs_checks::same_file;

/// Reads a whole regular file of at most `limit` bytes.
///
/// Only regular files are read (after following links): a folder, device,
/// pipe or socket is [`ReadError::NotAFile`], checked *before* opening, so
/// opening a pipe can never block. The file is then opened, checked again
/// (it must still be the same regular file) and read through a `take` of
/// `limit + 1` bytes, so a file that grows, or a huge file whose size was
/// not reported correctly, still costs at most `limit + 1` bytes of memory.
///
/// # Errors
/// [`ReadError::NotFound`] when the file does not exist,
/// [`ReadError::NotAFile`] as above, [`ReadError::TooLarge`] when the file
/// has more than `limit` bytes, and [`ReadError::Io`] otherwise.
pub fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, ReadError> {
    let io = |error: std::io::Error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ReadError::NotFound
        } else {
            ReadError::Io(error)
        }
    };
    let before = fs::metadata(path).map_err(io)?;
    if !before.is_file() {
        return Err(ReadError::NotAFile);
    }
    if before.len() > limit {
        return Err(ReadError::TooLarge { limit });
    }
    let file = File::open(path).map_err(io)?;
    let opened = file.metadata().map_err(ReadError::Io)?;
    if !opened.is_file() || !same_file(&before, &opened) {
        return Err(ReadError::NotAFile);
    }
    let cap = limit.saturating_add(1);
    // The reported size is only a hint for the first allocation.
    let hint = usize::try_from(opened.len().min(cap)).unwrap_or(0);
    let mut bytes = Vec::with_capacity(hint);
    file.take(cap).read_to_end(&mut bytes).map_err(ReadError::Io)?;
    if !u64::try_from(bytes.len()).is_ok_and(|len| len <= limit) {
        return Err(ReadError::TooLarge { limit });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_files_up_to_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        fs::write(&path, b"12345").unwrap();
        assert_eq!(read_bounded(&path, 5).unwrap(), b"12345");
        assert_eq!(read_bounded(&path, 100).unwrap(), b"12345");
        assert!(matches!(
            read_bounded(&path, 4),
            Err(ReadError::TooLarge { limit: 4 })
        ));
        fs::write(&path, b"").unwrap();
        assert_eq!(read_bounded(&path, 0).unwrap(), b"");
    }

    #[test]
    fn missing_files_and_folders_are_typed_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            read_bounded(&dir.path().join("missing"), 10),
            Err(ReadError::NotFound)
        ));
        assert!(matches!(read_bounded(dir.path(), 10), Err(ReadError::NotAFile)));
    }

    /// A file that is larger than its reported size (here: a file in
    /// `/proc`, which reports 0 bytes) is still read only up to the limit.
    #[cfg(target_os = "linux")]
    #[test]
    fn files_that_misreport_their_size_are_still_bounded() {
        let path = Path::new("/proc/self/maps");
        assert_eq!(fs::metadata(path).unwrap().len(), 0);
        assert!(matches!(
            read_bounded(path, 16),
            Err(ReadError::TooLarge { limit: 16 })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn devices_and_pipes_are_not_read() {
        assert!(matches!(
            read_bounded(Path::new("/dev/zero"), 10),
            Err(ReadError::NotAFile)
        ));
        assert!(matches!(
            read_bounded(Path::new("/dev/null"), 10),
            Err(ReadError::NotAFile)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn links_to_regular_files_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        fs::write(&path, b"data").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert_eq!(read_bounded(&link, 10).unwrap(), b"data");
        let to_device = dir.path().join("device");
        std::os::unix::fs::symlink("/dev/zero", &to_device).unwrap();
        assert!(matches!(read_bounded(&to_device, 10), Err(ReadError::NotAFile)));
    }
}
