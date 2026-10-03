//! Build and run orchestration: the pipeline from a project file to a running
//! program (`docs/spec/07-toolchain-build-run.md` §7.5–7.6).
//!
//! * [`frontend`]: the pure part, project bytes to generated C++.
//! * [`build_dir`]: the per-project build directory and safe file writing.

pub mod build_dir;
pub mod frontend;

pub use build_dir::{
    BuildDir, BuildDirError, default_cache_root, sandbox_dir, write_generated_files, write_if_changed,
};
pub use frontend::{Frontend, FrontendOptions, Stage, run_frontend};
