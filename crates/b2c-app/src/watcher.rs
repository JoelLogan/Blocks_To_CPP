//! Watching open project files for changes made outside the app
//! (`docs/spec/05-project-format.md` §5.10 "External change detection",
//! `docs/spec/08-security.md` §8.3 "Re-check on outside change").
//!
//! STUB until the package `w4-recovery-watcher`: [`Watchers`] records what
//! should be watched but watches nothing. The backend already calls it at
//! every point where a file starts or stops being watched (open, reload,
//! save, save as, close and shutdown), so the real watcher only fills in this
//! module. Until then the hash check before every save
//! ([`crate::Backend::project_save`] refuses with `changedOnDisk`) is the
//! only protection against overwriting an outside change.
//!
//! The real watcher watches the parent folder (editors replace files by
//! renaming), debounces 300 ms per handle, and sends
//! `projectChangedOnDisk { handle, deleted }` on the app channel only when
//! the SHA-256 of the file differs from the baseline it was given, so the
//! app's own saves never trigger it.

use std::path::Path;
use std::sync::Arc;

use b2c_ipc::Handle;

use crate::events::AppEvents;

/// The file watchers of the open projects (a no-op stub; see the module
/// documentation).
#[derive(Debug)]
pub(crate) struct Watchers {
    /// Where the real watcher sends `projectChangedOnDisk`.
    #[allow(dead_code)] // Used by the real watcher (w4-recovery-watcher).
    events: Arc<AppEvents>,
}

impl Watchers {
    /// Watchers that report on `events`.
    pub(crate) fn new(events: Arc<AppEvents>) -> Self {
        Self { events }
    }

    /// Starts watching `path` for `handle`, or replaces what was watched for
    /// it, with `baseline` (the SHA-256 of the file as the app last read or
    /// wrote it).
    #[allow(clippy::unused_self)] // A stub until w4-recovery-watcher.
    pub(crate) fn watch(&self, handle: &Handle, path: &Path, baseline: [u8; 32]) {
        let _ = (handle, path, baseline);
    }

    /// Stops watching for `handle` (nothing happens when it was not watched).
    #[allow(clippy::unused_self)] // A stub until w4-recovery-watcher.
    pub(crate) fn unwatch(&self, handle: &Handle) {
        let _ = handle;
    }

    /// Stops every watcher (shutdown).
    #[allow(clippy::unused_self)] // A stub until w4-recovery-watcher.
    pub(crate) fn stop_all(&self) {}
}
