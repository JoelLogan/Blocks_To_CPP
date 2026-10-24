//! Help links: `open_help_link` (`docs/spec/08-security.md` §8.8). The
//! webview names one of a closed set of pages; it never passes a URL, and the
//! browser is started from Rust (the webview has no opener permission).

use b2c_ipc::IpcError;
use b2c_ipc::dto::Empty;
use serde_json::Value;

use super::{BackendState, blocking, decode, shared};

/// `open_help_link` ([`Backend::open_help_link`](b2c_app::Backend::open_help_link)):
/// opens a fixed `https` help page in the system browser.
#[tauri::command]
pub(crate) async fn open_help_link(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<Empty, IpcError> {
    let backend = shared(&backend);
    blocking("open_help_link", move || backend.open_help_link(decode(request)?)).await
}
