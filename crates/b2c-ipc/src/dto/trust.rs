//! Workspace trust (`docs/spec/08-security.md` §8.3): `trust_get`, `trust_grant`
//! and `trust_revoke`.
//!
//! `trust_grant` shows a native confirmation dialog from the backend, so the
//! webview cannot grant trust silently. When the user stays in Restricted Mode or
//! cancels, it returns the unchanged trust.

use serde::{Deserialize, Serialize};

use crate::dto::id_request;
use crate::ids::Handle;
use crate::macros::string_enum;

string_enum! {
    /// Whether a project may be built and run.
    pub enum TrustState {
        /// Trusted: building and running are allowed.
        Trusted = "trusted",
        /// Restricted Mode: the project can be edited but not built or run.
        Restricted = "restricted",
    }
}

string_enum! {
    /// Why a project is trusted.
    pub enum TrustSource {
        /// It was created in this app and has not been opened from elsewhere.
        CreatedHere = "createdHere",
        /// The user trusted this project file.
        Project = "project",
        /// The user trusted a folder that contains it.
        Folder = "folder",
    }
}

string_enum! {
    /// Why a project is in Restricted Mode.
    pub enum RestrictedReason {
        /// It was never trusted on this machine.
        NoRecord = "noRecord",
        /// It was trusted, but its trust-relevant content (Raw C++, libraries,
        /// packs, defines) changed outside the app.
        ChangedOutside = "changedOutside",
    }
}

/// A project's trust state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Trust {
    /// Trusted or restricted.
    pub state: TrustState,
    /// Why it is trusted; `null` when restricted.
    pub source: Option<TrustSource>,
    /// Why it is restricted; `null` when trusted.
    pub restricted_reason: Option<RestrictedReason>,
    /// Whether the file carries the Windows Mark of the Web (downloaded from the
    /// Internet), which makes the trust dialog warn more strongly.
    pub mark_of_the_web: bool,
}

id_request!(
    /// The request of `trust_get`.
    TrustGetRequest, command = "trust_get",
    /// The project.
    handle: Handle = "handle"
);

id_request!(
    /// The request of `trust_grant`: ask the user, in a native dialog, whether to
    /// trust the project or its folder.
    TrustGrantRequest, command = "trust_grant",
    /// The project.
    handle: Handle = "handle"
);

id_request!(
    /// The request of `trust_revoke`: forget the project's trust record.
    TrustRevokeRequest, command = "trust_revoke",
    /// The project.
    handle: Handle = "handle"
);

/// The response of `trust_get`, `trust_grant` and `trust_revoke`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct TrustResponse {
    /// The project's trust state after the call.
    pub trust: Trust,
}
