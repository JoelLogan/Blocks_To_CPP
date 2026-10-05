//! Settings: `settings_get` and `settings_update`
//! (`docs/spec/05-project-format.md` §5.9). The webview may change only the
//! code style, the Run-on-errors choice and the console's scrollback.

use b2c_ipc::IpcError;
use b2c_ipc::dto::{SettingsGetResponse, SettingsUpdateResponse};
use serde_json::Value;

use super::{BackendState, blocking, decode, shared};

/// `settings_get` ([`Backend::settings_get`](b2c_app::Backend::settings_get)):
/// the settings and the notices from loading them.
#[tauri::command]
pub(crate) async fn settings_get(backend: BackendState<'_>) -> Result<SettingsGetResponse, IpcError> {
    let backend = shared(&backend);
    blocking("settings_get", move || backend.settings_get()).await
}

/// `settings_update` ([`Backend::settings_update`](b2c_app::Backend::settings_update)):
/// changes the settings the patch names and saves them.
#[tauri::command]
pub(crate) async fn settings_update(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<SettingsUpdateResponse, IpcError> {
    let backend = shared(&backend);
    blocking("settings_update", move || {
        backend.settings_update(decode(request)?)
    })
    .await
}
