//! The Blocks2Cpp desktop app: a thin Tauri 2 adapter around the backend
//! services of `b2c-app` (`docs/spec/02-architecture.md` §2.2–§2.6,
//! [ADR-0007](../../../docs/adr/0007-backend-crates-and-ipc-contract.md)).
//!
//! It holds no business logic. Every IPC command decodes its `request` with
//! [`b2c_ipc::decode()`] and calls one blocking method of [`b2c_app::Backend`]
//! on Tauri's blocking thread pool; a panic there becomes `internal` instead
//! of crossing the IPC boundary (`docs/spec/09-quality-and-delivery.md` §9.1).
//! What this crate adds is only what needs Tauri:
//!
//! * `commands`: the 36 command adapters, in step with
//!   [`b2c_ipc::COMMAND_NAMES`], `build.rs`, the capability and the isolation
//!   allowlist (a test checks all five).
//! * `channels`: Tauri channels as the backend's event and byte sinks (push
//!   messages never use Tauri's global events, §2.5.3).
//! * `dialogs`: the native open, save, compiler and trust dialogs, raised from
//!   Rust only (`docs/spec/08-security.md` §8.3, §8.8).
//! * `window`: the main window, its navigation rules, and closing it with
//!   unsaved changes (§2.6).
//! * [`logging`]: JSON-lines log files with rotation and the panic hook
//!   (`docs/spec/08-security.md` §8.11).
//! * `e2e` (feature `e2e-hooks`, never in a release build): the end-to-end
//!   test seams of [ADR-0009](../../../docs/adr/0009-e2e-tooling-and-test-seams.md).
//!
//! Hardening (`docs/spec/08-security.md` §8.8): the Content Security Policy,
//! the isolation pattern and the capability are set in `tauri.conf.json`,
//! `isolation/` and `capabilities/`. The window may navigate only within the
//! app and cannot open new windows; the native dialog plugin is registered
//! from Rust and the webview has no permission to call it.

mod channels;
mod commands;
mod dialogs;
#[cfg(feature = "e2e-hooks")]
pub mod e2e;
pub mod logging;
mod window;

use std::fmt;
use std::io::Write as _;
use std::process::ExitCode;
use std::sync::Arc;

use b2c_app::{Backend, BackendConfig, Dialogs};
use b2c_build::toolchains::DiscoveryScope;
use b2c_store::{Dirs, StoreError};
use tauri::{App, AppHandle, Manager as _, RunEvent, Runtime};

/// The app's version: `app_info`, the `generator.app` of saved projects, and
/// the bundle (`tauri.conf.json` reads the same version from
/// `apps/desktop/package.json`).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Starts the app and runs it until it exits: finds the app's folders, starts
/// the log file and the panic hook, builds the Tauri app (commands, dialog
/// plugin, window events), starts the [`Backend`] and opens the editor window.
/// When the app exits, every build and program is stopped
/// ([`Backend::shutdown`]).
///
/// Returns the process's exit code: failure when the app could not start.
pub fn run() -> ExitCode {
    let launch = match Launch::from_env() {
        Ok(launch) => launch,
        Err(error) => return failed_to_start(&error),
    };
    let level = logging::level_from_env();
    if let Err(error) = logging::init(&launch.dirs.logs, level) {
        // The app works without its log file; say so where a developer sees it.
        let _ = writeln!(
            std::io::stderr().lock(),
            "Blocks2Cpp is running without its log file: {error}"
        );
    }
    logging::install_panic_hook();
    tracing::info!(app_version = APP_VERSION, "starting");
    #[cfg(feature = "e2e-hooks")]
    tracing::warn!("this is an end-to-end test build: its folders, dialogs and toolchains can be scripted");
    match start(launch) {
        Ok(code) => code,
        Err(error) => failed_to_start(&error),
    }
}

/// Adds everything the app's IPC needs to `builder`: the native dialog plugin
/// (from Rust only; the capability grants the webview none of its commands),
/// the 36 commands and the window-close handling. The app must manage an
/// `Arc<Backend>` before its window is created.
///
/// Tests build the same app on Tauri's mock runtime with this function.
pub fn configure<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::app::app_info,
            commands::app::app_subscribe,
            commands::app::app_quit,
            commands::project::project_new,
            commands::project::project_open_dialog,
            commands::project::project_open_recent,
            commands::project::project_reload,
            commands::project::project_save,
            commands::project::project_save_as_dialog,
            commands::project::project_close,
            commands::project::project_set_dirty,
            commands::recent::recent_list,
            commands::recent::recent_remove,
            commands::recovery::recovery_save,
            commands::recovery::recovery_list,
            commands::recovery::recovery_restore,
            commands::recovery::recovery_discard,
            commands::trust::trust_get,
            commands::trust::trust_grant,
            commands::trust::trust_revoke,
            commands::toolchain::toolchain_list,
            commands::toolchain::toolchain_rescan,
            commands::toolchain::toolchain_add_dialog,
            commands::toolchain::toolchain_select,
            commands::toolchain::toolchain_setup_info,
            commands::build::build_start,
            commands::build::build_cancel,
            commands::build::build_cache_clear,
            commands::run::run_start,
            commands::run::run_input,
            commands::run::run_resize,
            commands::run::run_stop,
            commands::run::run_ack,
            commands::settings::settings_get,
            commands::settings::settings_update,
            commands::help::open_help_link,
        ])
        .on_window_event(window::on_window_event)
}

/// Why the app could not start. The messages name what failed; they contain
/// no path (the details go to the log at debug level).
#[derive(Debug, thiserror::Error)]
enum LaunchError {
    /// The app's folders could not be worked out from the environment.
    #[error("the app's folders could not be found ({0})")]
    Folders(#[source] StoreError),
    /// The end-to-end test settings are invalid.
    #[cfg(feature = "e2e-hooks")]
    #[error("the end-to-end test settings are invalid ({0})")]
    EndToEnd(#[from] e2e::E2eError),
    /// The backend refused its configuration.
    #[error("the backend could not start ({0})")]
    Backend(#[from] b2c_app::StartError),
    /// Tauri could not build the app or open its window.
    #[error("the window could not be opened ({0})")]
    Tauri(#[from] tauri::Error),
}

/// Logs why the app could not start and returns the failure exit code.
fn failed_to_start(error: &LaunchError) -> ExitCode {
    tracing::error!(%error, "the app could not start");
    // There may be no window to show this in. If stderr is gone too, the exit
    // code is all that is left.
    let _ = writeln!(std::io::stderr().lock(), "Blocks2Cpp could not start: {error}");
    ExitCode::FAILURE
}

/// How the app starts: its folders, where toolchain discovery looks, and (in
/// end-to-end builds) a script that answers the native dialogs.
struct Launch {
    dirs: Dirs,
    discovery: DiscoveryScope,
    #[cfg(feature = "e2e-hooks")]
    dialog_script: Option<e2e::DialogScript>,
}

impl fmt::Debug for Launch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Folders are paths: they are logged at debug level only, by `start`.
        f.debug_struct("Launch").finish_non_exhaustive()
    }
}

impl Launch {
    /// The normal start: the user's folders ([`Dirs::from_env`]) and the
    /// whole toolchain search order. With the `e2e-hooks` feature, the
    /// `B2C_E2E_*` variables can replace each of them.
    fn from_env() -> Result<Self, LaunchError> {
        #[cfg(feature = "e2e-hooks")]
        {
            let options = e2e::E2eOptions::from_env()?;
            let dirs = match &options.root {
                Some(root) => Dirs::under_root(root),
                None => Dirs::from_env().map_err(LaunchError::Folders)?,
            };
            let discovery = options
                .toolchain_dirs
                .map_or(DiscoveryScope::Default, DiscoveryScope::Only);
            Ok(Self {
                dirs,
                discovery,
                dialog_script: options.dialogs,
            })
        }
        #[cfg(not(feature = "e2e-hooks"))]
        {
            Ok(Self {
                dirs: Dirs::from_env().map_err(LaunchError::Folders)?,
                discovery: DiscoveryScope::Default,
            })
        }
    }

    /// The dialogs the backend raises: the native ones, or the script's.
    #[cfg_attr(
        not(feature = "e2e-hooks"),
        expect(clippy::unused_self, reason = "only end-to-end builds have a script to take")
    )]
    fn dialogs<R: Runtime>(&mut self, app: &AppHandle<R>) -> Arc<dyn Dialogs> {
        #[cfg(feature = "e2e-hooks")]
        if let Some(script) = self.dialog_script.take() {
            tracing::warn!("end-to-end hooks: the native dialogs are answered by a script");
            return Arc::new(e2e::ScriptedDialogs::new(script));
        }
        Arc::new(dialogs::NativeDialogs::new(app.clone()))
    }
}

/// Builds and runs the app; returns its exit code when the event loop ends.
fn start(launch: Launch) -> Result<ExitCode, LaunchError> {
    let app = configure(tauri::Builder::default())
        .setup(move |app| setup(app, launch).map_err(Into::into))
        .build(tauri::generate_context!())?;
    let code = app.run_return(|app, event| on_run_event(app, &event));
    Ok(u8::try_from(code).map_or(ExitCode::FAILURE, ExitCode::from))
}

/// Starts the backend, makes it the app's state and opens the main window
/// (Tauri's setup hook, on the main thread).
fn setup<R: Runtime>(app: &mut App<R>, mut launch: Launch) -> Result<(), LaunchError> {
    tracing::debug!(
        config = %launch.dirs.config.display(),
        cache = %launch.dirs.cache.display(),
        logs = %launch.dirs.logs.display(),
        "app folders"
    );
    let dialogs = launch.dialogs(app.handle());
    let backend = Backend::start(
        BackendConfig {
            dirs: launch.dirs,
            app_version: APP_VERSION,
            discovery_scope: launch.discovery,
            cwd: None,
        },
        dialogs,
    )?;
    app.manage(backend);
    window::open_main_window(app)?;
    Ok(())
}

/// The app's run loop events: on exit, shut the backend down (cancel every
/// build, kill every program's process tree). Shutting down is idempotent,
/// so it does not matter whether `app_quit` already did.
fn on_run_event<R: Runtime>(app: &AppHandle<R>, event: &RunEvent) {
    if let RunEvent::Exit = event {
        if let Some(backend) = app.try_state::<Arc<Backend>>() {
            backend.shutdown();
        }
        tracing::info!("exited");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_errors_name_what_failed_without_paths() {
        let error = LaunchError::Folders(StoreError::Invalid("HOME is not set"));
        assert_eq!(
            error.to_string(),
            "the app's folders could not be found (HOME is not set)"
        );
        let error = LaunchError::Backend(b2c_app::StartError::RelativeFolder("cache"));
        assert_eq!(
            error.to_string(),
            "the backend could not start (the cache folder is not an absolute path)"
        );
    }

    #[test]
    fn the_app_version_is_the_workspace_version() {
        assert_eq!(APP_VERSION, env!("CARGO_PKG_VERSION"));
        assert!(!APP_VERSION.is_empty());
    }
}
