//! Cancelling runs from another thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Stops one or more runs from another thread (the *Stop* button, a newer
/// build replacing an older one, closing a project).
///
/// Cloning gives another handle to the same token. Cancelling is permanent:
/// a cancelled token cancels every run it is attached to, including runs
/// that start later (those return [`crate::ProcessError::Cancelled`] without
/// starting anything). A running process is stopped within about 25 ms.
///
/// ```
/// use b2c_process::CancelToken;
///
/// let token = CancelToken::new();
/// let for_stop_button = token.clone();
/// assert!(!token.is_cancelled());
/// for_stop_button.cancel();
/// assert!(token.is_cancelled());
/// ```
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    cancelled: Arc<AtomicBool>,
}

impl CancelToken {
    /// A new token that is not cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancels every run attached to this token.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Whether [`CancelToken::cancel`] has been called.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}
