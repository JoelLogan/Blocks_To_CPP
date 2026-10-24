//! Running: `run_start`, `run_input`, `run_resize`, `run_stop` and `run_ack`
//! (`docs/spec/02-architecture.md` §2.4.3, `docs/spec/07-toolchain-build-run.md`
//! §7.6).

use std::sync::Arc;

use b2c_ipc::dto::{Empty, RunEvent, RunStartResponse};
use b2c_ipc::{ByteSink, EventSink, IpcError};
use serde_json::Value;
use tauri::ipc::{Channel, InvokeResponseBody};

use super::{BackendState, blocking, decode, shared};
use crate::channels::{ByteChannel, EventChannel};

/// `run_start` ([`Backend::run_start`](b2c_app::Backend::run_start)): runs a
/// successful, current build's program on a pseudo-terminal. `on_output` gets
/// its output as raw byte batches, numbered from 1; `on_event` gets
/// `started`, `skipped` and exactly one `exit` event, last.
#[tauri::command]
pub(crate) async fn run_start(
    backend: BackendState<'_>,
    request: Option<Value>,
    on_output: Channel<InvokeResponseBody>,
    on_event: Channel<RunEvent>,
) -> Result<RunStartResponse, IpcError> {
    let backend = shared(&backend);
    let output: Arc<dyn ByteSink> = Arc::new(ByteChannel::new(on_output));
    let events: Arc<dyn EventSink<RunEvent>> = Arc::new(EventChannel::new(on_event));
    blocking("run_start", move || {
        backend.run_start(decode(request)?, output, events)
    })
    .await
}

/// `run_input` ([`Backend::run_input`](b2c_app::Backend::run_input)): keyboard
/// input for the program (base64, at most 64 KiB, rate-limited).
#[tauri::command]
pub(crate) async fn run_input(backend: BackendState<'_>, request: Option<Value>) -> Result<Empty, IpcError> {
    let backend = shared(&backend);
    blocking("run_input", move || backend.run_input(decode(request)?)).await
}

/// `run_resize` ([`Backend::run_resize`](b2c_app::Backend::run_resize)): the
/// console's new size.
#[tauri::command]
pub(crate) async fn run_resize(backend: BackendState<'_>, request: Option<Value>) -> Result<Empty, IpcError> {
    let backend = shared(&backend);
    blocking("run_resize", move || backend.run_resize(decode(request)?)).await
}

/// `run_stop` ([`Backend::run_stop`](b2c_app::Backend::run_stop)): stops the
/// program and kills its process tree.
#[tauri::command]
pub(crate) async fn run_stop(backend: BackendState<'_>, request: Option<Value>) -> Result<Empty, IpcError> {
    let backend = shared(&backend);
    blocking("run_stop", move || backend.run_stop(decode(request)?)).await
}

/// `run_ack` ([`Backend::run_ack`](b2c_app::Backend::run_ack)): the console
/// has written the output batches up to `seq` (flow control).
#[tauri::command]
pub(crate) async fn run_ack(backend: BackendState<'_>, request: Option<Value>) -> Result<Empty, IpcError> {
    let backend = shared(&backend);
    blocking("run_ack", move || backend.run_ack(decode(request)?)).await
}
