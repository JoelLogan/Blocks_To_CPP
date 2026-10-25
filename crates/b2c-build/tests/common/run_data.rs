//! Run data as the golden tests use it (spec §9.2), for tests that build a
//! program and check what it does: a folder with `stdin.txt`, the expected
//! standard output (`stdout.txt`, or a regular expression in
//! `stdout.regex`), `exit_code.txt` and any input files the program reads.
//! The program runs in a copy of that folder, which stands in for the
//! project folder a program runs in by default (07 §7.6.2), so it names its
//! input files with paths relative to it. The copy must be the same after
//! the run: a program that adds, removes or changes a file there fails, so
//! no run leaves anything in the source tree for a later run to find.
//!
//! `tests/golden.rs` and `tests/assignments.rs` both build and check
//! programs with these helpers, so the two run checks cannot drift apart.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use b2c_build::{
    BuildOutcome, BuildRequest, CapturedRun, Configuration, FrontendOptions, ProgramExit, ProgramInput,
    RunRequest, ToolchainChoice, run_program_captured,
};
use b2c_ir::Severity;
use b2c_ir::sast::CppStandard;
use b2c_model::{Configurations, Language};
use b2c_toolchain::command::{BuildInputs, CommandPlan};
use b2c_toolchain::env::{HostEnv, compiler_env};
use b2c_toolchain::flags::ExtraFlags;

use super::real_toolchain;

/// How long one program may run.
pub(crate) const RUN_TIMEOUT: Duration = Duration::from_secs(30);

/// What a program must do with the input in its folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunData {
    /// The folder; the program runs in a copy of it.
    pub(crate) folder: PathBuf,
    /// Its standard input.
    pub(crate) stdin: Vec<u8>,
    /// Its exit code.
    pub(crate) exit_code: i32,
    /// Its standard output, exactly or as a pattern.
    pub(crate) stdout: ExpectedOutput,
}

/// The expected standard output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExpectedOutput {
    /// `stdout.txt`: exactly this text.
    Exact(String),
    /// `stdout.regex`: the whole output matches this pattern. The file's
    /// own last line breaks are not part of it, so a pattern ends with `\n`
    /// when the output ends with a line break.
    Pattern(String),
}

impl RunData {
    /// Reads the run data in `folder`, describing what is missing or wrong.
    pub(crate) fn read(folder: &Path) -> Result<Self, String> {
        let read = |name: &str| {
            std::fs::read(folder.join(name)).map_err(|error| format!("cannot read {name}: {error}"))
        };
        let stdin = read("stdin.txt")?;
        let exit_code = String::from_utf8_lossy(&read("exit_code.txt")?)
            .trim()
            .parse()
            .map_err(|error| format!("exit_code.txt is not a whole number: {error}"))?;
        let text = folder.join("stdout.txt").is_file();
        let pattern = folder.join("stdout.regex").is_file();
        let stdout = match (text, pattern) {
            (true, false) => {
                ExpectedOutput::Exact(String::from_utf8_lossy(&read("stdout.txt")?).into_owned())
            }
            (false, true) => {
                let pattern = String::from_utf8_lossy(&read("stdout.regex")?)
                    .trim_end_matches('\n')
                    .to_owned();
                regex::Regex::new(&pattern).map_err(|error| format!("stdout.regex is not valid: {error}"))?;
                ExpectedOutput::Pattern(pattern)
            }
            (true, true) => return Err("has both stdout.txt and stdout.regex; keep one".to_owned()),
            (false, false) => return Err("has neither stdout.txt nor stdout.regex".to_owned()),
        };
        Ok(Self {
            folder: folder.to_path_buf(),
            stdin,
            exit_code,
            stdout,
        })
    }

    /// Runs `executable` in a copy of the folder with `stdin.txt` as its
    /// input, checks its exit code and standard output, and then checks that
    /// the copy is unchanged.
    pub(crate) fn check(&self, executable: &Path) -> Result<(), String> {
        let before = snapshot(&self.folder)?;
        let copy = tempfile::Builder::new()
            .prefix("b2c-run-")
            .tempdir()
            .map_err(|error| format!("cannot make a folder to run in: {error}"))?;
        write_snapshot(&before, copy.path())?;
        let run = run_program_captured(&RunRequest {
            executable: executable.to_path_buf(),
            args: Vec::new(),
            input: ProgramInput::Bytes(self.stdin.clone()),
            timeout: Some(RUN_TIMEOUT),
            working_directory: copy.path().to_path_buf(),
        })
        .map_err(|error| error.to_string())?;
        self.compare(&run)?;
        let changed = changes(&before, &snapshot(copy.path())?);
        if changed.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "changed the folder it ran in, a copy of its run data ({}); run data has no \
                 way yet to say which files a program writes",
                changed.join(", ")
            ))
        }
    }

    /// Compares a finished run with the expected exit code and output.
    fn compare(&self, run: &CapturedRun) -> Result<(), String> {
        let stdout = output_text(&run.stdout);
        if run.exit != ProgramExit::Code(self.exit_code) {
            return Err(format!(
                "ended with {:?}, expected exit code {}; stderr:\n{}",
                run.exit,
                self.exit_code,
                output_text(&run.stderr)
            ));
        }
        match &self.stdout {
            ExpectedOutput::Exact(expected) if stdout == *expected => Ok(()),
            ExpectedOutput::Exact(expected) => Err(format!(
                "output differs.\n--- expected\n{expected}--- got\n{stdout}"
            )),
            ExpectedOutput::Pattern(pattern) => {
                let regex =
                    regex::Regex::new(&format!("^(?s:{pattern})$")).map_err(|error| error.to_string())?;
                if regex.is_match(&stdout) {
                    Ok(())
                } else {
                    Err(format!("output does not match stdout.regex:\n{stdout}"))
                }
            }
        }
    }
}

/// What a folder holds: every file and folder under it, by its path
/// relative to it, with a file's bytes (`None` for a folder).
type Snapshot = BTreeMap<PathBuf, Option<Vec<u8>>>;

/// Reads everything under `folder`. A link or anything else that is neither
/// a file nor a folder is an error: run data holds neither.
fn snapshot(folder: &Path) -> Result<Snapshot, String> {
    let mut entries = Snapshot::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let dir = folder.join(&relative);
        let listing =
            std::fs::read_dir(&dir).map_err(|error| format!("cannot list {}: {error}", dir.display()))?;
        for entry in listing {
            let entry = entry.map_err(|error| format!("cannot list {}: {error}", dir.display()))?;
            let path = relative.join(entry.file_name());
            let kind = entry
                .file_type()
                .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
            if kind.is_dir() {
                entries.insert(path.clone(), None);
                pending.push(path);
            } else if kind.is_file() {
                let bytes = std::fs::read(entry.path())
                    .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
                entries.insert(path, Some(bytes));
            } else {
                return Err(format!("{} is neither a file nor a folder", path.display()));
            }
        }
    }
    Ok(entries)
}

/// Writes a snapshot into the empty folder `into`.
fn write_snapshot(snapshot: &Snapshot, into: &Path) -> Result<(), String> {
    // A folder's path sorts before the paths in it.
    for (path, contents) in snapshot {
        let target = into.join(path);
        let written = match contents {
            None => std::fs::create_dir(&target),
            Some(bytes) => std::fs::write(&target, bytes),
        };
        written.map_err(|error| format!("cannot copy {}: {error}", path.display()))?;
    }
    Ok(())
}

/// What differs between two snapshots, one item per path.
fn changes(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    let mut changed = Vec::new();
    for (path, contents) in after {
        match before.get(path) {
            None => changed.push(format!("added {}", path.display())),
            Some(old) if old != contents => changed.push(format!("changed {}", path.display())),
            Some(_) => {}
        }
    }
    for path in before.keys().filter(|path| !after.contains_key(*path)) {
        changed.push(format!("removed {}", path.display()));
    }
    changed
}

/// Standard output as text; Windows text mode turns each `\n` into `\r\n`,
/// which this undoes exactly.
pub(crate) fn output_text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes).into_owned();
    if cfg!(windows) {
        text.replace("\r\n", "\n")
    } else {
        text
    }
}

/// Builds a project as the golden tests do: the debug configuration (with
/// run-time checks such as sanitizers where the compiler has them), the
/// first usable g++, and no diagnostic other than notes from the front end,
/// the toolchain or g++. Returns the executable.
pub(crate) fn build_project(project: &Path, cache: &Path) -> Result<PathBuf, String> {
    let bytes =
        std::fs::read(project).map_err(|error| format!("cannot read {}: {error}", project.display()))?;
    let request = BuildRequest {
        configuration: Configuration::Debug,
        toolchain: ToolchainChoice::Auto,
        cache_root: cache.to_path_buf(),
        frontend: FrontendOptions::default(),
        ide: false,
    };
    let report = b2c_build::build(&bytes, &request).map_err(|error| error.to_string())?;
    let reported: Vec<String> = report
        .diagnostics
        .iter()
        .filter(|d| d.severity != Severity::Info)
        .map(|d| format!("{} {:?}: {}", d.code.0, d.primary, d.message))
        .collect();
    match report.outcome {
        BuildOutcome::Built { executable } if reported.is_empty() => Ok(executable),
        BuildOutcome::Built { .. } => Err(format!("build warnings: {reported:#?}")),
        _ => Err(format!("not built: {reported:#?}")),
    }
}

/// Builds a hand-written C++ file with the g++ on `PATH`, with the flags the
/// debug configuration gives generated code (C++20, helpful warnings,
/// run-time checks where the compiler has them, hardening) and warnings as
/// errors. Returns the executable, in `out_dir`.
///
/// # Panics
/// Without g++ on `PATH`.
pub(crate) fn build_cpp(source: &Path, out_dir: &Path) -> Result<PathBuf, String> {
    let toolchain = real_toolchain().expect("g++ on PATH");
    let mut configuration = Configurations::default().debug;
    configuration.warnings_as_errors = true;
    let language = Language {
        standard: CppStandard::Cpp20,
        gnu_extensions: false,
    };
    let extra = ExtraFlags::none();
    let inputs = BuildInputs::new(
        &toolchain,
        &configuration,
        language,
        out_dir.to_path_buf(),
        &extra,
    );
    let plan = CommandPlan::new(&inputs).map_err(|problem| problem.message)?;
    let executable = out_dir.join(if cfg!(windows) { "program.exe" } else { "program" });
    let step = plan.compile_and_link(source, &executable);
    let step = step.with_plain_diagnostics().unwrap_or(step);
    let env = compiler_env(
        toolchain.platform(),
        toolchain.bin_dir(),
        out_dir,
        &HostEnv::from_process(&[]),
    );
    let command = step
        .process_command(out_dir, &env, None)
        .map_err(|error| error.to_string())?;
    let captured = b2c_process::run_captured(&command).map_err(|error| error.to_string())?;
    if captured.status == b2c_process::ExitStatus::Exited(0) {
        Ok(executable)
    } else {
        Err(format!(
            "g++ failed ({:?}):\n{}",
            captured.status,
            String::from_utf8_lossy(&captured.stderr)
        ))
    }
}
