//! Autosave and crash recovery: `recovery_save`, `recovery_list`,
//! `recovery_restore` and `recovery_discard`.

use serde::{Deserialize, Serialize};

use crate::dto::project::document_request;
use crate::dto::{DOCUMENT, SAMPLE_DOCUMENT, Trust, check_document, id_request};
use crate::error::IpcError;
use crate::ids::{Handle, SnapshotId};
use crate::schema::{FieldSpec, ObjectSchema};

use crate::commands::IpcRequest;

document_request!(
    /// The request of `recovery_save`: store a recovery snapshot of the project's
    /// current, possibly unsaved, document.
    RecoverySaveRequest, command = "recovery_save"
);

/// A recovery snapshot that can be restored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInfo {
    /// The snapshot's ID.
    pub snapshot_id: SnapshotId,
    /// The project's name.
    pub project_name: String,
    /// When the snapshot was written: an RFC 3339 UTC timestamp.
    pub saved_at: String,
    /// Whether the project had been saved to a file.
    pub has_path: bool,
}

/// The response of `recovery_list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RecoveryListResponse {
    /// The snapshots of app instances that are no longer running.
    pub snapshots: Vec<SnapshotInfo>,
}

id_request!(
    /// The request of `recovery_restore`.
    RecoveryRestoreRequest, command = "recovery_restore",
    /// The snapshot to restore.
    snapshot_id: SnapshotId = "snapshotId"
);

/// The response of `recovery_restore`: the snapshot as an open project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RecoveryRestoreResponse {
    /// The restored project's handle.
    pub handle: Handle,
    /// The project as BDM JSON text.
    pub document: String,
    /// Its trust state.
    pub trust: Trust,
    /// The file's name, when the project had been saved; `null` otherwise.
    pub file_name: Option<String>,
}

id_request!(
    /// The request of `recovery_discard`.
    RecoveryDiscardRequest, command = "recovery_discard",
    /// The snapshot to delete.
    snapshot_id: SnapshotId = "snapshotId"
);
