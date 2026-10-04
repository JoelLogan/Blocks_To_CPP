//! Running a built program (`docs/spec/07-toolchain-build-run.md` §7.6).

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use b2c_process::{Command, ExitStatus, Limits, Stdin};

/// Where the program's standard input comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramInput {
    /// The terminal (interactive runs).
    Terminal,
    /// The contents of a file.
    File(PathBuf),
    /// These bytes (tests).
    Bytes(Vec<u8>),
}

/// How to run a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRequest {
    /// The executable (absolute).
    pub executable: PathBuf,
    /// Its arguments (never a shell string).
    pub args: Vec<OsString>,
    /// Its standard input.
    pub input: ProgramInput,
    /// Stop it after this long.
    pub timeout: Option<Duration>,
    /// Its working directory (absolute).
    pub working_directory: PathBuf,
}

/// How a program ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramExit {
    /// It ended itself with this exit code.
    Code(i32),
    /// Unix: a signal stopped it.
    Signal(i32),
    /// Windows: an unhandled exception (an `NTSTATUS` code) ended it.
    Exception(u32),
    /// The timeout stopped it.
    TimedOut,
}

impl ProgramExit {
    /// A friendly description, e.g. *"Crashed: the program tried to use
    /// memory it doesn't own (segmentation fault)."* (spec §7.6.4).
    pub fn describe(self) -> String {
        match self {
            Self::Code(code) => ExitStatus::Exited(code).describe(),
            Self::Signal(signal) => ExitStatus::Signaled(signal).describe(),
            Self::Exception(code) => ExitStatus::Exception(code).describe(),
            Self::TimedOut => String::from("Stopped: the program ran longer than the time limit"),
        }
    }

    fn from_status(status: ExitStatus, timed_out: bool) -> Self {
        if timed_out {
            return Self::TimedOut;
        }
        match status {
            ExitStatus::Exited(code) => Self::Code(code),
            ExitStatus::Signaled(signal) => Self::Signal(signal),
            ExitStatus::Exception(code) => Self::Exception(code),
        }
    }
}

/// A problem starting the program.
#[derive(Debug, thiserror::Error)]
#[error("could not run the program: {0}")]
pub struct RunError(#[from] b2c_process::ProcessError);

/// The result of [`run_program_captured`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedRun {
    /// How it ended.
    pub exit: ProgramExit,
    /// Its standard output (capped; see [`b2c_process::DEFAULT_OUTPUT_CAP`]).
    pub stdout: Vec<u8>,
    /// Its standard error (capped).
    pub stderr: Vec<u8>,
}

/// The environment for a user's program: the user's own environment
/// (programs legitimately need it), as spec §7.6.2 says.
fn program_environment() -> impl Iterator<Item = (OsString, OsString)> {
    std::env::vars_os()
}

fn command(request: &RunRequest, stdin: Stdin) -> Result<Command, RunError> {
    let mut command = Command::new(&request.executable, &request.working_directory)?;
    command
        .args(request.args.iter().cloned())
        .envs(program_environment())
        .stdin(stdin);
    if let Some(timeout) = request.timeout {
        command.limits(Limits {
            timeout: Some(timeout),
            ..Limits::default()
        });
    }
    Ok(command)
}

fn stdin_for(input: &ProgramInput) -> Stdin {
    match input {
        ProgramInput::Terminal => Stdin::Inherit,
        ProgramInput::File(path) => Stdin::File(path.clone()),
        ProgramInput::Bytes(bytes) => Stdin::Bytes(bytes.clone()),
    }
}

/// Runs a program attached to this process's terminal: its output appears
/// directly, and its input is the terminal unless the request names a file.
///
/// # Errors
/// [`RunError`] if the program cannot be started.
pub fn run_program(request: &RunRequest) -> Result<ProgramExit, RunError> {
    let finished = b2c_process::run_interactive(&command(request, stdin_for(&request.input))?)?;
    Ok(ProgramExit::from_status(finished.status, finished.timed_out))
}

/// Runs a program with its output captured (for tests and tools).
///
/// # Errors
/// [`RunError`] if the program cannot be started.
pub fn run_program_captured(request: &RunRequest) -> Result<CapturedRun, RunError> {
    let captured = b2c_process::run_captured(&command(request, stdin_for(&request.input))?)?;
    Ok(CapturedRun {
        exit: ProgramExit::from_status(captured.status, captured.timed_out),
        stdout: captured.stdout,
        stderr: captured.stderr,
    })
}
