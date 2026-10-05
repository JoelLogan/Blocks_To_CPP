//! Autosave and crash recovery: `recovery_save`, `recovery_list`,
//! `recovery_restore` and `recovery_discard` (`docs/spec/05-project-format.md`
//! §5.10, `docs/spec/08-security.md` §8.3.1 "Restored snapshots").
//!
//! STUB until the package `w4-recovery-watcher`: every command returns
//! [`IpcError::Internal`] without changing anything, and
//! [`Backend::discard_snapshot_of`] does nothing. The backend already calls
//! `discard_snapshot_of` wherever a snapshot must go (a save, *Save as*,
//! closing a project, and shutdown for projects without unsaved changes), so
//! the real store (`b2c_store::recovery`) only has to be wired in here.

use b2c_ipc::dto::{
    Empty, RecoveryDiscardRequest, RecoveryListResponse, RecoveryRestoreRequest, RecoveryRestoreResponse,
    RecoverySaveRequest,
};
use b2c_ipc::{Handle, IpcError};

use crate::backend::{Backend, command_span};

// Commands take their request by value, as the adapter decodes it, so every
// command method has the same shape whether or not it keeps the request.
#[allow(clippy::needless_pass_by_value)]
impl Backend {
    /// `recovery_save`: writes the project's recovery snapshot.
    ///
    /// # Errors
    /// Always [`IpcError::Internal`] until `w4-recovery-watcher`.
    pub fn recovery_save(&self, request: RecoverySaveRequest) -> Result<Empty, IpcError> {
        let _span = command_span("recovery_save");
        let _ = request;
        Err(IpcError::Internal)
    }

    /// `recovery_list`: the snapshots of app instances that are no longer
    /// running.
    ///
    /// # Errors
    /// Always [`IpcError::Internal`] until `w4-recovery-watcher`.
    pub fn recovery_list(&self) -> Result<RecoveryListResponse, IpcError> {
        let _span = command_span("recovery_list");
        Err(IpcError::Internal)
    }

    /// `recovery_restore`: opens a snapshot as a project.
    ///
    /// # Errors
    /// Always [`IpcError::Internal`] until `w4-recovery-watcher`.
    pub fn recovery_restore(
        &self,
        request: RecoveryRestoreRequest,
    ) -> Result<RecoveryRestoreResponse, IpcError> {
        let _span = command_span("recovery_restore");
        let _ = request;
        Err(IpcError::Internal)
    }

    /// `recovery_discard`: deletes a snapshot.
    ///
    /// # Errors
    /// Always [`IpcError::Internal`] until `w4-recovery-watcher`.
    pub fn recovery_discard(&self, request: RecoveryDiscardRequest) -> Result<Empty, IpcError> {
        let _span = command_span("recovery_discard");
        let _ = request;
        Err(IpcError::Internal)
    }

    /// Deletes the recovery snapshot of `handle`, if it has one (a clean save
    /// or close). A no-op until `w4-recovery-watcher`.
    #[allow(clippy::unused_self)] // A stub until w4-recovery-watcher.
    pub(crate) fn discard_snapshot_of(&self, handle: &Handle) {
        let _ = handle;
    }
}
