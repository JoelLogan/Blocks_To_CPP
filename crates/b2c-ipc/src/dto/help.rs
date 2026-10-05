//! Help links: `open_help_link`.

use serde::{Deserialize, Serialize};

use crate::commands::IpcRequest;
use crate::error::IpcError;
use crate::links::LinkId;
use crate::schema::{FieldSchema, FieldSpec, ObjectSchema};

/// The request of `open_help_link`: open one of the fixed help pages in the OS
/// browser. The webview names the page; it never passes a URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OpenHelpLinkRequest {
    /// The page.
    pub link_id: LinkId,
}

impl IpcRequest for OpenHelpLinkRequest {
    const COMMAND: &'static str = "open_help_link";

    fn schema() -> &'static ObjectSchema {
        static SCHEMA: ObjectSchema = ObjectSchema {
            fields: &[FieldSpec::required(
                "linkId",
                FieldSchema::Enum {
                    values: LinkId::VALUES,
                },
            )],
        };
        &SCHEMA
    }

    fn sample() -> Self {
        Self {
            link_id: LinkId::DiagnosticsReference,
        }
    }

    fn validate(&self) -> Result<(), IpcError> {
        Ok(())
    }
}
