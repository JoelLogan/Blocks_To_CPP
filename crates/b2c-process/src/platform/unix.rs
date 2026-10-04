//! Unix containment: process groups and signals (no `unsafe` needed).

use std::io;
use std::os::unix::process::CommandExt as _;
use std::process::Child;

use rustix::process::{Pid, Signal, kill_process, kill_process_group};

use super::Placement;
use crate::command::Limits;

/// Applies the placement before spawning.
pub(crate) fn configure(command: &mut std::process::Command, placement: Placement, _captured: bool) {
    if placement == Placement::NewGroup {
        // The child calls `setpgid(0, 0)` before `exec`, so it and every
        // descendant that does not deliberately leave start in a group whose
        // ID is the child's PID.
        command.process_group(0);
    }
}

/// The child's process tree.
#[derive(Debug)]
pub(crate) struct Tree {
    pid: Pid,
    placement: Placement,
}

/// Puts a freshly spawned child under control.
///
/// # Errors
/// Fails if the memory limit cannot be applied.
pub(crate) fn contain(child: &mut Child, placement: Placement, limits: &Limits) -> io::Result<Tree> {
    let pid = Pid::from_child(child);
    if let Some(bytes) = limits.memory {
        apply_memory_limit(pid, bytes)?;
    }
    Ok(Tree { pid, placement })
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn apply_memory_limit(pid: Pid, bytes: u64) -> io::Result<()> {
    use rustix::process::{Resource, Rlimit, prlimit};
    // Lowering both limits is always allowed for our own child. Its
    // descendants inherit them when they are created.
    prlimit(
        Some(pid),
        Resource::As,
        Rlimit {
            current: Some(bytes),
            maximum: Some(bytes),
        },
    )?;
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn apply_memory_limit(_pid: Pid, _bytes: u64) -> io::Result<()> {
    // No `prlimit` on this system: documented as not enforced.
    Ok(())
}

/// Whether the child has exited, leaving it a zombie (not reaped) so its PID
/// and process group ID stay reserved until [`Tree::after_exit`] has run.
pub(crate) fn has_exited(child: &mut Child) -> io::Result<bool> {
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "freebsd",
        target_os = "macos"
    ))]
    {
        use rustix::process::{WaitId, WaitIdOptions, waitid};
        let status = waitid(
            WaitId::Pid(Pid::from_child(child)),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        )?;
        Ok(status.is_some())
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "freebsd",
        target_os = "macos"
    )))]
    {
        // Fallback: reaps, after which std keeps the status for `wait`.
        Ok(child.try_wait()?.is_some())
    }
}

impl Tree {
    /// Asks the whole tree to stop (`SIGTERM`).
    pub(crate) fn stop(&self) {
        self.signal(Signal::TERM);
    }

    /// Kills the whole tree (`SIGKILL`).
    pub(crate) fn kill(&self) {
        match self.placement {
            Placement::NewGroup => {
                // ESRCH (nothing left) is fine.
                let _ = kill_process_group(self.pid, Signal::KILL);
            }
            Placement::SharedGroup => kill_descendants(self.pid),
        }
    }

    /// Cleans up after the child itself has exited (but before it is
    /// reaped): anything it left running in its process group is killed.
    /// Leftovers of a shared-group child cannot be found any more (they were
    /// re-parented when it exited) and are left alone.
    pub(crate) fn after_exit(&self) {
        if self.placement == Placement::NewGroup {
            let _ = kill_process_group(self.pid, Signal::KILL);
        }
    }

    /// How many processes are in the tree, where that can be counted (Linux,
    /// new process group).
    pub(crate) fn process_count(&self) -> Option<usize> {
        #[cfg(target_os = "linux")]
        {
            if self.placement == Placement::NewGroup {
                return Some(crate::procfs::count_group(self.pid.as_raw_pid()));
            }
        }
        None
    }

    fn signal(&self, signal: Signal) {
        match self.placement {
            Placement::NewGroup => {
                let _ = kill_process_group(self.pid, signal);
            }
            Placement::SharedGroup => {
                #[cfg(target_os = "linux")]
                for pid in crate::procfs::descendants(self.pid.as_raw_pid()) {
                    if let Some(pid) = Pid::from_raw(pid) {
                        let _ = kill_process(pid, signal);
                    }
                }
                let _ = kill_process(self.pid, signal);
            }
        }
    }
}

/// Kills the child and every descendant that can be found, without a race
/// against new processes: each process found is frozen with `SIGSTOP` (a
/// stopped process cannot fork) and the search repeats until it finds
/// nothing new; then everything frozen is killed.
fn kill_descendants(root: Pid) {
    let _ = kill_process(root, Signal::STOP);
    #[cfg(target_os = "linux")]
    {
        let mut frozen: Vec<Pid> = Vec::new();
        // Bounded: each round either finds new processes or ends the loop,
        // and a fork bomb cannot outpace freezing for long.
        for _ in 0..64 {
            let found: Vec<Pid> = crate::procfs::descendants(root.as_raw_pid())
                .into_iter()
                .filter_map(Pid::from_raw)
                .filter(|pid| !frozen.contains(pid))
                .collect();
            if found.is_empty() {
                break;
            }
            for &pid in &found {
                let _ = kill_process(pid, Signal::STOP);
            }
            frozen.extend(found);
        }
        for pid in frozen {
            let _ = kill_process(pid, Signal::KILL);
        }
    }
    let _ = kill_process(root, Signal::KILL);
}
