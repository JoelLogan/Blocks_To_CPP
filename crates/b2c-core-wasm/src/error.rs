//! Errors of the facade itself.
//!
//! Problems in a project or a clipboard payload are never errors here: they
//! are diagnostics inside an ordinary result. A [`FacadeError`] means the
//! caller passed arguments the export cannot use (for example preview
//! options other than `{"indentWidth": 2 | 4}`, or a paste target naming a
//! block the document does not have), or a result could not be produced for
//! a reason that is a bug in Blocks2Cpp. On the wire it is
//! `{"error": {"kind": "<kind>", "message": "<text>"}}`, which the
//! TypeScript wrapper turns into a thrown `CoreError`.

use b2c_ir::Diagnostic;

/// Why an export could not produce its normal result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FacadeError {
    /// The preview options are not `{"indentWidth": 2 | 4}`. The message
    /// never repeats the input text.
    #[error("the preview options are not valid: {0}")]
    InvalidOptions(String),
    /// Another argument is malformed, too large, or names something the
    /// document does not have (a block ID list, a paste target, a seed). The
    /// message names the argument and repeats only text that passed the ID
    /// rules (`[A-Za-z0-9_]{1,32}`), never other input.
    #[error("the arguments are not valid: {0}")]
    InvalidArguments(String),
    /// A result could not be encoded as JSON (a bug in Blocks2Cpp).
    #[error("the result could not be encoded as JSON: {0}")]
    Encode(String),
    /// The core could not finish for a reason that is a bug in Blocks2Cpp
    /// (for example its state was in use by another call, or no fresh ID
    /// could be found).
    #[error("internal error: {0}")]
    Internal(String),
}

impl FacadeError {
    /// The stable wire name of the error kind: `invalidOptions`,
    /// `invalidArguments`, `encode` or `internal`.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidOptions(_) => "invalidOptions",
            Self::InvalidArguments(_) => "invalidArguments",
            Self::Encode(_) => "encode",
            Self::Internal(_) => "internal",
        }
    }
}

/// Why an export that checks untrusted input produced no result.
#[derive(Debug, Clone, PartialEq)]
pub enum Failure {
    /// The input is not valid, with every problem found: on the wire
    /// `{"ok": false, "diagnostics": [...]}` (`B2C-E01xx` loader codes for a
    /// document or a clipboard payload that does not load).
    Diagnostics(Vec<Diagnostic>),
    /// The call itself was refused: the error envelope.
    Error(FacadeError),
}

impl From<FacadeError> for Failure {
    fn from(error: FacadeError) -> Self {
        Self::Error(error)
    }
}

impl From<b2c_model::LoadError> for Failure {
    fn from(error: b2c_model::LoadError) -> Self {
        Self::Diagnostics(error.diagnostics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_messages() {
        let invalid = FacadeError::InvalidOptions(String::from("expected 2 or 4"));
        assert_eq!(invalid.kind(), "invalidOptions");
        assert_eq!(
            invalid.to_string(),
            "the preview options are not valid: expected 2 or 4"
        );
        let arguments = FacadeError::InvalidArguments(String::from("the seed is not 64 hex digits"));
        assert_eq!(arguments.kind(), "invalidArguments");
        assert_eq!(
            arguments.to_string(),
            "the arguments are not valid: the seed is not 64 hex digits"
        );
        let encode = FacadeError::Encode(String::from("boom"));
        assert_eq!(encode.kind(), "encode");
        assert_eq!(
            encode.to_string(),
            "the result could not be encoded as JSON: boom"
        );
        let internal = FacadeError::Internal(String::from("busy"));
        assert_eq!(internal.kind(), "internal");
        assert_eq!(internal.to_string(), "internal error: busy");
    }

    #[test]
    fn failures_from_errors() {
        let failure = Failure::from(FacadeError::Internal(String::from("x")));
        assert!(matches!(failure, Failure::Error(FacadeError::Internal(_))));
        let Err(load) = b2c_model::load(b"[]") else {
            panic!("[] is not a project");
        };
        let expected = load.diagnostics.clone();
        assert_eq!(Failure::from(load), Failure::Diagnostics(expected));
    }
}
