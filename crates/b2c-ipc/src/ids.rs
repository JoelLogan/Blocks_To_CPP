//! Opaque IDs (`docs/spec/02-architecture.md` §2.5 "Opaque handles",
//! `docs/spec/08-security.md` §8.12 T8 and T9).
//!
//! No request names a filesystem path. Everything the webview refers to is one of
//! these IDs, which the backend maps to its own state: a project handle to the
//! canonical path the user chose in a native dialog, a recent ID to the backend's
//! recent list, and so on. An unknown or stale ID is a typed error.
//!
//! | Type           | Format                  | Made from                                  |
//! |----------------|-------------------------|--------------------------------------------|
//! | [`Handle`]     | `ph_` + 32 hex digits   | 128 bits of OS CSPRNG output                |
//! | [`BuildId`]    | `bd_` + 32 hex digits   | 128 bits of OS CSPRNG output                |
//! | [`RunId`]      | `rn_` + 32 hex digits   | 128 bits of OS CSPRNG output                |
//! | [`RecentId`]   | `rc_` + 32 hex digits   | 128 bits of OS CSPRNG output                |
//! | [`SnapshotId`] | `sn_` + 32 hex digits   | 128 bits of OS CSPRNG output                |
//! | [`ToolchainId`]| `tc_` + 16 hex digits   | SHA-256 of the canonical driver path        |
//!
//! Hex digits are lower case. Every type validates its format when it is parsed or
//! deserialised, so a value of the type is always well-formed.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::error::{InvalidReason, IpcError};
use crate::schema::{FieldSchema, is_id};

/// Lower-case hex of `bytes`.
pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

/// Fills `buffer` from the OS CSPRNG.
fn random_bytes(buffer: &mut [u8]) -> Result<(), IpcError> {
    getrandom::fill(buffer).map_err(|_| IpcError::Internal)
}

macro_rules! opaque_id {
    (
        $(#[$meta:meta])*
        $name:ident, prefix = $prefix:literal, hex = $hex:literal, ts = $ts:literal
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(type = $ts))]
        pub struct $name(Box<str>);

        impl $name {
            /// The fixed prefix.
            pub const PREFIX: &'static str = $prefix;
            /// The number of lower-case hex digits after the prefix.
            pub const HEX_LEN: usize = $hex;
            /// The request schema of this ID.
            pub const SCHEMA: FieldSchema = FieldSchema::Id {
                prefix: $prefix,
                hex_len: $hex,
            };

            /// Parses an ID received over IPC.
            ///
            /// # Errors
            /// Returns [`IpcError::InvalidRequest`] with reason `badId` when `text`
            /// does not match the format.
            pub fn parse(text: &str) -> Result<Self, IpcError> {
                if is_id(text, Self::PREFIX, Self::HEX_LEN) {
                    Ok(Self(text.into()))
                } else {
                    Err(IpcError::invalid(InvalidReason::BadId, None))
                }
            }

            /// The ID text.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// A fixed, valid ID for samples and tests: the prefix followed by the
            /// hex digits `0123456789abcdef`, repeated to the full length.
            pub fn example() -> Self {
                let hex: String = "0123456789abcdef".chars().cycle().take(Self::HEX_LEN).collect();
                Self(format!("{}{hex}", Self::PREFIX).into())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                Self::parse(&text).map_err(|_| serde::de::Error::custom(MALFORMED_ID))
            }
        }
    };
}

/// The serde error text of a malformed ID (the decoder maps it to `badId`).
pub(crate) const MALFORMED_ID: &str = "malformed ID";

macro_rules! random_id {
    ($name:ident) => {
        impl $name {
            /// A new ID from 128 bits of OS CSPRNG output.
            ///
            /// # Errors
            /// Returns [`IpcError::Internal`] if the operating system cannot supply
            /// random bytes.
            pub fn random() -> Result<Self, IpcError> {
                let mut bytes = [0_u8; 16];
                random_bytes(&mut bytes)?;
                Ok(Self(
                    format!("{}{}", Self::PREFIX, hex_lower(&bytes)).into(),
                ))
            }
        }
    };
}

opaque_id!(
    /// An open project: maps, inside the backend, to the canonical path the user
    /// chose (or to no path for a new, unsaved project).
    Handle, prefix = "ph_", hex = 32, ts = "`ph_${string}`"
);
random_id!(Handle);

opaque_id!(
    /// A build session.
    BuildId, prefix = "bd_", hex = 32, ts = "`bd_${string}`"
);
random_id!(BuildId);

opaque_id!(
    /// A run session.
    RunId, prefix = "rn_", hex = 32, ts = "`rn_${string}`"
);
random_id!(RunId);

opaque_id!(
    /// An entry of the backend's recent-projects list.
    RecentId, prefix = "rc_", hex = 32, ts = "`rc_${string}`"
);
random_id!(RecentId);

opaque_id!(
    /// A recovery snapshot.
    SnapshotId, prefix = "sn_", hex = 32, ts = "`sn_${string}`"
);
random_id!(SnapshotId);

opaque_id!(
    /// A toolchain in the backend's toolchain list. Stable across restarts: it is
    /// derived from the canonical path of the compiler driver.
    ToolchainId, prefix = "tc_", hex = 16, ts = "`tc_${string}`"
);

impl ToolchainId {
    /// The ID of the compiler driver at `canonical`: `tc_` and the first 16 hex
    /// digits of the SHA-256 of the path's bytes (UTF-8 on Unix, UTF-16LE on
    /// Windows). The path must already be canonical, so that two spellings of one
    /// file get one ID.
    pub fn for_driver(canonical: &Path) -> Self {
        let digest: [u8; 32] = Sha256::digest(path_bytes(canonical)).into();
        let (first, _) = digest.split_at(Self::HEX_LEN / 2);
        Self(format!("{}{}", Self::PREFIX, hex_lower(first)).into())
    }
}

/// The bytes of a path that [`ToolchainId::for_driver`] hashes.
#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

/// The bytes of a path that [`ToolchainId::for_driver`] hashes.
#[cfg(windows)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// The bytes of a path that [`ToolchainId::for_driver`] hashes.
#[cfg(not(any(unix, windows)))]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().into_owned().into_bytes()
}

/// The characters of a project ID after `prj_`.
const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// The number of base62 digits that hold 96 bits (62^17 > 2^96 > 62^16).
const PROJECT_ID_DIGITS: usize = 17;

/// A new project ID for a project created in the app: `prj_` and 17 base62
/// characters encoding 96 bits of OS CSPRNG output.
///
/// # Errors
/// Returns [`IpcError::Internal`] if the operating system cannot supply random
/// bytes.
pub fn random_project_id() -> Result<b2c_ir::ProjectId, IpcError> {
    let mut bytes = [0_u8; 12];
    random_bytes(&mut bytes)?;
    project_id_from_bits(bytes)
}

/// The project ID that encodes `bytes` (big-endian), zero-padded to 17 digits.
fn project_id_from_bits(bytes: [u8; 12]) -> Result<b2c_ir::ProjectId, IpcError> {
    let mut value = bytes.iter().fold(0_u128, |acc, &b| (acc << 8) | u128::from(b));
    let mut digits = [b'0'; PROJECT_ID_DIGITS];
    for digit in digits.iter_mut().rev() {
        // `value % 62` is below 62, so the index is in bounds and the cast exact.
        let index = usize::try_from(value % 62).map_err(|_| IpcError::Internal)?;
        *digit = BASE62.get(index).copied().ok_or(IpcError::Internal)?;
        value /= 62;
    }
    let text = std::str::from_utf8(&digits).map_err(|_| IpcError::Internal)?;
    b2c_ir::ProjectId::new(&format!("prj_{text}")).map_err(|_| IpcError::Internal)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn random_ids_have_the_format() {
        let mut seen = HashSet::new();
        for _ in 0..100 {
            let handle = Handle::random().unwrap();
            assert!(is_id(handle.as_str(), "ph_", 32), "{handle}");
            assert!(seen.insert(handle));
        }
        assert!(BuildId::random().unwrap().as_str().starts_with("bd_"));
        assert!(RunId::random().unwrap().as_str().starts_with("rn_"));
        assert!(RecentId::random().unwrap().as_str().starts_with("rc_"));
        assert!(SnapshotId::random().unwrap().as_str().starts_with("sn_"));
    }

    #[test]
    fn parse_and_serde_validate() {
        let text = "ph_0123456789abcdef0123456789abcdef";
        let handle = Handle::parse(text).unwrap();
        assert_eq!(handle.to_string(), text);
        assert_eq!(serde_json::to_string(&handle).unwrap(), format!("\"{text}\""));
        assert_eq!(
            serde_json::from_str::<Handle>(&format!("\"{text}\"")).unwrap(),
            handle
        );
        for bad in [
            "",
            "ph_",
            "ph_0123456789ABCDEF0123456789abcdef",
            "ph_0123456789abcdef0123456789abcde",
            "ph_0123456789abcdef0123456789abcdef0",
            "bd_0123456789abcdef0123456789abcdef",
            "ph_0123456789abcdef0123456789abcdeg",
            " ph_0123456789abcdef0123456789abcdef",
        ] {
            assert_eq!(
                Handle::parse(bad).unwrap_err(),
                IpcError::invalid(InvalidReason::BadId, None),
                "{bad:?}"
            );
            let error = serde_json::from_value::<Handle>(serde_json::json!(bad)).unwrap_err();
            assert_eq!(error.to_string(), MALFORMED_ID);
        }
        assert!(serde_json::from_str::<Handle>("7").is_err());
    }

    #[test]
    fn toolchain_ids_are_stable_hashes() {
        let id = ToolchainId::for_driver(Path::new("/usr/bin/g++-13"));
        assert!(is_id(id.as_str(), "tc_", 16), "{id}");
        assert_eq!(id, ToolchainId::for_driver(Path::new("/usr/bin/g++-13")));
        assert_ne!(id, ToolchainId::for_driver(Path::new("/usr/bin/g++-14")));
        #[cfg(unix)]
        {
            // SHA-256("/usr/bin/g++") starts with these bytes (sha256sum).
            let expected: [u8; 32] = Sha256::digest(b"/usr/bin/g++").into();
            let id = ToolchainId::for_driver(Path::new("/usr/bin/g++"));
            assert_eq!(id.as_str(), format!("tc_{}", hex_lower(&expected[..8])));
        }
        assert!(ToolchainId::parse(id.as_str()).is_ok());
    }

    #[test]
    fn project_ids() {
        let zero = project_id_from_bits([0; 12]).unwrap();
        assert_eq!(zero.as_str(), "prj_00000000000000000");
        let max = project_id_from_bits([0xff; 12]).unwrap();
        // 2^96 - 1 in base62.
        assert_eq!(max.as_str(), "prj_1f2SI9UJPXvb7vdJ1");
        let mut bits = [0; 12];
        bits[11] = 61;
        assert_eq!(
            project_id_from_bits(bits).unwrap().as_str(),
            "prj_0000000000000000z"
        );
        bits[11] = 62;
        assert_eq!(
            project_id_from_bits(bits).unwrap().as_str(),
            "prj_00000000000000010"
        );
        let a = random_project_id().unwrap();
        let b = random_project_id().unwrap();
        assert_ne!(a, b);
        assert_eq!(a.as_str().len(), 21);
        assert!(a.as_str()[4..].bytes().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn hex() {
        assert_eq!(hex_lower(&[0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
        assert_eq!(hex_lower(&[]), "");
    }
}
