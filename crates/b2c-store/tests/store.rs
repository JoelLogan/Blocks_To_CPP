//! The stores through their public API, the way the desktop backend uses
//! them: folders under one root, settings, the recent list and a project
//! file, saved and reloaded.
// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used)]

use std::fs;

use b2c_store::project_file::{MAX_PROJECT_BYTES, sha256};
use b2c_store::settings::CodeStylePatch;
use b2c_store::{
    Backup, Dirs, ReadError, RecentStore, SettingsPatch, SettingsStore, StoreError, canonical_path,
    parse_rfc3339_utc, random_hex_id, read_project, rfc3339_utc, save_project, sha256_hex, write_atomic,
};

#[test]
fn a_session_saves_settings_recent_projects_and_a_project() {
    let root = tempfile::tempdir().unwrap();
    let dirs = Dirs::under_root(root.path());
    dirs.ensure().unwrap();

    // Settings: defaults first, then a change that survives a restart.
    let (settings, notices) = SettingsStore::open(&dirs.config);
    assert!(notices.is_empty());
    let patch = SettingsPatch {
        code_style: Some(CodeStylePatch {
            indent_width: Some(2),
        }),
        ..SettingsPatch::default()
    };
    settings.update(&patch).unwrap();
    let (settings, _) = SettingsStore::open(&dirs.config);
    assert_eq!(settings.get().code_style.indent_width, 2);

    // A project saved twice keeps the previous version and hashes the bytes.
    let projects = tempfile::tempdir().unwrap();
    let file = projects.path().join("game.b2c");
    save_project(&file, b"first\n").unwrap();
    save_project(&file, b"second\n").unwrap();
    let project_path = canonical_path(&file).unwrap();
    let bytes = read_project(&project_path).unwrap();
    assert_eq!(bytes, b"second\n");
    assert_eq!(sha256_hex(&bytes).len(), 64);
    assert_eq!(sha256(&bytes)[..], hex_to_bytes(&sha256_hex(&bytes))[..]);
    assert_eq!(
        fs::read(projects.path().join("game.b2c.bak")).unwrap(),
        b"first\n"
    );

    // The recent list refers to it by an opaque ID across restarts.
    let recent = RecentStore::open(&dirs.config);
    let id = recent.touch(&project_path, "Guessing game").unwrap();
    let recent = RecentStore::open(&dirs.config);
    assert_eq!(recent.path_of(&id), Some(project_path.clone()));
    assert_eq!(recent.list()[0].project_name, "Guessing game");
    assert!(recent.remove(&id).unwrap());
    assert_eq!(RecentStore::open(&dirs.config).path_of(&id), None);
}

#[test]
fn reads_are_bounded_and_typed() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        read_project(&dir.path().join("missing.b2c")),
        Err(ReadError::NotFound)
    ));
    assert!(matches!(read_project(dir.path()), Err(ReadError::NotAFile)));
    let big = dir.path().join("big.b2c");
    let file = fs::File::create(&big).unwrap();
    file.set_len(MAX_PROJECT_BYTES + 1).unwrap();
    assert!(matches!(
        read_project(&big),
        Err(ReadError::TooLarge {
            limit: MAX_PROJECT_BYTES
        })
    ));
    // As a store error, without the path in the message.
    let error = read_project(&big).unwrap_err().at(&big);
    assert!(matches!(error, StoreError::Invalid(_)));
    assert!(!error.to_string().contains("big.b2c"));
}

#[test]
fn timestamps_and_ids_have_their_fixed_forms() {
    let now = std::time::SystemTime::now();
    let text = rfc3339_utc(now);
    let parsed = parse_rfc3339_utc(&text).unwrap();
    assert!(now.duration_since(parsed).unwrap() < std::time::Duration::from_millis(1));
    let id = random_hex_id("sn_").unwrap();
    assert!(b2c_store::ids::is_hex_id(&id, "sn_"));
}

#[test]
fn writes_need_a_folder_and_leave_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("toolchains.json");
    write_atomic(&target, b"[]", Backup::None).unwrap();
    write_atomic(&target, b"{}", Backup::None).unwrap();
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["toolchains.json"]);
    assert!(
        write_atomic(
            &dir.path().join("no-such-folder").join("x.json"),
            b"",
            Backup::None
        )
        .is_err()
    );
}

/// Every machine-local file is `0600` (02 §2.7, 08 §8.6), also when an
/// older one was left wider; a project keeps the mode its user gave it.
#[cfg(unix)]
#[test]
fn machine_local_files_become_owner_only_and_projects_keep_their_mode() {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = |path: &std::path::Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let wide = || fs::Permissions::from_mode(0o666);

    let root = tempfile::tempdir().unwrap();
    let dirs = Dirs::under_root(root.path());
    dirs.ensure().unwrap();
    let settings_file = dirs.config.join("settings.json");
    let recent_file = dirs.config.join("recent.json");
    fs::write(&settings_file, b"{}").unwrap();
    fs::write(&recent_file, b"{}").unwrap();
    fs::set_permissions(&settings_file, wide()).unwrap();
    fs::set_permissions(&recent_file, wide()).unwrap();

    let (settings, _) = SettingsStore::open(&dirs.config);
    settings.update(&SettingsPatch::default()).unwrap();
    assert_eq!(mode(&settings_file), 0o600);
    let projects = tempfile::tempdir().unwrap();
    let project = projects.path().join("game.b2c");
    save_project(&project, b"first\n").unwrap();
    RecentStore::open(&dirs.config).touch(&project, "Game").unwrap();
    assert_eq!(mode(&recent_file), 0o600);

    // A new project is owner-only; one the user made group-readable stays
    // so, and so does its backup.
    assert_eq!(mode(&project), 0o600);
    fs::set_permissions(&project, fs::Permissions::from_mode(0o640)).unwrap();
    save_project(&project, b"second\n").unwrap();
    assert_eq!(mode(&project), 0o640);
    assert_eq!(mode(&projects.path().join("game.b2c.bak")), 0o640);
}

/// Someone else who can write the project's folder keeps swapping the
/// project file for a FIFO (named pipe) and back. Opening a FIFO that has no
/// writer blocks, so a check-then-open race would hang a read or the `.bak`
/// copy of a save forever. Every call must return: with the file read or
/// saved, or with the FIFO refused.
#[cfg(target_os = "linux")]
#[test]
fn a_file_swapped_for_a_fifo_never_blocks_a_read_or_a_save() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant};

    use rustix::fs::{CWD, Mode, mkfifoat};

    const RACE_FOR: Duration = Duration::from_secs(2);
    const TIMEOUT: Duration = Duration::from_secs(30);

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("game.b2c");
    fs::write(&target, b"old").unwrap();
    let stop = Arc::new(AtomicBool::new(false));

    // The attacker: a FIFO and a regular file, each renamed over the target
    // in turn, so the target is always one or the other.
    let swapper = {
        let (dir, target, stop) = (dir.path().to_path_buf(), target.clone(), Arc::clone(&stop));
        std::thread::spawn(move || {
            let (fifo, file) = (dir.join("swap.fifo"), dir.join("swap.file"));
            let mut swaps = 0_u64;
            while !stop.load(Ordering::Relaxed) {
                mkfifoat(CWD, &fifo, Mode::RUSR | Mode::WUSR).unwrap();
                fs::rename(&fifo, &target).unwrap();
                fs::write(&file, b"old").unwrap();
                fs::rename(&file, &target).unwrap();
                swaps += 1;
            }
            swaps
        })
    };

    // The victims: reads and saves in a loop, each reporting when it is done.
    let (done, finished) = mpsc::channel();
    let victims = [false, true].map(|save| {
        let (target, done) = (target.clone(), done.clone());
        std::thread::spawn(move || {
            let start = Instant::now();
            let mut calls = 0_u64;
            while start.elapsed() < RACE_FOR {
                if save {
                    // A FIFO at the target is refused; a file is saved.
                    let _ = save_project(&target, b"new");
                } else if let Ok(bytes) = read_project(&target) {
                    assert!(bytes == b"old" || bytes == b"new");
                }
                calls += 1;
            }
            done.send((save, calls)).unwrap();
        })
    });
    drop(done);
    let mut results = Vec::new();
    let deadline = Instant::now() + TIMEOUT;
    while results.len() < victims.len() {
        let left = deadline.saturating_duration_since(Instant::now());
        match finished.recv_timeout(left) {
            Ok(result) => results.push(result),
            Err(_) => break,
        }
    }
    stop.store(true, Ordering::Relaxed);
    let swaps = swapper.join().unwrap();
    let expected = victims.len();
    // A thread that ended without reporting panicked: show its panic. One
    // that is still running is blocked and cannot be joined.
    for victim in victims {
        if victim.is_finished() {
            victim.join().unwrap();
        }
    }
    assert_eq!(
        results.len(),
        expected,
        "a read or save blocked on the FIFO (finished: {results:?}, swaps: {swaps})"
    );
    assert!(swaps > 0);
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).unwrap())
        .collect()
}
