//! Workspace trust: `trust_get`, `trust_grant` and `trust_revoke`
//! (`docs/spec/08-security.md` §8.3).
//!
//! The backend holds each open project's trust ([`HandleTrust`]), computed when
//! the file is loaded (open, open recent, reload; restore in
//! `w4-recovery-watcher`) by [`b2c_store::TrustStore::evaluate`], and changed
//! only by:
//!
//! * `trust_grant`, after the native dialog ([`crate::Dialogs::confirm_trust`])
//!   — the only way to trust a project the app did not create;
//! * `trust_revoke`, which removes the project's own record;
//! * the first save of a project created here, and *Save as* of a trusted
//!   project, which record trust for the file (`docs/spec/08-security.md`
//!   §8.3.1).
//!
//! Build and run refuse a project that is not trusted
//! ([`HandleTrust::is_trusted`]).

use std::path::Path;
use std::time::Instant;

use b2c_ipc::IpcError;
use b2c_ipc::dto::{
    RestrictedReason as DtoReason, Trust, TrustGetRequest, TrustGrantRequest, TrustResponse,
    TrustRevokeRequest, TrustSource as DtoSource, TrustState,
};
use b2c_model::Document;
use b2c_store::{ProjectIdentity, RestrictedReason, TrustSource, TrustVerdict};

use crate::backend::{Backend, command_span};
use crate::dialogs::{TrustChoice, TrustPrompt, dialog_libraries, display_text};
use crate::errors::store_error;
use crate::limits::{MAX_DIALOG_TEXT_CHARS, TRUST_GRANT_INTERVAL};
use crate::projects::{ProjectEntry, lock};

/// The trust of an open project, as the backend holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum HandleTrust {
    /// A new project created in this app, trusted in memory until its first
    /// save records trust for its file.
    CreatedHere,
    /// Trusted by a record of the trust store.
    Trusted(TrustSource),
    /// Restricted Mode.
    Restricted(RestrictedReason),
}

impl HandleTrust {
    /// Whether building and running are allowed.
    pub(crate) fn is_trusted(self) -> bool {
        !matches!(self, Self::Restricted(_))
    }

    /// The trust of the store's verdict.
    pub(crate) fn from_verdict(verdict: TrustVerdict) -> Self {
        match verdict {
            TrustVerdict::Trusted(source) => Self::Trusted(source),
            TrustVerdict::Restricted(reason) => Self::Restricted(reason),
        }
    }

    /// The IPC form.
    pub(crate) fn to_dto(self, mark_of_the_web: bool) -> Trust {
        let (state, source, restricted_reason) = match self {
            Self::CreatedHere => (TrustState::Trusted, Some(DtoSource::CreatedHere), None),
            Self::Trusted(TrustSource::Project) => (TrustState::Trusted, Some(DtoSource::Project), None),
            Self::Trusted(TrustSource::Folder) => (TrustState::Trusted, Some(DtoSource::Folder), None),
            Self::Restricted(RestrictedReason::NoRecord) => {
                (TrustState::Restricted, None, Some(DtoReason::NoRecord))
            }
            Self::Restricted(RestrictedReason::ChangedOutside) => {
                (TrustState::Restricted, None, Some(DtoReason::ChangedOutside))
            }
        };
        Trust {
            state,
            source,
            restricted_reason,
            mark_of_the_web,
        }
    }
}

/// The trust store's identity of `document` saved at `canonical_path`.
pub(crate) fn identity(document: &Document, canonical_path: &Path) -> ProjectIdentity {
    ProjectIdentity {
        project_id: document.project.id.clone(),
        canonical_path: canonical_path.to_path_buf(),
        security_hash: b2c_model::security_hash(document),
    }
}

/// What the trust dialog shows for `entry`: the security summary of its
/// latest document, with everything taken from the project made safe to
/// show.
pub(crate) fn trust_prompt(entry: &ProjectEntry) -> TrustPrompt {
    let summary = b2c_model::security_summary(&entry.latest_document);
    let folder_display = entry
        .path
        .as_deref()
        .and_then(Path::parent)
        .map(|folder| display_text(&folder.display().to_string(), MAX_DIALOG_TEXT_CHARS))
        .unwrap_or_default();
    TrustPrompt {
        project_name: display_text(&entry.latest_document.project.name, MAX_DIALOG_TEXT_CHARS),
        folder_display,
        raw_cpp_blocks: summary.raw_cpp_blocks.len(),
        libraries: dialog_libraries(&summary.libraries),
        file_system_blocks: summary.file_system_blocks.len(),
        mark_of_the_web: entry.mark_of_the_web,
    }
}

// Commands take their request by value, as the adapter decodes it, so every
// command method has the same shape whether or not it keeps the request.
#[allow(clippy::needless_pass_by_value)]
impl Backend {
    /// `trust_get`: the project's trust.
    ///
    /// # Errors
    /// [`IpcError::UnknownHandle`].
    pub fn trust_get(&self, request: TrustGetRequest) -> Result<TrustResponse, IpcError> {
        let _span = command_span("trust_get");
        let entry = self.projects.get(&request.handle)?;
        let entry = lock(&entry);
        Ok(TrustResponse {
            trust: entry.trust.to_dto(entry.mark_of_the_web),
        })
    }

    /// `trust_grant`: asks the user in the native trust dialog, which lists the
    /// Raw C++ blocks, libraries and file-system blocks of the latest document
    /// the backend received for the project, and records the answer
    /// (`docs/spec/08-security.md` §8.3):
    ///
    /// * *Trust this project*: a project record with that document's security
    ///   hash (a project that was never saved is trusted in memory, like a new
    ///   one, until its first save records it);
    /// * *Trust everything in this folder*: a folder record for the project
    ///   file's folder (for a project that was never saved, the same as
    ///   *Trust this project*);
    /// * *Stay in Restricted Mode*, or closing the dialog: nothing changes.
    ///
    /// A project that is already trusted is returned unchanged without a
    /// dialog.
    ///
    /// # Errors
    /// [`IpcError::UnknownHandle`] (also when the project was closed while the
    /// dialog was open); [`IpcError::RateLimited`] within 2 s of the previous
    /// call for the project; [`IpcError::Busy`] while another dialog is open;
    /// [`IpcError::Io`] or [`IpcError::Internal`] when the trust store cannot
    /// be written (the project then stays restricted).
    pub fn trust_grant(&self, request: TrustGrantRequest) -> Result<TrustResponse, IpcError> {
        let _span = command_span("trust_grant");
        let entry_ref = self.projects.get(&request.handle)?;
        let unchanged = |entry: &ProjectEntry| TrustResponse {
            trust: entry.trust.to_dto(entry.mark_of_the_web),
        };
        {
            let entry = lock(&entry_ref);
            if entry.trust.is_trusted() {
                return Ok(unchanged(&entry));
            }
        }
        let (choice, listed, path) = {
            let _dialog = self.dialog_gate.enter()?;
            let (prompt, listed, path) = {
                let mut entry = lock(&entry_ref);
                if entry.trust.is_trusted() {
                    return Ok(unchanged(&entry));
                }
                let now = Instant::now();
                if entry
                    .last_trust_grant
                    .is_some_and(|last| now.saturating_duration_since(last) < TRUST_GRANT_INTERVAL)
                {
                    return Err(IpcError::RateLimited);
                }
                entry.last_trust_grant = Some(now);
                (
                    trust_prompt(&entry),
                    std::sync::Arc::clone(&entry.latest_document),
                    entry.path.clone(),
                )
            };
            (self.dialogs.confirm_trust(&prompt), listed, path)
        };
        tracing::info!(choice = ?choice, "trust dialog answered");
        // The project may have been closed while the dialog was open.
        let entry_ref = self.projects.get(&request.handle)?;
        let mut entry = lock(&entry_ref);
        let trust = match (choice, path.as_deref()) {
            (TrustChoice::StayRestricted, _) => entry.trust,
            (TrustChoice::TrustProject | TrustChoice::TrustFolder, None) => HandleTrust::CreatedHere,
            (TrustChoice::TrustProject, Some(path)) => {
                self.trust
                    .grant_project(&identity(&listed, path))
                    .map_err(|error| store_error("trust a project", &error))?;
                HandleTrust::Trusted(TrustSource::Project)
            }
            (TrustChoice::TrustFolder, Some(path)) => {
                let folder = path.parent().ok_or(IpcError::Internal)?;
                self.trust
                    .grant_folder(folder)
                    .map_err(|error| store_error("trust a folder", &error))?;
                HandleTrust::Trusted(TrustSource::Folder)
            }
        };
        // Only a project still bound to the file the user saw is changed.
        if entry.path == path {
            entry.trust = trust;
            entry.created_here |= trust == HandleTrust::CreatedHere;
        }
        Ok(TrustResponse {
            trust: entry.trust.to_dto(entry.mark_of_the_web),
        })
    }

    /// `trust_revoke`: removes the project's own trust record
    /// (`docs/spec/08-security.md` §8.3.1). A project in a trusted folder stays
    /// trusted, and the response says that the trust comes from the folder. A
    /// project created here that was never saved goes to Restricted Mode.
    /// Revoking never makes a restricted project trusted.
    ///
    /// # Errors
    /// [`IpcError::UnknownHandle`]; [`IpcError::Io`] or [`IpcError::Internal`]
    /// when the trust store cannot be written (nothing changes then).
    pub fn trust_revoke(&self, request: TrustRevokeRequest) -> Result<TrustResponse, IpcError> {
        let _span = command_span("trust_revoke");
        let entry_ref = self.projects.get(&request.handle)?;
        let mut entry = lock(&entry_ref);
        let trust = match entry.path.clone() {
            None => HandleTrust::Restricted(RestrictedReason::NoRecord),
            Some(path) => {
                let id = &entry.latest_document.project.id;
                self.trust
                    .revoke_project(id, &path)
                    .map_err(|error| store_error("revoke trust", &error))?;
                match entry.trust {
                    HandleTrust::Restricted(reason) => HandleTrust::Restricted(reason),
                    _ if self.trust.folder_covering(&path).is_some() => {
                        HandleTrust::Trusted(TrustSource::Folder)
                    }
                    _ => HandleTrust::Restricted(RestrictedReason::NoRecord),
                }
            }
        };
        entry.trust = trust;
        entry.created_here = false;
        Ok(TrustResponse {
            trust: trust.to_dto(entry.mark_of_the_web),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dto_forms() {
        let created = HandleTrust::CreatedHere.to_dto(false);
        assert_eq!(created.state, TrustState::Trusted);
        assert_eq!(created.source, Some(DtoSource::CreatedHere));
        assert_eq!(created.restricted_reason, None);
        let folder = HandleTrust::Trusted(TrustSource::Folder).to_dto(true);
        assert_eq!(folder.source, Some(DtoSource::Folder));
        assert!(folder.mark_of_the_web);
        let project = HandleTrust::Trusted(TrustSource::Project).to_dto(false);
        assert_eq!(project.source, Some(DtoSource::Project));
        let changed = HandleTrust::Restricted(RestrictedReason::ChangedOutside).to_dto(false);
        assert_eq!(changed.state, TrustState::Restricted);
        assert_eq!(changed.source, None);
        assert_eq!(changed.restricted_reason, Some(DtoReason::ChangedOutside));
        let none = HandleTrust::Restricted(RestrictedReason::NoRecord).to_dto(false);
        assert_eq!(none.restricted_reason, Some(DtoReason::NoRecord));
    }

    #[test]
    fn only_restricted_is_untrusted() {
        assert!(HandleTrust::CreatedHere.is_trusted());
        assert!(HandleTrust::Trusted(TrustSource::Project).is_trusted());
        assert!(HandleTrust::Trusted(TrustSource::Folder).is_trusted());
        assert!(!HandleTrust::Restricted(RestrictedReason::NoRecord).is_trusted());
        assert!(!HandleTrust::Restricted(RestrictedReason::ChangedOutside).is_trusted());
        assert_eq!(
            HandleTrust::from_verdict(TrustVerdict::Trusted(TrustSource::Folder)),
            HandleTrust::Trusted(TrustSource::Folder)
        );
        assert_eq!(
            HandleTrust::from_verdict(TrustVerdict::Restricted(RestrictedReason::ChangedOutside)),
            HandleTrust::Restricted(RestrictedReason::ChangedOutside)
        );
    }
}
