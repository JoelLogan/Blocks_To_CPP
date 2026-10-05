//! Build cache housekeeping: least-recently-used eviction over the size cap,
//! pruning of entries unused for 30 days, and *Clear build cache*
//! (`docs/spec/07-toolchain-build-run.md` §7.5.1; the file-system rules are
//! `docs/spec/08-security.md` §8.6).
//!
//! # Entries
//!
//! The unit of eviction is an **entry**: one build folder
//! `<cache>/builds/<projectFolder>/<config>-<hash8>/` (see
//! [`crate::build_dir`]). Its last use is the modification time of its `lock`
//! file, which every build and run touches; an entry without a `lock` file
//! uses the folder's own modification time. An entry whose lock is held,
//! exclusively by a build or shared by a running program ([`hold_for_run`]),
//! is *in use* and is never deleted.
//!
//! | Function | Removes |
//! |---|---|
//! | [`prune_and_evict`] | entries unused for [`EvictionPolicy::max_age`], then least recently used entries while the cache is over [`EvictionPolicy::max_bytes`] |
//! | [`evict_to_cap`] | least recently used entries while the cache is over [`EvictionPolicy::max_bytes`] |
//! | [`clear`] | everything in `builds/` except entries in use |
//! | [`usage`] | nothing; it measures `builds/` |
//!
//! The size of the cache is the sum of the lengths of the regular files in
//! `builds/`. Links count as nothing and are never followed, so a link inside
//! the cache that points elsewhere never makes the cache look larger.
//!
//! Eviction never removes the most recently used entry, so a program that
//! was just built survives even when it alone is over the cap. Deleting a
//! single entry may fail (on Windows, for example, while its program runs
//! without a [`hold_for_run`]); the entry is then counted as kept, the bytes
//! that were deleted are still reported, and the rest of the cache is
//! processed as usual.
//!
//! # Safety rules (08 §8.6)
//!
//! * Only `builds/` is touched. `sandbox/`, `toolchains.json` and everything
//!   else at the cache root never are (on Windows the cache root is also the
//!   machine folder with the trust store and the recovery snapshots).
//! * Links and junctions are never followed. Every item is examined with
//!   [`fs::symlink_metadata`]; on Windows every reparse point
//!   (`FILE_ATTRIBUTE_REPARSE_POINT`) counts as a link. A link is never
//!   descended into or measured; [`clear`] removes the link itself.
//! * Before anything is deleted, every level from the canonical `builds/`
//!   folder down to the folder being deleted is checked to be a real folder,
//!   and the folder's canonical path to lie inside `builds/`. Folder contents
//!   are deleted with [`fs::remove_dir_all`], which removes links instead of
//!   following them and works relative to open directory handles on Linux
//!   and Windows.
//! * An entry is deleted while this process holds its lock exclusively, so no
//!   build or run can take it during the deletion (a build that was waiting
//!   for the lock then finds its folder gone and fails; the next build starts
//!   afresh). Files at the top of the entry (`build-manifest.json`) go before
//!   its folders, so an entry that could be deleted only in part never looks
//!   like an up-to-date build.
//!
//! The cache is owner-only, so these checks protect against links that the
//! user or the user's programs create inside it. They do not defend against
//! another process of the same user swapping the folders above an entry for
//! links in the middle of a deletion; such a process can already delete
//! whatever this one could.
//!
//! # Errors
//!
//! The functions return [`io::Error`]. Problems with the cache's layout, such
//! as a `builds` folder that is a link, carry a [`CacheError`] (see
//! [`io::Error::get_ref`]). Failing to delete one entry is not an error. No
//! error message contains a path.

use std::ffi::OsStr;
use std::fs::{self, File, Metadata, TryLockError};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The default size cap: 2 GiB (settings `buildCache.maxBytes`,
/// `docs/spec/05-project-format.md` §5.9).
pub const DEFAULT_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// The default age after which an unused entry is pruned: 30 days.
pub const DEFAULT_MAX_AGE: Duration = Duration::from_hours(30 * 24);

/// The folder below the cache root that holds the build folders.
const BUILDS_DIR: &str = "builds";

/// The lock file in every build folder (the name [`crate::BuildDir::lock`]
/// uses).
const LOCK_FILE: &str = "lock";

/// `FILE_ATTRIBUTE_REPARSE_POINT` (Win32): set on symbolic links, junctions,
/// mount points and every other reparse point.
#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// How much the build cache may hold and for how long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvictionPolicy {
    /// The size cap in bytes (settings `buildCache.maxBytes`, default
    /// [`DEFAULT_MAX_BYTES`]).
    pub max_bytes: u64,
    /// Entries unused for at least this long are pruned by
    /// [`prune_and_evict`] (default [`DEFAULT_MAX_AGE`]).
    pub max_age: Duration,
}

impl EvictionPolicy {
    /// The policy for a size cap from the settings, with the default maximum
    /// age.
    pub const fn with_max_bytes(max_bytes: u64) -> Self {
        Self {
            max_bytes,
            max_age: DEFAULT_MAX_AGE,
        }
    }
}

impl Default for EvictionPolicy {
    /// 2 GiB and 30 days.
    fn default() -> Self {
        Self::with_max_bytes(DEFAULT_MAX_BYTES)
    }
}

/// What [`prune_and_evict`] or [`evict_to_cap`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EvictionReport {
    /// Entries deleted.
    pub removed: usize,
    /// Bytes deleted, including those of entries that could be deleted only
    /// in part.
    pub freed_bytes: u64,
    /// Entries that were due for deletion but kept: in use (their lock is
    /// held), or not completely deletable.
    pub skipped_locked: usize,
}

impl EvictionReport {
    fn record(&mut self, removal: Removal) {
        self.freed_bytes = self.freed_bytes.saturating_add(removal.freed());
        match removal {
            Removal::Removed { .. } => self.removed += 1,
            Removal::Kept { .. } => self.skipped_locked += 1,
            Removal::Gone => {}
        }
    }
}

/// What [`clear`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClearReport {
    /// Bytes deleted.
    pub freed_bytes: u64,
    /// Items in `builds/` that were kept: entries in use (their lock is
    /// held), and anything that could not be deleted completely.
    pub skipped_in_use: usize,
}

impl ClearReport {
    fn record(&mut self, removal: Removal) {
        self.freed_bytes = self.freed_bytes.saturating_add(removal.freed());
        if let Removal::Kept { .. } = removal {
            self.skipped_in_use += 1;
        }
    }
}

/// A problem with the build cache, carried inside the [`io::Error`] the
/// functions of this module return. The messages never contain a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CacheError {
    /// A path that must be absolute was relative.
    #[error("the build cache path must be absolute")]
    RelativePath,
    /// A folder of the cache (named in the message) is a link, a junction or
    /// not a folder at all.
    #[error("refusing to use the {0}: it is a link, a junction or not a folder")]
    NotAFolder(&'static str),
    /// A build folder's `lock` is a link or not a regular file.
    #[error("refusing to use the build folder's lock: it is a link or not a regular file")]
    LockNotAFile,
    /// A build or an eviction holds the build folder's lock.
    #[error("the build folder is in use")]
    InUse,
}

impl CacheError {
    fn kind(self) -> io::ErrorKind {
        match self {
            Self::RelativePath => io::ErrorKind::InvalidInput,
            Self::NotAFolder(_) => io::ErrorKind::NotADirectory,
            Self::LockNotAFile => io::ErrorKind::InvalidData,
            Self::InUse => io::ErrorKind::WouldBlock,
        }
    }
}

impl From<CacheError> for io::Error {
    fn from(error: CacheError) -> Self {
        Self::new(error.kind(), error)
    }
}

/// Prunes the entries unused for at least `policy.max_age` (as of `now`),
/// then deletes least recently used entries while the cache is larger than
/// `policy.max_bytes`. Entries in use are skipped, project folders left
/// empty are removed, and nothing outside `<cache_root>/builds/` is touched
/// (see the module documentation).
///
/// A missing cache root or `builds` folder is an empty cache.
///
/// # Errors
/// [`CacheError::RelativePath`] for a relative `cache_root`,
/// [`CacheError::NotAFolder`] when the cache root or its `builds` folder is a
/// link or not a folder, and I/O errors from resolving or listing them.
pub fn prune_and_evict(
    cache_root: &Path,
    policy: &EvictionPolicy,
    now: SystemTime,
) -> io::Result<EvictionReport> {
    evict(cache_root, policy, Some(now))
}

/// Deletes least recently used entries while the cache is larger than
/// `policy.max_bytes`; `policy.max_age` is not used. Otherwise as
/// [`prune_and_evict`].
///
/// # Errors
/// As [`prune_and_evict`].
pub fn evict_to_cap(cache_root: &Path, policy: &EvictionPolicy) -> io::Result<EvictionReport> {
    evict(cache_root, policy, None)
}

/// *Clear build cache*: deletes everything in `<cache_root>/builds/` except
/// entries in use. Stray files and links in `builds/` are removed too (links
/// themselves, never what they point to); project folders left empty are
/// removed; the `builds` folder itself, `sandbox/` and everything else at the
/// cache root are kept.
///
/// # Errors
/// As [`prune_and_evict`].
pub fn clear(cache_root: &Path) -> io::Result<ClearReport> {
    let mut report = ClearReport::default();
    let Some(layout) = Layout::open(cache_root)? else {
        return Ok(report);
    };
    for project in list(&layout.builds)? {
        if project.kind != Kind::Dir {
            report.record(remove_stray(&layout, &project, 0));
            continue;
        }
        let Ok(items) = list(&project.path) else {
            report.skipped_in_use += 1;
            continue;
        };
        for item in items {
            let removal = if item.kind == Kind::Dir {
                remove_entry(&layout, &item.path)
            } else {
                remove_stray(&layout, &item, 1)
            };
            report.record(removal);
        }
        remove_if_empty(&layout, &project.path);
    }
    Ok(report)
}

/// The size of the build cache in bytes: the lengths of the regular files in
/// `<cache_root>/builds/`, without following links. A missing cache is 0.
/// Folders that cannot be read are left out.
///
/// # Errors
/// As [`prune_and_evict`].
pub fn usage(cache_root: &Path) -> io::Result<u64> {
    Ok(Layout::open(cache_root)?.map_or(0, |layout| tree_size(&layout.builds)))
}

/// A running program's claim on its build folder; see [`hold_for_run`].
/// Dropping it releases the claim.
#[derive(Debug)]
#[must_use = "the build folder is protected only while the hold is kept"]
pub struct RunHold {
    _lock: File,
}

/// Marks the build folder `entry` as used now and keeps eviction and
/// [`clear`] away from it while the program built there runs: it sets the
/// modification time of the folder's `lock` file to now and takes a shared
/// lock on it until the returned [`RunHold`] is dropped. A missing `lock`
/// file is created (owner-only on Unix); an existing one must be a regular
/// file, not a link.
///
/// The shared lock also makes a build of the same folder (in this or another
/// process) wait until the program has ended, which on Windows it must do
/// anyway because a running program's file cannot be replaced.
///
/// # Errors
/// [`CacheError::RelativePath`]; [`CacheError::NotAFolder`] when `entry` is
/// a link or not a folder; [`CacheError::LockNotAFile`];
/// [`CacheError::InUse`] (kind [`io::ErrorKind::WouldBlock`]) while a build
/// or an eviction holds the lock; and I/O errors.
pub fn hold_for_run(entry: &Path) -> io::Result<RunHold> {
    if !entry.is_absolute() {
        return Err(CacheError::RelativePath.into());
    }
    if kind_of(&fs::symlink_metadata(entry)?) != Kind::Dir {
        return Err(CacheError::NotAFolder("build folder").into());
    }
    let lock = open_lock(entry, true)?;
    match lock.try_lock_shared() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Err(CacheError::InUse.into()),
        Err(TryLockError::Error(error)) => return Err(error),
    }
    lock.set_modified(SystemTime::now())?;
    Ok(RunHold { _lock: lock })
}

/// What a directory item is, judged without following links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A real folder.
    Dir,
    /// A regular file.
    File,
    /// A symbolic link or, on Windows, any reparse point (junctions included).
    Link,
    /// Anything else: a FIFO, a socket, a device.
    Other,
}

/// Classifies `metadata` from [`fs::symlink_metadata`] (or
/// [`fs::DirEntry::metadata`], which does not follow links either).
fn kind_of(metadata: &Metadata) -> Kind {
    if is_link(metadata) {
        Kind::Link
    } else if metadata.is_dir() {
        Kind::Dir
    } else if metadata.is_file() {
        Kind::File
    } else {
        Kind::Other
    }
}

/// Whether `metadata` describes a link: a symbolic link or, on Windows, any
/// reparse point at all.
fn is_link(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

/// Whether two metadata records describe the same file: the same device and
/// inode on Unix; elsewhere the same kind and length, which still catches a
/// swap to a different kind of file.
fn same_file(a: &Metadata, b: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    #[cfg(not(unix))]
    {
        a.is_file() == b.is_file() && a.len() == b.len()
    }
}

/// Whether `path` is a real folder (not a link to one).
fn is_plain_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| kind_of(&metadata) == Kind::Dir)
}

/// Treats "it is already gone" as success.
fn ignore_missing(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// The cache's `builds` folder, canonical and checked.
struct Layout {
    builds: PathBuf,
}

impl Layout {
    /// Checks the cache root and its `builds` folder. `None` when either
    /// does not exist (an empty cache).
    fn open(cache_root: &Path) -> io::Result<Option<Self>> {
        if !cache_root.is_absolute() {
            return Err(CacheError::RelativePath.into());
        }
        if !dir_or_missing(cache_root, "cache folder")? {
            return Ok(None);
        }
        let root = fs::canonicalize(cache_root)?;
        let builds = root.join(BUILDS_DIR);
        if !dir_or_missing(&builds, "build cache folder")? {
            return Ok(None);
        }
        let builds = fs::canonicalize(&builds)?;
        if builds.parent() != Some(root.as_path()) {
            return Err(CacheError::NotAFolder("build cache folder").into());
        }
        Ok(Some(Self { builds }))
    }

    /// Whether `dir` is still exactly `depth` real folders below `builds/`,
    /// with no level (`builds/` included) a link or junction, and resolves to
    /// a place that deep inside `builds/`. Checked right before deleting.
    fn holds_dir(&self, dir: &Path, depth: usize) -> bool {
        let Ok(relative) = dir.strip_prefix(&self.builds) else {
            return false;
        };
        let components: Vec<Component<'_>> = relative.components().collect();
        if components.len() != depth || !components.iter().all(is_plain_name) {
            return false;
        }
        let mut level = self.builds.clone();
        if !is_plain_dir(&level) {
            return false;
        }
        for component in components {
            level.push(component);
            if !is_plain_dir(&level) {
                return false;
            }
        }
        fs::canonicalize(dir).is_ok_and(|canonical| {
            canonical
                .strip_prefix(&self.builds)
                .is_ok_and(|inside| inside.components().count() == depth)
        })
    }
}

/// A single plain name: a normal component with no separator inside it. On
/// Windows the cache root is a verbatim (`\\?\`) path, after which `/` is not
/// a separator and `..` is not special, so `a/../b` would otherwise count as
/// one normal component.
fn is_plain_name(component: &Component<'_>) -> bool {
    match component {
        Component::Normal(name) => name
            .to_str()
            .is_some_and(|name| name != "." && name != ".." && !name.contains(['/', '\\'])),
        _ => false,
    }
}

/// `Ok(true)` for a real folder, `Ok(false)` when nothing is at `path`, and
/// [`CacheError::NotAFolder`] (naming `what`) for anything else.
fn dir_or_missing(path: &Path, what: &'static str) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if kind_of(&metadata) == Kind::Dir => Ok(true),
        Ok(_) => Err(CacheError::NotAFolder(what).into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// One item of a folder, examined without following links.
struct Item {
    path: PathBuf,
    kind: Kind,
    len: u64,
    modified: Option<SystemTime>,
}

impl Item {
    fn name(&self) -> Option<&OsStr> {
        self.path.file_name()
    }
}

/// The items of `dir`, sorted by path. Items that vanish while listing are
/// left out.
fn list(dir: &Path) -> io::Result<Vec<Item>> {
    let mut items = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        items.push(Item {
            path: entry.path(),
            kind: kind_of(&metadata),
            len: metadata.len(),
            modified: metadata.modified().ok(),
        });
    }
    items.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(items)
}

/// The lengths of the regular files below `dir`, without following links or
/// junctions. Walks iteratively (no recursion, whatever the depth); folders
/// that cannot be read are left out.
fn tree_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            match kind_of(&metadata) {
                Kind::Dir => pending.push(entry.path()),
                Kind::File => total = total.saturating_add(metadata.len()),
                Kind::Link | Kind::Other => {}
            }
        }
    }
    total
}

/// Opens `<entry>/lock` without following a link (for writing when `write`
/// is set). An existing lock must be a regular file, checked again after
/// opening; a missing one is created exclusively (`O_EXCL`, which never
/// follows a link), owner-only on Unix.
fn open_lock(entry: &Path, write: bool) -> io::Result<File> {
    let path = entry.join(LOCK_FILE);
    // A second round only when a build creates the lock between our look
    // and our exclusive create.
    for _ in 0..2 {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if kind_of(&metadata) == Kind::File => {
                let file = fs::OpenOptions::new().read(true).write(write).open(&path)?;
                if !same_file(&metadata, &file.metadata()?) {
                    return Err(CacheError::LockNotAFile.into());
                }
                return Ok(file);
            }
            Ok(_) => return Err(CacheError::LockNotAFile.into()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        match options.open(&path) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            other => return other,
        }
    }
    Err(CacheError::InUse.into())
}

/// Takes `entry`'s lock exclusively without waiting. `None` when someone
/// else holds it.
fn claim(entry: &Path) -> io::Result<Option<File>> {
    let lock = match open_lock(entry, false) {
        Ok(lock) => lock,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
        Err(error) => return Err(error),
    };
    match lock.try_lock() {
        Ok(()) => Ok(Some(lock)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

/// The outcome of deleting one item of `builds/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Removal {
    /// Deleted completely.
    Removed { freed: u64 },
    /// Kept, wholly or in part: in use, or a deletion failed.
    Kept { freed: u64 },
    /// Already gone: another eviction or clear (in this or another process)
    /// deleted it first. Counted neither as removed nor as kept.
    Gone,
}

impl Removal {
    fn freed(self) -> u64 {
        match self {
            Self::Removed { freed } | Self::Kept { freed } => freed,
            Self::Gone => 0,
        }
    }

    /// [`Removal::Kept`] for an item that is still there (but not where or
    /// what it should be), [`Removal::Gone`] for one that is not.
    fn kept_unless_gone(path: &Path) -> Self {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::Gone,
            _ => Self::Kept { freed: 0 },
        }
    }
}

/// Deletes the entry folder `entry` (two levels below `builds/`) unless it
/// is in use or no longer where it should be.
fn remove_entry(layout: &Layout, entry: &Path) -> Removal {
    if !layout.holds_dir(entry, 2) {
        return Removal::kept_unless_gone(entry);
    }
    // In use (`Ok(None)`), or a lock that cannot be used: keep the entry.
    let Ok(Some(lock)) = claim(entry) else {
        return Removal::kept_unless_gone(entry);
    };
    // Someone else may have deleted the entry before releasing the lock that
    // was just taken (on a lock file that is no longer in the folder).
    if !layout.holds_dir(entry, 2) {
        return Removal::kept_unless_gone(entry);
    }
    let before = tree_size(entry);
    let deleted = delete_locked_entry(entry, lock);
    let left = match fs::symlink_metadata(entry) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Ok(metadata) if kind_of(&metadata) == Kind::Dir => tree_size(entry),
        _ => before,
    };
    let freed = before.saturating_sub(left);
    match deleted {
        Ok(()) => Removal::Removed { freed },
        Err(_) => Removal::Kept { freed },
    }
}

/// Deletes an entry whose lock `lock` this process holds: its files first
/// (the build manifest before the program it describes), then its folders,
/// then the lock file and the entry itself. The lock is released at the end.
fn delete_locked_entry(entry: &Path, lock: File) -> io::Result<()> {
    let items = list(entry)?;
    for item in items.iter().filter(|item| item.kind != Kind::Dir) {
        if item.name() != Some(OsStr::new(LOCK_FILE)) {
            remove_non_dir(&item.path, item.kind)?;
        }
    }
    for item in items.iter().filter(|item| item.kind == Kind::Dir) {
        ignore_missing(fs::remove_dir_all(&item.path))?;
    }
    let lock_path = entry.join(LOCK_FILE);
    // Removing the name of a file this process has open works on Unix and,
    // with POSIX delete semantics (Windows 10 1903 and later on NTFS), on
    // Windows.
    ignore_missing(fs::remove_file(&lock_path))?;
    let removed = ignore_missing(fs::remove_dir(entry));
    drop(lock);
    #[cfg(windows)]
    if removed.is_err() {
        // Without POSIX semantics the lock file's name stays until its last
        // handle closes; it has closed now.
        ignore_missing(fs::remove_file(&lock_path))?;
        return ignore_missing(fs::remove_dir(entry));
    }
    removed
}

/// Deletes a file, a link (the link itself) or another non-folder item.
fn remove_non_dir(path: &Path, kind: Kind) -> io::Result<()> {
    match (ignore_missing(fs::remove_file(path)), kind) {
        // A link to a folder (and a junction) on Windows is removed as a
        // folder; `remove_dir` never follows it. On Unix `remove_file`
        // already removed any link.
        (Err(error), Kind::Link) => ignore_missing(fs::remove_dir(path)).map_err(|_| error),
        (result, _) => result,
    }
}

/// Deletes an item of `builds/` (`parent_depth` 0) or of a project folder
/// (`parent_depth` 1) that is not a folder: a stray file or a link (never
/// what it points to).
fn remove_stray(layout: &Layout, item: &Item, parent_depth: usize) -> Removal {
    let Some(parent) = item.path.parent() else {
        return Removal::Kept { freed: 0 };
    };
    if !layout.holds_dir(parent, parent_depth) {
        return Removal::kept_unless_gone(&item.path);
    }
    let metadata = match fs::symlink_metadata(&item.path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Removal::Gone,
        Err(_) => return Removal::Kept { freed: 0 },
    };
    let kind = kind_of(&metadata);
    if kind == Kind::Dir {
        // It became a folder since it was listed: leave it for next time.
        return Removal::Kept { freed: 0 };
    }
    let freed = if kind == Kind::File { metadata.len() } else { 0 };
    match remove_non_dir(&item.path, kind) {
        Ok(()) => Removal::Removed { freed },
        Err(_) => Removal::Kept { freed: 0 },
    }
}

/// Removes the project folder `project` if it is empty.
fn remove_if_empty(layout: &Layout, project: &Path) {
    if layout.holds_dir(project, 1) {
        // Fails, harmlessly, while the folder has entries.
        let _ = fs::remove_dir(project);
    }
}

/// An entry found by [`scan`].
struct Entry {
    path: PathBuf,
    last_used: Option<SystemTime>,
}

/// The cache as [`scan`] found it.
#[derive(Default)]
struct Scan {
    projects: Vec<PathBuf>,
    entries: Vec<Entry>,
    total: u64,
}

/// Lists the entries of the cache and measures it (as [`usage`] does).
fn scan(layout: &Layout) -> io::Result<Scan> {
    let mut scan = Scan::default();
    for project in list(&layout.builds)? {
        match project.kind {
            Kind::File => scan.total = scan.total.saturating_add(project.len),
            Kind::Dir => {
                // A project folder that cannot be read cannot be evicted.
                let Ok(items) = list(&project.path) else {
                    continue;
                };
                for item in items {
                    match item.kind {
                        Kind::Dir => {
                            scan.total = scan.total.saturating_add(tree_size(&item.path));
                            scan.entries.push(Entry {
                                last_used: last_used(&item),
                                path: item.path,
                            });
                        }
                        Kind::File => scan.total = scan.total.saturating_add(item.len),
                        Kind::Link | Kind::Other => {}
                    }
                }
                scan.projects.push(project.path);
            }
            Kind::Link | Kind::Other => {}
        }
    }
    Ok(scan)
}

/// When `entry` was last used: its lock file's modification time, or the
/// folder's own when it has no lock file (or one that is not a regular
/// file).
fn last_used(entry: &Item) -> Option<SystemTime> {
    match fs::symlink_metadata(entry.path.join(LOCK_FILE)) {
        Ok(metadata) if kind_of(&metadata) == Kind::File => metadata.modified().ok(),
        _ => entry.modified,
    }
}

/// Whether an entry last used at `last_used` is at least `max_age` old at
/// `now`. An unknown time or one in the future is never expired.
fn expired(last_used: Option<SystemTime>, now: SystemTime, max_age: Duration) -> bool {
    last_used.is_some_and(|used| now.duration_since(used).is_ok_and(|age| age >= max_age))
}

/// [`prune_and_evict`] (with `prune_at`) and [`evict_to_cap`] (without).
fn evict(
    cache_root: &Path,
    policy: &EvictionPolicy,
    prune_at: Option<SystemTime>,
) -> io::Result<EvictionReport> {
    let mut report = EvictionReport::default();
    let Some(layout) = Layout::open(cache_root)? else {
        return Ok(report);
    };
    let mut scan = scan(&layout)?;
    // Least recently used first; an unknown time counts as oldest, and equal
    // times are ordered by path so that eviction is deterministic.
    scan.entries
        .sort_by(|a, b| a.last_used.cmp(&b.last_used).then_with(|| a.path.cmp(&b.path)));
    let newest = scan.entries.len().checked_sub(1);
    let mut tried = vec![false; scan.entries.len()];

    if let Some(now) = prune_at {
        for (index, entry) in scan.entries.iter().enumerate() {
            if expired(entry.last_used, now, policy.max_age) {
                tried[index] = true;
                let removal = remove_entry(&layout, &entry.path);
                scan.total = scan.total.saturating_sub(removal.freed());
                report.record(removal);
            }
        }
    }
    for (index, entry) in scan.entries.iter().enumerate() {
        if scan.total <= policy.max_bytes {
            break;
        }
        if tried[index] || Some(index) == newest {
            continue;
        }
        let removal = remove_entry(&layout, &entry.path);
        scan.total = scan.total.saturating_sub(removal.freed());
        report.record(removal);
    }
    for project in &scan.projects {
        remove_if_empty(&layout, project);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_match_the_spec() {
        let policy = EvictionPolicy::default();
        assert_eq!(policy.max_bytes, 2_147_483_648);
        assert_eq!(policy.max_age.as_secs(), 2_592_000);
        assert_eq!(EvictionPolicy::with_max_bytes(7).max_bytes, 7);
        assert_eq!(EvictionPolicy::with_max_bytes(7).max_age, DEFAULT_MAX_AGE);
    }

    #[test]
    fn expiry_needs_a_known_past_time_at_least_max_age_ago() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_hours(100 * 24);
        let day = Duration::from_hours(24);
        assert!(expired(Some(now - 30 * day), now, 30 * day));
        assert!(expired(Some(now - 31 * day), now, 30 * day));
        assert!(!expired(Some(now - 29 * day), now, 30 * day));
        assert!(!expired(Some(now + day), now, 30 * day));
        assert!(!expired(None, now, 30 * day));
    }

    #[test]
    fn errors_carry_a_kind_and_no_path() {
        let error = io::Error::from(CacheError::NotAFolder("build cache folder"));
        assert_eq!(error.kind(), io::ErrorKind::NotADirectory);
        assert_eq!(
            error.to_string(),
            "refusing to use the build cache folder: it is a link, a junction or not a folder"
        );
        let inner = error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<CacheError>());
        assert_eq!(inner, Some(&CacheError::NotAFolder("build cache folder")));
        assert_eq!(
            io::Error::from(CacheError::InUse).kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(
            io::Error::from(CacheError::RelativePath).kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            io::Error::from(CacheError::LockNotAFile).kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn holds_dir_accepts_only_real_folders_at_the_right_depth() {
        let cache = tempfile::tempdir().unwrap();
        let entry = cache.path().join("builds/prj_a-00000000/debug-00000000");
        fs::create_dir_all(&entry).unwrap();
        let layout = Layout::open(cache.path()).unwrap().unwrap();
        let entry = layout.builds.join("prj_a-00000000/debug-00000000");
        assert!(layout.holds_dir(&entry, 2));
        assert!(!layout.holds_dir(&entry, 1));
        assert!(layout.holds_dir(entry.parent().unwrap(), 1));
        assert!(layout.holds_dir(&layout.builds, 0));
        assert!(!layout.holds_dir(&layout.builds.join("prj_a-00000000/../prj_a-00000000"), 1));
        assert!(!layout.holds_dir(&layout.builds.join("missing/entry"), 2));
        assert!(!layout.holds_dir(cache.path(), 0));
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            fs::create_dir(outside.path().join("entry")).unwrap();
            let link = layout.builds.join("prj_link-00000000");
            std::os::unix::fs::symlink(outside.path(), &link).unwrap();
            assert!(!layout.holds_dir(&link, 1));
            assert!(!layout.holds_dir(&link.join("entry"), 2));
        }
    }

    #[test]
    fn tree_size_counts_regular_files_only() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a/b/c")).unwrap();
        fs::write(dir.path().join("top"), [0u8; 10]).unwrap();
        fs::write(dir.path().join("a/b/c/deep"), [0u8; 32]).unwrap();
        assert_eq!(tree_size(dir.path()), 42);
        assert_eq!(tree_size(&dir.path().join("missing")), 0);
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            fs::write(outside.path().join("big"), [0u8; 1000]).unwrap();
            std::os::unix::fs::symlink(outside.path(), dir.path().join("a/link")).unwrap();
            std::os::unix::fs::symlink(outside.path().join("big"), dir.path().join("file-link")).unwrap();
            assert_eq!(tree_size(dir.path()), 42);
        }
    }

    #[test]
    fn a_lock_that_is_a_link_is_refused() {
        let entry = tempfile::tempdir().unwrap();
        fs::create_dir(entry.path().join(LOCK_FILE)).unwrap();
        let error = open_lock(entry.path(), false).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        #[cfg(unix)]
        {
            let entry = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let target = outside.path().join("victim");
            fs::write(&target, "keep").unwrap();
            std::os::unix::fs::symlink(&target, entry.path().join(LOCK_FILE)).unwrap();
            assert_eq!(
                open_lock(entry.path(), true).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            assert!(claim(entry.path()).is_err());
            assert_eq!(fs::read_to_string(&target).unwrap(), "keep");
        }
    }

    #[test]
    fn a_missing_lock_is_created_owner_only() {
        let entry = tempfile::tempdir().unwrap();
        let lock = claim(entry.path()).unwrap().unwrap();
        let path = entry.path().join(LOCK_FILE);
        assert!(fs::symlink_metadata(&path).unwrap().is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        // Held: a second claim fails without waiting.
        assert!(claim(entry.path()).unwrap().is_none());
        drop(lock);
        assert!(claim(entry.path()).unwrap().is_some());
    }
}
