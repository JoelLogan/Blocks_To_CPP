//! What to run: program, arguments, working directory, environment, input
//! and limits.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cancel::CancelToken;
use crate::error::ProcessError;

/// Default cap for each captured stream: 4 MiB (spec §7.5.2).
pub const DEFAULT_OUTPUT_CAP: usize = 4 * 1024 * 1024;

/// Default time between the polite stop request (`SIGTERM`) and the forced
/// kill (`SIGKILL`) for interactive runs on Unix (spec §7.5.4).
pub const DEFAULT_INTERACTIVE_GRACE: Duration = Duration::from_secs(2);

/// Where the child's standard input comes from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Stdin {
    /// Nothing: reading gives end-of-file at once.
    #[default]
    Null,
    /// These bytes, then end-of-file. They are written on a separate thread,
    /// so a child that does not read them cannot block the run.
    Bytes(Vec<u8>),
    /// The contents of this file, opened for reading by this process.
    File(PathBuf),
    /// This process's own standard input (normally the terminal), for
    /// interactive runs.
    Inherit,
}

/// How the child is placed relative to this process's process group on Unix.
/// Windows has no process groups in this sense: there the child is always in
/// its own Job Object and this setting is ignored.
///
/// A process in a process group other than the terminal's foreground group
/// cannot read from the terminal (it is stopped with `SIGTTIN`) and does not
/// receive Ctrl+C. Interactive programs need both, so:
///
/// * [`ProcessGroup::New`]: the child leads a new process group. The whole
///   tree is killed with one `killpg`, which is complete and race-free.
/// * [`ProcessGroup::Shared`]: the child stays in this process's group, which
///   is the terminal's foreground group when this process was started from a
///   shell. Terminal input, Ctrl+C (which stops both this process and the
///   program, like any shell pipeline) and Ctrl+Z work exactly as if the
///   program had been started directly. On timeout or cancellation the child
///   and, on Linux, every descendant found in `/proc` are stopped (frozen
///   with `SIGSTOP` until no new ones appear, then killed). A descendant that
///   detached itself from the child before that scan is not found.
/// * [`ProcessGroup::Auto`] (the default): `Shared` for interactive runs when
///   this process's standard input, output or error is a terminal, and for
///   captured runs whose standard input is [`Stdin::Inherit`] from a terminal;
///   `New` otherwise (pipes, files, CI, the app).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProcessGroup {
    /// Decide from the run kind and whether a terminal is attached.
    #[default]
    Auto,
    /// A new process group led by the child.
    New,
    /// This process's own process group.
    Shared,
}

/// Resource limits for one run.
///
/// | Limit | Windows | Linux |
/// |-------|---------|-------|
/// | `timeout` | wall clock, whole tree terminated | wall clock, whole tree killed |
/// | `memory` | Job Object `JobMemoryLimit` (all processes together) | `RLIMIT_AS` set on the child with `prlimit(2)` right after it starts and inherited by its descendants (address space per process, not a total) |
/// | `processes` | Job Object `ActiveProcessLimit` (further process creation fails) | a watchdog counts the processes in the child's process group every 100 ms and kills the group when there are more (only with [`ProcessGroup::New`]) |
/// | output caps | the same everywhere: bytes beyond the cap are read and discarded | |
///
/// Other Unix systems apply the timeout, output caps and memory limit
/// (through `prlimit` where available) but not the process watchdog.
///
/// `RLIMIT_AS` counts reserved address space, so it must not be used for
/// programs built with AddressSanitizer, which reserves terabytes of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Wall-clock limit for the run. `None` means no limit.
    pub timeout: Option<Duration>,
    /// Maximum bytes of standard output kept by [`crate::run_captured`].
    pub stdout_cap: usize,
    /// Maximum bytes of standard error kept by [`crate::run_captured`].
    pub stderr_cap: usize,
    /// Memory limit in bytes (see the table above).
    pub memory: Option<u64>,
    /// Maximum number of processes in the tree (see the table above).
    pub processes: Option<u32>,
    /// Unix: time between `SIGTERM` and `SIGKILL` when the tree is stopped.
    /// `None` means the default: no grace for captured runs (killed at once)
    /// and [`DEFAULT_INTERACTIVE_GRACE`] for interactive runs. Windows always
    /// terminates at once.
    pub grace: Option<Duration>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            timeout: None,
            stdout_cap: DEFAULT_OUTPUT_CAP,
            stderr_cap: DEFAULT_OUTPUT_CAP,
            memory: None,
            processes: None,
            grace: None,
        }
    }
}

/// A program to run: an absolute path, arguments, an explicit working
/// directory and an explicit, complete environment.
///
/// ```
/// # #[cfg(unix)] {
/// use b2c_process::Command;
///
/// let mut command = Command::new("/usr/bin/g++", "/tmp")?;
/// command.arg("-dumpmachine").env("LC_ALL", "C");
/// assert_eq!(command.program(), std::path::Path::new("/usr/bin/g++"));
///
/// assert!(Command::new("g++", "/tmp").is_err()); // relative: refused
/// # }
/// # Ok::<(), b2c_process::ProcessError>(())
/// ```
#[derive(Debug, Clone)]
pub struct Command {
    program: PathBuf,
    args: Vec<OsString>,
    cwd: PathBuf,
    env: BTreeMap<EnvKey, (OsString, OsString)>,
    stdin: Stdin,
    limits: Limits,
    group: ProcessGroup,
    cancel: Option<CancelToken>,
}

/// An environment variable name as the platform compares it: exact on Unix,
/// ASCII case-insensitive on Windows.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EnvKey(OsString);

impl EnvKey {
    fn new(name: &OsStr) -> Self {
        if cfg!(windows) {
            Self(name.to_ascii_uppercase())
        } else {
            Self(name.to_os_string())
        }
    }
}

impl Command {
    /// Creates a command for `program`, run in `working_dir`, with no
    /// arguments, an empty environment, no input and default [`Limits`].
    ///
    /// # Errors
    /// [`ProcessError::RelativeProgram`] or
    /// [`ProcessError::RelativeWorkingDir`] if either path is not absolute,
    /// and [`ProcessError::NotAnExe`] on Windows for anything but an `.exe`.
    pub fn new(program: impl Into<PathBuf>, working_dir: impl Into<PathBuf>) -> Result<Self, ProcessError> {
        let program = program.into();
        let cwd = working_dir.into();
        check_program(&program, cfg!(windows))?;
        if !cwd.is_absolute() {
            return Err(ProcessError::RelativeWorkingDir(cwd));
        }
        Ok(Self {
            program,
            args: Vec::new(),
            cwd,
            env: BTreeMap::new(),
            stdin: Stdin::Null,
            limits: Limits::default(),
            group: ProcessGroup::Auto,
            cancel: None,
        })
    }

    /// Appends one argument. It reaches the program as one `argv` entry; no
    /// shell ever interprets it.
    pub fn arg(&mut self, arg: impl Into<OsString>) -> &mut Self {
        self.args.push(arg.into());
        self
    }

    /// Appends several arguments.
    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Sets one environment variable, replacing an earlier value for the same
    /// name (compared case-insensitively on Windows). Variables that are not
    /// set here do not exist for the child.
    pub fn env(&mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> &mut Self {
        let name = name.into();
        self.env.insert(EnvKey::new(&name), (name, value.into()));
        self
    }

    /// Sets several environment variables (see [`Command::env`]).
    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<OsString>,
        V: Into<OsString>,
    {
        for (name, value) in vars {
            self.env(name, value);
        }
        self
    }

    /// Removes one environment variable set earlier.
    pub fn env_remove(&mut self, name: impl AsRef<OsStr>) -> &mut Self {
        self.env.remove(&EnvKey::new(name.as_ref()));
        self
    }

    /// Sets where standard input comes from.
    pub fn stdin(&mut self, stdin: Stdin) -> &mut Self {
        self.stdin = stdin;
        self
    }

    /// Sets the wall-clock timeout.
    pub fn timeout(&mut self, timeout: Duration) -> &mut Self {
        self.limits.timeout = Some(timeout);
        self
    }

    /// Replaces all limits.
    pub fn limits(&mut self, limits: Limits) -> &mut Self {
        self.limits = limits;
        self
    }

    /// Sets the process-group placement (Unix; see [`ProcessGroup`]).
    pub fn process_group(&mut self, group: ProcessGroup) -> &mut Self {
        self.group = group;
        self
    }

    /// Lets `token` stop this run from another thread.
    pub fn cancel_token(&mut self, token: &CancelToken) -> &mut Self {
        self.cancel = Some(token.clone());
        self
    }

    /// The absolute program path.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// The arguments (without the program itself).
    pub fn get_args(&self) -> &[OsString] {
        &self.args
    }

    /// The working directory.
    pub fn working_dir(&self) -> &Path {
        &self.cwd
    }

    /// The complete environment, sorted by name.
    pub fn get_envs(&self) -> impl Iterator<Item = (&OsStr, &OsStr)> {
        self.env
            .values()
            .map(|(name, value)| (name.as_os_str(), value.as_os_str()))
    }

    /// The value of one environment variable, if set.
    pub fn get_env(&self, name: impl AsRef<OsStr>) -> Option<&OsStr> {
        self.env
            .get(&EnvKey::new(name.as_ref()))
            .map(|(_, value)| value.as_os_str())
    }

    /// Where standard input comes from.
    pub fn get_stdin(&self) -> &Stdin {
        &self.stdin
    }

    /// The limits.
    pub fn get_limits(&self) -> &Limits {
        &self.limits
    }

    /// The process-group placement.
    pub fn get_process_group(&self) -> ProcessGroup {
        self.group
    }

    /// The cancellation token, if any.
    pub(crate) fn get_cancel(&self) -> Option<&CancelToken> {
        self.cancel.as_ref()
    }
}

/// Checks a program path: absolute, and on Windows an `.exe`.
fn check_program(program: &Path, windows: bool) -> Result<(), ProcessError> {
    if windows {
        // `Path::is_absolute` on Windows requires a drive or UNC prefix and a
        // root, so `\foo` and `C:foo` are rejected too.
        if !program.is_absolute() {
            return Err(ProcessError::RelativeProgram(program.to_path_buf()));
        }
        let is_exe = program
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"));
        // A trailing dot or space is stripped by Win32 path normalisation, so
        // `x.bat.` would run `x.bat`; refuse those names outright.
        let name = program
            .file_name()
            .map(OsStr::to_string_lossy)
            .unwrap_or_default();
        if !is_exe || name.ends_with('.') || name.ends_with(' ') {
            return Err(ProcessError::NotAnExe(program.to_path_buf()));
        }
    } else if !program.is_absolute() {
        return Err(ProcessError::RelativeProgram(program.to_path_buf()));
    }
    if program.file_name().is_none() {
        return Err(ProcessError::RelativeProgram(program.to_path_buf()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn relative_programs_are_refused() {
        for program in ["g++", "./g++", "bin/g++", "", "../usr/bin/g++"] {
            assert!(
                matches!(
                    Command::new(program, "/tmp"),
                    Err(ProcessError::RelativeProgram(_))
                ),
                "{program:?}"
            );
        }
        assert!(Command::new("/usr/bin/g++", "/tmp").is_ok());
        assert!(matches!(
            Command::new("/usr/bin/g++", "tmp"),
            Err(ProcessError::RelativeWorkingDir(_))
        ));
        assert!(matches!(
            Command::new("/", "/tmp"),
            Err(ProcessError::RelativeProgram(_))
        ));
    }

    #[test]
    fn windows_rules_accept_only_exe() {
        // `Path::is_absolute` is platform-specific, so only the extension
        // rules are checked here on every platform.
        let absolute = |name: &str| {
            if cfg!(windows) {
                PathBuf::from(format!(r"C:\tools\{name}"))
            } else {
                PathBuf::from(format!("/tools/{name}"))
            }
        };
        for bad in [
            "g++.bat", "g++.cmd", "g++.com", "g++.BAT", "g++", "g++.exe.", "g++.exe ", "x.bat.",
        ] {
            assert!(
                matches!(
                    check_program(&absolute(bad), true),
                    Err(ProcessError::NotAnExe(_) | ProcessError::RelativeProgram(_))
                ),
                "{bad:?}"
            );
        }
        if cfg!(windows) {
            assert!(check_program(&absolute("g++.exe"), true).is_ok());
            assert!(check_program(&absolute("G++.EXE"), true).is_ok());
        }
        assert!(check_program(Path::new("g++.exe"), true).is_err());
    }

    #[test]
    fn environment_is_explicit_and_replaceable() {
        let root = std::env::temp_dir();
        let program = if cfg!(windows) {
            r"C:\Windows\System32\cmd.exe"
        } else {
            "/bin/sh"
        };
        let mut command = Command::new(program, &root).unwrap();
        assert_eq!(command.get_envs().count(), 0);
        command.env("B", "1").env("A", "2").env("B", "3");
        let vars: Vec<_> = command.get_envs().collect();
        assert_eq!(
            vars,
            [
                (OsStr::new("A"), OsStr::new("2")),
                (OsStr::new("B"), OsStr::new("3"))
            ]
        );
        command.env_remove("A");
        assert_eq!(command.get_env("A"), None);
        assert_eq!(command.get_env("B"), Some(OsStr::new("3")));
    }

    #[cfg(windows)]
    #[test]
    fn windows_environment_names_ignore_case() {
        let mut command = Command::new(r"C:\Windows\System32\cmd.exe", r"C:\").unwrap();
        command.env("Path", "a").env("PATH", "b");
        assert_eq!(command.get_envs().count(), 1);
        assert_eq!(command.get_env("path"), Some(OsStr::new("b")));
    }
}
