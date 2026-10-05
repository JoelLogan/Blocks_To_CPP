//! Commands that compile and run: `build`, `run` and `toolchains`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use b2c_build::{
    BuildError, BuildOutcome, BuildReport, BuildRequest, Configuration, ProgramExit, ProgramInput,
    RunRequest, ToolchainChoice,
};
use b2c_ir::{Diagnostic, Severity};
use b2c_model::WorkingDirectory;

use super::{err, fail, out};
use crate::project_file::{self, shown};
use crate::report::{self, BlockIndex, terminal_safe};
use crate::{Config, Format, Status};

impl From<Config> for Configuration {
    fn from(config: Config) -> Self {
        match config {
            Config::Debug => Self::Debug,
            Config::Release => Self::Release,
        }
    }
}

/// The cache folder: `--cache-dir` (made absolute, as the compiler's working
/// folder inside it must be), or the per-user default.
fn cache_root(cache_dir: Option<&Path>) -> Result<PathBuf, Status> {
    match cache_dir {
        Some(dir) => std::path::absolute(dir).map_err(|error| {
            fail(&format!("cannot use the cache folder {}: {error}", shown(dir)));
            Status::Usage
        }),
        None => b2c_build::default_cache_root().ok_or_else(|| {
            fail("cannot find a folder for the build cache; pass --cache-dir <dir>");
            Status::Usage
        }),
    }
}

/// Reads and builds a project, printing every diagnostic to standard error.
fn build_project(
    path: &Path,
    config: Config,
    toolchain: Option<&Path>,
    cache_dir: Option<&Path>,
) -> Result<BuildReport, Status> {
    let bytes = project_file::read(path).map_err(|message| {
        fail(&message);
        Status::Usage
    })?;
    let toolchain = match toolchain {
        Some(path) if !path.is_absolute() => {
            fail(&format!(
                "--toolchain must be an absolute path, not {}",
                shown(path)
            ));
            return Err(Status::Usage);
        }
        Some(path) => ToolchainChoice::Path(path.to_path_buf()),
        None => ToolchainChoice::Auto,
    };
    // The CLI never reads the code style (07 §7.9): the default indent width
    // of 4. It never links the IDE init unit, so its builds have their own
    // build folders.
    let request = BuildRequest {
        configuration: config.into(),
        toolchain,
        cache_root: cache_root(cache_dir)?,
        frontend: b2c_build::FrontendOptions::default(),
        ide: false,
    };
    let report = b2c_build::build(&bytes, &request).map_err(|error| {
        fail(&terminal_safe(&error.to_string()));
        match error {
            // The compiler exists (it was probed) but could not be started.
            BuildError::Process(_) => Status::Toolchain,
            BuildError::BuildDir(_)
            | BuildError::Manifest(_)
            | BuildError::Cancelled
            | BuildError::Internal(_) => Status::Usage,
        }
    })?;
    // Notes (a sanitizer the compiler lacks, and so on) would repeat on every
    // build; `b2c check --format json` and the app show them.
    let worth_showing: Vec<Diagnostic> = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity != Severity::Info)
        .cloned()
        .collect();
    err(&report::render_text(
        &shown(path),
        &worth_showing,
        &BlockIndex::new(report.document.as_ref()),
    ));
    Ok(report)
}

/// `b2c build`: compile the project; print the executable's path.
pub(crate) fn build(
    path: &Path,
    config: Config,
    toolchain: Option<&Path>,
    out_path: Option<&Path>,
    cache_dir: Option<&Path>,
) -> Status {
    let report = match build_project(path, config, toolchain, cache_dir) {
        Ok(report) => report,
        Err(status) => return status,
    };
    match report.outcome {
        BuildOutcome::Built { executable } => {
            let shown_path = match out_path {
                Some(destination) => match install(&executable, destination) {
                    Ok(()) => destination.to_path_buf(),
                    Err(error) => {
                        fail(&terminal_safe(&format!(
                            "cannot copy the program to {}: {error}",
                            shown(destination)
                        )));
                        return Status::Usage;
                    }
                },
                None => executable,
            };
            out(&format!("{}\n", shown(&shown_path)));
            Status::Success
        }
        BuildOutcome::ProjectErrors => Status::ProjectErrors,
        BuildOutcome::ToolchainProblem => Status::Toolchain,
    }
}

/// Copies the built program to `destination` like every other file `b2c`
/// writes: atomically, never through a link, and executable.
fn install(executable: &Path, destination: &Path) -> Result<(), String> {
    let contents = std::fs::read(executable).map_err(|error| error.to_string())?;
    let destination = std::path::absolute(destination).map_err(|error| error.to_string())?;
    b2c_build::write_executable(&destination, &contents).map_err(|error| error.to_string())
}

/// `b2c run`: build if needed, then run the program with the terminal (or
/// `--stdin`) as its input.
pub(crate) fn run(
    path: &Path,
    config: Config,
    toolchain: Option<&Path>,
    stdin: Option<&Path>,
    timeout: Option<Duration>,
    cache_dir: Option<&Path>,
    args: &[OsString],
) -> Status {
    let Ok(report) = build_project(path, config, toolchain, cache_dir) else {
        return Status::RunFailed;
    };
    let BuildOutcome::Built { executable } = report.outcome else {
        fail("the program was not built, so it cannot run");
        return Status::RunFailed;
    };
    let Some(document) = report.document else {
        return Status::RunFailed;
    };

    // Arguments given on the command line replace the project's own.
    let args = if args.is_empty() {
        document.project.run.args.iter().map(OsString::from).collect()
    } else {
        args.to_vec()
    };
    let working_directory = match document.project.run.working_directory {
        WorkingDirectory::Project => match project_folder(path) {
            Ok(folder) => folder,
            Err(status) => return status,
        },
        WorkingDirectory::Sandbox => {
            let Ok(root) = cache_root(cache_dir) else {
                return Status::RunFailed;
            };
            match b2c_build::sandbox_dir(&root, &document.project.id) {
                Ok(folder) => folder,
                Err(error) => {
                    fail(&terminal_safe(&format!(
                        "cannot create the program's sandbox folder: {error}"
                    )));
                    return Status::RunFailed;
                }
            }
        }
    };
    let request = RunRequest {
        executable,
        args,
        input: stdin.map_or(ProgramInput::Terminal, |file| {
            ProgramInput::File(file.to_path_buf())
        }),
        timeout,
        working_directory,
    };
    match b2c_build::run_program(&request) {
        Ok(ProgramExit::Code(code)) => Status::Program(code),
        Ok(exit @ ProgramExit::Signal(signal)) => {
            err(&format!("b2c: {}\n", exit.describe()));
            Status::Program(128_i32.saturating_add(signal))
        }
        Ok(exit @ ProgramExit::Exception(code)) => {
            err(&format!("b2c: {}\n", exit.describe()));
            // NTSTATUS codes are 32-bit; pass them on as the same bits.
            Status::Program(i32::from_ne_bytes(code.to_ne_bytes()))
        }
        Ok(ProgramExit::TimedOut) => {
            fail("the program was stopped because it ran longer than --timeout");
            Status::TimedOut
        }
        // Only IDE sessions are stopped by the user; `run_program` never
        // reports it.
        Ok(ProgramExit::Stopped) => {
            fail("the program was stopped");
            Status::RunFailed
        }
        Err(error) => {
            fail(&terminal_safe(&error.to_string()));
            Status::RunFailed
        }
    }
}

/// The folder that contains the project file, as an absolute path.
fn project_folder(path: &Path) -> Result<PathBuf, Status> {
    let absolute = std::path::absolute(path).map_err(|error| {
        fail(&format!("cannot resolve {}: {error}", shown(path)));
        Status::RunFailed
    })?;
    Ok(absolute
        .parent()
        .map_or_else(|| absolute.clone(), Path::to_path_buf))
}

/// `b2c toolchains`: list the compilers found and whether they can be used.
pub(crate) fn toolchains(format: Format) -> Status {
    let found = b2c_build::list_toolchains(b2c_build::default_cache_root().as_deref());
    match format {
        Format::Json => out(&report::render_toolchains_json(&found)),
        Format::Text => out(&report::render_toolchains_text(&found)),
    }
    if found.iter().any(|toolchain| toolchain.usable) {
        Status::Success
    } else {
        Status::Toolchain
    }
}
