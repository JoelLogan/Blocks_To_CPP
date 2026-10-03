//! Identifiers of project entities (spec §5.4–5.5).
//!
//! IDs are opaque strings `[A-Za-z0-9_]{1,32}`. They never appear in generated
//! C++ (only in source maps and diagnostics), but they are validated anyway so
//! that a malicious project file cannot smuggle markup or control characters
//! into diagnostics or the UI.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Maximum ID length.
pub const MAX_ID_LEN: usize = 32;

/// Why a string is not a valid ID.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a valid ID (1–32 characters from A–Z, a–z, 0–9 and _)")]
pub struct IdError(pub String);

fn validate(value: &str) -> Result<(), IdError> {
    let ok = !value.is_empty()
        && value.len() <= MAX_ID_LEN
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(IdError(value.chars().take(40).collect()))
    }
}

macro_rules! define_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(Box<str>);

        impl $name {
            /// Validates and wraps an ID.
            ///
            /// # Errors
            /// Returns an error if the ID is empty, too long or has other characters.
            pub fn new(value: &str) -> Result<Self, IdError> {
                validate(value)?;
                Ok(Self(value.into()))
            }

            /// The ID text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = String::deserialize(deserializer)?;
                Self::new(&value).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_id!(
    /// A block's ID, unique within the project.
    BlockId
);
define_id!(
    /// A symbol's ID (variable, parameter, function, …), unique within the project.
    SymbolId
);
define_id!(
    /// A module's ID, unique within the project.
    ModuleId
);
define_id!(
    /// A project's ID.
    ProjectId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        assert!(BlockId::new("blk_Qm81").is_ok());
        assert!(BlockId::new(&"a".repeat(32)).is_ok());
        for bad in ["", "a-b", "a b", "<x>", "é", &"a".repeat(33)] {
            assert!(BlockId::new(bad).is_err(), "{bad}");
        }
        assert!(serde_json::from_str::<SymbolId>("\"sym_1\"").is_ok());
        assert!(serde_json::from_str::<SymbolId>("\"sym-1\"").is_err());
    }
}
