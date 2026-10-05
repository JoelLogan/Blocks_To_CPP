//! App-level commands: `app_info`, `app_subscribe` and `app_quit`.

use serde::{Deserialize, Serialize};

use crate::dto::{SettingsNotice, Toolchain};
use crate::ids::Handle;
use crate::macros::string_enum;

string_enum! {
    /// The operating system the backend runs on.
    pub enum Platform {
        /// Windows 10 or 11.
        Windows = "windows",
        /// Linux.
        Linux = "linux",
    }
}

impl Platform {
    /// The platform this binary was built for (Linux for every non-Windows target).
    pub const fn current() -> Self {
        if cfg!(windows) { Self::Windows } else { Self::Linux }
    }
}

/// The response of `app_info`. The frontend compares `ipcVersion` with the
/// version it was generated for and shows a blocking error on a mismatch
/// (`docs/spec/09-quality-and-delivery.md` §9.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// The app's version (`SemVer`).
    pub app_version: String,
    /// The IPC contract version, [`IPC_VERSION`](crate::IPC_VERSION).
    pub ipc_version: u32,
    /// The operating system.
    pub platform: Platform,
    /// The version of the block catalog the backend builds with.
    pub catalog_version: String,
}

/// The empty response `{}` of commands that return nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(type = "Record<string, never>"))]
#[serde(deny_unknown_fields)]
pub struct Empty {}

/// A notification that belongs to no command, pushed on the channel given to
/// `app_subscribe` (a later subscription replaces the earlier channel).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AppEvent {
    /// The file of an open project changed or disappeared outside the app.
    ProjectChangedOnDisk {
        /// The project.
        handle: Handle,
        /// Whether the file was deleted or renamed away (Reload is then
        /// unavailable).
        deleted: bool,
    },
    /// Background toolchain discovery produced a new list.
    ToolchainsUpdated {
        /// The toolchain list.
        toolchains: Vec<Toolchain>,
        /// Whether discovery is still running.
        discovering: bool,
    },
    /// Settings were reset or read partially when they were loaded.
    SettingsNotice {
        /// What happened to which keys.
        notices: Vec<SettingsNotice>,
    },
    /// The user asked to close the window while a project has unsaved changes. The
    /// frontend asks Save / Don't save / Cancel and then calls `app_quit`.
    CloseRequested,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_an_empty_object() {
        assert_eq!(serde_json::to_string(&Empty {}).unwrap(), "{}");
        assert_eq!(serde_json::from_str::<Empty>("{}").unwrap(), Empty {});
        assert!(serde_json::from_str::<Empty>(r#"{"a": 1}"#).is_err());
    }

    #[test]
    fn platform_is_the_build_target() {
        assert_eq!(Platform::current() == Platform::Windows, cfg!(windows));
    }
}
