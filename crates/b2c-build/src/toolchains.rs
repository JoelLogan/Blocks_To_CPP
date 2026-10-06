//! Choosing a compiler (`docs/spec/07-toolchain-build-run.md` §7.2–7.3).
//!
//! [`ToolchainRegistry`] is the toolchain list behind the app's toolchain
//! commands (`toolchain_list`, `toolchain_rescan`, `toolchain_add_dialog`,
//! `toolchain_select`, `toolchain_setup_info`) and the choice of compiler for
//! every app build:
//!
//! * **Opening never probes.** [`ToolchainRegistry::open`] only reads
//!   `toolchains.json` (format in [`STORE_FORMAT`] and 05 §5.9), so
//!   [`ToolchainRegistry::list`] answers at once with the cached results.
//!   [`ToolchainRegistry::spawn_discovery`] then discovers and probes on a
//!   background thread and calls back when it is done, so the app can send
//!   the `toolchainsUpdated` event.
//! * **Discovery** ([`ToolchainRegistry::rescan`]) follows the search order
//!   of [`b2c_toolchain::discovery`]. The folders the caller excludes (the
//!   open projects' folders), the cache root (the folder that holds the
//!   probes' folder) and the process's current directory are never
//!   searched. A compiler whose fingerprint is unchanged
//!   is not probed again; the others are probed at most
//!   [`MAX_PARALLEL_PROBES`] at a time, each invocation with the 10 s
//!   [`PROBE_TIMEOUT`], in a private temporary folder, with the sanitised
//!   environment ([`HostEnv::from_process`] with no pass-through). Scans
//!   that overlap are ordered by an epoch: only the newest one's result is
//!   kept, so a slow background discovery never overwrites a newer rescan.
//! * **Manual compilers** ([`ToolchainRegistry::add_explicit`], for *Choose
//!   g++ manually…*) are checked by
//!   [`explicit_candidate`] plus the name rules of 07 §7.2 (never `.bat` or
//!   `.cmd`; on Windows only a file named exactly `g++.exe`), canonicalised,
//!   refused (`B2C-T1002`) inside the folders the caller excludes (the open
//!   projects' folders and the cache root) before anything runs them,
//!   probed and kept with the source `manual`, even when they fail their
//!   health checks. A rescan keeps them, probes them again when they
//!   changed, and forgets them only when their file is gone.
//! * **Choosing** ([`ToolchainRegistry::choose`]): the selected toolchain
//!   first, checked again (fingerprint; probed again with a `B2C-T1009` note
//!   when it changed), then the others in discovery order. A selection that
//!   is missing, unusable or inside the project's folder gives the warning
//!   `B2C-T1022` and falls back, never silently; any compiler inside the
//!   project's folder is refused (`B2C-T1002`, binary planting, 08 §8.5).
//!   A build session chooses with [`ToolchainRegistry::choose_cancellable`],
//!   which stops waiting for a discovery, and stops probing, as soon as the
//!   build is cancelled.
//!
//! The command-line tool keeps its own simpler entry points
//! ([`ToolchainChoice`] for `b2c build --toolchain`, [`list_toolchains`] for
//! `b2c toolchains`) over the same file: the CLI and the app share
//! `toolchains.json` in the machine folder (02 §2.7, 07 §7.9). A compiler
//! given with `--toolchain` that is not in the list yet is remembered as a
//! manual one.
//!
//! Logging: the end of each discovery is an `info` event with counts and the
//! duration; paths are logged at `debug` level only (08 §8.11).

mod dto;
mod store;

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use b2c_ipc::ToolchainId;
use b2c_ipc::dto::{
    Distro, Platform as IpcPlatform, Toolchain as ToolchainDto, ToolchainListResponse, ToolchainSetupInfo,
    ToolchainSource,
};
use b2c_ir::{DiagSource, Diagnostic, Location, Severity};
use b2c_process::CancelToken;
use b2c_toolchain::codes::{self, SelectionProblem};
use b2c_toolchain::discovery::{Candidate, CandidateSource, DiscoveryEnv, discover, explicit_candidate};
use b2c_toolchain::env::HostEnv;
use b2c_toolchain::fingerprint::Fingerprint;
use b2c_toolchain::probe::{
    Capabilities, CompilerKind, PROBE_FORMAT, PROBE_TIMEOUT, ProbeError, ProbeOptions, Toolchain,
};
use b2c_toolchain::target::{Platform, Target};
use serde::Serialize;

pub use dto::{flavor, toolchain_dto};
use store::Entry;
pub use store::{
    MAX_DISCOVERED, MAX_MANUAL, MAX_STORE_BYTES, MAX_TOOLCHAINS, STORE_FILE, STORE_FORMAT,
    STORE_FORMAT_VERSION,
};

/// The probes' temporary folder inside the cache root (`<cache>/probe-tmp`);
/// each probe makes its own private folder in it.
pub const PROBE_TEMP_DIR: &str = "probe-tmp";

/// The most compilers probed at the same time (07 §7.2).
pub const MAX_PARALLEL_PROBES: usize = 4;

/// How long [`ToolchainRegistry::choose`] waits for a running discovery
/// before it chooses from what is known.
pub const DISCOVERY_WAIT: Duration = Duration::from_secs(30);

/// How often [`ToolchainRegistry::choose_cancellable`] looks at the build's
/// cancellation token while it waits for a discovery.
const CANCEL_POLL: Duration = Duration::from_millis(50);

/// Probes a compiler. [`RealProber`] runs g++; tests substitute their own.
pub trait Prober: Send + Sync {
    /// Probes the compiler at `path` (canonical and absolute) with
    /// `options`.
    ///
    /// # Errors
    /// [`ProbeError`] when the compiler cannot be probed at all (a compiler
    /// that runs but is broken, too old or not GCC is a [`Toolchain`] with
    /// problems instead).
    fn probe(&self, path: &Path, options: &ProbeOptions) -> Result<Toolchain, ProbeError>;
}

/// The real [`Prober`]: [`b2c_toolchain::probe::probe`].
#[derive(Debug, Clone, Copy, Default)]
pub struct RealProber;

impl Prober for RealProber {
    fn probe(&self, path: &Path, options: &ProbeOptions) -> Result<Toolchain, ProbeError> {
        b2c_toolchain::probe::probe(path, options)
    }
}

/// Where discovery looks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum DiscoveryScope {
    /// The whole search order of 07 §7.2, from this process's environment
    /// ([`DiscoveryEnv::from_process`]).
    #[default]
    Default,
    /// Only these folders, searched like `PATH` entries in this order, and
    /// no well-known location: for end-to-end tests
    /// (`B2C_E2E_TOOLCHAIN_DIRS`) and tests. The exclusions still apply,
    /// including the current directory.
    Only(Vec<PathBuf>),
}

/// The toolchain for a build, from [`ToolchainRegistry::choose`].
#[derive(Debug, Clone, PartialEq)]
pub enum Chosen {
    /// A usable toolchain, checked just now.
    Ready {
        /// The toolchain.
        toolchain: Box<Toolchain>,
        /// Warnings and notes to show with the build: `B2C-T1022` when the
        /// selection could not be used, `B2C-T1009` when the compiler was
        /// probed again, and the toolchain's own warnings and notes. None is
        /// an error.
        notes: Vec<Diagnostic>,
    },
    /// No usable toolchain.
    Unavailable {
        /// Why: `B2C-T1022` when a selection could not be used,
        /// `B2C-T1001`, then the errors of each toolchain that was refused.
        diagnostics: Vec<Diagnostic>,
    },
}

/// The app's toolchain list (see the module documentation). It is shared
/// between threads (`Arc<ToolchainRegistry>`); every method is safe to call
/// concurrently. [`ToolchainRegistry::rescan`],
/// [`ToolchainRegistry::add_explicit`] and [`ToolchainRegistry::choose`] can
/// run compilers and take seconds, so they must not run on a UI thread.
pub struct ToolchainRegistry {
    /// `toolchains.json`, or `None` to keep the list in memory only.
    store: Option<PathBuf>,
    /// The folder the probes make their private folders in (`None`: the
    /// system's temporary folder).
    probe_temp: Option<PathBuf>,
    scope: DiscoveryScope,
    prober: Arc<dyn Prober>,
    state: Mutex<State>,
    /// Signalled whenever a scan ends.
    idle: Condvar,
}

/// The list and the scans running on it.
#[derive(Debug, Default)]
struct State {
    /// Discovered toolchains in discovery order, then the manual ones in the
    /// order they were added. Unique by canonical path.
    entries: Vec<Entry>,
    /// Incremented when a scan starts; a scan's result is kept only while
    /// no newer scan has started.
    epoch: u64,
    /// Scans running now (background discoveries and rescans).
    scanning: usize,
    /// Whether a scan has finished since the registry was opened.
    scanned: bool,
    /// What the last scan excluded, for a scan that `choose` starts itself.
    last_excluded: Vec<PathBuf>,
}

impl fmt::Debug for ToolchainRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = f.debug_struct("ToolchainRegistry");
        out.field("store", &self.store).field("scope", &self.scope);
        if let Ok(state) = self.state.try_lock() {
            out.field("toolchains", &state.entries.len())
                .field("scanning", &state.scanning);
        }
        out.finish_non_exhaustive()
    }
}

impl ToolchainRegistry {
    /// Opens the list kept in `store` (`<Dirs.machine>/toolchains.json`),
    /// probing nothing. `probe_temp` (`<Dirs.cache>/probe-tmp`) is created
    /// owner-only when a probe first needs it; when it cannot be, probes use
    /// the system's temporary folder (each probe's own folder is private
    /// either way). A missing, unreadable or invalid file is an empty list.
    pub fn open(store: PathBuf, probe_temp: PathBuf, scope: DiscoveryScope, prober: Arc<dyn Prober>) -> Self {
        Self::with_store(Some(store), Some(probe_temp), scope, prober)
    }

    /// [`ToolchainRegistry::open`] with an optional store and probe folder.
    fn with_store(
        store: Option<PathBuf>,
        probe_temp: Option<PathBuf>,
        scope: DiscoveryScope,
        prober: Arc<dyn Prober>,
    ) -> Self {
        let entries = store.as_deref().map(store::load).unwrap_or_default();
        tracing::debug!(cached = entries.len(), "toolchain list loaded");
        Self {
            store,
            probe_temp,
            scope,
            prober,
            state: Mutex::new(State {
                entries,
                ..State::default()
            }),
            idle: Condvar::new(),
        }
    }

    /// The list as it is now, without probing (`toolchain_list`), with the
    /// `selected` toolchain marked. `discovering` says whether a scan is
    /// running.
    pub fn list(&self, selected: Option<&ToolchainId>) -> ToolchainListResponse {
        let state = self.lock();
        ToolchainListResponse {
            toolchains: state
                .entries
                .iter()
                .map(|entry| entry_dto(entry, selected))
                .collect(),
            discovering: state.scanning > 0,
        }
    }

    /// Discovers and probes again (`toolchain_rescan`), blocking until done,
    /// and returns the new list. `excluded` holds the folders never searched
    /// (the open projects' folders and the cache root); the cache root
    /// (`probe_temp`'s parent) and the current directory are always excluded
    /// too. When a newer scan started meanwhile, its result wins and this
    /// returns the list as it is.
    pub fn rescan(&self, excluded: &[PathBuf], selected: Option<&ToolchainId>) -> ToolchainListResponse {
        self.begin_scan();
        {
            let _running = ScanGuard {
                registry: self,
                done: None,
            };
            self.scan(excluded);
        }
        self.list(selected)
    }

    /// Starts a discovery on a background thread (at app startup) and calls
    /// `done` when it is over, whatever happened (also when the thread could
    /// not start). [`ToolchainRegistry::list`] reports `discovering` from
    /// the moment this returns.
    pub fn spawn_discovery(self: &Arc<Self>, excluded: Vec<PathBuf>, done: Box<dyn FnOnce() + Send>) {
        self.begin_scan();
        let guard = ScanGuard {
            registry: Arc::clone(self),
            done: Some(done),
        };
        let spawned = std::thread::Builder::new()
            .name(String::from("b2c-toolchain-discovery"))
            .spawn(move || {
                guard.registry.scan(&excluded);
                drop(guard);
            });
        // On failure the closure, and with it the guard, is dropped: the
        // scan count goes down and `done` runs.
        if let Err(error) = spawned {
            tracing::warn!(%error, "toolchain discovery could not start");
        }
    }

    /// Adds a compiler the user picked (`toolchain_add_dialog`): it must
    /// pass [`explicit_candidate`] (absolute, an executable file; a network
    /// path gives the warning `B2C-T1020`) and the name rules (never `.bat`
    /// or `.cmd`; on Windows only `g++.exe`), must not lie inside any of the
    /// `excluded` folders (the open projects' folders and the cache root:
    /// a project must never bring its own compiler, 08 §8.5), and is then
    /// probed. That check compares canonical paths and comes before the
    /// probe, which runs the program. It is then kept with the source
    /// `manual`, even when it is not usable (its problems say why),
    /// replacing an entry for the same file. The DTO's `selected` is false:
    /// the caller knows the selection.
    ///
    /// # Errors
    /// The `B2C-T1002` refusal, or the `B2C-T1003` probe failure; nothing is
    /// added (or run) then.
    pub fn add_explicit(&self, path: &Path, excluded: &[PathBuf]) -> Result<ToolchainDto, Vec<Diagnostic>> {
        self.add_explicit_for(path, excluded, Platform::host())
    }

    /// [`ToolchainRegistry::add_explicit`] with the platform's rules.
    fn add_explicit_for(
        &self,
        path: &Path,
        excluded: &[PathBuf],
        platform: Platform,
    ) -> Result<ToolchainDto, Vec<Diagnostic>> {
        manual_name_allowed(path, platform).map_err(|refusal| vec![refusal])?;
        let candidate = explicit_candidate(path, platform).map_err(|refusal| vec![refusal])?;
        if let Some(folder) = excluded_folder_of(&candidate.path, excluded) {
            tracing::info!("a compiler picked by hand inside a project folder or the cache was refused");
            tracing::debug!(path = %candidate.path.display(), folder = %folder.display(), "refused compiler");
            return Err(vec![inside_excluded(&candidate.found_as)]);
        }
        let probe = self
            .probe_candidate(&candidate, default_jobs(), None)
            .map_err(|failure| vec![failure])?;
        let entry = Entry::new(ToolchainSource::Manual, candidate.found_as, probe);
        let added = entry_dto(&entry, None);
        let mut state = self.lock();
        insert_manual(&mut state.entries, entry);
        self.save(&state.entries);
        tracing::info!(usable = added.usable, "a compiler was added by hand");
        Ok(added)
    }

    /// The probed toolchain with this ID, if it is in the list
    /// (`toolchain_select` checks IDs with it).
    pub fn get(&self, id: &ToolchainId) -> Option<Toolchain> {
        self.lock()
            .entries
            .iter()
            .find(|entry| entry.id == *id)
            .map(|entry| entry.probe.clone())
    }

    /// Chooses the toolchain for a build in the project folder `project_dir`
    /// (see the module documentation). When nothing usable is known yet, or
    /// the selection is missing, it first waits for a running discovery (at
    /// most [`DISCOVERY_WAIT`]), or runs one when none has run yet, and
    /// tries again.
    pub fn choose(&self, selected: Option<&ToolchainId>, project_dir: Option<&Path>) -> Chosen {
        self.choose_with(selected, project_dir, None, |project| {
            self.await_discovery(project)
        })
    }

    /// [`ToolchainRegistry::choose`] for a build session, on its thread,
    /// with the build's cancellation token (`BuildJob::toolchain`). Once
    /// `cancel` fires it stops waiting for a discovery (it looks every
    /// 50 ms) and stops probing a changed compiler, and returns what it has;
    /// the build then ends as cancelled. When no discovery has run yet, it
    /// starts one on a background thread (as
    /// [`ToolchainRegistry::spawn_discovery`] does) and waits for it, so a
    /// cancelled build never leaves a discovery half done.
    pub fn choose_cancellable(
        self: &Arc<Self>,
        selected: Option<&ToolchainId>,
        project_dir: Option<&Path>,
        cancel: &CancelToken,
    ) -> Chosen {
        self.choose_with(selected, project_dir, Some(cancel), |project| {
            self.await_discovery_cancellable(project, cancel)
        })
    }

    /// [`ToolchainRegistry::choose`] with a way to wait for discoveries and
    /// an optional cancellation token for the probes.
    fn choose_with(
        &self,
        selected: Option<&ToolchainId>,
        project_dir: Option<&Path>,
        cancel: Option<&CancelToken>,
        await_discovery: impl FnOnce(Option<&Path>) -> bool,
    ) -> Chosen {
        let project =
            project_dir.map(|dir| b2c_toolchain::paths::canonical(dir).unwrap_or_else(|_| dir.to_path_buf()));
        let project = project.as_deref();
        let mut attempt = self.attempt(selected, project, cancel);
        if attempt.wants_discovery() && !is_cancelled(cancel) && await_discovery(project) {
            attempt = self.attempt(selected, project, cancel);
        }
        attempt.into_chosen()
    }

    /// What the setup page needs (`toolchain_setup_info`): the platform,
    /// whether the list has no usable toolchain, and on Linux the
    /// distribution from `os-release` ([`b2c_toolchain::host`]).
    pub fn setup_info(&self) -> ToolchainSetupInfo {
        let no_usable_toolchain = !self.lock().entries.iter().any(|entry| entry.probe.is_usable());
        let platform = IpcPlatform::current();
        let distro = match platform {
            IpcPlatform::Linux => b2c_toolchain::host::read_os_release().map(|release| Distro {
                id: release.id,
                id_like: release.id_like,
            }),
            IpcPlatform::Windows => None,
        };
        ToolchainSetupInfo {
            platform,
            no_usable_toolchain,
            distro,
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Counts a scan as running (before it starts, so `list` says so).
    fn begin_scan(&self) {
        self.lock().scanning += 1;
    }

    /// Counts a scan as finished and wakes those waiting for it.
    fn end_scan(&self) {
        {
            let mut state = self.lock();
            state.scanning = state.scanning.saturating_sub(1);
        }
        self.idle.notify_all();
    }

    /// One discovery: finds the candidates, probes what changed and, unless
    /// a newer scan started meanwhile, makes the result the list and saves
    /// it. The caller counts the scan ([`ToolchainRegistry::begin_scan`]).
    fn scan(&self, excluded: &[PathBuf]) {
        let started = Instant::now();
        let (epoch, snapshot) = {
            let mut state = self.lock();
            state.epoch = state.epoch.wrapping_add(1);
            excluded.clone_into(&mut state.last_excluded);
            (state.epoch, state.entries.clone())
        };
        let found = self.discover_and_probe(excluded, &snapshot);
        let probed = found.probed;
        let mut state = self.lock();
        if state.epoch != epoch {
            tracing::debug!("a newer toolchain scan started, so this one's result is dropped");
            return;
        }
        state.entries = merge(found, &state.entries);
        state.scanned = true;
        self.save(&state.entries);
        let usable = state
            .entries
            .iter()
            .filter(|entry| entry.probe.is_usable())
            .count();
        tracing::info!(
            toolchains = state.entries.len(),
            usable,
            probed,
            elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "toolchain discovery finished"
        );
        tracing::debug!(
            paths = ?state.entries.iter().map(|entry| entry.found_as.display().to_string()).collect::<Vec<_>>(),
            "toolchains found"
        );
    }

    /// The discovered candidates and the manual entries of `snapshot`, each
    /// taken from the snapshot when unchanged and probed otherwise, in
    /// parallel.
    fn discover_and_probe(&self, excluded: &[PathBuf], snapshot: &[Entry]) -> ScanResult {
        let manual: HashSet<&Path> = snapshot
            .iter()
            .filter(|entry| entry.is_manual())
            .map(Entry::path)
            .collect();
        let mut jobs: Vec<Job> = self
            .candidates(excluded)
            .into_iter()
            .filter(|candidate| !manual.contains(candidate.path.as_path()))
            .take(MAX_DISCOVERED)
            .map(|candidate| {
                let cached = snapshot
                    .iter()
                    .find(|entry| entry.path() == candidate.path)
                    .cloned();
                Job::Discovered(candidate, cached)
            })
            .collect();
        jobs.extend(
            snapshot
                .iter()
                .filter(|entry| entry.is_manual())
                .cloned()
                .map(Job::Manual),
        );
        let workers = MAX_PARALLEL_PROBES.min(jobs.len()).max(1);
        // Share the cores between the probes expected to run at once (new
        // compilers; known ones have rarely changed), so that one probe alone
        // still runs its checks in parallel.
        let new = jobs
            .iter()
            .filter(|job| matches!(job, Job::Discovered(_, None)))
            .count();
        let per_probe = (default_jobs() / new.clamp(1, workers)).max(1);
        let mut result = ScanResult::default();
        for resolved in parallel(jobs, workers, &|job| self.resolve(job, per_probe))
            .into_iter()
            .flatten()
        {
            match resolved {
                Resolved::Discovered { entry, probed } => {
                    result.probed += usize::from(probed);
                    result.discovered.push(entry);
                }
                Resolved::Manual { key, entry, probed } => {
                    result.probed += usize::from(probed);
                    result.manual.insert(key, entry);
                }
            }
        }
        result
    }

    /// The candidates discovery finds, never in `excluded`, in the cache
    /// root (the folder of the probes' folder) or in the current directory.
    fn candidates(&self, excluded: &[PathBuf]) -> Vec<Candidate> {
        let mut excluded = excluded.to_vec();
        excluded.extend(
            self.probe_temp
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_path_buf),
        );
        match &self.scope {
            // Adds the current directory itself.
            DiscoveryScope::Default => discover(&DiscoveryEnv::from_process(excluded)),
            DiscoveryScope::Only(dirs) => {
                // A fresh, empty root, so that no well-known folder exists.
                let Ok(empty_root) = tempfile::tempdir() else {
                    tracing::warn!("no temporary folder for toolchain discovery; nothing was searched");
                    return Vec::new();
                };
                excluded.extend(std::env::current_dir().ok());
                discover(&DiscoveryEnv {
                    platform: Platform::host(),
                    path: dirs.clone(),
                    root: empty_root.path().to_path_buf(),
                    user_profile: None,
                    program_data: None,
                    local_app_data: None,
                    msys2_roots: Vec::new(),
                    extra_dirs: Vec::new(),
                    excluded,
                })
            }
        }
    }

    /// One scan job: an unchanged toolchain is taken as it is; a changed or
    /// new one is probed. A manual compiler whose file is gone is dropped
    /// (`entry: None`); one that cannot be probed right now is kept.
    fn resolve(&self, job: Job, jobs: usize) -> Resolved {
        match job {
            Job::Discovered(candidate, cached) => {
                let source = source_of(&candidate);
                let unchanged = cached.filter(|entry| entry.probe.is_current());
                let probed = unchanged.is_none();
                let probe =
                    unchanged.map_or_else(|| self.probe_or_unprobed(&candidate, jobs), |entry| entry.probe);
                Resolved::Discovered {
                    entry: Entry::new(source, candidate.found_as, probe),
                    probed,
                }
            }
            Job::Manual(entry) => {
                let key = entry.path().to_path_buf();
                if entry.probe.is_current() {
                    return Resolved::Manual {
                        key,
                        entry: Some(entry),
                        probed: false,
                    };
                }
                let Ok(candidate) = explicit_candidate(entry.path(), Platform::host()) else {
                    return Resolved::Manual {
                        key,
                        entry: None,
                        probed: false,
                    };
                };
                let updated = match self.probe_candidate(&candidate, jobs, None) {
                    Ok(probe) => Entry::new(ToolchainSource::Manual, entry.found_as, probe),
                    Err(_) => entry,
                };
                Resolved::Manual {
                    key,
                    entry: Some(updated),
                    probed: true,
                }
            }
        }
    }

    /// Probes a candidate; its location warnings come first in the result's
    /// problems. `cancel` stops the probe early.
    ///
    /// # Errors
    /// The `B2C-T1003` diagnostic when it cannot be probed at all (also when
    /// it was cancelled).
    fn probe_candidate(
        &self,
        candidate: &Candidate,
        jobs: usize,
        cancel: Option<&CancelToken>,
    ) -> Result<Toolchain, Diagnostic> {
        let options = ProbeOptions {
            cancel: cancel.cloned(),
            ..self.probe_options(jobs)
        };
        match self.prober.probe(&candidate.path, &options) {
            Ok(mut toolchain) => {
                let mut problems = candidate.warnings.clone();
                problems.append(&mut toolchain.problems);
                toolchain.problems = problems;
                Ok(toolchain)
            }
            Err(error) => Err(probe_failed(&candidate.found_as, &error)),
        }
    }

    /// [`ToolchainRegistry::probe_candidate`], or when that fails an
    /// unusable record that says why (and is probed again next time).
    fn probe_or_unprobed(&self, candidate: &Candidate, jobs: usize) -> Toolchain {
        self.probe_candidate(candidate, jobs, None)
            .unwrap_or_else(|failure| unprobed(candidate, failure))
    }

    fn probe_options(&self, jobs: usize) -> ProbeOptions {
        ProbeOptions {
            timeout: PROBE_TIMEOUT,
            temp_root: self.probe_root(),
            host: HostEnv::from_process(&[]),
            jobs,
            cancel: None,
        }
    }

    /// The probes' folder, created owner-only; `None` (the system's
    /// temporary folder) when it cannot be.
    fn probe_root(&self) -> Option<PathBuf> {
        let dir = self.probe_temp.as_ref()?;
        match b2c_store::ensure_private_dir(dir) {
            Ok(()) => Some(dir.clone()),
            Err(error) => {
                tracing::debug!(%error, path = %dir.display(), "the probe folder cannot be used; using the system's temporary folder");
                None
            }
        }
    }

    /// Saves the list, when it has a store.
    fn save(&self, entries: &[Entry]) {
        if let Some(path) = &self.store {
            store::save(path, entries);
        }
    }

    /// `entry` as it is now: unchanged, or probed again because its
    /// fingerprint changed (with the `B2C-T1009` note), which also updates
    /// the list. `cancel` stops that probe early.
    ///
    /// # Errors
    /// The `B2C-T1002` or `B2C-T1003` diagnostic when its file is gone or
    /// it cannot be probed (or the probe was cancelled; the list is then
    /// left as it was).
    fn current(
        &self,
        entry: &Entry,
        cancel: Option<&CancelToken>,
    ) -> Result<(Entry, Vec<Diagnostic>), Diagnostic> {
        if entry.probe.is_current() {
            return Ok((entry.clone(), Vec::new()));
        }
        let candidate = explicit_candidate(entry.path(), Platform::host())?;
        let probe = self.probe_candidate(&candidate, default_jobs(), cancel)?;
        let fresh = Entry::new(entry.source, entry.found_as.clone(), probe);
        // An entry that could not be probed before has no real fingerprint:
        // it did not change, it was only checked for the first time.
        let notes = if entry.probe.fingerprint.sha256.is_empty() {
            Vec::new()
        } else {
            vec![codes::toolchain_changed(&entry.found_as)]
        };
        let mut state = self.lock();
        if let Some(slot) = state.entries.iter_mut().find(|slot| slot.path() == fresh.path()) {
            *slot = fresh.clone();
            self.save(&state.entries);
        }
        Ok((fresh, notes))
    }

    /// One pass of [`ToolchainRegistry::choose`] over the list as it is. Once
    /// `cancel` fires, no further entry is checked.
    fn attempt(
        &self,
        selected: Option<&ToolchainId>,
        project: Option<&Path>,
        cancel: Option<&CancelToken>,
    ) -> Attempt {
        let entries = self.lock().entries.clone();
        let mut attempt = Attempt::default();
        if let Some(id) = selected {
            match entries.iter().find(|entry| entry.id == *id) {
                None => attempt.selection = Some(SelectionProblem::Missing),
                Some(entry) if is_inside(entry.path(), project) => {
                    attempt.selection = Some(SelectionProblem::InsideProject);
                    attempt.rejected.push(codes::inside_project(entry.path()));
                }
                Some(entry) => match self.current(entry, cancel) {
                    Ok((fresh, notes)) if fresh.probe.is_usable() => {
                        attempt.chosen = Some((fresh, notes));
                        return attempt;
                    }
                    Ok((fresh, _)) => {
                        attempt.selection = Some(SelectionProblem::Unusable);
                        attempt.rejected.extend(errors(fresh.probe.problems));
                    }
                    Err(problem) => {
                        attempt.selection = Some(SelectionProblem::Missing);
                        attempt.rejected.push(problem);
                    }
                },
            }
        }
        for entry in entries.iter().filter(|entry| Some(&entry.id) != selected) {
            if is_cancelled(cancel) {
                break;
            }
            if is_inside(entry.path(), project) {
                attempt.rejected.push(codes::inside_project(entry.path()));
                continue;
            }
            match self.current(entry, cancel) {
                Ok((fresh, notes)) if fresh.probe.is_usable() => {
                    attempt.chosen = Some((fresh, notes));
                    return attempt;
                }
                Ok((fresh, _)) => attempt.rejected.extend(errors(fresh.probe.problems)),
                Err(problem) => attempt.rejected.push(problem),
            }
        }
        attempt
    }

    /// Before choosing again: waits for a running scan (at most
    /// [`DISCOVERY_WAIT`]), or runs one when none has finished since the
    /// registry was opened. Returns whether there is anything new to try.
    fn await_discovery(&self, project: Option<&Path>) -> bool {
        let state = self.lock();
        if state.scanning > 0 {
            return self.wait_for_scans(state, None);
        }
        if state.scanned {
            return false;
        }
        let excluded = excluded_for(&state, project);
        drop(state);
        self.begin_scan();
        let _running = ScanGuard {
            registry: self,
            done: None,
        };
        self.scan(&excluded);
        true
    }

    /// [`ToolchainRegistry::await_discovery`] that gives up as soon as
    /// `cancel` fires (returning false), and runs a missing discovery on a
    /// background thread, so the wait for it can be cancelled too.
    fn await_discovery_cancellable(self: &Arc<Self>, project: Option<&Path>, cancel: &CancelToken) -> bool {
        let state = self.lock();
        if state.scanning > 0 {
            return self.wait_for_scans(state, Some(cancel));
        }
        if state.scanned {
            return false;
        }
        let excluded = excluded_for(&state, project);
        drop(state);
        self.spawn_discovery(excluded, Box::new(|| {}));
        self.wait_for_scans(self.lock(), Some(cancel))
    }

    /// Waits until no scan is running, at most [`DISCOVERY_WAIT`]. With a
    /// `cancel` token it also looks at the token every [`CANCEL_POLL`] and
    /// gives up once it fired. Returns whether there may be anything new to
    /// try: true when the scans ended or the wait timed out, false when
    /// cancelled.
    fn wait_for_scans(&self, mut state: MutexGuard<'_, State>, cancel: Option<&CancelToken>) -> bool {
        let started = Instant::now();
        loop {
            if state.scanning == 0 {
                return true;
            }
            if is_cancelled(cancel) {
                return false;
            }
            let Some(remaining) = DISCOVERY_WAIT
                .checked_sub(started.elapsed())
                .filter(|remaining| !remaining.is_zero())
            else {
                drop(state);
                tracing::warn!(
                    "toolchain discovery is taking long; choosing from the toolchains known so far"
                );
                return true;
            };
            let slice = if cancel.is_some() {
                remaining.min(CANCEL_POLL)
            } else {
                remaining
            };
            state = self
                .idle
                .wait_timeout(state, slice)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

/// Whether the optional token has fired.
fn is_cancelled(cancel: Option<&CancelToken>) -> bool {
    cancel.is_some_and(CancelToken::is_cancelled)
}

/// What a scan that `choose` starts excludes: what the last scan excluded,
/// and the project's folder.
fn excluded_for(state: &State, project: Option<&Path>) -> Vec<PathBuf> {
    let mut excluded = state.last_excluded.clone();
    excluded.extend(project.map(Path::to_path_buf));
    excluded
}

/// The DTO of an entry.
fn entry_dto(entry: &Entry, selected: Option<&ToolchainId>) -> ToolchainDto {
    toolchain_dto(&entry.probe, entry.source, &entry.found_as, selected)
}

/// Ends a scan when dropped (also when the scan panicked, or its thread
/// never started), then calls the caller's `done`.
struct ScanGuard<R: Deref<Target = ToolchainRegistry>> {
    registry: R,
    done: Option<Box<dyn FnOnce() + Send>>,
}

impl<R: Deref<Target = ToolchainRegistry>> Drop for ScanGuard<R> {
    fn drop(&mut self) {
        self.registry.end_scan();
        if let Some(done) = self.done.take() {
            done();
        }
    }
}

/// A unit of work of a scan.
enum Job {
    /// A discovered candidate, with the list's entry for the same file.
    Discovered(Candidate, Option<Entry>),
    /// A manual entry, checked again.
    Manual(Entry),
}

/// The result of a [`Job`].
enum Resolved {
    Discovered {
        entry: Entry,
        probed: bool,
    },
    Manual {
        /// The path the entry had.
        key: PathBuf,
        /// `None` when its file is gone.
        entry: Option<Entry>,
        probed: bool,
    },
}

/// What a scan found.
#[derive(Default)]
struct ScanResult {
    /// In discovery order.
    discovered: Vec<Entry>,
    /// The manual entries checked, by their path at the start of the scan.
    manual: HashMap<PathBuf, Option<Entry>>,
    /// How many compilers were probed.
    probed: usize,
}

/// The new list after a scan: what it discovered, then the manual entries of
/// the list as it is now (so one added during the scan is kept), updated by
/// the scan. A manual entry replaces a discovered one for the same file.
fn merge(found: ScanResult, current: &[Entry]) -> Vec<Entry> {
    let mut entries = found.discovered;
    for entry in current.iter().filter(|entry| entry.is_manual()) {
        let entry = match found.manual.get(entry.path()) {
            Some(Some(updated)) => updated.clone(),
            Some(None) => continue,
            None => entry.clone(),
        };
        entries.retain(|other| other.path() != entry.path());
        entries.push(entry);
    }
    entries
}

/// Adds a manual entry, replacing one for the same file and forgetting the
/// oldest manual entry beyond [`MAX_MANUAL`].
fn insert_manual(entries: &mut Vec<Entry>, entry: Entry) {
    entries.retain(|other| other.path() != entry.path());
    let manual = entries.iter().filter(|other| other.is_manual()).count();
    if manual >= MAX_MANUAL
        && let Some(oldest) = entries.iter().position(Entry::is_manual)
    {
        entries.remove(oldest);
    }
    entries.push(entry);
}

/// What a scan or a lookup was at the time: the IPC source of a candidate.
fn source_of(candidate: &Candidate) -> ToolchainSource {
    match candidate.source {
        CandidateSource::Path => ToolchainSource::Path,
        CandidateSource::WellKnown | CandidateSource::Extra => ToolchainSource::WellKnown,
        CandidateSource::Explicit => ToolchainSource::Manual,
    }
}

/// The name rules for a compiler added by hand (07 §7.2): never a batch
/// file, and on Windows only a file named exactly `g++.exe` (in any case).
///
/// # Errors
/// The `B2C-T1002` refusal.
fn manual_name_allowed(path: &Path, platform: Platform) -> Result<(), Diagnostic> {
    let batch = path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("bat") || extension.eq_ignore_ascii_case("cmd")
    });
    let gxx = path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("g++.exe"));
    let why = if batch {
        "batch files (.bat, .cmd) are never run as compilers"
    } else if platform == Platform::Windows && !gxx {
        "choose the program named g++.exe"
    } else {
        return Ok(());
    };
    Err(Diagnostic::error(
        codes::BAD_TOOLCHAIN_PATH,
        DiagSource::Toolchain,
        Location::project(),
        format!("The compiler {} cannot be used: {why}.", path.display()),
    ))
}

/// Whether `path` is inside the project folder (both canonical).
fn is_inside(path: &Path, project: Option<&Path>) -> bool {
    project.is_some_and(|dir| path.starts_with(dir))
}

/// The folder of `excluded` that the canonical `path` lies in, if any. Each
/// folder is compared in its canonical form (as given when it no longer
/// exists), as discovery compares them.
fn excluded_folder_of(path: &Path, excluded: &[PathBuf]) -> Option<PathBuf> {
    excluded
        .iter()
        .map(|dir| b2c_toolchain::paths::canonical(dir).unwrap_or_else(|_| dir.clone()))
        .find(|dir| path.starts_with(dir))
}

/// The `B2C-T1002` refusal of a compiler picked by hand inside a folder
/// where compilers are never run.
fn inside_excluded(found_as: &Path) -> Diagnostic {
    Diagnostic::error(
        codes::BAD_TOOLCHAIN_PATH,
        DiagSource::Toolchain,
        Location::project(),
        format!(
            "The compiler {} cannot be used: it is inside an open project's folder or the build cache, \
             and a project must never bring its own compiler. Choose a g++ installed outside your projects.",
            found_as.display()
        ),
    )
}

/// The error-level problems.
fn errors(problems: Vec<Diagnostic>) -> impl Iterator<Item = Diagnostic> {
    problems
        .into_iter()
        .filter(|problem| problem.severity == Severity::Error)
}

/// How many compiler invocations a probe runs at once by default: the
/// available cores, at most 4 (as [`ProbeOptions::default`]).
fn default_jobs() -> usize {
    std::thread::available_parallelism().map_or(2, |cores| cores.get().min(4))
}

/// The `B2C-T1003` diagnostic for a compiler that could not be probed.
fn probe_failed(found_as: &Path, error: &ProbeError) -> Diagnostic {
    Diagnostic::error(
        codes::NOT_RUNNABLE,
        DiagSource::Toolchain,
        Location::project(),
        format!(
            "The compiler {} could not be checked: {error}.",
            found_as.display()
        ),
    )
}

/// The record of a compiler that could not be probed: unusable, with
/// `failure` as its problem, and with no fingerprint, so it is probed again
/// whenever it is next needed.
fn unprobed(candidate: &Candidate, failure: Diagnostic) -> Toolchain {
    let mut problems = candidate.warnings.clone();
    problems.push(failure);
    Toolchain {
        format: PROBE_FORMAT,
        fingerprint: Fingerprint {
            path: candidate.path.clone(),
            size: 0,
            modified_ns: 0,
            sha256: String::new(),
        },
        kind: CompilerKind::Unknown,
        version: None,
        version_text: String::new(),
        target: Target::parse(""),
        capabilities: Capabilities::default(),
        problems,
    }
}

/// Runs `work` on every item with up to `workers` threads, keeping the
/// order. A result is `None` only if its worker died.
fn parallel<T: Send, R: Send>(
    items: Vec<T>,
    workers: usize,
    work: &(dyn Fn(T) -> R + Sync),
) -> Vec<Option<R>> {
    let count = items.len();
    let queue = Mutex::new(items.into_iter().enumerate().collect::<VecDeque<_>>());
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..count).map(|_| None).collect());
    let worker = || {
        loop {
            let next = queue.lock().unwrap_or_else(PoisonError::into_inner).pop_front();
            let Some((index, item)) = next else {
                break;
            };
            let result = work(item);
            if let Some(slot) = results
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get_mut(index)
            {
                *slot = Some(result);
            }
        }
    };
    std::thread::scope(|scope| {
        let started = (0..workers.min(count))
            .filter(|_| {
                std::thread::Builder::new()
                    .name(String::from("b2c-toolchain-probe"))
                    .spawn_scoped(scope, worker)
                    .is_ok()
            })
            .count();
        // No thread could start: do the work here.
        if started == 0 {
            worker();
        }
    });
    results.into_inner().unwrap_or_else(PoisonError::into_inner)
}

/// One pass of [`ToolchainRegistry::choose`].
#[derive(Default)]
struct Attempt {
    /// Why the selection could not be used, if there was one.
    selection: Option<SelectionProblem>,
    /// The toolchain chosen, with its `B2C-T1009` note if it was probed
    /// again.
    chosen: Option<(Entry, Vec<Diagnostic>)>,
    /// Why the others were refused (errors only).
    rejected: Vec<Diagnostic>,
}

impl Attempt {
    /// Whether a discovery could change the answer: nothing usable was
    /// found, or the selection was not in the list.
    fn wants_discovery(&self) -> bool {
        self.chosen.is_none() || self.selection == Some(SelectionProblem::Missing)
    }

    fn into_chosen(self) -> Chosen {
        let fallback = self.chosen.as_ref().map(|(entry, _)| entry.found_as.as_path());
        let mut diagnostics: Vec<Diagnostic> = self
            .selection
            .map(|problem| codes::selected_unavailable(problem, fallback))
            .into_iter()
            .collect();
        let Some((entry, changed)) = self.chosen else {
            diagnostics.push(codes::no_toolchain());
            diagnostics.extend(self.rejected);
            return Chosen::Unavailable { diagnostics };
        };
        diagnostics.extend(changed);
        // A usable toolchain has no error among its problems.
        diagnostics.extend(entry.probe.problems.iter().cloned());
        Chosen::Ready {
            toolchain: Box::new(entry.probe),
            notes: diagnostics,
        }
    }
}

// The command-line tool's interface.

/// Which compiler to use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolchainChoice {
    /// The first usable g++ found on this computer.
    Auto,
    /// This g++ (an absolute path).
    Path(PathBuf),
}

/// The result of choosing a toolchain.
pub(crate) enum Selected {
    /// A usable toolchain, plus warnings and notes about it.
    Usable(Box<Toolchain>, Vec<Diagnostic>),
    /// No usable toolchain; the diagnostics say why.
    Unusable(Vec<Diagnostic>),
}

impl ToolchainRegistry {
    /// The command-line tool's registry: the list in the machine folder from
    /// the environment ([`b2c_store::Dirs::from_env`]; in memory only when
    /// there is none), probes in `<cache_root>/probe-tmp`, and the whole
    /// search order.
    fn for_cli(cache_root: Option<&Path>) -> Self {
        let store = b2c_store::Dirs::from_env()
            .ok()
            .map(|dirs| dirs.machine.join(STORE_FILE));
        if store.is_none() {
            tracing::debug!("no machine folder, so toolchains are probed without being remembered");
        }
        Self::with_store(
            store,
            cache_root.map(|root| root.join(PROBE_TEMP_DIR)),
            DiscoveryScope::Default,
            Arc::new(RealProber),
        )
    }

    /// `b2c build --toolchain <path>`: that compiler, from the list when it
    /// is there and unchanged, otherwise probed (and remembered as a manual
    /// one when it is new).
    fn select_path(&self, path: &Path) -> Selected {
        let candidate = match explicit_candidate(path, Platform::host()) {
            Ok(candidate) => candidate,
            Err(refusal) => return Selected::Unusable(vec![refusal]),
        };
        let known = self
            .lock()
            .entries
            .iter()
            .find(|entry| entry.path() == candidate.path)
            .cloned();
        let (entry, notes) = match known {
            Some(entry) => match self.current(&entry, None) {
                Ok(current) => current,
                Err(failure) => return Selected::Unusable(vec![failure]),
            },
            None => match self.probe_candidate(&candidate, default_jobs(), None) {
                Ok(probe) => {
                    let entry = Entry::new(ToolchainSource::Manual, candidate.found_as, probe);
                    let mut state = self.lock();
                    insert_manual(&mut state.entries, entry.clone());
                    self.save(&state.entries);
                    (entry, Vec::new())
                }
                Err(failure) => return Selected::Unusable(vec![failure]),
            },
        };
        if entry.probe.is_usable() {
            let mut notes = notes;
            notes.extend(entry.probe.problems.iter().cloned());
            Selected::Usable(Box::new(entry.probe), notes)
        } else {
            Selected::Unusable(entry.probe.problems)
        }
    }

    /// `b2c build` without `--toolchain`: the first usable compiler in
    /// discovery order. Only the candidates up to that one are checked; new
    /// or changed ones are probed and remembered.
    fn select_discovered(&self, excluded: &[PathBuf]) -> Selected {
        let snapshot = self.lock().entries.clone();
        let mut rejected = Vec::new();
        for candidate in self.candidates(excluded).into_iter().take(MAX_DISCOVERED) {
            let known = snapshot
                .iter()
                .find(|entry| entry.path() == candidate.path)
                .filter(|entry| entry.probe.is_current())
                .cloned();
            let entry = known.unwrap_or_else(|| {
                let probe = self.probe_or_unprobed(&candidate, default_jobs());
                let fresh = Entry::new(source_of(&candidate), candidate.found_as.clone(), probe);
                self.remember_discovered(&fresh);
                fresh
            });
            if entry.probe.is_usable() {
                let notes = entry.probe.problems.clone();
                return Selected::Usable(Box::new(entry.probe), notes);
            }
            rejected.extend(errors(entry.probe.problems));
        }
        let mut problems = vec![codes::no_toolchain()];
        problems.extend(rejected);
        Selected::Unusable(problems)
    }

    /// Puts a freshly probed discovered toolchain in the list: in place of
    /// the entry for the same file (a manual one stays manual), or at the
    /// end of the discovered ones while there is room.
    fn remember_discovered(&self, fresh: &Entry) {
        let mut state = self.lock();
        if let Some(slot) = state.entries.iter_mut().find(|slot| slot.path() == fresh.path()) {
            slot.probe = fresh.probe.clone();
        } else if state.entries.iter().filter(|entry| !entry.is_manual()).count() < MAX_DISCOVERED {
            let at = state
                .entries
                .iter()
                .position(Entry::is_manual)
                .unwrap_or(state.entries.len());
            state.entries.insert(at, fresh.clone());
        } else {
            return;
        }
        self.save(&state.entries);
    }
}

/// Chooses the toolchain for a command-line build: `choice`, with probe
/// results kept in the shared `toolchains.json`. The cache root is never
/// searched.
pub(crate) fn select(choice: &ToolchainChoice, cache_root: &Path) -> Selected {
    let registry = ToolchainRegistry::for_cli(Some(cache_root));
    match choice {
        ToolchainChoice::Path(path) => registry.select_path(path),
        ToolchainChoice::Auto => registry.select_discovered(&[cache_root.to_path_buf()]),
    }
}

/// One compiler found on this computer, for `b2c toolchains`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolchainReport {
    /// The path it was found as.
    pub path: PathBuf,
    /// Its GCC version, when known.
    pub version: Option<String>,
    /// Its target triple, when known.
    pub target: Option<String>,
    /// The C++ standards it supports (e.g. `c++20`).
    pub standards: Vec<String>,
    /// Whether builds can use it.
    pub usable: bool,
    /// Problems and notes about it.
    pub problems: Vec<Diagnostic>,
}

/// Finds and checks every compiler on this computer (`b2c toolchains`): a
/// full discovery that never searches `cache_root` or the current
/// directory, probing new and changed compilers. The list, which also holds
/// the compilers added by hand in the app, is saved for the app and the next
/// run.
pub fn list_toolchains(cache_root: Option<&Path>) -> Vec<ToolchainReport> {
    let registry = ToolchainRegistry::for_cli(cache_root);
    let excluded: Vec<PathBuf> = cache_root.map(Path::to_path_buf).into_iter().collect();
    registry.rescan(&excluded, None);
    registry.lock().entries.iter().map(report).collect()
}

/// The report line of an entry.
fn report(entry: &Entry) -> ToolchainReport {
    let toolchain = &entry.probe;
    let standards = &toolchain.capabilities.standards;
    ToolchainReport {
        path: entry.found_as.clone(),
        version: toolchain.version.map(|version| version.to_string()),
        target: Some(toolchain.target.triple.clone()).filter(|triple| !triple.is_empty()),
        standards: [
            &standards.cpp17,
            &standards.cpp20,
            &standards.cpp23,
            &standards.cpp26,
        ]
        .into_iter()
        .flatten()
        .cloned()
        .collect(),
        usable: toolchain.is_usable(),
        problems: toolchain.problems.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use b2c_toolchain::target::GccVersion;

    use super::*;

    /// A usable GCC record for `path` (no file needed).
    fn gcc_at(path: &Path) -> Toolchain {
        let mut toolchain = unprobed(
            &Candidate {
                path: path.to_path_buf(),
                found_as: path.to_path_buf(),
                source: CandidateSource::Path,
                warnings: Vec::new(),
            },
            codes::no_toolchain(),
        );
        toolchain.problems.clear();
        toolchain.kind = CompilerKind::Gcc;
        toolchain.version = GccVersion::parse("14.2.0");
        toolchain
    }

    fn entry(source: ToolchainSource, path: &str) -> Entry {
        Entry::new(source, PathBuf::from(path), gcc_at(Path::new(path)))
    }

    fn paths(entries: &[Entry]) -> Vec<&Path> {
        entries.iter().map(Entry::path).collect()
    }

    /// Answers every probe with a usable GCC, counting the calls.
    #[derive(Default)]
    struct Counting(AtomicUsize);

    impl Prober for Counting {
        fn probe(&self, path: &Path, _options: &ProbeOptions) -> Result<Toolchain, ProbeError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            let mut toolchain = gcc_at(path);
            toolchain.fingerprint = Fingerprint::compute(path).map_err(|source| ProbeError::Unreadable {
                path: path.to_path_buf(),
                source,
            })?;
            Ok(toolchain)
        }
    }

    #[test]
    fn manual_names_follow_the_platform_rules() {
        let allowed = |path: &str, platform| manual_name_allowed(Path::new(path), platform);
        for platform in [Platform::Linux, Platform::Windows] {
            for script in ["/x/g++.bat", "/x/G++.CMD", "/x/g++.Bat"] {
                let refusal = allowed(script, platform).unwrap_err();
                assert_eq!(refusal.code.0, codes::BAD_TOOLCHAIN_PATH);
                assert!(refusal.message.contains("batch files"), "{}", refusal.message);
            }
        }
        assert!(allowed("/usr/bin/g++", Platform::Linux).is_ok());
        assert!(allowed("/opt/x86_64-linux-gnu-g++-14", Platform::Linux).is_ok());
        assert!(allowed("/x/g++.exe", Platform::Windows).is_ok());
        assert!(allowed("/x/G++.EXE", Platform::Windows).is_ok());
        for other in [
            "/x/x86_64-w64-mingw32-g++.exe",
            "/x/gcc.exe",
            "/x/g++",
            "/x/g++.exe.lnk",
        ] {
            let refusal = allowed(other, Platform::Windows).unwrap_err();
            assert!(refusal.message.contains("g++.exe"), "{}", refusal.message);
        }
    }

    #[test]
    fn windows_rules_apply_to_compilers_added_by_hand() {
        let dir = tempfile::tempdir().unwrap();
        let root = b2c_toolchain::paths::canonical(dir.path()).unwrap();
        for name in ["g++.exe", "x86_64-w64-mingw32-g++.exe", "g++.bat"] {
            std::fs::write(root.join(name), b"MZ").unwrap();
        }
        let prober = Arc::new(Counting::default());
        let registry = ToolchainRegistry::with_store(
            None,
            None,
            DiscoveryScope::Only(Vec::new()),
            Arc::clone(&prober) as Arc<dyn Prober>,
        );
        for refused in ["x86_64-w64-mingw32-g++.exe", "g++.bat"] {
            let problems = registry
                .add_explicit_for(&root.join(refused), &[], Platform::Windows)
                .unwrap_err();
            assert_eq!(problems.len(), 1);
            assert_eq!(problems[0].code.0, codes::BAD_TOOLCHAIN_PATH, "{refused}");
        }
        // Inside an excluded folder (an open project's): refused, never run.
        let problems = registry
            .add_explicit_for(
                &root.join("g++.exe"),
                &[PathBuf::from("/elsewhere"), root.clone()],
                Platform::Windows,
            )
            .unwrap_err();
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code.0, codes::BAD_TOOLCHAIN_PATH);
        assert!(
            problems[0].message.contains("inside an open project's folder"),
            "{}",
            problems[0].message
        );
        assert_eq!(prober.0.load(Ordering::SeqCst), 0);
        let added = registry
            .add_explicit_for(&root.join("g++.exe"), &[root.join("other")], Platform::Windows)
            .unwrap();
        assert_eq!(added.source, ToolchainSource::Manual);
        assert_eq!(prober.0.load(Ordering::SeqCst), 1);
        assert_eq!(registry.list(None).toolchains, [added]);
    }

    #[test]
    fn a_merge_keeps_manual_entries_added_during_the_scan() {
        let discovered = vec![entry(ToolchainSource::Path, "/usr/bin/g++")];
        let mut manual = HashMap::new();
        // Updated by the scan.
        let mut updated = entry(ToolchainSource::Manual, "/opt/a/g++");
        updated.probe.version = GccVersion::parse("15.1.0");
        manual.insert(PathBuf::from("/opt/a/g++"), Some(updated.clone()));
        // Gone.
        manual.insert(PathBuf::from("/opt/b/g++"), None);
        let current = vec![
            entry(ToolchainSource::Path, "/usr/local/bin/g++"),
            entry(ToolchainSource::Manual, "/opt/a/g++"),
            entry(ToolchainSource::Manual, "/opt/b/g++"),
            // Added while the scan ran.
            entry(ToolchainSource::Manual, "/opt/c/g++"),
            // Added while the scan ran, for a file the scan also found.
            entry(ToolchainSource::Manual, "/usr/bin/g++"),
        ];
        let merged = merge(
            ScanResult {
                discovered,
                manual,
                probed: 0,
            },
            &current,
        );
        assert_eq!(
            paths(&merged),
            [
                Path::new("/opt/a/g++"),
                Path::new("/opt/c/g++"),
                Path::new("/usr/bin/g++")
            ]
        );
        assert_eq!(merged[0], updated);
        assert!(merged.iter().all(Entry::is_manual));
    }

    #[test]
    fn manual_entries_replace_their_file_and_are_bounded() {
        let mut entries = vec![entry(ToolchainSource::Path, "/usr/bin/g++")];
        insert_manual(&mut entries, entry(ToolchainSource::Manual, "/usr/bin/g++"));
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_manual());
        for n in 0..MAX_MANUAL {
            insert_manual(
                &mut entries,
                entry(ToolchainSource::Manual, &format!("/m/{n}/g++")),
            );
        }
        assert_eq!(entries.len(), MAX_MANUAL);
        // The oldest manual one was forgotten.
        assert_eq!(entries[0].path(), Path::new("/m/0/g++"));
        assert_eq!(entries.last().unwrap().path(), Path::new("/m/63/g++"));
    }

    #[test]
    fn parallel_work_keeps_its_order() {
        let squares = parallel((0..20_u64).collect(), 4, &|n| {
            std::thread::sleep(Duration::from_millis(20 - n));
            n * n
        });
        let expected: Vec<Option<u64>> = (0..20_u64).map(|n| Some(n * n)).collect();
        assert_eq!(squares, expected);
        assert!(parallel(Vec::<u8>::new(), 4, &|n| n).is_empty());
        assert_eq!(parallel(vec![1, 2], 0, &|n| n + 1), [Some(2), Some(3)]);
    }

    #[test]
    fn sources_map_to_the_ipc_values() {
        let source = |source| {
            source_of(&Candidate {
                path: PathBuf::from("/g++"),
                found_as: PathBuf::from("/g++"),
                source,
                warnings: Vec::new(),
            })
        };
        assert_eq!(source(CandidateSource::Path), ToolchainSource::Path);
        assert_eq!(source(CandidateSource::WellKnown), ToolchainSource::WellKnown);
        assert_eq!(source(CandidateSource::Extra), ToolchainSource::WellKnown);
        assert_eq!(source(CandidateSource::Explicit), ToolchainSource::Manual);
    }

    #[test]
    fn a_scan_guard_ends_the_scan_and_calls_back() {
        let registry =
            ToolchainRegistry::with_store(None, None, DiscoveryScope::default(), Arc::new(RealProber));
        registry.begin_scan();
        assert!(registry.list(None).discovering);
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        drop(ScanGuard {
            registry: &registry,
            done: Some(Box::new(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            })),
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!registry.list(None).discovering);
        // Never below zero.
        registry.end_scan();
        assert!(!registry.list(None).discovering);
        assert!(format!("{registry:?}").contains("ToolchainRegistry"));
    }

    #[test]
    fn the_registry_can_be_shared_between_threads() {
        fn shared<T: Send + Sync + 'static>() {}
        shared::<ToolchainRegistry>();
        shared::<Chosen>();
        shared::<DiscoveryScope>();
    }

    #[test]
    fn inside_means_below_the_folder() {
        let project = Some(Path::new("/home/ada/game"));
        assert!(is_inside(Path::new("/home/ada/game/bin/g++"), project));
        assert!(is_inside(Path::new("/home/ada/game"), project));
        assert!(!is_inside(Path::new("/home/ada/game2/g++"), project));
        assert!(!is_inside(Path::new("/usr/bin/g++"), project));
        assert!(!is_inside(Path::new("/home/ada/game/g++"), None));
    }

    #[test]
    fn excluded_folders_contain_their_whole_tree_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = b2c_toolchain::paths::canonical(dir.path()).unwrap();
        let game = root.join("game");
        std::fs::create_dir_all(game.join("tools")).unwrap();
        let gxx = game.join("tools").join("g++");
        // Compared in canonical form, also when given through a parent link.
        let roundabout = game.join("tools").join(std::path::Component::ParentDir);
        assert_eq!(excluded_folder_of(&gxx, &[roundabout]), Some(game.clone()));
        assert_eq!(
            excluded_folder_of(&gxx, std::slice::from_ref(&game)),
            Some(game.clone())
        );
        // A folder that does not exist is compared as given.
        assert_eq!(
            excluded_folder_of(Path::new("/gone/game/g++"), &[PathBuf::from("/gone/game")]),
            Some(PathBuf::from("/gone/game"))
        );
        // A sibling whose name starts the same is not inside.
        assert_eq!(
            excluded_folder_of(&root.join("game2").join("g++"), std::slice::from_ref(&game)),
            None
        );
        assert_eq!(excluded_folder_of(&gxx, &[]), None);
    }

    #[test]
    fn unavailable_choices_list_the_reasons_in_order() {
        let attempt = Attempt {
            selection: Some(SelectionProblem::Missing),
            chosen: None,
            rejected: vec![codes::inside_project(Path::new("/p/g++"))],
        };
        let Chosen::Unavailable { diagnostics } = attempt.into_chosen() else {
            panic!("expected no toolchain");
        };
        let codes: Vec<&str> = diagnostics.iter().map(|d| d.code.0.as_str()).collect();
        assert_eq!(
            codes,
            [
                codes::SELECTED_UNAVAILABLE,
                codes::NO_TOOLCHAIN,
                codes::BAD_TOOLCHAIN_PATH
            ]
        );
    }

    /// Set in the child process of [`the_command_line_shares_the_machine_list`].
    #[cfg(unix)]
    const CLI_CHILD: &str = "B2C_TEST_TOOLCHAINS_CLI";

    /// Not a check of its own: in the child process started by
    /// [`the_command_line_shares_the_machine_list`] it runs the CLI's
    /// entry points, whose list is in the machine folder of the child's
    /// environment. In an ordinary test run it does nothing.
    #[cfg(unix)]
    #[test]
    fn cli_child() {
        let Some(root) = std::env::var_os(CLI_CHILD).map(PathBuf::from) else {
            return;
        };
        let fake = root.join("bin").join("g++");
        let cache = root.join("cache");
        let outcome = |selected: Selected| match selected {
            Selected::Usable(..) => String::from("usable"),
            Selected::Unusable(problems) => problems
                .iter()
                .map(|problem| problem.code.0.clone())
                .collect::<Vec<_>>()
                .join(","),
        };
        println!(
            "first: {}",
            outcome(select(&ToolchainChoice::Path(fake.clone()), &cache))
        );
        println!(
            "second: {}",
            outcome(select(&ToolchainChoice::Path(fake.clone()), &cache))
        );
        let listed = list_toolchains(Some(&cache));
        println!(
            "listed: {}",
            listed.iter().any(|report| report.path == fake && !report.usable)
        );
    }

    /// The CLI's `select` and `list_toolchains` keep their list in the
    /// machine folder (`$XDG_CONFIG_HOME/blocks2cpp/toolchains.json`), probe
    /// in `<cache>/probe-tmp`, and do not probe an unchanged compiler again.
    /// Runs in a child process with its own environment, so nothing of the
    /// real user's is touched.
    #[cfg(unix)]
    #[test]
    fn the_command_line_shares_the_machine_list() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let root = b2c_toolchain::paths::canonical(dir.path()).unwrap();
        let marker = root.join("runs");
        let fake = root.join("bin").join("g++");
        std::fs::create_dir_all(fake.parent().unwrap()).unwrap();
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\necho run >> '{}'\necho 'not a compiler'\n",
                marker.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

        let exe = std::env::current_exe().unwrap();
        let mut command = b2c_process::Command::new(exe, &root).unwrap();
        command
            .args(["--exact", "toolchains::tests::cli_child", "--nocapture"])
            .env(CLI_CHILD, &root)
            .env("HOME", root.join("home"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("PATH", "/usr/bin:/bin")
            .timeout(Duration::from_mins(2));
        if let Some(value) = std::env::var_os("LLVM_PROFILE_FILE") {
            command.env("LLVM_PROFILE_FILE", value);
        }
        let captured = b2c_process::run_captured(&command).unwrap();
        let stdout = String::from_utf8_lossy(&captured.stdout);
        assert!(
            captured.status.success(),
            "{stdout}{}",
            String::from_utf8_lossy(&captured.stderr)
        );
        let reported = |key: &str| {
            stdout
                .lines()
                .find_map(|line| {
                    line.rsplit_once(&format!("{key}: "))
                        .map(|(_, value)| value.trim().to_owned())
                })
                .unwrap_or_else(|| panic!("no {key}:\n{stdout}"))
        };
        // "not a compiler" is no GCC.
        assert_eq!(reported("first"), codes::UNKNOWN_COMPILER);
        assert_eq!(reported("second"), codes::UNKNOWN_COMPILER);
        assert_eq!(reported("listed"), "true");
        // Probed once: the second choice and the listing used the list.
        let runs = std::fs::read_to_string(&marker).unwrap();
        let first_probe = runs.lines().count();
        assert!(first_probe >= 1);
        assert!(root.join("cache").join(PROBE_TEMP_DIR).is_dir());
        let store = root.join("config").join("blocks2cpp").join(STORE_FILE);
        let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&store).unwrap()).unwrap();
        let manual = json["toolchains"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["foundAs"] == fake.display().to_string())
            .unwrap();
        assert_eq!(manual["source"], "manual");
        // Five questions in the first round, then nothing more.
        assert_eq!(first_probe, 5, "{runs}");
    }
}
