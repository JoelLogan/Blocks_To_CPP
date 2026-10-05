//! Autosave and crash recovery: `recovery_save`, `recovery_list`,
//! `recovery_restore` and `recovery_discard`
//! (`docs/spec/05-project-format.md` §5.10). Snapshots are named by opaque IDs.

use b2c_ipc::IpcError;
use b2c_ipc::dto::{Empty, RecoveryListResponse, RecoveryRestoreResponse};
use serde_json::Value;

use super::{BackendState, blocking, decode, shared};

/// `recovery_save` ([`Backend::recovery_save`](b2c_app::Backend::recovery_save)):
/// writes the project's autosave snapshot.
#[tauri::command]
pub(crate) async fn recovery_save(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<Empty, IpcError> {
    let backend = shared(&backend);
    blocking("recovery_save", move || backend.recovery_save(decode(request)?)).await
}

/// `recovery_list` ([`Backend::recovery_list`](b2c_app::Backend::recovery_list)):
/// the snapshots left by instances that exited or crashed.
#[tauri::command]
pub(crate) async fn recovery_list(backend: BackendState<'_>) -> Result<RecoveryListResponse, IpcError> {
    let backend = shared(&backend);
    blocking("recovery_list", move || backend.recovery_list()).await
}

/// `recovery_restore`
/// ([`Backend::recovery_restore`](b2c_app::Backend::recovery_restore)): opens
/// a snapshot as a project, with trust re-evaluated.
#[tauri::command]
pub(crate) async fn recovery_restore(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<RecoveryRestoreResponse, IpcError> {
    let backend = shared(&backend);
    blocking("recovery_restore", move || {
        backend.recovery_restore(decode(request)?)
    })
    .await
}

/// `recovery_discard`
/// ([`Backend::recovery_discard`](b2c_app::Backend::recovery_discard)):
/// deletes a snapshot.
#[tauri::command]
pub(crate) async fn recovery_discard(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<Empty, IpcError> {
    let backend = shared(&backend);
    blocking("recovery_discard", move || {
        backend.recovery_discard(decode(request)?)
    })
    .await
}
