//! Spawning processes safely (`docs/spec/07-toolchain-build-run.md` §7.5.2,
//! §7.6; `docs/spec/08-security.md` §8.5, §8.7).
//!
//! CONTRACT (implemented in milestone M1):
//! * Commands are an absolute program path plus an argv list; never a shell.
//!   `.bat`/`.cmd` programs are refused on Windows.
//! * The environment is exactly what the caller passes (nothing inherited
//!   implicitly); the working directory is explicit.
//! * Captured runs enforce a wall-clock timeout and output caps (truncating
//!   with a flag, never growing without bound), and kill the whole process
//!   tree on timeout or cancellation: a new process group + `killpg` on Unix,
//!   a Job Object with `KILL_ON_JOB_CLOSE` (and optional memory/process
//!   limits) on Windows.
//! * Interactive runs (for `b2c run`) inherit the terminal's stdio, support an
//!   optional timeout, and kill the tree on timeout.
//! * Exit status decoding: exit code, Unix signal, or Windows NTSTATUS.
//! * This is the only crate that may use `unsafe` (platform calls only, each
//!   with a `// SAFETY:` comment) and the only one that calls
//!   `std::process::Command::new`.
