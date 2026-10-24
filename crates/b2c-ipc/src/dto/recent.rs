//! The recent-projects list: `recent_list` and `recent_remove`.

use serde::{Deserialize, Serialize};

use crate::dto::id_request;
use crate::ids::RecentId;

/// One entry of the recent-projects list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RecentEntry {
    /// The entry's ID (`project_open_recent`, `recent_remove`).
    pub recent_id: RecentId,
    /// The project's name.
    pub project_name: String,
    /// Where the file is, for display only; it is never accepted back.
    pub display_path: String,
    /// When it was last opened: an RFC 3339 UTC timestamp.
    pub last_opened_at: String,
}

/// The response of `recent_list`: newest first, at most
/// [`RECENT_MAX`](crate::limits::RECENT_MAX) entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RecentListResponse {
    /// The entries.
    pub entries: Vec<RecentEntry>,
}

id_request!(
    /// The request of `recent_remove`.
    RecentRemoveRequest, command = "recent_remove",
    /// The entry to remove.
    recent_id: RecentId = "recentId"
);
