//! Toolchain fingerprints (spec §7.2): the identity of a compiler driver
//! binary, re-checked before each build so a swapped compiler is noticed.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Largest driver binary that is hashed (512 MiB); real drivers are a few
/// MiB.
const MAX_DRIVER_BYTES: u64 = 512 * 1024 * 1024;

/// The identity of a compiler driver binary: canonical path, size,
/// modification time and SHA-256. The probe adds the version and target
/// ([`crate::probe::Toolchain`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fingerprint {
    /// Canonical absolute path of the driver.
    pub path: PathBuf,
    /// Size in bytes.
    pub size: u64,
    /// Modification time in nanoseconds since the Unix epoch (0 if the
    /// file system does not record it).
    pub modified_ns: u64,
    /// SHA-256 of the file contents, lower-case hex.
    pub sha256: String,
}

impl Fingerprint {
    /// Computes the fingerprint of the file at `path`, which is canonicalised
    /// first.
    ///
    /// ```
    /// # #[cfg(unix)] {
    /// use b2c_toolchain::fingerprint::Fingerprint;
    ///
    /// let fingerprint = Fingerprint::compute("/bin/sh".as_ref())?;
    /// assert_eq!(fingerprint.sha256.len(), 64);
    /// assert!(fingerprint.is_current());
    /// # }
    /// # Ok::<(), std::io::Error>(())
    /// ```
    ///
    /// # Errors
    /// Any I/O error, or `InvalidInput` if the path is not a regular file or
    /// is larger than 512 MiB.
    pub fn compute(path: &Path) -> io::Result<Self> {
        let path = std::fs::canonicalize(path)?;
        let mut file = File::open(&path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a regular file"));
        }
        if metadata.len() > MAX_DRIVER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the file is too large to be a compiler",
            ));
        }
        let modified_ns = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |duration| {
                u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
            });
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 256 * 1024];
        let mut total: u64 = 0;
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            total = total.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
            if total > MAX_DRIVER_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "the file is too large to be a compiler",
                ));
            }
            hasher.update(buffer.get(..read).unwrap_or_default());
        }
        let digest = hasher.finalize();
        let sha256 = digest.iter().fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
            hex
        });
        Ok(Self {
            path,
            size: metadata.len(),
            modified_ns,
            sha256,
        })
    }

    /// Whether the file still has this fingerprint. The hash is recomputed
    /// (a few milliseconds for a typical driver), so any change is noticed,
    /// even one that restored the size and time stamp.
    pub fn is_current(&self) -> bool {
        Self::compute(&self.path).is_ok_and(|now| now == *self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_are_noticed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("g++");
        std::fs::write(&file, b"first").unwrap();
        let first = Fingerprint::compute(&file).unwrap();
        assert_eq!(first.size, 5);
        assert_eq!(
            first.sha256,
            "a7937b64b8caa58f03721bb6bacf5c78cb235febe0e70b1b84cd99541461a08e"
        );
        assert!(first.is_current());
        std::fs::write(&file, b"other").unwrap();
        assert!(!first.is_current());
        std::fs::remove_file(&file).unwrap();
        assert!(!first.is_current());
    }

    #[test]
    fn directories_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Fingerprint::compute(dir.path()).is_err());
    }
}
