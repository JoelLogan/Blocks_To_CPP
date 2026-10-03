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
}

impl From<Status> for ExitCode {
    fn from(status: Status) -> Self {
        Self::from(match status {
            Status::Success => 0,
            Status::ProjectErrors => 1,
            Status::Usage => 2,
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
        Command::Migrate { project, in_place } => commands::migrate(&project, in_place),
        Command::Fmt { project, check } => commands::fmt(&project, check),
    };
    let _ = std::io::stdout().flush();
    status.into()
}
