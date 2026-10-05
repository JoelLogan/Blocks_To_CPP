//! How completely a process tree is contained, and the plan for containing
//! one spawn (`docs/spec/07-toolchain-build-run.md` §7.5.2, §7.6.2;
//! `docs/spec/08-security.md` §8.7, §8.14 item 3;
//! `docs/adr/0008-pty-and-containment-in-b2c-process.md`).
//!
//! * **Windows:** every process runs in a Job Object
//!   ([`ContainmentLevel::JobObject`]); see `platform/windows.rs`.
//! * **Linux:** when the user's service manager can create cgroup v2 scopes
//!   ([`containment_level`] is [`ContainmentLevel::Cgroup`]), captured runs
//!   and sessions are started through `systemd-run --user --scope`, which
//!   puts them in a transient scope and then executes the program in place
//!   (same process ID, same terminal). The scope holds every process the
//!   program starts, including double-forked and `setsid` ones, and is
//!   killed as a whole with `cgroup.kill`. Without scopes, the tree is the
//!   program's process group ([`ContainmentLevel::ProcessGroupOnly`]),
//!   with `RLIMIT_AS` and an RSS watchdog for memory. See `cgroup.rs` and
//!   `rss.rs`.
//!
//! [`plan`] decides, before each spawn, what is started (the command itself
//! or `systemd-run` wrapping it) and which mechanism enforces each limit
//! ([`Enforcement`]); the platform `Tree` then applies it.

use crate::command::{Command, Limits};
use crate::error::ProcessError;

#[cfg(target_os = "linux")]
pub(crate) mod cgroup;
#[cfg(target_os = "linux")]
pub(crate) mod rss;

/// How a run is to be contained, chosen per [`Command`] with
/// [`Command::containment`].
///
/// ```
/// use b2c_process::{Command, Containment};
///
/// # #[cfg(unix)] {
/// let mut command = Command::new("/bin/true", std::env::temp_dir())?;
/// assert_eq!(command.get_containment(), Containment::Auto);
/// command.containment(Containment::ProcessGroupOnly);
/// # }
/// # Ok::<(), b2c_process::ProcessError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Containment {
    /// The strongest containment available. Windows: a Job Object. Linux: a
    /// cgroup v2 scope for [`crate::run_captured`] and the sessions of
    /// [`crate::spawn_pty`] and [`crate::spawn_piped`] when
    /// [`containment_level`] is [`ContainmentLevel::Cgroup`], otherwise the
    /// process group. [`crate::run_interactive`] (the command line's
    /// `b2c run`, attached to a terminal) never uses a scope.
    #[default]
    Auto,
    /// Linux: the process group only, even where scopes are available (this
    /// is how tests exercise the fallback, and how the detection's own trial
    /// run avoids needing a scope). Windows: no effect; a Job Object is
    /// always used.
    ProcessGroupOnly,
}

/// How completely a process tree is contained, for the run status
/// (`docs/spec/08-security.md` §8.14 item 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainmentLevel {
    /// Windows: a Job Object; nothing in the tree can leave it, and closing
    /// the app kills the tree.
    JobObject,
    /// Linux: a cgroup v2 scope, which also holds processes that left the
    /// process group (double-forked or `setsid` children).
    Cgroup,
    /// Linux without a usable cgroup v2 scope: the process group only. A
    /// program that deliberately leaves its group can outlive Stop.
    ProcessGroupOnly,
}

/// The containment this system offers to runs with [`Containment::Auto`].
///
/// Windows always gives [`ContainmentLevel::JobObject`]. Linux gives
/// [`ContainmentLevel::Cgroup`] when all of these hold, and
/// [`ContainmentLevel::ProcessGroupOnly`] otherwise:
///
/// * cgroup v2 is mounted at `/sys/fs/cgroup` (`cgroup.controllers` exists);
/// * `systemd-run` exists at `/usr/bin/systemd-run` or `/bin/systemd-run`
///   (an absolute path, never `PATH`);
/// * `XDG_RUNTIME_DIR` is set to an absolute path;
/// * a trial scope (`systemd-run --user --scope … -- cat /proc/self/cgroup`)
///   succeeds within 5 s and shows where the user's manager puts scopes.
///
/// The check runs once per process, on the first call (which can therefore
/// take up to about 5 s; later calls return at once); the app makes that
/// call on a background thread at startup. Other Unix systems give
/// [`ContainmentLevel::ProcessGroupOnly`].
///
/// ```
/// use b2c_process::{ContainmentLevel, containment_level};
///
/// let level = containment_level();
/// if cfg!(windows) {
///     assert_eq!(level, ContainmentLevel::JobObject);
/// } else {
///     assert_ne!(level, ContainmentLevel::JobObject);
/// }
/// ```
pub fn containment_level() -> ContainmentLevel {
    #[cfg(windows)]
    {
        ContainmentLevel::JobObject
    }
    #[cfg(target_os = "linux")]
    {
        if cgroup::detected().is_some() {
            ContainmentLevel::Cgroup
        } else {
            ContainmentLevel::ProcessGroupOnly
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        ContainmentLevel::ProcessGroupOnly
    }
}

/// Kills what is left of earlier app instances: every `b2c-*` scope of the
/// user's service manager whose owner (the process ID in its name,
/// `b2c-<kind>-<ownerPid>-<16 hex>.scope`) no longer runs, by writing `1` to
/// its `cgroup.kill` (falling back to freezing it and killing each of its
/// processes). Scopes of this process and of instances that still run are
/// left alone. Returns how many scopes were killed.
///
/// The app calls this once at startup, on a background thread
/// (`docs/spec/08-security.md` §8.14 item 3): a crashed app instance cannot
/// kill its programs itself. It never starts a process, and it looks only
/// below `/sys/fs/cgroup/user.slice/user-<uid>.slice/user@<uid>.service`
/// (and below the folder where this process's own scopes go, once
/// [`containment_level`] has found it), at most a few levels deep. It does
/// nothing on Windows (a Job Object dies with its owner) and on systems
/// without cgroup v2.
pub fn cleanup_stale_scopes() -> usize {
    #[cfg(target_os = "linux")]
    {
        cgroup::cleanup_stale_scopes()
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// What a spawn is, for the scope's name and for whether a scope is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// [`crate::run_captured`]: the compiler, probes (`b2c-build-…` scope).
    Build,
    /// A session (`b2c-run-…` scope).
    Run,
    /// [`crate::run_interactive`]: never a scope.
    Interactive,
}

impl Kind {
    /// The word in the scope's unit name, if this kind of spawn gets one.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn scope_word(self) -> Option<&'static str> {
        match self {
            Self::Build => Some("build"),
            Self::Run => Some("run"),
            Self::Interactive => None,
        }
    }
}

/// Which cgroup controllers the user's service manager can give a scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Controllers {
    /// `memory`: `MemoryMax` is enforced and `memory.events` is reported.
    pub(crate) memory: bool,
    /// `pids`: `TasksMax` is enforced and `pids.events` is reported.
    pub(crate) pids: bool,
}

/// Which mechanism enforces each limit of one spawn (see the table on
/// [`Limits`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Enforcement {
    /// Linux scope: `-p MemoryMax=… -p MemorySwapMax=0`.
    pub(crate) scope_memory_max: Option<u64>,
    /// Linux scope: `-p TasksMax=…`.
    pub(crate) scope_tasks_max: Option<u32>,
    /// Linux: `RLIMIT_AS` on the child (only ever from [`Limits::memory`],
    /// and only when no scope enforces memory).
    pub(crate) address_space: Option<u64>,
    /// Linux: the RSS watchdog's limit for the whole tree.
    pub(crate) rss_watch: Option<u64>,
    /// Linux: the process-count watchdog's limit.
    pub(crate) count_processes: Option<u32>,
    /// Linux scope: stop the run when `pids.events` reports that `TasksMax`
    /// refused a new task.
    pub(crate) pids_events: bool,
    /// Linux scope: report (and stop on) out-of-memory events from
    /// `memory.events`.
    pub(crate) oom_events: bool,
    /// Windows: the Job Object's `JobMemoryLimit`.
    pub(crate) job_memory: Option<u64>,
    /// Windows: the Job Object's `ActiveProcessLimit`.
    pub(crate) job_processes: Option<u32>,
}

impl Enforcement {
    /// The enforcement of `limits` for a spawn in a scope whose manager
    /// offers `scope` controllers, or without a scope (`None`).
    pub(crate) fn new(limits: &Limits, scope: Option<Controllers>) -> Self {
        let memory = min_some(limits.memory, limits.rss_limit);
        let mut enforcement = Self {
            job_memory: memory,
            job_processes: limits.processes,
            ..Self::default()
        };
        let controllers = scope.unwrap_or_default();
        if scope.is_some() {
            // Always passed, so the scope carries the limits even if the
            // manager gains the controller later; whether it is *enforced*
            // decides the watchdogs below.
            enforcement.scope_memory_max = memory;
            enforcement.scope_tasks_max = limits.processes;
        }
        if controllers.memory {
            enforcement.oom_events = memory.is_some();
        } else {
            enforcement.address_space = limits.memory;
            enforcement.rss_watch = memory;
        }
        if controllers.pids {
            enforcement.pids_events = limits.processes.is_some();
        } else {
            enforcement.count_processes = limits.processes;
        }
        enforcement
    }
}

/// The smaller of two optional limits.
fn min_some(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Why a watchdog stopped a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// Windows reports only memory (the job enforces the process limit itself);
// other Unix systems have no watchdog.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) enum Breach {
    /// Too many processes (the count, or `TasksMax` refusing a task).
    Processes,
    /// Over the memory limit (the RSS sum, an out-of-memory event in the
    /// scope, or the Job Object's memory-limit notification).
    Memory,
}

/// What the platform's `Tree` needs to contain one spawn.
#[derive(Debug)]
pub(crate) struct TreeSpec {
    /// Which mechanism enforces each limit.
    pub(crate) enforcement: Enforcement,
    /// The scope the program is started in (Linux).
    #[cfg(target_os = "linux")]
    pub(crate) scope: Option<cgroup::Scope>,
}

/// How one spawn is contained.
#[derive(Debug)]
pub(crate) struct Plan {
    /// What to start: the command itself, or `systemd-run` wrapping it.
    pub(crate) command: Command,
    /// The containment the spawn gets.
    pub(crate) level: ContainmentLevel,
    /// For the platform's `Tree`.
    pub(crate) tree: TreeSpec,
}

/// Works out how to contain `command`, started as `kind`. On Linux this may
/// run the one-time detection of [`containment_level`] (only for
/// [`Containment::Auto`]).
///
/// # Errors
/// [`ProcessError::InvalidCommand`] if the command cannot be wrapped in
/// `systemd-run` (it cannot happen for a command [`Command::new`] accepted;
/// the check is there so nothing panics).
// Only wrapping a command in `systemd-run` (Linux) can fail.
#[cfg_attr(not(target_os = "linux"), allow(clippy::unnecessary_wraps))]
pub(crate) fn plan(command: &Command, kind: Kind) -> Result<Plan, ProcessError> {
    #[cfg(target_os = "linux")]
    {
        if let Some(word) = kind.scope_word()
            && command.get_containment() == Containment::Auto
            && let Some(detected) = cgroup::detected()
        {
            let enforcement = Enforcement::new(command.get_limits(), Some(detected.controllers));
            let scope = cgroup::Scope::new(detected, word);
            let wrapped = scope.wrap(detected, command, &enforcement)?;
            return Ok(Plan {
                command: wrapped,
                level: ContainmentLevel::Cgroup,
                tree: TreeSpec {
                    enforcement,
                    scope: Some(scope),
                },
            });
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = kind;
    Ok(Plan {
        command: command.clone(),
        level: if cfg!(windows) {
            ContainmentLevel::JobObject
        } else {
            ContainmentLevel::ProcessGroupOnly
        },
        tree: TreeSpec {
            enforcement: Enforcement::new(command.get_limits(), None),
            #[cfg(target_os = "linux")]
            scope: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn limits(memory: Option<u64>, rss_limit: Option<u64>, processes: Option<u32>) -> Limits {
        Limits {
            memory,
            rss_limit,
            processes,
            ..Limits::default()
        }
    }

    #[test]
    fn the_fallback_uses_rlimit_as_and_the_watchdogs_for_the_compiler() {
        let compiler = Enforcement::new(&limits(Some(4 * GIB), None, Some(32)), None);
        assert_eq!(
            compiler,
            Enforcement {
                address_space: Some(4 * GIB),
                rss_watch: Some(4 * GIB),
                count_processes: Some(32),
                job_memory: Some(4 * GIB),
                job_processes: Some(32),
                ..Enforcement::default()
            }
        );
    }

    #[test]
    fn a_run_cap_never_becomes_an_address_space_limit() {
        let run = Enforcement::new(&limits(None, Some(256 << 20), None), None);
        assert_eq!(run.address_space, None);
        assert_eq!(run.rss_watch, Some(256 << 20));
        assert_eq!(run.job_memory, Some(256 << 20));
        // Inside a scope without the memory controller, the same.
        let scoped = Enforcement::new(
            &limits(None, Some(256 << 20), None),
            Some(Controllers {
                memory: false,
                pids: true,
            }),
        );
        assert_eq!(scoped.address_space, None);
        assert_eq!(scoped.rss_watch, Some(256 << 20));
        assert_eq!(scoped.scope_memory_max, Some(256 << 20));
        assert!(!scoped.oom_events);
    }

    #[test]
    fn a_scope_with_both_controllers_replaces_the_watchdogs() {
        let all = Controllers {
            memory: true,
            pids: true,
        };
        let compiler = Enforcement::new(&limits(Some(4 * GIB), None, Some(32)), Some(all));
        assert_eq!(
            compiler,
            Enforcement {
                scope_memory_max: Some(4 * GIB),
                scope_tasks_max: Some(32),
                pids_events: true,
                oom_events: true,
                job_memory: Some(4 * GIB),
                job_processes: Some(32),
                ..Enforcement::default()
            }
        );
        // A run without caps gets a scope without limits.
        assert_eq!(
            Enforcement::new(&Limits::default(), Some(all)),
            Enforcement::default()
        );
    }

    #[test]
    fn a_scope_without_controllers_keeps_the_fallback_mechanisms() {
        let compiler = Enforcement::new(
            &limits(Some(4 * GIB), None, Some(32)),
            Some(Controllers::default()),
        );
        assert_eq!(compiler.scope_memory_max, Some(4 * GIB));
        assert_eq!(compiler.scope_tasks_max, Some(32));
        assert_eq!(compiler.address_space, Some(4 * GIB));
        assert_eq!(compiler.rss_watch, Some(4 * GIB));
        assert_eq!(compiler.count_processes, Some(32));
        assert!(!compiler.pids_events && !compiler.oom_events);
    }

    #[test]
    fn the_smaller_memory_limit_wins() {
        let both = Enforcement::new(&limits(Some(GIB), Some(GIB / 2), None), None);
        assert_eq!(both.rss_watch, Some(GIB / 2));
        assert_eq!(both.address_space, Some(GIB));
        assert_eq!(both.job_memory, Some(GIB / 2));
        assert_eq!(min_some(None, None), None);
        assert_eq!(min_some(Some(3), None), Some(3));
        assert_eq!(min_some(None, Some(4)), Some(4));
    }

    #[test]
    fn opting_out_never_needs_a_scope() {
        let root = std::env::temp_dir();
        let program = if cfg!(windows) {
            r"C:\Windows\System32\cmd.exe"
        } else {
            "/bin/sh"
        };
        let mut command = Command::new(program, &root).unwrap();
        command.containment(Containment::ProcessGroupOnly);
        for kind in [Kind::Build, Kind::Run, Kind::Interactive] {
            let plan = plan(&command, kind).unwrap();
            assert_eq!(plan.command.program(), command.program());
            assert_eq!(plan.command.get_args(), command.get_args());
            let expected = if cfg!(windows) {
                ContainmentLevel::JobObject
            } else {
                ContainmentLevel::ProcessGroupOnly
            };
            assert_eq!(plan.level, expected);
        }
        // Interactive runs never get a scope, even with `Auto`.
        command.containment(Containment::Auto);
        let interactive = plan(&command, Kind::Interactive).unwrap();
        assert_eq!(interactive.command.program(), command.program());
        assert_ne!(interactive.level, ContainmentLevel::Cgroup);
    }

    #[test]
    fn the_level_is_stable() {
        assert_eq!(containment_level(), containment_level());
    }

    proptest! {
        /// Whatever the limits and controllers, `RLIMIT_AS` only ever
        /// comes from `memory`, every memory limit is enforced by some
        /// mechanism, and so is every process limit.
        #[test]
        fn every_limit_is_enforced_once_and_rlimit_only_from_memory(
            memory in proptest::option::of(1_u64..u64::MAX),
            rss in proptest::option::of(1_u64..u64::MAX),
            processes in proptest::option::of(1_u32..1000),
            scope in proptest::option::of((any::<bool>(), any::<bool>())),
        ) {
            let controllers = scope.map(|(memory, pids)| Controllers { memory, pids });
            let enforcement = Enforcement::new(&limits(memory, rss, processes), controllers);
            prop_assert!(enforcement.address_space.is_none() || enforcement.address_space == memory);
            let smallest = min_some(memory, rss);
            let memory_by_scope = controllers.is_some_and(|c| c.memory);
            prop_assert_eq!(enforcement.oom_events, memory_by_scope && smallest.is_some());
            prop_assert_eq!(enforcement.rss_watch, if memory_by_scope { None } else { smallest });
            let pids_by_scope = controllers.is_some_and(|c| c.pids);
            prop_assert_eq!(enforcement.pids_events, pids_by_scope && processes.is_some());
            prop_assert_eq!(enforcement.count_processes, if pids_by_scope { None } else { processes });
            prop_assert_eq!(enforcement.job_memory, smallest);
            prop_assert_eq!(enforcement.job_processes, processes);
            if controllers.is_some() {
                prop_assert_eq!(enforcement.scope_memory_max, smallest);
                prop_assert_eq!(enforcement.scope_tasks_max, processes);
            } else {
                prop_assert_eq!(enforcement.scope_memory_max, None);
                prop_assert_eq!(enforcement.scope_tasks_max, None);
            }
        }
    }
}
