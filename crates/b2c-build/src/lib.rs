//! Build and run orchestration: the pipeline from a project file to a running
//! program (`docs/spec/07-toolchain-build-run.md` §7.5–7.6).
//!
//! * [`frontend`]: the pure part, project bytes to generated C++.
//! * [`build_dir`]: the per-project build directory and safe file writing.
//! * [`session`]: the app's build sessions, one thread per build, with
//!   cancellation, progress and diagnostics events and build records.
//! * [`build`]: the same build, synchronously, for the command-line tool.
//! * [`ide`]: the IDE init unit that the app links into the programs it runs.
//! * [`cache`]: build cache eviction and clearing.
//! * [`run_program`] / [`run_program_captured`]: running what was built
//!   (the CLI).
//! * [`RunSessions`]: the IDE's running programs, in a pseudo-terminal with
//!   streamed, flood-protected output, input, resizing, Stop and exit
//!   decoding; [`run_environment`] gives their environment.
//! * [`list_toolchains`]: the compilers on this computer.
//!
//! ```no_run
//! use b2c_build::{BuildOutcome, BuildRequest, Configuration, FrontendOptions, ToolchainChoice};
//!
//! let project = std::fs::read("examples/hello_world.b2c")?;
//! let request = BuildRequest {
//!     configuration: Configuration::Debug,
//!     toolchain: ToolchainChoice::Auto,
//!     cache_root: b2c_build::default_cache_root().expect("a cache folder"),
//!     frontend: FrontendOptions::default(),
//!     ide: false,
//! };
//! let report = b2c_build::build(&project, &request)?;
//! if let BuildOutcome::Built { executable } = report.outcome {
//!     println!("built {}", executable.display());
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

// Failures to choose a toolchain are user-facing `Diagnostic`s (about 170
// bytes), made once per build; boxing them would add noise without benefit.
#![allow(clippy::result_large_err)]

pub mod build_dir;
pub mod cache;
mod compile;
pub mod frontend;
pub mod ide;
mod manifest;
mod run;
pub mod session;
mod toolchains;

pub use build_dir::{
    BuildDir, BuildDirError, default_cache_root, sandbox_dir, write_executable, write_generated_files,
    write_if_changed,
};
pub use compile::{
    BuildError, BuildOutcome, BuildReport, BuildRequest, Configuration, LIBRARIES_UNSUPPORTED, build,
};
pub use frontend::{
    DEFAULT_INDENT_WIDTH, Frontend, FrontendOptions, GENERATOR_INCOMPLETE, INDENT_WIDTHS, Stage, run_frontend,
};
pub use run::{
    ASAN_OPTIONS, ASAN_OPTIONS_NO_LEAKS, CapturedRun, ProgramExit, ProgramInput, PtySize, RunEnvOptions,
    RunError, RunRequest, RunSessions, RunSpec, TERM, UBSAN_OPTIONS, run_environment, run_program,
    run_program_captured,
};
pub use session::{BuildJob, BuildRecord, BuildSessions, RecordOutcome, StaleReason, ToolchainForBuild};
pub use toolchains::{ToolchainChoice, ToolchainReport, list_toolchains};
