//! Unix containment: process groups and signals, plus (Linux) the cgroup v2
//! scope and the watchdogs of `src/containment/`, and the fallback's
//! address-space limit, which the child sets on itself before `exec`
//! ([`limit_address_space`]). That limit's `pre_exec` hook is the only
//! `unsafe` code here.

use std::io;
use std::os::unix::process::CommandExt as _;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};

use rustix::process::{Pid, Signal, kill_process, kill_process_group};
#[cfg(any(target_os = "linux", target_os = "android"))]
use rustix::process::{Resource, Rlimit};

use super::Placement;
use crate::containment::{Breach, TreeSpec};
use crate::status::ExitStatus;

/// Applies the placement before spawning.
pub(crate) fn configure(command: &mut std::process::Command, placement: Placement, _captured: bool) {
    if placement == Placement::NewGroup {
        // The child calls `setpgid(0, 0)` before `exec`, so it and every
        // descendant that does not deliberately leave start in a group whose
        // ID is the child's PID.
        command.process_group(0);
    }
}

/// What the watchdog checks (see [`crate::containment::Enforcement`]).
#[derive(Debug, Clone, Copy, Default)]
struct Watch {
    /// The RSS watchdog's limit for the tree.
    rss: Option<u64>,
    /// The process-count watchdog's limit.
    processes: Option<u32>,
    /// Stop when the scope's `pids.events` reports a refused task.
    pids_events: bool,
    /// Stop when the scope's `memory.events` reports running out of memory.
    oom_events: bool,
}

/// The child's process tree: its process group (or, for a child in this
/// process's group, the child and its descendants), and on Linux its cgroup
/// scope when it has one.
#[derive(Debug)]
pub(crate) struct Tree {
    pid: Pid,
    placement: Placement,
    watch: Watch,
    #[cfg(target_os = "linux")]
    scope: Option<crate::containment::cgroup::Scope>,
    /// Set once a watchdog or a memory event found the tree out of memory.
    out_of_memory: AtomicBool,
    /// Set once the scope's `pids.events` showed a refused task.
    process_limit_hit: AtomicBool,
}

/// A started child, owned by its supervisor.
#[derive(Debug)]
pub(crate) struct Process(Child);

impl Process {
    /// Wraps a child started by the standard library.
    pub(crate) fn from_std(child: Child) -> Self {
        Self(child)
    }

    /// Whether the child has exited, without reaping it ([`has_exited`]).
    pub(crate) fn has_exited(&mut self) -> io::Result<bool> {
        has_exited(&mut self.0)
    }

    /// Waits for the child and reaps it.
    pub(crate) fn wait(&mut self) -> io::Result<ExitStatus> {
        self.0.wait().map(ExitStatus::from_std)
    }

    /// Kills the child itself (not its tree) and reaps it, after a setup
    /// failure.
    pub(crate) fn discard(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Gives the child the fallback's address-space limit (`RLIMIT_AS`) before
/// it runs the program, when `bytes` is set (Linux; see
/// [`crate::containment::Enforcement::address_space`]). Call it on every
/// command spawned with such a limit, before spawning.
///
/// The child sets the limit on itself between `fork` and `exec`, so the
/// program starts with it and every process it creates inherits it, however
/// early. (Set from this process after the spawn, a process the compiler
/// forked straight away could start before the limit and escape it.) The
/// limit never loosens one the child would inherit anyway: the soft and the
/// hard limit each become the smaller of this process's own and `bytes`
/// ([`address_space_limit`]). If the child cannot set it, the spawn fails.
///
/// Other Unix systems do not enforce the limit (documented on
/// [`crate::Limits`]).
#[cfg(any(target_os = "linux", target_os = "android"))]
#[allow(unsafe_code)] // `pre_exec`; see the SAFETY comment.
pub(crate) fn limit_address_space(command: &mut std::process::Command, bytes: Option<u64>) {
    let Some(bytes) = bytes else {
        return;
    };
    let limit = address_space_limit(bytes, rustix::process::getrlimit(Resource::As));
    let set_limit = move || {
        rustix::process::setrlimit(Resource::As, limit)
            .map_err(|errno| io::Error::from_raw_os_error(errno.raw_os_error()))
    };
    // SAFETY: the closure runs in the child between `fork` and `exec`, where
    // only async-signal-safe operations are allowed. It makes one system
    // call, `prlimit64(0, RLIMIT_AS, &limit, NULL)`, which rustix (its
    // `linux_raw` backend, the one this workspace builds) issues directly,
    // without locks or allocation. It captures only `limit`, a `Copy` value
    // computed before the fork, touches no shared state, and builds its
    // error with `io::Error::from_raw_os_error`, which does not allocate.
    // Lowering a resource limit needs no privilege, and `limit` is never
    // above the limits the child inherited, so the call can only fail on a
    // kernel without `prlimit64` (before 2.6.36), and the spawn then fails
    // with that error.
    unsafe {
        command.pre_exec(set_limit);
    }
}

/// Elsewhere the address-space limit is not enforced (see
/// [`crate::Limits`]).
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub(crate) fn limit_address_space(_command: &mut std::process::Command, _bytes: Option<u64>) {}

/// The address-space limit a child gets for a requested limit of `bytes`,
/// given the limit it would inherit (`inherited`, this process's own): the
/// soft and the hard limit are each the smaller of the inherited one
/// (`None`: unlimited) and `bytes`. So the child is never allowed more than
/// it would have had without the request, and lowering both needs no
/// privilege.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn address_space_limit(bytes: u64, inherited: Rlimit) -> Rlimit {
    let capped = |inherited: Option<u64>| Some(inherited.map_or(bytes, |inherited| inherited.min(bytes)));
    Rlimit {
        current: capped(inherited.current),
        maximum: capped(inherited.maximum),
    }
}

/// Puts a freshly spawned child under control as `spec` says: the
/// watchdogs, and on Linux its cgroup scope. (The address-space limit is
/// already in place: the child set it before `exec`,
/// [`limit_address_space`].)
///
/// # Errors
/// Never on Unix; the signature is shared with Windows, where assigning the
/// Job Object can fail.
#[allow(clippy::unnecessary_wraps)] // same signature as the Windows version
pub(crate) fn contain(child: &mut Child, placement: Placement, spec: TreeSpec) -> io::Result<Tree> {
    let pid = Pid::from_child(child);
    let enforcement = spec.enforcement;
    Ok(Tree {
        pid,
        placement,
        watch: Watch {
            rss: enforcement.rss_watch,
            processes: enforcement.count_processes,
            pids_events: enforcement.pids_events,
            oom_events: enforcement.oom_events,
        },
        #[cfg(target_os = "linux")]
        scope: spec.scope,
        out_of_memory: AtomicBool::new(false),
        process_limit_hit: AtomicBool::new(false),
    })
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
    /// Asks the whole tree to stop (`SIGTERM` to the process group; the
    /// scope's other processes are killed by [`Tree::kill`] afterwards).
    pub(crate) fn stop(&self) {
        self.signal(Signal::TERM);
    }

    /// Kills the whole tree: the scope (`cgroup.kill`) and the process group
    /// (`SIGKILL`), which until `systemd-run` has moved into the scope is
    /// the only place it is.
    pub(crate) fn kill(&self) {
        self.kill_scope();
        match self.placement {
            Placement::NewGroup => {
                // ESRCH (nothing left) is fine.
                let _ = kill_process_group(self.pid, Signal::KILL);
            }
            Placement::SharedGroup => kill_descendants(self.pid),
        }
    }

    /// Cleans up after the child itself has exited (but before it is
    /// reaped): anything it left running in its scope or process group is
    /// killed. Leftovers of a shared-group child outside a scope cannot be
    /// found any more (they were re-parented when it exited) and are left
    /// alone. The scope's events are read first, while it still exists (a
    /// fork bomb whose leader gave up leaves its processes in it).
    pub(crate) fn after_exit(&self) {
        self.note_scope_events();
        self.kill_scope();
        if self.placement == Placement::NewGroup {
            let _ = kill_process_group(self.pid, Signal::KILL);
        }
    }

    /// Whether [`Tree::watchdog`] has anything to check.
    pub(crate) fn watches(&self) -> bool {
        let watch = self.watch;
        watch.rss.is_some() || watch.processes.is_some() || watch.pids_events || watch.oom_events
    }

    /// One look at the tree for the supervisor (every 100 ms while it runs):
    /// whether it has gone over its process or memory limit. The supervisor
    /// then kills it.
    pub(crate) fn watchdog(&self) -> Option<Breach> {
        #[cfg(target_os = "linux")]
        {
            if let Some(scope) = &self.scope {
                if self.watch.oom_events && scope.oom_seen() {
                    self.out_of_memory.store(true, Ordering::Relaxed);
                    return Some(Breach::Memory);
                }
                if self.watch.pids_events && scope.tasks_max_hit() {
                    return Some(Breach::Processes);
                }
            }
            if self.watch.rss.is_none() && self.watch.processes.is_none() {
                return None;
            }
            let members = self.members();
            if let Some(max) = self.watch.processes
                && members.countable
                && members.pids.len() > usize::try_from(max).unwrap_or(usize::MAX)
            {
                return Some(Breach::Processes);
            }
            if let Some(limit) = self.watch.rss
                && crate::containment::rss::total(&members.pids) > limit
            {
                self.out_of_memory.store(true, Ordering::Relaxed);
                return Some(Breach::Memory);
            }
        }
        None
    }

    /// Whether the tree hit its process limit without a watchdog stopping
    /// it (the scope's `TasksMax` refused a task and the program then ended
    /// by itself), as noted by [`Tree::after_exit`].
    pub(crate) fn process_limit_hit(&self) -> bool {
        self.process_limit_hit.load(Ordering::Relaxed)
    }

    /// Whether the tree ran out of memory, decided after the child's exit
    /// (`failed`: it did not exit with code 0): a watchdog or scope event
    /// saw it, or the scope's events say so.
    pub(crate) fn out_of_memory(&self, failed: bool) -> bool {
        if self.out_of_memory.load(Ordering::Relaxed) {
            return true;
        }
        #[cfg(target_os = "linux")]
        {
            if self.watch.oom_events
                && let Some(scope) = &self.scope
            {
                return scope.final_out_of_memory(failed);
            }
        }
        let _ = failed;
        false
    }

    /// The processes the watchdogs look at (Linux).
    #[cfg(target_os = "linux")]
    fn members(&self) -> Members {
        if let Some(scope) = &self.scope {
            return Members {
                pids: scope.pids(),
                countable: true,
            };
        }
        let root = self.pid.as_raw_pid();
        match self.placement {
            Placement::NewGroup => Members {
                pids: crate::procfs::group_members(root),
                countable: true,
            },
            // This process's own group: only the child and its descendants
            // are the tree, and they are not counted against a process
            // limit (see `crate::Limits`).
            Placement::SharedGroup => {
                let mut pids = crate::procfs::descendants(root);
                pids.push(root);
                Members {
                    pids,
                    countable: false,
                }
            }
        }
    }

    /// Records what the scope's event files say, while it exists.
    #[cfg(target_os = "linux")]
    fn note_scope_events(&self) {
        if let Some(scope) = &self.scope {
            if self.watch.oom_events && scope.oom_seen() {
                self.out_of_memory.store(true, Ordering::Relaxed);
            }
            if self.watch.pids_events && scope.tasks_max_hit() {
                self.process_limit_hit.store(true, Ordering::Relaxed);
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    #[allow(clippy::unused_self)] // same signature as the Linux version
    fn note_scope_events(&self) {}

    #[cfg(target_os = "linux")]
    fn kill_scope(&self) {
        if let Some(scope) = &self.scope {
            scope.kill();
        }
    }

    #[cfg(not(target_os = "linux"))]
    #[allow(clippy::unused_self)] // same signature as the Linux version
    fn kill_scope(&self) {}

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

/// The processes of a tree, for the watchdogs.
#[cfg(target_os = "linux")]
struct Members {
    /// The processes of the tree.
    pids: Vec<i32>,
    /// Whether they are counted against a process limit.
    countable: bool,
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

#[cfg(all(test, any(target_os = "linux", target_os = "android")))]
mod tests {
    use std::process::Stdio;

    use rustix::process::{Resource, Rlimit, getrlimit};

    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn the_limit_never_loosens_an_inherited_one() {
        let unlimited = Rlimit {
            current: None,
            maximum: None,
        };
        assert_eq!(
            address_space_limit(4 * GIB, unlimited),
            Rlimit {
                current: Some(4 * GIB),
                maximum: Some(4 * GIB),
            }
        );
        // A lower inherited soft limit stays; the hard limit comes down.
        let soft = Rlimit {
            current: Some(GIB),
            maximum: None,
        };
        assert_eq!(
            address_space_limit(4 * GIB, soft),
            Rlimit {
                current: Some(GIB),
                maximum: Some(4 * GIB),
            }
        );
        // Both inherited limits lower: nothing changes (raising the hard
        // limit would need privilege and fail).
        let both = Rlimit {
            current: Some(GIB),
            maximum: Some(2 * GIB),
        };
        assert_eq!(address_space_limit(4 * GIB, both), both);
        // Higher inherited limits come down to the request.
        let high = Rlimit {
            current: Some(8 * GIB),
            maximum: Some(16 * GIB),
        };
        assert_eq!(
            address_space_limit(4 * GIB, high),
            Rlimit {
                current: Some(4 * GIB),
                maximum: Some(4 * GIB),
            }
        );
    }

    /// What `/bin/sh -c script` prints when spawned with `bytes` as its
    /// address-space limit, without [`contain`] ever running: the limit
    /// must already be there when the program starts.
    #[allow(clippy::unwrap_used)] // a test helper fails the test by panicking
    fn output_with_limit(script: &str, bytes: Option<u64>) -> String {
        let mut command = crate::Command::new("/bin/sh", std::env::temp_dir()).unwrap();
        command.args(["-c", script]).env("PATH", "/usr/bin:/bin");
        let mut std_command = command.to_std();
        std_command.stdin(Stdio::null()).stderr(Stdio::inherit());
        limit_address_space(&mut std_command, bytes);
        let output = std_command.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    }

    #[test]
    fn the_child_starts_with_the_limit_and_its_children_inherit_it() {
        let bytes = 768 * 1024 * 1024;
        let kib = (bytes / 1024).to_string();
        // `ulimit -v` reads the soft limit in KiB; the subshell is a process
        // the program forks before doing anything else.
        let output = output_with_limit("ulimit -v; (ulimit -H -v)", Some(bytes));
        let lines: Vec<&str> = output.lines().collect();
        let inherited = getrlimit(Resource::As);
        let expected =
            |limit: Option<u64>| limit.map_or(kib.clone(), |limit| (limit.min(bytes) / 1024).to_string());
        assert_eq!(
            lines,
            [expected(inherited.current), expected(inherited.maximum)],
            "{output}"
        );
    }

    #[test]
    fn without_a_limit_nothing_is_set() {
        let inherited = getrlimit(Resource::As);
        let shown = |limit: Option<u64>| {
            limit.map_or_else(|| "unlimited".to_owned(), |limit| (limit / 1024).to_string())
        };
        assert_eq!(
            output_with_limit("ulimit -v", None).trim(),
            shown(inherited.current)
        );
    }
}
