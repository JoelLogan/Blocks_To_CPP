//! Running a command: spawning, supervising (timeout, cancellation,
//! watchdog) and collecting the result.

use std::fs::File;
use std::io::{self, IsTerminal as _};
use std::process::{Child, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::capture::{Reader, spawn_writer};
use crate::command::{Command, DEFAULT_INTERACTIVE_GRACE, ProcessGroup, Stdin};
use crate::error::ProcessError;
use crate::platform::{self, Placement, Tree};
use crate::status::ExitStatus;

/// Shortest and longest pause between checks on a running child. Polling
/// (rather than a blocking wait on another thread) means the tree is only
/// ever signalled while the child is known not to have been reaped, so its
/// process ID can never have been reused.
const FIRST_POLL: Duration = Duration::from_millis(1);
const MAX_POLL: Duration = Duration::from_millis(25);
/// How often the process-count watchdog looks at the tree.
const WATCHDOG_INTERVAL: Duration = Duration::from_millis(100);
/// How long to wait for the output pipes to close after the program ended.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// The result of [`run_captured`].
#[allow(clippy::struct_excessive_bools)] // independent facts about one run
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    /// How the program ended.
    pub status: ExitStatus,
    /// Standard output, at most [`crate::Limits::stdout_cap`] bytes.
    pub stdout: Vec<u8>,
    /// Standard error, at most [`crate::Limits::stderr_cap`] bytes.
    pub stderr: Vec<u8>,
    /// Whether standard output was cut off at the cap (or, rarely, because a
    /// process that left the tree kept the pipe open after the run).
    pub stdout_truncated: bool,
    /// Whether standard error was cut off (see `stdout_truncated`).
    pub stderr_truncated: bool,
    /// Time from start to exit.
    pub duration: Duration,
    /// Whether the timeout stopped the program.
    pub timed_out: bool,
    /// Whether a [`crate::CancelToken`] stopped the program.
    pub cancelled: bool,
    /// Whether the process-count watchdog stopped the program (Linux; see
    /// [`crate::Limits`]).
    pub too_many_processes: bool,
}

/// The result of [`run_interactive`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finished {
    /// How the program ended.
    pub status: ExitStatus,
    /// Time from start to exit.
    pub duration: Duration,
    /// Whether the timeout stopped the program.
    pub timed_out: bool,
    /// Whether a [`crate::CancelToken`] stopped the program.
    pub cancelled: bool,
    /// Whether the process-count watchdog stopped the program.
    pub too_many_processes: bool,
}

/// Runs `command` to completion with standard output and error captured.
///
/// Both streams are read on helper threads while the program runs, so it
/// can never block on a full pipe; each keeps at most its cap and flags the
/// rest as truncated. On timeout or cancellation the whole tree is killed at
/// once (unless [`crate::Limits::grace`] asks for a polite `SIGTERM` first).
/// When the program exits, anything it left running in its tree is killed
/// too.
///
/// ```
/// # #[cfg(unix)] {
/// use std::time::Duration;
/// use b2c_process::{Command, ExitStatus, run_captured};
///
/// let mut command = Command::new("/bin/sh", std::env::temp_dir())?;
/// command.args(["-c", "echo out; echo err >&2; exit 3"]);
/// let captured = run_captured(&command)?;
/// assert_eq!(captured.status, ExitStatus::Exited(3));
/// assert_eq!(captured.stdout, b"out\n");
/// assert_eq!(captured.stderr, b"err\n");
///
/// let mut sleeper = Command::new("/bin/sh", std::env::temp_dir())?;
/// sleeper.args(["-c", "sleep 30"]).timeout(Duration::from_millis(100));
/// assert!(run_captured(&sleeper)?.timed_out);
/// # }
/// # Ok::<(), b2c_process::ProcessError>(())
/// ```
///
/// # Errors
/// [`ProcessError`] if the program cannot be started (missing, not
/// executable, unreadable input file), cannot be put under control, or was
/// cancelled before it started.
pub fn run_captured(command: &Command) -> Result<Captured, ProcessError> {
    let mut running = start(command, Mode::Captured)?;
    let stdout = running.child.stdout.take();
    let stderr = running.child.stderr.take();
    let limits = command.get_limits();
    let readers = stdout
        .map(|pipe| Reader::spawn("stdout", pipe, limits.stdout_cap))
        .transpose()
        .and_then(|out| {
            let err = stderr
                .map(|pipe| Reader::spawn("stderr", pipe, limits.stderr_cap))
                .transpose()?;
            Ok((out, err))
        });
    let (out_reader, err_reader) = match readers {
        Ok(readers) => readers,
        Err(source) => {
            running.abandon();
            return Err(ProcessError::Spawn {
                program: command.program().to_path_buf(),
                source,
            });
        }
    };
    let supervised = running.supervise(command, Duration::ZERO)?;
    let drain_deadline = Instant::now() + DRAIN_GRACE;
    let collect = |reader: Option<Reader>| match reader {
        Some(reader) => {
            let (output, complete) = reader.finish(drain_deadline);
            (output.data, output.truncated || !complete)
        }
        None => (Vec::new(), false),
    };
    let (stdout, stdout_truncated) = collect(out_reader);
    let (stderr, stderr_truncated) = collect(err_reader);
    Ok(Captured {
        status: supervised.status,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        duration: supervised.duration,
        timed_out: supervised.reason == Some(StopReason::Timeout),
        cancelled: supervised.reason == Some(StopReason::Cancelled),
        too_many_processes: supervised.reason == Some(StopReason::Processes),
    })
}

/// Runs `command` attached to this process's terminal: standard output and
/// error are inherited, and standard input is whatever the command says
/// (usually [`Stdin::Inherit`] for the terminal, or a file for `--stdin`).
///
/// On timeout or cancellation the tree gets `SIGTERM`, then `SIGKILL` after
/// [`crate::Limits::grace`] (default [`DEFAULT_INTERACTIVE_GRACE`]) on Unix;
/// Windows terminates the job at once. See [`ProcessGroup`] for how terminal
/// input and Ctrl+C keep working on Unix.
///
/// ```no_run
/// use std::time::Duration;
/// use b2c_process::{Command, Stdin, run_interactive};
///
/// let mut command = Command::new("/home/ada/.cache/blocks2cpp/builds/p/out/game", "/home/ada/game")?;
/// command
///     .envs(std::env::vars_os())
///     .stdin(Stdin::Inherit)
///     .timeout(Duration::from_secs(60));
/// let finished = run_interactive(&command)?;
/// if finished.timed_out {
///     eprintln!("stopped after 60 s");
/// } else {
///     eprintln!("{}", finished.status.describe());
/// }
/// # Ok::<(), b2c_process::ProcessError>(())
/// ```
///
/// # Errors
/// As for [`run_captured`].
pub fn run_interactive(command: &Command) -> Result<Finished, ProcessError> {
    let running = start(command, Mode::Interactive)?;
    let supervised = running.supervise(command, DEFAULT_INTERACTIVE_GRACE)?;
    Ok(Finished {
        status: supervised.status,
        duration: supervised.duration,
        timed_out: supervised.reason == Some(StopReason::Timeout),
        cancelled: supervised.reason == Some(StopReason::Cancelled),
        too_many_processes: supervised.reason == Some(StopReason::Processes),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Captured,
    Interactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopReason {
    Timeout,
    Cancelled,
    Processes,
}

/// A started, contained child.
struct Running {
    child: Child,
    tree: Tree,
    program: std::path::PathBuf,
    started: Instant,
}

/// What supervision observed.
struct Supervised {
    status: ExitStatus,
    duration: Duration,
    reason: Option<StopReason>,
}

/// Decides the process-group placement (see [`ProcessGroup`]).
fn placement(command: &Command, mode: Mode) -> Placement {
    let shared = match command.get_process_group() {
        ProcessGroup::New => false,
        ProcessGroup::Shared => true,
        ProcessGroup::Auto => match mode {
            Mode::Interactive => {
                io::stdin().is_terminal() || io::stdout().is_terminal() || io::stderr().is_terminal()
            }
            Mode::Captured => *command.get_stdin() == Stdin::Inherit && io::stdin().is_terminal(),
        },
    };
    if shared {
        Placement::SharedGroup
    } else {
        Placement::NewGroup
    }
}

/// Spawns and contains the child.
fn start(command: &Command, mode: Mode) -> Result<Running, ProcessError> {
    if command.get_cancel().is_some_and(crate::CancelToken::is_cancelled) {
        return Err(ProcessError::Cancelled);
    }
    let program = command.program().to_path_buf();
    // An absolute program, an argv list, an explicit directory and a cleared
    // environment (`docs/spec/08-security.md` §8.5).
    let mut std_command = command.to_std();
    let mut bytes = None;
    let stdin = match command.get_stdin() {
        Stdin::Null => Stdio::null(),
        Stdin::Inherit => Stdio::inherit(),
        Stdin::Bytes(data) => {
            bytes = Some(data.clone());
            Stdio::piped()
        }
        Stdin::File(path) => match File::open(path) {
            Ok(file) => Stdio::from(file),
            Err(source) => {
                return Err(ProcessError::StdinFile {
                    path: path.clone(),
                    source,
                });
            }
        },
    };
    std_command.stdin(stdin);
    match mode {
        Mode::Captured => std_command.stdout(Stdio::piped()).stderr(Stdio::piped()),
        Mode::Interactive => std_command.stdout(Stdio::inherit()).stderr(Stdio::inherit()),
    };
    let placement = placement(command, mode);
    platform::configure(&mut std_command, placement, mode == Mode::Captured);

    let mut child = std_command.spawn().map_err(|source| ProcessError::Spawn {
        program: program.clone(),
        source,
    })?;
    let started = Instant::now();
    let tree = match platform::contain(&mut child, placement, command.get_limits()) {
        Ok(tree) => tree,
        Err(source) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ProcessError::Containment { program, source });
        }
    };
    let mut running = Running {
        child,
        tree,
        program,
        started,
    };
    if let (Some(bytes), Some(pipe)) = (bytes, running.child.stdin.take())
        && let Err(source) = spawn_writer(pipe, bytes)
    {
        let program = running.program.clone();
        running.abandon();
        return Err(ProcessError::Spawn { program, source });
    }
    Ok(running)
}

impl Running {
    /// Kills the tree and reaps the child after a setup failure.
    fn abandon(mut self) {
        self.tree.kill();
        let _ = self.child.wait();
    }

    /// Waits for the child, enforcing the timeout, cancellation and the
    /// process watchdog, then cleans up the tree and reaps the child.
    fn supervise(mut self, command: &Command, default_grace: Duration) -> Result<Supervised, ProcessError> {
        let limits = command.get_limits();
        let deadline = limits
            .timeout
            .and_then(|timeout| self.started.checked_add(timeout));
        let grace = limits.grace.unwrap_or(default_grace);
        let cancel = command.get_cancel();
        let mut poll = FIRST_POLL;
        let mut next_watchdog = self.started + WATCHDOG_INTERVAL;
        let mut reason = None;
        let mut ended = None;
        loop {
            if self.exited()? {
                ended = Some(Instant::now());
                break;
            }
            let now = Instant::now();
            if cancel.is_some_and(crate::CancelToken::is_cancelled) {
                reason = Some(StopReason::Cancelled);
            } else if deadline.is_some_and(|deadline| now >= deadline) {
                reason = Some(StopReason::Timeout);
            } else if let Some(max) = limits.processes
                && now >= next_watchdog
            {
                next_watchdog = now + WATCHDOG_INTERVAL;
                let max = usize::try_from(max).unwrap_or(usize::MAX);
                if self.tree.process_count().is_some_and(|count| count > max) {
                    reason = Some(StopReason::Processes);
                }
            }
            if reason.is_some() {
                self.stop_tree(grace)?;
                break;
            }
            let mut pause = poll;
            if let Some(deadline) = deadline {
                pause = pause.min(deadline.saturating_duration_since(now));
            }
            thread::sleep(pause.max(Duration::from_micros(100)));
            poll = (poll * 2).min(MAX_POLL);
        }
        // Anything the child left running goes too, before it is reaped.
        self.tree.after_exit();
        let status = self.child.wait().map_err(|source| ProcessError::Wait {
            program: self.program.clone(),
            source,
        })?;
        let ended = ended.unwrap_or_else(Instant::now);
        Ok(Supervised {
            status: ExitStatus::from_std(status),
            duration: ended.saturating_duration_since(self.started),
            reason,
        })
    }

    fn exited(&mut self) -> Result<bool, ProcessError> {
        platform::has_exited(&mut self.child).map_err(|source| ProcessError::Wait {
            program: self.program.clone(),
            source,
        })
    }

    /// Stops the tree: a polite request and up to `grace` to exit, then a
    /// forced kill.
    fn stop_tree(&mut self, grace: Duration) -> Result<(), ProcessError> {
        if !grace.is_zero() {
            self.tree.stop();
            let until = Instant::now() + grace;
            while Instant::now() < until {
                if self.exited()? {
                    break;
                }
                thread::sleep(MAX_POLL);
            }
        }
        self.tree.kill();
        Ok(())
    }
}
