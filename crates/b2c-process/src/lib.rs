//! Spawning processes safely (`docs/spec/07-toolchain-build-run.md` §7.5.2,
//! §7.6; `docs/spec/08-security.md` §8.5, §8.7).
//!
//! This is the only crate in Blocks2Cpp that starts processes. Everything
//! else (the compiler, probe programs, the user's program) goes through the
//! entry points here:
//!
//! * [`run_captured`]: runs a command with its standard output and error
//!   captured in memory (up to a cap), for the compiler, toolchain probes and
//!   golden tests;
//! * [`run_interactive`]: runs a command attached to this process's terminal
//!   (standard output and error inherited), for `b2c run`;
//! * [`spawn_pty`] and [`spawn_piped`]: start an interactive *session* for
//!   the IDE's console: the program runs in a pseudo-terminal (openpty on
//!   Linux, `ConPTY` on Windows) or, as a fallback, with pipes, and the caller
//!   streams its output, writes its input, resizes the terminal and stops it
//!   through the returned [`PtyChild`].
//!
//! # Rules
//!
//! * A [`Command`] is an **absolute** program path plus an argv list, never a
//!   shell string. On Windows only `.exe` programs are accepted, so `.bat` and
//!   `.cmd` scripts (which go through `cmd.exe` parsing, CVE-2024-24576
//!   `BatBadBut`) can never run.
//! * The environment is exactly what the caller passes: nothing is inherited
//!   implicitly. The working directory is explicit and absolute.
//! * Every run can have a wall-clock timeout and can be cancelled from another
//!   thread with a [`CancelToken`]. Captured output is capped: the rest is
//!   read and thrown away (so the child never blocks on a full pipe) and the
//!   result is flagged as truncated.
//! * The whole process tree is stopped on timeout or cancellation, and any
//!   process the program left behind is stopped when it exits:
//!   * **Linux/Unix:** the child runs in a new process group, which is killed
//!     as a whole (`SIGKILL`; interactive runs and sessions get `SIGTERM`
//!     first and a grace period, and captured runs do when
//!     [`Limits::grace`] is set). See [`ProcessGroup`] for the one
//!     exception: interactive runs attached to a terminal. Sessions lead a
//!     new session (`setsid`), so their process group ID is their process
//!     ID; [`ContainmentLevel`] says how complete this is.
//!   * **Windows:** the child is created suspended, assigned to a Job Object
//!     with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and only then resumed, so no
//!     descendant can be created outside the job. The job is terminated on
//!     timeout, cancellation and exit, and closing it (even when this process
//!     dies) kills everything still in it. Sessions inherit no handles but
//!     their own terminal or pipe ends (an explicit handle list).
//! * Optional resource limits: see [`Limits`] for exactly what is enforced on
//!   each platform.
//!
//! # Example
//!
//! ```
//! # #[cfg(unix)] {
//! use std::time::Duration;
//! use b2c_process::{Command, Stdin, run_captured};
//!
//! let mut command = Command::new("/bin/sh", std::env::temp_dir())?;
//! command
//!     .args(["-c", "read name; echo \"hello $name\""])
//!     .env("PATH", "/usr/bin:/bin")
//!     .stdin(Stdin::Bytes(b"Ada\n".to_vec()))
//!     .timeout(Duration::from_secs(10));
//! let captured = run_captured(&command)?;
//! assert!(captured.status.success());
//! assert_eq!(captured.stdout, b"hello Ada\n");
//! # }
//! # Ok::<(), b2c_process::ProcessError>(())
//! ```
//!
//! # Unsafe code
//!
//! This is the only crate allowed to contain `unsafe`
//! (`docs/spec/02-architecture.md` §2.3), and only in the platform modules
//! that call the operating system: `platform/windows.rs` (Job Objects),
//! `pty/windows.rs` (`ConPTY`, `CreateProcessW`, pipes) and `pty/unix.rs` (the
//! `pre_exec` hook that makes a session's program a session leader). Every
//! `unsafe` block has a `// SAFETY:` comment and CODEOWNERS review.

#![deny(unsafe_code)]

mod cancel;
mod capture;
mod command;
mod error;
mod platform;
#[cfg(target_os = "linux")]
mod procfs;
mod pty;
mod run;
mod status;

pub use cancel::CancelToken;
pub use command::{Command, DEFAULT_INTERACTIVE_GRACE, DEFAULT_OUTPUT_CAP, Limits, ProcessGroup, Stdin};
pub use error::ProcessError;
pub use pty::{ContainmentLevel, IoMode, PtyChild, PtyExit, PtySize, PtyWriter, spawn_piped, spawn_pty};
pub use run::{Captured, Finished, run_captured, run_interactive};
pub use status::{Crash, ExitStatus};
