//! The trust store through its public API, the way the desktop backend uses
//! it (`docs/spec/05-project-format.md` §5.9, `docs/spec/08-security.md`
//! §8.3.1): every evaluation case, fail-closed loading, bounded reads,
//! round trips, revocation, saves in the app against changes outside it, and
//! several app instances sharing one file.
// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use b2c_ir::ProjectId;
use b2c_store::trust::{MAX_TRUST_BYTES, TRUST_FILE};
use b2c_store::{
    Dirs, ProjectIdentity, RestrictedReason, StoreError, TrustFileProblem, TrustSource, TrustStore,
    TrustVerdict, canonical_path, parse_rfc3339_utc, save_project,
};
use serde_json::Value;

const TRUSTED_PROJECT: TrustVerdict = TrustVerdict::Trusted(TrustSource::Project);
const TRUSTED_FOLDER: TrustVerdict = TrustVerdict::Trusted(TrustSource::Folder);
const NO_RECORD: TrustVerdict = TrustVerdict::Restricted(RestrictedReason::NoRecord);
const CHANGED_OUTSIDE: TrustVerdict = TrustVerdict::Restricted(RestrictedReason::ChangedOutside);

/// A machine folder and the projects folder of one test.
struct Fixture {
    _root: tempfile::TempDir,
    trust_file: PathBuf,
    projects: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let base = canonical_path(root.path()).unwrap();
        let dirs = Dirs::under_root(&base);
        let projects = base.join("projects");
        fs::create_dir(&projects).unwrap();
        Self {
            trust_file: dirs.machine.join(TRUST_FILE),
            projects,
            _root: root,
        }
    }

    fn open(&self) -> TrustStore {
        TrustStore::open(&self.trust_file)
    }

    /// A project file at `relative` below the projects folder (folders are
    /// created), by its canonical path.
    fn project_file(&self, relative: &str) -> PathBuf {
        let path = self.projects.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{}").unwrap();
        canonical_path(&path).unwrap()
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&fs::read(&self.trust_file).unwrap()).unwrap()
    }
}

fn identity(id: &str, path: &Path, hash: u8) -> ProjectIdentity {
    ProjectIdentity {
        project_id: ProjectId::new(id).unwrap(),
        canonical_path: path.to_path_buf(),
        security_hash: [hash; 32],
    }
}

#[test]
fn every_evaluation_case() {
    let fixture = Fixture::new();
    let store = fixture.open();
    let game = fixture.project_file("games/guess.b2c");
    let nested = fixture.project_file("class/week1/loops/count.b2c");
    let copy = fixture.project_file("downloads/guess.b2c");
    let sibling = fixture.project_file("classic/other.b2c");

    // Nothing is trusted at first.
    for (id, path) in [("prj_game", &game), ("prj_loop", &nested)] {
        assert_eq!(store.evaluate(&identity(id, path, 1)), NO_RECORD);
    }

    store.grant_project(&identity("prj_game", &game, 1)).unwrap();
    store
        .grant_folder(&canonical_path(&fixture.projects.join("class")).unwrap())
        .unwrap();

    // A project record with the same hash.
    assert_eq!(store.evaluate(&identity("prj_game", &game, 1)), TRUSTED_PROJECT);
    // The same project and path with another hash: changed outside the app.
    assert_eq!(store.evaluate(&identity("prj_game", &game, 2)), CHANGED_OUTSIDE);
    // A copied file at another path, or another project at the same path.
    assert_eq!(store.evaluate(&identity("prj_game", &copy, 1)), NO_RECORD);
    assert_eq!(store.evaluate(&identity("prj_other", &game, 1)), NO_RECORD);
    // A folder record covers everything below it, whatever the hash.
    assert_eq!(store.evaluate(&identity("prj_loop", &nested, 9)), TRUSTED_FOLDER);
    assert_eq!(
        store.folder_covering(&nested),
        Some(canonical_path(&fixture.projects.join("class")).unwrap())
    );
    // Folders are compared by whole path parts: "classic" is not in "class".
    assert_eq!(store.evaluate(&identity("prj_x", &sibling, 9)), NO_RECORD);
    assert_eq!(store.folder_covering(&sibling), None);
    // A path that escapes the folder with ".." is never covered.
    let escaping = fixture
        .projects
        .join("class")
        .join("..")
        .join("classic")
        .join("other.b2c");
    assert_eq!(store.evaluate(&identity("prj_x", &escaping, 9)), NO_RECORD);
    assert_eq!(store.folder_covering(&escaping), None);
    assert_eq!(store.problem(), None);
}

#[test]
fn records_survive_a_write_read_round_trip() {
    let fixture = Fixture::new();
    let game = fixture.project_file("game.b2c");
    let folder = canonical_path(&fixture.projects).unwrap();
    {
        let store = fixture.open();
        store.grant_project(&identity("prj_game", &game, 0xab)).unwrap();
        store.grant_folder(&folder).unwrap();
    }
    let store = fixture.open();
    assert_eq!(store.problem(), None);
    assert_eq!(
        store.evaluate(&identity("prj_game", &game, 0xab)),
        TRUSTED_PROJECT
    );
    assert_eq!(store.folder_covering(&game), Some(folder.clone()));

    let json = fixture.json();
    assert_eq!(json["format"], "blocks2cpp/trust");
    assert_eq!(json["formatVersion"], 1);
    let object = json.as_object().unwrap();
    assert_eq!(object.len(), 4);
    let project = &json["projects"][0];
    assert_eq!(project.as_object().unwrap().len(), 4);
    assert_eq!(project["projectId"], "prj_game");
    assert_eq!(project["canonicalPath"], game.to_str().unwrap());
    assert_eq!(project["rawCodeHashAtGrant"], "ab".repeat(32));
    assert!(parse_rfc3339_utc(project["grantedAt"].as_str().unwrap()).is_some());
    let folder_record = &json["folders"][0];
    assert_eq!(folder_record.as_object().unwrap().len(), 2);
    assert_eq!(folder_record["canonicalPath"], folder.to_str().unwrap());
    assert!(parse_rfc3339_utc(folder_record["grantedAt"].as_str().unwrap()).is_some());

    // Granting again replaces the record rather than adding one.
    store.grant_project(&identity("prj_game", &game, 0xcd)).unwrap();
    store.grant_folder(&folder).unwrap();
    let json = fixture.json();
    assert_eq!(json["projects"].as_array().unwrap().len(), 1);
    assert_eq!(json["folders"].as_array().unwrap().len(), 1);
    assert_eq!(json["projects"][0]["rawCodeHashAtGrant"], "cd".repeat(32));
    // A different project saved over the path replaces the old record.
    store.grant_project(&identity("prj_new", &game, 1)).unwrap();
    assert_eq!(store.evaluate(&identity("prj_new", &game, 1)), TRUSTED_PROJECT);
    let json = fixture.json();
    assert_eq!(json["projects"].as_array().unwrap().len(), 1);
    assert_eq!(json["projects"][0]["projectId"], "prj_new");
}

#[test]
fn a_corrupt_store_trusts_nothing_and_the_next_grant_writes_a_valid_file() {
    let fixture = Fixture::new();
    let game = fixture.project_file("game.b2c");
    let other = fixture.project_file("other.b2c");
    let folder = canonical_path(&fixture.projects).unwrap();
    fixture
        .open()
        .grant_project(&identity("prj_game", &game, 1))
        .unwrap();
    fixture.open().grant_folder(&folder).unwrap();
    let valid = fs::read(&fixture.trust_file).unwrap();

    let truncated = valid[..valid.len() / 2].to_vec();
    let newer = String::from_utf8(valid.clone())
        .unwrap()
        .replace("\"formatVersion\": 1", "\"formatVersion\": 2")
        .into_bytes();
    let wrong_tag = String::from_utf8(valid.clone())
        .unwrap()
        .replace("blocks2cpp/trust", "blocks2cpp/recent")
        .into_bytes();
    let extra_key = String::from_utf8(valid.clone())
        .unwrap()
        .replacen("\"projects\"", "\"extra\": true, \"projects\"", 1)
        .into_bytes();
    for (contents, problem) in [
        (b"not json at all".to_vec(), TrustFileProblem::Invalid),
        (Vec::new(), TrustFileProblem::Invalid),
        (truncated, TrustFileProblem::Invalid),
        (wrong_tag, TrustFileProblem::Invalid),
        (extra_key, TrustFileProblem::Invalid),
        (newer, TrustFileProblem::NewerVersion),
    ] {
        fs::write(&fixture.trust_file, &contents).unwrap();
        let store = fixture.open();
        assert_eq!(
            store.problem(),
            Some(problem),
            "{}",
            String::from_utf8_lossy(&contents)
        );
        // Every lookup is restricted.
        assert_eq!(store.evaluate(&identity("prj_game", &game, 1)), NO_RECORD);
        assert_eq!(store.evaluate(&identity("prj_x", &other, 1)), NO_RECORD);
        assert_eq!(store.folder_covering(&game), None);
        // Revoking changes nothing, so nothing is written.
        assert!(
            !store
                .revoke_project(&ProjectId::new("prj_game").unwrap(), &game)
                .unwrap()
        );
        assert_eq!(fs::read(&fixture.trust_file).unwrap(), contents);
        // The next grant replaces the file with a valid one.
        store.grant_project(&identity("prj_other", &other, 3)).unwrap();
        assert_eq!(store.problem(), None);
        let reopened = fixture.open();
        assert_eq!(reopened.problem(), None);
        assert_eq!(
            reopened.evaluate(&identity("prj_other", &other, 3)),
            TRUSTED_PROJECT
        );
        assert_eq!(reopened.evaluate(&identity("prj_game", &game, 1)), NO_RECORD);
        assert_eq!(fixture.json()["projects"].as_array().unwrap().len(), 1);
    }
    // A corrupt file appearing later is noticed by the next evaluation.
    let store = fixture.open();
    assert_eq!(store.evaluate(&identity("prj_other", &other, 3)), TRUSTED_PROJECT);
    fs::write(&fixture.trust_file, b"{").unwrap();
    assert_eq!(store.evaluate(&identity("prj_other", &other, 3)), NO_RECORD);
    assert_eq!(store.problem(), Some(TrustFileProblem::Invalid));
}

#[test]
fn the_read_is_bounded() {
    let fixture = Fixture::new();
    let game = fixture.project_file("game.b2c");
    fixture
        .open()
        .grant_project(&identity("prj_game", &game, 1))
        .unwrap();
    let valid = fs::read(&fixture.trust_file).unwrap();
    let limit = usize::try_from(MAX_TRUST_BYTES).unwrap();

    // Exactly at the limit (padded with JSON white space): still read.
    let mut padded = valid.clone();
    padded.resize(limit, b' ');
    fs::write(&fixture.trust_file, &padded).unwrap();
    let store = fixture.open();
    assert_eq!(store.problem(), None);
    assert_eq!(store.evaluate(&identity("prj_game", &game, 1)), TRUSTED_PROJECT);

    // One byte more: not read at all, so nothing is trusted.
    padded.push(b' ');
    fs::write(&fixture.trust_file, &padded).unwrap();
    let store = fixture.open();
    assert_eq!(store.problem(), Some(TrustFileProblem::TooLarge));
    assert_eq!(store.evaluate(&identity("prj_game", &game, 1)), NO_RECORD);
    store.grant_project(&identity("prj_game", &game, 1)).unwrap();
    assert!(fs::metadata(&fixture.trust_file).unwrap().len() < MAX_TRUST_BYTES);
    assert_eq!(
        fixture.open().evaluate(&identity("prj_game", &game, 1)),
        TRUSTED_PROJECT
    );
}

#[test]
fn revoking_removes_only_the_project_record() {
    let fixture = Fixture::new();
    let store = fixture.open();
    let game = fixture.project_file("games/guess.b2c");
    let other = fixture.project_file("other.b2c");
    let games = canonical_path(&fixture.projects.join("games")).unwrap();
    let id = ProjectId::new("prj_game").unwrap();
    store.grant_project(&identity("prj_game", &game, 1)).unwrap();
    store.grant_project(&identity("prj_other", &other, 1)).unwrap();
    store.grant_folder(&games).unwrap();
    assert_eq!(store.evaluate(&identity("prj_game", &game, 1)), TRUSTED_PROJECT);

    // Another project's ID or another path removes nothing.
    assert!(
        !store
            .revoke_project(&ProjectId::new("prj_x").unwrap(), &game)
            .unwrap()
    );
    assert!(!store.revoke_project(&id, &other).unwrap());
    assert!(!store.revoke_project(&id, Path::new("relative.b2c")).unwrap());

    assert!(store.revoke_project(&id, &game).unwrap());
    assert!(!store.revoke_project(&id, &game).unwrap());
    // The folder still covers it, and says so.
    assert_eq!(store.evaluate(&identity("prj_game", &game, 1)), TRUSTED_FOLDER);
    assert_eq!(store.folder_covering(&game), Some(games));
    // The other project and the folder record are untouched.
    assert_eq!(store.evaluate(&identity("prj_other", &other, 1)), TRUSTED_PROJECT);
    let json = fixture.json();
    assert_eq!(json["projects"].as_array().unwrap().len(), 1);
    assert_eq!(json["projects"][0]["projectId"], "prj_other");
    assert_eq!(json["folders"].as_array().unwrap().len(), 1);
}

/// 08 §8.3: a define edited in the file outside the app restricts the
/// project again; the same edit saved in the app keeps it trusted.
#[test]
fn saves_in_the_app_keep_trust_and_changes_outside_restrict() {
    let fixture = Fixture::new();
    let store = fixture.open();
    let example =
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello_world.b2c")).unwrap();
    let document = b2c_model::load(&example).unwrap();
    let file = fixture.projects.join("hello.b2c");
    save_project(&file, b2c_model::to_canonical_json(&document).as_bytes()).unwrap();
    let path = canonical_path(&file).unwrap();
    let opened = |path: &Path| {
        let document = b2c_model::load(&fs::read(path).unwrap()).unwrap();
        ProjectIdentity {
            project_id: document.project.id.clone(),
            canonical_path: path.to_path_buf(),
            security_hash: b2c_model::security_hash(&document),
        }
    };
    store.grant_project(&opened(&path)).unwrap();
    assert_eq!(store.evaluate(&opened(&path)), TRUSTED_PROJECT);

    // Moving a block does not touch the security hash.
    let mut json: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    json["modules"][0]["workspace"]["blocks"][0]["x"] = 500.into();
    fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();
    assert_eq!(store.evaluate(&opened(&path)), TRUSTED_PROJECT);

    // A define added outside the app.
    json["project"]["build"]["defines"] = serde_json::json!([{"name": "GAME_LEVEL", "value": {"int": 3}}]);
    let edited = serde_json::to_vec_pretty(&json).unwrap();
    fs::write(&path, &edited).unwrap();
    assert_eq!(store.evaluate(&opened(&path)), CHANGED_OUTSIDE);
    assert_eq!(fixture.open().evaluate(&opened(&path)), CHANGED_OUTSIDE);

    // The same edit made and saved in the app (the project is trusted
    // there, so the app records the save).
    let mut fresh: Value =
        serde_json::from_slice(&b2c_model::to_canonical_json(&document).into_bytes()).unwrap();
    fs::write(&path, serde_json::to_vec_pretty(&fresh).unwrap()).unwrap();
    assert_eq!(store.evaluate(&opened(&path)), TRUSTED_PROJECT);
    fresh["project"]["build"]["defines"] = json["project"]["build"]["defines"].clone();
    let saved = b2c_model::load(&serde_json::to_vec(&fresh).unwrap()).unwrap();
    save_project(&path, b2c_model::to_canonical_json(&saved).as_bytes()).unwrap();
    assert!(store.record_save(&opened(&path)).unwrap());
    assert_eq!(fixture.open().evaluate(&opened(&path)), TRUSTED_PROJECT);
    // The old hash is now the outside change.
    let mut old = opened(&path);
    old.security_hash = b2c_model::security_hash(&document);
    assert_eq!(store.evaluate(&old), CHANGED_OUTSIDE);
}

#[test]
fn recording_a_save_needs_a_project_record() {
    let fixture = Fixture::new();
    let store = fixture.open();
    let game = fixture.project_file("games/guess.b2c");
    // No record: nothing is written, not even the file.
    assert!(!store.record_save(&identity("prj_game", &game, 1)).unwrap());
    assert!(!fixture.trust_file.exists());
    // A folder record is not a project record.
    store
        .grant_folder(&canonical_path(&fixture.projects.join("games")).unwrap())
        .unwrap();
    let before = fs::read(&fixture.trust_file).unwrap();
    assert!(!store.record_save(&identity("prj_game", &game, 1)).unwrap());
    assert_eq!(fs::read(&fixture.trust_file).unwrap(), before);
    // With a record: the hash is updated and the grant time kept.
    store.grant_project(&identity("prj_game", &game, 1)).unwrap();
    let granted_at = fixture.json()["projects"][0]["grantedAt"].clone();
    assert!(store.record_save(&identity("prj_game", &game, 2)).unwrap());
    let json = fixture.json();
    assert_eq!(json["projects"][0]["rawCodeHashAtGrant"], "02".repeat(32));
    assert_eq!(json["projects"][0]["grantedAt"], granted_at);
    // An unchanged hash writes nothing.
    let before = fs::read(&fixture.trust_file).unwrap();
    assert!(store.record_save(&identity("prj_game", &game, 2)).unwrap());
    assert_eq!(fs::read(&fixture.trust_file).unwrap(), before);
    // Another project ID at the path is not this project's record.
    assert!(!store.record_save(&identity("prj_other", &game, 3)).unwrap());
}

#[test]
fn paths_that_cannot_be_recorded_are_refused_and_nothing_is_written() {
    let fixture = Fixture::new();
    let store = fixture.open();
    let invalid = |result: Result<(), StoreError>| matches!(result, Err(StoreError::Invalid(_)));
    assert!(invalid(store.grant_project(&identity(
        "prj_x",
        Path::new("relative.b2c"),
        1
    ))));
    let dotted = fixture.projects.join("a").join("..").join("x.b2c");
    assert!(invalid(store.grant_project(&identity("prj_x", &dotted, 1))));
    let long = fixture.projects.join("x".repeat(9 * 1024));
    assert!(invalid(store.grant_project(&identity("prj_x", &long, 1))));
    assert!(invalid(store.grant_folder(Path::new("relative"))));
    let root = if cfg!(windows) {
        PathBuf::from(r"C:\")
    } else {
        PathBuf::from("/")
    };
    assert!(invalid(store.grant_folder(&root)));
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        let bad = fixture.projects.join(std::ffi::OsStr::from_bytes(b"\xff.b2c"));
        assert!(matches!(
            store.grant_project(&identity("prj_x", &bad, 1)),
            Err(StoreError::PathNotUnicode)
        ));
        assert!(matches!(
            store.grant_folder(&bad),
            Err(StoreError::PathNotUnicode)
        ));
        assert!(!store.record_save(&identity("prj_x", &bad, 1)).unwrap());
    }
    assert!(!fixture.trust_file.exists());
}

/// A file that cannot be read at all is not "corrupt": it trusts nothing,
/// but a change reports the error instead of replacing it.
#[test]
fn an_unreadable_store_is_never_overwritten() {
    let fixture = Fixture::new();
    let game = fixture.project_file("game.b2c");
    fs::create_dir_all(&fixture.trust_file).unwrap();
    let store = fixture.open();
    assert_eq!(store.problem(), Some(TrustFileProblem::NotAFile));
    assert_eq!(store.evaluate(&identity("prj_game", &game, 1)), NO_RECORD);
    let error = store.grant_project(&identity("prj_game", &game, 1)).unwrap_err();
    assert!(matches!(error, StoreError::Link { .. }), "{error:?}");
    assert!(!error.to_string().contains(fixture.trust_file.to_str().unwrap()));
    assert!(fixture.trust_file.is_dir());
}

/// Several app instances share the file: each change is applied on top of
/// what is on disk, under the lock, and every evaluation reads the file.
#[test]
fn instances_never_undo_each_other() {
    let fixture = Fixture::new();
    let first = fixture.open();
    let second = fixture.open();
    let a = fixture.project_file("a.b2c");
    let b = fixture.project_file("b.b2c");
    first.grant_project(&identity("prj_a", &a, 1)).unwrap();
    second.grant_project(&identity("prj_b", &b, 1)).unwrap();
    for store in [&first, &second, &fixture.open()] {
        assert_eq!(store.evaluate(&identity("prj_a", &a, 1)), TRUSTED_PROJECT);
        assert_eq!(store.evaluate(&identity("prj_b", &b, 1)), TRUSTED_PROJECT);
    }
    // A revocation in one instance holds in the other.
    assert!(
        second
            .revoke_project(&ProjectId::new("prj_a").unwrap(), &a)
            .unwrap()
    );
    assert_eq!(first.evaluate(&identity("prj_a", &a, 1)), NO_RECORD);

    // Many instances granting at once lose nothing.
    let paths: Vec<PathBuf> = (0..32)
        .map(|index| fixture.project_file(&format!("p{index}.b2c")))
        .collect();
    let trust_file = Arc::new(fixture.trust_file.clone());
    let threads: Vec<_> = paths
        .chunks(4)
        .map(|chunk| {
            let chunk = chunk.to_vec();
            let trust_file = Arc::clone(&trust_file);
            thread::spawn(move || {
                let store = TrustStore::open(&trust_file);
                for path in chunk {
                    store.grant_project(&identity("prj_many", &path, 5)).unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let store = fixture.open();
    for path in &paths {
        assert_eq!(
            store.evaluate(&identity("prj_many", path, 5)),
            TRUSTED_PROJECT,
            "{}",
            path.display()
        );
    }
    assert_eq!(fixture.json()["projects"].as_array().unwrap().len(), 33);
}

/// A change waits for another instance's change, and gives up after a
/// while instead of hanging.
#[test]
fn changes_wait_for_the_lock_with_a_time_limit() {
    let fixture = Fixture::new();
    let game = fixture.project_file("game.b2c");
    let store = fixture.open();
    store.grant_project(&identity("prj_game", &game, 1)).unwrap();
    let lock_path = fixture.trust_file.with_file_name("trust.json.lock");
    assert!(lock_path.is_file());

    let held = fs::OpenOptions::new().write(true).open(&lock_path).unwrap();
    held.lock().unwrap();
    let releaser = thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        held.unlock().unwrap();
    });
    let started = Instant::now();
    store.grant_project(&identity("prj_game", &game, 2)).unwrap();
    assert!(started.elapsed() >= Duration::from_millis(250));
    releaser.join().unwrap();
    assert_eq!(store.evaluate(&identity("prj_game", &game, 2)), TRUSTED_PROJECT);

    let held = fs::OpenOptions::new().write(true).open(&lock_path).unwrap();
    held.lock().unwrap();
    let started = Instant::now();
    let error = store.grant_project(&identity("prj_game", &game, 3)).unwrap_err();
    assert!(
        matches!(&error, StoreError::Io { source, .. } if source.kind() == io::ErrorKind::TimedOut),
        "{error:?}"
    );
    assert!(started.elapsed() >= Duration::from_secs(4));
    // Evaluations never wait for the lock.
    assert_eq!(store.evaluate(&identity("prj_game", &game, 2)), TRUSTED_PROJECT);
    held.unlock().unwrap();
}

#[cfg(unix)]
#[test]
fn files_are_owner_only_and_links_are_refused() {
    use std::os::unix::fs::PermissionsExt as _;
    let fixture = Fixture::new();
    let game = fixture.project_file("game.b2c");
    let store = fixture.open();
    store.grant_project(&identity("prj_game", &game, 1)).unwrap();
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&fixture.trust_file), 0o600);
    assert_eq!(mode(&fixture.trust_file.with_file_name("trust.json.lock")), 0o600);
    assert_eq!(mode(fixture.trust_file.parent().unwrap()), 0o700);

    // A link planted at the lock is refused, and so is one at the file.
    let lock = fixture.trust_file.with_file_name("trust.json.lock");
    fs::remove_file(&lock).unwrap();
    let elsewhere = fixture.projects.join("elsewhere");
    fs::write(&elsewhere, b"").unwrap();
    std::os::unix::fs::symlink(&elsewhere, &lock).unwrap();
    assert!(matches!(
        store.grant_project(&identity("prj_game", &game, 2)),
        Err(StoreError::Link { .. })
    ));
    fs::remove_file(&lock).unwrap();
    fs::remove_file(&fixture.trust_file).unwrap();
    let target = fixture.projects.join("target.json");
    fs::write(&target, b"").unwrap();
    std::os::unix::fs::symlink(&target, &fixture.trust_file).unwrap();
    assert!(matches!(
        store.grant_project(&identity("prj_game", &game, 2)),
        Err(StoreError::Link { .. })
    ));
    assert_eq!(fs::read(&target).unwrap(), b"");
}

/// Paths are compared ignoring letter case on Windows (08 §8.3.1).
#[cfg(windows)]
#[test]
fn windows_paths_are_compared_ignoring_case() {
    let fixture = Fixture::new();
    let store = fixture.open();
    let game = fixture.project_file("Games/Guess.b2c");
    store.grant_project(&identity("prj_game", &game, 1)).unwrap();
    let lower = PathBuf::from(game.to_str().unwrap().to_lowercase());
    let upper = PathBuf::from(game.to_str().unwrap().to_uppercase());
    assert_eq!(store.evaluate(&identity("prj_game", &lower, 1)), TRUSTED_PROJECT);
    assert_eq!(store.evaluate(&identity("prj_game", &upper, 1)), TRUSTED_PROJECT);
    // The verbatim form of the same path matches too.
    let verbatim = PathBuf::from(format!(r"\\?\{}", game.to_str().unwrap()));
    assert_eq!(
        store.evaluate(&identity("prj_game", &verbatim, 1)),
        TRUSTED_PROJECT
    );
    // Granting the other spelling replaces the record.
    store.grant_project(&identity("prj_game", &upper, 2)).unwrap();
    assert_eq!(fixture.json()["projects"].as_array().unwrap().len(), 1);
    // Folders too.
    let games = canonical_path(&fixture.projects.join("Games")).unwrap();
    store.grant_folder(&games).unwrap();
    let nested = PathBuf::from(games.to_str().unwrap().to_uppercase()).join("NEW.B2C");
    assert_eq!(store.evaluate(&identity("prj_new", &nested, 1)), TRUSTED_FOLDER);
    assert!(store.folder_covering(&nested).is_some());
    let lower_folder = PathBuf::from(games.to_str().unwrap().to_lowercase());
    store.grant_folder(&lower_folder).unwrap();
    assert_eq!(fixture.json()["folders"].as_array().unwrap().len(), 1);
}
