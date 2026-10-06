//! Autosave and crash recovery: `recovery_save`, `recovery_list`,
//! `recovery_restore` and `recovery_discard` (`docs/spec/05-project-format.md`
//! §5.10, `docs/spec/08-security.md` §8.3.1 "Restored snapshots").
//!
//! The snapshots live in the recovery folder (`Dirs::recovery`, never next to
//! the project), managed by [`b2c_store::RecoveryStore`]: one folder per
//! running app instance, guarded by an exclusive lock, so only the snapshots
//! of instances that exited or crashed are offered for restore. This module
//! decides what goes into a snapshot and how a snapshot comes back as a
//! project:
//!
//! * **Saving.** `recovery_save` validates the document like `project_save`
//!   (the size before parsing, then the strict loader) and writes its
//!   canonical form as the project's one snapshot, keyed by its handle, with
//!   the facts the restore needs: whether the project had a file, the file's
//!   canonical path, whether it was trusted at that moment and its security
//!   hash. The document also becomes the project's latest document, which
//!   the trust dialog lists from.
//! * **Deleting.** A clean save, *Save as*, closing the project, and
//!   shutdown for projects without unsaved changes delete the snapshot
//!   ([`Backend::discard_snapshot_of`]). Dropping the backend without
//!   shutting down (a crash) leaves the snapshots for the next start.
//! * **Restoring.** `recovery_restore` loads the snapshot's document with the
//!   strict loader, opens it under a new handle (counted against the limit of
//!   open projects) with unsaved changes, writes the restored project's own
//!   snapshot and only then discards the old one, so a second crash loses
//!   nothing. A project that had a file is bound to it again (its canonical
//!   form, or the folder's canonical form plus the name when the file is
//!   gone); the file's current SHA-256, when it exists, is the baseline for
//!   saving and for the file watcher. A snapshot of a project that was never
//!   saved has no file.
//! * **Trust of a restored project** (08 §8.3.1): a project that was never
//!   saved, and was trusted as *created here* when the snapshot was written,
//!   is trusted again as *created here*. A project with a file is trusted
//!   only when a trust record still covers that file and either its hash
//!   equals the snapshot's security hash or the snapshot was written while
//!   the project was trusted; a folder record covers it as when the file is
//!   opened. Everything else restores in Restricted Mode: a snapshot whose
//!   path could not be recorded, a snapshot whose document does not match
//!   its own metadata, and a file whose record is gone.
//! * **Bounded.** Documents are at most 32 MiB in both directions; the store
//!   bounds every read, the number of snapshots and the listing. A snapshot
//!   restored in this session is never offered again by it, even when it
//!   could not be discarded.
//!
//! Errors are typed: an unknown snapshot (malformed, gone, restored already,
//! or of a running instance) is `unknownSnapshot`; store failures are `io`
//! or `internal` with the details at debug level. Project content never goes
//! into the log.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use b2c_ipc::dto::{
    Empty, RecoveryDiscardRequest, RecoveryListResponse, RecoveryRestoreRequest, RecoveryRestoreResponse,
    RecoverySaveRequest, SnapshotInfo,
};
use b2c_ipc::{Handle, IpcError, SnapshotId};
use b2c_model::Document;
use b2c_store::recovery::is_unknown_snapshot;
use b2c_store::{
    RecoveryStore, RestrictedReason, SnapshotMeta, StoreError, TrustSource, TrustVerdict, canonical_path,
    rfc3339_utc,
};

use crate::backend::{Backend, command_span};
use crate::errors::store_error;
use crate::limits::MAX_DOCUMENT_BYTES;
use crate::projects::{ProjectEntry, file_name, lock};
use crate::trust::{HandleTrust, identity};
use crate::watcher::FileState;

/// The backend's recovery snapshots: this instance's [`RecoveryStore`] and
/// the snapshots of other instances restored in this session.
#[derive(Debug)]
pub(crate) struct RecoveryState {
    /// The recovery folder.
    dir: PathBuf,
    /// The store, once it could be opened. Opening is tried at startup and
    /// again by each command until it succeeds, so a recovery folder that
    /// cannot be used never stops the app from starting.
    store: Mutex<Option<Arc<RecoveryStore>>>,
    /// The IDs of snapshots being restored, or restored in this session but
    /// not discarded: they are not listed or restored again.
    taken: Mutex<HashSet<String>>,
}

impl RecoveryState {
    /// Opens this instance's recovery store in `dir` (logging a warning when
    /// it cannot be opened yet).
    pub(crate) fn open(dir: &Path) -> Self {
        let state = Self {
            dir: dir.to_path_buf(),
            store: Mutex::new(None),
            taken: Mutex::new(HashSet::new()),
        };
        if let Err(error) = state.store() {
            tracing::warn!(
                ?error,
                "the recovery folder cannot be used; autosave is off for now"
            );
        }
        state
    }

    /// The store, opening it if that has not succeeded yet.
    ///
    /// # Errors
    /// The [`IpcError`] of the failure to open it (`io` or `internal`).
    fn store(&self) -> Result<Arc<RecoveryStore>, IpcError> {
        let mut store = lock(&self.store);
        if let Some(store) = store.as_ref() {
            return Ok(Arc::clone(store));
        }
        let opened = Arc::new(
            RecoveryStore::open(&self.dir)
                .map_err(|error| store_error("open the recovery folder", &error))?,
        );
        tracing::debug!(dir = %opened.dir().display(), instance = opened.instance_id(), "recovery store opened");
        *store = Some(Arc::clone(&opened));
        Ok(opened)
    }

    /// The store if it is open (deleting needs no store that never opened:
    /// nothing was written).
    fn current(&self) -> Option<Arc<RecoveryStore>> {
        lock(&self.store).clone()
    }

    fn taken(&self) -> MutexGuard<'_, HashSet<String>> {
        self.taken.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Claims `snapshot_id` for a restore.
    ///
    /// # Errors
    /// [`IpcError::UnknownSnapshot`] when it is being restored or was
    /// restored in this session.
    fn claim(&self, snapshot_id: &str) -> Result<Claim<'_>, IpcError> {
        if self.taken().insert(snapshot_id.to_owned()) {
            Ok(Claim {
                state: self,
                id: snapshot_id.to_owned(),
                keep: false,
            })
        } else {
            Err(IpcError::UnknownSnapshot)
        }
    }
}

/// A snapshot claimed by a restore in progress; the claim ends when this is
/// dropped, unless [`Claim::keep`] made it permanent.
struct Claim<'a> {
    state: &'a RecoveryState,
    id: String,
    keep: bool,
}

impl Claim<'_> {
    /// Keeps the snapshot claimed for the rest of the session (it was
    /// restored but could not be discarded).
    fn keep(mut self) {
        self.keep = true;
    }
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        if !self.keep {
            self.state.taken().remove(&self.id);
        }
    }
}

/// The canonical bytes of `document`.
///
/// # Errors
/// [`IpcError::PayloadTooLarge`] when they are larger than a project file may
/// be.
fn canonical_text(document: &Document) -> Result<String, IpcError> {
    let text = b2c_model::to_canonical_json(document);
    if text.len() > MAX_DOCUMENT_BYTES {
        return Err(IpcError::too_large(MAX_DOCUMENT_BYTES));
    }
    Ok(text)
}

/// What the snapshot of `entry` with `document` records about it.
///
/// `hasPath` is `false` only for a project that has no file and is trusted
/// as *created here*: that is what restores as *created here*, so a project
/// without a file that is restricted (revoked, or restored from a snapshot
/// whose path could not be recorded) keeps `hasPath: true` and restores
/// restricted again.
fn snapshot_meta(entry: &ProjectEntry, document: &Document, app_version: &str) -> SnapshotMeta {
    let unsaved_here = entry.path.is_none() && entry.trust == HandleTrust::CreatedHere;
    SnapshotMeta {
        project_id: document.project.id.clone(),
        project_name: document.project.name.clone(),
        has_path: !unsaved_here,
        bound_path: entry.path.clone(),
        saved_at: rfc3339_utc(SystemTime::now()),
        app_version: app_version.to_owned(),
        trusted_at_write: entry.trust.is_trusted(),
        security_hash: b2c_model::security_hash(document),
    }
}

/// Loads a snapshot's document with the strict loader, with the errors of
/// [`b2c_ipc::parse_document`] (`newerFormat`, `invalidDocument`).
fn load_snapshot(bytes: &[u8]) -> Result<Document, IpcError> {
    match std::str::from_utf8(bytes) {
        Ok(text) => b2c_ipc::parse_document(text),
        // Not UTF-8: the loader says so with its diagnostics.
        Err(_) => b2c_model::load(bytes).map_err(|error| IpcError::InvalidDocument {
            diagnostics: b2c_ipc::diag::convert_all(&error.diagnostics),
        }),
    }
}

/// The [`IpcError`] of a failed read or discard of a snapshot.
fn snapshot_error(context: &'static str, error: &StoreError) -> IpcError {
    if is_unknown_snapshot(error) {
        IpcError::UnknownSnapshot
    } else {
        store_error(context, error)
    }
}

/// The trust of a restored project with a file, from the trust store's
/// verdict on the file's identity with the snapshot's document (08 §8.3.1):
/// a record that matches trusts it; a project record with another hash
/// trusts it only when the snapshot was written while the project was
/// trusted (its edits were made in the app); nothing else does.
fn restored_trust(verdict: TrustVerdict, trusted_at_write: bool) -> HandleTrust {
    match verdict {
        TrustVerdict::Trusted(source) => HandleTrust::Trusted(source),
        TrustVerdict::Restricted(RestrictedReason::ChangedOutside) if trusted_at_write => {
            HandleTrust::Trusted(TrustSource::Project)
        }
        TrustVerdict::Restricted(reason) => HandleTrust::Restricted(reason),
    }
}

/// The file a restored project is bound to: the canonical form of the
/// recorded path while it exists, otherwise the canonical form of its folder
/// with the file name (or the recorded path itself, which the store checked
/// is absolute and has no `.` or `..` parts).
fn rebind(bound: &Path) -> PathBuf {
    if let Ok(path) = canonical_path(bound) {
        return path;
    }
    match (bound.parent(), bound.file_name()) {
        (Some(folder), Some(name)) => {
            canonical_path(folder).map_or_else(|_| bound.to_path_buf(), |folder| folder.join(name))
        }
        _ => bound.to_path_buf(),
    }
}

/// How a snapshot comes back as a project.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Restored {
    path: Option<PathBuf>,
    trust: HandleTrust,
    created_here: bool,
    /// The file as it is now, when the project has a file whose state could
    /// be read.
    file: Option<FileState>,
    mark_of_the_web: bool,
}

impl Restored {
    /// The SHA-256 of the file now, when it exists and could be read.
    fn baseline(&self) -> Option<[u8; 32]> {
        match self.file {
            Some(FileState::Content(hash)) => Some(hash),
            _ => None,
        }
    }
}

// Commands take their request by value, as the adapter decodes it, so every
// command method has the same shape whether or not it keeps the request.
#[allow(clippy::needless_pass_by_value)]
impl Backend {
    /// `recovery_save`: writes the project's recovery snapshot (autosave,
    /// every 30 s while it has unsaved changes and when the window loses
    /// focus). The document is validated like `project_save`'s and becomes
    /// the project's latest document; the snapshot replaces the project's
    /// previous one as a whole.
    ///
    /// # Errors
    /// The document's errors (`payloadTooLarge`, `invalidDocument`,
    /// `newerFormat`) before anything else; [`IpcError::UnknownHandle`]
    /// (also when the project was closed while the snapshot was written; it
    /// is then deleted); [`IpcError::Internal`] after shutdown, or when the
    /// store refuses the snapshot; [`IpcError::Io`] when it cannot be
    /// written.
    pub fn recovery_save(&self, request: RecoverySaveRequest) -> Result<Empty, IpcError> {
        let _span = command_span("recovery_save");
        let document = b2c_ipc::parse_document(&request.document)?;
        let text = canonical_text(&document)?;
        let entry_ref = self.projects.get(&request.handle)?;
        if self.is_shut_down() {
            return Err(IpcError::Internal);
        }
        let store = self.recovery.store()?;
        {
            // Held while writing, so a save or close of the project comes
            // wholly before or after this snapshot.
            let mut entry = lock(&entry_ref);
            let meta = snapshot_meta(&entry, &document, self.app_version);
            entry.set_latest(document, text.len());
            store
                .write(request.handle.as_str(), text.as_bytes(), &meta)
                .map_err(|error| store_error("write a recovery snapshot", &error))?;
        }
        // A close that ran meanwhile deleted the snapshot before this one was
        // written, or will delete it after: either way none may remain.
        if self.projects.get(&request.handle).is_err() {
            self.discard_snapshot_of(&request.handle);
            return Err(IpcError::UnknownHandle);
        }
        Ok(Empty {})
    }

    /// `recovery_list`: the snapshots offered for restore, those of app
    /// instances that are no longer running, newest first (at most 100).
    /// Snapshots restored in this session are left out.
    ///
    /// # Errors
    /// [`IpcError::Io`] or [`IpcError::Internal`] when the recovery folder
    /// cannot be used.
    pub fn recovery_list(&self) -> Result<RecoveryListResponse, IpcError> {
        let _span = command_span("recovery_list");
        let store = self.recovery.store()?;
        let listed = store.list_restorable();
        let taken = self.recovery.taken();
        let snapshots = listed
            .into_iter()
            .filter(|listing| !taken.contains(&listing.snapshot_id))
            .filter_map(|listing| {
                let Ok(snapshot_id) = SnapshotId::parse(&listing.snapshot_id) else {
                    tracing::debug!("a recovery snapshot with an unexpected ID is not offered");
                    return None;
                };
                Some(SnapshotInfo {
                    snapshot_id,
                    project_name: listing.meta.project_name,
                    saved_at: listing.meta.saved_at,
                    has_path: listing.meta.has_path,
                })
            })
            .collect();
        Ok(RecoveryListResponse { snapshots })
    }

    /// `recovery_restore`: opens a listed snapshot as a project with unsaved
    /// changes, under a new handle, with the trust rules of the module
    /// documentation. The restored project gets its own snapshot before the
    /// old one is discarded; if that cannot be written, the old one is kept
    /// (and not offered again in this session).
    ///
    /// # Errors
    /// [`IpcError::TooManyHandles`] before anything else;
    /// [`IpcError::UnknownSnapshot`] for a snapshot that does not exist (any
    /// more), belongs to a running instance, or was restored already; the
    /// document's errors (`newerFormat`, `invalidDocument`,
    /// `payloadTooLarge`); [`IpcError::Io`] or [`IpcError::Internal`] when
    /// the snapshot cannot be read.
    pub fn recovery_restore(
        &self,
        request: RecoveryRestoreRequest,
    ) -> Result<RecoveryRestoreResponse, IpcError> {
        let _span = command_span("recovery_restore");
        self.projects.ensure_room()?;
        let store = self.recovery.store()?;
        let snapshot_id = request.snapshot_id.as_str();
        let claim = self.recovery.claim(snapshot_id)?;
        let (bytes, meta) = store
            .read(snapshot_id)
            .map_err(|error| snapshot_error("read a recovery snapshot", &error))?;
        let document = load_snapshot(&bytes)?;
        drop(bytes);
        let text = canonical_text(&document)?;
        let restored = self.restored(&document, &meta);
        let entry = ProjectEntry {
            path: restored.path.clone(),
            project_id: document.project.id.clone(),
            baseline_sha256: restored.baseline(),
            latest_bytes_len: text.len(),
            latest_document: Arc::new(document),
            created_here: restored.created_here,
            trust: restored.trust,
            mark_of_the_web: restored.mark_of_the_web,
            // The snapshot holds changes that were never saved.
            dirty: true,
            last_trust_grant: None,
        };
        let new_meta = snapshot_meta(&entry, &entry.latest_document, self.app_version);
        let handle = self.projects.insert(entry)?;
        tracing::debug!(trust = ?restored.trust, "recovery snapshot restored");

        // The restored project's own snapshot first, then the old one goes.
        match store.write(handle.as_str(), text.as_bytes(), &new_meta) {
            Ok(_) => match store.discard(snapshot_id) {
                Ok(()) => {}
                // Discarded meanwhile (`recovery_discard`): nothing to do.
                Err(error) if is_unknown_snapshot(&error) => {}
                Err(error) => {
                    let _ = store_error("discard a restored recovery snapshot", &error);
                    tracing::warn!("a restored recovery snapshot could not be discarded");
                    claim.keep();
                }
            },
            Err(error) => {
                let _ = store_error("write the snapshot of a restored project", &error);
                tracing::warn!("a restored project's snapshot could not be written; the old one is kept");
                claim.keep();
            }
        }
        if let (Some(path), Some(file)) = (restored.path.as_deref(), restored.file) {
            self.watchers.watch_state(&handle, path, file);
        }
        Ok(RecoveryRestoreResponse {
            handle,
            document: text,
            trust: restored.trust.to_dto(restored.mark_of_the_web),
            file_name: restored.path.as_deref().map(file_name),
        })
    }

    /// `recovery_discard`: deletes a listed snapshot.
    ///
    /// # Errors
    /// [`IpcError::UnknownSnapshot`] for a snapshot that does not exist (any
    /// more) or belongs to a running instance; [`IpcError::Io`] when it
    /// cannot be deleted.
    pub fn recovery_discard(&self, request: RecoveryDiscardRequest) -> Result<Empty, IpcError> {
        let _span = command_span("recovery_discard");
        let store = self.recovery.store()?;
        let snapshot_id = request.snapshot_id.as_str();
        store
            .discard(snapshot_id)
            .map_err(|error| snapshot_error("discard a recovery snapshot", &error))?;
        self.recovery.taken().remove(snapshot_id);
        Ok(Empty {})
    }

    /// Deletes the recovery snapshot of `handle`, if it has one (a clean save
    /// or close, and shutdown for projects without unsaved changes). A
    /// failure is logged: the snapshot is then offered at the next start,
    /// which loses nothing.
    pub(crate) fn discard_snapshot_of(&self, handle: &Handle) {
        let Some(store) = self.recovery.current() else {
            return;
        };
        if let Err(error) = store.delete_for(handle.as_str()) {
            let _ = store_error("delete a recovery snapshot", &error);
            tracing::warn!("a recovery snapshot could not be deleted");
        }
    }

    /// How the snapshot with `document` and `meta` comes back as a project
    /// (see the module documentation).
    fn restored(&self, document: &Document, meta: &SnapshotMeta) -> Restored {
        if !meta.has_path {
            return Restored {
                path: None,
                trust: HandleTrust::CreatedHere,
                created_here: true,
                file: None,
                mark_of_the_web: false,
            };
        }
        let Some(bound) = meta.bound_path.as_deref() else {
            // It had a file whose path could not be recorded.
            return Restored {
                path: None,
                trust: HandleTrust::Restricted(RestrictedReason::NoRecord),
                created_here: false,
                file: None,
                mark_of_the_web: false,
            };
        };
        let path = rebind(bound);
        let file = match FileState::of(&path) {
            Ok(state) => Some(state),
            Err(error) => {
                tracing::debug!(path = %path.display(), %error, "cannot read the file of a restored project");
                None
            }
        };
        let mark_of_the_web =
            file.is_some_and(|state| state != FileState::Missing) && b2c_store::mark_of_the_web(&path);
        // The metadata was written with this document; if it does not match
        // it, neither the trust facts nor the document can be relied on.
        let consistent = document.project.id == meta.project_id
            && b2c_model::security_hash(document) == meta.security_hash;
        let trust = if consistent {
            restored_trust(
                self.trust.evaluate(&identity(document, &path)),
                meta.trusted_at_write,
            )
        } else {
            tracing::warn!("a recovery snapshot does not match its metadata; it restores restricted");
            HandleTrust::Restricted(RestrictedReason::NoRecord)
        };
        Restored {
            path: Some(path),
            trust,
            created_here: false,
            file,
            mark_of_the_web,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_trust_follows_the_rules_of_08_8_3_1() {
        use RestrictedReason::{ChangedOutside, NoRecord};
        use TrustSource::{Folder, Project};
        for trusted_at_write in [false, true] {
            assert_eq!(
                restored_trust(TrustVerdict::Trusted(Project), trusted_at_write),
                HandleTrust::Trusted(Project)
            );
            assert_eq!(
                restored_trust(TrustVerdict::Trusted(Folder), trusted_at_write),
                HandleTrust::Trusted(Folder)
            );
            assert_eq!(
                restored_trust(TrustVerdict::Restricted(NoRecord), trusted_at_write),
                HandleTrust::Restricted(NoRecord)
            );
        }
        assert_eq!(
            restored_trust(TrustVerdict::Restricted(ChangedOutside), true),
            HandleTrust::Trusted(Project)
        );
        assert_eq!(
            restored_trust(TrustVerdict::Restricted(ChangedOutside), false),
            HandleTrust::Restricted(ChangedOutside)
        );
    }

    #[test]
    fn rebinding_keeps_the_name_of_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let folder = canonical_path(dir.path()).unwrap();
        let file = folder.join("game.b2c");
        std::fs::write(&file, b"{}").unwrap();
        assert_eq!(rebind(&file), file);
        std::fs::remove_file(&file).unwrap();
        assert_eq!(rebind(&file), file);
        let gone = folder.join("gone").join("game.b2c");
        assert_eq!(rebind(&gone), gone);
        // A root has no file name: it stays as it canonicalises.
        let root = folder.ancestors().last().unwrap();
        assert_eq!(rebind(root), root);
    }

    #[cfg(unix)]
    #[test]
    fn rebinding_resolves_links_in_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let folder = canonical_path(dir.path()).unwrap();
        std::fs::create_dir(folder.join("real")).unwrap();
        std::os::unix::fs::symlink(folder.join("real"), folder.join("link")).unwrap();
        assert_eq!(
            rebind(&folder.join("link").join("game.b2c")),
            folder.join("real").join("game.b2c")
        );
    }

    #[test]
    fn baselines_exist_only_for_readable_files() {
        let restored = |file| Restored {
            path: Some(PathBuf::from("/p/game.b2c")),
            trust: HandleTrust::Restricted(RestrictedReason::NoRecord),
            created_here: false,
            file,
            mark_of_the_web: false,
        };
        assert_eq!(
            restored(Some(FileState::Content([7; 32]))).baseline(),
            Some([7; 32])
        );
        assert_eq!(restored(Some(FileState::Missing)).baseline(), None);
        assert_eq!(restored(Some(FileState::TooLarge)).baseline(), None);
        assert_eq!(restored(None).baseline(), None);
    }

    #[test]
    fn snapshots_that_are_not_projects_are_invalid() {
        assert!(matches!(
            load_snapshot(b"\xff\xfe not text"),
            Err(IpcError::InvalidDocument { .. })
        ));
        assert!(matches!(
            load_snapshot(b"{\"format\": 1}"),
            Err(IpcError::InvalidDocument { .. })
        ));
    }

    #[test]
    fn unknown_snapshots_have_their_own_error() {
        let unknown = StoreError::Io {
            action: "find the recovery snapshot",
            path: PathBuf::from("/r"),
            source: std::io::ErrorKind::NotFound.into(),
        };
        assert_eq!(snapshot_error("read", &unknown), IpcError::UnknownSnapshot);
        assert_eq!(
            snapshot_error("read", &StoreError::Invalid("damaged")),
            IpcError::Internal
        );
    }

    #[test]
    fn claims_end_unless_kept() {
        let dir = tempfile::tempdir().unwrap();
        let state = RecoveryState::open(&dir.path().join("recovery"));
        let claim = state.claim("sn_1").unwrap();
        assert_eq!(state.claim("sn_1").err(), Some(IpcError::UnknownSnapshot));
        drop(claim);
        state.claim("sn_1").unwrap().keep();
        assert_eq!(state.claim("sn_1").err(), Some(IpcError::UnknownSnapshot));
        assert!(state.current().is_some());
    }
}
