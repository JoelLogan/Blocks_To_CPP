//! Linux cgroup v2 scopes from the user's service manager
//! (`docs/adr/0008-pty-and-containment-in-b2c-process.md`).
//!
//! An unprivileged process cannot create cgroups in the shared hierarchy;
//! the user's systemd instance owns a delegated subtree and creates
//! transient *scopes* in it on request. `systemd-run --user --scope` asks
//! for one containing its own process, waits until it exists, then
//! executes the program **in place**: the program keeps the process ID,
//! process group, session and terminal that this crate set up, and every
//! process it starts stays in the scope, whatever it does with process
//! groups and sessions.
//!
//! * **Detection** ([`detected`]): once per process, see
//!   [`crate::containment_level`]. The trial scope runs
//!   `cat /proc/self/cgroup`, which shows the folder the manager puts
//!   scopes in (for example `…/user@1000.service/app.slice`); every later
//!   scope of this process is `<that folder>/<unit>.scope`. That folder's
//!   `cgroup.controllers` says whether `memory` and `pids` are delegated;
//!   without them the limits fall back to the watchdogs
//!   ([`super::Enforcement`]).
//! * **Spawning** ([`Scope::wrap`]): `systemd-run --user --scope --quiet
//!   --collect [--expand-environment=no] --unit=b2c-<build|run>-<ownerPid>-<16 hex>
//!   [-p MemoryMax=… -p MemorySwapMax=0] [-p TasksMax=…] -- <program> <args>`,
//!   found by absolute path, with the command's own environment plus only
//!   `XDG_RUNTIME_DIR` and `DBUS_SESSION_BUS_ADDRESS`, which it needs to
//!   reach the manager (it adds `INVOCATION_ID` itself). The program sees
//!   those too. `--expand-environment=no` (systemd 254 and later; detection
//!   finds out whether it is understood) keeps a future systemd from
//!   expanding `$VARIABLES` in the program's arguments.
//! * **Killing** ([`kill_cgroup`]): `1` written to the scope's
//!   `cgroup.kill` (Linux 5.14) kills everything in it at once. Older
//!   kernels freeze the scope (`cgroup.freeze`, so nothing in it can fork or
//!   exit and no process ID can be reused) and `SIGKILL` every process in
//!   `cgroup.procs` until it is empty. Until `systemd-run` has moved itself
//!   into the scope, the program's process group (which the platform
//!   module kills as well) holds it.
//! * **Events:** `memory.events` (`oom`, `oom_kill`) says that the scope ran
//!   out of memory, `pids.events` (`max`) that `TasksMax` refused a task.
//!   With `--collect` the manager removes the scope as soon as it is empty,
//!   which can be before the exit of its last process has been noticed;
//!   then the hierarchical `oom` and `oom_kill` counts of the folder the
//!   scopes are in, taken before the spawn and again after a failed exit,
//!   stand in for it ([`Scope::final_out_of_memory`]).
//! * **Stale scopes** ([`cleanup_stale_scopes`]): scopes whose owner (the
//!   process ID in the name) has died are killed.
//!
//! Nothing here uses `unsafe`: the cgroup files are read and written as
//! ordinary files (never following a symbolic link, never created), and
//! signals go through `rustix`.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustix::io::Errno;
use rustix::process::{Pid, Signal, kill_process, test_kill_process};

use super::{Containment, Controllers, Enforcement};
use crate::command::{Command, Limits, Stdin};
use crate::error::ProcessError;

/// Where the cgroup v2 hierarchy is mounted.
const CGROUP_ROOT: &str = "/sys/fs/cgroup";
/// Where `systemd-run` may be; never looked up on `PATH`.
const SYSTEMD_RUN: [&str; 2] = ["/usr/bin/systemd-run", "/bin/systemd-run"];
/// The program of the trial scope.
const CAT: [&str; 2] = ["/usr/bin/cat", "/bin/cat"];
/// How long detection may take in all.
const DETECT_TIMEOUT: Duration = Duration::from_secs(5);
/// The variables `systemd-run` needs to reach the user's manager.
const BUS_VARIABLES: [&str; 2] = ["XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS"];
/// Largest control file read (`*.events`, `cgroup.controllers`, the trial's
/// output).
const MAX_CONTROL_FILE: u64 = 64 * 1024;
/// Largest `cgroup.procs` read (about 100,000 process IDs).
const MAX_PROCS_FILE: u64 = 1024 * 1024;
/// How deep, and how many cgroups, a scope's own sub-cgroups are searched
/// for processes.
const MAX_PROCS_DEPTH: usize = 8;
const MAX_PROCS_CGROUPS: usize = 1024;
/// Rounds of the freeze-and-kill fallback, and the pause between them.
const KILL_ROUNDS: usize = 200;
const KILL_PAUSE: Duration = Duration::from_millis(2);
/// How deep, and how many folders, the stale-scope search looks.
const STALE_DEPTH: usize = 4;
const STALE_FOLDERS: usize = 4096;

/// What detection found: everything a scope needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Detected {
    /// The absolute path of `systemd-run`.
    pub(crate) systemd_run: PathBuf,
    /// The folder this process's scopes are created in.
    pub(crate) parent: PathBuf,
    /// Which controllers scopes get.
    pub(crate) controllers: Controllers,
    /// Whether `systemd-run` understands `--expand-environment=no`.
    pub(crate) no_expand: bool,
    /// `XDG_RUNTIME_DIR` and, when set, `DBUS_SESSION_BUS_ADDRESS`.
    pub(crate) bus_env: Vec<(OsString, OsString)>,
}

/// Why scopes are not available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unavailable {
    /// No cgroup v2 hierarchy at `/sys/fs/cgroup`.
    NoCgroup2,
    /// No `systemd-run` at its two absolute paths.
    NoSystemdRun,
    /// `XDG_RUNTIME_DIR` is unset, empty or relative.
    NoRuntimeDir,
    /// No `cat` for the trial.
    NoCat,
    /// The trial scope failed or took too long.
    TrialFailed,
    /// The trial did not end up in the expected scope.
    UnexpectedCgroup,
}

static DETECTED: OnceLock<Option<Detected>> = OnceLock::new();

/// The detection result, detecting on the first call.
pub(crate) fn detected() -> Option<&'static Detected> {
    #[cfg(test)]
    {
        if let Some(fake) = tests::FAKE.with(std::cell::Cell::get) {
            return Some(fake);
        }
    }
    DETECTED.get_or_init(|| detect().ok()).as_ref()
}

/// The detection result if detection has already run.
fn detected_if_done() -> Option<&'static Detected> {
    DETECTED.get().and_then(Option::as_ref)
}

/// The preconditions that need no process.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    systemd_run: PathBuf,
    cat: PathBuf,
    bus_env: Vec<(OsString, OsString)>,
}

fn detect() -> Result<Detected, Unavailable> {
    let root = Path::new(CGROUP_ROOT);
    let found = preconditions(root, &SYSTEMD_RUN, &CAT, &|name| std::env::var_os(name))?;
    let deadline = Instant::now() + DETECT_TIMEOUT;
    let mut no_expand = true;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Unavailable::TrialFailed);
        }
        let unit = unit_name("detect", std::process::id(), random_suffix());
        let command =
            trial_command(&found, &unit, no_expand, remaining).map_err(|_| Unavailable::TrialFailed)?;
        let captured = crate::run_captured(&command).map_err(|_| Unavailable::TrialFailed)?;
        if captured.status.success() && !captured.timed_out {
            let parent = scope_parent(&String::from_utf8_lossy(&captured.stdout), &unit, root)?;
            let controllers = read_control(&parent.join("cgroup.controllers"))
                .map(|text| parse_controllers(&text))
                .unwrap_or_default();
            return Ok(Detected {
                systemd_run: found.systemd_run,
                parent,
                controllers,
                no_expand,
                bus_env: found.bus_env,
            });
        }
        // systemd before 254 does not know the option (and never expands
        // variables in scope mode, so it is not needed there).
        if no_expand && !captured.timed_out && mentions_expand_option(&captured.stderr) {
            no_expand = false;
            continue;
        }
        return Err(Unavailable::TrialFailed);
    }
}

/// Checks everything detection needs before the trial: cgroup v2 at `root`,
/// `systemd-run` and `cat` at one of their paths, and `XDG_RUNTIME_DIR`
/// from `env`.
fn preconditions(
    root: &Path,
    systemd_run: &[&str],
    cat: &[&str],
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<Found, Unavailable> {
    if !root.join("cgroup.controllers").is_file() {
        return Err(Unavailable::NoCgroup2);
    }
    let systemd_run = first_executable(systemd_run).ok_or(Unavailable::NoSystemdRun)?;
    let mut bus_env = Vec::new();
    for name in BUS_VARIABLES {
        if let Some(value) = env(name).filter(|value| !value.is_empty()) {
            bus_env.push((OsString::from(name), value));
        }
    }
    let runtime_dir_ok = bus_env
        .first()
        .is_some_and(|(name, value)| name == BUS_VARIABLES[0] && Path::new(value).is_absolute());
    if !runtime_dir_ok {
        return Err(Unavailable::NoRuntimeDir);
    }
    let cat = first_executable(cat).ok_or(Unavailable::NoCat)?;
    Ok(Found {
        systemd_run,
        cat,
        bus_env,
    })
}

/// The first of `paths` that is an executable file.
fn first_executable(paths: &[&str]) -> Option<PathBuf> {
    paths.iter().map(PathBuf::from).find(|path| {
        fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    })
}

/// The trial: `systemd-run … -- cat /proc/self/cgroup` in a scope named
/// `unit`, contained by its process group only (detection must not need
/// itself).
fn trial_command(
    found: &Found,
    unit: &str,
    no_expand: bool,
    timeout: Duration,
) -> Result<Command, ProcessError> {
    let mut command = Command::new(&found.systemd_run, "/")?;
    command
        .args(scope_arguments(unit, no_expand, None, None))
        .arg(&found.cat)
        .arg("/proc/self/cgroup")
        .envs(found.bus_env.iter().cloned())
        .stdin(Stdin::Null)
        .containment(Containment::ProcessGroupOnly)
        .limits(Limits {
            timeout: Some(timeout),
            stdout_cap: usize::try_from(MAX_CONTROL_FILE).unwrap_or(usize::MAX),
            stderr_cap: usize::try_from(MAX_CONTROL_FILE).unwrap_or(usize::MAX),
            ..Limits::default()
        });
    Ok(command)
}

/// Whether `systemd-run`'s error output complains about
/// `--expand-environment`.
fn mentions_expand_option(stderr: &[u8]) -> bool {
    String::from_utf8_lossy(stderr).contains("expand-environment")
}

/// The arguments of `systemd-run` up to and including `--`.
fn scope_arguments(
    unit: &str,
    no_expand: bool,
    memory_max: Option<u64>,
    tasks_max: Option<u32>,
) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = ["--user", "--scope", "--quiet", "--collect"]
        .into_iter()
        .map(OsString::from)
        .collect();
    if no_expand {
        arguments.push("--expand-environment=no".into());
    }
    arguments.push(format!("--unit={unit}").into());
    if let Some(bytes) = memory_max {
        arguments.extend(["-p".into(), format!("MemoryMax={bytes}").into()]);
        arguments.extend(["-p".into(), "MemorySwapMax=0".into()]);
    }
    if let Some(tasks) = tasks_max {
        arguments.extend(["-p".into(), format!("TasksMax={tasks}").into()]);
    }
    arguments.push("--".into());
    arguments
}

/// The folder below `root` that holds the trial scope `unit`, from the
/// trial's `/proc/self/cgroup` (the cgroup v2 line is `0::<path>`). The path
/// must be absolute and plain, end in `<unit>.scope`, and its folder must
/// exist.
fn scope_parent(cgroup_file: &str, unit: &str, root: &Path) -> Result<PathBuf, Unavailable> {
    let line = cgroup_file
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .ok_or(Unavailable::UnexpectedCgroup)?;
    let path = Path::new(line);
    let plain = path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::RootDir | Component::Normal(_)));
    let expected = format!("{unit}.scope");
    if !plain || path.file_name().and_then(|name| name.to_str()) != Some(expected.as_str()) {
        return Err(Unavailable::UnexpectedCgroup);
    }
    let folder = path
        .parent()
        .and_then(|parent| parent.strip_prefix("/").ok())
        .ok_or(Unavailable::UnexpectedCgroup)?;
    let parent = root.join(folder);
    if !parent.is_dir() {
        return Err(Unavailable::UnexpectedCgroup);
    }
    Ok(parent)
}

/// Which of the controllers this crate uses `cgroup.controllers` lists.
fn parse_controllers(text: &str) -> Controllers {
    let mut controllers = Controllers::default();
    for name in text.split_ascii_whitespace() {
        match name {
            "memory" => controllers.memory = true,
            "pids" => controllers.pids = true,
            _ => {}
        }
    }
    controllers
}

/// A scope's unit name without the `.scope` suffix:
/// `b2c-<word>-<owner>-<16 hex>`.
fn unit_name(word: &str, owner: u32, suffix: u64) -> String {
    format!("b2c-{word}-{owner}-{suffix:016x}")
}

/// The owner process ID of a folder named like one of this crate's scopes
/// (`b2c-<word>-<owner>-<16 lower-case hex>.scope`, the word 1 to 16
/// lower-case letters), or `None` for any other name.
fn scope_owner(folder_name: &str) -> Option<u32> {
    let rest = folder_name.strip_prefix("b2c-")?.strip_suffix(".scope")?;
    let mut parts = rest.split('-');
    let (word, owner, suffix) = (parts.next()?, parts.next()?, parts.next()?);
    let word_ok = (1..=16).contains(&word.len()) && word.bytes().all(|b| b.is_ascii_lowercase());
    let owner_ok = (1..=10).contains(&owner.len()) && owner.bytes().all(|b| b.is_ascii_digit());
    let suffix_ok = suffix.len() == 16 && suffix.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if parts.next().is_some() || !word_ok || !owner_ok || !suffix_ok {
        return None;
    }
    owner.parse().ok().filter(|&owner| owner != 0)
}

/// 64 random bits for a unit name. Only uniqueness matters (the owner's
/// process ID is part of the name), so if the system's random source fails,
/// a counter mixed with the clock stands in.
fn random_suffix() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut bytes = [0_u8; 8];
    let mut filled = 0;
    for _ in 0..8 {
        let Some(rest) = bytes.get_mut(filled..) else {
            break;
        };
        match rustix::rand::getrandom(rest, rustix::rand::GetRandomFlags::empty()) {
            Ok(read) => filled += read,
            Err(Errno::INTR) => {}
            Err(_) => break,
        }
        if filled >= bytes.len() {
            return u64::from_le_bytes(bytes);
        }
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    #[allow(clippy::cast_possible_truncation)] // only the low bits are wanted
    let nanos = nanos as u64;
    nanos ^ COUNTER.fetch_add(1, Ordering::Relaxed).rotate_left(32)
}

/// One spawn's scope.
#[derive(Debug)]
pub(crate) struct Scope {
    /// The unit name without `.scope`.
    unit: String,
    /// `<parent>/<unit>.scope`, which exists from the moment `systemd-run`
    /// is in it until the scope is empty.
    dir: PathBuf,
    /// The folder the scopes are in.
    parent: PathBuf,
    /// The parent's hierarchical `oom` + `oom_kill` count before the spawn
    /// (when the memory controller is delegated).
    oom_before: Option<u64>,
}

impl Scope {
    /// A new scope for a spawn of kind `word` (`build`, `run`).
    pub(crate) fn new(detected: &Detected, word: &str) -> Self {
        let unit = unit_name(word, std::process::id(), random_suffix());
        Self {
            dir: detected.parent.join(format!("{unit}.scope")),
            parent: detected.parent.clone(),
            oom_before: if detected.controllers.memory {
                oom_count(&detected.parent)
            } else {
                None
            },
            unit,
        }
    }

    /// `command` started through `systemd-run` in this scope, with the
    /// scope limits of `enforcement`. The wrapped command keeps the
    /// original's working directory, input, limits, process group and
    /// cancellation token; its environment is the original's plus the
    /// manager's bus variables.
    ///
    /// # Errors
    /// Those of [`Command::new`], which cannot happen for the absolute
    /// `systemd-run` path and an already accepted working directory.
    pub(crate) fn wrap(
        &self,
        detected: &Detected,
        command: &Command,
        enforcement: &Enforcement,
    ) -> Result<Command, ProcessError> {
        let mut wrapped = Command::new(&detected.systemd_run, command.working_dir())?;
        wrapped
            .args(scope_arguments(
                &self.unit,
                detected.no_expand,
                enforcement.scope_memory_max,
                enforcement.scope_tasks_max,
            ))
            .arg(command.program())
            .args(command.get_args().iter().cloned())
            .envs(
                command
                    .get_envs()
                    .map(|(name, value)| (name.to_os_string(), value.to_os_string())),
            )
            .envs(detected.bus_env.iter().cloned())
            .stdin(command.get_stdin().clone())
            .limits(command.get_limits().clone())
            .process_group(command.get_process_group())
            .containment(Containment::ProcessGroupOnly);
        if let Some(token) = command.get_cancel() {
            wrapped.cancel_token(token);
        }
        Ok(wrapped)
    }

    /// Kills everything in the scope (nothing, if it does not exist yet or
    /// any more).
    pub(crate) fn kill(&self) {
        kill_cgroup(&self.dir);
    }

    /// The processes in the scope.
    pub(crate) fn pids(&self) -> Vec<i32> {
        cgroup_pids(&self.dir)
    }

    /// Whether the scope has run out of memory so far.
    pub(crate) fn oom_seen(&self) -> bool {
        oom_count(&self.dir).is_some_and(|count| count > 0)
    }

    /// Whether `TasksMax` has refused a new task so far.
    pub(crate) fn tasks_max_hit(&self) -> bool {
        read_control(&self.dir.join("pids.events"))
            .and_then(|text| flat_value(&text, "max"))
            .is_some_and(|count| count > 0)
    }

    /// Whether the run ran out of memory, decided after its exit: from the
    /// scope's `memory.events` while it exists; once the manager has removed
    /// it, a run that `failed` counts as out of memory when the hierarchical
    /// count of the scopes' folder grew since the spawn.
    pub(crate) fn final_out_of_memory(&self, failed: bool) -> bool {
        match oom_count(&self.dir) {
            Some(count) => count > 0,
            None => {
                failed
                    && self
                        .oom_before
                        .zip(oom_count(&self.parent))
                        .is_some_and(|(before, now)| now > before)
            }
        }
    }
}

/// `oom` + `oom_kill` from the `memory.events` of cgroup `dir`, or `None`
/// when it cannot be read.
fn oom_count(dir: &Path) -> Option<u64> {
    let text = read_control(&dir.join("memory.events"))?;
    let oom = flat_value(&text, "oom").unwrap_or(0);
    let kills = flat_value(&text, "oom_kill").unwrap_or(0);
    Some(oom.saturating_add(kills))
}

/// The value of `key` in a flat-keyed cgroup file (`key value` lines).
fn flat_value(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once(' ')?;
        (name == key).then(|| value.trim().parse().ok()).flatten()
    })
}

/// The process IDs listed in a `cgroup.procs` text.
fn parse_pids(text: &str) -> impl Iterator<Item = i32> + '_ {
    text.lines()
        .filter_map(|line| line.trim().parse::<i32>().ok())
        .filter(|&pid| pid > 0)
}

/// Every process in cgroup `dir` and its sub-cgroups (bounded), sorted.
pub(crate) fn cgroup_pids(dir: &Path) -> Vec<i32> {
    let mut pids = Vec::new();
    let mut pending = vec![(dir.to_path_buf(), 0_usize)];
    let mut visited = 0_usize;
    while let Some((cgroup, depth)) = pending.pop() {
        visited += 1;
        if visited > MAX_PROCS_CGROUPS {
            break;
        }
        if let Some(text) = read_bounded(&cgroup.join("cgroup.procs"), MAX_PROCS_FILE) {
            pids.extend(parse_pids(&text));
        }
        if depth < MAX_PROCS_DEPTH
            && let Ok(entries) = fs::read_dir(&cgroup)
        {
            pending.extend(
                entries
                    .filter_map(Result::ok)
                    .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                    .map(|entry| (entry.path(), depth + 1)),
            );
        }
    }
    pids.sort_unstable();
    pids.dedup();
    pids
}

/// Kills every process in cgroup `dir` and below: `cgroup.kill`, or the
/// freeze-and-kill fallback where that file does not exist (before Linux
/// 5.14). Does nothing when the cgroup does not exist.
pub(crate) fn kill_cgroup(dir: &Path) {
    if write_control(&dir.join("cgroup.kill"), b"1").is_ok() || !dir.is_dir() {
        return;
    }
    freeze_and_kill(dir);
}

/// Freezes cgroup `dir` (frozen processes can neither fork nor exit, so the
/// IDs read from `cgroup.procs` stay theirs), sends `SIGKILL` to every
/// process in it until none is left (a frozen process still dies of
/// `SIGKILL`), and thaws it again. Without a freezer (before Linux 5.2) the
/// same loop runs unfrozen.
fn freeze_and_kill(dir: &Path) {
    let frozen = write_control(&dir.join("cgroup.freeze"), b"1").is_ok();
    for _ in 0..KILL_ROUNDS {
        let pids = cgroup_pids(dir);
        if pids.is_empty() {
            break;
        }
        for pid in pids.into_iter().filter_map(Pid::from_raw) {
            let _ = kill_process(pid, Signal::KILL);
        }
        thread::sleep(KILL_PAUSE);
    }
    if frozen {
        let _ = write_control(&dir.join("cgroup.freeze"), b"0");
    }
}

/// Kills the scopes of dead owners below the user's service manager (and
/// below the folder of this process's scopes, once detection has found it).
pub(crate) fn cleanup_stale_scopes() -> usize {
    let uid = rustix::process::getuid().as_raw();
    let mut roots =
        vec![Path::new(CGROUP_ROOT).join(format!("user.slice/user-{uid}.slice/user@{uid}.service"))];
    if let Some(detected) = detected_if_done()
        && !roots.iter().any(|root| detected.parent.starts_with(root))
    {
        roots.push(detected.parent.clone());
    }
    let own = std::process::id();
    let mut killed = 0;
    for root in &roots {
        for scope in stale_scopes(root, own, &is_alive) {
            kill_cgroup(&scope);
            killed += 1;
        }
    }
    killed
}

/// The folders below `root` (breadth first, at most [`STALE_DEPTH`] levels
/// and [`STALE_FOLDERS`] folders, never following links) named like this
/// crate's scopes whose owner is neither `own` nor `alive`.
fn stale_scopes(root: &Path, own: u32, alive: &dyn Fn(u32) -> bool) -> Vec<PathBuf> {
    let mut stale = Vec::new();
    let mut level = vec![root.to_path_buf()];
    let mut seen = 0_usize;
    for _ in 0..STALE_DEPTH {
        let mut next = Vec::new();
        for folder in &level {
            let Ok(entries) = fs::read_dir(folder) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    continue;
                }
                seen += 1;
                if seen > STALE_FOLDERS {
                    return stale;
                }
                let name = entry.file_name();
                match name.to_str().and_then(scope_owner) {
                    Some(owner) if owner != own && !alive(owner) => stale.push(entry.path()),
                    // A live scope of ours: nothing below it to look at.
                    Some(_) => {}
                    None => next.push(entry.path()),
                }
            }
        }
        level = next;
    }
    stale
}

/// Whether process `pid` exists (a process of another user counts).
fn is_alive(pid: u32) -> bool {
    let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) else {
        return false;
    };
    !matches!(test_kill_process(pid), Err(Errno::SRCH))
}

/// Writes `value` to a cgroup control file: never created, never through
/// a symbolic link.
fn write_control(path: &Path, value: &[u8]) -> io::Result<()> {
    OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?
        .write_all(value)
}

/// Reads a small cgroup control file.
fn read_control(path: &Path) -> Option<String> {
    read_bounded(path, MAX_CONTROL_FILE)
}

/// Reads at most `limit` bytes of a cgroup file, never through a symbolic
/// link.
fn read_bounded(path: &Path, limit: u64) -> Option<String> {
    let file: File = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .ok()?;
    let mut text = String::new();
    file.take(limit).read_to_string(&mut text).ok()?;
    Some(text)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use proptest::prelude::*;

    use super::*;

    fn no_env(_: &str) -> Option<OsString> {
        None
    }

    fn detected_for(parent: &Path, controllers: Controllers) -> Detected {
        Detected {
            systemd_run: PathBuf::from("/usr/bin/systemd-run"),
            parent: parent.to_path_buf(),
            controllers,
            no_expand: true,
            bus_env: vec![
                ("XDG_RUNTIME_DIR".into(), "/run/user/1000".into()),
                (
                    "DBUS_SESSION_BUS_ADDRESS".into(),
                    "unix:path=/run/user/1000/bus".into(),
                ),
            ],
        }
    }

    #[test]
    fn unit_names_round_trip() {
        let name = unit_name("build", 4242, 0x0123_4567_89ab_cdef);
        assert_eq!(name, "b2c-build-4242-0123456789abcdef");
        assert_eq!(scope_owner(&format!("{name}.scope")), Some(4242));
        assert_eq!(scope_owner(&format!("{}.scope", unit_name("run", 7, 1))), Some(7));
        for other in [
            "b2c-build-4242-0123456789abcdef",
            "b2c-build-4242-0123456789abcde.scope",
            "b2c-build-4242-0123456789ABCDEF.scope",
            "b2c-build-0-0123456789abcdef.scope",
            "b2c--4242-0123456789abcdef.scope",
            "b2c-Build-4242-0123456789abcdef.scope",
            "b2c-build-42x-0123456789abcdef.scope",
            "b2c-build-4242-0123456789abcdef-x.scope",
            "b2c-build-12345678901-0123456789abcdef.scope",
            "b2c-abcdefghijklmnopq-1-0123456789abcdef.scope",
            "app-gnome-firefox-1234.scope",
            "run-r0123.scope",
            "b2c.slice",
            "",
        ] {
            assert_eq!(scope_owner(other), None, "{other:?}");
        }
    }

    #[test]
    fn random_suffixes_differ() {
        let first = random_suffix();
        assert!((0..8).any(|_| random_suffix() != first));
    }

    #[test]
    fn the_arguments_match_the_specified_command_line() {
        let build = scope_arguments("b2c-build-1-00000000000000ff", true, Some(4 << 30), Some(32));
        assert_eq!(
            build,
            [
                "--user",
                "--scope",
                "--quiet",
                "--collect",
                "--expand-environment=no",
                "--unit=b2c-build-1-00000000000000ff",
                "-p",
                "MemoryMax=4294967296",
                "-p",
                "MemorySwapMax=0",
                "-p",
                "TasksMax=32",
                "--",
            ]
            .map(OsString::from)
        );
        let run = scope_arguments("b2c-run-1-00000000000000ff", false, None, None);
        assert_eq!(
            run,
            [
                "--user",
                "--scope",
                "--quiet",
                "--collect",
                "--unit=b2c-run-1-00000000000000ff",
                "--"
            ]
            .map(OsString::from)
        );
    }

    #[test]
    fn a_wrapped_command_keeps_everything_but_the_program() {
        let dir = tempfile::tempdir().unwrap();
        let detected = detected_for(
            dir.path(),
            Controllers {
                memory: true,
                pids: true,
            },
        );
        let token = crate::CancelToken::new();
        let mut command = Command::new("/bin/echo", "/tmp").unwrap();
        command
            .args(["-n", "$HOME", "--"])
            .env("ONLY", "this")
            .env("XDG_RUNTIME_DIR", "/elsewhere")
            .stdin(Stdin::Bytes(b"in".to_vec()))
            .process_group(crate::ProcessGroup::New)
            .cancel_token(&token)
            .limits(Limits {
                memory: Some(1 << 30),
                processes: Some(32),
                ..Limits::default()
            });
        let enforcement = Enforcement::new(command.get_limits(), Some(detected.controllers));
        let scope = Scope::new(&detected, "build");
        let wrapped = scope.wrap(&detected, &command, &enforcement).unwrap();
        assert_eq!(wrapped.program(), Path::new("/usr/bin/systemd-run"));
        let args: Vec<_> = wrapped
            .get_args()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let dashes = args.iter().position(|arg| arg == "--").unwrap();
        assert_eq!(
            args.get(dashes + 1..).unwrap(),
            ["/bin/echo", "-n", "$HOME", "--"]
        );
        assert!(args.contains(&format!("--unit={}", scope.unit)));
        assert!(args.contains(&"MemoryMax=1073741824".to_owned()));
        assert!(args.contains(&"TasksMax=32".to_owned()));
        assert!(
            scope
                .unit
                .starts_with(&format!("b2c-build-{}-", std::process::id()))
        );
        assert_eq!(scope.dir, dir.path().join(format!("{}.scope", scope.unit)));
        assert_eq!(wrapped.working_dir(), Path::new("/tmp"));
        let env: Vec<_> = wrapped.get_envs().collect();
        assert_eq!(
            env,
            [
                (
                    std::ffi::OsStr::new("DBUS_SESSION_BUS_ADDRESS"),
                    std::ffi::OsStr::new("unix:path=/run/user/1000/bus")
                ),
                (std::ffi::OsStr::new("ONLY"), std::ffi::OsStr::new("this")),
                (
                    std::ffi::OsStr::new("XDG_RUNTIME_DIR"),
                    std::ffi::OsStr::new("/run/user/1000")
                ),
            ]
        );
        assert_eq!(wrapped.get_stdin(), &Stdin::Bytes(b"in".to_vec()));
        assert_eq!(wrapped.get_limits(), command.get_limits());
        assert_eq!(wrapped.get_process_group(), crate::ProcessGroup::New);
        assert_eq!(wrapped.get_containment(), Containment::ProcessGroupOnly);
        token.cancel();
        assert!(wrapped.get_cancel().is_some_and(crate::CancelToken::is_cancelled));
    }

    #[test]
    fn preconditions_are_checked_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("cgroup");
        fs::create_dir(&root).unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let tool = bin.join("tool");
        fs::write(&tool, "").unwrap();
        let not_executable = bin.join("plain");
        fs::write(&not_executable, "").unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&not_executable, fs::Permissions::from_mode(0o644)).unwrap();
        let tool = tool.to_str().unwrap();
        let missing = bin.join("missing");
        let missing = missing.to_str().unwrap();
        let not_executable = not_executable.to_str().unwrap();
        let runtime = |name: &str| (name == "XDG_RUNTIME_DIR").then(|| OsString::from("/run/user/1000"));

        assert_eq!(
            preconditions(&root, &[tool], &[tool], &runtime),
            Err(Unavailable::NoCgroup2)
        );
        fs::write(root.join("cgroup.controllers"), "cpu memory pids\n").unwrap();
        assert_eq!(
            preconditions(&root, &[missing, not_executable], &[tool], &runtime),
            Err(Unavailable::NoSystemdRun)
        );
        assert_eq!(
            preconditions(&root, &[tool], &[tool], &no_env),
            Err(Unavailable::NoRuntimeDir)
        );
        let relative = |name: &str| (name == "XDG_RUNTIME_DIR").then(|| OsString::from("run/user"));
        assert_eq!(
            preconditions(&root, &[tool], &[tool], &relative),
            Err(Unavailable::NoRuntimeDir)
        );
        // The bus address alone is not enough: systemd-run needs the folder.
        let bus_only =
            |name: &str| (name == "DBUS_SESSION_BUS_ADDRESS").then(|| OsString::from("unix:path=/x"));
        assert_eq!(
            preconditions(&root, &[tool], &[tool], &bus_only),
            Err(Unavailable::NoRuntimeDir)
        );
        assert_eq!(
            preconditions(&root, &[tool], &[missing], &runtime),
            Err(Unavailable::NoCat)
        );
        let found = preconditions(&root, &[missing, tool], &[tool], &runtime).unwrap();
        assert_eq!(found.systemd_run, Path::new(tool));
        assert_eq!(
            found.bus_env,
            [(
                OsString::from("XDG_RUNTIME_DIR"),
                OsString::from("/run/user/1000")
            )]
        );
        let both = |name: &str| match name {
            "XDG_RUNTIME_DIR" => Some(OsString::from("/run/user/1000")),
            "DBUS_SESSION_BUS_ADDRESS" => Some(OsString::from("unix:path=/run/user/1000/bus")),
            _ => None,
        };
        assert_eq!(
            preconditions(&root, &[tool], &[tool], &both)
                .unwrap()
                .bus_env
                .len(),
            2
        );
    }

    #[test]
    fn the_trial_is_contained_without_a_scope() {
        let found = Found {
            systemd_run: PathBuf::from("/usr/bin/systemd-run"),
            cat: PathBuf::from("/usr/bin/cat"),
            bus_env: vec![("XDG_RUNTIME_DIR".into(), "/run/user/1000".into())],
        };
        let command = trial_command(&found, "b2c-detect-1-0000000000000001", true, DETECT_TIMEOUT).unwrap();
        assert_eq!(command.get_containment(), Containment::ProcessGroupOnly);
        assert_eq!(command.get_limits().timeout, Some(DETECT_TIMEOUT));
        let args: Vec<_> = command
            .get_args()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args.get(args.len() - 3..).unwrap(),
            ["--", "/usr/bin/cat", "/proc/self/cgroup"]
        );
        assert_eq!(command.get_envs().count(), 1);
        assert!(mentions_expand_option(
            b"systemd-run: unrecognized option '--expand-environment=no'\n"
        ));
        assert!(!mentions_expand_option(
            b"Failed to connect to bus: No medium found\n"
        ));
    }

    #[test]
    fn the_scope_folder_comes_from_the_trial() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let slice = root.join("user.slice/user-1000.slice/user@1000.service/app.slice");
        fs::create_dir_all(&slice).unwrap();
        let unit = "b2c-detect-9-00000000000000aa";
        let output = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/b2c-detect-9-00000000000000aa.scope\n";
        assert_eq!(scope_parent(output, unit, root), Ok(slice.clone()));
        // A hybrid system lists v1 hierarchies first.
        let hybrid = format!("12:pids:/user.slice\n1:name=systemd:/x\n{output}");
        assert_eq!(scope_parent(&hybrid, unit, root), Ok(slice));
        for bad in [
            "",
            "1:name=systemd:/user.slice/b2c-detect-9-00000000000000aa.scope\n",
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/other.scope\n",
            "0::user.slice/b2c-detect-9-00000000000000aa.scope\n",
            "0::/user.slice/../b2c-detect-9-00000000000000aa.scope\n",
            "0::/missing.slice/b2c-detect-9-00000000000000aa.scope\n",
            "0::/b2c-detect-9-00000000000000aa.scope/../b2c-detect-9-00000000000000aa.scope\n",
        ] {
            assert_eq!(
                scope_parent(bad, unit, root),
                Err(Unavailable::UnexpectedCgroup),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn controllers_and_events_are_parsed() {
        assert_eq!(
            parse_controllers("cpuset cpu io memory pids\n"),
            Controllers {
                memory: true,
                pids: true
            }
        );
        assert_eq!(parse_controllers("cpu io\n"), Controllers::default());
        assert_eq!(
            parse_controllers("pids"),
            Controllers {
                memory: false,
                pids: true
            }
        );
        let events = "low 0\nhigh 0\nmax 12\noom 1\noom_kill 1\noom_group_kill 0\n";
        assert_eq!(flat_value(events, "oom"), Some(1));
        assert_eq!(flat_value(events, "oom_kill"), Some(1));
        assert_eq!(flat_value(events, "max"), Some(12));
        assert_eq!(flat_value(events, "oom_group"), None);
        assert_eq!(flat_value("max x\n", "max"), None);
        assert_eq!(parse_pids("12\n  7\nx\n-3\n0\n\n").collect::<Vec<_>>(), [12, 7]);
    }

    #[test]
    fn events_of_a_scope_that_is_gone_fall_back_to_the_parent() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path();
        fs::write(parent.join("memory.events"), "oom 3\noom_kill 2\n").unwrap();
        let detected = detected_for(
            parent,
            Controllers {
                memory: true,
                pids: true,
            },
        );
        let scope = Scope::new(&detected, "run");
        assert_eq!(scope.oom_before, Some(5));
        // No scope folder: not seen yet, and no unexplained growth.
        assert!(!scope.oom_seen());
        assert!(!scope.final_out_of_memory(true));
        // The parent grew while the scope existed: a failed run was killed.
        fs::write(parent.join("memory.events"), "oom 4\noom_kill 3\n").unwrap();
        assert!(scope.final_out_of_memory(true));
        assert!(!scope.final_out_of_memory(false));
        // While the scope exists, its own events decide.
        fs::create_dir(&scope.dir).unwrap();
        fs::write(scope.dir.join("memory.events"), "oom 0\noom_kill 0\n").unwrap();
        assert!(!scope.final_out_of_memory(true));
        fs::write(scope.dir.join("memory.events"), "oom 1\noom_kill 1\n").unwrap();
        assert!(scope.oom_seen());
        assert!(scope.final_out_of_memory(false));
        assert!(!scope.tasks_max_hit());
        fs::write(scope.dir.join("pids.events"), "max 0\n").unwrap();
        assert!(!scope.tasks_max_hit());
        fs::write(scope.dir.join("pids.events"), "max 4\n").unwrap();
        assert!(scope.tasks_max_hit());
        // Without the memory controller nothing is snapshotted.
        let plain = Scope::new(&detected_for(parent, Controllers::default()), "run");
        assert_eq!(plain.oom_before, None);
        assert!(!plain.final_out_of_memory(true));
    }

    #[test]
    fn control_files_are_never_read_or_written_through_links() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        fs::write(&target, "oom 1\n").unwrap();
        symlink(&target, dir.path().join("memory.events")).unwrap();
        assert_eq!(oom_count(dir.path()), None);
        symlink(&target, dir.path().join("cgroup.kill")).unwrap();
        assert!(write_control(&dir.path().join("cgroup.kill"), b"1").is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "oom 1\n");
        // Never created either.
        assert!(write_control(&dir.path().join("cgroup.freeze"), b"1").is_err());
        assert!(!dir.path().join("cgroup.freeze").exists());
    }

    #[test]
    fn killing_a_missing_or_plain_folder_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        kill_cgroup(&dir.path().join("missing.scope"));
        fs::write(dir.path().join("cgroup.procs"), "").unwrap();
        kill_cgroup(dir.path());
        assert!(cgroup_pids(dir.path()).is_empty());
    }

    #[test]
    fn stale_scopes_are_those_of_dead_owners() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let app = root.join("app.slice");
        let nested = root.join("a.slice/b.slice");
        fs::create_dir_all(&app).unwrap();
        fs::create_dir_all(&nested).unwrap();
        let dead = app.join("b2c-run-100-00000000000000aa.scope");
        let alive = app.join("b2c-build-200-00000000000000bb.scope");
        let mine = app.join("b2c-run-300-00000000000000cc.scope");
        let deep_dead = nested.join("b2c-build-101-00000000000000dd.scope");
        let too_deep = root.join("a.slice/b.slice/c.slice/d.slice/b2c-run-102-00000000000000ee.scope");
        let unrelated = app.join("app-gnome-firefox-100.scope");
        let file = app.join("b2c-run-103-00000000000000ff.scope.txt");
        for folder in [&dead, &alive, &mine, &deep_dead, &too_deep, &unrelated] {
            fs::create_dir_all(folder).unwrap();
        }
        fs::write(&file, "").unwrap();
        // A link to a stale-looking folder elsewhere is not followed.
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(outside.path().join("b2c-run-104-0000000000000011.scope")).unwrap();
        symlink(outside.path(), app.join("link.slice")).unwrap();
        symlink(
            outside.path().join("b2c-run-104-0000000000000011.scope"),
            app.join("b2c-run-105-0000000000000022.scope"),
        )
        .unwrap();

        let alive_pids = |pid: u32| pid == 200;
        let mut found = stale_scopes(root, 300, &alive_pids);
        found.sort();
        let mut expected = vec![dead, deep_dead];
        expected.sort();
        assert_eq!(found, expected);
        assert!(stale_scopes(&root.join("missing"), 300, &alive_pids).is_empty());
    }

    #[test]
    fn liveness_is_checked_by_process_id() {
        assert!(is_alive(std::process::id()));
        assert!(is_alive(1));
        assert!(!is_alive(0));
        assert!(!is_alive(u32::MAX));
    }

    /// A cgroup v2 folder this test may create and move processes into
    /// (root on a system with a writable v2 hierarchy, as in the hybrid
    /// development container); `None` elsewhere, where the tests that need
    /// it are skipped. Dropping it kills what is left in it and removes it,
    /// so a failing test leaves nothing behind.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Option<Self> {
            let mounts = fs::read_to_string("/proc/self/mounts").ok()?;
            let name = format!("b2c-test-{}-{:016x}", std::process::id(), random_suffix());
            mounts
                .lines()
                .filter_map(|line| {
                    let mut fields = line.split_whitespace();
                    let mount = fields.nth(1)?;
                    (fields.next()? == "cgroup2").then(|| PathBuf::from(mount))
                })
                .find_map(|mount| {
                    let dir = mount.join(&name);
                    fs::create_dir(&dir).ok().map(|()| Self(dir))
                })
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            kill_cgroup(&self.0);
            let _ = wait_until_empty(&self.0);
            // Scopes the fake `systemd-run` created inside, then the folder.
            if let Ok(entries) = fs::read_dir(&self.0) {
                for entry in entries.filter_map(Result::ok) {
                    if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                        let _ = fs::remove_dir(entry.path());
                    }
                }
            }
            let _ = fs::remove_dir(&self.0);
        }
    }

    thread_local! {
        /// What [`detected`] returns on this test thread instead of the
        /// real detection (see [`with_fake_scopes`]).
        pub(super) static FAKE: std::cell::Cell<Option<&'static Detected>> =
            const { std::cell::Cell::new(None) };
    }

    /// A stand-in for `systemd-run` that understands the options this crate
    /// passes: it creates the scope's cgroup in `parent`, moves itself into
    /// it, and executes the program in place, as the real one does. Without
    /// a user service manager, this is how the scope plumbing is exercised
    /// in a development container (as root, with a writable cgroup v2
    /// hierarchy).
    fn fake_systemd_run(dir: &Path, parent: &Path) -> PathBuf {
        let script = format!(
            r#"#!/bin/sh
unit=
while [ $# -gt 0 ]; do
  case "$1" in
    --user|--scope|--quiet|--collect|--expand-environment=no) shift ;;
    --unit=*) unit="${{1#--unit=}}"; shift ;;
    -p) shift 2 ;;
    --) shift; break ;;
    *) echo "fake systemd-run: unexpected $1" >&2; exit 1 ;;
  esac
done
[ -n "$XDG_RUNTIME_DIR" ] || {{ echo "fake systemd-run: no XDG_RUNTIME_DIR" >&2; exit 1; }}
/bin/mkdir "{parent}/$unit.scope" || exit 1
echo $$ > "{parent}/$unit.scope/cgroup.procs" || exit 1
exec "$@"
"#,
            parent = parent.display()
        );
        let path = dir.join("systemd-run");
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// Runs `test` on this thread with scopes from the fake `systemd-run`
    /// in a scratch cgroup; skipped (with a logged reason) where no such
    /// cgroup can be made.
    fn with_fake_scopes(name: &str, test: impl FnOnce(&Path)) {
        if !Path::new("/usr/bin/setsid").exists() {
            eprintln!("skipped {name}: /usr/bin/setsid is not installed");
            return;
        }
        let Some(scratch) = Scratch::new() else {
            eprintln!("skipped {name}: no cgroup v2 hierarchy this test may create a cgroup in");
            return;
        };
        let bin = tempfile::tempdir().unwrap();
        let detected: &'static Detected = Box::leak(Box::new(Detected {
            systemd_run: fake_systemd_run(bin.path(), &scratch.0),
            parent: scratch.0.clone(),
            controllers: Controllers::default(),
            no_expand: true,
            bus_env: vec![("XDG_RUNTIME_DIR".into(), "/run/user/fake".into())],
        }));
        FAKE.with(|fake| fake.set(Some(detected)));
        let reset = Reset;
        test(&scratch.0);
        drop(reset);
    }

    /// Clears the fake detection, also when the test panics.
    struct Reset;

    impl Drop for Reset {
        fn drop(&mut self) {
            FAKE.with(|fake| fake.set(None));
        }
    }

    /// The `b2c-<word>-<this process>-<hex>.scope` folders in `parent`.
    fn scopes_in(parent: &Path, word: &str) -> Vec<PathBuf> {
        let prefix = format!("b2c-{word}-{}-", std::process::id());
        fs::read_dir(parent)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
            .map(|entry| entry.path())
            .collect()
    }

    fn sh(script: &str) -> Command {
        let mut command = Command::new("/bin/sh", "/").unwrap();
        command.args(["-c", script]).env("PATH", "/usr/bin:/bin");
        command
    }

    /// The first number after `label` in `text`.
    fn number_after(text: &str, label: &str) -> i32 {
        let start = text.find(label).unwrap() + label.len();
        text.get(start..)
            .unwrap()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap()
    }

    fn is_gone(pid: i32) -> bool {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let state = fs::read_to_string(format!("/proc/{pid}/stat"))
                .ok()
                .and_then(|stat| {
                    let after = stat.rsplit_once(')')?.1.to_owned();
                    after.split_whitespace().next()?.chars().next()
                });
            match state {
                None | Some('Z' | 'X') => return true,
                Some(_) if Instant::now() > until => return false,
                Some(_) => thread::sleep(Duration::from_millis(10)),
            }
        }
    }

    /// Starts a sleeper that double-forks and leaves the session, waits
    /// until it has, and prints `escaped <pid>`.
    const ESCAPE: &str = "( /usr/bin/setsid /bin/sleep 600 </dev/null >/dev/null 2>&1 & p=$!; \
        while [ \"$(cut -d' ' -f6 /proc/$p/stat)\" != \"$p\" ]; do sleep 0.01; done; \
        echo \"escaped $p\" )";

    #[test]
    fn a_scoped_captured_run_loses_its_escaped_children() {
        with_fake_scopes("a_scoped_captured_run_loses_its_escaped_children", |parent| {
            let mut command = sh(&format!(
                "{ESCAPE}; cat /proc/self/cgroup; echo \"bus=$XDG_RUNTIME_DIR\""
            ));
            command.env("XDG_RUNTIME_DIR", "/elsewhere");
            let result = crate::run_captured(&command).unwrap();
            let output = String::from_utf8_lossy(&result.stdout).into_owned();
            assert!(result.status.success(), "{result:?}");
            let escaped = number_after(&output, "escaped ");
            let gone = is_gone(escaped);
            if !gone {
                let _ = kill_process(Pid::from_raw(escaped).unwrap(), Signal::KILL);
            }
            assert!(gone, "the escaped sleeper {escaped} outlived its run");
            let scopes = scopes_in(parent, "build");
            assert_eq!(scopes.len(), 1, "{scopes:?}");
            let unit = scopes[0].file_name().unwrap().to_string_lossy().into_owned();
            assert!(output.contains(&format!("/{unit}\n")), "{output:?}");
            // The scope's bus variables replace the command's own.
            assert!(output.contains("bus=/run/user/fake\n"), "{output:?}");
            assert!(!result.out_of_memory && !result.too_many_processes);
        });
    }

    #[test]
    fn stop_takes_the_escaped_children_of_a_scoped_session() {
        with_fake_scopes("stop_takes_the_escaped_children_of_a_scoped_session", |parent| {
            let mut child = crate::spawn_piped(&sh(&format!("{ESCAPE}; exec sleep 600"))).unwrap();
            assert_eq!(child.containment(), super::super::ContainmentLevel::Cgroup);
            let mut reader = child.take_reader().unwrap();
            let mut output = Vec::new();
            let mut buffer = [0_u8; 256];
            while !String::from_utf8_lossy(&output).contains('\n') {
                let read = std::io::Read::read(&mut reader, &mut buffer).unwrap();
                assert!(read > 0, "no output");
                output.extend_from_slice(buffer.get(..read).unwrap());
            }
            let escaped = number_after(&String::from_utf8_lossy(&output), "escaped ");
            // The program kept its process ID through the fake systemd-run's
            // exec, and the scope holds it and the escaped sleeper.
            let scope = scopes_in(parent, "run").pop().unwrap();
            let pids = cgroup_pids(&scope);
            assert!(pids.contains(&i32::try_from(child.pid()).unwrap()), "{pids:?}");
            assert!(pids.contains(&escaped), "{pids:?}");
            child.stop();
            let exit = child.wait().unwrap();
            assert!(exit.stopped, "{exit:?}");
            let gone = is_gone(escaped);
            if !gone {
                let _ = kill_process(Pid::from_raw(escaped).unwrap(), Signal::KILL);
            }
            assert!(gone, "the escaped sleeper {escaped} outlived Stop");
        });
    }

    #[test]
    fn a_scope_without_the_pids_controller_counts_its_processes() {
        with_fake_scopes(
            "a_scope_without_the_pids_controller_counts_its_processes",
            |parent| {
                let mut command = sh("i=0; while [ $i -lt 300 ]; do sleep 60 & i=$((i+1)); done; wait");
                command.limits(Limits {
                    processes: Some(16),
                    timeout: Some(Duration::from_secs(30)),
                    ..Limits::default()
                });
                let result = crate::run_captured(&command).unwrap();
                assert!(result.too_many_processes, "{result:?}");
                assert!(!result.timed_out);
                let scope = scopes_in(parent, "build").pop().unwrap();
                assert!(wait_until_empty(&scope), "left: {:?}", cgroup_pids(&scope));
            },
        );
    }

    #[test]
    fn a_scope_without_the_memory_controller_keeps_the_rss_watchdog() {
        with_fake_scopes(
            "a_scope_without_the_memory_controller_keeps_the_rss_watchdog",
            |_| {
                // The shell holds a 400 MB string; the cap is 128 MiB.
                let mut command = sh("x=$(head -c 400000000 /dev/zero | tr '\\0' a); sleep 30");
                command.limits(Limits {
                    rss_limit: Some(128 * 1024 * 1024),
                    timeout: Some(Duration::from_secs(30)),
                    ..Limits::default()
                });
                let result = crate::run_captured(&command).unwrap();
                assert!(result.out_of_memory, "{result:?}");
                assert!(!result.timed_out);
            },
        );
    }

    #[test]
    fn stale_scopes_in_the_scope_folder_are_killed() {
        with_fake_scopes("stale_scopes_in_the_scope_folder_are_killed", |parent| {
            // A scope of a "dead" owner holding an escaped sleeper.
            let dead = std::process::id().wrapping_add(1_000_000);
            let stale = parent.join(format!("{}.scope", unit_name("run", dead, 7)));
            fs::create_dir(&stale).unwrap();
            let mut child = crate::spawn_piped(&{
                let mut command = sh(&format!("read go; {ESCAPE}"));
                command.containment(Containment::ProcessGroupOnly);
                command
            })
            .unwrap();
            let mut reader = child.take_reader().unwrap();
            write_control(&stale.join("cgroup.procs"), child.pid().to_string().as_bytes()).unwrap();
            std::io::Write::write_all(&mut child.writer(), b"go\n").unwrap();
            let mut output = String::new();
            std::io::Read::read_to_string(&mut reader, &mut output).unwrap();
            assert!(child.wait().unwrap().status.success());
            let escaped = number_after(&output, "escaped ");
            assert_eq!(cgroup_pids(&stale), [escaped]);
            // A live scope of this process next to it is left alone.
            let mine = parent.join(format!("{}.scope", unit_name("run", std::process::id(), 8)));
            fs::create_dir(&mine).unwrap();

            let found = stale_scopes(parent, std::process::id(), &is_alive);
            assert_eq!(found, std::slice::from_ref(&stale));
            for scope in &found {
                kill_cgroup(scope);
            }
            let gone = is_gone(escaped);
            if !gone {
                let _ = kill_process(Pid::from_raw(escaped).unwrap(), Signal::KILL);
            }
            assert!(gone, "the stale scope's sleeper {escaped} survived");
            assert!(wait_until_empty(&stale));
        });
    }

    /// A shell that waits for a line on its input before it starts a
    /// `setsid` sleeper and a plain one, so it can be moved into a cgroup
    /// first.
    fn waiting_sleepers() -> crate::PtyChild {
        let mut command = Command::new("/bin/sh", "/").unwrap();
        command
            .args([
                "-c",
                "read go; /usr/bin/setsid /bin/sleep 60 & /bin/sleep 60 & echo started; wait",
            ])
            .env("PATH", "/usr/bin:/bin")
            .containment(Containment::ProcessGroupOnly);
        crate::spawn_piped(&command).unwrap()
    }

    fn wait_for_pids(dir: &Path, count: usize) -> Vec<i32> {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let pids = cgroup_pids(dir);
            if pids.len() >= count || Instant::now() > until {
                return pids;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_until_empty(dir: &Path) -> bool {
        let until = Instant::now() + Duration::from_secs(10);
        while Instant::now() < until {
            if cgroup_pids(dir).is_empty() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    fn kills_everything_in_a_real_cgroup(kill: fn(&Path)) {
        if !Path::new("/usr/bin/setsid").exists() {
            eprintln!("skipped: /usr/bin/setsid is not installed");
            return;
        }
        let Some(scratch) = Scratch::new() else {
            eprintln!("skipped: no cgroup v2 hierarchy this test may create a cgroup in");
            return;
        };
        let dir = scratch.0.clone();
        let mut child = waiting_sleepers();
        // Kept open until the end: the shell must not die of SIGPIPE.
        let output = child.take_reader();
        if write_control(&dir.join("cgroup.procs"), child.pid().to_string().as_bytes()).is_err() {
            child.kill();
            eprintln!("skipped: cannot move a process into {}", dir.display());
            return;
        }
        std::io::Write::write_all(&mut child.writer(), b"go\n").unwrap();
        // The shell, the setsid sleeper and the plain sleeper.
        let pids = wait_for_pids(&dir, 3);
        assert!(pids.len() >= 3, "{pids:?}");
        kill(&dir);
        assert!(wait_until_empty(&dir), "left: {:?}", cgroup_pids(&dir));
        let exit = child.wait().unwrap();
        assert_eq!(exit.status, crate::ExitStatus::Signaled(9));
        drop(output);
    }

    #[test]
    fn cgroup_kill_takes_processes_that_left_the_group() {
        kills_everything_in_a_real_cgroup(kill_cgroup);
    }

    #[test]
    fn the_freezing_fallback_takes_them_too() {
        kills_everything_in_a_real_cgroup(freeze_and_kill);
    }

    proptest! {
        #[test]
        fn names_never_panic_the_parser(name in ".{0,80}") {
            let _ = scope_owner(&name);
        }

        #[test]
        fn every_generated_name_parses(word in "[a-z]{1,16}", owner in 1_u32.., suffix in any::<u64>()) {
            let name = format!("{}.scope", unit_name(&word, owner, suffix));
            prop_assert_eq!(scope_owner(&name), Some(owner));
        }

        #[test]
        fn trial_output_never_panics(text in ".{0,200}") {
            let _ = scope_parent(&text, "b2c-detect-1-0000000000000001", Path::new("/nonexistent"));
        }
    }
}
