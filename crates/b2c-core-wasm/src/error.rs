//! Errors of the facade itself.
//!
//! Problems in a project are never errors here: they are diagnostics inside
//! an ordinary result. A [`FacadeError`] means the caller passed arguments
//! the export cannot use (for example preview options other than
//! `{"indentWidth": 2 | 4}`), or a result could not be encoded. On the wire
//! it is `{"error": {"kind": "<kind>", "message": "<text>"}}`, which the
//! TypeScript wrapper turns into a thrown `CoreError`.

/// Why an export could not produce its normal result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FacadeError {
    /// The preview options are not `{"indentWidth": 2 | 4}`. The message
    /// never repeats the input text.
    #[error("the preview options are not valid: {0}")]
    InvalidOptions(String),
    /// A result could not be encoded as JSON (a bug in Blocks2Cpp).
    #[error("the result could not be encoded as JSON: {0}")]
    Encode(String),
}

impl FacadeError {
    /// The stable wire name of the error kind: `invalidOptions` or `encode`.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidOptions(_) => "invalidOptions",
            Self::Encode(_) => "encode",
        }
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
        let encode = FacadeError::Encode(String::from("boom"));
        assert_eq!(encode.kind(), "encode");
        assert_eq!(
            encode.to_string(),
            "the result could not be encoded as JSON: boom"
        );
    }
}
