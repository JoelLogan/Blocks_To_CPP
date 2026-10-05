//! Toolchains: list, rescan, add (native dialog), select and setup
//! information (`docs/spec/07-toolchain-build-run.md` §7.2–§7.3,
//! `docs/spec/04-user-interface.md` §4.6).

use b2c_ipc::IpcError;
use b2c_ipc::dto::{Empty, ToolchainAddDialogResponse, ToolchainListResponse, ToolchainSetupInfo};
use serde_json::Value;

use super::{BackendState, blocking, decode, shared};

/// `toolchain_list` ([`Backend::toolchain_list`](b2c_app::Backend::toolchain_list)):
/// the known toolchains, and whether discovery is still running.
#[tauri::command]
pub(crate) async fn toolchain_list(backend: BackendState<'_>) -> Result<ToolchainListResponse, IpcError> {
    let backend = shared(&backend);
    blocking("toolchain_list", move || backend.toolchain_list()).await
}

/// `toolchain_rescan` ([`Backend::toolchain_rescan`](b2c_app::Backend::toolchain_rescan)):
/// starts discovery again.
#[tauri::command]
pub(crate) async fn toolchain_rescan(backend: BackendState<'_>) -> Result<ToolchainListResponse, IpcError> {
    let backend = shared(&backend);
    blocking("toolchain_rescan", move || backend.toolchain_rescan()).await
}

/// `toolchain_add_dialog`
/// ([`Backend::toolchain_add_dialog`](b2c_app::Backend::toolchain_add_dialog)):
/// the native file dialog for a g++ executable, which is then probed.
#[tauri::command]
pub(crate) async fn toolchain_add_dialog(
    backend: BackendState<'_>,
) -> Result<ToolchainAddDialogResponse, IpcError> {
    let backend = shared(&backend);
    blocking("toolchain_add_dialog", move || backend.toolchain_add_dialog()).await
}

/// `toolchain_select` ([`Backend::toolchain_select`](b2c_app::Backend::toolchain_select)):
/// makes a toolchain of the list the default.
#[tauri::command]
pub(crate) async fn toolchain_select(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<Empty, IpcError> {
    let backend = shared(&backend);
    blocking("toolchain_select", move || {
        backend.toolchain_select(decode(request)?)
    })
    .await
}

/// `toolchain_setup_info`
/// ([`Backend::toolchain_setup_info`](b2c_app::Backend::toolchain_setup_info)):
/// the platform and Linux distribution, for the setup page's instructions.
#[tauri::command]
pub(crate) async fn toolchain_setup_info(backend: BackendState<'_>) -> Result<ToolchainSetupInfo, IpcError> {
    let backend = shared(&backend);
    blocking("toolchain_setup_info", move || backend.toolchain_setup_info()).await
}
