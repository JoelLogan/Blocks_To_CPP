//! Shared file-system checks: links and reparse points (08 §8.6).

use std::fs::Metadata;

/// `FILE_ATTRIBUTE_REPARSE_POINT` (Win32): set on symbolic links,
/// junctions, mount points and every other reparse point.
#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// Whether `metadata` (from `symlink_metadata`, which does not follow
/// links) describes a link: a symbolic link, or on Windows any
/// name-surrogate reparse point (a symbolic link, junction or mount point).
/// Files are never written through such an entry.
///
/// Other reparse points on files, such as cloud-storage placeholders (a
/// project in a synced Documents folder), are not links and are accepted.
pub(crate) fn is_link(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// Whether `metadata` (from `symlink_metadata`) describes a link or, on
/// Windows, any reparse point at all. The app's own folders must be none of
/// these.
pub(crate) fn is_link_or_reparse_point(metadata: &Metadata) -> bool {
    if is_link(metadata) {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

/// Whether two metadata records describe the same file (Unix: same device
/// and inode). Elsewhere only the kind and size are compared, which still
/// catches a swap to a different kind of file.
pub(crate) fn same_file(a: &Metadata, b: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    #[cfg(not(unix))]
    {
        a.is_file() == b.is_file() && a.len() == b.len()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn links_are_recognised() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        std::fs::write(&file, "x").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        let metadata = |path: &std::path::Path| std::fs::symlink_metadata(path).unwrap();
        assert!(!is_link(&metadata(&file)));
        assert!(is_link(&metadata(&link)));
        assert!(!is_link_or_reparse_point(&metadata(&file)));
        assert!(is_link_or_reparse_point(&metadata(&link)));
        assert!(!is_link_or_reparse_point(&metadata(dir.path())));
        let a = std::fs::metadata(&file).unwrap();
        assert!(same_file(&a, &std::fs::metadata(&link).unwrap()));
        assert!(!same_file(&a, &std::fs::metadata(dir.path()).unwrap()));
    }
}
