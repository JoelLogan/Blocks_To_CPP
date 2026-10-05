//! The [`Backend`]: its state, its start, the app-level commands (`app_info`,
//! `app_subscribe`, `app_quit`, `open_help_link`), closing the window and
//! shutting down (`docs/spec/02-architecture.md` §2.5–§2.6).
//!
//! The other commands are implemented in their own modules (`projects`,
//! `trust`, `settings`, `recent`, `toolchains`, `build_run`, `recovery`), each
//! as methods of [`Backend`].

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::SystemTime;

use b2c_build::cache::{self, EvictionPolicy};
use b2c_build::toolchains::{
    DiscoveryScope, PROBE_TEMP_DIR, Prober, RealProber, STORE_FILE, ToolchainRegistry,
};
use b2c_build::{BuildSessions, RunSessions};
use b2c_ipc::dto::{AppEvent, AppInfo, Empty, OpenHelpLinkRequest, Platform};
use b2c_ipc::{EventSink, IPC_VERSION, IoKind, IpcError};
use b2c_store::trust::TRUST_FILE;
use b2c_store::{Dirs, RecentStore, SettingsStore, TrustStore};

use crate::dialogs::{DialogGate, Dialogs};
use crate::errors::StartError;
use crate::events::AppEvents;
use crate::limits::SHUTDOWN_BUILD_WAIT;
use crate::projects::ProjectTable;
use crate::settings::notices_to_dto;
use crate::watcher::Watchers;

/// What the backend needs to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendConfig {
    /// The app's folders (`docs/spec/02-architecture.md` §2.7): normally
    /// [`Dirs::from_env`], or [`Dirs::under_root`] in end-to-end tests. Every
    /// folder must be an absolute path.
    pub dirs: Dirs,
    /// The app's version (`app_info`, and `generator.app` of new projects).
    pub app_version: &'static str,
    /// Where toolchain discovery looks: the whole search order, or only some
    /// folders (end-to-end tests).
    pub discovery_scope: DiscoveryScope,
    /// The process's current directory, which discovery never searches
    /// (`docs/spec/07-toolchain-build-run.md` §7.2); `None` reads it from the
    /// process.
    pub cwd: Option<PathBuf>,
}

/// Opens `https` URLs in the user's browser (`open_help_link`). The real one
/// is [`SystemOpener`]; tests substitute their own so no browser starts.
pub trait UrlOpener: Send + Sync {
    /// Opens `url`, one of the fixed help links.
    ///
    /// # Errors
    /// An I/O error when no browser could be started.
    fn open(&self, url: &'static str) -> io::Result<()>;
}

/// The real [`UrlOpener`]: `b2c_build::os::open_https_url` (`xdg-open` by its
/// absolute path with the URL as its only argument on Linux, `ShellExecuteW`
/// on Windows).
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemOpener;

impl UrlOpener for SystemOpener {
    fn open(&self, url: &'static str) -> io::Result<()> {
        b2c_build::os::open_https_url(url).map_err(|error| io::Error::other(error.to_string()))
    }
}

/// The parts of the backend that touch the outside world and that tests
/// replace: the compiler prober and the browser opener.
#[derive(Clone)]
pub struct Services {
    /// Probes compilers for the toolchain list ([`RealProber`] runs g++).
    pub prober: Arc<dyn Prober>,
    /// Opens help links ([`SystemOpener`]).
    pub opener: Arc<dyn UrlOpener>,
}

impl Default for Services {
    fn default() -> Self {
        Self {
            prober: Arc::new(RealProber),
            opener: Arc::new(SystemOpener),
        }
    }
}

impl std::fmt::Debug for Services {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Services").finish_non_exhaustive()
    }
}

/// The Tauri-free backend: one blocking method per IPC command, typed with
/// the `b2c_ipc` DTOs (see the crate documentation for the list).
///
/// It is shared between the command threads (`Arc<Backend>`); every method
/// takes `&self` and is safe to call concurrently. Each part of the state has
/// its own lock, and no lock is held while a dialog is open or a process
/// runs.
pub struct Backend {
    pub(crate) dirs: Dirs,
    pub(crate) app_version: &'static str,
    /// The current directory, excluded from discovery.
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) dialogs: Arc<dyn Dialogs>,
    pub(crate) opener: Arc<dyn UrlOpener>,
    pub(crate) dialog_gate: DialogGate,
    pub(crate) settings: SettingsStore,
    /// The notices from loading the settings (`settings_get`).
    pub(crate) settings_notices: Vec<b2c_ipc::dto::SettingsNotice>,
    pub(crate) trust: TrustStore,
    pub(crate) recent: RecentStore,
    pub(crate) toolchains: Arc<ToolchainRegistry>,
    pub(crate) builds: BuildSessions,
    pub(crate) runs: RunSessions,
    pub(crate) projects: ProjectTable,
    pub(crate) events: Arc<AppEvents>,
    pub(crate) watchers: Watchers,
    pub(crate) shut_down: AtomicBool,
}

impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Backend")
            .field("app_version", &self.app_version)
            .field("projects", &self.projects.len())
            .field("shut_down", &self.shut_down.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

/// A debug span for one command; no arguments, no content.
pub(crate) fn command_span(name: &'static str) -> tracing::span::EnteredSpan {
    tracing::debug_span!("command", name).entered()
}

// Commands take their request by value, as the adapter decodes it, so every
// command method has the same shape whether or not it keeps the request.
#[allow(clippy::needless_pass_by_value)]
impl Backend {
    /// Starts the backend with the real [`Services`]; see
    /// [`Backend::start_with`].
    ///
    /// # Errors
    /// As [`Backend::start_with`].
    pub fn start(config: BackendConfig, dialogs: Arc<dyn Dialogs>) -> Result<Arc<Self>, StartError> {
        Self::start_with(config, dialogs, Services::default())
    }

    /// Starts the backend: loads the settings, the trust store, the recent
    /// list and the cached toolchain list (every read size-bounded; nothing is
    /// probed, so the first `toolchain_list` answers at once), then starts the
    /// background work (`docs/spec/01-overview.md` N2):
    ///
    /// * toolchain discovery, which sends `toolchainsUpdated` when it is done;
    /// * startup maintenance: containment detection, killing stale cgroup
    ///   scopes of crashed instances, and the 30-day prune and size-cap
    ///   eviction of the build cache.
    ///
    /// # Errors
    /// [`StartError::RelativeFolder`] when a folder of `config.dirs` (or
    /// `config.cwd`) is not absolute.
    pub fn start_with(
        config: BackendConfig,
        dialogs: Arc<dyn Dialogs>,
        services: Services,
    ) -> Result<Arc<Self>, StartError> {
        let BackendConfig {
            dirs,
            app_version,
            discovery_scope,
            cwd,
        } = config;
        for (name, path) in [
            ("config", &dirs.config),
            ("machine", &dirs.machine),
            ("cache", &dirs.cache),
            ("recovery", &dirs.recovery),
            ("logs", &dirs.logs),
        ] {
            if !path.is_absolute() {
                return Err(StartError::RelativeFolder(name));
            }
        }
        if cwd.as_deref().is_some_and(|cwd| !cwd.is_absolute()) {
            return Err(StartError::RelativeFolder("current"));
        }
        let cwd = cwd.or_else(|| std::env::current_dir().ok());

        let (settings, notices) = SettingsStore::open(&dirs.config);
        let settings_notices = notices_to_dto(&notices);
        for notice in &settings_notices {
            tracing::warn!(key = %notice.key, reason = notice.reason.as_str(), "a setting was reset or read partially");
        }
        let trust = TrustStore::open(&dirs.machine.join(TRUST_FILE));
        if let Some(problem) = trust.problem() {
            tracing::warn!(%problem, "the trust store is not usable");
        }
        let recent = RecentStore::open(&dirs.config);
        let toolchains = Arc::new(ToolchainRegistry::open(
            dirs.machine.join(STORE_FILE),
            dirs.cache.join(PROBE_TEMP_DIR),
            discovery_scope,
            services.prober,
        ));
        let builds = BuildSessions::new(dirs.cache.clone());
        let events = Arc::new(AppEvents::new(settings_notices.clone()));
        let backend = Arc::new(Self {
            watchers: Watchers::new(Arc::clone(&events)),
            dirs,
            app_version,
            cwd,
            dialogs,
            opener: services.opener,
            dialog_gate: DialogGate::default(),
            settings,
            settings_notices,
            trust,
            recent,
            toolchains,
            builds,
            runs: RunSessions::new(),
            projects: ProjectTable::default(),
            events,
            shut_down: AtomicBool::new(false),
        });
        backend.install_build_hook();
        backend.start_discovery();
        backend.start_maintenance();
        tracing::info!(app_version, "backend started");
        Ok(backend)
    }

    /// `app_info`: versions and platform (`docs/spec/02-architecture.md`
    /// §2.5.7).
    pub fn app_info(&self) -> AppInfo {
        let _span = command_span("app_info");
        AppInfo {
            app_version: self.app_version.to_owned(),
            ipc_version: IPC_VERSION,
            platform: Platform::current(),
            catalog_version: b2c_build::CATALOG_VERSION.to_owned(),
        }
    }

    /// `app_subscribe`: makes `sink` the channel of app events, replacing an
    /// earlier one. The first subscriber also gets the settings notices from
    /// startup.
    pub fn app_subscribe(&self, sink: Arc<dyn EventSink<AppEvent>>) {
        let _span = command_span("app_subscribe");
        self.events.subscribe(sink);
    }

    /// `app_quit`: shuts down ([`Backend::shutdown`]); the adapter then exits
    /// the process.
    ///
    /// # Errors
    /// None today; the `Result` keeps the command shape.
    pub fn app_quit(&self) -> Result<Empty, IpcError> {
        let _span = command_span("app_quit");
        self.shutdown();
        Ok(Empty {})
    }

    /// `open_help_link`: opens one of the fixed `https` help pages in the
    /// browser (`docs/spec/08-security.md` §8.8). The webview names the page;
    /// it never passes a URL.
    ///
    /// # Errors
    /// [`IpcError::Io`] when no browser could be started.
    pub fn open_help_link(&self, request: OpenHelpLinkRequest) -> Result<Empty, IpcError> {
        let _span = command_span("open_help_link");
        let url = request.link_id.url();
        if !url.starts_with("https://") {
            tracing::error!(link = request.link_id.as_str(), "a help link is not https");
            return Err(IpcError::Internal);
        }
        self.opener.open(url).map_err(|error| {
            tracing::warn!(link = request.link_id.as_str(), %error, "cannot open a help link");
            IpcError::Io { kind: IoKind::Other }
        })?;
        Ok(Empty {})
    }

    /// Whether any open project has unsaved changes (as reported by
    /// `project_set_dirty`).
    pub fn has_dirty(&self) -> bool {
        self.projects.any_dirty()
    }

    /// The window asks to close (`docs/spec/02-architecture.md` §2.6). Returns
    /// `true` when it may close: no project has unsaved changes, or nobody
    /// listens on the app channel to ask the user. Otherwise it sends
    /// `closeRequested` and returns `false`; the frontend asks Save / Don't
    /// save / Cancel and then calls `app_quit`.
    pub fn request_close(&self) -> bool {
        let _span = command_span("request_close");
        if !self.has_dirty() {
            return true;
        }
        let asked = self.events.send(AppEvent::CloseRequested);
        if !asked {
            tracing::warn!("unsaved changes, but no frontend to ask: closing");
        }
        !asked
    }

    /// Shuts down (`app_quit`, and the app's exit): cancels every build,
    /// stops every program and kills its process tree, stops the file
    /// watchers and deletes the recovery snapshots of projects without
    /// unsaved changes. Idempotent: only the first call does anything. Later
    /// `build_start` and `run_start` calls are refused.
    pub fn shutdown(&self) {
        if self.shut_down.swap(true, Ordering::AcqRel) {
            return;
        }
        let _span = command_span("shutdown");
        self.builds.cancel_all();
        self.runs.stop_all();
        if !self.builds.wait_idle(SHUTDOWN_BUILD_WAIT) {
            tracing::warn!("builds were still running after shutdown waited for them");
        }
        self.watchers.stop_all();
        for handle in self.projects.clean_handles() {
            self.discard_snapshot_of(&handle);
        }
        tracing::info!("backend shut down");
    }

    /// Whether [`Backend::shutdown`] has run.
    pub(crate) fn is_shut_down(&self) -> bool {
        self.shut_down.load(Ordering::Acquire)
    }

    /// The build cache root (`Dirs::cache`).
    pub(crate) fn cache_root(&self) -> &Path {
        &self.dirs.cache
    }

    /// Runs cache eviction after each finished build (on the build's thread,
    /// after its `finished` event was sent), keeping the build's own folder:
    /// the program that was just built is about to be run.
    fn install_build_hook(self: &Arc<Self>) {
        let weak: Weak<Self> = Arc::downgrade(self);
        self.builds.set_on_finished(Box::new(move |record| {
            if let Some(backend) = weak.upgrade() {
                backend.evict_cache(record.build_dir.as_deref());
            }
        }));
    }

    /// Prunes and evicts the build cache with the current size cap, never
    /// deleting the entry `keep`.
    pub(crate) fn evict_cache(&self, keep: Option<&Path>) {
        let policy = EvictionPolicy::with_max_bytes(self.settings.get().build_cache.max_bytes);
        match cache::prune_and_evict_keeping(self.cache_root(), &policy, SystemTime::now(), keep) {
            Ok(report) => tracing::debug!(
                removed = report.removed,
                freed_bytes = report.freed_bytes,
                skipped = report.skipped_locked,
                "build cache evicted"
            ),
            Err(error) => tracing::warn!(%error, "build cache eviction failed"),
        }
    }

    /// Starts toolchain discovery in the background; `toolchainsUpdated`
    /// follows when it is done.
    fn start_discovery(self: &Arc<Self>) {
        let weak: Weak<Self> = Arc::downgrade(self);
        let excluded = self.excluded_folders();
        self.toolchains.spawn_discovery(
            excluded,
            Box::new(move || {
                if let Some(backend) = weak.upgrade() {
                    backend.send_toolchains_updated();
                }
            }),
        );
    }

    /// Startup maintenance on a background thread: containment detection
    /// (which can take seconds), stale cgroup scopes of crashed instances, and
    /// the 30-day prune and size-cap eviction of the build cache
    /// (`docs/spec/07-toolchain-build-run.md` §7.5.1, `docs/spec/08-security.md`
    /// §8.14).
    fn start_maintenance(self: &Arc<Self>) {
        let cache_root = self.cache_root().to_path_buf();
        let policy = EvictionPolicy::with_max_bytes(self.settings.get().build_cache.max_bytes);
        let spawned = std::thread::Builder::new()
            .name(String::from("b2c-startup-maintenance"))
            .spawn(move || {
                let level = b2c_build::os::containment_level();
                tracing::info!(level = ?level, "program containment");
                let killed = b2c_build::os::cleanup_stale_scopes();
                if killed > 0 {
                    tracing::info!(killed, "stopped programs left behind by a crashed instance");
                }
                match cache::prune_and_evict(&cache_root, &policy, SystemTime::now()) {
                    Ok(report) => tracing::info!(
                        removed = report.removed,
                        freed_bytes = report.freed_bytes,
                        "build cache pruned"
                    ),
                    Err(error) => tracing::warn!(%error, "build cache pruning failed"),
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "startup maintenance could not start");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backend_can_be_shared_between_threads() {
        fn shared<T: Send + Sync>() {}
        shared::<Backend>();
        shared::<Services>();
        shared::<BackendConfig>();
    }

    #[test]
    fn spans_carry_only_the_command_name() {
        let span = command_span("project_save");
        drop(span);
    }
}
