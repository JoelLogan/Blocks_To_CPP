//! The recovery store through its public API, the way the desktop backend
//! uses it (`docs/spec/05-project-format.md` §5.10): a crash leaves the
//! snapshots of unsaved projects for the next start, running instances never
//! see each other's snapshots, reads are bounded, writes are atomic and
//! owner-only, and stopped instances are tidied up.
// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, SystemTime};

use b2c_ir::ProjectId;
use b2c_store::recovery::{
    MAX_RESTORABLE_SNAPSHOTS, MAX_SNAPSHOT_DOCUMENT_BYTES, MAX_SNAPSHOT_META_BYTES, RECOVERY_FORMAT,
    is_unknown_snapshot,
};
use b2c_store::{Dirs, RecoveryStore, SnapshotMeta, StoreError, canonical_path, rfc3339_utc, sha256_hex};

/// A recovery folder under a temporary root, as the app computes it.
struct Fixture {
    _root: tempfile::TempDir,
    recovery: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let base = canonical_path(root.path()).unwrap();
        Self {
            recovery: Dirs::under_root(&base).recovery,
            _root: root,
        }
    }

    fn open(&self) -> RecoveryStore {
        RecoveryStore::open(&self.recovery).unwrap()
    }

    /// The names in the recovery folder, sorted.
    fn names(&self) -> Vec<String> {
        names(&self.recovery)
    }

    /// The folder of instance `id`.
    fn instance(&self, id: &str) -> PathBuf {
        self.recovery.join(id)
    }
}

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// An absolute project path on this platform.
fn project_path(name: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!(r"C:\Users\Ada\games\{name}"))
    } else {
        PathBuf::from(format!("/home/ada/games/{name}"))
    }
}

fn meta_at(name: &str, path: Option<&Path>, saved: SystemTime) -> SnapshotMeta {
    SnapshotMeta {
        project_id: ProjectId::new("prj_4kq9Xb2LmT7pRz1s").unwrap(),
        project_name: name.to_owned(),
        has_path: path.is_some(),
        bound_path: path.map(Path::to_path_buf),
        saved_at: rfc3339_utc(saved),
        app_version: "0.2.0".to_owned(),
        trusted_at_write: path.is_some(),
        security_hash: [0xab; 32],
    }
}

fn meta(name: &str, path: Option<&Path>) -> SnapshotMeta {
    meta_at(name, path, SystemTime::now())
}

/// The plan's first test: a crash leaves the snapshot, the next start lists
/// and reads it, and after a discard it is gone with its instance.
#[test]
fn a_snapshot_survives_a_crash_and_can_be_restored_then_discarded() {
    let fixture = Fixture::new();
    let path = project_path("guess.b2c");
    let crashed = fixture.open();
    let crashed_id = crashed.instance_id().to_owned();
    let document = br#"{"format":"blocks2cpp/project"}"#;
    let saved = meta("Guessing game", Some(&path));
    let snapshot = crashed.write("ph_1", document, &saved).unwrap();
    let new_project = meta("New project", None);
    let unsaved = crashed.write("ph_2", b"{}", &new_project).unwrap();
    // Simulated crash: the store goes away without deleting its snapshots.
    drop(crashed);
    assert!(fixture.names().contains(&format!("{crashed_id}.lock")));

    let next = fixture.open();
    let listed = next.list_restorable();
    let ids: BTreeSet<&str> = listed
        .iter()
        .map(|listing| listing.snapshot_id.as_str())
        .collect();
    assert_eq!(ids, BTreeSet::from([snapshot.as_str(), unsaved.as_str()]));
    let listing = listed
        .iter()
        .find(|listing| listing.snapshot_id == snapshot)
        .unwrap();
    assert_eq!(listing.meta, saved);

    let (bytes, read_meta) = next.read(&snapshot).unwrap();
    assert_eq!(bytes, document);
    assert_eq!(read_meta, saved);
    let (bytes, read_meta) = next.read(&unsaved).unwrap();
    assert_eq!(bytes, b"{}");
    assert_eq!(read_meta, new_project);
    assert!(!read_meta.has_path);

    // Reading leaves the snapshot in place; discarding removes it.
    assert_eq!(next.list_restorable().len(), 2);
    next.discard(&snapshot).unwrap();
    assert!(is_unknown_snapshot(&next.read(&snapshot).unwrap_err()));
    assert!(is_unknown_snapshot(&next.discard(&snapshot).unwrap_err()));
    assert_eq!(next.list_restorable().len(), 1);
    next.discard(&unsaved).unwrap();
    assert!(next.list_restorable().is_empty());
    // The crashed instance's folder and lock file went with its last snapshot.
    let own = next.instance_id().to_owned();
    assert_eq!(fixture.names(), [own.clone(), format!("{own}.lock")]);
}

#[test]
fn a_clean_save_or_close_deletes_the_snapshot() {
    let fixture = Fixture::new();
    let store = fixture.open();
    let id = store.write("ph_1", b"one", &meta("Guess", None)).unwrap();
    store.write("ph_2", b"two", &meta("Other", None)).unwrap();
    store.delete_for("ph_2").unwrap();
    drop(store);
    let next = fixture.open();
    let listed: Vec<String> = next
        .list_restorable()
        .into_iter()
        .map(|l| l.snapshot_id)
        .collect();
    assert_eq!(listed, [id]);
}

#[test]
fn a_clean_exit_without_snapshots_leaves_nothing_behind() {
    let fixture = Fixture::new();
    let store = fixture.open();
    store.write("ph_1", b"one", &meta("Guess", None)).unwrap();
    store.delete_for("ph_1").unwrap();
    drop(store);
    assert!(fixture.names().is_empty());
}

/// The plan's second test: two running instances (both locks held) never
/// see, read or discard each other's snapshots.
#[test]
fn running_instances_never_see_each_others_snapshots() {
    let fixture = Fixture::new();
    let first = fixture.open();
    let second = fixture.open();
    assert_ne!(first.instance_id(), second.instance_id());
    let a = first.write("ph_1", b"first", &meta("First", None)).unwrap();
    let b = second.write("ph_1", b"second", &meta("Second", None)).unwrap();
    assert_ne!(a, b);
    assert!(first.list_restorable().is_empty());
    assert!(second.list_restorable().is_empty());
    // Not even with the ID: neither the other instance's nor its own.
    for (store, id) in [(&first, &b), (&second, &a), (&first, &a)] {
        assert!(is_unknown_snapshot(&store.read(id).unwrap_err()));
        assert!(is_unknown_snapshot(&store.discard(id).unwrap_err()));
    }
    // Both snapshots are untouched.
    assert!(
        fixture
            .instance(first.instance_id())
            .join(format!("{a}.json"))
            .is_file()
    );
    assert!(
        fixture
            .instance(second.instance_id())
            .join(format!("{b}.json"))
            .is_file()
    );

    // Once one of them has stopped, the other offers its snapshot.
    drop(first);
    let listed = second.list_restorable();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].snapshot_id, a);
    assert_eq!(second.read(&a).unwrap().0, b"first");
}

#[test]
fn unknown_and_malformed_ids_are_unknown_snapshots() {
    let fixture = Fixture::new();
    let store = fixture.open();
    for id in [
        String::new(),
        "sn_".to_owned(),
        format!("sn_{}", "0".repeat(32)),
        format!("sn_{}", "A".repeat(32)),
        "../../etc/passwd".to_owned(),
        format!("rc_{}", "0".repeat(32)),
    ] {
        assert!(is_unknown_snapshot(&store.read(&id).unwrap_err()), "{id}");
        assert!(is_unknown_snapshot(&store.discard(&id).unwrap_err()), "{id}");
    }
}

/// Writes one snapshot in a store that then "crashes"; returns the snapshot
/// ID and the instance folder.
fn crashed_snapshot(fixture: &Fixture, document: &[u8], meta: &SnapshotMeta) -> (String, PathBuf) {
    let store = fixture.open();
    let id = store.write("ph_1", document, meta).unwrap();
    let dir = fixture.instance(store.instance_id());
    drop(store);
    (id, dir)
}

/// The metadata file `json` padded with trailing white space (still valid
/// JSON) to `len` bytes.
fn padded(json: &[u8], len: u64) -> Vec<u8> {
    let mut bytes = json.to_vec();
    bytes.resize(usize::try_from(len).unwrap(), b' ');
    bytes
}

/// The plan's third test: metadata is read up to 64 KiB, documents up to
/// the project limit.
#[test]
fn reads_are_bounded() {
    let fixture = Fixture::new();
    let (id, dir) = crashed_snapshot(&fixture, b"{}", &meta("Guess", None));
    let meta_file = dir.join(format!("{id}.json"));
    let json = fs::read(&meta_file).unwrap();

    // Metadata of exactly the limit is read, one byte more is not.
    fs::write(&meta_file, padded(&json, MAX_SNAPSHOT_META_BYTES)).unwrap();
    let store = fixture.open();
    assert_eq!(store.list_restorable().len(), 1);
    assert_eq!(store.read(&id).unwrap().0, b"{}");
    fs::write(&meta_file, padded(&json, MAX_SNAPSHOT_META_BYTES + 1)).unwrap();
    assert!(store.list_restorable().is_empty());
    assert!(matches!(store.read(&id), Err(StoreError::Invalid(_))));

    // A document over the limit is neither listed nor read (the file is
    // sparse: it is refused by its size, without being loaded).
    fs::write(&meta_file, &json).unwrap();
    let document = dir.join(format!("{id}.b2c"));
    fs::File::options()
        .write(true)
        .open(&document)
        .unwrap()
        .set_len(MAX_SNAPSHOT_DOCUMENT_BYTES + 1)
        .unwrap();
    assert!(store.list_restorable().is_empty());
    assert!(matches!(store.read(&id), Err(StoreError::Invalid(_))));
    // It can still be discarded.
    store.discard(&id).unwrap();
    assert!(!dir.exists());
}

#[test]
fn the_largest_document_is_written_and_read_back() {
    let fixture = Fixture::new();
    let size = usize::try_from(MAX_SNAPSHOT_DOCUMENT_BYTES).unwrap();
    let document: Vec<u8> = (0..size).map(|index| b"{}[] \n"[index % 6]).collect();
    let (id, _) = crashed_snapshot(&fixture, &document, &meta("Big", None));
    let (bytes, _) = fixture.open().read(&id).unwrap();
    // Compared by hash: a failure must not print 32 MiB.
    assert_eq!(sha256_hex(&bytes), sha256_hex(&document));
    let store = fixture.open();
    let mut larger = document;
    larger.push(b' ');
    assert!(matches!(
        store.write("ph_big", &larger, &meta("Bigger", None)),
        Err(StoreError::Invalid(_))
    ));
}

#[test]
fn invalid_and_newer_metadata_is_not_offered_and_left_alone() {
    let fixture = Fixture::new();
    let (id, dir) = crashed_snapshot(&fixture, b"{}", &meta("Guess", None));
    let meta_file = dir.join(format!("{id}.json"));
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&meta_file).unwrap()).unwrap();
    assert_eq!(value["format"], RECOVERY_FORMAT);
    assert_eq!(value["formatVersion"], 1);

    value["formatVersion"] = 2.into();
    fs::write(&meta_file, serde_json::to_vec(&value).unwrap()).unwrap();
    let store = fixture.open();
    assert!(store.list_restorable().is_empty());
    let error = store.read(&id).unwrap_err();
    assert!(matches!(error, StoreError::Invalid(_)), "{error:?}");
    // A newer version may restore it: nothing was removed.
    assert_eq!(names(&dir), [format!("{id}.b2c"), format!("{id}.json")]);

    value["formatVersion"] = 1.into();
    value["projectId"] = "<script>".into();
    fs::write(&meta_file, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(store.list_restorable().is_empty());
    assert!(matches!(store.read(&id), Err(StoreError::Invalid(_))));
    assert_eq!(names(&dir).len(), 2);
}

#[test]
fn a_document_that_does_not_match_its_metadata_is_never_returned() {
    let fixture = Fixture::new();
    let (id, dir) = crashed_snapshot(&fixture, b"original", &meta("Guess", None));
    // Another document put in its place, as a crash between the two writes
    // without the previous copy would leave it.
    fs::write(dir.join(format!("{id}.b2c")), b"replacement").unwrap();
    let store = fixture.open();
    assert!(matches!(store.read(&id), Err(StoreError::Invalid(_))));
    // With the matching previous document next to it, that one is returned.
    fs::write(dir.join(format!("{id}.prev.b2c")), b"original").unwrap();
    assert_eq!(store.read(&id).unwrap().0, b"original");
}

/// The plan's fourth test: writes replace the snapshot atomically, keep its
/// ID, sync the folder and leave no temporary files.
#[test]
fn writes_are_atomic_and_keep_the_snapshot_id() {
    let fixture = Fixture::new();
    let store = fixture.open();
    let dir = fixture.instance(store.instance_id());
    let first = store.write("ph_1", b"v1", &meta("Guess", None)).unwrap();
    let other = store.write("ph_2", b"w1", &meta("Other", None)).unwrap();
    assert_ne!(first, other);
    for version in 2..=5 {
        let document = format!("v{version}");
        #[cfg(unix)]
        let syncs = b2c_process::os::directory_syncs();
        let id = store
            .write(
                "ph_1",
                document.as_bytes(),
                &meta(&format!("Guess {version}"), None),
            )
            .unwrap();
        assert_eq!(id, first);
        #[cfg(unix)]
        assert!(b2c_process::os::directory_syncs() > syncs);
        assert_eq!(
            fs::read(dir.join(format!("{id}.b2c"))).unwrap(),
            document.as_bytes()
        );
    }
    // Exactly the two pairs: no temporary file, no previous copy.
    let expected: BTreeSet<String> = [&first, &other]
        .iter()
        .flat_map(|id| [format!("{id}.b2c"), format!("{id}.json")])
        .collect();
    assert_eq!(names(&dir).into_iter().collect::<BTreeSet<_>>(), expected);
    drop(store);
    let next = fixture.open();
    let (bytes, read_meta) = next.read(&first).unwrap();
    assert_eq!(bytes, b"v5");
    assert_eq!(read_meta.project_name, "Guess 5");
}

/// The plan's fifth test: the recovery and instance folders are `0700`, the
/// lock and snapshot files `0600`.
#[cfg(unix)]
#[test]
fn folders_are_0700_and_files_0600() {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = |path: &Path| fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777;
    let fixture = Fixture::new();
    // A recovery folder that already exists with wider permissions is
    // made private.
    fs::create_dir_all(&fixture.recovery).unwrap();
    fs::set_permissions(&fixture.recovery, fs::Permissions::from_mode(0o755)).unwrap();
    let store = fixture.open();
    assert_eq!(mode(&fixture.recovery), 0o700);
    let instance = store.instance_id().to_owned();
    assert_eq!(mode(&fixture.instance(&instance)), 0o700);
    assert_eq!(mode(&fixture.recovery.join(format!("{instance}.lock"))), 0o600);
    let id = store.write("ph_1", b"one", &meta("Guess", None)).unwrap();
    store.write("ph_1", b"two", &meta("Guess", None)).unwrap();
    for name in [format!("{id}.b2c"), format!("{id}.json")] {
        assert_eq!(mode(&fixture.instance(&instance).join(&name)), 0o600, "{name}");
    }
    // A lock file a lister creates for a folder without one is 0600 too.
    drop(store);
    fs::remove_file(fixture.recovery.join(format!("{instance}.lock"))).unwrap();
    let next = fixture.open();
    assert_eq!(next.list_restorable().len(), 1);
    assert_eq!(mode(&fixture.recovery.join(format!("{instance}.lock"))), 0o600);
}

#[test]
fn stopped_instances_are_tidied_up() {
    let fixture = Fixture::new();
    // A stopped instance with a snapshot, a temporary file of an interrupted
    // write and a document without metadata.
    let (id, dir) = crashed_snapshot(&fixture, b"{}", &meta("Guess", None));
    let orphan = format!("sn_{}", "1".repeat(32));
    fs::write(dir.join(".b2c-a1B2c3.tmp"), b"half").unwrap();
    fs::write(dir.join(format!("{orphan}.b2c")), b"no metadata").unwrap();
    fs::write(dir.join(format!("{orphan}.prev.b2c")), b"no metadata").unwrap();
    fs::write(dir.join("notes.txt"), b"not ours").unwrap();
    // An empty stopped instance, and a lock file without a folder.
    let empty = "2".repeat(32);
    fs::create_dir(fixture.instance(&empty)).unwrap();
    fs::write(fixture.recovery.join(format!("{empty}.lock")), b"").unwrap();
    let lone_lock = format!("{}.lock", "3".repeat(32));
    fs::write(fixture.recovery.join(&lone_lock), b"").unwrap();
    // Things that are not instances at all.
    fs::create_dir(fixture.recovery.join("not-an-instance")).unwrap();
    fs::write(fixture.recovery.join(format!("{}.lock", "X".repeat(32))), b"").unwrap();

    let store = fixture.open();
    let listed = store.list_restorable();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].snapshot_id, id);
    assert_eq!(
        names(&dir),
        ["notes.txt".to_owned(), format!("{id}.b2c"), format!("{id}.json")]
    );
    let own = store.instance_id().to_owned();
    let crashed = dir.file_name().unwrap().to_str().unwrap().to_owned();
    let mut expected = vec![
        own.clone(),
        format!("{own}.lock"),
        crashed.clone(),
        format!("{crashed}.lock"),
        "not-an-instance".to_owned(),
        format!("{}.lock", "X".repeat(32)),
    ];
    expected.sort();
    assert_eq!(fixture.names(), expected);
    // After the discard, the folder keeps the file that is not ours.
    store.discard(&id).unwrap();
    assert_eq!(names(&dir), ["notes.txt"]);
}

#[test]
fn snapshots_are_listed_newest_first_and_capped() {
    let fixture = Fixture::new();
    let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000);
    let mut expected = Vec::new();
    // More than the cap, spread over several stopped instances.
    let total = MAX_RESTORABLE_SNAPSHOTS + 10;
    for chunk in 0..total.div_ceil(50) {
        let store = fixture.open();
        for index in chunk * 50..total.min((chunk + 1) * 50) {
            let seconds = u64::try_from(index).unwrap();
            let saved = start + Duration::from_secs(seconds);
            let id = store
                .write(
                    &format!("ph_{index}"),
                    b"{}",
                    &meta_at(&format!("P{index}"), None, saved),
                )
                .unwrap();
            expected.push((saved, id));
        }
    }
    expected.sort_by_key(|(saved, _)| std::cmp::Reverse(*saved));
    let listed = fixture.open().list_restorable();
    assert_eq!(listed.len(), MAX_RESTORABLE_SNAPSHOTS);
    let ids: Vec<&str> = listed
        .iter()
        .map(|listing| listing.snapshot_id.as_str())
        .collect();
    let newest: Vec<&str> = expected
        .iter()
        .take(MAX_RESTORABLE_SNAPSHOTS)
        .map(|(_, id)| id.as_str())
        .collect();
    assert_eq!(ids, newest);
}

/// Several new instances listing and discarding the same stopped instance at
/// once: a lister sees an instance whole or not at all, and a snapshot is
/// discarded exactly once.
#[test]
fn concurrent_instances_share_a_stopped_instance_safely() {
    let fixture = Fixture::new();
    let crashed = fixture.open();
    let mut ids = BTreeSet::new();
    for index in 0..8 {
        ids.insert(
            crashed
                .write(&format!("ph_{index}"), b"{}", &meta("Guess", None))
                .unwrap(),
        );
    }
    drop(crashed);
    let ids = Arc::new(ids);
    let stores: Vec<Arc<RecoveryStore>> = (0..4).map(|_| Arc::new(fixture.open())).collect();
    let barrier = Arc::new(Barrier::new(stores.len()));
    let handles: Vec<_> = stores
        .iter()
        .map(|store| {
            let (store, ids, barrier) = (Arc::clone(store), Arc::clone(&ids), Arc::clone(&barrier));
            thread::spawn(move || {
                barrier.wait();
                for _ in 0..20 {
                    let listed: BTreeSet<String> = store
                        .list_restorable()
                        .into_iter()
                        .map(|l| l.snapshot_id)
                        .collect();
                    assert!(listed.is_empty() || listed == *ids, "{listed:?}");
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    // Every store discards every snapshot; each one goes exactly once.
    let barrier = Arc::new(Barrier::new(stores.len()));
    let handles: Vec<_> = stores
        .iter()
        .map(|store| {
            let (store, ids, barrier) = (Arc::clone(store), Arc::clone(&ids), Arc::clone(&barrier));
            thread::spawn(move || {
                barrier.wait();
                let mut discarded = 0;
                for id in ids.iter() {
                    match store.discard(id) {
                        Ok(()) => discarded += 1,
                        Err(error) => assert!(is_unknown_snapshot(&error), "{error:?}"),
                    }
                }
                discarded
            })
        })
        .collect();
    let discarded: usize = handles.into_iter().map(|handle| handle.join().unwrap()).sum();
    assert_eq!(discarded, ids.len());
    assert!(stores[0].list_restorable().is_empty());
}

#[cfg(unix)]
#[test]
fn links_are_never_followed() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let (id, dir) = crashed_snapshot(&fixture, b"{}", &meta("Guess", None));
    let outside = fixture.recovery.parent().unwrap().join("outside");
    fs::create_dir(&outside).unwrap();
    // A snapshot file replaced by a link to a file elsewhere is not read.
    let document = dir.join(format!("{id}.b2c"));
    fs::rename(&document, outside.join("doc.b2c")).unwrap();
    symlink(outside.join("doc.b2c"), &document).unwrap();
    let store = fixture.open();
    assert!(store.list_restorable().is_empty());
    assert!(matches!(store.read(&id), Err(StoreError::Link { .. })));
    // A link named like an instance folder is not an instance.
    let linked = "4".repeat(32);
    symlink(&outside, fixture.instance(&linked)).unwrap();
    fs::write(outside.join(format!("{id}.json")), b"{}").unwrap();
    assert!(store.list_restorable().is_empty());
    assert!(fixture.instance(&linked).exists());
    // Discarding removes the link, not the file it points to.
    store.discard(&id).unwrap();
    assert!(outside.join("doc.b2c").is_file());
    // A recovery folder that is a link is refused.
    let linked_recovery = fixture.recovery.parent().unwrap().join("linked-recovery");
    symlink(&outside, &linked_recovery).unwrap();
    assert!(matches!(
        RecoveryStore::open(&linked_recovery),
        Err(StoreError::Link { .. })
    ));
}

#[test]
fn a_bound_path_that_cannot_be_recorded_restores_without_a_path() {
    let fixture = Fixture::new();
    let long = project_path(&"x".repeat(b2c_store::recovery::MAX_BOUND_PATH_BYTES));
    let (id, _) = crashed_snapshot(&fixture, b"{}", &meta("Guess", Some(&long)));
    let (_, read_meta) = fixture.open().read(&id).unwrap();
    assert!(read_meta.has_path);
    assert_eq!(read_meta.bound_path, None);
}

#[test]
fn snapshot_content_never_appears_in_errors() {
    let fixture = Fixture::new();
    let store = fixture.open();
    let secret = "TOP-SECRET-PROJECT-CONTENT";
    let mut bad = meta(secret, Some(Path::new("relative/secret.b2c")));
    bad.app_version = secret.to_owned();
    let errors = [
        store.write(secret, secret.as_bytes(), &bad).unwrap_err(),
        store.read(secret).unwrap_err(),
    ];
    for error in errors {
        let mut next: Option<&dyn std::error::Error> = Some(&error);
        while let Some(current) = next {
            let text = current.to_string();
            assert!(!text.contains("SECRET") && !text.contains("secret"), "{text}");
            next = current.source();
        }
    }
}
