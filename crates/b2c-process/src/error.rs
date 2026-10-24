//! Why a process could not be run.

use std::io;
use std::path::PathBuf;

/// Why a process could not be started or watched. A program that starts and
/// then fails is not an error: its [`crate::ExitStatus`] says what happened.
#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    /// The program path is not absolute (programs are never looked up on
    /// `PATH` or relative to the current directory).
    #[error("the program path {} is not absolute, so it was not run", .0.display())]
    RelativeProgram(PathBuf),
    /// Windows: the program is not an `.exe` (for example a `.bat` or `.cmd`
    /// script, which would go through `cmd.exe`).
    #[error("{} is not an .exe program, so it was not run (scripts such as .bat and .cmd files are never run)", .0.display())]
    NotAnExe(PathBuf),
    /// The working directory is not absolute.
    #[error("the working folder {} is not an absolute path", .0.display())]
    RelativeWorkingDir(PathBuf),
    /// The file for standard input could not be opened.
    #[error("cannot open {} as the program's input: {source}", path.display())]
    StdinFile {
        /// The file.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// The operating system refused to start the program.
    #[error("cannot start {}: {source}", program.display())]
    Spawn {
        /// The program.
        program: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// The program started but could not be put under control (Windows Job
    /// Object setup failed). It was stopped again before it ran any code.
    #[error("cannot set up process control for {}: {source}", program.display())]
    Containment {
        /// The program.
        program: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// Waiting for the program failed.
    #[error("lost track of {}: {source}", program.display())]
    Wait {
        /// The program.
        program: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// The run was cancelled before the program started.
    #[error("cancelled before the program started")]
    Cancelled,
    /// No pseudo-terminal could be created (Linux: `/dev/ptmx`; Windows: the
    /// pseudo console). Nothing was started; [`crate::spawn_piped`] is the
    /// fallback.
    #[error("cannot create a terminal for the program: {0}")]
    Pty(#[source] io::Error),
    /// A terminal size with a side of 0 or more than
    /// [`crate::PtySize::MAX_SIDE`] characters.
    #[error("a terminal of {cols} columns and {rows} rows is not possible")]
    InvalidPtySize {
        /// The requested columns.
        cols: u16,
        /// The requested rows.
        rows: u16,
    },
    /// Resizing the terminal failed. The program keeps running with the old
    /// size.
    #[error("cannot resize the terminal: {0}")]
    Resize(#[source] io::Error),
    /// The command cannot be handed to the operating system as given (for
    /// example an argument containing a NUL character, or a Windows command
    /// line longer than 32,767 characters). Nothing was started.
    #[error("cannot run {}: {reason}", program.display())]
    InvalidCommand {
        /// The program.
        program: PathBuf,
        /// What is wrong, in plain English.
        reason: &'static str,
    },
    /// [`crate::os::open_https_url`] was given something other than a plain
    /// `https://` URL (see that function for the exact rules). Nothing was
    /// started.
    #[error("refusing to open a link that is not a plain https:// address")]
    InvalidUrl,
    /// No program to open links with was found: on Linux, `xdg-open` is in
    /// neither `/usr/bin` nor `/bin`; on Windows, no program is registered
    /// for `https` links.
    #[error("cannot open the link: no web browser or link opener was found")]
    NoUrlOpener,
    /// The link opener could not be started or reported a failure.
    #[error("cannot open the link: {source}")]
    OpenUrl {
        /// The underlying error.
        source: io::Error,
    },
}
