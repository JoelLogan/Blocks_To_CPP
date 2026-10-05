//! The typed error every IPC command returns (`docs/spec/09-quality-and-delivery.md`
//! §9.1: "No panics cross the IPC boundary: command handlers return typed errors").
//!
//! The JSON form is `{ "code": "<camelCase code>", …arguments }`, stable for the
//! frontend's i18n layer, which turns each code into user-facing text
//! (`docs/spec/04-user-interface.md` §4.9). The [`Display`](std::fmt::Display) text
//! is for logs only: it is fixed per variant and never contains a path or project
//! content. Details such as the path of a failed write go to the log at debug level
//! by the code that has them, never into an [`IpcError`].

use serde::{Deserialize, Serialize};

use crate::diag::Diagnostic;
use crate::macros::string_enum;

string_enum! {
    /// Why a request was rejected before any handler logic ran.
    pub enum InvalidReason {
        /// The request is not the expected JSON shape: not an object, or a value of
        /// the wrong JSON type.
        Malformed = "malformed",
        /// A key that the request type does not define.
        UnknownField = "unknownField",
        /// A required key is missing.
        MissingField = "missingField",
        /// A string that is not one of the enum's values.
        BadEnum = "badEnum",
        /// An opaque ID that does not match its format.
        BadId = "badId",
        /// A number outside its allowed range.
        OutOfRange = "outOfRange",
        /// Text that is not valid base64.
        BadEncoding = "badEncoding",
    }
}

string_enum! {
    /// The class of a failed file operation (the path is logged, never sent).
    pub enum IoKind {
        /// The file or folder does not exist.
        NotFound = "notFound",
        /// The operating system refused access.
        PermissionDenied = "permissionDenied",
        /// The target already exists.
        AlreadyExists = "alreadyExists",
        /// Any other I/O failure.
        Other = "other",
    }
}

impl From<std::io::ErrorKind> for IoKind {
    fn from(kind: std::io::ErrorKind) -> Self {
        match kind {
            std::io::ErrorKind::NotFound => Self::NotFound,
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            std::io::ErrorKind::AlreadyExists => Self::AlreadyExists,
            _ => Self::Other,
        }
    }
}

/// The error of every IPC command.
///
/// Serialised as `{ "code": "…", … }` with camelCase codes and fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "code", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum IpcError {
    /// The request does not match its schema. Nothing was changed.
    #[error("invalid request ({reason}{})", field_suffix(.field.as_deref()))]
    InvalidRequest {
        /// What is wrong.
        reason: InvalidReason,
        /// The dotted path of the offending field inside the request (for example
        /// `runOptions.cols`), when it is known.
        field: Option<String>,
    },
    /// A value is larger than its limit. Nothing was parsed or changed.
    #[error("payload larger than {limit}")]
    PayloadTooLarge {
        /// The limit that was exceeded (bytes, or characters for base64 text).
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        limit: u64,
    },
    /// The project handle is not open (never existed, or already closed).
    #[error("unknown project handle")]
    UnknownHandle,
    /// The build ID is unknown or its project was closed.
    #[error("unknown build")]
    UnknownBuild,
    /// The run ID is unknown.
    #[error("unknown run")]
    UnknownRun,
    /// The recent-projects entry no longer exists.
    #[error("unknown recent-projects entry")]
    UnknownRecent,
    /// The recovery snapshot no longer exists or belongs to a running instance.
    #[error("unknown recovery snapshot")]
    UnknownSnapshot,
    /// The toolchain ID is not in the toolchain list.
    #[error("unknown toolchain")]
    UnknownToolchain,
    /// The document is not a valid project; the diagnostics say why.
    #[error("invalid project document ({} problem(s))", .diagnostics.len())]
    InvalidDocument {
        /// Every problem the loader found.
        diagnostics: Vec<Diagnostic>,
    },
    /// The document was made by a newer version of Blocks2Cpp
    /// (`docs/spec/05-project-format.md` §5.7).
    #[error("project made by a newer version")]
    NewerFormat {
        /// The app version that saved the project, when the file names a plausible
        /// one: the user needs at least this version.
        needs: Option<String>,
    },
    /// The project is in Restricted Mode, so it cannot be built or run
    /// (`docs/spec/08-security.md` §8.3).
    #[error("project is restricted")]
    Restricted,
    /// The file changed on disk since it was opened or last saved; it was not
    /// overwritten.
    #[error("file changed on disk")]
    ChangedOnDisk,
    /// The project has never been saved, so it has no file to reload or save to.
    #[error("project has no file")]
    NoPath,
    /// The file no longer exists.
    #[error("file not found")]
    NotFound,
    /// The build is older than the project's current content, so it cannot run.
    #[error("build is stale")]
    StaleBuild,
    /// The build did not succeed, so there is nothing to run.
    #[error("build did not succeed")]
    BuildNotSuccessful,
    /// The analyser found errors, so the project cannot be built
    /// (`docs/spec/07-toolchain-build-run.md` §7.6.1).
    #[error("project has {count} error(s)")]
    ProjectErrors {
        /// How many errors.
        count: u32,
    },
    /// The program is not running any more.
    #[error("program not running")]
    NotRunning,
    /// Too many calls in a short time; try again later.
    #[error("rate limited")]
    RateLimited,
    /// Another native dialog is open.
    #[error("busy")]
    Busy,
    /// Too many projects are open.
    #[error("too many open projects")]
    TooManyHandles,
    /// Too many programs are running.
    #[error("too many running programs")]
    TooManySessions,
    /// The chosen compiler cannot be used; the diagnostics say why.
    #[error("toolchain rejected ({} problem(s))", .diagnostics.len())]
    ToolchainRejected {
        /// The `B2C-T1xxx` problems found.
        diagnostics: Vec<Diagnostic>,
    },
    /// A file operation failed.
    #[error("I/O error ({kind})")]
    Io {
        /// The class of the failure.
        kind: IoKind,
    },
    /// An unexpected internal failure; the details are in the log.
    #[error("internal error")]
    Internal,
}

/// The ` at <field>` part of the log text of [`IpcError::InvalidRequest`].
fn field_suffix(field: Option<&str>) -> String {
    field.map(|f| format!(" at {f}")).unwrap_or_default()
}

impl IpcError {
    /// Every error code, in declaration order (the `code` values of the JSON form).
    pub const CODES: &'static [&'static str] = &[
        "invalidRequest",
        "payloadTooLarge",
        "unknownHandle",
        "unknownBuild",
        "unknownRun",
        "unknownRecent",
        "unknownSnapshot",
        "unknownToolchain",
        "invalidDocument",
        "newerFormat",
        "restricted",
        "changedOnDisk",
        "noPath",
        "notFound",
        "staleBuild",
        "buildNotSuccessful",
        "projectErrors",
        "notRunning",
        "rateLimited",
        "busy",
        "tooManyHandles",
        "tooManySessions",
        "toolchainRejected",
        "io",
        "internal",
    ];

    /// The stable machine code of this error, as in the JSON form.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidRequest { .. } => "invalidRequest",
            Self::PayloadTooLarge { .. } => "payloadTooLarge",
            Self::UnknownHandle => "unknownHandle",
            Self::UnknownBuild => "unknownBuild",
            Self::UnknownRun => "unknownRun",
            Self::UnknownRecent => "unknownRecent",
            Self::UnknownSnapshot => "unknownSnapshot",
            Self::UnknownToolchain => "unknownToolchain",
            Self::InvalidDocument { .. } => "invalidDocument",
            Self::NewerFormat { .. } => "newerFormat",
            Self::Restricted => "restricted",
            Self::ChangedOnDisk => "changedOnDisk",
            Self::NoPath => "noPath",
            Self::NotFound => "notFound",
            Self::StaleBuild => "staleBuild",
            Self::BuildNotSuccessful => "buildNotSuccessful",
            Self::ProjectErrors { .. } => "projectErrors",
            Self::NotRunning => "notRunning",
            Self::RateLimited => "rateLimited",
            Self::Busy => "busy",
            Self::TooManyHandles => "tooManyHandles",
            Self::TooManySessions => "tooManySessions",
            Self::ToolchainRejected { .. } => "toolchainRejected",
            Self::Io { .. } => "io",
            Self::Internal => "internal",
        }
    }

    /// An [`IpcError::InvalidRequest`] for a field (or the whole request when
    /// `field` is `None`).
    pub fn invalid(reason: InvalidReason, field: Option<&str>) -> Self {
        Self::InvalidRequest {
            reason,
            field: field.map(str::to_owned),
        }
    }

    /// An [`IpcError::PayloadTooLarge`] for a limit given in bytes or characters.
    pub fn too_large(limit: usize) -> Self {
        Self::PayloadTooLarge {
            limit: u64::try_from(limit).unwrap_or(u64::MAX),
        }
    }

    /// The [`IpcError::Io`] for an I/O error. Only the error's kind is kept: the
    /// caller logs the details (which may name a path) itself.
    pub fn io(error: &std::io::Error) -> Self {
        Self::Io {
            kind: error.kind().into(),
        }
    }

    /// The same error with the field path of an [`IpcError::InvalidRequest`]
    /// replaced by `field` when it has none; other errors are returned unchanged.
    #[must_use]
    pub fn at_field(self, field: &str) -> Self {
        match self {
            Self::InvalidRequest { reason, field: None } => Self::invalid(reason, Some(field)),
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_fixed_text() {
        assert_eq!(
            IpcError::invalid(InvalidReason::OutOfRange, Some("runOptions.cols")).to_string(),
            "invalid request (outOfRange at runOptions.cols)"
        );
        assert_eq!(
            IpcError::invalid(InvalidReason::Malformed, None).to_string(),
            "invalid request (malformed)"
        );
        assert_eq!(IpcError::too_large(10).to_string(), "payload larger than 10");
        let error = std::io::Error::new(std::io::ErrorKind::NotFound, "/home/someone/secret.b2c");
        let ipc = IpcError::io(&error);
        assert_eq!(
            ipc,
            IpcError::Io {
                kind: IoKind::NotFound
            }
        );
        assert!(!ipc.to_string().contains("secret"));
        assert_eq!(
            IpcError::io(&std::io::Error::other("x")),
            IpcError::Io { kind: IoKind::Other }
        );
    }

    #[test]
    fn at_field_fills_only_a_missing_field() {
        let error = IpcError::invalid(InvalidReason::BadEncoding, None).at_field("data");
        assert_eq!(error, IpcError::invalid(InvalidReason::BadEncoding, Some("data")));
        let error = IpcError::invalid(InvalidReason::BadId, Some("handle")).at_field("data");
        assert_eq!(error, IpcError::invalid(InvalidReason::BadId, Some("handle")));
        assert_eq!(IpcError::Busy.at_field("data"), IpcError::Busy);
    }

    #[test]
    fn code_matches_the_json_form() {
        let json = serde_json::to_value(IpcError::TooManyHandles).unwrap();
        assert_eq!(json, serde_json::json!({"code": "tooManyHandles"}));
        assert_eq!(IpcError::TooManyHandles.code(), "tooManyHandles");
        // Every variant is covered by tests/snapshots.rs, which checks code() against
        // the JSON of each one and that CODES lists each code once.
    }
}
