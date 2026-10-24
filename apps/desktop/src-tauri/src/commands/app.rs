//! App commands: `app_info`, `app_subscribe` and `app_quit`
//! (`docs/spec/02-architecture.md` §2.5.2, §2.5.3, §2.5.7, §2.6).

use std::sync::Arc;

use b2c_ipc::dto::{AppEvent, AppInfo, Empty};
use b2c_ipc::{EventSink, IpcError};
use tauri::ipc::Channel;
use tauri::{AppHandle, Runtime};

use super::{BackendState, blocking, shared};
use crate::channels::EventChannel;

/// `app_info` ([`Backend::app_info`](b2c_app::Backend::app_info)): versions
/// and platform; the frontend refuses to run against another IPC version.
#[tauri::command]
pub(crate) async fn app_info(backend: BackendState<'_>) -> Result<AppInfo, IpcError> {
    let backend = shared(&backend);
    blocking("app_info", move || Ok(backend.app_info())).await
}

/// `app_subscribe` ([`Backend::app_subscribe`](b2c_app::Backend::app_subscribe)):
/// makes `on_event` the channel of app events (external changes, toolchain
/// updates, settings notices, close requests), replacing an earlier one.
#[tauri::command]
pub(crate) async fn app_subscribe(
    backend: BackendState<'_>,
    on_event: Channel<AppEvent>,
) -> Result<Empty, IpcError> {
    let backend = shared(&backend);
    let sink: Arc<dyn EventSink<AppEvent>> = Arc::new(EventChannel::new(on_event));
    blocking("app_subscribe", move || {
        backend.app_subscribe(sink);
        Ok(Empty {})
    })
    .await
}

/// `app_quit` ([`Backend::app_quit`](b2c_app::Backend::app_quit)): shuts the
/// backend down (every build cancelled, every program's process tree killed)
/// and then exits the app. The frontend calls it after asking about unsaved
/// changes.
#[tauri::command]
pub(crate) async fn app_quit<R: Runtime>(
    app: AppHandle<R>,
    backend: BackendState<'_>,
) -> Result<Empty, IpcError> {
    let backend = shared(&backend);
    let response = blocking("app_quit", move || backend.app_quit()).await?;
    tracing::info!("quitting at the user's request");
    app.exit(0);
    Ok(response)
}
