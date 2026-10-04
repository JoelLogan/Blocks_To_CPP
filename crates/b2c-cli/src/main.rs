//! The `b2c` command-line tool (`docs/spec/07-toolchain-build-run.md` §7.9).
//!
//! Exit codes: `0` success, `1` project errors, `2` usage (including an
//! unreadable input file), `3` toolchain problem. `b2c run` returns the
//! program's own exit code, or `125` when it could not build or start it.

mod commands;
mod project_file;
mod report;

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

/// Build real C++ programs from Blocks2Cpp projects.
#[derive(Debug, Parser)]
#[command(name = "b2c", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// How diagnostics are printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    /// Human-readable text.
    Text,
    /// One versioned JSON object (for tools and CI).
    Json,
}

/// Which build configuration to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Config {
    /// Fast to build, easy to debug.
    Debug,
    /// Optimised.
    Release,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check a project for problems without building it.
    Check {
        /// The project file (`.b2c`).
        project: PathBuf,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Write the generated C++ files into a folder.
    Generate {
        /// The project file (`.b2c`).
        project: PathBuf,
        /// The folder to write into (created if missing).
        #[arg(long)]
        out: PathBuf,
        /// Leave out the "edit the blocks, not this file" banner.
        #[arg(long)]
        export: bool,
    },
    /// Compile a project into an executable.
    Build {
        /// The project file (`.b2c`).
        project: PathBuf,
        /// Build configuration.
        #[arg(long, value_enum, default_value_t = Config::Debug)]
        config: Config,
        /// The g++ to use (an absolute path); found automatically when omitted.
        #[arg(long)]
        toolchain: Option<PathBuf>,
        /// Copy the finished executable to this path.
        #[arg(long)]
        out: Option<PathBuf>,
        /// The build cache folder (defaults to the per-user cache).
        #[arg(long)]
        cache_dir: Option<PathBuf>,
    },
    /// Build a project if needed, then run it.
    Run {
        /// The project file (`.b2c`).
        project: PathBuf,
        /// Build configuration.
        #[arg(long, value_enum, default_value_t = Config::Debug)]
        config: Config,
        /// The g++ to use (an absolute path); found automatically when omitted.
        #[arg(long)]
        toolchain: Option<PathBuf>,
        /// Feed this file to the program's input instead of the terminal.
        #[arg(long)]
        stdin: Option<PathBuf>,
        /// Stop the program after this long, e.g. `500ms`, `10s`, `2m`.
        #[arg(long, value_parser = commands::parse_duration)]
        timeout: Option<std::time::Duration>,
        /// The build cache folder (defaults to the per-user cache).
        #[arg(long)]
        cache_dir: Option<PathBuf>,
        /// Arguments passed to the program.
        #[arg(last = true)]
        args: Vec<std::ffi::OsString>,
    },
    /// List the C++ compilers found on this computer.
    Toolchains {
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Upgrade a project file to the current format.
    Migrate {
        /// The project file (`.b2c`).
        project: PathBuf,
        /// Rewrite the file instead of printing the result.
        #[arg(long)]
        in_place: bool,
    },
    /// Rewrite a project file in canonical form.
    Fmt {
        /// The project file (`.b2c`).
        project: PathBuf,
        /// Only report whether the file is already formatted.
        #[arg(long)]
        check: bool,
    },
}

/// Process exit statuses (§7.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Success,
    ProjectErrors,
    Usage,
    Toolchain,
    /// `b2c run` stopped the program at `--timeout`.
    TimedOut,
    /// `b2c run` could not build or start the program.
    RunFailed,
    /// `b2c run`: the program's own exit code, passed on unchanged.
    Program(i32),
}

impl From<Status> for ExitCode {
    fn from(status: Status) -> Self {
        Self::from(match status {
            Status::Success => 0,
            Status::ProjectErrors => 1,
            Status::Usage => 2,
            Status::Toolchain => 3,
            Status::TimedOut => 124,
            Status::RunFailed => 125,
            // Only reached on Unix, where exit codes are 0–255; main() passes
            // other codes on through std::process::exit.
            Status::Program(code) => u8::try_from(code & 0xFF).unwrap_or(1),
        })
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            // clap prints help/version to stdout and usage errors to stderr.
            let _ = error.print();
            return ExitCode::from(if error.use_stderr() { 2 } else { 0 });
        }
    };
    let status = match cli.command {
        Command::Check { project, format } => commands::check(&project, format),
        Command::Generate { project, out, export } => commands::generate(&project, &out, export),
        Command::Build {
            project,
            config,
            toolchain,
            out,
            cache_dir,
        } => commands::build(
            &project,
            config,
            toolchain.as_deref(),
            out.as_deref(),
            cache_dir.as_deref(),
        ),
        Command::Run {
            project,
            config,
            toolchain,
            stdin,
            timeout,
            cache_dir,
            args,
        } => commands::run(
            &project,
            config,
            toolchain.as_deref(),
            stdin.as_deref(),
            timeout,
            cache_dir.as_deref(),
            &args,
        ),
        Command::Toolchains { format } => commands::toolchains(format),
        Command::Migrate { project, in_place } => commands::migrate(&project, in_place),
        Command::Fmt { project, check } => commands::fmt(&project, check),
    };
    let _ = std::io::stdout().flush();
    if let Status::Program(code) = status
        && u8::try_from(code).is_err()
    {
        // A Windows status such as 0xC0000005 does not fit ExitCode::from(u8).
        let _ = std::io::stderr().flush();
        std::process::exit(code);
    }
    status.into()
}
