//! `project_export_dialog`: export as a C++ project
//! (`docs/spec/06-compiler-pipeline.md` §6.11,
//! `docs/plans/m3-contracts/w3-export-run.md` §4).
//!
//! The backend generates the C++ itself from the document, as for a build,
//! and writes the export into `<chosen folder>/<folder name>/` (the
//! project's name made a file name, else the program's), never anywhere
//! else: a new or empty folder, or, after the user confirms in a native
//! dialog, an earlier export of the same project (whose own files only are
//! replaced). A folder with other files, or another project's export, gives
//! `io` (`alreadyExists`) before any question. Export is allowed
//! in Restricted Mode: it runs nothing (`docs/spec/08-security.md` §8.3
//! disables building and running).

use std::path::Path;

use b2c_build::export::{self, ExportError, ExportMode, FolderState};
use b2c_ipc::dto::{ProjectExportDialogRequest, ProjectExportDialogResponse};
use b2c_ipc::{IoKind, IpcError};

use crate::backend::{Backend, command_span};
use crate::dialogs::display_text;
use crate::projects::lock;

/// The most characters of the folder's name shown.
const MAX_FOLDER_NAME_CHARS: usize = 100;

/// The response for a cancelled export.
fn cancelled() -> ProjectExportDialogResponse {
    ProjectExportDialogResponse {
        cancelled: true,
        folder_name: None,
        files: 0,
    }
}

// Commands take their request by value, as the adapter decodes it.
#[allow(clippy::needless_pass_by_value)]
impl Backend {
    /// `project_export_dialog`: see the module documentation. The folder
    /// dialog starts in the project's folder.
    ///
    /// # Errors
    /// The document's errors (`payloadTooLarge`, `invalidDocument`,
    /// `newerFormat`); [`IpcError::UnknownHandle`];
    /// [`IpcError::ProjectErrors`] when the project has errors;
    /// [`IpcError::DefineNotExportable`] for a define only the export
    /// cannot use;
    /// [`IpcError::Busy`] while another native dialog is open;
    /// [`IpcError::Io`] (`alreadyExists` for a folder with other files in
    /// it, otherwise the failure's kind).
    pub fn project_export_dialog(
        &self,
        request: ProjectExportDialogRequest,
    ) -> Result<ProjectExportDialogResponse, IpcError> {
        let _span = command_span("project_export_dialog");
        b2c_ipc::parse_document(&request.document)?;
        let entry_ref = self.projects.get(&request.handle)?;
        let project_folder = lock(&entry_ref).folder().map(Path::to_path_buf);
        let export = export::prepare(
            request.document.as_bytes(),
            self.settings.get().code_style.indent_width,
            self.app_version,
        )
        .map_err(|error| export_error(&error))?;
        let picked = {
            let _dialog = self.dialog_gate.enter()?;
            self.dialogs.pick_export_folder(project_folder.as_deref())
        };
        let Some(picked) = picked else {
            return Ok(cancelled());
        };
        let parent = std::fs::canonicalize(&picked).map_err(|error| {
            tracing::debug!(%error, "the chosen export folder cannot be found");
            IpcError::io(&error)
        })?;
        let target = parent.join(&export.folder_name);
        let mode = match export::folder_state(&target) {
            FolderState::Empty => ExportMode::New,
            FolderState::EarlierExport
                if export::earlier_export_project(&target)
                    .flatten()
                    .is_some_and(|id| id != export.project_id) =>
            {
                tracing::info!("export refused: the folder holds another project's export");
                return Err(IpcError::Io {
                    kind: IoKind::AlreadyExists,
                });
            }
            FolderState::EarlierExport => {
                let replace = {
                    let _dialog = self.dialog_gate.enter()?;
                    self.dialogs.confirm_replace_export(&export.folder_name)
                };
                if !replace {
                    return Ok(cancelled());
                }
                ExportMode::Replace
            }
            FolderState::Other => {
                tracing::info!("export refused: the folder holds other files");
                return Err(IpcError::Io {
                    kind: IoKind::AlreadyExists,
                });
            }
        };
        let written = export::write(&target, &export, mode).map_err(|error| export_error(&error))?;
        tracing::info!(files = written.len(), "project exported");
        Ok(ProjectExportDialogResponse {
            cancelled: false,
            folder_name: Some(display_text(&export.folder_name, MAX_FOLDER_NAME_CHARS)),
            files: u32::try_from(written.len()).unwrap_or(u32::MAX),
        })
    }
}

/// The IPC error for a failed export; the details go to the log.
fn export_error(error: &ExportError) -> IpcError {
    tracing::debug!(%error, "export failed");
    match error {
        ExportError::ProjectErrors(problems) => {
            let errors = problems
                .iter()
                .filter(|problem| problem.severity == b2c_ir::Severity::Error)
                .count();
            IpcError::ProjectErrors {
                count: u32::try_from(errors.max(1)).unwrap_or(u32::MAX),
            }
        }
        ExportError::DefineNotExportable(_) => IpcError::DefineNotExportable,
        ExportError::NotEmpty
        | ExportError::NoEarlierExport
        | ExportError::OtherProject
        | ExportError::NotWrittenByExport(_) => IpcError::Io {
            kind: IoKind::AlreadyExists,
        },
        ExportError::NotAPlainFile => IpcError::Io { kind: IoKind::Other },
        ExportError::Io { source, .. } => IpcError::io(source),
    }
}
