//! Recovery snapshots: autosaved copies of projects with unsaved changes,
//! kept per app instance in the recovery folder and offered for restore
//! after a crash (`docs/spec/05-project-format.md` §5.10,
//! `docs/spec/02-architecture.md` §2.7, `docs/spec/01-overview.md` N10).
//!
//! ```text
//! <recovery>/
//! ├── <instanceId>.lock            held exclusively by that instance while it runs
//! └── <instanceId>/                instanceId: 32 lower-case hex digits, new for each start
//!     ├── <snapshotId>.b2c         the document, as the editor last sent it
//!     ├── <snapshotId>.json        its metadata
//!     └── <snapshotId>.prev.b2c    the previous document, only while a new one is written
//! ```
//!
//! The metadata (read limit 64 KiB, [`MAX_SNAPSHOT_META_BYTES`]):
//!
//! ```json
//! {
//!   "format": "blocks2cpp/recovery",
//!   "formatVersion": 1,
//!   "projectId": "prj_4kq9Xb2LmT7pRz1s",
//!   "projectName": "Guessing game",
//!   "hasPath": true,
//!   "boundPath": "/home/ada/games/guess.b2c",
//!   "savedAt": "2026-10-05T09:30:00.000Z",
//!   "appVersion": "0.2.0",
//!   "trustedAtWrite": true,
//!   "securityHash": "<64 lower-case hex digits>",
//!   "documentHash": "<64 lower-case hex digits>"
//! }
//! ```
//!
//! CONTRACT (milestone M2):
//! * **Instances.** [`RecoveryStore::open`] picks a fresh instance ID from
//!   the OS random number generator, creates `<instanceId>.lock` exclusively
//!   (`0600`), takes an exclusive lock on it, and only then creates the
//!   private (`0700`) instance folder. The lock is held for the store's
//!   whole lifetime, so the operating system releases it when the app exits
//!   or crashes. Several app instances can share one recovery folder: each
//!   writes only into its own instance folder, and the snapshots of an
//!   instance are offered for restore ([`RecoveryStore::list_restorable`])
//!   only while its lock is free, that is, after it exited or crashed. The
//!   lock file is never read (Windows locks are mandatory).
//! * **One snapshot per key.** [`RecoveryStore::write`] keeps one snapshot
//!   per key (the project's handle). Its ID, `sn_` + 32 hex digits, stays the
//!   same until [`RecoveryStore::delete_for`] removes it (on a clean save or
//!   close). The document and its metadata are written atomically
//!   ([`write_atomic`], `0600`). They form a pair through `documentHash`, the
//!   SHA-256 of the document: before a document is replaced, the current one
//!   is renamed to `<snapshotId>.prev.b2c`, and the new metadata goes in last.
//!   A crash at any point therefore leaves either the old pair or the new one
//!   restorable, and [`RecoveryStore::read`] never returns a document with
//!   metadata that was written for another one (which could carry another
//!   project's trust facts).
//! * **Restore.** [`RecoveryStore::list_restorable`] lists the snapshots of
//!   the other instances whose lock it can take, newest first, at most
//!   [`MAX_RESTORABLE_SNAPSHOTS`]. While it holds such a lock it also tidies
//!   up: temporary files and documents without metadata are removed, and so
//!   are instance folders left empty, with their lock files, and lock files
//!   without a folder. Snapshots whose metadata is invalid or from a newer
//!   version are not listed and are left alone. [`RecoveryStore::read`] and
//!   [`RecoveryStore::discard`] take the instance lock again (waiting at most
//!   1 s for another instance that is listing), so neither ever touches the
//!   snapshots of a running instance. This store does not restore anything
//!   itself: the app reads a snapshot, opens it as a project, writes the
//!   restored project's own snapshot under its new handle and then discards
//!   the old one.
//! * **Bounded and validated.** Documents are at most the project limit of
//!   05 §5.6 ([`MAX_SNAPSHOT_DOCUMENT_BYTES`], 32 MiB) and metadata at most
//!   64 KiB, read with [`read_bounded`] (at most one byte more). Metadata is
//!   validated strictly (format tag, version, no unknown or duplicate keys,
//!   project ID, absolute bound path, RFC 3339 time, version text, hashes).
//!   Snapshot files are read only when they are regular files, never through
//!   a link. An instance keeps at most [`MAX_SNAPSHOTS_PER_INSTANCE`]
//!   snapshots, directory scans stop after 10,000 entries, and one listing
//!   reads at most 1,000 metadata files.
//! * **Unrecordable paths.** A bound path that is not valid Unicode, or is
//!   longer than [`MAX_BOUND_PATH_BYTES`], cannot be written to JSON. Such a
//!   snapshot is still written, with `hasPath: true` and `boundPath: null`,
//!   so it restores without a path and is never mistaken for a project that
//!   was never saved (which the app trusts as *created here*).
//! * **Private.** Nothing here logs, and no error message contains a path
//!   (08 §8.11): project content goes only into the snapshot files.
//!
//! Errors are [`StoreError`]s. A snapshot ID that is malformed, unknown, or
//! belongs to this instance or to a running one is an [`StoreError::Io`]
//! with kind [`io::ErrorKind::NotFound`] ([`is_unknown_snapshot`]), which
//! the app reports as `unknownSnapshot`.

use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File, Metadata, OpenOptions, TryLockError};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use b2c_ir::ProjectId;
use b2c_model::limits::MAX_FILE_BYTES;
use serde::{Deserialize, Deserializer, Serialize};

use crate::atomic::{Backup, write_atomic};
use crate::dirs::ensure_private_dir;
use crate::error::{ReadError, StoreError};
use crate::fs_checks::{is_link, is_link_or_reparse_point, same_file};
use crate::ids::{hex_lower, is_hex_id, is_lower_hex, random_hex_id};
use crate::project_file::{MAX_PROJECT_BYTES, sha256};
use crate::read::read_bounded;
use crate::time::parse_rfc3339_utc;

/// The `format` value of a snapshot's metadata.
pub const RECOVERY_FORMAT: &str = "blocks2cpp/recovery";
/// The `formatVersion` of a snapshot's metadata this version reads and
/// writes.
pub const RECOVERY_FORMAT_VERSION: u64 = 1;
/// The prefix of snapshot IDs (`sn_` + 32 lower-case hex digits).
pub const SNAPSHOT_ID_PREFIX: &str = "sn_";
/// The largest snapshot document, in bytes: the project limit of 05 §5.6
/// (32 MiB).
pub const MAX_SNAPSHOT_DOCUMENT_BYTES: u64 = MAX_PROJECT_BYTES;
/// The largest metadata file that is read, in bytes (64 KiB). Writes never
/// go over it.
pub const MAX_SNAPSHOT_META_BYTES: u64 = 64 * 1024;
/// The longest project name recorded, in bytes of UTF-8; longer names are
/// shortened at a character boundary.
pub const MAX_SNAPSHOT_NAME_BYTES: usize = 1024;
/// The longest bound path recorded, in bytes of UTF-8.
pub const MAX_BOUND_PATH_BYTES: usize = 8 * 1024;
/// The longest `appVersion`, in bytes.
pub const MAX_APP_VERSION_BYTES: usize = 64;
/// The longest key [`RecoveryStore::write`] accepts, in bytes.
pub const MAX_SNAPSHOT_KEY_BYTES: usize = 256;
/// The most snapshots one instance keeps at a time (twice the app's limit
/// of open projects).
pub const MAX_SNAPSHOTS_PER_INSTANCE: usize = 64;
/// The most snapshots [`RecoveryStore::list_restorable`] returns.
pub const MAX_RESTORABLE_SNAPSHOTS: usize = 100;

/// How many directory entries one scan looks at, at most.
const MAX_SCANNED_ENTRIES: usize = 10_000;
/// How many snapshots' metadata one listing reads, at most (64 MiB at the
/// metadata limit). Far more than the app ever leaves behind; the rest is
/// listed once some have been restored or discarded.
const MAX_EXAMINED_SNAPSHOTS: usize = 1_000;
/// How long [`RecoveryStore::read`] and [`RecoveryStore::discard`] wait for
/// another instance that holds a stopped instance's lock while listing it
/// (which takes milliseconds). A running instance holds its lock for good, so
/// asking for one of its snapshots costs this much before `NotFound`; the app
/// never does that, because it only asks for listed snapshots.
const FOREIGN_LOCK_WAIT: Duration = Duration::from_secs(1);
/// How long a wait for a lock sleeps between two attempts.
const LOCK_RETRY: Duration = Duration::from_millis(10);
/// How many instance IDs [`RecoveryStore::open`] tries.
const OPEN_ATTEMPTS: usize = 4;
/// The number of hex digits of an instance ID.
const INSTANCE_ID_DIGITS: usize = 32;
/// The suffix of an instance's lock file.
const LOCK_SUFFIX: &str = ".lock";
/// The suffix of a snapshot's metadata.
const META_SUFFIX: &str = ".json";
/// The suffix of a snapshot's document.
const DOCUMENT_SUFFIX: &str = ".b2c";
/// The suffix of a snapshot's previous document.
const PREVIOUS_SUFFIX: &str = ".prev.b2c";
/// The prefix and suffix of [`write_atomic`]'s temporary files.
const TEMP_PREFIX: &str = ".b2c-";
const TEMP_SUFFIX: &str = ".tmp";

/// What a snapshot records about its project, besides the document.
///
/// On disk this is the metadata file shown in the module documentation;
/// `format`, `formatVersion` and `documentHash` are added by the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotMeta {
    /// The project's ID (`project.id` in the document).
    pub project_id: ProjectId,
    /// The project's name, for the restore offer. At most
    /// [`MAX_SNAPSHOT_NAME_BYTES`] are recorded.
    pub project_name: String,
    /// Whether the project was bound to a file when the snapshot was
    /// written. `false` means a project that was never saved.
    pub has_path: bool,
    /// The canonical path of the project's file; `None` for a project never
    /// saved, and also when [`SnapshotMeta::has_path`] is `true` but the path
    /// could not be recorded (not valid Unicode, or longer than
    /// [`MAX_BOUND_PATH_BYTES`]): such a snapshot restores without a path,
    /// in Restricted Mode.
    pub bound_path: Option<PathBuf>,
    /// When the snapshot was written: an RFC 3339 UTC timestamp
    /// (`crate::rfc3339_utc`).
    pub saved_at: String,
    /// The version of the app that wrote it (SemVer text, at most
    /// [`MAX_APP_VERSION_BYTES`]).
    pub app_version: String,
    /// Whether the project was trusted when the snapshot was written
    /// (08 §8.3).
    pub trusted_at_write: bool,
    /// The document's security hash (`b2c_model::security_hash`).
    pub security_hash: [u8; 32],
}

/// A snapshot offered for restore by [`RecoveryStore::list_restorable`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotListing {
    /// The snapshot's ID, for [`RecoveryStore::read`] and
    /// [`RecoveryStore::discard`].
    pub snapshot_id: String,
    /// Its metadata.
    pub meta: SnapshotMeta,
}

/// Whether `error` says that a snapshot ID names no snapshot that may be
/// read or discarded: malformed, unknown, gone, or a snapshot of this
/// instance or of a running one. The app reports it as `unknownSnapshot`.
pub fn is_unknown_snapshot(error: &StoreError) -> bool {
    matches!(error, StoreError::Io { source, .. } if source.kind() == io::ErrorKind::NotFound)
}

/// The recovery store of one app instance (see the module documentation).
///
/// Dropping it releases the instance lock. When the instance folder is
/// empty at that point it is removed, together with the lock file;
/// snapshots still in it (projects with unsaved changes when the app ended)
/// stay, and the next start offers them for restore. Dropping without
/// deleting snapshots is therefore exactly what a crash leaves behind.
#[derive(Debug)]
pub struct RecoveryStore {
    /// The recovery folder.
    dir: PathBuf,
    /// This instance's ID.
    instance_id: String,
    /// `<dir>/<instance_id>`.
    instance_dir: PathBuf,
    /// The lock on `<dir>/<instance_id>.lock`, held for the store's lifetime.
    lock: InstanceLock,
    /// This instance's snapshots by key.
    snapshots: Mutex<HashMap<String, Slot>>,
    /// Serialises the work on other instances' folders (listing, reading,
    /// discarding), so this store never contends with itself for their
    /// locks.
    foreign: Mutex<()>,
}

/// One of this instance's snapshots.
#[derive(Debug)]
struct Slot {
    /// The snapshot ID.
    id: String,
    /// Which document the metadata on disk belongs to.
    state: SlotState,
}

/// Which document of a snapshot its metadata on disk belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotState {
    /// The last write finished: the metadata belongs to `<id>.b2c`.
    Complete,
    /// No write finished since the last rename (or none was made yet): the
    /// metadata, if there is any, belongs to `<id>.prev.b2c`, and `<id>.b2c`
    /// may hold anything.
    Incomplete,
}

/// The steps of [`RecoveryStore::write`] at which a test hook can inject a
/// fault (or a crash).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteStep {
    /// The current document was renamed to `<id>.prev.b2c`.
    Rotated,
    /// The new document is in place; its metadata is not yet.
    DocumentWritten,
}

impl RecoveryStore {
    /// Opens a new instance in `recovery_dir` (normally `Dirs::recovery`),
    /// creating that folder privately (`0700`) if needed. The instance gets a
    /// fresh ID, its lock file `<instanceId>.lock` (`0600`, locked
    /// exclusively until the store is dropped) and its folder
    /// `<instanceId>/` (`0700`).
    ///
    /// # Errors
    /// As [`ensure_private_dir`] for the recovery folder;
    /// [`StoreError::Random`] when the OS random number generator fails;
    /// [`StoreError::Io`] when the lock file or the instance folder cannot
    /// be created or locked.
    pub fn open(recovery_dir: &Path) -> Result<Self, StoreError> {
        ensure_private_dir(recovery_dir)?;
        for _ in 0..OPEN_ATTEMPTS {
            let instance_id = random_hex_id("")?;
            let Some(lock) = InstanceLock::create(&recovery_dir.join(lock_name(&instance_id)))? else {
                continue;
            };
            let instance_dir = recovery_dir.join(&instance_id);
            match create_instance_dir(&instance_dir) {
                Ok(true) => {
                    return Ok(Self {
                        dir: recovery_dir.to_path_buf(),
                        instance_id,
                        instance_dir,
                        lock,
                        snapshots: Mutex::new(HashMap::new()),
                        foreign: Mutex::new(()),
                    });
                }
                // A folder of that name already exists: try another ID.
                Ok(false) => lock.remove(),
                Err(error) => {
                    lock.remove();
                    return Err(error);
                }
            }
        }
        Err(StoreError::io(
            "claim a recovery instance",
            recovery_dir,
            io::ErrorKind::AlreadyExists.into(),
        ))
    }

    /// The recovery folder (for the debug log).
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// This instance's ID (32 lower-case hex digits).
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    /// Writes the snapshot for `key` (the project's handle): `document`, the
    /// project text as the editor sent it, and `meta`. Returns the snapshot
    /// ID, which stays the same for `key` until [`RecoveryStore::delete_for`].
    ///
    /// The previous snapshot for `key` is replaced as a whole: a crash or an
    /// error at any point leaves the old snapshot or the new one restorable
    /// (see the module documentation). `meta.project_name` is shortened to
    /// [`MAX_SNAPSHOT_NAME_BYTES`]; a bound path that cannot be recorded is
    /// written as `null` (see [`SnapshotMeta::bound_path`]).
    ///
    /// # Errors
    /// [`StoreError::Invalid`] for an empty key or one longer than
    /// [`MAX_SNAPSHOT_KEY_BYTES`], a document larger than
    /// [`MAX_SNAPSHOT_DOCUMENT_BYTES`], a new key beyond
    /// [`MAX_SNAPSHOTS_PER_INSTANCE`], and invalid metadata (a bound path
    /// without `has_path`, a relative bound path or one with `.` or `..`
    /// parts, a `saved_at` that is not RFC 3339, an `app_version` that is
    /// empty, too long or not SemVer text); otherwise the errors of
    /// [`ensure_private_dir`] and [`write_atomic`].
    pub fn write(&self, key: &str, document: &[u8], meta: &SnapshotMeta) -> Result<String, StoreError> {
        self.write_with_hook(key, document, meta, &mut |_| Ok(()))
    }

    /// [`RecoveryStore::write`] with a hook called after each step of
    /// [`WriteStep`]; an error from it aborts the write there, as a failed
    /// step would, so tests can inject faults and crashes.
    pub(crate) fn write_with_hook(
        &self,
        key: &str,
        document: &[u8],
        meta: &SnapshotMeta,
        hook: &mut dyn FnMut(WriteStep) -> io::Result<()>,
    ) -> Result<String, StoreError> {
        check_key(key)?;
        if document.len() > MAX_FILE_BYTES {
            return Err(StoreError::Invalid(
                "the snapshot's document is larger than 32 MiB, the most a project can be",
            ));
        }
        let meta_bytes = encode_meta(meta, &sha256(document))?;
        let mut snapshots = self.lock_snapshots();
        if !snapshots.contains_key(key) {
            if snapshots.len() >= MAX_SNAPSHOTS_PER_INSTANCE {
                return Err(StoreError::Invalid(
                    "this app instance already keeps the most recovery snapshots it may",
                ));
            }
            let id = random_hex_id(SNAPSHOT_ID_PREFIX)?;
            snapshots.insert(
                key.to_owned(),
                Slot {
                    id,
                    state: SlotState::Incomplete,
                },
            );
        }
        let slot = snapshots
            .get_mut(key)
            .ok_or(StoreError::Invalid("the snapshot's record disappeared"))?;
        ensure_private_dir(&self.instance_dir)?;
        let files = SnapshotFiles::new(&self.instance_dir, &slot.id);
        if slot.state == SlotState::Complete && regular_file(&files.document).is_some() {
            // Keep the document the metadata belongs to until new metadata
            // is in place.
            let rotated = b2c_process::os::atomic_replace(&files.document, &files.previous);
            // Unless the rename itself failed (only the folder sync can fail
            // after it), the metadata now belongs to the previous document.
            if rotated.is_ok() || regular_file(&files.document).is_none() {
                slot.state = SlotState::Incomplete;
            }
            rotated
                .map_err(|source| StoreError::io("keep the previous snapshot", &files.previous, source))?;
            hook(WriteStep::Rotated)
                .map_err(|source| StoreError::io("write the snapshot", &files.document, source))?;
        }
        write_atomic(&files.document, document, Backup::None)?;
        hook(WriteStep::DocumentWritten)
            .map_err(|source| StoreError::io("write the snapshot", &files.meta, source))?;
        write_atomic(&files.meta, &meta_bytes, Backup::None)?;
        slot.state = SlotState::Complete;
        // Only a leftover now: the metadata no longer belongs to it.
        let _ = remove_if_present(&files.previous);
        Ok(slot.id.clone())
    }

    /// Deletes the snapshot for `key`, if there is one (after a clean save or
    /// close). The metadata goes first, so the snapshot stops being
    /// restorable before its document is removed. A later
    /// [`RecoveryStore::write`] for `key` starts a new snapshot with a new ID.
    ///
    /// # Errors
    /// [`StoreError::Io`] when a file cannot be removed; the snapshot then
    /// keeps its ID, and calling again finishes the deletion.
    pub fn delete_for(&self, key: &str) -> Result<(), StoreError> {
        let mut snapshots = self.lock_snapshots();
        let Some(slot) = snapshots.get_mut(key) else {
            return Ok(());
        };
        let files = SnapshotFiles::new(&self.instance_dir, &slot.id);
        remove_if_present(&files.meta)?;
        // Without metadata, whatever documents remain are leftovers.
        slot.state = SlotState::Incomplete;
        remove_if_present(&files.document)?;
        remove_if_present(&files.previous)?;
        snapshots.remove(key);
        Ok(())
    }

    /// The snapshots offered for restore: those of the other instances in
    /// the recovery folder whose lock is free (instances that exited or
    /// crashed), with valid metadata and a document of at most
    /// [`MAX_SNAPSHOT_DOCUMENT_BYTES`]; newest first (by `savedAt`, then by
    /// ID), at most [`MAX_RESTORABLE_SNAPSHOTS`], each ID once.
    ///
    /// While it holds a stopped instance's lock it also tidies that instance
    /// up (see the module documentation): empty instance folders and their
    /// lock files, and lock files without a folder, are removed. Problems
    /// with one instance (an unreadable folder, a lock that cannot be taken)
    /// only leave that instance out; this method never fails.
    pub fn list_restorable(&self) -> Vec<SnapshotListing> {
        let _foreign = self.lock_foreign();
        let mut found = Vec::new();
        let mut budget = MAX_EXAMINED_SNAPSHOTS;
        for id in self.other_instances() {
            if budget == 0 {
                break;
            }
            self.collect_instance(&id, &mut found, &mut budget);
        }
        // Newest first; equal times in ID order, so the result is stable.
        found.sort_by(|a, b| b.saved.cmp(&a.saved).then_with(|| a.id.cmp(&b.id)));
        let mut seen = BTreeSet::new();
        found
            .into_iter()
            .filter(|snapshot| seen.insert(snapshot.id.clone()))
            .take(MAX_RESTORABLE_SNAPSHOTS)
            .map(|snapshot| SnapshotListing {
                snapshot_id: snapshot.id,
                meta: snapshot.meta,
            })
            .collect()
    }

    /// Reads a snapshot of a stopped instance: its document (at most
    /// [`MAX_SNAPSHOT_DOCUMENT_BYTES`], reading at most one byte more) and
    /// its metadata (at most [`MAX_SNAPSHOT_META_BYTES`]). The document is
    /// the one the metadata was written for (`documentHash`): the current one,
    /// or after a crash during a write the previous one. The snapshot stays
    /// where it is; see [`RecoveryStore::discard`].
    ///
    /// # Errors
    /// [`StoreError::Io`] with kind [`io::ErrorKind::NotFound`] when the ID
    /// names no snapshot of a stopped instance ([`is_unknown_snapshot`]);
    /// [`StoreError::Invalid`] when the metadata is too large, invalid or
    /// from a newer version, or no document matches it (too large, or
    /// damaged); [`StoreError::Link`] for a snapshot file that is a link or
    /// not a regular file; [`StoreError::Io`] when a file cannot be read.
    pub fn read(&self, snapshot_id: &str) -> Result<(Vec<u8>, SnapshotMeta), StoreError> {
        let _foreign = self.lock_foreign();
        let (_lock, instance_dir) = self.locate(snapshot_id)?;
        let files = SnapshotFiles::new(&instance_dir, snapshot_id);
        let parsed = read_meta(&files.meta)?;
        for candidate in [&files.document, &files.previous] {
            match read_snapshot_file(candidate, MAX_SNAPSHOT_DOCUMENT_BYTES) {
                Ok(bytes) if sha256(&bytes) == parsed.document_hash => return Ok((bytes, parsed.meta)),
                // Another document (a leftover), none, or one that is too large.
                Ok(_) | Err(SnapshotFileError::Read(ReadError::NotFound | ReadError::TooLarge { .. })) => {}
                Err(error) => return Err(error.at(candidate)),
            }
        }
        Err(StoreError::Invalid(
            "the recovery snapshot is damaged: no document matches its metadata",
        ))
    }

    /// Deletes a snapshot of a stopped instance (the user chose *Discard*,
    /// or the app restored it and wrote the restored project's own
    /// snapshot). The instance's folder and lock file go too once nothing is
    /// left in it.
    ///
    /// # Errors
    /// [`StoreError::Io`] with kind [`io::ErrorKind::NotFound`] when the ID
    /// names no snapshot of a stopped instance ([`is_unknown_snapshot`]);
    /// [`StoreError::Io`] when a file cannot be removed.
    pub fn discard(&self, snapshot_id: &str) -> Result<(), StoreError> {
        let _foreign = self.lock_foreign();
        let (lock, instance_dir) = self.locate(snapshot_id)?;
        SnapshotFiles::new(&instance_dir, snapshot_id).remove()?;
        tidy(&instance_dir);
        if fs::remove_dir(&instance_dir).is_ok() {
            lock.remove();
        }
        Ok(())
    }

    /// The IDs of the other instances in the recovery folder: names of
    /// folders and lock files that are instance IDs, in ID order.
    fn other_instances(&self) -> BTreeSet<String> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return BTreeSet::new();
        };
        let mut ids = BTreeSet::new();
        for entry in entries.take(MAX_SCANNED_ENTRIES).flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let id = name.strip_suffix(LOCK_SUFFIX).unwrap_or(name);
            if is_instance_id(id) && id != self.instance_id {
                ids.insert(id.to_owned());
            }
        }
        ids
    }

    /// Adds the snapshots of instance `id` to `found` when its lock is free,
    /// and tidies it up (see [`RecoveryStore::list_restorable`]). Reads at
    /// most `budget` metadata files and takes them off it.
    fn collect_instance(&self, id: &str, found: &mut Vec<Found>, budget: &mut usize) {
        let lock_path = self.dir.join(lock_name(id));
        let instance_dir = self.dir.join(id);
        match dir_kind(&instance_dir) {
            DirKind::Missing => {
                // A lock file without a folder: an instance that ended
                // cleanly but could not remove it, or one starting right now
                // (it takes its lock before it creates its folder, and
                // notices if its lock file was removed under it).
                if let Some(lock) = InstanceLock::try_existing(&lock_path)
                    && dir_kind(&instance_dir) == DirKind::Missing
                {
                    lock.remove();
                }
            }
            // A link, a file or a reparse point named like an instance:
            // not ours, left alone.
            DirKind::Other => {}
            DirKind::Real => {
                let Some(lock) = InstanceLock::try_for_folder(&lock_path, &instance_dir) else {
                    return;
                };
                tidy(&instance_dir);
                let before = found.len();
                found.extend(snapshots_in(&instance_dir, budget));
                if found.len() == before && fs::remove_dir(&instance_dir).is_ok() {
                    lock.remove();
                }
            }
        }
    }

    /// The folder of the stopped instance that holds snapshot `snapshot_id`,
    /// with that instance's lock taken.
    fn locate(&self, snapshot_id: &str) -> Result<(InstanceLock, PathBuf), StoreError> {
        if !is_hex_id(snapshot_id, SNAPSHOT_ID_PREFIX) {
            return Err(self.unknown_snapshot());
        }
        let meta_name = format!("{snapshot_id}{META_SUFFIX}");
        for id in self.other_instances() {
            let instance_dir = self.dir.join(&id);
            if dir_kind(&instance_dir) != DirKind::Real
                || regular_file(&instance_dir.join(&meta_name)).is_none()
            {
                continue;
            }
            let lock_path = self.dir.join(lock_name(&id));
            let Some(lock) = InstanceLock::wait_for_folder(&lock_path, &instance_dir, FOREIGN_LOCK_WAIT)
            else {
                continue;
            };
            // Another instance may have discarded it meanwhile.
            if regular_file(&instance_dir.join(&meta_name)).is_some() {
                return Ok((lock, instance_dir));
            }
        }
        Err(self.unknown_snapshot())
    }

    /// The error for a snapshot ID that names no snapshot of a stopped
    /// instance.
    fn unknown_snapshot(&self) -> StoreError {
        StoreError::io(
            "find the recovery snapshot",
            &self.dir,
            io::ErrorKind::NotFound.into(),
        )
    }

    /// This instance's snapshots; their record is only changed in ways that
    /// keep it truthful, so it stays usable after a panic elsewhere.
    fn lock_snapshots(&self) -> MutexGuard<'_, HashMap<String, Slot>> {
        self.snapshots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_foreign(&self) -> MutexGuard<'_, ()> {
        self.foreign.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for RecoveryStore {
    fn drop(&mut self) {
        // Succeeds only when the folder is empty: the snapshots of projects
        // that still had unsaved changes stay for the next start.
        let removed = match fs::remove_dir(&self.instance_dir) {
            Ok(()) => true,
            Err(error) => error.kind() == io::ErrorKind::NotFound,
        };
        if removed && self.lock.still_at_path() {
            let _ = fs::remove_file(&self.lock.path);
        }
        // `self.lock` is dropped next, which releases the lock.
    }
}

/// An exclusive lock on an instance's lock file; released when dropped.
#[derive(Debug)]
struct InstanceLock {
    /// The locked file, kept open for as long as the lock is held.
    file: File,
    /// Its path.
    path: PathBuf,
}

impl InstanceLock {
    /// Creates a new lock file at `path` (`0600`, exclusively, so an
    /// existing file or link is never opened) and locks it. `None` when a
    /// file already exists there, or when the new file was locked or removed
    /// by a lister before this instance locked it (it was then taken for a
    /// leftover).
    fn create(path: &Path) -> Result<Option<Self>, StoreError> {
        let file = match open_new(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(None),
            Err(source) => return Err(StoreError::io("create the recovery lock", path, source)),
        };
        #[cfg(unix)]
        {
            // The creation mode is narrowed by the umask; make it exactly 0600.
            use std::os::unix::fs::PermissionsExt as _;
            if let Err(source) = file.set_permissions(fs::Permissions::from_mode(0o600)) {
                let _ = fs::remove_file(path);
                return Err(StoreError::io("make the recovery lock private", path, source));
            }
        }
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Error(source)) => {
                let _ = fs::remove_file(path);
                return Err(StoreError::io("lock the recovery instance", path, source));
            }
        }
        let lock = Self {
            file,
            path: path.to_path_buf(),
        };
        Ok(lock.still_at_path().then_some(lock))
    }

    /// Locks the existing lock file at `path` without waiting; `None` when
    /// it is missing, a link or not a file, held by a running instance, or
    /// replaced or removed meanwhile.
    fn try_existing(path: &Path) -> Option<Self> {
        let metadata = fs::symlink_metadata(path).ok()?;
        if is_link(&metadata) || !metadata.is_file() {
            return None;
        }
        Self::lock_opened(open_existing(path).ok()?, path)
    }

    /// Locks the lock file `path` of the instance folder `folder` without
    /// waiting, creating it (`0600`) when it is missing: a folder without a
    /// lock file belongs to no running instance. `None` when the lock is held
    /// or unusable, and when the folder is gone once the lock is taken (the
    /// lock file is then removed as a leftover).
    fn try_for_folder(path: &Path, folder: &Path) -> Option<Self> {
        let lock = match fs::symlink_metadata(path) {
            Ok(_) => Self::try_existing(path)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => match open_new(path) {
                Ok(file) => Self::lock_opened(file, path)?,
                // Another lister created it first.
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Self::try_existing(path)?,
                Err(_) => return None,
            },
            Err(_) => return None,
        };
        if dir_kind(folder) == DirKind::Real {
            Some(lock)
        } else {
            // Discarded meanwhile by another instance (which then removes
            // the lock file too), or never there.
            lock.remove();
            None
        }
    }

    /// [`InstanceLock::try_for_folder`], retrying for at most `wait` while
    /// another instance holds the lock (it is probably listing) and the
    /// folder is still there.
    fn wait_for_folder(path: &Path, folder: &Path, wait: Duration) -> Option<Self> {
        let deadline = Instant::now() + wait;
        loop {
            if let Some(lock) = Self::try_for_folder(path, folder) {
                return Some(lock);
            }
            if Instant::now() >= deadline || dir_kind(folder) != DirKind::Real {
                return None;
            }
            thread::sleep(LOCK_RETRY);
        }
    }

    /// Locks `file`, opened from `path`, without waiting, and checks that
    /// `path` still names it.
    fn lock_opened(file: File, path: &Path) -> Option<Self> {
        file.try_lock().ok()?;
        let lock = Self {
            file,
            path: path.to_path_buf(),
        };
        lock.still_at_path().then_some(lock)
    }

    /// Whether the lock file's path still names the locked file (a regular
    /// file, not a link). A lock on a file that was removed or replaced
    /// protects nothing.
    ///
    /// On Unix the device and inode are compared. On Windows only the kind
    /// and size are (the standard library has no stable file IDs there), so
    /// what this catches is a lock file that is gone or delete-pending. That
    /// is enough: a lock file is removed only by whoever holds its lock, and
    /// a new one is created at the same path only for an instance folder
    /// that exists, which an instance creates after its lock is held.
    fn still_at_path(&self) -> bool {
        let Ok(at_path) = fs::symlink_metadata(&self.path) else {
            return false;
        };
        let Ok(held) = self.file.metadata() else {
            return false;
        };
        !is_link(&at_path) && at_path.is_file() && same_file(&at_path, &held)
    }

    /// Removes the lock file while still holding the lock, then releases
    /// it. Anyone who opened the file before sees afterwards that its path no
    /// longer names it ([`InstanceLock::still_at_path`]).
    fn remove(self) {
        if self.still_at_path() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        // Closing the file releases the lock too; unlocking first releases
        // it at once on Windows.
        let _ = self.file.unlock();
    }
}

/// Opens a new lock file: read and write (a lock needs one of them),
/// exclusively created, `0600` on Unix (before the umask).
fn open_new(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(path)
}

/// Opens an existing lock file for locking. It is never read: on Windows
/// the locked range of a held lock cannot be read.
fn open_existing(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).write(true).open(path)
}

/// Creates the instance folder `0700` without following links; `false`
/// when something already exists at `path`.
fn create_instance_dir(path: &Path) -> Result<bool, StoreError> {
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    match builder.create(path) {
        // Also makes the mode exactly 0700 whatever the umask.
        Ok(()) => ensure_private_dir(path).map(|()| true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(source) => Err(StoreError::io("create the recovery folder", path, source)),
    }
}

/// What is at an instance folder's path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirKind {
    /// Nothing.
    Missing,
    /// A real folder (not a link, junction or other reparse point).
    Real,
    /// Something else, or something that cannot be inspected.
    Other,
}

fn dir_kind(path: &Path) -> DirKind {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !is_link_or_reparse_point(&metadata) => DirKind::Real,
        Err(error) if error.kind() == io::ErrorKind::NotFound => DirKind::Missing,
        Ok(_) | Err(_) => DirKind::Other,
    }
}

/// The metadata of `path` when it is a regular file (not a link).
fn regular_file(path: &Path) -> Option<Metadata> {
    fs::symlink_metadata(path)
        .ok()
        .filter(|metadata| metadata.is_file() && !is_link(metadata))
}

/// Removes the file at `path`; a missing file is fine.
fn remove_if_present(path: &Path) -> Result<(), StoreError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(StoreError::io("remove the recovery snapshot", path, source)),
    }
}

/// `<id>.lock`.
fn lock_name(instance_id: &str) -> String {
    format!("{instance_id}{LOCK_SUFFIX}")
}

/// Whether `text` is an instance ID: 32 lower-case hex digits.
fn is_instance_id(text: &str) -> bool {
    is_lower_hex(text, INSTANCE_ID_DIGITS)
}

/// The files of one snapshot.
#[derive(Debug)]
struct SnapshotFiles {
    /// `<id>.json`.
    meta: PathBuf,
    /// `<id>.b2c`.
    document: PathBuf,
    /// `<id>.prev.b2c`.
    previous: PathBuf,
}

impl SnapshotFiles {
    fn new(instance_dir: &Path, snapshot_id: &str) -> Self {
        Self {
            meta: instance_dir.join(format!("{snapshot_id}{META_SUFFIX}")),
            document: instance_dir.join(format!("{snapshot_id}{DOCUMENT_SUFFIX}")),
            previous: instance_dir.join(format!("{snapshot_id}{PREVIOUS_SUFFIX}")),
        }
    }

    /// Removes the snapshot, metadata first.
    fn remove(&self) -> Result<(), StoreError> {
        remove_if_present(&self.meta)?;
        remove_if_present(&self.document)?;
        remove_if_present(&self.previous)
    }
}

/// What a file in an instance folder is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotFileKind {
    Meta,
    Document,
    Previous,
}

/// The snapshot ID and kind of a snapshot file's name, if it is one.
fn snapshot_file(name: &str) -> Option<(&str, SnapshotFileKind)> {
    let (id, kind) = if let Some(id) = name.strip_suffix(META_SUFFIX) {
        (id, SnapshotFileKind::Meta)
    } else if let Some(id) = name.strip_suffix(PREVIOUS_SUFFIX) {
        (id, SnapshotFileKind::Previous)
    } else {
        (name.strip_suffix(DOCUMENT_SUFFIX)?, SnapshotFileKind::Document)
    };
    is_hex_id(id, SNAPSHOT_ID_PREFIX).then_some((id, kind))
}

/// The names in `dir` (at most [`MAX_SCANNED_ENTRIES`]) that are valid
/// Unicode; none when it cannot be read.
fn names_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .take(MAX_SCANNED_ENTRIES)
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

/// Removes what a stopped instance left behind that can never be restored:
/// temporary files of interrupted writes, and documents without metadata.
/// Everything else (snapshots, files this version does not know) stays.
fn tidy(instance_dir: &Path) {
    let names = names_in(instance_dir);
    let with_meta: BTreeSet<&str> = names
        .iter()
        .filter_map(|name| match snapshot_file(name) {
            Some((id, SnapshotFileKind::Meta)) => Some(id),
            _ => None,
        })
        .collect();
    for name in &names {
        let leftover = match snapshot_file(name) {
            Some((id, SnapshotFileKind::Document | SnapshotFileKind::Previous)) => !with_meta.contains(id),
            Some((_, SnapshotFileKind::Meta)) => false,
            None => name.starts_with(TEMP_PREFIX) && name.ends_with(TEMP_SUFFIX),
        };
        let path = instance_dir.join(name);
        // Files and links only (removing a link removes only the link).
        if leftover && fs::symlink_metadata(&path).is_ok_and(|metadata| !metadata.is_dir()) {
            let _ = fs::remove_file(&path);
        }
    }
}

/// A snapshot found by [`RecoveryStore::list_restorable`].
#[derive(Debug)]
struct Found {
    id: String,
    meta: SnapshotMeta,
    /// `meta.saved_at`, parsed, for the order.
    saved: SystemTime,
}

/// The restorable snapshots in a stopped instance's folder: valid metadata
/// and a document of at most [`MAX_SNAPSHOT_DOCUMENT_BYTES`] (the current
/// or the previous one). Reads at most `budget` metadata files and takes
/// them off it.
fn snapshots_in(instance_dir: &Path, budget: &mut usize) -> Vec<Found> {
    let mut found = Vec::new();
    for name in names_in(instance_dir) {
        let Some((id, SnapshotFileKind::Meta)) = snapshot_file(&name) else {
            continue;
        };
        let Some(rest) = budget.checked_sub(1) else {
            break;
        };
        let files = SnapshotFiles::new(instance_dir, id);
        let has_document = [&files.document, &files.previous].into_iter().any(|path| {
            regular_file(path).is_some_and(|metadata| metadata.len() <= MAX_SNAPSHOT_DOCUMENT_BYTES)
        });
        if !has_document {
            continue;
        }
        *budget = rest;
        if let Ok(parsed) = read_meta(&files.meta) {
            found.push(Found {
                id: id.to_owned(),
                meta: parsed.meta,
                saved: parsed.saved,
            });
        }
    }
    found
}

/// Why a snapshot file could not be read.
#[derive(Debug)]
enum SnapshotFileError {
    /// It is a link (which is never followed).
    Link,
    /// [`read_bounded`] failed.
    Read(ReadError),
}

impl SnapshotFileError {
    fn at(self, path: &Path) -> StoreError {
        match self {
            Self::Link => StoreError::link(path),
            Self::Read(error) => error.at(path),
        }
    }
}

/// Reads a snapshot file of at most `limit` bytes; links are refused.
fn read_snapshot_file(path: &Path, limit: u64) -> Result<Vec<u8>, SnapshotFileError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link(&metadata) => return Err(SnapshotFileError::Link),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(SnapshotFileError::Read(ReadError::NotFound));
        }
        Err(error) => return Err(SnapshotFileError::Read(ReadError::Io(error))),
    }
    read_bounded(path, limit).map_err(SnapshotFileError::Read)
}

/// Metadata read from a file.
#[derive(Debug)]
struct ParsedMeta {
    meta: SnapshotMeta,
    /// `savedAt`, parsed.
    saved: SystemTime,
    /// `documentHash`.
    document_hash: [u8; 32],
}

/// Why metadata cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetaProblem {
    /// Not valid metadata of this format.
    Invalid,
    /// Written by a newer version.
    Newer,
}

/// Reads and validates the metadata file at `path`.
fn read_meta(path: &Path) -> Result<ParsedMeta, StoreError> {
    let bytes = match read_snapshot_file(path, MAX_SNAPSHOT_META_BYTES) {
        Ok(bytes) => bytes,
        Err(SnapshotFileError::Read(ReadError::TooLarge { .. })) => {
            return Err(StoreError::Invalid(
                "the recovery snapshot's metadata is larger than 64 KiB",
            ));
        }
        Err(error) => return Err(error.at(path)),
    };
    parse_meta(&bytes).map_err(|problem| match problem {
        MetaProblem::Invalid => StoreError::Invalid("the recovery snapshot's metadata is invalid"),
        MetaProblem::Newer => {
            StoreError::Invalid("the recovery snapshot was written by a newer version of Blocks2Cpp")
        }
    })
}

/// The metadata header, read first so that a newer version is told apart
/// from an invalid file.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    format: String,
    format_version: u64,
}

/// The metadata as written.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MetaOut<'a> {
    format: &'static str,
    format_version: u64,
    project_id: &'a str,
    project_name: &'a str,
    has_path: bool,
    bound_path: Option<&'a str>,
    saved_at: &'a str,
    app_version: &'a str,
    trusted_at_write: bool,
    security_hash: String,
    document_hash: String,
}

/// The metadata as read: every key required, nothing else allowed.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MetaIn {
    #[allow(dead_code)] // checked through `Header`
    format: String,
    #[allow(dead_code)] // checked through `Header`
    format_version: u64,
    project_id: String,
    project_name: String,
    has_path: bool,
    /// Required, but may be `null` (an `Option` alone would also accept a
    /// missing key).
    #[serde(deserialize_with = "nullable")]
    bound_path: Option<String>,
    saved_at: String,
    app_version: String,
    trusted_at_write: bool,
    security_hash: String,
    document_hash: String,
}

/// A required value that may be `null`.
fn nullable<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

/// Parses and validates metadata (see the module documentation).
fn parse_meta(bytes: &[u8]) -> Result<ParsedMeta, MetaProblem> {
    let header: Header = serde_json::from_slice(bytes).map_err(|_| MetaProblem::Invalid)?;
    if header.format != RECOVERY_FORMAT {
        return Err(MetaProblem::Invalid);
    }
    if header.format_version > RECOVERY_FORMAT_VERSION {
        return Err(MetaProblem::Newer);
    }
    if header.format_version != RECOVERY_FORMAT_VERSION {
        return Err(MetaProblem::Invalid);
    }
    let json: MetaIn = serde_json::from_slice(bytes).map_err(|_| MetaProblem::Invalid)?;
    let bound_path = match json.bound_path {
        Some(text) if json.has_path && valid_bound_path(&text) => Some(PathBuf::from(text)),
        Some(_) => return Err(MetaProblem::Invalid),
        None => None,
    };
    let valid = json.project_name.len() <= MAX_SNAPSHOT_NAME_BYTES && valid_app_version(&json.app_version);
    if !valid {
        return Err(MetaProblem::Invalid);
    }
    Ok(ParsedMeta {
        saved: parse_rfc3339_utc(&json.saved_at).ok_or(MetaProblem::Invalid)?,
        document_hash: parse_hash(&json.document_hash).ok_or(MetaProblem::Invalid)?,
        meta: SnapshotMeta {
            project_id: ProjectId::new(&json.project_id).map_err(|_| MetaProblem::Invalid)?,
            project_name: json.project_name,
            has_path: json.has_path,
            bound_path,
            saved_at: json.saved_at,
            app_version: json.app_version,
            trusted_at_write: json.trusted_at_write,
            security_hash: parse_hash(&json.security_hash).ok_or(MetaProblem::Invalid)?,
        },
    })
}

/// The metadata file for `meta` and a document with SHA-256
/// `document_hash` (see [`RecoveryStore::write`] for the rules).
fn encode_meta(meta: &SnapshotMeta, document_hash: &[u8; 32]) -> Result<Vec<u8>, StoreError> {
    let bound_path = match &meta.bound_path {
        None => None,
        Some(_) if !meta.has_path => {
            return Err(StoreError::Invalid(
                "a snapshot of a project without a file cannot have a bound path",
            ));
        }
        Some(path) if !is_plain_absolute(path) => {
            return Err(StoreError::Invalid(
                "a snapshot's bound path must be absolute, without '.' or '..' parts",
            ));
        }
        // Recorded when it can be; otherwise `null` with `hasPath: true`.
        Some(path) => path.to_str().filter(|text| valid_bound_path(text)),
    };
    if parse_rfc3339_utc(&meta.saved_at).is_none() {
        return Err(StoreError::Invalid(
            "a snapshot's time must be an RFC 3339 timestamp",
        ));
    }
    if !valid_app_version(&meta.app_version) {
        return Err(StoreError::Invalid(
            "a snapshot's app version must be 1 to 64 characters of SemVer text",
        ));
    }
    let out = MetaOut {
        format: RECOVERY_FORMAT,
        format_version: RECOVERY_FORMAT_VERSION,
        project_id: meta.project_id.as_str(),
        project_name: shorten(&meta.project_name, MAX_SNAPSHOT_NAME_BYTES),
        has_path: meta.has_path,
        bound_path,
        saved_at: &meta.saved_at,
        app_version: &meta.app_version,
        trusted_at_write: meta.trusted_at_write,
        security_hash: hex_lower(&meta.security_hash),
        document_hash: hex_lower(document_hash),
    };
    let mut bytes = serde_json::to_vec_pretty(&out)
        .map_err(|_| StoreError::Invalid("the snapshot's metadata could not be serialised"))?;
    bytes.push(b'\n');
    // Cannot happen with the limits above (at most about 56 KiB even when
    // every character needs an escape); checked so a write never produces a
    // file that would not be read back.
    if !u64::try_from(bytes.len()).is_ok_and(|len| len <= MAX_SNAPSHOT_META_BYTES) {
        return Err(StoreError::Invalid(
            "the snapshot's metadata is larger than 64 KiB",
        ));
    }
    Ok(bytes)
}

/// Checks a key of [`RecoveryStore::write`].
fn check_key(key: &str) -> Result<(), StoreError> {
    if key.is_empty() || key.len() > MAX_SNAPSHOT_KEY_BYTES {
        return Err(StoreError::Invalid(
            "a recovery snapshot's key must be 1 to 256 bytes long",
        ));
    }
    Ok(())
}

/// Whether `path` is absolute without `.` or `..` parts.
fn is_plain_absolute(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
}

/// Whether `text` may be recorded as a bound path: at most
/// [`MAX_BOUND_PATH_BYTES`], no NUL, absolute, no `.` or `..` parts.
fn valid_bound_path(text: &str) -> bool {
    text.len() <= MAX_BOUND_PATH_BYTES && !text.contains('\0') && is_plain_absolute(Path::new(text))
}

/// Whether `text` is an app version: 1 to [`MAX_APP_VERSION_BYTES`] bytes of
/// SemVer characters (ASCII letters and digits, `.`, `-` and `+`).
fn valid_app_version(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_APP_VERSION_BYTES
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
}

/// 64 lower-case hexadecimal digits as 32 bytes.
fn parse_hash(text: &str) -> Option<[u8; 32]> {
    if !is_lower_hex(text, 64) {
        return None;
    }
    let mut hash = [0_u8; 32];
    for (byte, pair) in hash.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        let pair = std::str::from_utf8(pair).ok()?;
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(hash)
}

/// `text` cut to at most `max` bytes at a character boundary.
fn shorten(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::time::rfc3339_utc;

    fn meta(name: &str, path: Option<&Path>) -> SnapshotMeta {
        SnapshotMeta {
            project_id: ProjectId::new("prj_test").unwrap(),
            project_name: name.to_owned(),
            has_path: path.is_some(),
            bound_path: path.map(Path::to_path_buf),
            saved_at: rfc3339_utc(SystemTime::now()),
            app_version: "0.2.0".to_owned(),
            trusted_at_write: true,
            security_hash: [7; 32],
        }
    }

    /// An absolute path on this platform.
    fn absolute(name: &str) -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(format!(r"C:\games\{name}"))
        } else {
            PathBuf::from(format!("/games/{name}"))
        }
    }

    /// A recovery folder in a temporary folder.
    fn recovery_dir() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("recovery");
        (root, dir)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names = names_in(dir);
        names.sort();
        names
    }

    fn fail(_: WriteStep) -> io::Result<()> {
        Err(io::Error::other("simulated crash"))
    }

    #[test]
    fn metadata_round_trips() {
        let path = absolute("guess.b2c");
        let original = meta("Guess", Some(&path));
        let bytes = encode_meta(&original, &[9; 32]).unwrap();
        let parsed = parse_meta(&bytes).unwrap();
        assert_eq!(parsed.meta, original);
        assert_eq!(parsed.document_hash, [9; 32]);
        assert_eq!(parsed.saved, parse_rfc3339_utc(&original.saved_at).unwrap());
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains(r#""format": "blocks2cpp/recovery""#), "{text}");
        assert!(text.contains(r#""formatVersion": 1"#), "{text}");
        assert!(text.ends_with("}\n"));
        // A project never saved.
        let unsaved = meta("New project", None);
        assert_eq!(
            parse_meta(&encode_meta(&unsaved, &[0; 32]).unwrap())
                .unwrap()
                .meta,
            unsaved
        );
    }

    #[test]
    fn invalid_metadata_is_refused_when_written() {
        let path = absolute("guess.b2c");
        let mut bad = meta("Guess", Some(&path));
        bad.has_path = false;
        assert!(matches!(encode_meta(&bad, &[0; 32]), Err(StoreError::Invalid(_))));
        // (An inner `.` is no part at all: `Path::components` drops it.)
        let up = absolute("../guess.b2c");
        for relative in [Path::new("guess.b2c"), Path::new("./guess.b2c"), up.as_path()] {
            let bad = meta("Guess", Some(relative));
            assert!(
                matches!(encode_meta(&bad, &[0; 32]), Err(StoreError::Invalid(_))),
                "{}",
                relative.display()
            );
        }
        let mut bad = meta("Guess", None);
        bad.saved_at = "yesterday".to_owned();
        assert!(matches!(encode_meta(&bad, &[0; 32]), Err(StoreError::Invalid(_))));
        for version in [
            "",
            "0.2.0 beta",
            "0.2.0\n",
            &"1".repeat(MAX_APP_VERSION_BYTES + 1),
        ] {
            let mut bad = meta("Guess", None);
            bad.app_version = version.to_owned();
            assert!(
                matches!(encode_meta(&bad, &[0; 32]), Err(StoreError::Invalid(_))),
                "{version:?}"
            );
        }
        let mut good = meta("Guess", None);
        good.app_version = "1.0.0-rc.1+build.5".to_owned();
        assert!(encode_meta(&good, &[0; 32]).is_ok());
    }

    #[test]
    fn long_names_are_shortened_and_long_paths_not_recorded() {
        let name = "é".repeat(MAX_SNAPSHOT_NAME_BYTES);
        let long = absolute(&"a".repeat(MAX_BOUND_PATH_BYTES));
        let parsed = parse_meta(&encode_meta(&meta(&name, Some(&long)), &[0; 32]).unwrap()).unwrap();
        assert_eq!(parsed.meta.project_name.len(), MAX_SNAPSHOT_NAME_BYTES);
        assert!(name.starts_with(&parsed.meta.project_name));
        // The project had a file, but its path could not be recorded.
        assert!(parsed.meta.has_path);
        assert_eq!(parsed.meta.bound_path, None);
    }

    /// The largest metadata possible, with every character escaped, stays
    /// below the read limit.
    #[test]
    fn the_largest_metadata_stays_readable() {
        let name = "\u{1}".repeat(MAX_SNAPSHOT_NAME_BYTES);
        let root = if cfg!(windows) { r"C:\" } else { "/" };
        let mut path = root.to_owned();
        path.push_str(&"\u{1}".repeat(MAX_BOUND_PATH_BYTES - path.len()));
        let mut worst = meta(&name, Some(Path::new(&path)));
        worst.app_version = "1".repeat(MAX_APP_VERSION_BYTES);
        let bytes = encode_meta(&worst, &[0xff; 32]).unwrap();
        assert!(bytes.len() > 50 * 1024, "{}", bytes.len());
        assert!(u64::try_from(bytes.len()).unwrap() <= MAX_SNAPSHOT_META_BYTES);
        assert_eq!(parse_meta(&bytes).unwrap().meta, worst);
    }

    #[cfg(unix)]
    #[test]
    fn paths_that_are_not_unicode_are_not_recorded() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt as _;
        let path = Path::new(OsStr::from_bytes(b"/games/\xff.b2c"));
        let parsed = parse_meta(&encode_meta(&meta("Guess", Some(path)), &[0; 32]).unwrap()).unwrap();
        assert!(parsed.meta.has_path);
        assert_eq!(parsed.meta.bound_path, None);
    }

    #[test]
    fn metadata_is_parsed_strictly() {
        let path = absolute("guess.b2c");
        let good: serde_json::Value =
            serde_json::from_slice(&encode_meta(&meta("Guess", Some(&path)), &[0; 32]).unwrap()).unwrap();
        let check = |change: &dyn Fn(&mut serde_json::Value)| {
            let mut value = good.clone();
            change(&mut value);
            parse_meta(&serde_json::to_vec(&value).unwrap()).map(|parsed| parsed.meta)
        };
        assert!(check(&|_| {}).is_ok());
        let invalid = Err(MetaProblem::Invalid);
        assert_eq!(check(&|v| v["format"] = "blocks2cpp/trust".into()), invalid);
        assert_eq!(check(&|v| v["formatVersion"] = 0.into()), invalid);
        assert_eq!(check(&|v| v["formatVersion"] = 2.into()), Err(MetaProblem::Newer));
        assert_eq!(check(&|v| v["extra"] = true.into()), invalid);
        assert_eq!(check(&|v| v["projectId"] = "prj-1".into()), invalid);
        assert_eq!(check(&|v| v["hasPath"] = false.into()), invalid);
        assert_eq!(check(&|v| v["hasPath"] = "yes".into()), invalid);
        assert_eq!(check(&|v| v["boundPath"] = "games/guess.b2c".into()), invalid);
        assert_eq!(check(&|v| v["savedAt"] = "2026-13-01T00:00:00Z".into()), invalid);
        assert_eq!(check(&|v| v["appVersion"] = "".into()), invalid);
        assert_eq!(check(&|v| v["trustedAtWrite"] = 1.into()), invalid);
        assert_eq!(check(&|v| v["securityHash"] = "AB".repeat(32).into()), invalid);
        assert_eq!(check(&|v| v["documentHash"] = "0".repeat(63).into()), invalid);
        assert_eq!(
            check(&|v| v["projectName"] = "x".repeat(MAX_SNAPSHOT_NAME_BYTES + 1).into()),
            invalid
        );
        for key in [
            "format",
            "formatVersion",
            "projectId",
            "projectName",
            "hasPath",
            "boundPath",
            "savedAt",
            "appVersion",
            "trustedAtWrite",
            "securityHash",
            "documentHash",
        ] {
            assert_eq!(
                check(&|v| {
                    v.as_object_mut().unwrap().remove(key);
                }),
                invalid,
                "{key}"
            );
        }
        // `null` is fine for a project never saved, and for an unrecordable
        // path.
        assert!(check(&|v| v["boundPath"] = serde_json::Value::Null).is_ok());
        // Duplicate keys and other JSON.
        let text = String::from_utf8(serde_json::to_vec(&good).unwrap()).unwrap();
        let duplicate = text.replacen("\"hasPath\":true", "\"hasPath\":true,\"hasPath\":true", 1);
        assert_ne!(duplicate, text);
        assert_eq!(parse_meta(duplicate.as_bytes()).map(|p| p.meta), invalid);
        for other in [&b""[..], b"[]", b"null", b"{", &[0xff, 0xfe]] {
            assert_eq!(parse_meta(other).map(|p| p.meta), invalid);
        }
    }

    #[test]
    fn keys_and_documents_are_bounded() {
        let (_root, dir) = recovery_dir();
        let store = RecoveryStore::open(&dir).unwrap();
        let unsaved = meta("Guess", None);
        for key in [String::new(), "k".repeat(MAX_SNAPSHOT_KEY_BYTES + 1)] {
            assert!(matches!(
                store.write(&key, b"{}", &unsaved),
                Err(StoreError::Invalid(_))
            ));
        }
        assert!(
            store
                .write(&"k".repeat(MAX_SNAPSHOT_KEY_BYTES), b"{}", &unsaved)
                .is_ok()
        );
        let huge = vec![b' '; MAX_FILE_BYTES + 1];
        assert!(matches!(
            store.write("big", &huge, &unsaved),
            Err(StoreError::Invalid(_))
        ));
        // Nothing was written for the refused calls.
        assert_eq!(names(&store.instance_dir).len(), 2);
    }

    #[test]
    fn an_instance_keeps_a_bounded_number_of_snapshots() {
        let (_root, dir) = recovery_dir();
        let store = RecoveryStore::open(&dir).unwrap();
        let unsaved = meta("Guess", None);
        for index in 0..MAX_SNAPSHOTS_PER_INSTANCE {
            store.write(&format!("ph_{index}"), b"{}", &unsaved).unwrap();
        }
        assert!(matches!(
            store.write("one more", b"{}", &unsaved),
            Err(StoreError::Invalid(_))
        ));
        // Existing keys can still be written, and deleting makes room.
        store.write("ph_0", b"{ }", &unsaved).unwrap();
        store.delete_for("ph_1").unwrap();
        store.write("one more", b"{}", &unsaved).unwrap();
    }

    #[test]
    fn a_write_replaces_the_pair_and_leaves_nothing_else() {
        let (_root, dir) = recovery_dir();
        let store = RecoveryStore::open(&dir).unwrap();
        let unsaved = meta("Guess", None);
        let id = store.write("ph_a", b"one", &unsaved).unwrap();
        assert_eq!(store.write("ph_a", b"two", &unsaved).unwrap(), id);
        assert_eq!(
            names(&store.instance_dir),
            [format!("{id}.b2c"), format!("{id}.json")]
        );
        assert_eq!(
            fs::read(store.instance_dir.join(format!("{id}.b2c"))).unwrap(),
            b"two"
        );
    }

    /// Reads the snapshot `id` of a stopped instance through a new store.
    fn restore(dir: &Path, id: &str) -> Result<(Vec<u8>, SnapshotMeta), StoreError> {
        RecoveryStore::open(dir).unwrap().read(id)
    }

    #[test]
    fn a_crash_after_the_rename_restores_the_previous_pair() {
        let (_root, dir) = recovery_dir();
        let store = RecoveryStore::open(&dir).unwrap();
        let first = meta("First", None);
        let id = store.write("ph_a", b"one", &first).unwrap();
        let mut second = meta("Second", None);
        second.trusted_at_write = false;
        assert!(
            store
                .write_with_hook(
                    "ph_a",
                    b"two",
                    &second,
                    &mut |step| if step == WriteStep::Rotated {
                        fail(step)
                    } else {
                        Ok(())
                    }
                )
                .is_err()
        );
        assert_eq!(
            names(&store.instance_dir),
            [format!("{id}.json"), format!("{id}.prev.b2c")]
        );
        drop(store);
        assert_eq!(restore(&dir, &id).unwrap(), (b"one".to_vec(), first));
    }

    #[test]
    fn a_crash_before_the_metadata_restores_the_previous_pair() {
        let (_root, dir) = recovery_dir();
        let store = RecoveryStore::open(&dir).unwrap();
        let first = meta("First", None);
        let id = store.write("ph_a", b"one", &first).unwrap();
        let mut second = meta("Second", None);
        second.trusted_at_write = false;
        assert!(
            store
                .write_with_hook(
                    "ph_a",
                    b"two",
                    &second,
                    &mut |step| if step == WriteStep::DocumentWritten {
                        fail(step)
                    } else {
                        Ok(())
                    }
                )
                .is_err()
        );
        assert_eq!(
            names(&store.instance_dir),
            [
                format!("{id}.b2c"),
                format!("{id}.json"),
                format!("{id}.prev.b2c")
            ]
        );
        drop(store);
        // The new document is never paired with the old metadata.
        assert_eq!(restore(&dir, &id).unwrap(), (b"one".to_vec(), first));
    }

    #[test]
    fn writes_after_a_failed_one_keep_the_pairs_consistent() {
        let (_root, dir) = recovery_dir();
        let store = RecoveryStore::open(&dir).unwrap();
        let unsaved = meta("Guess", None);
        let id = store.write("ph_a", b"one", &unsaved).unwrap();
        // Two failed writes in a row, the second without a rename: the
        // metadata still belongs to the first document.
        for document in [&b"two"[..], b"three"] {
            assert!(
                store
                    .write_with_hook("ph_a", document, &unsaved, &mut |step| {
                        if step == WriteStep::DocumentWritten {
                            fail(step)
                        } else {
                            Ok(())
                        }
                    })
                    .is_err()
            );
            let files = SnapshotFiles::new(&store.instance_dir, &id);
            assert_eq!(fs::read(&files.previous).unwrap(), b"one");
            assert_eq!(fs::read(&files.document).unwrap(), document);
        }
        // A good write completes the pair and removes the leftover.
        let mut last = meta("Last", None);
        last.trusted_at_write = false;
        assert_eq!(store.write("ph_a", b"four", &last).unwrap(), id);
        assert_eq!(
            names(&store.instance_dir),
            [format!("{id}.b2c"), format!("{id}.json")]
        );
        drop(store);
        assert_eq!(restore(&dir, &id).unwrap(), (b"four".to_vec(), last));
    }

    #[test]
    fn a_crash_during_the_first_write_leaves_nothing_restorable() {
        let (_root, dir) = recovery_dir();
        let store = RecoveryStore::open(&dir).unwrap();
        assert!(
            store
                .write_with_hook("ph_a", b"one", &meta("Guess", None), &mut fail)
                .is_err()
        );
        let instance = store.instance_id().to_owned();
        drop(store);
        let next = RecoveryStore::open(&dir).unwrap();
        assert!(next.list_restorable().is_empty());
        // The document without metadata was a leftover: the folder and its
        // lock file are gone.
        assert_eq!(
            names(&dir),
            [next.instance_id().to_owned(), lock_name(next.instance_id())]
        );
        assert!(!names(&dir).contains(&instance));
    }

    #[test]
    fn a_damaged_snapshot_is_not_restored() {
        let (_root, dir) = recovery_dir();
        let store = RecoveryStore::open(&dir).unwrap();
        let id = store.write("ph_a", b"one", &meta("Guess", None)).unwrap();
        let files = SnapshotFiles::new(&store.instance_dir, &id);
        drop(store);
        fs::write(&files.document, b"tampered").unwrap();
        let next = RecoveryStore::open(&dir).unwrap();
        assert_eq!(next.list_restorable().len(), 1);
        assert!(matches!(next.read(&id), Err(StoreError::Invalid(_))));
        next.discard(&id).unwrap();
        assert!(next.list_restorable().is_empty());
    }

    #[test]
    fn deleting_removes_every_file_and_a_new_write_gets_a_new_id() {
        let (_root, dir) = recovery_dir();
        let store = RecoveryStore::open(&dir).unwrap();
        let unsaved = meta("Guess", None);
        let id = store.write("ph_a", b"one", &unsaved).unwrap();
        assert!(
            store
                .write_with_hook(
                    "ph_a",
                    b"two",
                    &unsaved,
                    &mut |step| if step == WriteStep::DocumentWritten {
                        fail(step)
                    } else {
                        Ok(())
                    }
                )
                .is_err()
        );
        store.delete_for("ph_a").unwrap();
        assert!(names(&store.instance_dir).is_empty());
        // Deleting again, or an unknown key, is fine.
        store.delete_for("ph_a").unwrap();
        store.delete_for("ph_unknown").unwrap();
        let again = store.write("ph_a", b"three", &unsaved).unwrap();
        assert_ne!(again, id);
        assert!(is_hex_id(&again, SNAPSHOT_ID_PREFIX));
    }

    #[test]
    fn snapshot_file_names_are_recognised() {
        let id = format!("sn_{}", "0123456789abcdef".repeat(2));
        assert_eq!(
            snapshot_file(&format!("{id}.json")),
            Some((id.as_str(), SnapshotFileKind::Meta))
        );
        assert_eq!(
            snapshot_file(&format!("{id}.b2c")),
            Some((id.as_str(), SnapshotFileKind::Document))
        );
        assert_eq!(
            snapshot_file(&format!("{id}.prev.b2c")),
            Some((id.as_str(), SnapshotFileKind::Previous))
        );
        for other in [
            format!("{id}.txt"),
            format!("{id}.lock"),
            "sn_1.json".to_owned(),
            format!("SN_{}.json", "0".repeat(32)),
            ".b2c-abc.tmp".to_owned(),
        ] {
            assert_eq!(snapshot_file(&other), None, "{other}");
        }
    }

    #[test]
    fn errors_never_show_paths() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("secret-project-recovery");
        let store = RecoveryStore::open(&dir).unwrap();
        let errors = [
            store.read("sn_unknown").unwrap_err(),
            store.discard(&format!("sn_{}", "0".repeat(32))).unwrap_err(),
            store.write("", b"", &meta("Guess", None)).unwrap_err(),
        ];
        for error in &errors {
            let text = error.to_string();
            assert!(!text.contains("secret-project"), "{text}");
        }
        assert!(is_unknown_snapshot(&errors[0]));
        assert!(is_unknown_snapshot(&errors[1]));
        assert!(!is_unknown_snapshot(&errors[2]));
    }

    fn arb_meta() -> impl Strategy<Value = SnapshotMeta> {
        (
            "[A-Za-z0-9_]{1,32}",
            any::<String>(),
            proptest::option::of("[a-zA-Z0-9 é_\u{1}-]{1,40}"),
            any::<bool>(),
            any::<[u8; 32]>(),
            0_u64..253_402_300_799_000,
        )
            .prop_map(|(id, name, path, trusted, hash, millis)| {
                let name = shorten(&name, MAX_SNAPSHOT_NAME_BYTES).to_owned();
                let path = path.map(|tail| absolute(&tail));
                SnapshotMeta {
                    project_id: ProjectId::new(&id).unwrap(),
                    project_name: name,
                    has_path: path.is_some(),
                    bound_path: path,
                    saved_at: rfc3339_utc(SystemTime::UNIX_EPOCH + Duration::from_millis(millis)),
                    app_version: "0.2.0".to_owned(),
                    trusted_at_write: trusted,
                    security_hash: hash,
                }
            })
    }

    proptest! {
        /// Every metadata the store writes reads back unchanged.
        #[test]
        fn written_metadata_reads_back(meta in arb_meta(), hash in any::<[u8; 32]>()) {
            let bytes = encode_meta(&meta, &hash).unwrap();
            let parsed = parse_meta(&bytes).unwrap();
            prop_assert_eq!(parsed.meta, meta);
            prop_assert_eq!(parsed.document_hash, hash);
        }

        /// No input makes the parser panic, and what it accepts is valid.
        #[test]
        fn arbitrary_metadata_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
            if let Ok(parsed) = parse_meta(&bytes) {
                prop_assert!(parsed.meta.project_name.len() <= MAX_SNAPSHOT_NAME_BYTES);
            }
        }
    }
}
