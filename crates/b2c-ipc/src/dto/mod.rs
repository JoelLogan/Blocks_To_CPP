//! The request, response and channel message types of every command.
//!
//! Conventions (`docs/spec/02-architecture.md` §2.5):
//!
//! * Every type is a dedicated IPC type with camelCase keys and camelCase enum
//!   values. Cache and storage types never cross IPC.
//! * Requests are `#[serde(deny_unknown_fields)]` at every level, carry no
//!   filesystem path (only the opaque IDs of [`crate::ids`]), and implement
//!   [`IpcRequest`](crate::commands::IpcRequest) with an exact schema.
//! * Project documents cross IPC as BDM JSON text in a `document` string, in both
//!   directions; the backend parses them with [`crate::decode::parse_document`].
//! * Tagged unions use the tag `kind` (channel events), `status` (dialog results)
//!   or `type` (exit status).
//! * Optional values are `null`, never absent, except in [`SettingsPatch`], whose
//!   keys are absent when unchanged.

mod app;
mod build;
mod help;
mod project;
mod recent;
mod recovery;
mod run;
mod settings;
mod toolchain;
mod trust;

pub use app::{AppEvent, AppInfo, Empty, Platform};
pub use build::{
    BuildCacheClearResponse, BuildCancelRequest, BuildConfig, BuildEvent, BuildOutcome, BuildStage,
    BuildStartRequest, BuildStartResponse,
};
pub use help::OpenHelpLinkRequest;
pub use project::{
    ProjectCloseRequest, ProjectNewRequest, ProjectNewResponse, ProjectOpenDialogResponse,
    ProjectOpenRecentRequest, ProjectOpened, ProjectReloadRequest, ProjectReloadResponse,
    ProjectSaveAsDialogRequest, ProjectSaveAsDialogResponse, ProjectSaveRequest, ProjectSaveResponse,
    ProjectSavedAs, ProjectSetDirtyRequest, Template,
};
pub use recent::{RecentEntry, RecentListResponse, RecentRemoveRequest};
pub use recovery::{
    RecoveryDiscardRequest, RecoveryListResponse, RecoveryRestoreRequest, RecoveryRestoreResponse,
    RecoverySaveRequest, SnapshotInfo,
};
pub use run::{
    Containment, Crash, ExitStatus, RunAckRequest, RunEvent, RunInputRequest, RunMode, RunOptions,
    RunResizeRequest, RunStartRequest, RunStartResponse, RunStopRequest, SanitizerKind, SanitizerReport,
    SanitizerTool,
};
pub use settings::{
    BuildCacheSettings, CodeStyle, CodeStylePatch, ConsolePatch, ConsoleSettings, IndentWidth,
    NewProjectSettings, NoticeReason, OnErrors, RunSettings, RunSettingsPatch, Settings, SettingsGetResponse,
    SettingsNotice, SettingsPatch, SettingsUpdateResponse, ToolchainSettings,
};
pub use toolchain::{
    CppStandard, Distro, Toolchain, ToolchainAddDialogResponse, ToolchainCapabilities, ToolchainListResponse,
    ToolchainSelectRequest, ToolchainSetupInfo, ToolchainSource,
};
pub use trust::{
    RestrictedReason, Trust, TrustGetRequest, TrustGrantRequest, TrustResponse, TrustRevokeRequest,
    TrustSource, TrustState,
};

use crate::error::IpcError;
use crate::limits::MAX_DOCUMENT_BYTES;
use crate::schema::FieldSchema;

/// The schema of a `document` field: BDM JSON text of at most
/// [`MAX_DOCUMENT_BYTES`] bytes.
pub(crate) const DOCUMENT: FieldSchema = FieldSchema::String {
    max_len: MAX_DOCUMENT_BYTES,
};

/// Rechecks the size of a `document` field (the schema checked it already).
pub(crate) fn check_document(document: &str) -> Result<(), IpcError> {
    if document.len() > MAX_DOCUMENT_BYTES {
        Err(IpcError::too_large(MAX_DOCUMENT_BYTES))
    } else {
        Ok(())
    }
}

/// The smallest valid project document, used by the request samples.
pub(crate) const SAMPLE_DOCUMENT: &str = include_str!("sample.b2c");

/// Defines a request whose only key is one opaque ID.
macro_rules! id_request {
    (
        $(#[$meta:meta])*
        $name:ident, command = $command:literal,
        $(#[$fmeta:meta])* $field:ident: $ty:ty = $json:literal
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        #[cfg_attr(feature = "ts", derive(ts_rs::TS))]
        #[serde(deny_unknown_fields, rename_all = "camelCase")]
        pub struct $name {
            $(#[$fmeta])*
            pub $field: $ty,
        }

        impl crate::commands::IpcRequest for $name {
            const COMMAND: &'static str = $command;

            fn schema() -> &'static crate::schema::ObjectSchema {
                static SCHEMA: crate::schema::ObjectSchema = crate::schema::ObjectSchema {
                    fields: &[crate::schema::FieldSpec::required($json, <$ty>::SCHEMA)],
                };
                &SCHEMA
            }

            fn sample() -> Self {
                Self {
                    $field: <$ty>::example(),
                }
            }

            fn validate(&self) -> Result<(), crate::error::IpcError> {
                Ok(())
            }
        }
    };
}

pub(crate) use id_request;
