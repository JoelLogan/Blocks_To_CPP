//! Trust: `trust_get`, `trust_grant` and `trust_revoke`
//! (`docs/spec/08-security.md` §8.3, §8.3.1). Only the native trust dialog,
//! raised by the backend, can grant trust; the webview cannot answer it.

use b2c_ipc::IpcError;
use b2c_ipc::dto::TrustResponse;
use serde_json::Value;

use super::{BackendState, blocking, decode, shared};

/// `trust_get` ([`Backend::trust_get`](b2c_app::Backend::trust_get)): the
/// project's trust state.
#[tauri::command]
pub(crate) async fn trust_get(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<TrustResponse, IpcError> {
    let backend = shared(&backend);
    blocking("trust_get", move || backend.trust_get(decode(request)?)).await
}

/// `trust_grant` ([`Backend::trust_grant`](b2c_app::Backend::trust_grant)):
/// shows the native trust dialog, listing what the backend's own copy of the
/// project contains, and records the user's choice.
#[tauri::command]
pub(crate) async fn trust_grant(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<TrustResponse, IpcError> {
    let backend = shared(&backend);
    blocking("trust_grant", move || backend.trust_grant(decode(request)?)).await
}

/// `trust_revoke` ([`Backend::trust_revoke`](b2c_app::Backend::trust_revoke)):
/// removes the project's trust record.
#[tauri::command]
pub(crate) async fn trust_revoke(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<TrustResponse, IpcError> {
    let backend = shared(&backend);
    blocking("trust_revoke", move || backend.trust_revoke(decode(request)?)).await
}
