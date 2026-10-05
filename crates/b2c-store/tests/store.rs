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

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).unwrap())
        .collect()
}
