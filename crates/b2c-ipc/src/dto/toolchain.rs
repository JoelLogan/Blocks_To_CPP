//! Toolchains: `toolchain_list`, `toolchain_rescan`, `toolchain_add_dialog`,
//! `toolchain_select` and `toolchain_setup_info`.
//!
//! [`Toolchain`] is a dedicated IPC type, separate from the toolchain cache's
//! storage type: it carries what the setup page, the settings page and the status
//! bar show (`docs/spec/04-user-interface.md` §4.1 and §4.6).

use serde::{Deserialize, Serialize};

use crate::diag::Diagnostic;
use crate::dto::{Platform, id_request};
use crate::ids::ToolchainId;
use crate::macros::string_enum;

string_enum! {
    /// How a toolchain was found. `toolchains.json` stores the same value with
    /// each cached toolchain (`docs/spec/05-project-format.md` §5.9), so a
    /// cached entry reports the source that discovery gives it.
    pub enum ToolchainSource {
        /// On `PATH`.
        Path = "path",
        /// In a well-known install location (for example `C:\msys64\ucrt64\bin`).
        WellKnown = "wellKnown",
        /// Added by the user with `toolchain_add_dialog`.
        Manual = "manual",
    }
}

string_enum! {
    /// A C++ language standard.
    pub enum CppStandard {
        /// C++17.
        Cpp17 = "c++17",
        /// C++20.
        Cpp20 = "c++20",
        /// C++23.
        Cpp23 = "c++23",
        /// C++26.
        Cpp26 = "c++26",
    }
}

/// What a toolchain supports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ToolchainCapabilities {
    /// The C++ standards it accepts.
    pub standards: Vec<CppStandard>,
    /// Whether `std::format` works.
    pub std_format: bool,
    /// Whether the address and undefined-behaviour sanitizers work.
    pub sanitizers: bool,
    /// Whether it reports diagnostics as SARIF.
    pub sarif: bool,
}

/// A compiler toolchain the backend found or the user added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Toolchain {
    /// Its ID (`toolchain_select`).
    pub id: ToolchainId,
    /// The g++ version, for example `13.3.0`, when the probe found one.
    pub version: Option<String>,
    /// The target triple, for example `x86_64-linux-gnu`.
    pub target: Option<String>,
    /// The distribution flavour, for example `MSYS2 UCRT64`.
    pub flavor: Option<String>,
    /// Where it is, for display only (the status bar and the toolchain page); it
    /// is never accepted back. It is the path the compiler was found as, before
    /// links were resolved (for example `/usr/bin/g++`, or the file the user
    /// picked), which `toolchains.json` keeps as `foundAs` so that a cached entry
    /// shows the same path as a discovered one.
    pub display_path: String,
    /// How it was found.
    pub source: ToolchainSource,
    /// Whether it passed every health check and can build.
    pub usable: bool,
    /// Whether it is the selected toolchain.
    pub selected: bool,
    /// What it supports.
    pub capabilities: ToolchainCapabilities,
    /// The failed health checks, as `B2C-T1xxx` diagnostics.
    pub problems: Vec<Diagnostic>,
}

/// The response of `toolchain_list` and `toolchain_rescan`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ToolchainListResponse {
    /// The toolchains, in discovery order.
    pub toolchains: Vec<Toolchain>,
    /// Whether background discovery is still running; a `toolchainsUpdated` app
    /// event follows when it finishes.
    pub discovering: bool,
}

/// The response of `toolchain_add_dialog`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "status", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ToolchainAddDialogResponse {
    /// The user cancelled the dialog.
    Cancelled,
    /// The chosen compiler was probed and added, even when it is not usable. A
    /// file that is not an acceptable g++ or cannot be probed is not added: the
    /// command fails with
    /// [`IpcError::ToolchainRejected`](crate::IpcError::ToolchainRejected)
    /// instead.
    Ok {
        /// The added toolchain; when `usable` is false, `problems` says why.
        toolchain: Toolchain,
    },
}

id_request!(
    /// The request of `toolchain_select`: make a toolchain the default.
    ToolchainSelectRequest, command = "toolchain_select",
    /// The toolchain.
    toolchain_id: ToolchainId = "toolchainId"
);

/// A Linux distribution, from `/etc/os-release`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Distro {
    /// `ID`, for example `ubuntu`.
    pub id: String,
    /// `ID_LIKE`, split at spaces, for example `["debian"]`.
    pub id_like: Vec<String>,
}

/// The response of `toolchain_setup_info`: what the setup page needs to suggest
/// an install command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ToolchainSetupInfo {
    /// The operating system.
    pub platform: Platform,
    /// Whether no usable toolchain was found.
    pub no_usable_toolchain: bool,
    /// The Linux distribution, when it could be read; `null` on Windows.
    pub distro: Option<Distro>,
}
