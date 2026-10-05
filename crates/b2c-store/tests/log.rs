//! The rotating log writer through its public API, with the app's settings
//! (`docs/spec/08-security.md` §8.11: `blocks2cpp.log` and
//! `blocks2cpp.1.log` to `blocks2cpp.4.log`, 5 MiB each, `0600` in a `0700`
//! folder).
// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

use b2c_store::log::{LOG_BASE, LOG_FILES, LOG_MAX_BYTES};
use b2c_store::{Dirs, RotatingFile};

/// The log files in `dir`, sorted by name.
fn log_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// A JSON line like the app's, about 200 bytes, numbered.
fn line(sequence: u64) -> String {
    format!(
        "{{\"timestamp\":\"2026-10-05T14:03:27.512Z\",\"level\":\"INFO\",\"target\":\"b2c_build\",\"seq\":{sequence},\"message\":\"{}\"}}\n",
        "x".repeat(96)
    )
}

/// The sequence numbers of the lines of a file, checking that every line is
/// whole.
fn sequences(path: &Path) -> Vec<u64> {
    let text = fs::read_to_string(path).unwrap();
    assert!(
        text.is_empty() || text.ends_with('\n'),
        "a partial last line in {}",
        path.display()
    );
    text.lines()
        .map(|line| {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            value["seq"].as_u64().unwrap()
        })
        .collect()
}

/// 09 §9.1 / 08 §8.11: after more than 25 MiB, at most five files remain,
/// each at most 5 MiB, the oldest lines are gone and no line is split.
#[test]
fn more_than_25_mib_leaves_five_files_of_at_most_5_mib() {
    let root = tempfile::tempdir().unwrap();
    let dirs = Dirs::under_root(root.path());
    let mut log = RotatingFile::open(&dirs.logs, LOG_BASE, LOG_MAX_BYTES, LOG_FILES).unwrap();
    let mut written = 0_u64;
    let mut sequence = 0_u64;
    while written <= 30 * 1024 * 1024 {
        let text = line(sequence);
        log.write_all(text.as_bytes()).unwrap();
        written += u64::try_from(text.len()).unwrap();
        sequence += 1;
    }
    log.flush().unwrap();
    let last = sequence - 1;

    assert_eq!(
        log_files(&dirs.logs),
        [
            "blocks2cpp.1.log",
            "blocks2cpp.2.log",
            "blocks2cpp.3.log",
            "blocks2cpp.4.log",
            "blocks2cpp.log"
        ]
    );
    // Oldest file first; every file within the limit, and nearly full but
    // for the current one.
    let paths: Vec<_> = log.paths().into_iter().rev().collect();
    let mut all = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let size = fs::metadata(path).unwrap().len();
        assert!(size <= LOG_MAX_BYTES, "{} is {size} bytes", path.display());
        if index + 1 < paths.len() {
            assert!(
                size > LOG_MAX_BYTES - 1024,
                "{} is only {size} bytes",
                path.display()
            );
        }
        all.extend(sequences(path));
    }
    // The lines are consecutive up to the last one written: only the oldest
    // were dropped.
    assert_eq!(all.last(), Some(&last));
    assert!(all.windows(2).all(|pair| pair[1] == pair[0] + 1));
    let first = all[0];
    assert!(first > 0, "the oldest lines are dropped");
    let kept: u64 = paths.iter().map(|path| fs::metadata(path).unwrap().len()).sum();
    assert!(kept > 4 * LOG_MAX_BYTES);
}

#[test]
fn reopening_appends_and_removes_leftover_files() {
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    {
        let mut log = RotatingFile::open(&logs, "app", 1024, 8).unwrap();
        for sequence in 0..40 {
            log.write_all(line(sequence).as_bytes()).unwrap();
        }
    }
    assert_eq!(log_files(&logs).len(), 8);
    fs::write(logs.join("app.1.log.bak"), b"kept").unwrap();
    fs::write(logs.join("notes.txt"), b"kept").unwrap();
    fs::create_dir(logs.join("app.9.log")).unwrap();

    // Fewer files now: generations 3 and above are deleted (not the folder
    // that only looks like one, nor unrelated files), and new lines are
    // appended to the current file.
    let before = sequences(&logs.join("app.log"));
    let mut log = RotatingFile::open(&logs, "app", 1024, 3).unwrap();
    assert_eq!(
        log_files(&logs),
        [
            "app.1.log",
            "app.1.log.bak",
            "app.2.log",
            "app.9.log",
            "app.log",
            "notes.txt"
        ]
    );
    log.write_all(line(40).as_bytes()).unwrap();
    let after = sequences(&logs.join("app.log"));
    assert!(after.starts_with(&before) || after == [40]);
    assert_eq!(after.last(), Some(&40));
}

/// Several app instances write the same log: every file stays within the
/// limit and there are never more files than allowed.
#[test]
fn instances_sharing_a_log_keep_its_limits() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = RotatingFile::open(dir.path(), "shared", 4096, 4).unwrap();
    let mut second = RotatingFile::open(dir.path(), "shared", 4096, 4).unwrap();
    for sequence in 0..400 {
        let writer = if sequence % 3 == 0 {
            &mut second
        } else {
            &mut first
        };
        writer.write_all(line(sequence).as_bytes()).unwrap();
        let names = log_files(dir.path());
        assert!(names.len() <= 4, "{names:?}");
        for name in names {
            let size = fs::metadata(dir.path().join(&name)).unwrap().len();
            assert!(size <= 4096, "{name} is {size} bytes");
        }
    }
}

/// The app shares one writer between threads behind a mutex (as a tracing
/// `MakeWriter`): lines never mix.
#[test]
fn a_shared_writer_keeps_lines_whole() {
    let dir = tempfile::tempdir().unwrap();
    let log = Arc::new(Mutex::new(
        RotatingFile::open(dir.path(), "app", 64 * 1024, 3).unwrap(),
    ));
    let threads: Vec<_> = (0..4_u64)
        .map(|thread| {
            let log = Arc::clone(&log);
            thread::spawn(move || {
                for index in 0..500 {
                    let text = line(thread * 1_000 + index);
                    log.lock().unwrap().write_all(text.as_bytes()).unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let mut seen = 0;
    for name in log_files(dir.path()) {
        let path = dir.path().join(&name);
        assert!(fs::metadata(&path).unwrap().len() <= 64 * 1024);
        seen += sequences(&path).len();
    }
    assert!(seen > 0);
}

#[cfg(unix)]
#[test]
fn files_are_owner_only_in_a_private_folder() {
    use std::os::unix::fs::PermissionsExt as _;
    let root = tempfile::tempdir().unwrap();
    let dirs = Dirs::under_root(root.path());
    fs::create_dir_all(&dirs.logs).unwrap();
    fs::set_permissions(&dirs.logs, fs::Permissions::from_mode(0o755)).unwrap();
    let current = dirs.logs.join("blocks2cpp.log");
    fs::write(&current, b"").unwrap();
    fs::set_permissions(&current, fs::Permissions::from_mode(0o644)).unwrap();

    let mut log = RotatingFile::open(&dirs.logs, LOG_BASE, 256, LOG_FILES).unwrap();
    for sequence in 0..20 {
        log.write_all(line(sequence).as_bytes()).unwrap();
    }
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&dirs.logs), 0o700);
    for path in log.paths() {
        assert_eq!(mode(&path), 0o600, "{}", path.display());
    }
}

#[cfg(unix)]
#[test]
fn links_are_never_written_through() {
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    fs::create_dir(&logs).unwrap();
    let target = dir.path().join("target.txt");
    fs::write(&target, b"precious").unwrap();
    std::os::unix::fs::symlink(&target, logs.join("app.log")).unwrap();
    let error = RotatingFile::open(&logs, "app", 1024, 2).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(!error.to_string().contains("target"));
    assert_eq!(fs::read(&target).unwrap(), b"precious");

    // A link that appears at the current file's path before a rotation is
    // replaced by the rotation's rename, never followed.
    fs::remove_file(logs.join("app.log")).unwrap();
    let mut log = RotatingFile::open(&logs, "app", 64, 2).unwrap();
    log.write_all(b"first line\n").unwrap();
    std::os::unix::fs::symlink(&target, logs.join("app.1.log")).unwrap();
    for _ in 0..10 {
        log.write_all(b"another line\n").unwrap();
    }
    assert_eq!(fs::read(&target).unwrap(), b"precious");
    assert!(
        !fs::symlink_metadata(logs.join("app.1.log"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
