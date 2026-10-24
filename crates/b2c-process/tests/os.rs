//! The OS helpers of `b2c_process::os`: atomic replacement (both platforms),
//! non-blocking opens, DLL search hardening (a no-op here; on Windows it
//! changes the whole process, so it has a test binary of its own,
//! `tests/os_windows.rs`) and the link opener's URL check.
// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::Read as _;

use b2c_process::ProcessError;
use b2c_process::os::{atomic_replace, open_https_url, open_read_nonblocking};

#[test]
fn regular_files_open_and_read_normally() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.b2c");
    let contents = vec![b'x'; 256 * 1024];
    fs::write(&path, &contents).unwrap();
    let mut file = open_read_nonblocking(&path).unwrap();
    assert!(file.metadata().unwrap().is_file());
    let mut read = Vec::new();
    file.read_to_end(&mut read).unwrap();
    assert_eq!(read, contents);
    assert_eq!(
        open_read_nonblocking(&dir.path().join("missing"))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::NotFound
    );
}

/// A FIFO that no process ever opens for writing: a plain `File::open`
/// would wait forever. The open must return at once, and the handle must
/// show what it is, so the caller can refuse it.
#[cfg(target_os = "linux")]
#[test]
fn a_fifo_without_a_writer_opens_at_once() {
    use std::os::unix::fs::FileTypeExt as _;
    use std::sync::mpsc;
    use std::time::Duration;

    use rustix::fs::{CWD, Mode, mkfifoat};

    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("fifo");
    mkfifoat(CWD, &fifo, Mode::RUSR | Mode::WUSR).unwrap();
    let (done, opened) = mpsc::channel();
    std::thread::spawn(move || {
        let file = open_read_nonblocking(&fifo).unwrap();
        done.send(file.metadata().unwrap().file_type().is_fifo()).unwrap();
    });
    let is_fifo = opened
        .recv_timeout(Duration::from_secs(30))
        .expect("opening the FIFO blocked");
    assert!(is_fifo);
}

#[test]
fn replaces_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("project.b2c");
    let temp = dir.path().join(".project.b2c.tmp");
    fs::write(&target, "old").unwrap();
    fs::write(&temp, "new").unwrap();
    atomic_replace(&temp, &target).unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    assert!(!temp.exists());
}

#[test]
fn creates_a_missing_target() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("settings.json");
    let temp = dir.path().join(".settings.json.tmp");
    fs::write(&temp, "{}").unwrap();
    atomic_replace(&temp, &target).unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "{}");
}

#[test]
fn a_failed_replace_leaves_the_target_alone() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("project.b2c");
    fs::write(&target, "old").unwrap();
    assert!(atomic_replace(&dir.path().join("missing.tmp"), &target).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "old");
}

#[test]
fn long_paths_work() {
    let dir = tempfile::tempdir().unwrap();
    let mut deep = dir.path().to_path_buf();
    // Well past MAX_PATH (260) in total, each level within the 255 limit.
    for level in 0..4 {
        deep.push(format!("{level}{}", "d".repeat(80)));
    }
    fs::create_dir_all(&deep).unwrap();
    let target = deep.join("project.b2c");
    let temp = deep.join(".project.b2c.tmp");
    fs::write(&target, "old").unwrap();
    fs::write(&temp, "new").unwrap();
    atomic_replace(&temp, &target).unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "new");
}

#[cfg(unix)]
#[test]
fn the_directory_is_synced() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("file");
    let temp = dir.path().join("file.tmp");
    fs::write(&temp, "data").unwrap();
    let before = b2c_process::os::directory_syncs();
    atomic_replace(&temp, &target).unwrap();
    assert!(b2c_process::os::directory_syncs() > before);
}

#[cfg(unix)]
#[test]
fn a_link_at_the_target_is_replaced_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let victim = dir.path().join("victim");
    fs::write(&victim, "keep me").unwrap();
    let target = dir.path().join("target");
    std::os::unix::fs::symlink(&victim, &target).unwrap();
    let temp = dir.path().join("target.tmp");
    fs::write(&temp, "new").unwrap();
    atomic_replace(&temp, &target).unwrap();
    assert!(!fs::symlink_metadata(&target).unwrap().file_type().is_symlink());
    assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    assert_eq!(fs::read_to_string(&victim).unwrap(), "keep me");
}

#[test]
fn unsafe_urls_are_refused_before_anything_starts() {
    for url in [
        "http://example.com/",
        "file:///etc/passwd",
        "https://example.com/\"; calc.exe; \"",
        "https://example.com/a b",
        "https://example.com/$(touch x)",
        "https://user@example.com/",
    ] {
        assert!(
            matches!(open_https_url(url), Err(ProcessError::InvalidUrl)),
            "{url}"
        );
    }
}

#[cfg(not(windows))]
#[test]
fn dll_hardening_is_a_no_op_elsewhere() {
    b2c_process::os::harden_dll_search().unwrap();
    b2c_process::os::harden_dll_search().unwrap();
}
