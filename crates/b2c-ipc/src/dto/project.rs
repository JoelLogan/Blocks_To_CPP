//! Project commands: `project_new`, `project_open_dialog`, `project_open_recent`,
//! `project_reload`, `project_save`, `project_save_as_dialog`, `project_close` and
//! `project_set_dirty`.
//!
//! The webview never names a file. Opening and saving under a new name go through
//! native dialogs raised by the backend; everything else names the project by its
//! [`Handle`]. A dialog the user cancels is a `{ "status": "cancelled" }` result,
//! not an error.

use serde::{Deserialize, Serialize};

use crate::commands::IpcRequest;
use crate::dto::{DOCUMENT, SAMPLE_DOCUMENT, Trust, check_document, id_request};
use crate::error::IpcError;
use crate::ids::{Handle, RecentId};
use crate::macros::string_enum;
use crate::schema::{FieldSchema, FieldSpec, ObjectSchema};

string_enum! {
    /// The bundled templates a new project can start from.
    pub enum Template {
        /// One module `main` with an empty program.
        Empty = "empty",
        /// The Hello World example.
        HelloWorld = "helloWorld",
    }
}

/// The request of `project_new`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProjectNewRequest {
    /// The template to start from.
    pub template: Template,
}

impl IpcRequest for ProjectNewRequest {
    const COMMAND: &'static str = "project_new";

    fn schema() -> &'static ObjectSchema {
        static SCHEMA: ObjectSchema = ObjectSchema {
            fields: &[FieldSpec::required(
                "template",
                FieldSchema::Enum {
                    values: Template::VALUES,
                },
            )],
        };
        &SCHEMA
    }

    fn sample() -> Self {
        Self {
            template: Template::HelloWorld,
        }
    }

    fn validate(&self) -> Result<(), IpcError> {
        Ok(())
    }
}

/// The response of `project_new`: a new, unsaved project, trusted because it was
/// created here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ProjectNewResponse {
    /// The new project's handle.
    pub handle: Handle,
    /// The project as BDM JSON text.
    pub document: String,
    /// Its trust state.
    pub trust: Trust,
}

/// A project that was opened from a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ProjectOpened {
    /// The project's handle.
    pub handle: Handle,
    /// The project as BDM JSON text (migrated to the current format when it was
    /// older).
    pub document: String,
    /// Its trust state.
    pub trust: Trust,
    /// The file's name, without its folder, for display.
    pub file_name: String,
    /// The format version the file had, when it was migrated in memory; `null`
    /// when it is current.
    pub migrated_from: Option<u32>,
}

/// The response of `project_open_dialog`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ProjectOpenDialogResponse {
    /// The user cancelled the dialog.
    Cancelled,
    /// The project was opened.
    Ok(ProjectOpened),
}

id_request!(
    /// The request of `project_open_recent`.
    ProjectOpenRecentRequest, command = "project_open_recent",
    /// The entry of the recent-projects list.
    recent_id: RecentId = "recentId"
);

id_request!(
    /// The request of `project_reload`: read the project's file again, after it
    /// changed outside the app.
    ProjectReloadRequest, command = "project_reload",
    /// The project.
    handle: Handle = "handle"
);

/// The response of `project_reload`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ProjectReloadResponse {
    /// The project as BDM JSON text.
    pub document: String,
    /// Its trust state, checked again.
    pub trust: Trust,
    /// The format version the file had, when it was migrated in memory.
    pub migrated_from: Option<u32>,
}

/// Defines a request with a handle and a document.
macro_rules! document_request {
    ($(#[$meta:meta])* $name:ident, command = $command:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[cfg_attr(feature = "ts", derive(ts_rs::TS))]
        #[serde(deny_unknown_fields, rename_all = "camelCase")]
        pub struct $name {
            /// The project.
            pub handle: Handle,
            /// The project as BDM JSON text.
            pub document: String,
        }

        impl IpcRequest for $name {
            const COMMAND: &'static str = $command;

            fn schema() -> &'static ObjectSchema {
                static SCHEMA: ObjectSchema = ObjectSchema {
                    fields: &[
                        FieldSpec::required("handle", Handle::SCHEMA),
                        FieldSpec::required("document", DOCUMENT),
                    ],
                };
                &SCHEMA
            }

            fn sample() -> Self {
                Self {
                    handle: Handle::example(),
                    document: SAMPLE_DOCUMENT.to_owned(),
                }
            }

            fn validate(&self) -> Result<(), IpcError> {
                check_document(&self.document)
            }
        }
    };
}

pub(crate) use document_request;

document_request!(
    /// The request of `project_save`: write the document to the file bound to the
    /// handle (atomically; never over a file that changed on disk).
    ProjectSaveRequest, command = "project_save"
);

/// The response of `project_save`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ProjectSaveResponse {
    /// When it was saved: an RFC 3339 UTC timestamp.
    pub saved_at: String,
    /// The lower-case hex SHA-256 of the bytes written.
    pub hash: String,
}

document_request!(
    /// The request of `project_save_as_dialog`: ask for a new file with a native
    /// dialog, write the document there and rebind the handle to it.
    ProjectSaveAsDialogRequest, command = "project_save_as_dialog"
);

/// A project saved under a new name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ProjectSavedAs {
    /// The project's handle, now bound to the new file.
    pub handle: Handle,
    /// When it was saved: an RFC 3339 UTC timestamp.
    pub saved_at: String,
    /// The lower-case hex SHA-256 of the bytes written.
    pub hash: String,
    /// The new file's name, without its folder, for display.
    pub file_name: String,
}

/// The response of `project_save_as_dialog`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ProjectSaveAsDialogResponse {
    /// The user cancelled the dialog; nothing was written.
    Cancelled,
    /// The project was saved.
    Ok(ProjectSavedAs),
}

id_request!(
    /// The request of `project_close`.
    ProjectCloseRequest, command = "project_close",
    /// The project.
    handle: Handle = "handle"
);

/// The request of `project_set_dirty`: whether the project has unsaved changes,
/// so the backend can ask before the window closes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProjectSetDirtyRequest {
    /// The project.
    pub handle: Handle,
    /// Whether it has unsaved changes.
    pub dirty: bool,
}

impl IpcRequest for ProjectSetDirtyRequest {
    const COMMAND: &'static str = "project_set_dirty";

    fn schema() -> &'static ObjectSchema {
        static SCHEMA: ObjectSchema = ObjectSchema {
            fields: &[
                FieldSpec::required("handle", Handle::SCHEMA),
                FieldSpec::required("dirty", FieldSchema::Bool),
            ],
        };
        &SCHEMA
    }

    fn sample() -> Self {
        Self {
            handle: Handle::example(),
            dirty: true,
        }
    }

    fn validate(&self) -> Result<(), IpcError> {
        Ok(())
    }
}
