//! Project commands: new, open (dialog and recent), reload, save, save as,
//! close and the dirty flag (`docs/spec/02-architecture.md` §2.5.2,
//! `docs/spec/05-project-format.md` §5.10).
//!
//! The document crosses IPC as BDM JSON text in both directions; the backend
//! checks its size before parsing it with the strict loader. No request
//! carries a path: files come from the native dialogs, the recent list or the
//! handle's bound file.

use b2c_ipc::IpcError;
use b2c_ipc::dto::{
    Empty, ProjectNewResponse, ProjectOpenDialogResponse, ProjectOpened, ProjectReloadResponse,
    ProjectSaveAsDialogResponse, ProjectSaveResponse,
};
use serde_json::Value;

use super::{BackendState, blocking, decode, shared};

/// `project_new` ([`Backend::project_new`](b2c_app::Backend::project_new)):
/// a project from a built-in template, trusted because it was created here.
#[tauri::command]
pub(crate) async fn project_new(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<ProjectNewResponse, IpcError> {
    let backend = shared(&backend);
    blocking("project_new", move || backend.project_new(decode(request)?)).await
}

/// `project_open_dialog`
/// ([`Backend::project_open_dialog`](b2c_app::Backend::project_open_dialog)):
/// the native open dialog, then the file is loaded and its trust evaluated.
#[tauri::command]
pub(crate) async fn project_open_dialog(
    backend: BackendState<'_>,
) -> Result<ProjectOpenDialogResponse, IpcError> {
    let backend = shared(&backend);
    blocking("project_open_dialog", move || backend.project_open_dialog()).await
}

/// `project_open_recent`
/// ([`Backend::project_open_recent`](b2c_app::Backend::project_open_recent)):
/// opens an entry of the recent list by its ID.
#[tauri::command]
pub(crate) async fn project_open_recent(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<ProjectOpened, IpcError> {
    let backend = shared(&backend);
    blocking("project_open_recent", move || {
        backend.project_open_recent(decode(request)?)
    })
    .await
}

/// `project_reload` ([`Backend::project_reload`](b2c_app::Backend::project_reload)):
/// reads the project's file again, re-checking trust.
#[tauri::command]
pub(crate) async fn project_reload(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<ProjectReloadResponse, IpcError> {
    let backend = shared(&backend);
    blocking("project_reload", move || backend.project_reload(decode(request)?)).await
}

/// `project_save` ([`Backend::project_save`](b2c_app::Backend::project_save)):
/// writes the document to the handle's own file, atomically, unless the file
/// changed on disk.
#[tauri::command]
pub(crate) async fn project_save(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<ProjectSaveResponse, IpcError> {
    let backend = shared(&backend);
    blocking("project_save", move || backend.project_save(decode(request)?)).await
}

/// `project_save_as_dialog`
/// ([`Backend::project_save_as_dialog`](b2c_app::Backend::project_save_as_dialog)):
/// the native save dialog, then the document is written and the handle bound
/// to the new file.
#[tauri::command]
pub(crate) async fn project_save_as_dialog(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<ProjectSaveAsDialogResponse, IpcError> {
    let backend = shared(&backend);
    blocking("project_save_as_dialog", move || {
        backend.project_save_as_dialog(decode(request)?)
    })
    .await
}

/// `project_close` ([`Backend::project_close`](b2c_app::Backend::project_close)):
/// stops the project's build and program and forgets the handle.
#[tauri::command]
pub(crate) async fn project_close(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<Empty, IpcError> {
    let backend = shared(&backend);
    blocking("project_close", move || backend.project_close(decode(request)?)).await
}

/// `project_set_dirty`
/// ([`Backend::project_set_dirty`](b2c_app::Backend::project_set_dirty)):
/// whether the project has unsaved changes, so closing the window asks first.
#[tauri::command]
pub(crate) async fn project_set_dirty(
    backend: BackendState<'_>,
    request: Option<Value>,
) -> Result<Empty, IpcError> {
    let backend = shared(&backend);
    blocking("project_set_dirty", move || {
        backend.project_set_dirty(decode(request)?)
    })
    .await
}
