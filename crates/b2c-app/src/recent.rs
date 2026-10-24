//! The recent-projects list: `recent_list` and `recent_remove`
//! (`docs/spec/05-project-format.md` §5.9, `docs/spec/04-user-interface.md`
//! §4.10). `project_open_recent` is in `projects`.
//!
//! The list lives in `recent.json` ([`b2c_store::RecentStore`]): at most 10
//! projects, newest first. The webview sees only the opaque `rc_` IDs and a
//! display path; the path behind an ID never leaves the backend except for
//! display, and is never accepted back.

use std::path::Path;

use b2c_ipc::dto::{Empty, RecentEntry, RecentListResponse, RecentRemoveRequest};
use b2c_ipc::{IpcError, RecentId};

use crate::backend::{Backend, command_span};
use crate::errors::store_error;
use crate::limits::RECENT_MAX;

// Commands take their request by value, as the adapter decodes it, so every
// command method has the same shape whether or not it keeps the request.
#[allow(clippy::needless_pass_by_value)]
impl Backend {
    /// `recent_list`: the recent projects, newest first, at most 10. A file
    /// that no longer exists stays in the list (opening it gives `notFound`).
    ///
    /// # Errors
    /// None today; the `Result` keeps the command shape.
    pub fn recent_list(&self) -> Result<RecentListResponse, IpcError> {
        let _span = command_span("recent_list");
        let entries = self
            .recent
            .list()
            .into_iter()
            .filter_map(|entry| {
                Some(RecentEntry {
                    recent_id: RecentId::parse(&entry.id).ok()?,
                    project_name: entry.project_name,
                    display_path: entry.path.display().to_string(),
                    last_opened_at: entry.last_opened_at,
                })
            })
            .take(RECENT_MAX)
            .collect();
        Ok(RecentListResponse { entries })
    }

    /// `recent_remove`: removes an entry from the list.
    ///
    /// # Errors
    /// [`IpcError::UnknownRecent`] when there is no such entry;
    /// [`IpcError::Io`] when the list cannot be saved (it is then unchanged).
    pub fn recent_remove(&self, request: RecentRemoveRequest) -> Result<Empty, IpcError> {
        let _span = command_span("recent_remove");
        let removed = self
            .recent
            .remove(request.recent_id.as_str())
            .map_err(|error| store_error("update the recent list", &error))?;
        if removed {
            Ok(Empty {})
        } else {
            Err(IpcError::UnknownRecent)
        }
    }

    /// Records that the project at `path` was opened or saved now. A failure
    /// is logged and does not fail the command.
    pub(crate) fn touch_recent(&self, path: &Path, project_name: &str) {
        if let Err(error) = self.recent.touch(path, project_name) {
            let _ = store_error("update the recent list", &error);
            tracing::warn!("the recent list could not be updated");
        }
    }
}
