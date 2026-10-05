//! Open projects: `project_new`, `project_open_dialog`, `project_open_recent`,
//! `project_reload`, `project_save`, `project_save_as_dialog`, `project_close`
//! and `project_set_dirty` (`docs/spec/02-architecture.md` §2.5.2,
//! `docs/spec/05-project-format.md` §5.6–§5.10, `docs/spec/08-security.md`
//! §8.3 and §8.6).
//!
//! Each open project is a [`ProjectEntry`] behind an opaque [`Handle`]
//! (`ph_` + 128 random bits). The entry holds the canonical path the user
//! chose in a native dialog (or none for a new project), never a path from the
//! webview, so a save can only ever write to that file.
//!
//! **Opening** (dialog, recent, reload): the path is canonicalised, at most
//! 32 MiB + 1 bytes are read, the bytes are loaded with the strict loader
//! (`b2c_model::load`: a newer format gives `newerFormat`, an older one is
//! migrated in memory), the trust verdict is computed from the security hash
//! and the Mark of the Web, the SHA-256 of the bytes becomes the baseline for
//! external-change detection, the file is watched and the recent list
//! updated. Opening never builds or runs anything.
//!
//! **Saving** writes only to the handle's file, after checking that the file
//! on disk still has the baseline hash (`changedOnDisk` otherwise; it is then
//! left untouched): the canonical form, atomically, with the previous version
//! kept as `<name>.b2c.bak`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Instant, SystemTime};

use b2c_ipc::dto::{
    Empty, ProjectCloseRequest, ProjectNewRequest, ProjectNewResponse, ProjectOpenDialogResponse,
    ProjectOpenRecentRequest, ProjectOpened, ProjectReloadRequest, ProjectReloadResponse,
    ProjectSaveAsDialogRequest, ProjectSaveAsDialogResponse, ProjectSaveRequest, ProjectSaveResponse,
    ProjectSavedAs, ProjectSetDirtyRequest,
};
use b2c_ipc::{Handle, IpcError, RecentId};
use b2c_model::Document;
use b2c_store::project_file::sha256;
use b2c_store::{ReadError, canonical_path, read_project, rfc3339_utc, save_project};
use serde::Deserialize;

use crate::backend::{Backend, command_span};
use crate::dialogs::suggested_file_name;
use crate::errors::{io_error, project_read_error, store_error};
use crate::limits::{MAX_DOCUMENT_BYTES, MAX_OPEN_HANDLES};
use crate::templates;
use crate::trust::{HandleTrust, identity};

/// Locks `mutex`, also after a panic elsewhere while it was locked: every
/// value behind these locks is only replaced field by field with complete
/// values, so it is always consistent.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An open project.
#[derive(Debug, Clone)]
pub(crate) struct ProjectEntry {
    /// The canonical path of its file, or `None` for a project that was never
    /// saved. Comes only from a dialog, the recent list or recovery metadata.
    pub(crate) path: Option<PathBuf>,
    /// The project's ID (of the latest document).
    pub(crate) project_id: b2c_ir::ProjectId,
    /// The SHA-256 of the file's bytes as the app last read or wrote them:
    /// the baseline for external-change detection. `None` without a file.
    pub(crate) baseline_sha256: Option<[u8; 32]>,
    /// The latest document the backend received for the project (opened,
    /// saved, built; snapshotted in `w4-recovery-watcher`). The trust dialog
    /// lists from it, and `run_start` compares builds with it.
    pub(crate) latest_document: Arc<Document>,
    /// The size of the latest document's text, in bytes.
    pub(crate) latest_bytes_len: usize,
    /// Created in this app with `project_new` and trusted in memory until its
    /// first save records trust for its file.
    pub(crate) created_here: bool,
    /// The project's trust.
    pub(crate) trust: HandleTrust,
    /// Whether the file has the Mark of the Web.
    pub(crate) mark_of_the_web: bool,
    /// Whether it has unsaved changes (`project_set_dirty`).
    pub(crate) dirty: bool,
    /// When `trust_grant` was last called for it (rate limit).
    pub(crate) last_trust_grant: Option<Instant>,
}

impl ProjectEntry {
    /// The folder of the project file, when it has one.
    pub(crate) fn folder(&self) -> Option<&Path> {
        self.path.as_deref().and_then(Path::parent)
    }

    /// Replaces the latest document.
    pub(crate) fn set_latest(&mut self, document: Document, bytes_len: usize) {
        self.project_id = document.project.id.clone();
        self.latest_document = Arc::new(document);
        self.latest_bytes_len = bytes_len;
    }
}

/// A shared, separately locked [`ProjectEntry`].
pub(crate) type EntryRef = Arc<Mutex<ProjectEntry>>;

/// The open projects by handle, at most [`MAX_OPEN_HANDLES`]. The table's
/// lock is never held while an entry is locked.
#[derive(Debug, Default)]
pub(crate) struct ProjectTable {
    map: Mutex<HashMap<Handle, EntryRef>>,
}

impl ProjectTable {
    /// The entry of `handle`.
    ///
    /// # Errors
    /// [`IpcError::UnknownHandle`] when no project is open under it.
    pub(crate) fn get(&self, handle: &Handle) -> Result<EntryRef, IpcError> {
        lock(&self.map)
            .get(handle)
            .cloned()
            .ok_or(IpcError::UnknownHandle)
    }

    /// Fails when no other project can be opened.
    ///
    /// # Errors
    /// [`IpcError::TooManyHandles`].
    pub(crate) fn ensure_room(&self) -> Result<(), IpcError> {
        if lock(&self.map).len() >= MAX_OPEN_HANDLES {
            Err(IpcError::TooManyHandles)
        } else {
            Ok(())
        }
    }

    /// Adds `entry` under a new random handle.
    ///
    /// # Errors
    /// [`IpcError::TooManyHandles`], or [`IpcError::Internal`] when the OS
    /// random number generator fails.
    pub(crate) fn insert(&self, entry: ProjectEntry) -> Result<Handle, IpcError> {
        let mut map = lock(&self.map);
        if map.len() >= MAX_OPEN_HANDLES {
            return Err(IpcError::TooManyHandles);
        }
        let mut handle = Handle::random()?;
        while map.contains_key(&handle) {
            handle = Handle::random()?;
        }
        map.insert(handle.clone(), Arc::new(Mutex::new(entry)));
        Ok(handle)
    }

    /// Removes the project of `handle`.
    pub(crate) fn remove(&self, handle: &Handle) -> Option<EntryRef> {
        lock(&self.map).remove(handle)
    }

    /// How many projects are open.
    pub(crate) fn len(&self) -> usize {
        lock(&self.map).len()
    }

    /// Every open project.
    pub(crate) fn all(&self) -> Vec<(Handle, EntryRef)> {
        lock(&self.map)
            .iter()
            .map(|(handle, entry)| (handle.clone(), Arc::clone(entry)))
            .collect()
    }

    /// Whether any project has unsaved changes.
    pub(crate) fn any_dirty(&self) -> bool {
        self.all().iter().any(|(_, entry)| lock(entry).dirty)
    }

    /// The handles of projects without unsaved changes.
    pub(crate) fn clean_handles(&self) -> Vec<Handle> {
        self.all()
            .into_iter()
            .filter(|(_, entry)| !lock(entry).dirty)
            .map(|(handle, _)| handle)
            .collect()
    }

    /// The folders of the open projects that have files.
    pub(crate) fn folders(&self) -> Vec<PathBuf> {
        self.all()
            .iter()
            .filter_map(|(_, entry)| lock(entry).folder().map(Path::to_path_buf))
            .collect()
    }
}

/// A project file read and evaluated from disk.
#[derive(Debug)]
pub(crate) struct LoadedFile {
    /// Its canonical path.
    pub(crate) path: PathBuf,
    /// The loaded (and, if it was older, migrated) document.
    pub(crate) document: Document,
    /// The SHA-256 of its bytes.
    pub(crate) baseline: [u8; 32],
    /// The `formatVersion` it had, when it was migrated.
    pub(crate) migrated_from: Option<u32>,
    /// Its trust verdict.
    pub(crate) trust: HandleTrust,
    /// Whether it has the Mark of the Web.
    pub(crate) mark_of_the_web: bool,
}

/// The header keys read to tell whether a file was migrated.
#[derive(Deserialize)]
struct Header {
    #[serde(rename = "formatVersion")]
    format_version: u32,
}

/// The `formatVersion` of a file that loaded, when it is older than the
/// current one (it was migrated in memory).
fn migrated_from(bytes: &[u8]) -> Option<u32> {
    let header: Header = serde_json::from_slice(bytes).ok()?;
    (header.format_version < b2c_model::CURRENT_FORMAT_VERSION).then_some(header.format_version)
}

/// Loads project bytes read from a file, with the IPC errors of
/// [`b2c_ipc::parse_document`] (`newerFormat`, `invalidDocument`).
fn load_bytes(bytes: &[u8]) -> Result<Document, IpcError> {
    match std::str::from_utf8(bytes) {
        Ok(text) => b2c_ipc::parse_document(text),
        // Not UTF-8: the loader says so with its diagnostics.
        Err(_) => b2c_model::load(bytes).map_err(|error| IpcError::InvalidDocument {
            diagnostics: b2c_ipc::diag::convert_all(&error.diagnostics),
        }),
    }
}

/// The name of a file, for display.
pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The canonical bytes of `document`.
///
/// # Errors
/// [`IpcError::PayloadTooLarge`] when they would be larger than a project
/// file may be (they could never be opened again).
fn canonical_bytes(document: &Document) -> Result<Vec<u8>, IpcError> {
    let text = b2c_model::to_canonical_json(document);
    if text.len() > MAX_DOCUMENT_BYTES {
        return Err(IpcError::too_large(MAX_DOCUMENT_BYTES));
    }
    Ok(text.into_bytes())
}

// Commands take their request by value, as the adapter decodes it, so every
// command method has the same shape whether or not it keeps the request.
#[allow(clippy::needless_pass_by_value)]
impl Backend {
    /// Reads, loads and evaluates the project file at `path`.
    ///
    /// # Errors
    /// [`IpcError::NotFound`] for a missing file, [`IpcError::PayloadTooLarge`]
    /// for one over 32 MiB (read no further), [`IpcError::NewerFormat`] and
    /// [`IpcError::InvalidDocument`] from the loader, [`IpcError::Io`].
    pub(crate) fn load_file(&self, path: &Path) -> Result<LoadedFile, IpcError> {
        let canonical = canonical_path(path).map_err(|error| match &error {
            b2c_store::StoreError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
                tracing::debug!(path = %path.display(), "the project file does not exist");
                IpcError::NotFound
            }
            _ => store_error("find a project file", &error),
        })?;
        let bytes = read_project(&canonical).map_err(|error| project_read_error(&canonical, &error))?;
        let document = load_bytes(&bytes)?;
        let migrated_from = migrated_from(&bytes);
        let mark_of_the_web = b2c_store::mark_of_the_web(&canonical);
        let trust = HandleTrust::from_verdict(self.trust.evaluate(&identity(&document, &canonical)));
        tracing::debug!(path = %canonical.display(), trust = ?trust, "project file loaded");
        Ok(LoadedFile {
            baseline: sha256(&bytes),
            path: canonical,
            document,
            migrated_from,
            trust,
            mark_of_the_web,
        })
    }

    /// Opens the file at `path` as a new handle (dialog and recent list).
    fn open_file(&self, path: &Path) -> Result<ProjectOpened, IpcError> {
        self.projects.ensure_room()?;
        let loaded = self.load_file(path)?;
        let document_text = b2c_model::to_canonical_json(&loaded.document);
        let trust = loaded.trust.to_dto(loaded.mark_of_the_web);
        let entry = ProjectEntry {
            path: Some(loaded.path.clone()),
            project_id: loaded.document.project.id.clone(),
            baseline_sha256: Some(loaded.baseline),
            latest_bytes_len: document_text.len(),
            latest_document: Arc::new(loaded.document),
            created_here: false,
            trust: loaded.trust,
            mark_of_the_web: loaded.mark_of_the_web,
            dirty: false,
            last_trust_grant: None,
        };
        let project_name = entry.latest_document.project.name.clone();
        let handle = self.projects.insert(entry)?;
        self.watchers.watch(&handle, &loaded.path, loaded.baseline);
        self.touch_recent(&loaded.path, &project_name);
        Ok(ProjectOpened {
            handle,
            document: document_text,
            trust,
            file_name: file_name(&loaded.path),
            migrated_from: loaded.migrated_from,
        })
    }

    /// `project_new`: a new, unsaved project from a bundled template, with a
    /// fresh project ID and the default C++ standard from the settings. It is
    /// trusted (*created here*) and has no file until it is saved with
    /// `project_save_as_dialog`.
    ///
    /// # Errors
    /// [`IpcError::TooManyHandles`]; [`IpcError::Internal`] when the OS random
    /// number generator fails.
    pub fn project_new(&self, request: ProjectNewRequest) -> Result<ProjectNewResponse, IpcError> {
        let _span = command_span("project_new");
        self.projects.ensure_room()?;
        let standard = self.settings.get().new_project.standard;
        let document = templates::instantiate(request.template, standard, self.app_version)?;
        let text = b2c_model::to_canonical_json(&document);
        let entry = ProjectEntry {
            path: None,
            project_id: document.project.id.clone(),
            baseline_sha256: None,
            latest_bytes_len: text.len(),
            latest_document: Arc::new(document),
            created_here: true,
            trust: HandleTrust::CreatedHere,
            mark_of_the_web: false,
            dirty: false,
            last_trust_grant: None,
        };
        let trust = entry.trust.to_dto(false);
        let handle = self.projects.insert(entry)?;
        Ok(ProjectNewResponse {
            handle,
            document: text,
            trust,
        })
    }

    /// `project_open_dialog`: opens the `*.b2c` file the user picks in the
    /// native open dialog.
    ///
    /// # Errors
    /// [`IpcError::TooManyHandles`] (before any dialog), [`IpcError::Busy`],
    /// and the errors of opening (`notFound`, `payloadTooLarge`,
    /// `newerFormat`, `invalidDocument`, `io`).
    pub fn project_open_dialog(&self) -> Result<ProjectOpenDialogResponse, IpcError> {
        let _span = command_span("project_open_dialog");
        self.projects.ensure_room()?;
        let picked = {
            let _dialog = self.dialog_gate.enter()?;
            self.dialogs.open_project()
        };
        let Some(path) = picked else {
            return Ok(ProjectOpenDialogResponse::Cancelled);
        };
        self.open_file(&path).map(ProjectOpenDialogResponse::Ok)
    }

    /// `project_open_recent`: opens the project of a recent-list entry, the
    /// same way as the open dialog. A file that no longer exists gives
    /// `notFound` and stays in the list.
    ///
    /// # Errors
    /// [`IpcError::UnknownRecent`], and the errors of opening.
    pub fn project_open_recent(&self, request: ProjectOpenRecentRequest) -> Result<ProjectOpened, IpcError> {
        let _span = command_span("project_open_recent");
        let path = self.recent_path(&request.recent_id)?;
        self.open_file(&path)
    }

    /// The path recorded under a recent ID.
    fn recent_path(&self, id: &RecentId) -> Result<PathBuf, IpcError> {
        self.recent.path_of(id.as_str()).ok_or(IpcError::UnknownRecent)
    }

    /// `project_reload`: reads the project's file again (after it changed
    /// outside the app), with the full open flow, trust re-check included.
    /// The project's unsaved changes are discarded.
    ///
    /// # Errors
    /// [`IpcError::UnknownHandle`], [`IpcError::NoPath`] for a project that was
    /// never saved, and the errors of opening.
    pub fn project_reload(&self, request: ProjectReloadRequest) -> Result<ProjectReloadResponse, IpcError> {
        let _span = command_span("project_reload");
        let entry_ref = self.projects.get(&request.handle)?;
        let mut entry = lock(&entry_ref);
        let path = entry.path.clone().ok_or(IpcError::NoPath)?;
        let loaded = self.load_file(&path)?;
        let text = b2c_model::to_canonical_json(&loaded.document);
        entry.set_latest(loaded.document, text.len());
        entry.path = Some(loaded.path.clone());
        entry.baseline_sha256 = Some(loaded.baseline);
        entry.trust = loaded.trust;
        entry.created_here = false;
        entry.mark_of_the_web = loaded.mark_of_the_web;
        entry.dirty = false;
        let trust = entry.trust.to_dto(entry.mark_of_the_web);
        let project_name = entry.latest_document.project.name.clone();
        drop(entry);
        self.watchers
            .watch(&request.handle, &loaded.path, loaded.baseline);
        self.touch_recent(&loaded.path, &project_name);
        Ok(ProjectReloadResponse {
            document: text,
            trust,
            migrated_from: loaded.migrated_from,
        })
    }

    /// `project_save`: writes the document to the project's own file
    /// (`docs/spec/05-project-format.md` §5.10): validated with the strict
    /// loader, refused when the file changed on disk since it was opened or
    /// last saved, written in canonical form atomically with the previous
    /// version kept as `<name>.b2c.bak`. The new bytes' hash becomes the
    /// baseline, the recovery snapshot is deleted, and for a trusted project
    /// the trust record follows the new content (the first save of a project
    /// created here records it).
    ///
    /// # Errors
    /// The document's errors (`payloadTooLarge`, `invalidDocument`,
    /// `newerFormat`) before anything else; [`IpcError::UnknownHandle`];
    /// [`IpcError::NoPath`] for a project that was never saved (the frontend
    /// uses `project_save_as_dialog`); [`IpcError::ChangedOnDisk`] when the
    /// file changed or disappeared outside the app (it is not touched);
    /// [`IpcError::Io`].
    pub fn project_save(&self, request: ProjectSaveRequest) -> Result<ProjectSaveResponse, IpcError> {
        let _span = command_span("project_save");
        let document = b2c_ipc::parse_document(&request.document)?;
        let bytes = canonical_bytes(&document)?;
        let entry_ref = self.projects.get(&request.handle)?;
        let mut entry = lock(&entry_ref);
        let path = entry.path.clone().ok_or(IpcError::NoPath)?;
        let baseline = entry.baseline_sha256.ok_or(IpcError::Internal)?;
        check_unchanged(&path, &baseline)?;
        save_project(&path, &bytes).map_err(|error| store_error("save a project", &error))?;
        let hash = sha256(&bytes);
        let saved_at = rfc3339_utc(SystemTime::now());
        self.record_trust_after_save(&mut entry, &document, &path);
        entry.set_latest(document, bytes.len());
        entry.baseline_sha256 = Some(hash);
        entry.dirty = false;
        drop(entry);
        self.watchers.watch(&request.handle, &path, hash);
        self.discard_snapshot_of(&request.handle);
        Ok(ProjectSaveResponse {
            saved_at,
            hash: b2c_model::hex(&hash),
        })
    }

    /// After `document` was saved at `path`: a trusted project's record
    /// follows the saved content ("edits inside the app update the record");
    /// a project created here gets its first record. A restricted project's
    /// trust is never changed. A trust store that cannot be written is
    /// logged and leaves the project's trust in memory as it was.
    fn record_trust_after_save(&self, entry: &mut ProjectEntry, document: &Document, path: &Path) {
        let identity = identity(document, path);
        match entry.trust {
            HandleTrust::Restricted(_) => {}
            HandleTrust::CreatedHere => match self.trust.grant_project(&identity) {
                Ok(()) => {
                    entry.trust = HandleTrust::Trusted(b2c_store::TrustSource::Project);
                    entry.created_here = false;
                }
                Err(error) => {
                    let _ = store_error("record trust at the first save", &error);
                    tracing::warn!(
                        "the trust of a new project could not be recorded; it is trusted until it closes"
                    );
                }
            },
            HandleTrust::Trusted(_) => {
                if let Err(error) = self.trust.record_save(&identity) {
                    let _ = store_error("record a save in the trust store", &error);
                    tracing::warn!("the trust store could not record a save");
                }
            }
        }
    }

    /// `project_save_as_dialog`: asks for a new file in the native save
    /// dialog, writes the document there and rebinds the project to it. The
    /// old file is not touched. A trusted project gets trust recorded for the
    /// new file; a restricted one stays restricted.
    ///
    /// # Errors
    /// The document's errors before anything else; [`IpcError::UnknownHandle`];
    /// [`IpcError::Busy`]; [`IpcError::Io`] when the file cannot be written
    /// (the project stays bound to its old file).
    pub fn project_save_as_dialog(
        &self,
        request: ProjectSaveAsDialogRequest,
    ) -> Result<ProjectSaveAsDialogResponse, IpcError> {
        let _span = command_span("project_save_as_dialog");
        let document = b2c_ipc::parse_document(&request.document)?;
        let bytes = canonical_bytes(&document)?;
        let suggested = {
            let entry_ref = self.projects.get(&request.handle)?;
            let entry = lock(&entry_ref);
            suggested_file_name(entry.path.as_deref(), &document.project.name)
        };
        let picked = {
            let _dialog = self.dialog_gate.enter()?;
            self.dialogs.save_project_as(&suggested)
        };
        let Some(picked) = picked else {
            return Ok(ProjectSaveAsDialogResponse::Cancelled);
        };
        let target = save_target(&picked)?;
        // The project may have been closed while the dialog was open.
        let entry_ref = self.projects.get(&request.handle)?;
        let mut entry = lock(&entry_ref);
        save_project(&target, &bytes).map_err(|error| store_error("save a project as", &error))?;
        let path = canonical_path(&target).unwrap_or(target);
        let hash = sha256(&bytes);
        let saved_at = rfc3339_utc(SystemTime::now());
        let trust = match entry.trust {
            HandleTrust::Restricted(_) => HandleTrust::Restricted(b2c_store::RestrictedReason::NoRecord),
            current => match self.trust.grant_project(&identity(&document, &path)) {
                Ok(()) => HandleTrust::Trusted(b2c_store::TrustSource::Project),
                Err(error) => {
                    let _ = store_error("record trust for a saved-as project", &error);
                    tracing::warn!("the trust of a saved-as project could not be recorded");
                    // A project created here stays trusted in memory, and its
                    // next save tries again; any other is restricted at its
                    // new path.
                    if current == HandleTrust::CreatedHere {
                        current
                    } else {
                        HandleTrust::Restricted(b2c_store::RestrictedReason::NoRecord)
                    }
                }
            },
        };
        entry.set_latest(document, bytes.len());
        entry.path = Some(path.clone());
        entry.baseline_sha256 = Some(hash);
        entry.trust = trust;
        entry.created_here = trust == HandleTrust::CreatedHere;
        entry.dirty = false;
        let project_name = entry.latest_document.project.name.clone();
        drop(entry);
        self.watchers.unwatch(&request.handle);
        self.watchers.watch(&request.handle, &path, hash);
        self.touch_recent(&path, &project_name);
        self.discard_snapshot_of(&request.handle);
        Ok(ProjectSaveAsDialogResponse::Ok(ProjectSavedAs {
            handle: request.handle.clone(),
            saved_at,
            hash: b2c_model::hex(&hash),
            file_name: file_name(&path),
        }))
    }

    /// `project_close`: cancels the project's build, stops its program
    /// (killing the process tree), stops watching its file, deletes its
    /// recovery snapshot and forgets the handle; its build IDs become unknown.
    ///
    /// # Errors
    /// [`IpcError::UnknownHandle`].
    pub fn project_close(&self, request: ProjectCloseRequest) -> Result<Empty, IpcError> {
        let _span = command_span("project_close");
        self.projects
            .remove(&request.handle)
            .ok_or(IpcError::UnknownHandle)?;
        let key = request.handle.as_str();
        self.builds.forget_project(key);
        self.runs.stop_project(key);
        self.watchers.unwatch(&request.handle);
        self.discard_snapshot_of(&request.handle);
        Ok(Empty {})
    }

    /// `project_set_dirty`: records whether the project has unsaved changes,
    /// so closing the window asks first.
    ///
    /// # Errors
    /// [`IpcError::UnknownHandle`].
    pub fn project_set_dirty(&self, request: ProjectSetDirtyRequest) -> Result<Empty, IpcError> {
        let _span = command_span("project_set_dirty");
        let entry = self.projects.get(&request.handle)?;
        lock(&entry).dirty = request.dirty;
        Ok(Empty {})
    }
}

/// Fails unless the file at `path` still has the `baseline` hash: a file
/// changed, replaced or deleted outside the app is never overwritten.
fn check_unchanged(path: &Path, baseline: &[u8; 32]) -> Result<(), IpcError> {
    match read_project(path) {
        Ok(bytes) if sha256(&bytes) == *baseline => Ok(()),
        Ok(_) | Err(ReadError::NotFound | ReadError::TooLarge { .. } | ReadError::NotAFile) => {
            tracing::debug!(path = %path.display(), "the project file changed outside the app; not saved");
            Err(IpcError::ChangedOnDisk)
        }
        Err(ReadError::Io(error)) => Err(io_error("read a project file before saving", path, &error)),
    }
}

/// The file to write for a path picked in the save dialog: its folder must
/// exist (it is canonicalised) and the name must be a plain file name.
fn save_target(picked: &Path) -> Result<PathBuf, IpcError> {
    let name = picked
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            tracing::debug!(path = %picked.display(), "the save dialog returned a path without a file name");
            IpcError::Io {
                kind: b2c_ipc::IoKind::Other,
            }
        })?;
    let folder = picked
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .ok_or(IpcError::Io {
            kind: b2c_ipc::IoKind::Other,
        })?;
    let folder = canonical_path(folder).map_err(|error| store_error("find the folder to save in", &error))?;
    Ok(folder.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrated_versions_are_older_ones_only() {
        assert_eq!(migrated_from(br#"{"formatVersion": 1, "x": [1]}"#), None);
        assert_eq!(migrated_from(br#"{"formatVersion": 0}"#), Some(0));
        assert_eq!(migrated_from(br#"{"formatVersion": 7}"#), None);
        assert_eq!(migrated_from(b"not json"), None);
    }

    #[test]
    fn file_names_are_for_display() {
        assert_eq!(file_name(Path::new("/home/ada/game.b2c")), "game.b2c");
        assert_eq!(file_name(Path::new("/")), "");
    }

    #[test]
    fn save_targets_need_an_existing_folder_and_a_name() {
        let dir = tempfile::tempdir().unwrap();
        let target = save_target(&dir.path().join("game.b2c")).unwrap();
        assert_eq!(target, canonical_path(dir.path()).unwrap().join("game.b2c"));
        assert!(matches!(
            save_target(&dir.path().join("missing").join("game.b2c")),
            Err(IpcError::Io { .. })
        ));
        assert!(matches!(
            save_target(Path::new("game.b2c")),
            Err(IpcError::Io { .. })
        ));
        assert!(matches!(save_target(Path::new("/")), Err(IpcError::Io { .. })));
    }

    #[test]
    fn changed_files_are_detected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.b2c");
        std::fs::write(&path, b"one").unwrap();
        let baseline = sha256(b"one");
        check_unchanged(&path, &baseline).unwrap();
        std::fs::write(&path, b"two").unwrap();
        assert_eq!(check_unchanged(&path, &baseline), Err(IpcError::ChangedOnDisk));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(check_unchanged(&path, &baseline), Err(IpcError::ChangedOnDisk));
        std::fs::create_dir(&path).unwrap();
        assert_eq!(check_unchanged(&path, &baseline), Err(IpcError::ChangedOnDisk));
    }
}
