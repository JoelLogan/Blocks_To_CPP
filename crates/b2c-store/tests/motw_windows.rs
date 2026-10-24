//! Mark-of-the-Web on a real NTFS alternate data stream
//! (`docs/spec/08-security.md` §8.3.1). Windows only: other platforms have
//! no `Zone.Identifier` streams (see the unit tests in `src/motw.rs`).
#![cfg(windows)]
// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use b2c_store::{canonical_path, mark_of_the_web};

/// A project file in a fresh folder, by its canonical path.
fn project(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, b"{}").unwrap();
    canonical_path(&path).unwrap()
}

/// Writes `contents` into `<path>:Zone.Identifier`, as browsers do.
fn mark(path: &Path, contents: &[u8]) {
    let mut stream = path.as_os_str().to_os_string();
    stream.push(":Zone.Identifier");
    fs::write(PathBuf::from(stream), contents).unwrap();
}

#[test]
fn internet_zones_mark_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let internet = project(dir.path(), "internet.b2c");
    mark(&internet, b"[ZoneTransfer]\r\nZoneId=3");
    assert!(mark_of_the_web(&internet));

    let restricted = project(dir.path(), "restricted.b2c");
    mark(
        &restricted,
        b"[ZoneTransfer]\r\nZoneId=4\r\nHostUrl=https://example.com/restricted.b2c\r\n",
    );
    assert!(mark_of_the_web(&restricted));

    // The stream is read through the verbatim form of the path too.
    let verbatim = PathBuf::from(format!(r"\\?\{}", internet.to_str().unwrap()));
    assert!(mark_of_the_web(&verbatim));
}

#[test]
fn other_zones_and_missing_or_broken_streams_do_not() {
    let dir = tempfile::tempdir().unwrap();
    let trusted = project(dir.path(), "trusted.b2c");
    mark(&trusted, b"[ZoneTransfer]\r\nZoneId=2");
    assert!(!mark_of_the_web(&trusted));

    let plain = project(dir.path(), "plain.b2c");
    assert!(!mark_of_the_web(&plain));

    let broken = project(dir.path(), "broken.b2c");
    mark(&broken, b"[ZoneTransfer]\r\nZoneId=three");
    assert!(!mark_of_the_web(&broken));

    let oversized = project(dir.path(), "oversized.b2c");
    let mut contents = b"[ZoneTransfer]\r\nZoneId=3\r\n".to_vec();
    contents.resize(64 * 1024 + 1, b'\n');
    mark(&oversized, &contents);
    assert!(!mark_of_the_web(&oversized));

    assert!(!mark_of_the_web(&dir.path().join("missing.b2c")));
}
