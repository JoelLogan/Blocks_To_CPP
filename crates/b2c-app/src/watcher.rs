//! Watching open project files for changes made outside the app
//! (`docs/spec/05-project-format.md` §5.10 "External changes",
//! `docs/spec/08-security.md` §8.3 "Re-check on outside change").
//!
//! For each open project with a file, the backend calls
//! [`Watchers::watch`] with the SHA-256 of the file as the app last read or
//! wrote it (the *baseline*): on open, reload, save, *Save as* and restore.
//! It calls [`Watchers::unwatch`] on close and [`Watchers::stop_all`] on
//! shutdown.
//!
//! * **What is watched.** The file's parent folder, non-recursively, with the
//!   operating system's change notifications (`notify`'s recommended watcher:
//!   inotify on Linux, `ReadDirectoryChangesW` on Windows), because other
//!   editors replace files by renaming. One OS watcher serves every project;
//!   each folder is watched once, however many open projects it holds. A
//!   folder that a check finds gone has lost its OS watch, which is set up
//!   again the next time a project there is watched (opened, saved,
//!   reloaded).
//! * **Events only wake the watcher.** An event that concerns a project's
//!   file (same folder and file name, ASCII letter case ignored), its folder
//!   itself, or everything (an overflowed event queue) makes the project due
//!   for a check [`WATCH_DEBOUNCE`] (300 ms) after the last such event, but
//!   at most [`WATCH_MAX_DELAY`] after the first. Reads (`open`, `close`
//!   without writing) and metadata changes are ignored, so the watcher's own
//!   reads never wake it.
//! * **The hash decides.** A check reads the file again with the project
//!   limit of 32 MiB (`b2c_store::read_project`, at most one byte more) on
//!   the watcher's own thread and compares its SHA-256 with the baseline. Only
//!   a difference is a change. A missing file, or something that is not a
//!   regular file, counts as deleted (`deleted: true`: *Reload* is then not
//!   available); a file over the limit counts as changed.
//! * **Once per change.** The frontend gets `projectChangedOnDisk { handle,
//!   deleted }` on the app channel when the file's state differs from the
//!   baseline and from the state last reported. A file that goes back to the
//!   baseline resets this, so the next change is reported again.
//! * **The app's own saves never notify.** A save holds its project
//!   ([`Watchers::hold`]) from before it writes until after it has passed the
//!   new baseline to [`Watchers::watch`]: checks wait while a project is
//!   held, and a check that read the file during a save, or before a new
//!   baseline arrived, is discarded and done again afterwards.
//! * **Failures are quiet.** If the OS watcher cannot be created or a folder
//!   cannot be watched (inotify limits, a folder that is gone), this is
//!   logged and the project goes unwatched; the hash check before every save
//!   (`project_save` refuses with `changedOnDisk`) still protects the file. A
//!   file that cannot be read for another reason (a sharing violation on
//!   Windows) is tried again a few times.
//! * **Private.** Paths and errors are logged at debug level only, and file
//!   content never.

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use b2c_ipc::Handle;
use b2c_ipc::dto::AppEvent;
use b2c_store::project_file::sha256;
use b2c_store::{ReadError, read_project};
use notify::event::{AccessKind, AccessMode, ModifyKind};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::events::AppEvents;

/// How long a project's file must be quiet (no event that concerns it)
/// before it is hashed again.
pub const WATCH_DEBOUNCE: Duration = Duration::from_millis(300);

/// The longest a check is put off while events keep coming, counted from
/// the first event since the last check.
pub const WATCH_MAX_DELAY: Duration = Duration::from_secs(2);

/// How many more times a file that could not be read (for a reason other
/// than not existing) is tried, [`WATCH_DEBOUNCE`] apart, before the watcher
/// waits for the next event.
const MAX_READ_RETRIES: u8 = 3;

/// The name of the watcher's checking thread.
const THREAD_NAME: &str = "b2c-file-watch";

/// What a project's file looks like on disk, as far as change detection is
/// concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FileState {
    /// A regular file of at most 32 MiB whose bytes have this SHA-256.
    Content([u8; 32]),
    /// No file: deleted, renamed away, or replaced by something that is not
    /// a regular file.
    Missing,
    /// A file larger than a project file may be (not read further).
    TooLarge,
}

impl FileState {
    /// The state of the file at `path` now, read with the project limit.
    ///
    /// # Errors
    /// The I/O error when the file exists but cannot be read.
    pub(crate) fn of(path: &Path) -> std::io::Result<Self> {
        match read_project(path) {
            Ok(bytes) => Ok(Self::Content(sha256(&bytes))),
            Err(ReadError::NotFound | ReadError::NotAFile) => Ok(Self::Missing),
            Err(ReadError::TooLarge { .. }) => Ok(Self::TooLarge),
            Err(ReadError::Io(error)) => Err(error),
        }
    }
}

/// The file watchers of the open projects (see the module documentation).
///
/// The OS watcher and the checking thread start with the first
/// [`Watchers::watch`], so a backend that never opens a file starts neither.
/// Dropping it stops both, as [`Watchers::stop_all`] does.
pub(crate) struct Watchers {
    /// The state shared with the OS watcher's callback and the checking
    /// thread.
    shared: Arc<Shared>,
    /// The OS side. Never locked while `shared.state` is, and never by the
    /// OS watcher's callback: `notify` calls the callback on its own thread,
    /// which its `watch` and `unwatch` wait for.
    os: Mutex<Os>,
}

impl std::fmt::Debug for Watchers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watchers")
            .field("projects", &self.shared.lock().projects.len())
            .finish_non_exhaustive()
    }
}

/// What the callback and the checking thread share.
struct Shared {
    /// Where `projectChangedOnDisk` goes.
    events: Arc<AppEvents>,
    state: Mutex<State>,
    /// Wakes the checking thread: a project became due, a hold ended, or the
    /// watchers stopped.
    wake: Condvar,
}

impl Shared {
    /// Locks the state, also after a panic elsewhere while it was locked:
    /// every change to it leaves it consistent.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The OS watcher, the folders it watches, and the checking thread.
#[derive(Default)]
struct Os {
    watcher: Option<RecommendedWatcher>,
    /// The folder watched for each project.
    folder_of: HashMap<Handle, PathBuf>,
    /// How many projects each watched folder serves.
    refs: HashMap<PathBuf, usize>,
    worker: Option<JoinHandle<()>>,
    stopped: bool,
}

/// The projects being watched and the holds of saves in progress.
#[derive(Debug, Default)]
struct State {
    projects: HashMap<Handle, Watched>,
    /// How many saves currently hold each project.
    holds: HashMap<Handle, usize>,
    /// Counts [`Watchers::watch`] calls, so a check knows whether the
    /// baseline it compared with is still current.
    generation: u64,
    /// Watched folders a check found gone: the OS dropped their watch, so it
    /// is set up again when a project there is watched next.
    lost_folders: HashSet<PathBuf>,
    stopped: bool,
}

/// One watched project.
#[derive(Debug, Clone)]
struct Watched {
    /// The project file (canonical).
    path: PathBuf,
    /// Its folder, as watched.
    folder: PathBuf,
    /// Its file name.
    name: OsString,
    /// The file as the app last read or wrote it.
    baseline: FileState,
    /// The state last reported to the frontend, while it differs from the
    /// baseline.
    reported: Option<FileState>,
    /// When the next check is due.
    due: Option<Instant>,
    /// The first event since the last check.
    pending_since: Option<Instant>,
    /// The `generation` of the `watch` call that set the baseline.
    generation: u64,
    /// Failed reads since the last good one.
    failures: u8,
}

/// A check the checking thread is to make.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Job {
    handle: Handle,
    path: PathBuf,
    folder: PathBuf,
    generation: u64,
}

/// What the checking thread does next.
#[derive(Debug, PartialEq, Eq)]
enum Next {
    /// Checks a project now.
    Check(Job),
    /// Sleeps until this time, or until woken.
    WaitUntil(Instant),
    /// Sleeps until woken.
    Wait,
    /// Ends: the watchers stopped.
    Stop,
}

impl State {
    /// Makes `watched` due after an event at `now`.
    fn mark(watched: &mut Watched, now: Instant) {
        let first = *watched.pending_since.get_or_insert(now);
        watched.due = Some((now + WATCH_DEBOUNCE).min(first + WATCH_MAX_DELAY));
    }

    /// Makes every project due (the OS lost events). Returns whether there
    /// was any.
    fn mark_all(&mut self, now: Instant) -> bool {
        for watched in self.projects.values_mut() {
            Self::mark(watched, now);
        }
        !self.projects.is_empty()
    }

    /// Makes the projects that `paths` concern due. Returns whether any did.
    fn mark_paths(&mut self, paths: &[PathBuf], now: Instant) -> bool {
        let mut marked = false;
        for watched in self.projects.values_mut() {
            if paths.iter().any(|path| concerns(watched, path)) {
                Self::mark(watched, now);
                marked = true;
            }
        }
        marked
    }

    /// The next thing the checking thread does at `now`. A project that is
    /// due is taken off the schedule; a held one waits for its hold to end.
    fn next(&mut self, now: Instant) -> Next {
        if self.stopped {
            return Next::Stop;
        }
        let mut earliest: Option<Instant> = None;
        let mut ready: Option<(&Handle, Instant)> = None;
        for (handle, watched) in &self.projects {
            let Some(due) = watched.due else {
                continue;
            };
            if self.holds.contains_key(handle) {
                continue;
            }
            if due <= now {
                // The most overdue first, so no project waits forever.
                if ready.is_none_or(|(_, other)| due < other) {
                    ready = Some((handle, due));
                }
            } else if earliest.is_none_or(|other| due < other) {
                earliest = Some(due);
            }
        }
        if let Some((handle, _)) = ready {
            let handle = handle.clone();
            if let Some(watched) = self.projects.get_mut(&handle) {
                watched.due = None;
                watched.pending_since = None;
                return Next::Check(Job {
                    handle,
                    path: watched.path.clone(),
                    folder: watched.folder.clone(),
                    generation: watched.generation,
                });
            }
        }
        earliest.map_or(Next::Wait, Next::WaitUntil)
    }

    /// Records the outcome of `job` (the file's state, or the error reading
    /// it) at `now`, and returns the event to send, if any.
    fn finish(&mut self, job: &Job, observed: std::io::Result<FileState>, now: Instant) -> Option<AppEvent> {
        let held = self.holds.contains_key(&job.handle);
        let watched = self.projects.get_mut(&job.handle)?;
        if watched.generation != job.generation {
            // A new baseline arrived while the file was read: this result
            // compared with the old one. Later events have their own check.
            return None;
        }
        if held {
            // A save is in progress: check again once it has ended.
            watched.due.get_or_insert(now);
            return None;
        }
        let state = match observed {
            Ok(state) => state,
            Err(error) => {
                watched.failures = watched.failures.saturating_add(1);
                tracing::debug!(path = %watched.path.display(), %error, "cannot read a watched project file");
                if watched.failures <= MAX_READ_RETRIES && watched.due.is_none() {
                    watched.due = Some(now + WATCH_DEBOUNCE);
                }
                return None;
            }
        };
        watched.failures = 0;
        if state == watched.baseline {
            watched.reported = None;
            return None;
        }
        if watched.reported == Some(state) {
            return None;
        }
        watched.reported = Some(state);
        tracing::debug!(path = %watched.path.display(), ?state, "a project file changed outside the app");
        Some(AppEvent::ProjectChangedOnDisk {
            handle: job.handle.clone(),
            deleted: state == FileState::Missing,
        })
    }

    /// Ends one hold of `handle`; when none is left, a check that waited for
    /// it is due after [`WATCH_DEBOUNCE`] (so late events of the save are
    /// taken in too). Returns whether the checking thread should wake.
    fn release(&mut self, handle: &Handle, now: Instant) -> bool {
        let Some(count) = self.holds.get_mut(handle) else {
            return false;
        };
        *count = count.saturating_sub(1);
        if *count > 0 {
            return false;
        }
        self.holds.remove(handle);
        match self.projects.get_mut(handle) {
            Some(watched) if watched.due.is_some() => {
                watched.due = Some(now + WATCH_DEBOUNCE);
                true
            }
            _ => false,
        }
    }
}

/// Whether an event about `path` may concern the file of `watched`: the file
/// itself (its name compared ignoring ASCII letter case, so a change spelled
/// differently on Windows is not missed), or the folder (removed or renamed
/// away). Extra matches only cost a hash, which decides.
fn concerns(watched: &Watched, path: &Path) -> bool {
    if path == watched.folder {
        return true;
    }
    path.parent() == Some(watched.folder.as_path())
        && path
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(&watched.name) || may_be_short_name(name))
}

/// Whether `name` may be a Windows 8.3 short name (`GAME~1.B2C`), which
/// `ReadDirectoryChangesW` reports when a program used it.
fn may_be_short_name(name: &OsStr) -> bool {
    cfg!(windows) && name.to_string_lossy().contains('~')
}

/// Whether an event of this kind can mean that a file's bytes changed:
/// reads and metadata changes cannot, and are ignored (the watcher's own
/// reads cause them).
fn may_change_content(kind: EventKind) -> bool {
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(_)) => false,
        _ => true,
    }
}

/// The OS watcher's callback, on `notify`'s thread: marks the projects the
/// event concerns as due and wakes the checking thread. It only takes the
/// state lock, briefly.
fn on_event(shared: &Weak<Shared>, result: &notify::Result<Event>) {
    let Some(shared) = shared.upgrade() else {
        return;
    };
    let now = Instant::now();
    let woke = {
        let mut state = shared.lock();
        match result {
            Ok(event) if event.need_rescan() || event.paths.is_empty() => {
                may_change_content(event.kind) && state.mark_all(now)
            }
            Ok(event) => may_change_content(event.kind) && state.mark_paths(&event.paths, now),
            Err(error) => {
                // Events may have been lost (a queue overflow, a limit):
                // check everything.
                tracing::debug!(%error, "the file watcher reported an error");
                state.mark_all(now)
            }
        }
    };
    if woke {
        shared.wake.notify_all();
    }
}

/// The checking thread: waits for due projects, hashes their files outside
/// the lock, and sends `projectChangedOnDisk` when [`State::finish`] says
/// so. Ends when the watchers stop.
fn run_checks(shared: &Shared) {
    loop {
        let job = {
            let mut state = shared.lock();
            loop {
                let now = Instant::now();
                match state.next(now) {
                    Next::Stop => return,
                    Next::Check(job) => break job,
                    Next::WaitUntil(until) => {
                        state = shared
                            .wake
                            .wait_timeout(state, until.saturating_duration_since(now))
                            .unwrap_or_else(PoisonError::into_inner)
                            .0;
                    }
                    Next::Wait => {
                        state = shared.wake.wait(state).unwrap_or_else(PoisonError::into_inner);
                    }
                }
            }
        };
        let observed = FileState::of(&job.path);
        let folder_gone = matches!(observed, Ok(FileState::Missing)) && !job.folder.is_dir();
        let event = {
            let mut state = shared.lock();
            if folder_gone {
                state.lost_folders.insert(job.folder.clone());
            }
            state.finish(&job, observed, Instant::now())
        };
        if let Some(event) = event {
            // Nobody subscribed is harmless: the save-time check remains.
            let _ = shared.events.send(event);
        }
    }
}

/// While alive, the watcher makes no check of one project (see
/// [`Watchers::hold`]).
#[must_use = "the hold ends when this is dropped"]
pub(crate) struct WatchHold<'a> {
    watchers: &'a Watchers,
    handle: Handle,
}

impl Drop for WatchHold<'_> {
    fn drop(&mut self) {
        let wake = self.watchers.shared.lock().release(&self.handle, Instant::now());
        if wake {
            self.watchers.shared.wake.notify_all();
        }
    }
}

impl Watchers {
    /// Watchers that report on `events`. Starts nothing yet.
    pub(crate) fn new(events: Arc<AppEvents>) -> Self {
        Self {
            shared: Arc::new(Shared {
                events,
                state: Mutex::new(State::default()),
                wake: Condvar::new(),
            }),
            os: Mutex::new(Os::default()),
        }
    }

    /// Starts watching `path` (a canonical project file) for `handle`, or
    /// replaces what was watched for it, with `baseline`, the SHA-256 of the
    /// file as the app last read or wrote it. Nothing is reported for the
    /// change that led here.
    pub(crate) fn watch(&self, handle: &Handle, path: &Path, baseline: [u8; 32]) {
        self.watch_state(handle, path, FileState::Content(baseline));
    }

    /// [`Watchers::watch`] with any baseline state: a restored project whose
    /// file is missing ([`FileState::Missing`]) is told when a file appears.
    pub(crate) fn watch_state(&self, handle: &Handle, path: &Path, baseline: FileState) {
        let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
            tracing::debug!(path = %path.display(), "a project path without a folder is not watched");
            return;
        };
        // Held throughout, so the project's state and its folder's watch
        // change together when several commands watch or unwatch at once.
        let mut os = self.lock_os();
        {
            let mut state = self.shared.lock();
            if state.stopped {
                return;
            }
            state.generation = state.generation.wrapping_add(1);
            let generation = state.generation;
            let (due, pending_since) = state
                .projects
                .get(handle)
                .map_or((None, None), |old| (old.due, old.pending_since));
            state.projects.insert(
                handle.clone(),
                Watched {
                    path: path.to_path_buf(),
                    folder: folder.to_path_buf(),
                    name: name.to_os_string(),
                    baseline,
                    reported: None,
                    // Events not yet checked are checked against the new
                    // baseline.
                    due,
                    pending_since,
                    generation,
                    failures: 0,
                },
            );
        }
        self.watch_folder(&mut os, handle, folder);
    }

    /// Stops watching for `handle` (nothing happens when it was not
    /// watched). Its folder stays watched while other projects use it.
    pub(crate) fn unwatch(&self, handle: &Handle) {
        let mut os = self.lock_os();
        self.shared.lock().projects.remove(handle);
        self.release_folder(&mut os, handle);
    }

    /// Holds `handle` while the app saves its file: no check is made until
    /// the returned guard is dropped, which the save does after passing the
    /// new baseline to [`Watchers::watch`] (or after failing, when the file
    /// still has the old one). The events of the save are then checked
    /// against the baseline current at that time, so the app's own saves
    /// never notify. Holds of one project may overlap.
    pub(crate) fn hold(&self, handle: &Handle) -> WatchHold<'_> {
        *self.shared.lock().holds.entry(handle.clone()).or_insert(0) += 1;
        WatchHold {
            watchers: self,
            handle: handle.clone(),
        }
    }

    /// Stops every watcher and the checking thread (shutdown). Final: later
    /// [`Watchers::watch`] calls do nothing. Idempotent.
    pub(crate) fn stop_all(&self) {
        {
            let mut state = self.shared.lock();
            state.stopped = true;
            state.projects.clear();
        }
        self.shared.wake.notify_all();
        let (watcher, worker) = {
            let mut os = self.lock_os();
            os.stopped = true;
            os.folder_of.clear();
            os.refs.clear();
            (os.watcher.take(), os.worker.take())
        };
        // Dropping the OS watcher ends its thread; outside the lock.
        drop(watcher);
        if let Some(worker) = worker
            && worker.thread().id() != std::thread::current().id()
            && worker.join().is_err()
        {
            tracing::warn!("the file watcher's thread ended with a panic");
        }
    }

    /// Makes sure `folder` is watched for `handle`, starting the OS watcher
    /// and the checking thread if needed; stops watching the folder the
    /// project used before when no other project needs it; and sets the
    /// folder's watch up again when a check found the folder gone (the OS
    /// dropped the watch) and it exists again.
    fn watch_folder(&self, guard: &mut MutexGuard<'_, Os>, handle: &Handle, folder: &Path) {
        let os = &mut **guard;
        if os.stopped {
            return;
        }
        let same = os.folder_of.get(handle).is_some_and(|current| current == folder);
        if !same {
            self.release_folder(os, handle);
        }
        self.start(os);
        let Some(watcher) = os.watcher.as_mut() else {
            return;
        };
        let watched_already = os.refs.contains_key(folder);
        let lost = self.take_lost(folder);
        if lost && watched_already {
            // Unknown to the OS watcher by now, or still set on the folder
            // that was renamed away: either way, start afresh.
            let _ = watcher.unwatch(folder);
        }
        if (lost || !watched_already)
            && let Err(error) = watcher.watch(folder, RecursiveMode::NonRecursive)
        {
            tracing::debug!(folder = %folder.display(), %error, "cannot watch a project folder");
            tracing::warn!("a project folder cannot be watched; outside changes are found when saving");
            if watched_already {
                // Other projects still count on it: try again next time.
                self.shared.lock().lost_folders.insert(folder.to_path_buf());
            } else {
                return;
            }
        }
        if !same {
            *os.refs.entry(folder.to_path_buf()).or_insert(0) += 1;
            os.folder_of.insert(handle.clone(), folder.to_path_buf());
        }
    }

    /// Whether `folder` was found gone and exists again (and is then no
    /// longer marked).
    fn take_lost(&self, folder: &Path) -> bool {
        if !self.shared.lock().lost_folders.contains(folder) {
            return false;
        }
        // Outside the state lock: a file-system call.
        let exists = folder.is_dir();
        exists && self.shared.lock().lost_folders.remove(folder)
    }

    /// Forgets the folder `handle` watched, and stops watching it when it
    /// was the last project there.
    fn release_folder(&self, os: &mut Os, handle: &Handle) {
        let Some(folder) = os.folder_of.remove(handle) else {
            return;
        };
        let Some(count) = os.refs.get_mut(&folder) else {
            return;
        };
        *count = count.saturating_sub(1);
        if *count > 0 {
            return;
        }
        os.refs.remove(&folder);
        self.shared.lock().lost_folders.remove(&folder);
        if let Some(watcher) = os.watcher.as_mut()
            && let Err(error) = watcher.unwatch(&folder)
        {
            tracing::debug!(folder = %folder.display(), %error, "cannot stop watching a project folder");
        }
    }

    /// Starts the checking thread and the OS watcher when they are not
    /// running yet; a failure is logged and tried again at the next watch.
    fn start(&self, os: &mut Os) {
        if os.worker.is_none() {
            let shared = Arc::clone(&self.shared);
            match std::thread::Builder::new()
                .name(String::from(THREAD_NAME))
                .spawn(move || run_checks(&shared))
            {
                Ok(worker) => os.worker = Some(worker),
                Err(error) => tracing::warn!(%error, "the file watcher's thread could not start"),
            }
        }
        if os.watcher.is_none() {
            let shared = Arc::downgrade(&self.shared);
            match notify::recommended_watcher(move |result: notify::Result<Event>| on_event(&shared, &result))
            {
                Ok(watcher) => os.watcher = Some(watcher),
                Err(error) => {
                    tracing::debug!(%error, "cannot create the file watcher");
                    tracing::warn!("file watching is not available; outside changes are found when saving");
                }
            }
        }
    }

    fn lock_os(&self) -> MutexGuard<'_, Os> {
        self.os.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for Watchers {
    fn drop(&mut self) {
        self.stop_all();
    }
}

#[cfg(test)]
mod tests {
    use b2c_ipc::sink::testing::RecordingSink;

    use super::*;

    fn handle(n: u8) -> Handle {
        Handle::parse(&format!("ph_{}", format!("{n:02x}").repeat(16))).unwrap()
    }

    fn watched(path: &str, baseline: FileState, generation: u64) -> Watched {
        let path = PathBuf::from(path);
        Watched {
            folder: path.parent().unwrap().to_path_buf(),
            name: path.file_name().unwrap().to_os_string(),
            path,
            baseline,
            reported: None,
            due: None,
            pending_since: None,
            generation,
            failures: 0,
        }
    }

    fn state_with(projects: Vec<(Handle, Watched)>) -> State {
        State {
            projects: projects.into_iter().collect(),
            ..State::default()
        }
    }

    fn job(state: &mut State, now: Instant) -> Job {
        match state.next(now) {
            Next::Check(job) => job,
            other => panic!("expected a check, got {other:?}"),
        }
    }

    const A: [u8; 32] = [1; 32];
    const B: [u8; 32] = [2; 32];
    const C: [u8; 32] = [3; 32];

    #[test]
    fn only_content_changes_wake_the_watcher() {
        use notify::event::{CreateKind, DataChange, MetadataKind, RemoveKind, RenameMode};
        assert!(may_change_content(EventKind::Modify(ModifyKind::Data(
            DataChange::Any
        ))));
        assert!(may_change_content(EventKind::Modify(ModifyKind::Name(
            RenameMode::To
        ))));
        assert!(may_change_content(EventKind::Create(CreateKind::File)));
        assert!(may_change_content(EventKind::Remove(RemoveKind::Any)));
        assert!(may_change_content(EventKind::Access(AccessKind::Close(
            AccessMode::Write
        ))));
        assert!(may_change_content(EventKind::Any));
        assert!(may_change_content(EventKind::Other));
        assert!(!may_change_content(EventKind::Access(AccessKind::Open(
            AccessMode::Any
        ))));
        assert!(!may_change_content(EventKind::Access(AccessKind::Close(
            AccessMode::Read
        ))));
        assert!(!may_change_content(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Any
        ))));
    }

    #[test]
    fn events_concern_the_file_and_its_folder() {
        let w = watched("/p/games/Game.b2c", FileState::Content(A), 1);
        assert!(concerns(&w, Path::new("/p/games/Game.b2c")));
        assert!(concerns(&w, Path::new("/p/games/GAME.B2C")));
        assert!(concerns(&w, Path::new("/p/games")));
        assert!(!concerns(&w, Path::new("/p/games/Game.b2c.bak")));
        assert!(!concerns(&w, Path::new("/p/games/.b2c-1234.tmp")));
        assert!(!concerns(&w, Path::new("/p/other/Game.b2c")));
        assert!(!concerns(&w, Path::new("/p")));
        assert_eq!(concerns(&w, Path::new("/p/games/GAME~1.B2C")), cfg!(windows));
    }

    #[test]
    fn events_are_debounced_per_project() {
        let start = Instant::now();
        let mut state = state_with(vec![
            (handle(1), watched("/p/a.b2c", FileState::Content(A), 1)),
            (handle(2), watched("/q/b.b2c", FileState::Content(A), 1)),
        ]);
        assert_eq!(state.next(start), Next::Wait);
        assert!(state.mark_paths(&[PathBuf::from("/p/a.b2c")], start));
        assert!(!state.mark_paths(&[PathBuf::from("/p/other.b2c")], start));
        assert_eq!(state.next(start), Next::WaitUntil(start + WATCH_DEBOUNCE));
        // Another event pushes the check back.
        let later = start + Duration::from_millis(200);
        state.mark_paths(&[PathBuf::from("/p/a.b2c")], later);
        assert_eq!(state.next(later), Next::WaitUntil(later + WATCH_DEBOUNCE));
        let due = later + WATCH_DEBOUNCE;
        let job = job(&mut state, due);
        assert_eq!(job.handle, handle(1));
        assert_eq!(job.path, PathBuf::from("/p/a.b2c"));
        // Taken off the schedule; the other project was never due.
        assert_eq!(state.next(due), Next::Wait);
    }

    #[test]
    fn a_stream_of_events_is_checked_at_most_after_the_longest_delay() {
        let start = Instant::now();
        let mut state = state_with(vec![(handle(1), watched("/p/a.b2c", FileState::Content(A), 1))]);
        let mut now = start;
        while now < start + WATCH_MAX_DELAY + WATCH_DEBOUNCE {
            state.mark_paths(&[PathBuf::from("/p/a.b2c")], now);
            now += Duration::from_millis(100);
        }
        assert_eq!(state.projects[&handle(1)].due, Some(start + WATCH_MAX_DELAY));
    }

    #[test]
    fn lost_events_check_everything() {
        let now = Instant::now();
        let mut state = state_with(vec![
            (handle(1), watched("/p/a.b2c", FileState::Content(A), 1)),
            (handle(2), watched("/q/b.b2c", FileState::Content(A), 1)),
        ]);
        assert!(state.mark_all(now));
        assert!(state.projects.values().all(|w| w.due.is_some()));
        assert!(!State::default().mark_all(now));
    }

    #[test]
    fn changes_are_reported_once_and_reset_at_the_baseline() {
        let now = Instant::now();
        let h = handle(1);
        let mut state = state_with(vec![(h.clone(), watched("/p/a.b2c", FileState::Content(A), 7))]);
        let job = Job {
            handle: h.clone(),
            path: PathBuf::from("/p/a.b2c"),
            folder: PathBuf::from("/p"),
            generation: 7,
        };
        let changed = AppEvent::ProjectChangedOnDisk {
            handle: h.clone(),
            deleted: false,
        };
        // Unchanged: nothing.
        assert_eq!(state.finish(&job, Ok(FileState::Content(A)), now), None);
        // Changed: once.
        assert_eq!(
            state.finish(&job, Ok(FileState::Content(B)), now),
            Some(changed.clone())
        );
        assert_eq!(state.finish(&job, Ok(FileState::Content(B)), now), None);
        // Changed again: a new change.
        assert_eq!(
            state.finish(&job, Ok(FileState::Content(C)), now),
            Some(changed.clone())
        );
        // Deleted.
        assert_eq!(
            state.finish(&job, Ok(FileState::Missing), now),
            Some(AppEvent::ProjectChangedOnDisk {
                handle: h.clone(),
                deleted: true,
            })
        );
        assert_eq!(state.finish(&job, Ok(FileState::Missing), now), None);
        // Back to the baseline, then the same change again: reported again.
        assert_eq!(state.finish(&job, Ok(FileState::Content(A)), now), None);
        assert_eq!(
            state.finish(&job, Ok(FileState::Content(B)), now),
            Some(changed.clone())
        );
        // Too large counts as changed, not deleted.
        assert_eq!(state.finish(&job, Ok(FileState::TooLarge), now), Some(changed));
    }

    #[test]
    fn a_missing_baseline_reports_a_file_that_appears() {
        let now = Instant::now();
        let h = handle(1);
        let mut state = state_with(vec![(h.clone(), watched("/p/a.b2c", FileState::Missing, 1))]);
        let job = Job {
            handle: h.clone(),
            path: PathBuf::from("/p/a.b2c"),
            folder: PathBuf::from("/p"),
            generation: 1,
        };
        assert_eq!(state.finish(&job, Ok(FileState::Missing), now), None);
        assert_eq!(
            state.finish(&job, Ok(FileState::Content(A)), now),
            Some(AppEvent::ProjectChangedOnDisk {
                handle: h,
                deleted: false
            })
        );
    }

    #[test]
    fn results_for_an_old_baseline_or_an_unwatched_project_are_dropped() {
        let now = Instant::now();
        let h = handle(1);
        let mut state = state_with(vec![(h.clone(), watched("/p/a.b2c", FileState::Content(A), 2))]);
        let old_job = Job {
            handle: h.clone(),
            path: PathBuf::from("/p/a.b2c"),
            folder: PathBuf::from("/p"),
            generation: 1,
        };
        assert_eq!(state.finish(&old_job, Ok(FileState::Content(B)), now), None);
        assert_eq!(state.projects[&h].reported, None);
        let other = Job {
            handle: handle(9),
            path: PathBuf::from("/p/a.b2c"),
            folder: PathBuf::from("/p"),
            generation: 2,
        };
        assert_eq!(state.finish(&other, Ok(FileState::Content(B)), now), None);
    }

    #[test]
    fn held_projects_wait_for_the_end_of_the_save() {
        let start = Instant::now();
        let h = handle(1);
        let mut state = state_with(vec![(h.clone(), watched("/p/a.b2c", FileState::Content(A), 1))]);
        state.holds.insert(h.clone(), 2);
        state.mark_paths(&[PathBuf::from("/p/a.b2c")], start);
        let late = start + Duration::from_secs(5);
        // Due, but held: no check, and no timed wake-up either.
        assert_eq!(state.next(late), Next::Wait);
        // A check that read the file during the save is put back.
        let job = Job {
            handle: h.clone(),
            path: PathBuf::from("/p/a.b2c"),
            folder: PathBuf::from("/p"),
            generation: 1,
        };
        state.projects.get_mut(&h).unwrap().due = None;
        assert_eq!(state.finish(&job, Ok(FileState::Content(B)), late), None);
        assert_eq!(state.projects[&h].due, Some(late));
        // The first of two holds ends: still held.
        assert!(!state.release(&h, late));
        assert_eq!(state.next(late), Next::Wait);
        // The last one ends: due again after the debounce.
        assert!(state.release(&h, late));
        assert!(state.holds.is_empty());
        assert_eq!(state.next(late), Next::WaitUntil(late + WATCH_DEBOUNCE));
        // Releasing a project that is not held does nothing.
        assert!(!state.release(&h, late));
    }

    #[test]
    fn read_errors_are_retried_a_few_times() {
        let now = Instant::now();
        let h = handle(1);
        let mut state = state_with(vec![(h.clone(), watched("/p/a.b2c", FileState::Content(A), 1))]);
        let job = Job {
            handle: h.clone(),
            path: PathBuf::from("/p/a.b2c"),
            folder: PathBuf::from("/p"),
            generation: 1,
        };
        for _ in 0..MAX_READ_RETRIES {
            let denied = Err(std::io::ErrorKind::PermissionDenied.into());
            assert_eq!(state.finish(&job, denied, now), None);
            assert_eq!(state.projects[&h].due, Some(now + WATCH_DEBOUNCE));
            state.projects.get_mut(&h).unwrap().due = None;
        }
        let denied = Err(std::io::ErrorKind::PermissionDenied.into());
        assert_eq!(state.finish(&job, denied, now), None);
        assert_eq!(state.projects[&h].due, None);
        // A good read resets the count.
        assert_eq!(state.finish(&job, Ok(FileState::Content(A)), now), None);
        assert_eq!(state.projects[&h].failures, 0);
    }

    #[test]
    fn stopped_state_stops_the_thread() {
        let mut state = State {
            stopped: true,
            ..State::default()
        };
        assert_eq!(state.next(Instant::now()), Next::Stop);
    }

    #[test]
    fn file_states_are_read_with_the_project_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.b2c");
        assert_eq!(FileState::of(&path).unwrap(), FileState::Missing);
        std::fs::write(&path, b"{}").unwrap();
        assert_eq!(FileState::of(&path).unwrap(), FileState::Content(sha256(b"{}")));
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert_eq!(FileState::of(&path).unwrap(), FileState::Missing);
    }

    #[test]
    fn watching_and_stopping_without_events() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().canonicalize().unwrap();
        let events = Arc::new(AppEvents::new(Vec::new()));
        let sink = Arc::new(RecordingSink::new());
        events.subscribe(sink.clone());
        let watchers = Watchers::new(events);
        let one = handle(1);
        let two = handle(2);
        watchers.watch(&one, &folder.join("a.b2c"), A);
        watchers.watch(&two, &folder.join("b.b2c"), A);
        {
            let os = watchers.lock_os();
            assert_eq!(os.refs.get(&folder), Some(&2));
            assert!(os.worker.is_some());
        }
        // Watching again with a new baseline keeps one reference.
        watchers.watch(&one, &folder.join("a.b2c"), B);
        assert_eq!(watchers.lock_os().refs.get(&folder), Some(&2));
        watchers.unwatch(&one);
        assert_eq!(watchers.lock_os().refs.get(&folder), Some(&1));
        watchers.unwatch(&two);
        watchers.unwatch(&two);
        assert!(watchers.lock_os().refs.is_empty());
        {
            let _hold = watchers.hold(&one);
            assert_eq!(watchers.shared.lock().holds.get(&one), Some(&1));
        }
        assert!(watchers.shared.lock().holds.is_empty());
        watchers.stop_all();
        watchers.stop_all();
        watchers.watch(&one, &folder.join("a.b2c"), A);
        assert!(watchers.shared.lock().projects.is_empty());
        assert!(watchers.lock_os().worker.is_none());
        assert!(sink.is_empty());
    }

    #[test]
    fn a_path_without_a_folder_is_not_watched() {
        let watchers = Watchers::new(Arc::new(AppEvents::new(Vec::new())));
        watchers.watch(&handle(1), Path::new("/"), A);
        assert!(watchers.shared.lock().projects.is_empty());
        assert!(watchers.lock_os().worker.is_none());
    }
}
