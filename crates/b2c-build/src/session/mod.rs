//! Build sessions (`docs/spec/02-architecture.md` §2.6,
//! `docs/spec/07-toolchain-build-run.md` §7.5.4).
//!
//! Each build runs as a session on its own thread with a cancellation token.
//! [`BuildSessions::start`] returns the build's ID at once; the session
//! reports progress, diagnostics and exactly one `finished` event, always
//! the last, through the [`EventSink`] it was given (02 §2.5.3):
//!
//! * `progress` `generate` 0/1, then 1/1;
//! * `progress` `compile` done/total, once per translation unit as it
//!   finishes (a single unit is compiled and linked in one invocation:
//!   `compile` 0/1, then `compile` 1/1 and `link` 1/1);
//! * `progress` `link` 0/1, then 1/1, after every unit compiled;
//! * a `diagnostics` batch after each stage that reported any (the front
//!   end; the toolchain and its notes; the compilers, in translation-unit
//!   order; the linker);
//! * `finished` with the outcome, the project hash and the elapsed time.
//!
//! A project has at most one active build: starting another one for the
//! same project key cancels the earlier one, which then finishes as
//! `cancelled` (the new one waits for the build folder's lock, which the
//! cancelled one releases at once). A cancelled session stops at its next
//! check: between the front end's stages (load, resolve, analyse), while
//! the toolchain is being chosen, while it waits for the build folder's
//! lock, and between compiler steps. Cancelling kills the compiler's
//! process tree (`SIGTERM`, then `SIGKILL` after 2 s on Linux; the Job
//! Object on Windows) and deletes partial outputs and the manifest.
//!
//! The compiler is chosen on the session's thread, after
//! [`BuildSessions::start`] has returned the ID, and only for a project
//! without errors ([`BuildJob::toolchain`]), so a choice that waits for a
//! toolchain discovery never delays `build_start` and can be cancelled.
//!
//! The record of each build ([`BuildRecord`]) is kept until its project is
//! forgotten, at most [`MAX_RECORDS_PER_PROJECT`] per project (older ones are
//! dropped, and their IDs become unknown). No lock is held while a compiler
//! runs: the state lock only guards the maps of sessions and records.
//!
//! Logging: each session runs in a `build` span (`build_id`, `config`,
//! `ide`, `toolchain` version) with events for every step's duration and the
//! outcome. Paths appear only at debug level; source text, compiler output
//! and diagnostics never do.

mod record;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::{Duration, Instant};

use b2c_ipc::dto::BuildEvent;
use b2c_ipc::{BuildId, EventSink, IpcError};
use b2c_ir::Diagnostic;
use b2c_process::CancelToken;
use b2c_toolchain::probe::Toolchain;

pub use self::record::{BuildRecord, RecordOutcome, StaleReason};
use crate::compile::{Configuration, JobEvent, JobInput, JobResult, elapsed_ms, run_build_job};
use crate::frontend::FrontendOptions;
use crate::toolchains::Chosen;

/// The most build records kept per project; older ones are dropped.
pub const MAX_RECORDS_PER_PROJECT: usize = 8;

/// The toolchain a build uses, chosen by the caller's
/// [`BuildJob::toolchain`] (in the app, the toolchain registry's
/// [`crate::toolchains::ToolchainRegistry::choose_cancellable`], whose
/// [`Chosen`] converts with `into()`).
#[derive(Debug, Clone)]
pub enum ToolchainForBuild {
    /// A usable toolchain, with warnings and notes about it to show (for
    /// example a selection that fell back to another compiler). Its
    /// fingerprint is checked again before the build, and a changed compiler
    /// is probed again (`B2C-T1009`).
    Ready {
        /// The probed toolchain.
        toolchain: Box<Toolchain>,
        /// Diagnostics to report with the build.
        notes: Vec<Diagnostic>,
    },
    /// No usable toolchain; the build ends as `toolchainProblem` with these
    /// diagnostics (unless the project itself has errors).
    Unavailable {
        /// Why.
        diagnostics: Vec<Diagnostic>,
    },
}

impl From<Chosen> for ToolchainForBuild {
    fn from(chosen: Chosen) -> Self {
        match chosen {
            Chosen::Ready { toolchain, notes } => Self::Ready { toolchain, notes },
            Chosen::Unavailable { diagnostics } => Self::Unavailable { diagnostics },
        }
    }
}

/// One build to run.
///
/// ```no_run
/// # use std::sync::Arc;
/// # use b2c_build::{BuildJob, Configuration, FrontendOptions};
/// # use b2c_build::toolchains::ToolchainRegistry;
/// # fn job(registry: Arc<ToolchainRegistry>, bytes: Vec<u8>) -> BuildJob {
/// BuildJob {
///     project_key: String::from("ph_1"),
///     document: bytes,
///     configuration: Configuration::Debug,
///     // Runs on the session's thread, only for a project without errors.
///     toolchain: Box::new(move |cancel| registry.choose_cancellable(None, None, cancel).into()),
///     frontend: FrontendOptions::default(),
///     ide: true,
/// }
/// # }
/// ```
pub struct BuildJob {
    /// The project the build belongs to (for example its handle): one
    /// active build per key, and [`BuildSessions::forget_project`] drops its
    /// records.
    pub project_key: String,
    /// The project file's bytes (BDM JSON, untrusted). The backend always
    /// generates the C++ itself from it.
    pub document: Vec<u8>,
    /// Debug or release.
    pub configuration: Configuration,
    /// Chooses the compiler. It is called at most once, on the session's
    /// thread after [`BuildSessions::start`] has returned, and only when the
    /// front end found no errors, with the build's cancellation token. It
    /// may take a while (waiting for a toolchain discovery, probing a
    /// changed compiler) and should return soon after the token fires; the
    /// build then ends as `cancelled` whatever it returned.
    pub toolchain: Box<dyn FnOnce(&CancelToken) -> ToolchainForBuild + Send>,
    /// How C++ is generated (the indent width comes from the settings).
    pub frontend: FrontendOptions,
    /// Link the IDE init unit (the app's builds do).
    pub ide: bool,
}

impl std::fmt::Debug for BuildJob {
    /// The document's length, not its bytes; the toolchain choice is not
    /// shown.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildJob")
            .field("project_key", &self.project_key)
            .field("document", &format_args!("{} bytes", self.document.len()))
            .field("configuration", &self.configuration)
            .field("frontend", &self.frontend)
            .field("ide", &self.ide)
            .finish_non_exhaustive()
    }
}

/// A hook called when a build finishes (for example cache eviction).
type FinishedHook = Arc<dyn Fn(&BuildRecord) + Send + Sync>;

/// The build sessions of the app.
pub struct BuildSessions {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for BuildSessions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildSessions").finish_non_exhaustive()
    }
}

struct Shared {
    cache_root: PathBuf,
    state: Mutex<State>,
    /// Signalled whenever a session ends.
    ended: Condvar,
    on_finished: RwLock<Option<FinishedHook>>,
}

#[derive(Default)]
struct State {
    /// Every known build, running or finished.
    builds: HashMap<BuildId, Entry>,
    /// The latest build of each project key, while it runs.
    active: HashMap<String, BuildId>,
    /// The finished builds of each project key, oldest first.
    finished: HashMap<String, VecDeque<BuildId>>,
    /// Sessions whose thread has not ended yet (including cancelled ones).
    running: usize,
}

struct Entry {
    project_key: String,
    cancel: CancelToken,
    record: Option<Arc<BuildRecord>>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl BuildSessions {
    /// Build sessions whose builds go to `<cache_root>/builds/`. The cache
    /// root must be absolute (normally [`crate::default_cache_root`]; the
    /// command-line tool uses the same one).
    pub fn new(cache_root: PathBuf) -> Self {
        Self {
            shared: Arc::new(Shared {
                cache_root,
                state: Mutex::new(State::default()),
                ended: Condvar::new(),
                on_finished: RwLock::new(None),
            }),
        }
    }

    /// Starts a build on its own thread and returns its ID at once. An
    /// active build of the same project key is cancelled first. Events go
    /// to `sink`; the last one is always `finished`.
    ///
    /// # Errors
    /// [`IpcError::Internal`] when no random ID can be made or no thread
    /// can be started (nothing is started then).
    pub fn start(&self, job: BuildJob, sink: Arc<dyn EventSink<BuildEvent>>) -> Result<BuildId, IpcError> {
        let id = BuildId::random()?;
        let cancel = CancelToken::new();
        {
            let mut state = self.shared.lock();
            if let Some(previous) = state.active.get(&job.project_key)
                && let Some(entry) = state.builds.get(previous)
            {
                entry.cancel.cancel();
            }
            state.builds.insert(
                id.clone(),
                Entry {
                    project_key: job.project_key.clone(),
                    cancel: cancel.clone(),
                    record: None,
                },
            );
            state.active.insert(job.project_key.clone(), id.clone());
            state.running += 1;
        }
        let shared = Arc::clone(&self.shared);
        let thread_id = id.clone();
        let project_key = job.project_key.clone();
        let spawned = std::thread::Builder::new()
            .name(String::from("b2c-build"))
            .spawn(move || run_session(&shared, &thread_id, job, &cancel, sink.as_ref()));
        if let Err(error) = spawned {
            tracing::error!(error = %error, "could not start a build thread");
            let mut state = self.shared.lock();
            state.builds.remove(&id);
            if state.active.get(&project_key) == Some(&id) {
                state.active.remove(&project_key);
            }
            state.running = state.running.saturating_sub(1);
            return Err(IpcError::Internal);
        }
        Ok(id)
    }

    /// Cancels a build. Cancelling a build that has finished does nothing.
    ///
    /// # Errors
    /// [`IpcError::UnknownBuild`] when no build has this ID (any more).
    pub fn cancel(&self, id: &BuildId) -> Result<(), IpcError> {
        let state = self.shared.lock();
        let entry = state.builds.get(id).ok_or(IpcError::UnknownBuild)?;
        if entry.record.is_none() {
            entry.cancel.cancel();
        }
        Ok(())
    }

    /// Cancels the active build of a project, if any (Stop, or closing the
    /// project).
    pub fn cancel_project(&self, key: &str) {
        let state = self.shared.lock();
        if let Some(entry) = state.active.get(key).and_then(|id| state.builds.get(id)) {
            entry.cancel.cancel();
        }
    }

    /// Cancels every running build (app shutdown); see
    /// [`Self::wait_idle`].
    pub fn cancel_all(&self) {
        let state = self.shared.lock();
        for entry in state.builds.values().filter(|entry| entry.record.is_none()) {
            entry.cancel.cancel();
        }
    }

    /// Waits until no build session is running, at most `timeout`. Returns
    /// whether they all ended. After [`Self::cancel_all`] this takes at most
    /// the compilers' 2 s grace plus a moment, so the app can shut down
    /// without leaving compilers behind.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now().checked_add(timeout);
        let mut state = self.shared.lock();
        while state.running > 0 {
            let remaining = match deadline {
                Some(deadline) => match deadline.checked_duration_since(Instant::now()) {
                    Some(remaining) if !remaining.is_zero() => remaining,
                    _ => return false,
                },
                None => Duration::from_hours(1),
            };
            state = self
                .shared
                .ended
                .wait_timeout(state, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }

    /// The record of a finished build; `None` while it runs, or when the ID
    /// is unknown.
    pub fn record(&self, id: &BuildId) -> Option<Arc<BuildRecord>> {
        self.shared
            .lock()
            .builds
            .get(id)
            .and_then(|entry| entry.record.clone())
    }

    /// Forgets a project (it was closed): cancels its active build and drops
    /// every record of it. Its build IDs become unknown.
    pub fn forget_project(&self, key: &str) {
        let mut state = self.shared.lock();
        state.builds.retain(|_, entry| {
            if entry.project_key == key {
                entry.cancel.cancel();
                false
            } else {
                true
            }
        });
        state.active.remove(key);
        state.finished.remove(key);
    }

    /// Sets the hook called with every finished build's record, on the
    /// session's thread, after its `finished` event was sent. A later call
    /// replaces it.
    ///
    /// The app runs cache eviction there, keeping the build's own folder so
    /// the program that was just built is never deleted before it runs:
    /// [`crate::cache::prune_and_evict_keeping`] (or
    /// [`crate::cache::evict_to_cap_keeping`]) with
    /// `record.build_dir.as_deref()`.
    pub fn set_on_finished(&self, hook: Box<dyn Fn(&BuildRecord) + Send + Sync>) {
        let hook: FinishedHook = Arc::from(hook);
        *self
            .shared
            .on_finished
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Some(hook);
    }
}

impl Drop for BuildSessions {
    /// Cancels every running build, so no compiler outlives the sessions for
    /// long (their threads finish on their own).
    fn drop(&mut self) {
        self.cancel_all();
    }
}

/// Converts a build's own events to channel messages.
fn to_channel(event: JobEvent) -> BuildEvent {
    match event {
        JobEvent::Progress { stage, done, total } => BuildEvent::Progress { stage, done, total },
        JobEvent::Diagnostics(items) => BuildEvent::Diagnostics {
            items: b2c_ipc::diag::convert_all(&items),
        },
    }
}

/// The body of a session's thread.
fn run_session(
    shared: &Shared,
    id: &BuildId,
    job: BuildJob,
    cancel: &CancelToken,
    sink: &dyn EventSink<BuildEvent>,
) {
    let started = Instant::now();
    let BuildJob {
        project_key,
        document,
        configuration,
        toolchain,
        frontend,
        ide,
    } = job;
    let input = JobInput {
        build_id: Some(id.as_str()),
        document: &document,
        configuration,
        frontend: &frontend,
        ide,
        cache_root: &shared.cache_root,
    };
    // Once the receiver is gone, nothing more is sent; the build still
    // finishes, so its record is there if the project asks again.
    let mut open = true;
    let mut emit = |event: JobEvent| {
        if open {
            open = sink.send(to_channel(event));
        }
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_build_job(&input, toolchain, cancel, &mut emit)
    }))
    .unwrap_or_else(|_| {
        tracing::error!("a build session panicked; it is reported as failed");
        JobResult {
            outcome: RecordOutcome::Failed,
            failure: None,
            project_hash: None,
            config_key: String::new(),
            build_dir: None,
            executable: None,
            document: None,
            sanitizers: false,
            leak_detection: false,
            toolchain_bin: None,
        }
    });
    let record = Arc::new(BuildRecord {
        build_id: id.clone(),
        project_key,
        outcome: result.outcome,
        project_hash: result.project_hash,
        config_key: result.config_key,
        build_dir: result.build_dir,
        executable: result.executable,
        document: result.document,
        sanitizers: result.sanitizers,
        leak_detection: result.leak_detection,
        toolchain_bin: result.toolchain_bin,
        ide,
    });
    store_record(shared, id, &record);
    // The record is stored first, so a run started as soon as `finished`
    // arrives finds it.
    sink.send(BuildEvent::finished(
        record.outcome.into(),
        record.project_hash.as_ref(),
        elapsed_ms(started),
    ));
    let hook = shared
        .on_finished
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    if let Some(hook) = hook {
        hook(&record);
    }
    let mut state = shared.lock();
    state.running = state.running.saturating_sub(1);
    drop(state);
    shared.ended.notify_all();
}

/// Stores a finished build's record (unless its project was forgotten
/// meanwhile) and drops the oldest records over the per-project limit.
fn store_record(shared: &Shared, id: &BuildId, record: &Arc<BuildRecord>) {
    let mut state = shared.lock();
    let key = record.project_key.clone();
    if state.active.get(&key) == Some(id) {
        state.active.remove(&key);
    }
    let Some(entry) = state.builds.get_mut(id) else {
        return;
    };
    entry.record = Some(Arc::clone(record));
    let finished = state.finished.entry(key).or_default();
    finished.push_back(id.clone());
    let mut dropped = Vec::new();
    while finished.len() > MAX_RECORDS_PER_PROJECT {
        if let Some(oldest) = finished.pop_front() {
            dropped.push(oldest);
        }
    }
    for oldest in dropped {
        state.builds.remove(&oldest);
    }
}
