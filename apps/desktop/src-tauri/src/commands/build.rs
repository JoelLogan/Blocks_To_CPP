//! Building: `build_start`, `build_cancel` and `build_cache_clear`
//! (`docs/spec/02-architecture.md` §2.4.2, `docs/spec/07-toolchain-build-run.md`
//! §7.5).

use std::sync::Arc;

use b2c_ipc::dto::{BuildCacheClearResponse, BuildEvent, BuildStartResponse, Empty};
use b2c_ipc::{EventSink, IpcError};
use serde_json::Value;
use tauri::ipc::Channel;

use super::{BackendState, blocking, decode, shared};
use crate::channels::EventChannel;

/// `build_start` ([`Backend::build_start`](b2c_app::Backend::build_start)):
/// builds the document on its own thread and returns the build's ID at once.
/// `on_event` gets the progress, the diagnostics and exactly one `finished`
/// event, last. A project in Restricted Mode is refused before anything runs.
#[tauri::command]
pub(crate) async fn build_start(
    backend: BackendState<'_>,
    request: Option<Value>,
    on_event: Channel<BuildEvent>,
) -> Result<BuildStartResponse, IpcError> {
    let backend = shared(&backend);
    let sink: Arc<dyn EventSink<BuildEvent>> = Arc::new(EventChannel::new(on_event));
    blocking("build_start", move || backend.build_start(decode(request)?, sink)).await
}

/// `build_cancel` ([`Backend::build_cancel`](b2c_app::Backend::build_cancel)):
/// cancels a build, killing the compiler's process tree.
#[tauri::command]
pub(crate) async fn build_cancel(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<Empty, IpcError> {
    let backend = shared(&backend);
    blocking("build_cancel", move || backend.build_cancel(decode(request)?)).await
}

/// `build_cache_clear`
/// ([`Backend::build_cache_clear`](b2c_app::Backend::build_cache_clear)):
/// deletes the build cache, except what a build or a running program uses.
#[tauri::command]
pub(crate) async fn build_cache_clear(
    backend: BackendState<'_>,
) -> Result<BuildCacheClearResponse, IpcError> {
    let backend = shared(&backend);
    blocking("build_cache_clear", move || backend.build_cache_clear()).await
}
