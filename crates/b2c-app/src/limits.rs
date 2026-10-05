//! The bounds the backend enforces itself, whatever the webview does
//! (`docs/spec/02-architecture.md` §2.5.6 and §2.6, `docs/spec/08-security.md`
//! §8.8 and §8.12).
//!
//! The size and range limits of requests are checked when they are decoded
//! (`b2c_ipc::decode`); this module holds the limits on state: open projects,
//! native dialogs, trust prompts, and the text the backend shows in native
//! dialogs. The run limits (8 programs, input rates) are enforced by
//! [`b2c_build::RunSessions`].

use std::time::Duration;

pub use b2c_ipc::limits::{MAX_CONCURRENT_RUNS, MAX_DOCUMENT_BYTES, MAX_OPEN_HANDLES, RECENT_MAX};

/// The shortest time between two `trust_grant` calls for one project
/// (`rateLimited` otherwise).
pub const TRUST_GRANT_INTERVAL: Duration = Duration::from_millis(b2c_ipc::limits::TRUST_GRANT_INTERVAL_MS);

/// The most characters of a project name, folder or library name shown in a
/// native dialog; longer text is cut and ends with `…`.
pub const MAX_DIALOG_TEXT_CHARS: usize = 120;

/// The most library names the trust dialog lists; the rest are counted.
pub const MAX_DIALOG_LIBRARIES: usize = 10;

/// The most characters of the file name (without `.b2c`) suggested by the
/// save dialog.
pub const MAX_SUGGESTED_STEM_CHARS: usize = 64;

/// How long shutdown waits for cancelled builds to end (their compilers get
/// 2 s to stop before they are killed).
pub const SHUTDOWN_BUILD_WAIT: Duration = Duration::from_secs(3);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_match_the_contract() {
        assert_eq!(MAX_OPEN_HANDLES, 32);
        assert_eq!(MAX_CONCURRENT_RUNS, 8);
        assert_eq!(TRUST_GRANT_INTERVAL, Duration::from_secs(2));
        assert_eq!(MAX_DOCUMENT_BYTES, 33_554_432);
        assert_eq!(RECENT_MAX, b2c_store::recent::MAX_RECENT);
    }
}
