//! Explicit limits on everything that crosses the IPC boundary
//! (`docs/spec/02-architecture.md` §2.5, `docs/spec/08-security.md` §8.8).
//!
//! The request decoder ([`crate::decode()`]) enforces the size and range limits; the
//! backend services enforce the rate and session limits. The isolation hook checks
//! the same sizes in the webview (`apps/desktop/src-tauri/isolation/`), but the
//! backend never relies on it.

use std::ops::RangeInclusive;

/// The largest project document a request may carry, in UTF-8 bytes: the same
/// limit as project files and clipboard pastes (`docs/spec/05-project-format.md` §5.6).
pub const MAX_DOCUMENT_BYTES: usize = b2c_model::limits::MAX_FILE_BYTES;

/// The most bytes one `run_input` call may send to a running program (64 KiB).
pub const MAX_RUN_INPUT_BYTES: usize = 65_536;

/// The longest base64 text of a `run_input` call: [`MAX_RUN_INPUT_BYTES`] encoded
/// with padding.
pub const MAX_RUN_INPUT_BASE64: usize = MAX_RUN_INPUT_BYTES.div_ceil(3) * 4;

/// The terminal widths a run may have, in columns.
pub const RUN_COLS: RangeInclusive<u16> = 2..=1000;

/// The terminal heights a run may have, in rows.
pub const RUN_ROWS: RangeInclusive<u16> = 1..=1000;

/// The most project handles open at the same time (`tooManyHandles` above it).
pub const MAX_OPEN_HANDLES: usize = 32;

/// The most programs running at the same time, app-wide (`tooManySessions` above it).
pub const MAX_CONCURRENT_RUNS: usize = 8;

/// The most `run_input` calls per second and run (`rateLimited` above it).
pub const RUN_INPUT_CALLS_PER_SEC: u32 = 200;

/// The most input bytes per second and run (`rateLimited` above it).
pub const RUN_INPUT_BYTES_PER_SEC: usize = 1_048_576;

/// The shortest time between two `trust_grant` calls for one handle, in milliseconds.
pub const TRUST_GRANT_INTERVAL_MS: u64 = 2_000;

/// The most entries in the recent-projects list.
pub const RECENT_MAX: usize = 10;

/// The longest time program output waits before it is sent as a batch, in milliseconds.
pub const OUTPUT_BATCH_MS: u64 = 16;

/// The most output bytes sent but not yet acknowledged with `run_ack`. Above it the
/// backend keeps only the tail of the output and reports the skipped lines.
pub const OUTPUT_UNACKED_MAX: usize = 4_194_304;

/// The console scrollback sizes the settings allow, in lines.
pub const SCROLLBACK_LINES: RangeInclusive<u32> = 1_000..=100_000;

/// The largest integer JavaScript represents exactly (`Number.MAX_SAFE_INTEGER`):
/// the upper bound of every counter the webview sends back, such as `run_ack.seq`.
pub const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_match_the_contract() {
        assert_eq!(MAX_DOCUMENT_BYTES, 33_554_432);
        assert_eq!(MAX_RUN_INPUT_BASE64, 87_384);
        assert_eq!(MAX_SAFE_INTEGER, 9_007_199_254_740_991);
        assert_eq!((*RUN_COLS.start(), *RUN_COLS.end()), (2, 1000));
        assert_eq!((*RUN_ROWS.start(), *RUN_ROWS.end()), (1, 1000));
        assert_eq!(
            (*SCROLLBACK_LINES.start(), *SCROLLBACK_LINES.end()),
            (1_000, 100_000)
        );
    }
}
