//! Platform-specific process containment: process groups on Unix, Job
//! Objects on Windows.
//!
//! Each platform module provides the same items:
//!
//! * `configure(&mut std::process::Command, Placement, captured)`: settings
//!   applied before spawning (new process group; created suspended).
//! * `contain(&mut Child, Placement, &Limits) -> io::Result<Tree>`: called
//!   right after spawning; applies limits and puts the child under control
//!   (on Windows: assigns the job, then resumes the child).
//! * `has_exited(&mut Child) -> io::Result<bool>`: whether the child has
//!   exited, *without* reaping it, so its ID cannot be reused while the tree
//!   is still being cleaned up.
//! * `Tree::{stop, kill, after_exit, process_count}`. A `Tree` is `Send +
//!   Sync`, so a session's supervisor thread and its owner can share it
//!   (`src/pty/session.rs`).
//!
//! Windows also provides `job_tree(&Limits)` and `Tree::assign`, for the
//! sessions of `src/pty/windows.rs`, which create the child with
//! `CreateProcessW` themselves.

/// Where the child goes relative to this process's process group (resolved
/// from [`crate::ProcessGroup`]). Ignored on Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placement {
    /// A new process group led by the child.
    NewGroup,
    /// This process's own process group (terminal-attached runs).
    SharedGroup,
}

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{Tree, configure, contain, has_exited};

// Win32 calls for Job Objects. The other `unsafe` code of Blocks2Cpp is in
// the session modules of `src/pty/`.
#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{Tree, configure, contain, has_exited, job_tree};
