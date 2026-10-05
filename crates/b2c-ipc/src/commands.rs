//! The command table: every IPC command of milestone M2, with its request schema,
//! its channels and its response type (`docs/spec/02-architecture.md` §2.5).
//!
//! The table is the single source for everything that must list the commands in
//! step: the generated TypeScript client (`packages/ipc-types`), the isolation
//! allowlist (`apps/desktop/src-tauri/isolation/allowlist.generated.js`), and the
//! desktop adapter's handler list, permissions and capability, which a test there
//! compares with [`COMMAND_NAMES`].
//!
//! **Invocation shape.** A command with arguments takes exactly one top-level key
//! `request` (a JSON object) plus its named channel keys (`onEvent`, `onOutput`).
//! A command without arguments takes `{}`. Handlers decode `request` with
//! [`crate::decode::decode`].

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::dto;
use crate::error::IpcError;
use crate::schema::ObjectSchema;

/// A request type: the `request` argument of one command.
pub trait IpcRequest: Serialize + DeserializeOwned {
    /// The command that takes this request.
    const COMMAND: &'static str;

    /// The exact shape of the request's JSON.
    fn schema() -> &'static ObjectSchema;

    /// A fixed, valid request, used for the isolation samples and the
    /// schema-versus-serde test.
    fn sample() -> Self;

    /// Checks what the schema cannot: values that need the typed request, such as
    /// the decoded size of base64 text. [`crate::decode::decode`] calls it after
    /// the schema check and serde.
    ///
    /// # Errors
    /// Returns [`IpcError::InvalidRequest`] or [`IpcError::PayloadTooLarge`].
    fn validate(&self) -> Result<(), IpcError>;
}

/// A channel argument of a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelSpec {
    /// The argument key, for example `onEvent`.
    pub name: &'static str,
    /// Whether the channel carries raw bytes (`InvokeResponseBody::Raw`) instead
    /// of JSON messages.
    pub raw: bool,
    /// The TypeScript type of one message, for example `BuildEvent` or
    /// `ArrayBuffer`.
    pub ts_type: &'static str,
}

/// One command of the table.
#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    /// The command name, as registered with Tauri.
    pub name: &'static str,
    /// The schema of the `request` argument, or `None` for a command without
    /// arguments.
    pub request: Option<fn() -> &'static ObjectSchema>,
    /// The TypeScript type of the `request` argument, when there is one.
    pub request_ts: Option<&'static str>,
    /// A valid `request` value, when there is one ([`IpcRequest::sample`] as JSON).
    pub sample: Option<fn() -> Value>,
    /// The channel arguments, in parameter order.
    pub channels: &'static [ChannelSpec],
    /// The TypeScript type of the response.
    pub response_ts: &'static str,
}

/// The JSON of `T`'s sample request.
fn sample_json<T: IpcRequest>() -> Value {
    serde_json::to_value(T::sample()).unwrap_or(Value::Null)
}

/// A command without arguments.
const fn plain(
    name: &'static str,
    channels: &'static [ChannelSpec],
    response_ts: &'static str,
) -> CommandSpec {
    CommandSpec {
        name,
        request: None,
        request_ts: None,
        sample: None,
        channels,
        response_ts,
    }
}

/// A command whose `request` argument is a `T`.
const fn with_request<T: IpcRequest>(
    request_ts: &'static str,
    channels: &'static [ChannelSpec],
    response_ts: &'static str,
) -> CommandSpec {
    CommandSpec {
        name: T::COMMAND,
        request: Some(T::schema),
        request_ts: Some(request_ts),
        sample: Some(sample_json::<T>),
        channels,
        response_ts,
    }
}

const fn json_channel(name: &'static str, ts_type: &'static str) -> ChannelSpec {
    ChannelSpec {
        name,
        raw: false,
        ts_type,
    }
}

const NO_CHANNELS: &[ChannelSpec] = &[];
const APP_EVENTS: &[ChannelSpec] = &[json_channel("onEvent", "AppEvent")];
const BUILD_EVENTS: &[ChannelSpec] = &[json_channel("onEvent", "BuildEvent")];
const RUN_CHANNELS: &[ChannelSpec] = &[
    ChannelSpec {
        name: "onOutput",
        raw: true,
        ts_type: "ArrayBuffer",
    },
    json_channel("onEvent", "RunEvent"),
];

/// Every command, in a fixed order (by area: app, project, recent, recovery,
/// trust, toolchain, build, run, settings, help).
pub const COMMANDS: &[CommandSpec] = &[
    plain("app_info", NO_CHANNELS, "AppInfo"),
    plain("app_subscribe", APP_EVENTS, "Empty"),
    plain("app_quit", NO_CHANNELS, "Empty"),
    with_request::<dto::ProjectNewRequest>("ProjectNewRequest", NO_CHANNELS, "ProjectNewResponse"),
    plain("project_open_dialog", NO_CHANNELS, "ProjectOpenDialogResponse"),
    with_request::<dto::ProjectOpenRecentRequest>("ProjectOpenRecentRequest", NO_CHANNELS, "ProjectOpened"),
    with_request::<dto::ProjectReloadRequest>("ProjectReloadRequest", NO_CHANNELS, "ProjectReloadResponse"),
    with_request::<dto::ProjectSaveRequest>("ProjectSaveRequest", NO_CHANNELS, "ProjectSaveResponse"),
    with_request::<dto::ProjectSaveAsDialogRequest>(
        "ProjectSaveAsDialogRequest",
        NO_CHANNELS,
        "ProjectSaveAsDialogResponse",
    ),
    with_request::<dto::ProjectCloseRequest>("ProjectCloseRequest", NO_CHANNELS, "Empty"),
    with_request::<dto::ProjectSetDirtyRequest>("ProjectSetDirtyRequest", NO_CHANNELS, "Empty"),
    plain("recent_list", NO_CHANNELS, "RecentListResponse"),
    with_request::<dto::RecentRemoveRequest>("RecentRemoveRequest", NO_CHANNELS, "Empty"),
    with_request::<dto::RecoverySaveRequest>("RecoverySaveRequest", NO_CHANNELS, "Empty"),
    plain("recovery_list", NO_CHANNELS, "RecoveryListResponse"),
    with_request::<dto::RecoveryRestoreRequest>(
        "RecoveryRestoreRequest",
        NO_CHANNELS,
        "RecoveryRestoreResponse",
    ),
    with_request::<dto::RecoveryDiscardRequest>("RecoveryDiscardRequest", NO_CHANNELS, "Empty"),
    with_request::<dto::TrustGetRequest>("TrustGetRequest", NO_CHANNELS, "TrustResponse"),
    with_request::<dto::TrustGrantRequest>("TrustGrantRequest", NO_CHANNELS, "TrustResponse"),
    with_request::<dto::TrustRevokeRequest>("TrustRevokeRequest", NO_CHANNELS, "TrustResponse"),
    plain("toolchain_list", NO_CHANNELS, "ToolchainListResponse"),
    plain("toolchain_rescan", NO_CHANNELS, "ToolchainListResponse"),
    plain("toolchain_add_dialog", NO_CHANNELS, "ToolchainAddDialogResponse"),
    with_request::<dto::ToolchainSelectRequest>("ToolchainSelectRequest", NO_CHANNELS, "Empty"),
    plain("toolchain_setup_info", NO_CHANNELS, "ToolchainSetupInfo"),
    with_request::<dto::BuildStartRequest>("BuildStartRequest", BUILD_EVENTS, "BuildStartResponse"),
    with_request::<dto::BuildCancelRequest>("BuildCancelRequest", NO_CHANNELS, "Empty"),
    plain("build_cache_clear", NO_CHANNELS, "BuildCacheClearResponse"),
    with_request::<dto::RunStartRequest>("RunStartRequest", RUN_CHANNELS, "RunStartResponse"),
    with_request::<dto::RunInputRequest>("RunInputRequest", NO_CHANNELS, "Empty"),
    with_request::<dto::RunResizeRequest>("RunResizeRequest", NO_CHANNELS, "Empty"),
    with_request::<dto::RunStopRequest>("RunStopRequest", NO_CHANNELS, "Empty"),
    with_request::<dto::RunAckRequest>("RunAckRequest", NO_CHANNELS, "Empty"),
    plain("settings_get", NO_CHANNELS, "SettingsGetResponse"),
    with_request::<dto::SettingsPatch>("SettingsPatch", NO_CHANNELS, "SettingsUpdateResponse"),
    with_request::<dto::OpenHelpLinkRequest>("OpenHelpLinkRequest", NO_CHANNELS, "Empty"),
];

/// The names of [`COMMANDS`], in the same order.
pub const COMMAND_NAMES: &[&str] = &[
    "app_info",
    "app_subscribe",
    "app_quit",
    "project_new",
    "project_open_dialog",
    "project_open_recent",
    "project_reload",
    "project_save",
    "project_save_as_dialog",
    "project_close",
    "project_set_dirty",
    "recent_list",
    "recent_remove",
    "recovery_save",
    "recovery_list",
    "recovery_restore",
    "recovery_discard",
    "trust_get",
    "trust_grant",
    "trust_revoke",
    "toolchain_list",
    "toolchain_rescan",
    "toolchain_add_dialog",
    "toolchain_select",
    "toolchain_setup_info",
    "build_start",
    "build_cancel",
    "build_cache_clear",
    "run_start",
    "run_input",
    "run_resize",
    "run_stop",
    "run_ack",
    "settings_get",
    "settings_update",
    "open_help_link",
];

/// The table entry of the command `name`, if it exists.
pub fn command(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|c| c.name == name)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn names_are_unique_and_in_table_order() {
        assert_eq!(COMMANDS.len(), 36);
        let names: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
        assert_eq!(names, COMMAND_NAMES);
        let unique: HashSet<&str> = names.iter().copied().collect();
        assert_eq!(unique.len(), names.len());
        for name in COMMAND_NAMES {
            assert!(
                name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
                "{name}"
            );
            assert_eq!(command(name).map(|c| c.name), Some(*name));
        }
        assert!(command("app_version").is_none());
        assert!(command("project_export_dialog").is_none());
    }

    #[test]
    fn requests_are_complete() {
        for spec in COMMANDS {
            assert_eq!(spec.request.is_some(), spec.request_ts.is_some(), "{}", spec.name);
            assert_eq!(spec.request.is_some(), spec.sample.is_some(), "{}", spec.name);
            if let Some(sample) = spec.sample {
                assert!(sample().is_object(), "{}", spec.name);
            }
            let channels: HashSet<&str> = spec.channels.iter().map(|c| c.name).collect();
            assert_eq!(channels.len(), spec.channels.len(), "{}", spec.name);
            assert!(!channels.contains("request"), "{}", spec.name);
        }
    }
}
