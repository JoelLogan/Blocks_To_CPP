//! Interactive sessions: a program running in a pseudo-terminal (or, as a
//! fallback, with pipes), streamed to and from the caller while it runs
//! (`docs/spec/07-toolchain-build-run.md` §7.6.2; `docs/spec/08-security.md`
//! §8.7). The behaviour callers rely on is documented on [`PtyChild`].
//!
//! # Structure
//!
//! * this module: the public types, argument checks and the assembly of a
//!   [`PtyChild`] from the platform parts;
//! * `session`: the platform-neutral supervisor thread (exit detection,
//!   whole-tree cleanup, stop with a grace period, cancellation, timeout and
//!   the process watchdog);
//! * `unix`: `/dev/ptmx` terminals or pipes, and the `pre_exec` hook that
//!   makes the program a session leader (`setsid`, `TIOCSCTTY`);
//! * `windows`: `ConPTY` or pipes, `CreateProcessW` with an attribute list,
//!   and the Job Object assigned while the program is suspended;
//! * `cmdline`: the pure Windows command-line quoting and environment block,
//!   compiled (and tested) on every platform.

use std::fmt;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::command::{Command, DEFAULT_INTERACTIVE_GRACE, Stdin};
use crate::error::ProcessError;
use crate::status::ExitStatus;

mod session;

#[cfg(any(windows, test))]
mod cmdline;

#[cfg(unix)]
#[allow(unsafe_code)] // `pre_exec` only; see the SAFETY comment there.
mod unix;
#[cfg(unix)]
use unix as platform;

#[cfg(windows)]
#[allow(unsafe_code)] // Win32 calls; every block has a SAFETY comment.
mod windows;
#[cfg(windows)]
use windows as platform;

/// The size of a session's terminal in character cells.
///
/// ```
/// use b2c_process::PtySize;
///
/// let size = PtySize { cols: 100, rows: 30 };
/// assert_eq!(PtySize::default(), PtySize { cols: 80, rows: 24 });
/// assert!(size.cols <= PtySize::MAX_SIDE);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PtySize {
    /// Columns (characters per line), 1 to [`PtySize::MAX_SIDE`].
    pub cols: u16,
    /// Rows (lines), 1 to [`PtySize::MAX_SIDE`].
    pub rows: u16,
}

impl PtySize {
    /// The largest side the platforms accept (a Windows `COORD` is a signed
    /// 16-bit value). Callers apply their own, smaller bounds (the IDE allows
    /// 2 to 1000 columns and 1 to 1000 rows).
    pub const MAX_SIDE: u16 = 32_767;

    /// Checks that both sides are between 1 and [`PtySize::MAX_SIDE`].
    ///
    /// # Errors
    /// [`ProcessError::InvalidPtySize`] otherwise.
    pub fn validate(self) -> Result<Self, ProcessError> {
        let fits = |side: u16| (1..=Self::MAX_SIDE).contains(&side);
        if fits(self.cols) && fits(self.rows) {
            Ok(self)
        } else {
            Err(ProcessError::InvalidPtySize {
                cols: self.cols,
                rows: self.rows,
            })
        }
    }
}

impl Default for PtySize {
    /// The classic 80 by 24 terminal.
    fn default() -> Self {
        Self { cols: 80, rows: 24 }
    }
}

/// How a session's program is connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IoMode {
    /// A pseudo-terminal (openpty on Linux, `ConPTY` on Windows).
    Pty,
    /// Pipes: standard output and error merged, standard input a pipe. The
    /// fallback when no terminal can be created.
    Pipes,
}

/// How completely a session's process tree is contained, for the run
/// status (`docs/spec/08-security.md` §8.14 item 3).
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

/// How a session's program ended.
#[allow(clippy::struct_excessive_bools)] // independent facts about one run
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtyExit {
    /// The exit status (for a stopped program, the signal or code that
    /// ended it; [`PtyExit::stopped`] says that it was stopped).
    pub status: ExitStatus,
    /// Whether the session ended the program: [`PtyChild::stop`],
    /// [`PtyChild::kill`], dropping the [`PtyChild`], or the command's
    /// [`crate::CancelToken`]. Shown as *Stopped* rather than the decoded
    /// status.
    pub stopped: bool,
    /// Time from start until the exit was detected.
    pub duration: Duration,
    /// Whether the command's [`crate::Limits::timeout`] stopped the program.
    pub timed_out: bool,
    /// Whether the process-count watchdog stopped the program (Linux; see
    /// [`crate::Limits`]).
    pub too_many_processes: bool,
}

/// Writes to a session's program: keystrokes in PTY mode, standard input in
/// pipe mode. Clones write to the same program.
///
/// A write blocks while the program's input buffer is full. Once the
/// program has ended, writes fail with [`std::io::ErrorKind::BrokenPipe`].
#[derive(Clone)]
pub struct PtyWriter {
    inner: Arc<platform::Writer>,
}

impl fmt::Debug for PtyWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PtyWriter").finish_non_exhaustive()
    }
}

impl Write for PtyWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        // Nothing is buffered here: every write goes straight to the
        // terminal or pipe.
        Ok(())
    }
}

/// A running session, started by [`spawn_pty`] or [`spawn_piped`]: one
/// reader for everything the program prints, a cloneable [`PtyWriter`] for
/// its input, and `resize`, `stop`, `kill` and `wait`.
///
/// # Containment
///
/// * **Linux/Unix:** the program leads a new session (`setsid`), so its
///   process group ID equals its process ID and the whole group is
///   signalled at once. In PTY mode the terminal is its controlling terminal,
///   so byte `0x03` written to it reaches the program as `SIGINT`, like
///   Ctrl+C in a real terminal. A process that deliberately leaves the group
///   (`setsid`, a double fork) is out of reach:
///   [`ContainmentLevel::ProcessGroupOnly`].
/// * **Windows:** the program is created suspended, put into a Job Object
///   with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and only then resumed, so no
///   descendant can escape and the tree dies with the job, even when this
///   process dies. It inherits no handle except its own pipe ends (pipe
///   mode) or its pseudo console (PTY mode, Windows 10 1809 or later).
///
/// When the program exits, anything it left running in its tree is killed.
///
/// # Input and output
///
/// In PTY mode the program sees a terminal on standard input, output and
/// error (`isatty` is true) of the requested size; the terminal echoes input
/// and ends output lines with `\r\n`, as any terminal does. In pipe mode
/// standard output and error are merged into the one reader.
///
/// The reader blocks until output is available. After the program has
/// ended it returns end-of-file once the output is complete, or at the
/// latest when no output arrived for 2 s, or 10 s after the end (a process
/// that escaped the tree can hold the terminal open). Writes block while the
/// program's input buffer is full and fail with
/// [`std::io::ErrorKind::BrokenPipe`] once it has ended; a caller that must
/// never block writes from a thread of its own.
///
/// # Sharing and dropping
///
/// `PtyChild` is `Send + Sync`: share it in an `Arc` to wait on one thread
/// while another writes, resizes or stops. Dropping a `PtyChild` kills the
/// program's tree if it is still running.
pub struct PtyChild {
    shared: Arc<session::Shared>,
    /// In a mutex only so that `PtyChild` is `Sync` (the reader itself is
    /// only `Send`); [`PtyChild::take_reader`] takes it without locking.
    reader: Mutex<Option<Box<dyn Read + Send>>>,
    writer: PtyWriter,
    terminal: platform::Terminal,
    mode: IoMode,
    pid: u32,
    program: PathBuf,
}

impl fmt::Debug for PtyChild {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PtyChild")
            .field("pid", &self.pid)
            .field("mode", &self.mode)
            .field("containment", &self.containment())
            .finish_non_exhaustive()
    }
}

/// Starts `command` in a pseudo-terminal of `size`.
///
/// The command's arguments, working directory and environment are used as
/// given (add `TERM` yourself). Its standard input must be [`Stdin::Null`]:
/// input comes from [`PtyChild::writer`]. The program always runs in a new
/// session or process group ([`crate::ProcessGroup`] is ignored).
/// [`crate::Limits`] apply as for [`crate::run_interactive`] (timeout,
/// memory, processes, grace); the output caps do not.
///
/// ```
/// # #[cfg(unix)] {
/// use std::io::{Read as _, Write as _};
/// use b2c_process::{Command, PtySize, spawn_pty};
///
/// let mut command = Command::new("/bin/sh", std::env::temp_dir())?;
/// command.args(["-c", "read name; echo \"hello $name\""]);
/// let mut child = spawn_pty(&command, PtySize { cols: 80, rows: 24 })?;
/// let mut output = child.take_reader().expect("first call");
/// child.writer().write_all(b"Ada\n").expect("the program is running");
/// let mut text = Vec::new();
/// output.read_to_end(&mut text).expect("terminal output");
/// assert!(child.wait()?.status.success());
/// // The terminal echoes the input and ends lines with \r\n.
/// assert!(String::from_utf8_lossy(&text).contains("hello Ada\r\n"));
/// # }
/// # Ok::<(), b2c_process::ProcessError>(())
/// ```
///
/// # Errors
/// [`ProcessError::Pty`] when no terminal can be created (then
/// [`spawn_piped`] may be used instead), [`ProcessError::InvalidPtySize`],
/// [`ProcessError::InvalidCommand`], and the errors of
/// [`crate::run_captured`].
pub fn spawn_pty(command: &Command, size: PtySize) -> Result<PtyChild, ProcessError> {
    start(command, Io::Pty(size.validate()?))
}

/// Starts `command` with pipes: standard output and standard error merged
/// into the one reader, standard input from [`PtyChild::writer`]. Otherwise
/// the same as [`spawn_pty`]; [`PtyChild::resize`] does nothing.
///
/// # Errors
/// As for [`spawn_pty`], except [`ProcessError::Pty`] and
/// [`ProcessError::InvalidPtySize`].
pub fn spawn_piped(command: &Command) -> Result<PtyChild, ProcessError> {
    start(command, Io::Pipes)
}

/// How the platform modules connect the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Io {
    Pty(PtySize),
    Pipes,
}

fn start(command: &Command, io: Io) -> Result<PtyChild, ProcessError> {
    if command.get_cancel().is_some_and(crate::CancelToken::is_cancelled) {
        return Err(ProcessError::Cancelled);
    }
    let program = command.program().to_path_buf();
    if *command.get_stdin() != Stdin::Null {
        return Err(ProcessError::InvalidCommand {
            program,
            reason: "a session's input comes from its writer, so the command's standard input must be Stdin::Null",
        });
    }
    let spawned = platform::spawn(command, io)?;
    let limits = command.get_limits();
    let grace = limits.grace.unwrap_or(DEFAULT_INTERACTIVE_GRACE);
    let shared = session::Shared::new(spawned.tree, spawned.started, grace);
    let reader = spawned.output.into_reader(&shared);
    let writer = PtyWriter {
        inner: Arc::new(spawned.input.into_writer(&shared)),
    };
    let supervision = session::Supervision {
        timeout: limits.timeout,
        processes: limits.processes,
        cancel: command.get_cancel().cloned(),
    };
    session::start(&shared, spawned.program, supervision).map_err(|source| ProcessError::Spawn {
        program: program.clone(),
        source,
    })?;
    Ok(PtyChild {
        shared,
        reader: Mutex::new(Some(reader)),
        writer,
        terminal: spawned.terminal,
        mode: match io {
            Io::Pty(_) => IoMode::Pty,
            Io::Pipes => IoMode::Pipes,
        },
        pid: spawned.pid,
        program,
    })
}

impl PtyChild {
    /// The program's output (standard output and error together), once:
    /// later calls return `None`. Read it on a thread of its own until
    /// end-of-file; the program blocks when its output is not read.
    pub fn take_reader(&mut self) -> Option<Box<dyn Read + Send>> {
        self.reader
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    /// A writer for the program's input. All clones write to the same
    /// program.
    pub fn writer(&self) -> PtyWriter {
        self.writer.clone()
    }

    /// Changes the terminal size; the program gets `SIGWINCH` (Linux) or a
    /// console resize event (Windows). Does nothing in pipe mode or after the
    /// program has ended.
    ///
    /// # Errors
    /// [`ProcessError::InvalidPtySize`], or [`ProcessError::Resize`] if the
    /// system refused the new size.
    pub fn resize(&self, size: PtySize) -> Result<(), ProcessError> {
        let size = size.validate()?;
        if self.shared.has_ended() {
            return Ok(());
        }
        self.terminal.resize(size).map_err(ProcessError::Resize)
    }

    /// Stops the program's whole tree: `SIGTERM` to the process group, then
    /// `SIGKILL` after [`crate::Limits::grace`] (default
    /// [`DEFAULT_INTERACTIVE_GRACE`], 2 s) for anything still running.
    /// Windows terminates the Job Object at once. Returns immediately; the
    /// exit is reported by [`PtyChild::wait`] with [`PtyExit::stopped`] set.
    /// Does nothing once the program has ended.
    pub fn stop(&self) {
        self.shared.stop();
    }

    /// Kills the program's whole tree at once (`SIGKILL`; Windows: the Job
    /// Object is terminated). Does nothing once the program has ended.
    pub fn kill(&self) {
        self.shared.kill();
    }

    /// Waits until the program has ended and returns how. Can be called
    /// again; it returns the same result. It takes `&self`, so one thread
    /// can wait while another calls [`PtyChild::stop`] (share the
    /// `PtyChild` in an `Arc`).
    ///
    /// # Errors
    /// [`ProcessError::Wait`] if the program could not be watched.
    pub fn wait(&self) -> Result<PtyExit, ProcessError> {
        self.shared.wait().map_err(|source| self.wait_error(source))
    }

    /// How the program ended, or `None` while it is running.
    ///
    /// # Errors
    /// As for [`PtyChild::wait`].
    pub fn try_wait(&self) -> Result<Option<PtyExit>, ProcessError> {
        self.shared.try_wait().map_err(|source| self.wait_error(source))
    }

    /// Whether the program runs in a terminal or with pipes.
    pub fn mode(&self) -> IoMode {
        self.mode
    }

    /// How completely the program's tree is contained.
    pub fn containment(&self) -> ContainmentLevel {
        platform::CONTAINMENT
    }

    /// The program's process ID (on Linux also its process group and
    /// session ID).
    pub fn pid(&self) -> u32 {
        self.pid
    }

    fn wait_error(&self, source: io::Error) -> ProcessError {
        ProcessError::Wait {
            program: self.program.clone(),
            source,
        }
    }
}

impl Drop for PtyChild {
    fn drop(&mut self) {
        // Nothing may keep running unobserved: a session that is dropped
        // (closing a project, shutting down) takes its program with it. The
        // supervisor thread still reaps it.
        self.shared.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_can_be_shared_between_threads() {
        fn shareable<T: Send + Sync>() {}
        shareable::<PtyChild>();
        shareable::<PtyWriter>();
        shareable::<PtyExit>();
    }

    #[test]
    fn sizes_are_validated() {
        assert!(PtySize { cols: 1, rows: 1 }.validate().is_ok());
        assert!(
            PtySize {
                cols: PtySize::MAX_SIDE,
                rows: PtySize::MAX_SIDE
            }
            .validate()
            .is_ok()
        );
        for (cols, rows) in [
            (0, 24),
            (80, 0),
            (0, 0),
            (PtySize::MAX_SIDE + 1, 24),
            (80, u16::MAX),
        ] {
            assert!(
                matches!(
                    PtySize { cols, rows }.validate(),
                    Err(ProcessError::InvalidPtySize { cols: c, rows: r }) if c == cols && r == rows
                ),
                "{cols}x{rows}"
            );
        }
    }

    #[test]
    fn invalid_size_error_names_both_sides() {
        let error = PtySize { cols: 0, rows: 5 }.validate().unwrap_err();
        assert_eq!(
            error.to_string(),
            "a terminal of 0 columns and 5 rows is not possible"
        );
    }
}
