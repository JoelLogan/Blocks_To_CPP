//! The `os-release` reader of `b2c_toolchain::host` against real files
//! (`tests/fixtures/os-release`, copied from the distributions named) and
//! files built at run time (the oversized one, which would only bloat the
//! repository).

// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(clippy::panic, clippy::expect_used)]

use std::path::{Path, PathBuf};

use b2c_toolchain::host::{
    MAX_OS_RELEASE_BYTES, OS_RELEASE_PATHS, OsRelease, parse_os_release, read_os_release, read_os_release_in,
};

fn fixture(name: &str) -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "tests",
        "fixtures",
        "os-release",
        name,
    ]
    .iter()
    .collect()
}

fn read_fixture(name: &str) -> Option<OsRelease> {
    read_os_release_in(&[&fixture(name)])
}

fn release(id: &str, like: &[&str]) -> OsRelease {
    OsRelease {
        id: id.to_owned(),
        id_like: like.iter().map(|word| (*word).to_owned()).collect(),
    }
}

#[test]
fn real_distribution_files_are_read() {
    assert_eq!(read_fixture("ubuntu"), Some(release("ubuntu", &["debian"])));
    assert_eq!(read_fixture("debian"), Some(release("debian", &[])));
    assert_eq!(read_fixture("fedora"), Some(release("fedora", &[])));
    assert_eq!(read_fixture("arch"), Some(release("arch", &[])));
    assert_eq!(
        read_fixture("rocky"),
        Some(release("rocky", &["rhel", "centos", "fedora"]))
    );
}

#[test]
fn malformed_files_are_not_trusted() {
    assert_eq!(read_fixture("malformed"), None);
    assert_eq!(read_fixture("unterminated"), None);
}

#[test]
fn oversized_files_are_refused_without_being_loaded() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let path = dir.path().join("os-release");
    let limit = usize::try_from(MAX_OS_RELEASE_BYTES).expect("64 KiB fits");
    let mut text = String::from("ID=ubuntu\n");
    text.push_str(&"#".repeat(limit - text.len()));
    std::fs::write(&path, &text).expect("written");
    assert_eq!(read_os_release_in(&[&path]), Some(release("ubuntu", &[])));
    text.push('#');
    std::fs::write(&path, &text).expect("written");
    assert_eq!(read_os_release_in(&[&path]), None);
    assert_eq!(parse_os_release(text.as_bytes()), None);
}

#[test]
fn the_second_file_is_read_only_when_the_first_does_not_exist() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let missing = dir.path().join("missing");
    assert_eq!(
        read_os_release_in(&[&missing, &fixture("fedora")]),
        Some(release("fedora", &[]))
    );
    // A first file that exists but is malformed is the answer: no fallback.
    assert_eq!(
        read_os_release_in(&[&fixture("malformed"), &fixture("fedora")]),
        None
    );
    // A folder in its place is not read, and is no reason to fall back.
    assert_eq!(read_os_release_in(&[dir.path(), &fixture("fedora")]), None);
    assert_eq!(read_os_release_in(&[&missing]), None);
    assert_eq!(read_os_release_in(&[]), None);
}

#[cfg(target_os = "linux")]
#[test]
fn a_fifo_in_place_of_the_file_never_blocks() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let fifo = dir.path().join("os-release");
    let status = b2c_process::run_captured(
        b2c_process::Command::new(Path::new("/usr/bin/mkfifo"), dir.path())
            .expect("a command")
            .arg(&fifo),
    )
    .expect("mkfifo runs");
    assert!(status.status.success(), "mkfifo failed");
    assert_eq!(read_os_release_in(&[&fifo]), None);
}

#[test]
fn this_computer_reports_a_well_formed_distribution_or_none() {
    let found = read_os_release();
    if cfg!(windows) {
        assert_eq!(found, None);
        return;
    }
    let exists = OS_RELEASE_PATHS.iter().any(|path| Path::new(path).exists());
    if let Some(release) = found {
        assert!(exists, "a distribution without an os-release file");
        assert!(!release.id.is_empty());
        for word in std::iter::once(&release.id).chain(&release.id_like) {
            assert!(
                word.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b)),
                "{word:?}"
            );
        }
    }
}
