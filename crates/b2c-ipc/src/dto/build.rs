//! Building: `build_start` with its event channel, `build_cancel` and
//! `build_cache_clear` (`docs/spec/02-architecture.md` §2.4.2).
//!
//! The backend never compiles C++ from the webview: `build_start` carries the BDM
//! document, and the backend regenerates the C++ itself.

use serde::{Deserialize, Serialize};

use crate::commands::IpcRequest;
use crate::diag::Diagnostic;
use crate::dto::{DOCUMENT, SAMPLE_DOCUMENT, check_document, id_request};
use crate::error::IpcError;
use crate::ids::{BuildId, Handle, hex_lower};
use crate::macros::string_enum;
use crate::schema::{FieldSchema, FieldSpec, ObjectSchema};

string_enum! {
    /// A build configuration of the project.
    pub enum BuildConfig {
        /// Debug: no optimisation, debug information, sanitizers.
        Debug = "debug",
        /// Release: optimised.
        Release = "release",
    }
}

/// The request of `build_start`. Its channel `onEvent` carries [`BuildEvent`]s.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BuildStartRequest {
    /// The project.
    pub handle: Handle,
    /// The project as BDM JSON text: what to build.
    pub document: String,
    /// The configuration.
    pub config: BuildConfig,
}

impl IpcRequest for BuildStartRequest {
    const COMMAND: &'static str = "build_start";

    fn schema() -> &'static ObjectSchema {
        static SCHEMA: ObjectSchema = ObjectSchema {
            fields: &[
                FieldSpec::required("handle", Handle::SCHEMA),
                FieldSpec::required("document", DOCUMENT),
                FieldSpec::required(
                    "config",
                    FieldSchema::Enum {
                        values: BuildConfig::VALUES,
                    },
                ),
            ],
        };
        &SCHEMA
    }

    fn sample() -> Self {
        Self {
            handle: Handle::example(),
            document: SAMPLE_DOCUMENT.to_owned(),
            config: BuildConfig::Debug,
        }
    }

    fn validate(&self) -> Result<(), IpcError> {
        check_document(&self.document)
    }
}

/// The response of `build_start`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BuildStartResponse {
    /// The new build (`build_cancel`, `run_start`).
    pub build_id: BuildId,
}

id_request!(
    /// The request of `build_cancel`. Cancelling a finished build does nothing.
    BuildCancelRequest, command = "build_cancel",
    /// The build.
    build_id: BuildId = "buildId"
);

/// The response of `build_cache_clear`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BuildCacheClearResponse {
    /// How many bytes were deleted.
    pub freed_bytes: u64,
    /// How many build folders were kept because a build or run was using them.
    pub skipped_in_use: u32,
}

string_enum! {
    /// A stage of a build.
    pub enum BuildStage {
        /// Generating C++ from the blocks.
        Generate = "generate",
        /// Compiling translation units.
        Compile = "compile",
        /// Linking the program.
        Link = "link",
    }
}

string_enum! {
    /// How a build ended.
    pub enum BuildOutcome {
        /// The program was built.
        Built = "built",
        /// Nothing changed since the last successful build; the cached program is
        /// current.
        UpToDate = "upToDate",
        /// The analyser or the compiler found errors in the project.
        ProjectErrors = "projectErrors",
        /// The toolchain is missing or unusable.
        ToolchainProblem = "toolchainProblem",
        /// The build was cancelled.
        Cancelled = "cancelled",
        /// An internal failure; the details are in the log.
        Failed = "failed",
    }
}

/// A message on the `onEvent` channel of `build_start`. Exactly one
/// [`BuildEvent::Finished`] is sent, and it is always the last message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum BuildEvent {
    /// Progress through a stage.
    Progress {
        /// The stage.
        stage: BuildStage,
        /// How many steps of the stage are done.
        done: u32,
        /// How many steps the stage has.
        total: u32,
    },
    /// Diagnostics found so far (more may follow).
    Diagnostics {
        /// The diagnostics, mapped to blocks where possible.
        items: Vec<Diagnostic>,
    },
    /// The build ended.
    Finished {
        /// How it ended.
        outcome: BuildOutcome,
        /// The content hash of the project that was built (64 lower-case hex
        /// digits), when the document could be loaded.
        project_hash: Option<String>,
        /// How long the build took, in milliseconds.
        elapsed_ms: u64,
    },
}

impl BuildEvent {
    /// A [`BuildEvent::Finished`] with the project hash formatted as lower-case hex.
    pub fn finished(outcome: BuildOutcome, project_hash: Option<&[u8; 32]>, elapsed_ms: u64) -> Self {
        Self::Finished {
            outcome,
            project_hash: project_hash.map(|hash| hex_lower(hash)),
            elapsed_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finished_formats_the_hash() {
        let event = BuildEvent::finished(BuildOutcome::Built, Some(&[0xab; 32]), 12);
        let BuildEvent::Finished { project_hash, .. } = &event else {
            panic!("finished");
        };
        assert_eq!(project_hash.as_deref(), Some("ab".repeat(32).as_str()));
        let json = serde_json::to_value(BuildEvent::finished(BuildOutcome::Cancelled, None, 3)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "finished", "outcome": "cancelled", "projectHash": null, "elapsedMs": 3})
        );
    }
}
