//! Errors: why the backend cannot start ([`StartError`]), and how internal
//! failures become the typed [`IpcError`]s the webview sees
//! (`docs/spec/02-architecture.md` §2.5.5, `docs/spec/09-quality-and-delivery.md`
//! §9.1).
//!
//! An [`IpcError`] never carries a path or project content. The functions here
//! log the details of a failure, including its path, at debug level only
//! (`docs/spec/08-security.md` §8.11), and return the error's class: `io` with
//! the kind of an I/O failure, or `internal`.

use std::io;
use std::path::Path;

use b2c_ipc::{IoKind, IpcError};
use b2c_store::{ReadError, StoreError};

/// Why [`crate::Backend::start`] failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StartError {
    /// One of the app's folders is not an absolute path, so files could end up
    /// relative to whatever the current directory is.
    #[error("the {0} folder is not an absolute path")]
    RelativeFolder(&'static str),
}

/// The [`IpcError`] for a failed store operation; the details are logged at
/// debug level.
pub(crate) fn store_error(context: &'static str, error: &StoreError) -> IpcError {
    match error {
        StoreError::Io { action, path, source } => {
            tracing::debug!(context, action, path = %path.display(), error = %source, "store operation failed");
            IpcError::io(source)
        }
        StoreError::Link { path } => {
            tracing::debug!(context, path = %path.display(), "refused a link or an unexpected kind of file");
            IpcError::Io { kind: IoKind::Other }
        }
        StoreError::PathNotUnicode => {
            tracing::debug!(context, "a path is not valid Unicode");
            IpcError::Io { kind: IoKind::Other }
        }
        StoreError::Invalid(rule) => {
            tracing::debug!(context, rule, "a store refused a value");
            IpcError::Internal
        }
        StoreError::Random { source } => {
            tracing::warn!(context, error = %source, "the system's random number generator failed");
            IpcError::Internal
        }
    }
}

/// The [`IpcError`] for a failed read of a file the user chose (a project):
/// a missing file is `notFound`, a file over the limit `payloadTooLarge`.
pub(crate) fn project_read_error(path: &Path, error: &ReadError) -> IpcError {
    tracing::debug!(path = %path.display(), error = %error, "cannot read a project file");
    match error {
        ReadError::NotFound => IpcError::NotFound,
        ReadError::TooLarge { .. } => IpcError::too_large(b2c_ipc::limits::MAX_DOCUMENT_BYTES),
        ReadError::NotAFile => IpcError::Io { kind: IoKind::Other },
        ReadError::Io(source) => IpcError::io(source),
    }
}

/// The [`IpcError`] for a failed I/O operation on `path`; the details are
/// logged at debug level.
pub(crate) fn io_error(action: &'static str, path: &Path, error: &io::Error) -> IpcError {
    tracing::debug!(action, path = %path.display(), error = %error, "I/O failed");
    IpcError::io(error)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn store_errors_keep_only_their_class() {
        let path = PathBuf::from("/home/someone/secret/game.b2c");
        let io = StoreError::Io {
            action: "replace the file",
            path: path.clone(),
            source: io::ErrorKind::PermissionDenied.into(),
        };
        assert_eq!(
            store_error("save", &io),
            IpcError::Io {
                kind: IoKind::PermissionDenied
            }
        );
        assert_eq!(
            store_error("save", &StoreError::Link { path }),
            IpcError::Io { kind: IoKind::Other }
        );
        assert_eq!(
            store_error("save", &StoreError::Invalid("rule")),
            IpcError::Internal
        );
        assert_eq!(
            store_error("save", &StoreError::PathNotUnicode),
            IpcError::Io { kind: IoKind::Other }
        );
        assert_eq!(
            store_error(
                "save",
                &StoreError::Random {
                    source: io::Error::other("x")
                }
            ),
            IpcError::Internal
        );
    }

    #[test]
    fn read_errors_of_projects() {
        let path = Path::new("/p/game.b2c");
        assert_eq!(project_read_error(path, &ReadError::NotFound), IpcError::NotFound);
        assert_eq!(
            project_read_error(path, &ReadError::TooLarge { limit: 1 }),
            IpcError::PayloadTooLarge { limit: 33_554_432 }
        );
        assert_eq!(
            project_read_error(path, &ReadError::NotAFile),
            IpcError::Io { kind: IoKind::Other }
        );
        assert_eq!(
            project_read_error(path, &ReadError::Io(io::ErrorKind::PermissionDenied.into())),
            IpcError::Io {
                kind: IoKind::PermissionDenied
            }
        );
        assert_eq!(
            io_error("x", path, &io::ErrorKind::NotFound.into()),
            IpcError::Io {
                kind: IoKind::NotFound
            }
        );
    }

    #[test]
    fn start_errors_name_the_folder() {
        assert_eq!(
            StartError::RelativeFolder("cache").to_string(),
            "the cache folder is not an absolute path"
        );
    }
}
