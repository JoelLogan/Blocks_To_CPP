//! The run options kept on this computer: `run_input_file`, the file a
//! project's programs read as their standard input
//! (`docs/plans/m3-contracts/w3-export-run.md` §3.2). The arguments and the
//! working folder are in the project; the external terminal is a machine
//! setting (`settings.json` `run.externalTerminal`).
//!
//! The file comes only from a native dialog and is kept by the project's ID
//! until the app ends (at most [`MAX_INPUT_FILES`] projects). Only its name
//! crosses IPC.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use b2c_ipc::IpcError;
use b2c_ipc::dto::{RunInputFileAction, RunInputFileRequest, RunInputFileResponse};

use crate::backend::{Backend, command_span};
use crate::dialogs::display_text;
use crate::projects::lock;

/// The most projects whose input file is remembered.
pub(crate) const MAX_INPUT_FILES: usize = 256;

/// The most characters of a file name shown.
const MAX_FILE_NAME_CHARS: usize = 100;

/// The input files chosen for projects, by project ID.
#[derive(Debug, Default)]
pub(crate) struct InputFiles {
    files: Mutex<HashMap<String, PathBuf>>,
}

impl InputFiles {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, PathBuf>> {
        self.files.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The input file of the project `project_id`, if one is chosen.
    pub(crate) fn get(&self, project_id: &str) -> Option<PathBuf> {
        self.lock().get(project_id).cloned()
    }

    fn set(&self, project_id: &str, path: PathBuf) {
        let mut files = self.lock();
        if files.len() >= MAX_INPUT_FILES && !files.contains_key(project_id) {
            // Bounded: forget one other project's choice.
            if let Some(other) = files.keys().next().cloned() {
                files.remove(&other);
            }
        }
        files.insert(project_id.to_owned(), path);
    }

    fn clear(&self, project_id: &str) {
        self.lock().remove(project_id);
    }
}

/// The response for the file `path` (or none).
fn response(path: Option<&Path>, cancelled: bool) -> RunInputFileResponse {
    RunInputFileResponse {
        file_name: path.map(|path| {
            path.file_name()
                .map(|name| display_text(&name.to_string_lossy(), MAX_FILE_NAME_CHARS))
                .unwrap_or_default()
        }),
        cancelled,
    }
}

// Commands take their request by value, as the adapter decodes it.
#[allow(clippy::needless_pass_by_value)]
impl Backend {
    /// `run_input_file`: says, chooses (in a native dialog that starts in
    /// the project's folder) or forgets the file the project's programs read
    /// as their standard input. The chosen file must be a regular file; it
    /// is opened again, read-only, each time a program starts.
    ///
    /// # Errors
    /// [`IpcError::UnknownHandle`]; [`IpcError::Busy`] while another native
    /// dialog is open; [`IpcError::Io`] (`other`) for a pick that is not a
    /// regular file, (`notFound`) for one that is gone.
    pub fn run_input_file(&self, request: RunInputFileRequest) -> Result<RunInputFileResponse, IpcError> {
        let _span = command_span("run_input_file");
        let entry_ref = self.projects.get(&request.handle)?;
        let (project_id, folder) = {
            let entry = lock(&entry_ref);
            (
                entry.project_id.as_str().to_owned(),
                entry.folder().map(Path::to_path_buf),
            )
        };
        match request.action {
            RunInputFileAction::Get => Ok(response(self.input_files.get(&project_id).as_deref(), false)),
            RunInputFileAction::Clear => {
                self.input_files.clear(&project_id);
                Ok(response(None, false))
            }
            RunInputFileAction::Choose => {
                let picked = {
                    let _dialog = self.dialog_gate.enter()?;
                    self.dialogs.pick_input_file(folder.as_deref())
                };
                let Some(picked) = picked else {
                    return Ok(response(self.input_files.get(&project_id).as_deref(), true));
                };
                let path = std::fs::canonicalize(&picked).map_err(|error| {
                    tracing::debug!(%error, "the chosen input file cannot be found");
                    IpcError::io(&error)
                })?;
                if !std::fs::metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
                    tracing::info!("the chosen input file is not a regular file");
                    return Err(IpcError::Io {
                        kind: b2c_ipc::IoKind::Other,
                    });
                }
                self.input_files.set(&project_id, path.clone());
                Ok(response(Some(&path), false))
            }
        }
    }
}
