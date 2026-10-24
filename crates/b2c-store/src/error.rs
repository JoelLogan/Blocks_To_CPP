//! Errors of the stores.
//!
//! Their messages never contain a path: paths are private (08 §8.11), so
//! callers log the `path` fields at debug level only and show users a
//! message of their own.

use std::io;
use std::path::{Path, PathBuf};

/// Why a store operation failed.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// An I/O operation failed.
    #[error("could not {action}: {source}")]
    Io {
        /// What was being done, as a verb phrase ("replace the file").
        action: &'static str,
        /// The path involved (for the debug log only).
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
    /// A path that must be a real file or folder is a symbolic link, a
    /// junction or another reparse point, or not the expected kind of file
    /// system object (for example a folder where a file should be). Nothing
    /// was read or written through it.
    #[error("refusing to use a path that is a link or not the expected kind of file or folder")]
    Link {
        /// The offending path (for the debug log only).
        path: PathBuf,
    },
    /// A value breaks a rule of the store; the text names the rule.
    #[error("{0}")]
    Invalid(&'static str),
    /// A path is not valid Unicode, so it cannot be recorded in a JSON file.
    #[error("the path is not valid Unicode, so it cannot be recorded")]
    PathNotUnicode,
    /// The operating system's random number generator failed.
    #[error("the system's random number generator failed: {source}")]
    Random {
        /// The underlying error.
        #[source]
        source: io::Error,
    },
}

impl StoreError {
    /// An [`StoreError::Io`] error for `path`.
    pub(crate) fn io(action: &'static str, path: &Path, source: io::Error) -> Self {
        Self::Io {
            action,
            path: path.to_path_buf(),
            source,
        }
    }

    /// A [`StoreError::Link`] error for `path`.
    pub(crate) fn link(path: &Path) -> Self {
        Self::Link {
            path: path.to_path_buf(),
        }
    }
}

/// Why [`crate::read::read_bounded`] could not read a file.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// The file is larger than the limit. At most `limit + 1` bytes were read.
    #[error("the file is larger than {limit} bytes")]
    TooLarge {
        /// The limit, in bytes.
        limit: u64,
    },
    /// The path is not a regular file (a folder, a device, a pipe, ...).
    #[error("not a regular file")]
    NotAFile,
    /// The file does not exist.
    #[error("the file does not exist")]
    NotFound,
    /// Another I/O error.
    #[error("could not read the file: {0}")]
    Io(#[source] io::Error),
}

impl ReadError {
    /// This error as a [`StoreError`] about `path`, for stores that report
    /// read failures as errors: [`ReadError::NotAFile`] becomes
    /// [`StoreError::Link`], [`ReadError::TooLarge`] becomes
    /// [`StoreError::Invalid`] and the others [`StoreError::Io`].
    pub fn at(self, path: &Path) -> StoreError {
        match self {
            Self::TooLarge { .. } => StoreError::Invalid("the file is larger than its size limit"),
            Self::NotAFile => StoreError::link(path),
            Self::NotFound => StoreError::io("read the file", path, io::ErrorKind::NotFound.into()),
            Self::Io(source) => StoreError::io("read the file", path, source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_never_show_paths() {
        let path = Path::new("/home/someone/secret-project/project.b2c");
        let errors = [
            StoreError::io("replace the file", path, io::ErrorKind::PermissionDenied.into()),
            StoreError::link(path),
            ReadError::NotFound.at(path),
            ReadError::NotAFile.at(path),
            ReadError::TooLarge { limit: 10 }.at(path),
            ReadError::Io(io::ErrorKind::Other.into()).at(path),
        ];
        for error in errors {
            let text = error.to_string();
            assert!(!text.contains("secret-project"), "{text}");
            assert!(!text.is_empty());
        }
    }
}
