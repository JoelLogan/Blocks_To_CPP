//! Recent projects: `recent_list` and `recent_remove`
//! (`docs/spec/05-project-format.md` §5.9). Entries are named by opaque IDs;
//! their paths are for display only and never come back over IPC.

use b2c_ipc::IpcError;
use b2c_ipc::dto::{Empty, RecentListResponse};
use serde_json::Value;

use super::{BackendState, blocking, decode, shared};

/// `recent_list` ([`Backend::recent_list`](b2c_app::Backend::recent_list)):
/// the recent projects, newest first.
#[tauri::command]
pub(crate) async fn recent_list(backend: BackendState<'_>) -> Result<RecentListResponse, IpcError> {
    let backend = shared(&backend);
    blocking("recent_list", move || backend.recent_list()).await
}

/// `recent_remove` ([`Backend::recent_remove`](b2c_app::Backend::recent_remove)):
/// removes one entry from the list (the file is not touched).
#[tauri::command]
pub(crate) async fn recent_remove(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<Empty, IpcError> {
    let backend = shared(&backend);
    blocking("recent_remove", move || backend.recent_remove(decode(request)?)).await
}
