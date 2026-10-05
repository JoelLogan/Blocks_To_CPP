//! Platform-specific process containment: process groups (and on Linux
//! cgroup scopes and watchdogs) on Unix, Job Objects on Windows. What is used
//! for a spawn is decided beforehand by [`crate::containment::plan`].
//!
//! Each platform module provides the same items:
//!
//! * `configure(&mut std::process::Command, Placement, captured)`: settings
//!   applied before spawning with the standard library (new process group;
//!   created suspended).
//! * `contain(&mut Child, Placement, TreeSpec) -> io::Result<Tree>`: called
//!   right after spawning; applies limits and puts the child under control
//!   (on Windows: assigns the job, then resumes the child).
//! * `has_exited(&mut Child) -> io::Result<bool>` (Unix; Windows uses it
//!   inside `Process`): whether the child has exited, *without* reaping it,
//!   so its ID cannot be reused while the tree is still being cleaned up.
//! * `Process`: a started child as its supervisor owns it (`has_exited`,
//!   `wait`, `discard`).
//! * `Tree::{stop, kill, after_exit, watches, watchdog, out_of_memory}`. A
//!   `Tree` is `Send + Sync`, so a session's supervisor thread and its owner
//!   can share it (`src/pty/session.rs`).
//!
//! Windows also provides `job_tree(&Enforcement)` and `Tree::assign`, for
//! processes created suspended with `CreateProcessW` (`create.rs`), and
//! `spawn_captured`, the captured runs' spawn with an explicit handle list.

/// Where the child goes relative to this process's process group (resolved
/// from [`crate::ProcessGroup`]). Ignored on Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placement {
    /// A new process group led by the child.
    NewGroup,
    /// This process's own process group (terminal-attached runs).
    SharedGroup,
}

#[cfg(any(windows, test))]
pub(crate) mod cmdline;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{Process, Tree, configure, contain, has_exited};

// Win32 calls for Job Objects and process creation. The other `unsafe` code
// of Blocks2Cpp is in the session modules of `src/pty/` and in `src/os/`.
#[cfg(windows)]
#[allow(unsafe_code)]
pub(crate) mod create;
#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{Process, Tree, configure, contain, job_tree, spawn_captured};
