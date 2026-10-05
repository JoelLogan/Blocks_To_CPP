//! The trust store: `trust.json` in the machine folder, and the evaluation
//! that decides whether a project may be built and run
//! (`docs/spec/05-project-format.md` §5.9, `docs/spec/08-security.md` §8.1
//! and §8.3).
//!
//! ```json
//! {
//!   "format": "blocks2cpp/trust",
//!   "formatVersion": 1,
//!   "projects": [
//!     {
//!       "projectId": "prj_4kq9Xb2LmT7pRz1s",
//!       "canonicalPath": "/home/ada/games/guess.b2c",
//!       "rawCodeHashAtGrant": "<64 lower-case hex digits>",
//!       "grantedAt": "2026-10-05T09:30:00.000Z"
//!     }
//!   ],
//!   "folders": [
//!     { "canonicalPath": "/home/ada/games", "grantedAt": "2026-10-05T09:31:00.000Z" }
//!   ]
//! }
//! ```
//!
//! CONTRACT (milestone M2):
//! * **Evaluation** ([`TrustStore::evaluate`], 08 §8.3.1). A project record
//!   applies only when both the project ID and the canonical path are equal
//!   (paths compared by their parts, ignoring letter case on Windows). The
//!   project is trusted ([`TrustSource::Project`]) when the security hash
//!   (`b2c_model::security_hash`) is equal too. A folder record covers the
//!   folder and everything below it, compared by path parts, with no hash
//!   ([`TrustSource::Folder`]), so an outside change does not re-flag a
//!   project there. Otherwise a project whose ID and path match a record
//!   with another hash changed outside the app
//!   ([`RestrictedReason::ChangedOutside`]), and anything else has no record
//!   ([`RestrictedReason::NoRecord`]): a copy of a trusted file at another
//!   path, a different project saved over a trusted path, a path that is
//!   relative or has `.` or `..` parts. *Created here* (a new project that
//!   was never saved) is decided by the app, not by the store.
//! * **Fail closed** (05 §5.9). The file is read with a 4 MiB bound
//!   ([`MAX_TRUST_BYTES`]) and validated strictly: the format tag, version 1,
//!   no unknown or duplicate keys, valid project IDs, absolute canonical
//!   paths, 64 lower-case hex digits per hash, RFC 3339 times, at most one
//!   record per path and the record limits. A missing, oversized or invalid
//!   file, or one written by a newer version, means nothing is trusted
//!   ([`TrustStore::problem`] says why, for the app's warning log), and the
//!   next grant replaces it with a valid file. A file that cannot be read at
//!   all (an I/O error, or not a regular file) also trusts nothing, but is
//!   never overwritten: changes fail with the error instead.
//! * **Only the backend writes it** (08 §8.3.1), through
//!   [`TrustStore::grant_project`] (after the native trust dialog, and at the
//!   first save of a project created in the app),
//!   [`TrustStore::record_save`] (a trusted project saved in the app),
//!   [`TrustStore::grant_folder`] and [`TrustStore::revoke_project`]. Every
//!   change re-reads the file, applies itself on top and writes the result
//!   atomically ([`write_atomic`], `0600` in a `0700` folder), while holding
//!   an exclusive lock on `trust.json.lock` next to it, so app instances
//!   running at the same time never undo each other's grants or revocations.
//!   Every evaluation reads the file again, so a revocation in one instance
//!   holds in the others at their next open.
//! * **Bounded.** At most [`MAX_TRUSTED_PROJECTS`] project records and
//!   [`MAX_TRUSTED_FOLDERS`] folder records, with paths of at most
//!   [`MAX_TRUST_PATH_BYTES`]; when a grant goes over a limit (or the file
//!   would go over [`MAX_TRUST_BYTES`]), the records granted longest ago are
//!   dropped, so the file always stays readable. Dropping trust is safe: the
//!   user is asked again.
//! * **Paths** must be valid Unicode to be recorded
//!   ([`StoreError::PathNotUnicode`]), absolute, without `.` or `..` parts,
//!   and canonical (`crate::canonical_path`); the store cannot check the last
//!   rule without touching the disk, so callers pass canonical paths. A whole
//!   drive or the file-system root cannot be trusted as a folder.
//!
//! Who may change trust is not this module's concern: the app calls the
//! changing methods only after the native dialog or a save in the app, and
//! calls [`TrustStore::record_save`] only for a project that is trusted
//! (for a restricted one it would turn an outside change into trust).
//! Nothing here logs; no error message contains a path (08 §8.11).

use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use b2c_ir::ProjectId;
use serde::{Deserialize, Serialize};

use crate::atomic::{Backup, write_atomic};
use crate::dirs::ensure_private_dir;
use crate::error::{ReadError, StoreError};
use crate::fs_checks::is_link;
use crate::ids::{hex_lower, is_lower_hex};
use crate::read::read_bounded;
use crate::time::{parse_rfc3339_utc, rfc3339_utc};

/// The trust store's file name in the machine folder (`Dirs::machine`).
pub const TRUST_FILE: &str = "trust.json";
/// The `format` value of the trust store.
pub const TRUST_FORMAT: &str = "blocks2cpp/trust";
/// The `formatVersion` this version reads and writes.
pub const TRUST_FORMAT_VERSION: u64 = 1;
/// The largest trust store that is read, in bytes (4 MiB). Writes never go
/// over it.
pub const MAX_TRUST_BYTES: u64 = 4 * 1024 * 1024;
/// The most project records the store keeps; a grant beyond it drops the
/// record granted longest ago.
pub const MAX_TRUSTED_PROJECTS: usize = 10_000;
/// The most folder records the store keeps; a grant beyond it drops the
/// folder granted longest ago.
pub const MAX_TRUSTED_FOLDERS: usize = 1_000;
/// The longest path that is recorded, in bytes of UTF-8.
pub const MAX_TRUST_PATH_BYTES: usize = 8 * 1024;
/// How long a change waits for another app instance to finish its own.
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a change sleeps between two attempts to take the lock.
const LOCK_RETRY: Duration = Duration::from_millis(10);

/// What the trust of a project is evaluated for: the project's ID, the
/// canonical path of its file and its security hash
/// (`b2c_model::security_hash` of the document).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectIdentity {
    /// The project's ID (`project.id` in the file).
    pub project_id: ProjectId,
    /// The canonical path of the project file (`crate::canonical_path`).
    pub canonical_path: PathBuf,
    /// The security hash of the document.
    pub security_hash: [u8; 32],
}

/// Which record makes a project trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrustSource {
    /// A record for this project at this path, with the same security hash.
    Project,
    /// A trusted folder that contains the project file.
    Folder,
}

/// Why a project is in Restricted Mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RestrictedReason {
    /// No record applies: the project was never trusted at this path.
    NoRecord,
    /// The project has a record at this path, but its security-relevant
    /// content (Raw C++, libraries, packs, defines) changed outside the app.
    ChangedOutside,
}

/// The result of [`TrustStore::evaluate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrustVerdict {
    /// Building and running are allowed.
    Trusted(TrustSource),
    /// Restricted Mode: the project can be edited but not built or run.
    Restricted(RestrictedReason),
}

impl TrustVerdict {
    /// Whether the verdict allows building and running.
    pub fn is_trusted(self) -> bool {
        matches!(self, Self::Trusted(_))
    }
}

/// Why the trust store file was not used (so nothing is trusted). The app
/// logs it as a warning; the texts contain no path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TrustFileProblem {
    /// The file is larger than [`MAX_TRUST_BYTES`].
    #[error("the trust store is larger than its 4 MiB limit, so nothing is trusted")]
    TooLarge,
    /// A folder, device or other non-file is where the file should be.
    #[error("the trust store is not a regular file, so nothing is trusted")]
    NotAFile,
    /// The file could not be read.
    #[error("the trust store could not be read ({kind}), so nothing is trusted")]
    Unreadable {
        /// The kind of I/O error.
        kind: io::ErrorKind,
    },
    /// The file is not JSON, not a trust store, or breaks one of its rules.
    #[error("the trust store is not valid, so nothing is trusted")]
    Invalid,
    /// The file was written by a newer version of Blocks2Cpp.
    #[error("the trust store was written by a newer version of Blocks2Cpp, so nothing is trusted")]
    NewerVersion,
}

/// `trust.json` (see the module documentation). Cheap to share: every
/// method takes `&self`, and the struct is `Send` and `Sync`.
#[derive(Debug)]
pub struct TrustStore {
    /// The file.
    path: PathBuf,
    /// Serialises this process's changes (other processes are kept out by
    /// the lock file).
    changes: Mutex<()>,
    /// What was wrong with the file when it was last read.
    problem: Mutex<Option<TrustFileProblem>>,
}

impl TrustStore {
    /// Opens the trust store at `path` (normally `<Dirs::machine>/trust.json`,
    /// [`TRUST_FILE`]) and reads it once. Never fails and creates nothing: a
    /// missing file trusts nothing, and so does an oversized or invalid one,
    /// which [`TrustStore::problem`] then reports for the warning log.
    pub fn open(path: &Path) -> Self {
        let store = Self {
            path: path.to_path_buf(),
            changes: Mutex::new(()),
            problem: Mutex::new(None),
        };
        // Only for the problem it records.
        let _ = store.current();
        store
    }

    /// The trust store's path (for the debug log).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What was wrong with the file when it was last read (by
    /// [`TrustStore::open`], an evaluation or a change), or `None` when it
    /// was valid or missing.
    pub fn problem(&self) -> Option<TrustFileProblem> {
        *self.problem.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether the project may be built and run (see the module
    /// documentation for the rules). Reads the file again, so changes made
    /// by other app instances count; when it cannot be used, nothing is
    /// trusted.
    pub fn evaluate(&self, p: &ProjectIdentity) -> TrustVerdict {
        self.current().verdict(p)
    }

    /// The trusted folder that contains `path` (or is `path`), as recorded;
    /// the outermost one when several do. `None` when no folder covers it,
    /// and for a relative path or one with `.` or `..` parts.
    pub fn folder_covering(&self, path: &Path) -> Option<PathBuf> {
        let key = PathKey::of(path)?;
        self.current()
            .covering_folder(&key)
            .map(|folder| PathBuf::from(&folder.path))
    }

    /// Records that the user trusts this project at this path with this
    /// security hash (after the native trust dialog, or at the first save of
    /// a project created in the app). A record for the same path is replaced,
    /// whatever its project ID: a path holds one project at a time.
    ///
    /// # Errors
    /// [`StoreError::PathNotUnicode`] for a path that is not valid Unicode;
    /// [`StoreError::Invalid`] for a relative path, one with `.` or `..`
    /// parts, or one longer than [`MAX_TRUST_PATH_BYTES`]; otherwise the
    /// errors of reading, locking and writing the file (see
    /// [`TrustStore::revoke_project`]). Nothing is changed on error.
    pub fn grant_project(&self, p: &ProjectIdentity) -> Result<(), StoreError> {
        let (path, key) = recordable(&p.canonical_path)?;
        let now = SystemTime::now();
        self.change(|records| {
            records.projects.retain(|record| record.key != key);
            records.make_room(Kind::Project, MAX_TRUSTED_PROJECTS - 1);
            records.projects.push(ProjectRecord {
                project_id: p.project_id.clone(),
                path,
                key: key.clone(),
                hash: p.security_hash,
                granted_at: rfc3339_utc(now),
                granted: now,
            });
            Ok(((), Some(Keep::Project(key))))
        })
    }

    /// Records that the user trusts everything in `folder` (its canonical
    /// path), recursively. Granting a folder that is already trusted only
    /// renews its time.
    ///
    /// # Errors
    /// As [`TrustStore::grant_project`], and [`StoreError::Invalid`] for a
    /// whole drive or the file-system root, which cannot be trusted.
    pub fn grant_folder(&self, folder: &Path) -> Result<(), StoreError> {
        let (path, key) = recordable(folder)?;
        if folder.parent().is_none() {
            return Err(StoreError::Invalid(
                "a whole drive or the file system's root folder cannot be trusted",
            ));
        }
        let now = SystemTime::now();
        self.change(|records| {
            records.folders.retain(|record| record.key != key);
            records.make_room(Kind::Folder, MAX_TRUSTED_FOLDERS - 1);
            records.folders.push(FolderRecord {
                path,
                key: key.clone(),
                granted_at: rfc3339_utc(now),
                granted: now,
            });
            Ok(((), Some(Keep::Folder(key))))
        })
    }

    /// Records a save in the app: when there is a record for this project
    /// ID at this path, its hash becomes `p.security_hash` (its grant time
    /// stays). Returns whether there was such a record; without one nothing
    /// is written (a project trusted through its folder needs none).
    ///
    /// Call it only for a project that is trusted: for a restricted project
    /// whose content changed outside the app, it would make that content
    /// trusted.
    ///
    /// # Errors
    /// As [`TrustStore::revoke_project`].
    pub fn record_save(&self, p: &ProjectIdentity) -> Result<bool, StoreError> {
        let Some(key) = PathKey::of(&p.canonical_path) else {
            return Ok(false);
        };
        self.change(|records| {
            let Some(record) = records.project_mut(&p.project_id, &key) else {
                return Ok((false, None));
            };
            if record.hash == p.security_hash {
                return Ok((true, None));
            }
            record.hash = p.security_hash;
            Ok((true, Some(Keep::Project(key.clone()))))
        })
    }

    /// Removes the record of project `id` at `canonical_path` and returns
    /// whether there was one. Folder records are never touched, so a project
    /// in a trusted folder stays trusted (see [`TrustStore::folder_covering`]).
    ///
    /// # Errors
    /// [`StoreError::Link`] or [`StoreError::Io`] when the existing file
    /// cannot be read at all (it is then left alone);
    /// [`StoreError::Io`] when the lock cannot be taken within 5 seconds
    /// (kind `TimedOut`) or the file cannot be written; otherwise as
    /// [`write_atomic`]. Nothing is changed on error.
    pub fn revoke_project(&self, id: &ProjectId, canonical_path: &Path) -> Result<bool, StoreError> {
        let Some(key) = PathKey::of(canonical_path) else {
            return Ok(false);
        };
        self.change(|records| {
            let before = records.projects.len();
            records
                .projects
                .retain(|record| !(record.project_id == *id && record.key == key));
            if records.projects.len() == before {
                Ok((false, None))
            } else {
                Ok((true, Some(Keep::Nothing)))
            }
        })
    }

    /// The records on disk now; none when the file cannot be used (and the
    /// problem is recorded).
    fn current(&self) -> Records {
        let loaded = load(&self.path);
        self.set_problem(loaded.as_ref().err().map(|failure| failure.problem));
        loaded.unwrap_or_default()
    }

    /// Applies `apply` to the records on disk and writes the result, under
    /// both locks. `apply` returns its result and, when the records changed,
    /// which record must survive [`Records::fit`]; `None` writes nothing.
    fn change<T>(
        &self,
        apply: impl FnOnce(&mut Records) -> Result<(T, Option<Keep>), StoreError>,
    ) -> Result<T, StoreError> {
        let _changes = self.changes.lock().unwrap_or_else(PoisonError::into_inner);
        let dir = self
            .path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .ok_or(StoreError::Invalid("the trust store's path has no folder"))?;
        ensure_private_dir(dir)?;
        let _lock = FileLock::acquire(&lock_path(&self.path)?)?;
        let mut records = match load(&self.path) {
            Ok(records) => {
                self.set_problem(None);
                records
            }
            Err(failure) => {
                self.set_problem(Some(failure.problem));
                if let Some(error) = failure.unreadable {
                    return Err(error);
                }
                // Corrupt, oversized or newer: replaced by the next write.
                Records::default()
            }
        };
        let (value, keep) = apply(&mut records)?;
        if let Some(keep) = keep {
            let bytes = records.fit(&keep)?;
            write_atomic(&self.path, &bytes, Backup::None)?;
            self.set_problem(None);
        }
        Ok(value)
    }

    fn set_problem(&self, problem: Option<TrustFileProblem>) {
        *self.problem.lock().unwrap_or_else(PoisonError::into_inner) = problem;
    }
}

/// The lock file of the trust store at `path`: `trust.json.lock`.
fn lock_path(path: &Path) -> Result<PathBuf, StoreError> {
    let name = path
        .file_name()
        .ok_or(StoreError::Invalid("the trust store's path has no file name"))?;
    let mut lock = name.to_os_string();
    lock.push(".lock");
    Ok(path.with_file_name(lock))
}

/// An exclusive lock on the trust store's lock file, held by every change
/// across all app instances; released when dropped.
struct FileLock(File);

impl FileLock {
    /// Opens (creating it `0600` if needed) and locks `path`, waiting at
    /// most [`LOCK_TIMEOUT`] for another instance to release it. A link or
    /// non-file at `path` is refused.
    fn acquire(path: &Path) -> Result<Self, StoreError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if is_link(&metadata) || !metadata.is_file() => {
                return Err(StoreError::link(path));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(StoreError::io("inspect the trust store's lock", path, source)),
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options
            .open(path)
            .map_err(|source| StoreError::io("open the trust store's lock", path, source))?;
        let metadata = file
            .metadata()
            .map_err(|source| StoreError::io("inspect the trust store's lock", path, source))?;
        if !metadata.is_file() {
            return Err(StoreError::link(path));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            if metadata.permissions().mode() & 0o777 != 0o600 {
                file.set_permissions(fs::Permissions::from_mode(0o600))
                    .map_err(|source| StoreError::io("make the trust store's lock private", path, source))?;
            }
        }
        let deadline = Instant::now() + LOCK_TIMEOUT;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self(file)),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => thread::sleep(LOCK_RETRY),
                Err(TryLockError::WouldBlock) => {
                    return Err(StoreError::io(
                        "lock the trust store",
                        path,
                        io::ErrorKind::TimedOut.into(),
                    ));
                }
                Err(TryLockError::Error(source)) => {
                    return Err(StoreError::io("lock the trust store", path, source));
                }
            }
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // Closing the file releases the lock too; unlocking first releases
        // it at once on Windows.
        let _ = self.0.unlock();
    }
}

/// Why [`load`] returned no records.
#[derive(Debug)]
struct LoadFailure {
    /// What is wrong, for [`TrustStore::problem`].
    problem: TrustFileProblem,
    /// For a file that could not be read at all: the error a change reports
    /// instead of overwriting it.
    unreadable: Option<StoreError>,
}

impl LoadFailure {
    /// A file that was read but cannot be used.
    fn unusable(problem: TrustFileProblem) -> Self {
        Self {
            problem,
            unreadable: None,
        }
    }
}

/// Reads and validates the file. A missing file is an empty store.
fn load(path: &Path) -> Result<Records, LoadFailure> {
    let bytes = match read_bounded(path, MAX_TRUST_BYTES) {
        Ok(bytes) => bytes,
        Err(ReadError::NotFound) => return Ok(Records::default()),
        Err(ReadError::TooLarge { .. }) => return Err(LoadFailure::unusable(TrustFileProblem::TooLarge)),
        Err(error @ ReadError::NotAFile) => {
            return Err(LoadFailure {
                problem: TrustFileProblem::NotAFile,
                unreadable: Some(error.at(path)),
            });
        }
        Err(ReadError::Io(source)) => {
            return Err(LoadFailure {
                problem: TrustFileProblem::Unreadable { kind: source.kind() },
                unreadable: Some(StoreError::io("read the trust store", path, source)),
            });
        }
    };
    parse(&bytes).map_err(LoadFailure::unusable)
}

/// The header, read first so that a newer version is told apart from an
/// invalid file.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    format: String,
    format_version: u64,
}

/// The file, strictly.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FileJson {
    format: String,
    format_version: u64,
    projects: Vec<ProjectJson>,
    folders: Vec<FolderJson>,
}

/// A project record in the file.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectJson {
    project_id: String,
    canonical_path: String,
    raw_code_hash_at_grant: String,
    granted_at: String,
}

/// A folder record in the file.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FolderJson {
    canonical_path: String,
    granted_at: String,
}

/// Parses and validates the file's bytes (see the module documentation).
fn parse(bytes: &[u8]) -> Result<Records, TrustFileProblem> {
    let header: Header = serde_json::from_slice(bytes).map_err(|_| TrustFileProblem::Invalid)?;
    if header.format != TRUST_FORMAT {
        return Err(TrustFileProblem::Invalid);
    }
    if header.format_version > TRUST_FORMAT_VERSION {
        return Err(TrustFileProblem::NewerVersion);
    }
    if header.format_version != TRUST_FORMAT_VERSION {
        return Err(TrustFileProblem::Invalid);
    }
    let file: FileJson = serde_json::from_slice(bytes).map_err(|_| TrustFileProblem::Invalid)?;
    if file.projects.len() > MAX_TRUSTED_PROJECTS || file.folders.len() > MAX_TRUSTED_FOLDERS {
        return Err(TrustFileProblem::Invalid);
    }
    let mut records = Records::default();
    for project in file.projects {
        let record = ProjectRecord::from_json(project).ok_or(TrustFileProblem::Invalid)?;
        if records.projects.iter().any(|seen| seen.key == record.key) {
            return Err(TrustFileProblem::Invalid);
        }
        records.projects.push(record);
    }
    for folder in file.folders {
        let record = FolderRecord::from_json(folder).ok_or(TrustFileProblem::Invalid)?;
        if records.folders.iter().any(|seen| seen.key == record.key) {
            return Err(TrustFileProblem::Invalid);
        }
        records.folders.push(record);
    }
    Ok(records)
}

/// A project record.
#[derive(Debug, Clone)]
struct ProjectRecord {
    project_id: ProjectId,
    /// The canonical path as recorded.
    path: String,
    /// [`PathKey`] of `path`.
    key: PathKey,
    hash: [u8; 32],
    /// `grantedAt` as recorded.
    granted_at: String,
    /// `grantedAt` parsed.
    granted: SystemTime,
}

impl ProjectRecord {
    /// A record from the file, if it is valid.
    fn from_json(json: ProjectJson) -> Option<Self> {
        let key = valid_path(&json.canonical_path)?;
        Some(Self {
            project_id: ProjectId::new(&json.project_id).ok()?,
            key,
            hash: parse_hash(&json.raw_code_hash_at_grant)?,
            granted: parse_rfc3339_utc(&json.granted_at)?,
            path: json.canonical_path,
            granted_at: json.granted_at,
        })
    }

    fn to_json(&self) -> ProjectJson {
        ProjectJson {
            project_id: self.project_id.as_str().to_owned(),
            canonical_path: self.path.clone(),
            raw_code_hash_at_grant: hex_lower(&self.hash),
            granted_at: self.granted_at.clone(),
        }
    }
}

/// A folder record.
#[derive(Debug, Clone)]
struct FolderRecord {
    /// The canonical path as recorded.
    path: String,
    /// [`PathKey`] of `path`.
    key: PathKey,
    /// `grantedAt` as recorded.
    granted_at: String,
    /// `grantedAt` parsed.
    granted: SystemTime,
}

impl FolderRecord {
    /// A record from the file, if it is valid.
    fn from_json(json: FolderJson) -> Option<Self> {
        let key = valid_path(&json.canonical_path)?;
        // Never a whole drive or the root (see `TrustStore::grant_folder`).
        Path::new(&json.canonical_path).parent()?;
        Some(Self {
            key,
            granted: parse_rfc3339_utc(&json.granted_at)?,
            path: json.canonical_path,
            granted_at: json.granted_at,
        })
    }

    fn to_json(&self) -> FolderJson {
        FolderJson {
            canonical_path: self.path.clone(),
            granted_at: self.granted_at.clone(),
        }
    }
}

/// The record a change made, which [`Records::fit`] must not drop.
#[derive(Debug, Clone)]
enum Keep {
    /// A change that only removed records.
    Nothing,
    /// The project record at this path.
    Project(PathKey),
    /// The folder record at this path.
    Folder(PathKey),
}

/// The two kinds of record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Project,
    Folder,
}

/// The records of the store.
#[derive(Debug, Clone, Default)]
struct Records {
    projects: Vec<ProjectRecord>,
    folders: Vec<FolderRecord>,
}

impl Records {
    /// The verdict for `p` (see the module documentation).
    fn verdict(&self, p: &ProjectIdentity) -> TrustVerdict {
        let Some(key) = PathKey::of(&p.canonical_path) else {
            return TrustVerdict::Restricted(RestrictedReason::NoRecord);
        };
        let record = self
            .projects
            .iter()
            .find(|record| record.project_id == p.project_id && record.key == key);
        if record.is_some_and(|record| record.hash == p.security_hash) {
            TrustVerdict::Trusted(TrustSource::Project)
        } else if self.covering_folder(&key).is_some() {
            TrustVerdict::Trusted(TrustSource::Folder)
        } else if record.is_some() {
            TrustVerdict::Restricted(RestrictedReason::ChangedOutside)
        } else {
            TrustVerdict::Restricted(RestrictedReason::NoRecord)
        }
    }

    /// The outermost folder record that covers `key`.
    fn covering_folder(&self, key: &PathKey) -> Option<&FolderRecord> {
        self.folders
            .iter()
            .filter(|folder| key.is_within(&folder.key))
            .min_by_key(|folder| folder.key.len())
    }

    /// The record of project `id` at `key`.
    fn project_mut(&mut self, id: &ProjectId, key: &PathKey) -> Option<&mut ProjectRecord> {
        self.projects
            .iter_mut()
            .find(|record| record.project_id == *id && record.key == *key)
    }

    /// Drops the oldest records of `kind` until at most `limit` remain.
    fn make_room(&mut self, kind: Kind, limit: usize) {
        while self.count(kind) > limit && self.drop_oldest(&[kind], &Keep::Nothing).is_some() {}
    }

    fn count(&self, kind: Kind) -> usize {
        match kind {
            Kind::Project => self.projects.len(),
            Kind::Folder => self.folders.len(),
        }
    }

    /// Drops the record granted longest ago among `kinds`, never the one
    /// `keep` names (on equal times, the one listed first), and returns
    /// about how many bytes it took in the file; `None` when there is no
    /// such record.
    fn drop_oldest(&mut self, kinds: &[Kind], keep: &Keep) -> Option<u64> {
        let projects = self
            .projects
            .iter()
            .enumerate()
            .filter(|(_, record)| !matches!(keep, Keep::Project(key) if record.key == *key))
            .map(|(index, record)| (record.granted, Kind::Project, index));
        let folders = self
            .folders
            .iter()
            .enumerate()
            .filter(|(_, record)| !matches!(keep, Keep::Folder(key) if record.key == *key))
            .map(|(index, record)| (record.granted, Kind::Folder, index));
        // `min_by_key` returns the first of equal minimums.
        let (_, kind, index) = projects
            .chain(folders)
            .filter(|(_, kind, _)| kinds.contains(kind))
            .min_by_key(|&(granted, _, _)| granted)?;
        let json = match kind {
            Kind::Project => serde_json::to_vec_pretty(&self.projects.remove(index).to_json()),
            Kind::Folder => serde_json::to_vec_pretty(&self.folders.remove(index).to_json()),
        };
        Some(json.map_or(1, |json| entry_size(&json)))
    }

    /// The file's bytes, after dropping the records granted longest ago
    /// (never the one `keep` names) until they fit in [`MAX_TRUST_BYTES`].
    fn fit(&mut self, keep: &Keep) -> Result<Vec<u8>, StoreError> {
        self.fit_within(keep, MAX_TRUST_BYTES)
    }

    /// [`Records::fit`] with the limit as a parameter (for tests).
    fn fit_within(&mut self, keep: &Keep, limit: u64) -> Result<Vec<u8>, StoreError> {
        loop {
            let bytes = self.to_bytes()?;
            let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            if size <= limit {
                return Ok(bytes);
            }
            // Drop records worth at most the excess (by an upper bound of
            // their size), then measure again.
            let mut excess = size - limit;
            let mut dropped_any = false;
            while excess > 0 {
                let Some(dropped) = self.drop_oldest(&[Kind::Project, Kind::Folder], keep) else {
                    break;
                };
                dropped_any = true;
                excess = excess.saturating_sub(dropped.max(1));
            }
            if !dropped_any {
                return Err(StoreError::Invalid("the trust store has no room for this record"));
            }
        }
    }

    /// The file's bytes: pretty JSON and a final line feed.
    fn to_bytes(&self) -> Result<Vec<u8>, StoreError> {
        let file = FileJson {
            format: TRUST_FORMAT.to_owned(),
            format_version: TRUST_FORMAT_VERSION,
            projects: self.projects.iter().map(ProjectRecord::to_json).collect(),
            folders: self.folders.iter().map(FolderRecord::to_json).collect(),
        };
        let mut bytes = serde_json::to_vec_pretty(&file)
            .map_err(|_| StoreError::Invalid("the trust store could not be serialised"))?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

/// At least as many bytes as dropping a record whose pretty JSON on its own
/// is `json` saves in the file: there each of its lines is indented by four
/// more spaces, and it is separated from its neighbours by a comma and a
/// line feed (2 bytes), or, as the only record of its list, by the line
/// feeds and indentation that an empty `[]` does not have (4 bytes). An
/// upper bound, so that [`Records::fit_within`] never drops more records
/// than it must.
fn entry_size(json: &[u8]) -> u64 {
    let lines = json.split(|&byte| byte == b'\n').count();
    u64::try_from(json.len() + 4 * lines + 4).unwrap_or(u64::MAX)
}

/// `path` as it is recorded, with its key.
///
/// # Errors
/// [`StoreError::PathNotUnicode`], or [`StoreError::Invalid`] for a path
/// that cannot be recorded.
fn recordable(path: &Path) -> Result<(String, PathKey), StoreError> {
    let text = path.to_str().ok_or(StoreError::PathNotUnicode)?;
    if text.len() > MAX_TRUST_PATH_BYTES {
        return Err(StoreError::Invalid(
            "the path is too long to record in the trust store",
        ));
    }
    let key = valid_path(text).ok_or(StoreError::Invalid(
        "only absolute paths without '.' or '..' parts can be trusted",
    ))?;
    Ok((text.to_owned(), key))
}

/// The key of a recorded path, if it may be recorded: at most
/// [`MAX_TRUST_PATH_BYTES`], no NUL, absolute, no `.` or `..` parts.
fn valid_path(text: &str) -> Option<PathKey> {
    if text.len() > MAX_TRUST_PATH_BYTES || text.contains('\0') {
        return None;
    }
    PathKey::of(Path::new(text))
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

/// A path as trust compares it: its parts in order. On Windows letter case
/// is folded and the verbatim prefixes (`\\?\C:`, `\\?\UNC\server\share`)
/// equal their plain forms; elsewhere parts are compared byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PathKey(Vec<OsString>);

impl PathKey {
    /// The key of an absolute path without `.` or `..` parts; `None` for any
    /// other path (which can never match a record, nor be covered by one).
    fn of(path: &Path) -> Option<Self> {
        if !path.is_absolute() {
            return None;
        }
        let mut parts = Vec::new();
        for component in path.components() {
            match component {
                Component::Prefix(prefix) => parts.push(prefix_key(prefix.kind())),
                Component::RootDir => parts.push(OsString::from(std::path::MAIN_SEPARATOR_STR)),
                Component::Normal(part) => parts.push(fold(part)),
                Component::CurDir | Component::ParentDir => return None,
            }
        }
        Some(Self(parts))
    }

    /// Whether this path is `folder` or below it.
    fn is_within(&self, folder: &Self) -> bool {
        self.0.starts_with(&folder.0)
    }

    fn len(&self) -> usize {
        self.0.len()
    }
}

/// The key part of a Windows path prefix (see [`PathKey`]).
fn prefix_key(prefix: Prefix<'_>) -> OsString {
    let mut key = OsString::new();
    match prefix {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
            key.push(char::from(letter.to_ascii_uppercase()).to_string());
            key.push(":");
        }
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
            key.push(r"\\");
            key.push(fold(server));
            key.push(r"\");
            key.push(fold(share));
        }
        Prefix::Verbatim(name) => {
            key.push(r"\\?\");
            key.push(fold(name));
        }
        Prefix::DeviceNS(name) => {
            key.push(r"\\.\");
            key.push(fold(name));
        }
    }
    key
}

/// A path part as compared: on Windows with letter case folded (parts that
/// are not valid Unicode are kept as they are, so they only equal
/// themselves); elsewhere unchanged.
fn fold(part: &OsStr) -> OsString {
    if cfg!(windows)
        && let Some(text) = part.to_str()
    {
        return OsString::from(fold_case(text));
    }
    part.to_os_string()
}

/// `text` with every character mapped to its simple upper case, as Windows
/// file systems compare names: only characters of the Basic Multilingual
/// Plane whose upper case is a single character change (`ß` stays `ß`).
fn fold_case(text: &str) -> String {
    text.chars()
        .map(|c| {
            if u32::from(c) > 0xFFFF {
                return c;
            }
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(single), None) => single,
                _ => c,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use super::*;

    fn id(text: &str) -> ProjectId {
        ProjectId::new(text).unwrap()
    }

    /// An absolute path on this platform.
    fn abs(parts: &[&str]) -> PathBuf {
        let mut path = if cfg!(windows) {
            PathBuf::from(r"C:\")
        } else {
            PathBuf::from("/")
        };
        path.extend(parts);
        path
    }

    fn identity(project: &str, path: &Path, hash: u8) -> ProjectIdentity {
        ProjectIdentity {
            project_id: id(project),
            canonical_path: path.to_path_buf(),
            security_hash: [hash; 32],
        }
    }

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn project(project: &str, path: &Path, hash: u8, granted: SystemTime) -> ProjectRecord {
        let text = path.to_str().unwrap().to_owned();
        ProjectRecord {
            project_id: id(project),
            key: PathKey::of(path).unwrap(),
            path: text,
            hash: [hash; 32],
            granted_at: rfc3339_utc(granted),
            granted,
        }
    }

    fn folder(path: &Path, granted: SystemTime) -> FolderRecord {
        FolderRecord {
            key: PathKey::of(path).unwrap(),
            path: path.to_str().unwrap().to_owned(),
            granted_at: rfc3339_utc(granted),
            granted,
        }
    }

    #[test]
    fn every_evaluation_case() {
        let game = abs(&["home", "ada", "games", "guess.b2c"]);
        let nested = abs(&["home", "ada", "shared", "class", "week1", "loop.b2c"]);
        let records = Records {
            projects: vec![project("prj_game", &game, 1, at(1))],
            folders: vec![folder(&abs(&["home", "ada", "shared"]), at(2))],
        };
        let verdict = |p: &ProjectIdentity| records.verdict(p);
        // A project record with the same hash.
        assert_eq!(
            verdict(&identity("prj_game", &game, 1)),
            TrustVerdict::Trusted(TrustSource::Project)
        );
        // The same ID and path with another hash: changed outside.
        assert_eq!(
            verdict(&identity("prj_game", &game, 2)),
            TrustVerdict::Restricted(RestrictedReason::ChangedOutside)
        );
        // A copy at another path, or another project at the same path.
        let copy = abs(&["home", "ada", "Downloads", "guess.b2c"]);
        assert_eq!(
            verdict(&identity("prj_game", &copy, 1)),
            TrustVerdict::Restricted(RestrictedReason::NoRecord)
        );
        assert_eq!(
            verdict(&identity("prj_other", &game, 1)),
            TrustVerdict::Restricted(RestrictedReason::NoRecord)
        );
        // A folder record covers everything below it, whatever the hash.
        assert_eq!(
            verdict(&identity("prj_any", &nested, 9)),
            TrustVerdict::Trusted(TrustSource::Folder)
        );
        // Parts are compared whole: "shared2" is not below "shared".
        let sibling = abs(&["home", "ada", "shared2", "x.b2c"]);
        assert_eq!(
            verdict(&identity("prj_any", &sibling, 9)),
            TrustVerdict::Restricted(RestrictedReason::NoRecord)
        );
        // Paths that are not absolute and plain never match.
        let mut escaped = abs(&["home", "ada", "shared"]);
        escaped.extend(["..", "secret", "x.b2c"]);
        assert_eq!(
            verdict(&identity("prj_any", &escaped, 9)),
            TrustVerdict::Restricted(RestrictedReason::NoRecord)
        );
        assert_eq!(
            verdict(&identity("prj_game", Path::new("guess.b2c"), 1)),
            TrustVerdict::Restricted(RestrictedReason::NoRecord)
        );
        // Nothing is trusted without records.
        assert_eq!(
            Records::default().verdict(&identity("prj_game", &game, 1)),
            TrustVerdict::Restricted(RestrictedReason::NoRecord)
        );
        assert!(TrustVerdict::Trusted(TrustSource::Folder).is_trusted());
        assert!(!TrustVerdict::Restricted(RestrictedReason::NoRecord).is_trusted());
    }

    #[test]
    fn a_folder_wins_over_an_outside_change_and_the_outermost_folder_is_named() {
        let game = abs(&["work", "inner", "game.b2c"]);
        let records = Records {
            projects: vec![project("prj_game", &game, 1, at(1))],
            folders: vec![
                folder(&abs(&["work", "inner"]), at(2)),
                folder(&abs(&["work"]), at(3)),
            ],
        };
        assert_eq!(
            records.verdict(&identity("prj_game", &game, 2)),
            TrustVerdict::Trusted(TrustSource::Folder)
        );
        assert_eq!(
            records.verdict(&identity("prj_game", &game, 1)),
            TrustVerdict::Trusted(TrustSource::Project)
        );
        let key = PathKey::of(&game).unwrap();
        assert_eq!(
            records.covering_folder(&key).map(|f| f.path.as_str()),
            abs(&["work"]).to_str()
        );
        // A folder covers itself.
        let work = PathKey::of(&abs(&["work"])).unwrap();
        assert!(records.covering_folder(&work).is_some());
    }

    /// One project record and one folder record, as file bytes.
    fn sample_file() -> Vec<u8> {
        let mut records = Records {
            projects: vec![project("prj_game", &abs(&["g.b2c"]), 0xab, at(1_000))],
            folders: vec![folder(&abs(&["games"]), at(2_000))],
        };
        records.fit(&Keep::Nothing).unwrap()
    }

    #[test]
    fn files_round_trip() {
        let bytes = sample_file();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains(r#""format": "blocks2cpp/trust""#), "{text}");
        assert!(text.contains(r#""formatVersion": 1"#), "{text}");
        assert!(text.contains(&"ab".repeat(32)), "{text}");
        assert!(text.contains("1970-01-01T00:16:40.000Z"), "{text}");
        assert!(text.ends_with("}\n"));
        let parsed = parse(&bytes).unwrap();
        assert_eq!(parsed.projects.len(), 1);
        assert_eq!(parsed.projects[0].hash, [0xab; 32]);
        assert_eq!(parsed.projects[0].granted, at(1_000));
        assert_eq!(parsed.folders[0].path, abs(&["games"]).to_str().unwrap());
        assert_eq!(parsed.to_bytes().unwrap(), bytes);
    }

    #[test]
    fn files_are_validated_strictly() {
        let bytes = sample_file();
        let text = String::from_utf8(bytes.clone()).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let broken = |change: &dyn Fn(&mut serde_json::Value)| {
            let mut value = value.clone();
            change(&mut value);
            parse(&serde_json::to_vec(&value).unwrap()).err()
        };
        let invalid = Some(TrustFileProblem::Invalid);
        assert_eq!(broken(&|v| v["format"] = "blocks2cpp/recent".into()), invalid);
        assert_eq!(broken(&|v| v["formatVersion"] = 0.into()), invalid);
        assert_eq!(broken(&|v| v["formatVersion"] = 1.5.into()), invalid);
        assert_eq!(
            broken(&|v| v["formatVersion"] = 2.into()),
            Some(TrustFileProblem::NewerVersion)
        );
        assert_eq!(broken(&|v| v["extra"] = true.into()), invalid);
        assert_eq!(broken(&|v| v["projects"][0]["extra"] = true.into()), invalid);
        assert_eq!(broken(&|v| v["folders"][0]["extra"] = true.into()), invalid);
        assert_eq!(
            broken(&|v| v["projects"][0]["projectId"] = "prj-1".into()),
            invalid
        );
        assert_eq!(
            broken(&|v| v["projects"][0]["canonicalPath"] = "g.b2c".into()),
            invalid
        );
        let dotted = abs(&["a", "..", "g.b2c"]);
        assert_eq!(
            broken(&|v| v["projects"][0]["canonicalPath"] = dotted.to_str().unwrap().into()),
            invalid
        );
        let with_nul = format!("{}\0", abs(&["g.b2c"]).to_str().unwrap());
        assert_eq!(
            broken(&|v| v["projects"][0]["canonicalPath"] = with_nul.clone().into()),
            invalid
        );
        assert_eq!(
            broken(&|v| v["projects"][0]["rawCodeHashAtGrant"] = "AB".repeat(32).into()),
            invalid
        );
        assert_eq!(
            broken(&|v| v["projects"][0]["rawCodeHashAtGrant"] = "ab".repeat(31).into()),
            invalid
        );
        assert_eq!(
            broken(&|v| v["projects"][0]["grantedAt"] = "yesterday".into()),
            invalid
        );
        assert_eq!(broken(&|v| v["folders"][0]["grantedAt"] = 5.into()), invalid);
        let root = abs(&[]);
        assert_eq!(
            broken(&|v| v["folders"][0]["canonicalPath"] = root.to_str().unwrap().into()),
            invalid
        );
        assert_eq!(
            broken(&|v| {
                let first = v["projects"][0].clone();
                v["projects"].as_array_mut().unwrap().push(first);
            }),
            invalid
        );
        assert_eq!(
            broken(&|v| {
                let first = v["folders"][0].clone();
                v["folders"].as_array_mut().unwrap().push(first);
            }),
            invalid
        );
        assert_eq!(
            broken(&|v| {
                v.as_object_mut().unwrap().remove("folders");
            }),
            invalid
        );
        for bytes in [
            &b"not json"[..],
            b"[]",
            b"{}",
            b"null",
            br#"{"format": "blocks2cpp/trust"}"#,
        ] {
            assert_eq!(parse(bytes).err(), invalid, "{}", String::from_utf8_lossy(bytes));
        }
        // Duplicate keys are refused, not resolved.
        let duplicate = text.replacen(
            r#""formatVersion": 1,"#,
            r#""formatVersion": 1, "formatVersion": 1,"#,
            1,
        );
        assert_eq!(parse(duplicate.as_bytes()).err(), invalid);
    }

    #[test]
    fn hashes_parse_only_from_lower_case_hex() {
        let text = "0123456789abcdef".repeat(4);
        let hash = parse_hash(&text).unwrap();
        assert_eq!(hash[0], 0x01);
        assert_eq!(hash[31], 0xef);
        assert_eq!(hex_lower(&hash), text);
        assert_eq!(parse_hash(&text.to_uppercase()), None);
        assert_eq!(parse_hash(&text[..62]), None);
        assert_eq!(parse_hash(&format!("{text}0")), None);
    }

    #[test]
    fn limits_drop_the_records_granted_longest_ago() {
        let mut records = Records::default();
        for (index, seconds) in [30, 10, 20].into_iter().enumerate() {
            let path = abs(&[&format!("p{index}.b2c")]);
            records.projects.push(project("prj_x", &path, 0, at(seconds)));
        }
        records.folders.push(folder(&abs(&["f"]), at(15)));
        records.make_room(Kind::Project, 2);
        let names: Vec<&str> = records.projects.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(
            names,
            [
                abs(&["p0.b2c"]).to_str().unwrap(),
                abs(&["p2.b2c"]).to_str().unwrap()
            ]
        );
        assert_eq!(records.folders.len(), 1);
        // The byte limit drops the oldest of either kind, never the kept one.
        let keep = Keep::Project(PathKey::of(&abs(&["p2.b2c"])).unwrap());
        let full = records.to_bytes().unwrap().len();
        let mut fitted = records.clone();
        let bytes = fitted
            .fit_within(&keep, u64::try_from(full - 1).unwrap())
            .unwrap();
        assert!(bytes.len() < full);
        assert_eq!(fitted.folders.len(), 0, "the folder (15 s) was the oldest");
        assert_eq!(fitted.projects.len(), 2);
        let mut tight = records.clone();
        let bytes = tight.fit_within(&keep, 1).err();
        assert!(matches!(bytes, Some(StoreError::Invalid(_))));
        let mut small = records;
        let only_kept = {
            let mut probe = small.clone();
            probe
                .projects
                .retain(|r| r.path == abs(&["p2.b2c"]).to_str().unwrap());
            probe.folders.clear();
            probe.to_bytes().unwrap().len()
        };
        small
            .fit_within(&keep, u64::try_from(only_kept).unwrap())
            .unwrap();
        assert_eq!(small.projects.len(), 1);
        assert_eq!(small.projects[0].path, abs(&["p2.b2c"]).to_str().unwrap());
    }

    proptest::proptest! {
        /// Fitting under a byte limit drops only the oldest records, never
        /// the kept one, and no more than it must: putting back the newest
        /// record it dropped would break the limit again.
        #[test]
        fn fitting_drops_the_fewest_oldest_records(
            entries in proptest::collection::vec((1_usize..300, proptest::bool::ANY), 1..25),
            slack in 0_usize..6_000,
        ) {
            let mut records = Records::default();
            for (index, &(length, is_folder)) in entries.iter().enumerate() {
                let name = format!("{index}-{}", "x".repeat(length));
                let path = abs(&[&name]);
                let granted = at(u64::try_from(index).unwrap());
                if is_folder {
                    records.folders.push(folder(&path, granted));
                } else {
                    records.projects.push(project("prj_x", &path, 7, granted));
                }
            }
            // The newest record is the one just granted.
            let (newest_is_folder, newest_key) = match (records.projects.last(), records.folders.last()) {
                (Some(p), Some(f)) if f.granted > p.granted => (true, f.key.clone()),
                (Some(p), _) => (false, p.key.clone()),
                (None, Some(f)) => (true, f.key.clone()),
                (None, None) => unreachable!(),
            };
            let keep = if newest_is_folder { Keep::Folder(newest_key.clone()) } else { Keep::Project(newest_key.clone()) };
            let mut alone = records.clone();
            alone.projects.retain(|r| r.key == newest_key && !newest_is_folder);
            alone.folders.retain(|r| r.key == newest_key && newest_is_folder);
            let limit = alone.to_bytes().unwrap().len() + slack;

            let mut fitted = records.clone();
            let bytes = fitted.fit_within(&keep, u64::try_from(limit).unwrap()).unwrap();
            proptest::prop_assert!(bytes.len() <= limit);
            proptest::prop_assert_eq!(&bytes, &fitted.to_bytes().unwrap());
            let kept: Vec<SystemTime> = fitted
                .projects.iter().map(|r| r.granted)
                .chain(fitted.folders.iter().map(|r| r.granted))
                .collect();
            proptest::prop_assert!(kept.contains(&at(u64::try_from(entries.len() - 1).unwrap())));
            let dropped_projects: Vec<&ProjectRecord> = records.projects.iter().filter(|r| !kept.contains(&r.granted)).collect();
            let dropped_folders: Vec<&FolderRecord> = records.folders.iter().filter(|r| !kept.contains(&r.granted)).collect();
            let newest_dropped = dropped_projects.iter().map(|r| r.granted).chain(dropped_folders.iter().map(|r| r.granted)).max();
            if let Some(newest_dropped) = newest_dropped {
                // Only the oldest were dropped.
                proptest::prop_assert!(kept.iter().all(|&granted| granted > newest_dropped));
                let mut back = fitted.clone();
                back.projects.extend(dropped_projects.iter().filter(|r| r.granted == newest_dropped).map(|r| (*r).clone()));
                back.folders.extend(dropped_folders.iter().filter(|r| r.granted == newest_dropped).map(|r| (*r).clone()));
                proptest::prop_assert!(back.to_bytes().unwrap().len() > limit);
            }
        }
    }

    #[test]
    fn case_folding_is_simple_upper_case() {
        assert_eq!(fold_case("Ada/Games/x.b2c"), "ADA/GAMES/X.B2C");
        assert_eq!(fold_case("straße"), "STRAßE");
        assert_eq!(fold_case("ÉCOLE é"), "ÉCOLE É");
        assert_eq!(fold_case("\u{10428}"), "\u{10428}");
        assert_eq!(fold_case("ǆ"), "Ǆ");
    }

    #[test]
    fn keys_compare_parts() {
        let key = |path: &str| PathKey::of(Path::new(path));
        if cfg!(windows) {
            assert_eq!(key(r"C:\Users\Ada\x.b2c"), key(r"c:\users\ada\X.B2C"));
            assert_eq!(key(r"\\?\C:\Users\x.b2c"), key(r"C:\Users\x.b2c"));
            assert_eq!(key(r"\\?\UNC\Server\Share\x.b2c"), key(r"\\server\share\X.b2c"));
            assert_ne!(key(r"C:\a\x.b2c"), key(r"D:\a\x.b2c"));
            assert!(key(r"C:\a\..\x.b2c").is_none());
            assert!(key(r"a\x.b2c").is_none());
        } else {
            assert_eq!(key("/home//ada/x.b2c"), key("/home/ada/x.b2c"));
            assert_ne!(key("/home/Ada/x.b2c"), key("/home/ada/x.b2c"));
            assert!(key("/home/ada/../x.b2c").is_none());
            assert!(key("home/x.b2c").is_none());
            assert!(key("/home/ada/x.b2c").unwrap().is_within(&key("/home").unwrap()));
            assert!(!key("/home2/x.b2c").unwrap().is_within(&key("/home").unwrap()));
        }
    }

    #[test]
    fn problems_never_show_paths() {
        for problem in [
            TrustFileProblem::TooLarge,
            TrustFileProblem::NotAFile,
            TrustFileProblem::Unreadable {
                kind: io::ErrorKind::PermissionDenied,
            },
            TrustFileProblem::Invalid,
            TrustFileProblem::NewerVersion,
        ] {
            let text = problem.to_string();
            assert!(text.contains("nothing is trusted"), "{text}");
            assert!(!text.contains('/') && !text.contains('\\'), "{text}");
        }
    }
}
