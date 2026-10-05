//! Build and run orchestration: the pipeline from a project file to a running
//! program (`docs/spec/07-toolchain-build-run.md` §7.5–7.6).
//!
//! * [`frontend`]: the pure part, project bytes to generated C++.
//! * [`build_dir`]: the per-project build directory and safe file writing.
//! * [`build`]: the front end, then g++ in the build cache.
//! * [`run_program`] / [`run_program_captured`]: running what was built.
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
mod run;
mod toolchains;

pub use build_dir::{
    BuildDir, BuildDirError, default_cache_root, sandbox_dir, write_executable, write_generated_files,
    write_if_changed,
};
pub use compile::{
    BuildError, BuildOutcome, BuildReport, BuildRequest, Configuration, LIBRARIES_UNSUPPORTED, build,
};
pub use frontend::{Frontend, FrontendOptions, GENERATOR_INCOMPLETE, Stage, run_frontend};
pub use run::{
    CapturedRun, ProgramExit, ProgramInput, RunError, RunRequest, run_program, run_program_captured,
};
pub use toolchains::{ToolchainChoice, ToolchainReport, list_toolchains};
