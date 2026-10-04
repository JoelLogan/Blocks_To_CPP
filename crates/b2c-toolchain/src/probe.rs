//! Probing what a toolchain can do (spec §7.3).
//!
//! [`probe`] runs a set of tiny compiler invocations through `b2c-process`
//! (each with a 10 s timeout, in a private temporary folder, with the
//! sanitised environment of [`crate::env`]) and returns a [`Toolchain`]: a
//! serialisable record that callers cache by [`Fingerprint`] (spec: stored
//! in `toolchains.json`) and re-check with [`Toolchain::is_current`].
//!
//! Probes run in two rounds, each in parallel: first the quick questions
//! (`-dumpfullversion`, `-dumpmachine`, `--version`,
//! `-print-prog-name=cc1plus`); then, only for a GCC that is new enough,
//! everything else (hello world, standards, library features, diagnostics
//! format, sanitizers, hardening, static linking). With g++ 13 on four
//! cores the whole probe takes about a second.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use b2c_ir::Diagnostic;
use b2c_ir::sast::CppStandard;
use b2c_process::{CancelToken, Captured, Command, Limits, ProcessError, Stdin, run_captured};
use serde::{Deserialize, Serialize};

use crate::codes;
use crate::env::{HostEnv, compiler_env};
use crate::fingerprint::Fingerprint;
use crate::target::{GccVersion, Platform, Target, TargetOs};

/// Timeout for each probe invocation (spec §7.3).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// The oldest supported GCC major version (spec §7.1).
pub const MIN_GCC_MAJOR: u32 = 11;
/// Version of the [`Toolchain`] record; cached records with another version
/// must be probed again.
pub const PROBE_FORMAT: u32 = 1;

/// Most output kept from one probe invocation.
const PROBE_OUTPUT_CAP: usize = 256 * 1024;
/// What the probe programs print.
const MARKER: &str = "b2c-probe-ok";
const HELLO: &str = "#include <cstdio>\nint main() { std::puts(\"b2c-probe-ok\"); return 0; }\n";
const TRIVIAL: &str = "int main() { return 0; }\n";

/// How the compiler reports diagnostics (spec §7.5.3 ladder).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsFormat {
    /// GCC 15+: `-fdiagnostics-add-output=sarif:…`, text kept on stderr.
    AddOutputSarif,
    /// GCC 13–14: `-fdiagnostics-format=sarif-file`.
    SarifFile,
    /// GCC 10–14: `-fdiagnostics-format=json` on stderr.
    Json,
    /// `-fdiagnostics-plain-output` text.
    Plain,
}

/// Which compiler the program really is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompilerKind {
    /// GCC's `g++`.
    Gcc,
    /// Clang pretending to be `g++` (not supported yet).
    Clang,
    /// Not recognised (did not answer, or not a compiler).
    Unknown,
}

/// The `-std=` spelling that works for each C++ standard, or `None` when the
/// compiler does not support it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Standards {
    /// `c++17`.
    pub cpp17: Option<String>,
    /// `c++20`.
    pub cpp20: Option<String>,
    /// `c++23` (`c++2b` before GCC 11.something).
    pub cpp23: Option<String>,
    /// `c++26` (`c++2c` before GCC 14).
    pub cpp26: Option<String>,
}

impl Standards {
    /// The working spelling for a standard (without `-std=`), e.g. `c++2b`.
    pub fn spelling(&self, standard: CppStandard) -> Option<&str> {
        match standard {
            CppStandard::Cpp17 => self.cpp17.as_deref(),
            CppStandard::Cpp20 => self.cpp20.as_deref(),
            CppStandard::Cpp23 => self.cpp23.as_deref(),
            CppStandard::Cpp26 => self.cpp26.as_deref(),
        }
    }
}

/// Standard library features that compiled (spec §7.3, used for feature
/// gating in [06 §6.10]).
#[allow(clippy::struct_excessive_bools)] // independent capabilities
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryFeatures {
    /// `<format>` (`std::format`), C++20.
    pub format: bool,
    /// `<print>` (`std::print`), C++23.
    pub print: bool,
    /// `<stacktrace>`, C++23 (linking it may need `-lstdc++exp`).
    pub stacktrace: bool,
    /// `std::ranges` and views, C++20.
    pub ranges: bool,
    /// `std::jthread`, C++20.
    pub jthread: bool,
    /// `contains` on associative containers, C++20.
    pub contains: bool,
    /// Designated initialisers, C++20.
    pub designated_initializers: bool,
}

/// Sanitizers that linked and ran.
#[allow(clippy::struct_excessive_bools)] // independent capabilities
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sanitizers {
    /// `-fsanitize=address,undefined` (Linux).
    pub address_undefined: bool,
    /// `-fsanitize=undefined` with its runtime library.
    pub undefined: bool,
    /// `-fsanitize=undefined -fsanitize-undefined-trap-on-error` (no runtime
    /// library; the Windows choice).
    pub undefined_trap: bool,
    /// AddressSanitizer's leak detection works where programs run. It fails
    /// under `ptrace` restrictions (some containers, debuggers); programs
    /// then need `ASAN_OPTIONS=detect_leaks=0`.
    pub leak_detection: bool,
}

/// Hardening options that were accepted and linked (spec §7.4.3).
#[allow(clippy::struct_excessive_bools)] // independent capabilities
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hardening {
    /// `-fhardened` (GCC 14+, GNU/Linux).
    pub fhardened: bool,
    /// `-D_FORTIFY_SOURCE` (Linux, with optimisation).
    pub fortify_source: bool,
    /// `-fstack-protector-strong`.
    pub stack_protector_strong: bool,
    /// `-fstack-clash-protection`.
    pub stack_clash_protection: bool,
    /// `-fcf-protection`.
    pub cf_protection: bool,
    /// `-fPIE -pie`.
    pub pie: bool,
    /// `-Wl,-z,relro,-z,now`.
    pub relro_now: bool,
    /// `-Wl,-z,noexecstack`.
    pub noexecstack: bool,
    /// Windows: `-Wl,--dynamicbase,--nxcompat,--high-entropy-va`.
    pub windows_aslr_dep: bool,
}

/// Everything probed about a toolchain that works.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Where `cc1plus` is (`-print-prog-name=cc1plus`).
    pub cc1plus: Option<PathBuf>,
    /// A hello-world program compiled, linked and ran.
    pub hello_world: bool,
    /// Supported standards.
    pub standards: Standards,
    /// Library features.
    pub library: LibraryFeatures,
    /// The diagnostics format to use.
    pub diagnostics: Option<DiagnosticsFormat>,
    /// Sanitizers.
    pub sanitizers: Sanitizers,
    /// Hardening.
    pub hardening: Hardening,
    /// `-static` links (probed for Windows targets only, where it is the
    /// default; Linux programs are always linked dynamically).
    pub static_link: bool,
}

/// A probed toolchain: what the UI lists and builds use.
///
/// It is serialisable so callers can cache it (spec: `toolchains.json`),
/// keyed by [`Toolchain::fingerprint`]. Before each build, call
/// [`Toolchain::is_current`]; if the driver changed, probe again and report
/// [`crate::codes::toolchain_changed`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toolchain {
    /// [`PROBE_FORMAT`] when probed.
    pub format: u32,
    /// Identity of the driver binary (its canonical path is what runs).
    pub fingerprint: Fingerprint,
    /// GCC, Clang or unknown.
    pub kind: CompilerKind,
    /// `-dumpfullversion` (or `-dumpversion`).
    pub version: Option<GccVersion>,
    /// First line of `--version`, for display.
    pub version_text: String,
    /// `-dumpmachine`.
    pub target: Target,
    /// What works.
    pub capabilities: Capabilities,
    /// Problems found (errors make the toolchain unusable).
    pub problems: Vec<Diagnostic>,
}

impl Toolchain {
    /// The driver to run: canonical and absolute.
    pub fn path(&self) -> &Path {
        &self.fingerprint.path
    }

    /// The folder containing the driver (put first on the compiler's `PATH`).
    pub fn bin_dir(&self) -> &Path {
        self.fingerprint.path.parent().unwrap_or(&self.fingerprint.path)
    }

    /// The platform the programs it builds run on.
    pub fn platform(&self) -> Platform {
        self.target.platform()
    }

    /// Whether builds can use it: GCC, new enough, working, no error-level
    /// problem.
    pub fn is_usable(&self) -> bool {
        self.format == PROBE_FORMAT && self.kind == CompilerKind::Gcc && !b2c_ir::has_errors(&self.problems)
    }

    /// Whether the driver binary is unchanged since probing.
    pub fn is_current(&self) -> bool {
        self.format == PROBE_FORMAT && self.fingerprint.is_current()
    }
}

/// Settings for [`probe`].
#[derive(Debug, Clone)]
pub struct ProbeOptions {
    /// Timeout per invocation ([`PROBE_TIMEOUT`]).
    pub timeout: Duration,
    /// Where the private temporary folder is created (default: the system
    /// temporary folder; the folder itself is created with a random name and
    /// owner-only permissions).
    pub temp_root: Option<PathBuf>,
    /// This computer's environment for the compiler.
    pub host: HostEnv,
    /// How many invocations run at once (default: available cores, at most
    /// 4).
    pub jobs: usize,
    /// Stops probing early.
    pub cancel: Option<CancelToken>,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            timeout: PROBE_TIMEOUT,
            temp_root: None,
            host: HostEnv::from_process(&[]),
            jobs: std::thread::available_parallelism().map_or(2, |n| n.get().min(4)),
            cancel: None,
        }
    }
}

/// Why a toolchain could not be probed at all. (A compiler that runs but is
/// broken, old or not GCC is not an error: it is a [`Toolchain`] with
/// problems.)
#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    /// The file cannot be read (missing, not a file, unreadable).
    #[error("cannot read the compiler {}: {source}", path.display())]
    Unreadable {
        /// The path.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// The private temporary folder could not be created.
    #[error("cannot create a temporary folder for checking the compiler: {0}")]
    TempDir(io::Error),
    /// The probe was cancelled.
    #[error("checking the compiler was cancelled")]
    Cancelled,
}

/// Probes the compiler at `path` (an absolute path, normally from
/// [`crate::discovery`]).
///
/// ```no_run
/// use b2c_toolchain::probe::{ProbeOptions, probe};
///
/// let toolchain = probe("/usr/bin/g++".as_ref(), &ProbeOptions::default())?;
/// if toolchain.is_usable() {
///     println!("g++ {:?} for {}", toolchain.version, toolchain.target.triple);
/// } else {
///     for problem in &toolchain.problems {
///         println!("{}", problem.message);
///     }
/// }
/// # Ok::<(), b2c_toolchain::probe::ProbeError>(())
/// ```
///
/// # Errors
/// [`ProbeError`] if the file cannot be fingerprinted, no temporary folder
/// can be created, or the probe was cancelled.
pub fn probe(path: &Path, options: &ProbeOptions) -> Result<Toolchain, ProbeError> {
    let fingerprint = Fingerprint::compute(path).map_err(|source| ProbeError::Unreadable {
        path: path.to_path_buf(),
        source,
    })?;
    let mut builder = tempfile::Builder::new();
    builder.prefix("b2c-probe-");
    let temp = match &options.temp_root {
        Some(root) => builder.tempdir_in(root),
        None => builder.tempdir(),
    }
    .map_err(ProbeError::TempDir)?;
    let temp_path = std::fs::canonicalize(temp.path()).map_err(ProbeError::TempDir)?;
    let bin_dir = fingerprint
        .path
        .parent()
        .unwrap_or(&fingerprint.path)
        .to_path_buf();
    let env = compiler_env(Platform::host(), &bin_dir, &temp_path, &options.host);
    let prober = Prober {
        gxx: fingerprint.path.clone(),
        dir: temp_path,
        env: env.vars,
        timeout: options.timeout,
        cancel: options.cancel.as_ref(),
        exe_suffix: "",
    };
    let toolchain = prober.run(fingerprint, options.jobs.max(1));
    if options.cancel.as_ref().is_some_and(CancelToken::is_cancelled) {
        return Err(ProbeError::Cancelled);
    }
    Ok(toolchain)
}

/// One probing session.
struct Prober<'a> {
    gxx: PathBuf,
    dir: PathBuf,
    env: Vec<(OsString, OsString)>,
    timeout: Duration,
    cancel: Option<&'a CancelToken>,
    exe_suffix: &'static str,
}

/// The result of a sanitizer run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SanitizerRun {
    Works,
    WorksWithoutLeakDetection,
    Fails,
}

impl Prober<'_> {
    fn run(mut self, fingerprint: Fingerprint, jobs: usize) -> Toolchain {
        let mut toolchain = Toolchain {
            format: PROBE_FORMAT,
            fingerprint,
            kind: CompilerKind::Unknown,
            version: None,
            version_text: String::new(),
            target: Target::parse(""),
            capabilities: Capabilities::default(),
            problems: Vec::new(),
        };
        let shown = toolchain.fingerprint.path.display().to_string();

        // Round 1: the quick questions.
        let questions: [&[&str]; 5] = [
            &["-dumpfullversion"],
            &["-dumpversion"],
            &["-dumpmachine"],
            &["--version"],
            &["-print-prog-name=cc1plus"],
        ];
        let this = &self;
        let answers = parallel(
            jobs,
            questions
                .iter()
                .map(|args| Box::new(move || this.gxx(args)) as Job<'_, Result<Captured, String>>)
                .collect(),
        );
        let answer = |index: usize| -> Option<String> {
            match answers.get(index) {
                Some(Some(Ok(captured))) if captured.status.success() && !captured.timed_out => {
                    Some(String::from_utf8_lossy(&captured.stdout).trim().to_owned())
                }
                _ => None,
            }
        };
        let version_output = answer(0).or_else(|| answer(1));
        let Some(version_output) = version_output else {
            let reason = match answers.first() {
                Some(Some(Err(error))) => error.clone(),
                Some(Some(Ok(captured))) if captured.timed_out => {
                    format!("it did not answer within {} ms", self.timeout.as_millis())
                }
                _ => String::from("it did not report a version"),
            };
            toolchain.problems.push(codes::error(
                codes::NOT_RUNNABLE,
                format!("The compiler {shown} could not be used: {reason}."),
            ));
            return toolchain;
        };
        toolchain.version_text = answer(3)
            .and_then(|text| text.lines().next().map(str::to_owned))
            .unwrap_or_default();
        toolchain.target = Target::parse(&answer(2).unwrap_or_default());
        self.exe_suffix = toolchain.target.platform().exe_suffix();

        let Some(version) = identify(
            &mut toolchain,
            &shown,
            &version_output,
            answer(3).as_deref(),
            answer(4),
        ) else {
            return toolchain;
        };

        // Round 2: everything else.
        self.second_round(&mut toolchain, version, jobs);
        toolchain
    }

    fn second_round(&self, toolchain: &mut Toolchain, version: GccVersion, jobs: usize) {
        let platform = toolchain.target.platform();
        let hardening_groups = hardening_groups(platform, version);
        let all_hardening: Vec<&'static str> = hardening_groups.iter().flatten().copied().collect();

        let (checks, work): (Vec<Check>, Vec<Job<'_, Answer>>) = self
            .second_round_jobs(platform, version, all_hardening)
            .into_iter()
            .unzip();
        let results: Vec<(Check, Answer)> = checks
            .into_iter()
            .zip(parallel(jobs, work))
            .map(|(check, answer)| (check, answer.unwrap_or(Answer::Bool(false))))
            .collect();
        record_answers(toolchain, &results);
        let ok = |wanted: Check| results.contains(&(wanted, Answer::Bool(true)));

        let shown = toolchain.fingerprint.path.display().to_string();
        let caps = &mut toolchain.capabilities;
        // Hardening: all at once, or each group on its own when that failed.
        let group_ok: Vec<bool> = if ok(Check::HardeningAll) {
            vec![true; hardening_groups.len()]
        } else {
            let jobs_list: Vec<Job<'_, bool>> = hardening_groups
                .iter()
                .enumerate()
                .map(|(index, flags)| {
                    Box::new(move || self.links(&format!("hardening-{index}"), HELLO, flags)) as Job<'_, bool>
                })
                .collect();
            parallel(jobs, jobs_list)
                .into_iter()
                .map(|ok| ok.unwrap_or(false))
                .collect()
        };
        let group = |index: usize| group_ok.get(index).copied().unwrap_or(false);
        caps.hardening = match platform {
            Platform::Linux => Hardening {
                fhardened: ok(Check::Fhardened),
                fortify_source: group(0),
                stack_protector_strong: group(1),
                stack_clash_protection: group(2),
                cf_protection: group(3),
                pie: group(4),
                relro_now: group(5),
                noexecstack: group(6),
                windows_aslr_dep: false,
            },
            Platform::Windows => Hardening {
                stack_protector_strong: group(0),
                windows_aslr_dep: group(1),
                ..Hardening::default()
            },
        };

        if !caps.hello_world {
            toolchain.problems.push(codes::error(
                codes::BROKEN_INSTALL,
                format!(
                    "{shown} could not build and run a tiny test program, so the installation is probably broken or incomplete. Reinstall the compiler."
                ),
            ));
        } else if caps.standards.cpp20.is_none() {
            toolchain.problems.push(codes::error(
                codes::BROKEN_INSTALL,
                format!("{shown} cannot compile C++20, so the installation is probably broken. Reinstall the compiler."),
            ));
        }
    }

    /// The round-2 checks: standards, library features, diagnostics formats,
    /// sanitizers, static linking and hardening, each as a job.
    fn second_round_jobs(
        &self,
        platform: Platform,
        version: GccVersion,
        all_hardening: Vec<&'static str>,
    ) -> Vec<(Check, Job<'_, Answer>)> {
        let mut jobs_list: Vec<(Check, Job<'_, Answer>)> = Vec::new();
        jobs_list.push((
            Check::Hello,
            Box::new(|| Answer::Bool(self.links_and_runs("hello", HELLO, &[]))),
        ));
        for (check, spelling) in [
            (Check::Std("c++17"), "c++17"),
            (Check::Std("c++20"), "c++20"),
            (Check::Std("c++23"), "c++23"),
            (Check::Std("c++2b"), "c++2b"),
            (Check::Std("c++26"), "c++26"),
            (Check::Std("c++2c"), "c++2c"),
        ] {
            jobs_list.push((
                check,
                Box::new(move || {
                    let flag = format!("-std={spelling}");
                    Answer::Bool(self.compiles(&format!("std-{spelling}"), TRIVIAL, &[flag.as_str()]))
                }),
            ));
        }
        for (check, standard, source) in FEATURES {
            jobs_list.push((
                *check,
                Box::new(move || {
                    let name = format!("feature-{}", check.slug());
                    Answer::Bool(self.compiles(&name, source, &[standard]))
                }),
            ));
        }
        for (check, flag) in [
            (
                Check::AddOutputSarif,
                "-fdiagnostics-add-output=sarif:version=2.1,file=probe-add-output.sarif",
            ),
            (Check::SarifFile, "-fdiagnostics-format=sarif-file"),
            (Check::Json, "-fdiagnostics-format=json"),
        ] {
            jobs_list.push((
                check,
                Box::new(move || {
                    Answer::Bool(self.compiles(&format!("diag-{}", check.slug()), TRIVIAL, &[flag]))
                }),
            ));
        }
        if platform == Platform::Linux {
            jobs_list.push((
                Check::AddressUndefined,
                Box::new(|| {
                    Answer::Sanitizer(self.sanitizer_run(
                        "asan",
                        &["-fsanitize=address,undefined", "-fno-sanitize-recover=undefined"],
                        true,
                    ))
                }),
            ));
            jobs_list.push((
                Check::Undefined,
                Box::new(|| {
                    Answer::Sanitizer(self.sanitizer_run(
                        "ubsan",
                        &["-fsanitize=undefined", "-fno-sanitize-recover=undefined"],
                        false,
                    ))
                }),
            ));
            if version.major >= 14 {
                jobs_list.push((
                    Check::Fhardened,
                    Box::new(|| Answer::Bool(self.links("fhardened", HELLO, &["-fhardened"]))),
                ));
            }
        } else {
            jobs_list.push((
                Check::Static,
                Box::new(|| Answer::Bool(self.links_and_runs("static", HELLO, &["-static"]))),
            ));
        }
        jobs_list.push((
            Check::UndefinedTrap,
            Box::new(|| {
                Answer::Sanitizer(self.sanitizer_run(
                    "ubsan-trap",
                    &["-fsanitize=undefined", "-fsanitize-undefined-trap-on-error"],
                    false,
                ))
            }),
        ));
        jobs_list.push((
            Check::HardeningAll,
            Box::new(move || Answer::Bool(self.links("hardening", HELLO, &all_hardening))),
        ));

        jobs_list
    }

    /// A Blocks2Cpp process command for a program in the probe folder.
    fn command(&self, program: &Path) -> Result<Command, String> {
        let mut command = Command::new(program, &self.dir).map_err(|error| error.to_string())?;
        command
            .envs(self.env.iter().cloned())
            .stdin(Stdin::Null)
            .limits(Limits {
                timeout: Some(self.timeout),
                stdout_cap: PROBE_OUTPUT_CAP,
                stderr_cap: PROBE_OUTPUT_CAP,
                ..Limits::default()
            });
        if let Some(cancel) = self.cancel {
            command.cancel_token(cancel);
        }
        Ok(command)
    }

    /// Runs g++ with `args`.
    fn gxx(&self, args: &[&str]) -> Result<Captured, String> {
        let mut command = self.command(&self.gxx)?;
        command.args(args);
        run_captured(&command).map_err(|error| match error {
            ProcessError::Spawn { source, .. } => source.to_string(),
            other => other.to_string(),
        })
    }

    /// Writes `name.cpp` and returns its file name.
    fn source(&self, name: &str, text: &str) -> Option<String> {
        let file = format!("{name}.cpp");
        std::fs::write(self.dir.join(&file), text).ok()?;
        Some(file)
    }

    fn succeeded(result: &Result<Captured, String>) -> bool {
        matches!(result, Ok(captured) if captured.status.success() && !captured.timed_out)
    }

    /// Whether `text` passes `-fsyntax-only` with `flags`.
    fn compiles(&self, name: &str, text: &str, flags: &[&str]) -> bool {
        let Some(file) = self.source(name, text) else {
            return false;
        };
        let mut args: Vec<&str> = flags.to_vec();
        args.extend(["-fsyntax-only", file.as_str()]);
        Self::succeeded(&self.gxx(&args))
    }

    /// Compiles and links `text` with `flags`; returns the executable.
    fn link(&self, name: &str, text: &str, flags: &[&str]) -> Option<PathBuf> {
        let file = self.source(name, text)?;
        let output = format!("{name}{}", self.exe_suffix);
        let mut args: Vec<&str> = flags.to_vec();
        args.extend([file.as_str(), "-o", output.as_str()]);
        Self::succeeded(&self.gxx(&args)).then(|| self.dir.join(output))
    }

    fn links(&self, name: &str, text: &str, flags: &[&str]) -> bool {
        self.link(name, text, flags).is_some()
    }

    /// Runs a probe program; `extra_env` is added to the probe environment.
    fn execute(&self, program: &Path, extra_env: &[(&str, &str)]) -> Result<Captured, String> {
        let mut command = self.command(program)?;
        command.envs(extra_env.iter().map(|(name, value)| (*name, *value)));
        run_captured(&command).map_err(|error| error.to_string())
    }

    fn printed_marker(result: &Result<Captured, String>) -> bool {
        matches!(result, Ok(captured) if Self::succeeded(result) && String::from_utf8_lossy(&captured.stdout).contains(MARKER))
    }

    fn links_and_runs(&self, name: &str, text: &str, flags: &[&str]) -> bool {
        self.link(name, text, flags)
            .is_some_and(|program| Self::printed_marker(&self.execute(&program, &[])))
    }

    /// Links and runs a sanitizer build. For AddressSanitizer, a run that
    /// fails only because LeakSanitizer cannot work here is retried with
    /// `detect_leaks=0`.
    fn sanitizer_run(&self, name: &str, flags: &[&str], address: bool) -> SanitizerRun {
        let Some(program) = self.link(name, HELLO, flags) else {
            return SanitizerRun::Fails;
        };
        if !address {
            return if Self::printed_marker(&self.execute(&program, &[])) {
                SanitizerRun::Works
            } else {
                SanitizerRun::Fails
            };
        }
        let with_leaks = self.execute(&program, &[("ASAN_OPTIONS", "detect_leaks=1")]);
        if Self::printed_marker(&with_leaks) {
            return SanitizerRun::Works;
        }
        let leak_trouble = matches!(&with_leaks, Ok(captured) if String::from_utf8_lossy(&captured.stderr).contains("LeakSanitizer"));
        if leak_trouble
            && Self::printed_marker(&self.execute(&program, &[("ASAN_OPTIONS", "detect_leaks=0")]))
        {
            SanitizerRun::WorksWithoutLeakDetection
        } else {
            SanitizerRun::Fails
        }
    }
}

/// The individual probes of round 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Check {
    Hello,
    Std(&'static str),
    Format,
    Print,
    Stacktrace,
    Ranges,
    Jthread,
    Contains,
    Designated,
    AddOutputSarif,
    SarifFile,
    Json,
    AddressUndefined,
    Undefined,
    UndefinedTrap,
    Fhardened,
    HardeningAll,
    Static,
}

impl Check {
    fn slug(self) -> &'static str {
        match self {
            Self::Hello => "hello",
            Self::Std(spelling) => spelling,
            Self::Format => "format",
            Self::Print => "print",
            Self::Stacktrace => "stacktrace",
            Self::Ranges => "ranges",
            Self::Jthread => "jthread",
            Self::Contains => "contains",
            Self::Designated => "designated",
            Self::AddOutputSarif => "add-output",
            Self::SarifFile => "sarif-file",
            Self::Json => "json",
            Self::AddressUndefined => "asan",
            Self::Undefined => "ubsan",
            Self::UndefinedTrap => "ubsan-trap",
            Self::Fhardened => "fhardened",
            Self::HardeningAll => "hardening",
            Self::Static => "static",
        }
    }
}

/// Library feature snippets: check, standard flag, source.
const FEATURES: &[(Check, &str, &str)] = &[
    (
        Check::Format,
        "-std=c++20",
        "#include <format>\nint main() { return std::format(\"{}\", 1).size() == 1 ? 0 : 1; }\n",
    ),
    (
        Check::Print,
        "-std=c++2b",
        "#include <print>\nint main() { std::print(\"{}\", 1); }\n",
    ),
    (
        Check::Stacktrace,
        "-std=c++2b",
        "#include <stacktrace>\nint main() { return std::stacktrace::current().size() > 100000 ? 1 : 0; }\n",
    ),
    (
        Check::Ranges,
        "-std=c++20",
        "#include <ranges>\n#include <vector>\nint main() {\n  std::vector<int> v{1, 2, 3};\n  auto big = v | std::views::filter([](int x) { return x > 1; });\n  return std::ranges::distance(big) == 2 ? 0 : 1;\n}\n",
    ),
    (
        Check::Jthread,
        "-std=c++20",
        "#include <thread>\nint main() { std::jthread t([] {}); }\n",
    ),
    (
        Check::Contains,
        "-std=c++20",
        "#include <map>\n#include <set>\nint main() { std::map<int, int> m; std::set<int> s; return m.contains(1) || s.contains(2) ? 1 : 0; }\n",
    ),
    (
        Check::Designated,
        "-std=c++20",
        "struct Point { int x; int y; };\nint main() { Point p{.x = 1, .y = 2}; return p.x + p.y - 3; }\n",
    ),
];

/// Round 1 checks on the answers: Clang, the GCC version and minimum, the
/// target and an installed `cc1plus`. Records problems in `toolchain` and
/// returns the version when probing can continue.
fn identify(
    toolchain: &mut Toolchain,
    shown: &str,
    version_output: &str,
    version_text: Option<&str>,
    cc1plus: Option<String>,
) -> Option<GccVersion> {
    if version_text.is_some_and(|text| text.to_ascii_lowercase().contains("clang")) {
        toolchain.kind = CompilerKind::Clang;
        toolchain.problems.push(codes::error(
            codes::CLANG,
            format!("{shown} is Clang, which Blocks2Cpp does not support yet. Choose a GCC g++ instead."),
        ));
        return None;
    }
    let Some(version) = GccVersion::parse(version_output) else {
        toolchain.problems.push(codes::error(
            codes::UNKNOWN_COMPILER,
            format!("{shown} does not look like the GCC C++ compiler (g++)."),
        ));
        return None;
    };
    toolchain.kind = CompilerKind::Gcc;
    toolchain.version = Some(version);
    if version.major < MIN_GCC_MAJOR {
        toolchain.problems.push(codes::error(
                codes::TOO_OLD,
                format!(
                    "{shown} is GCC {version}, which is too old: Blocks2Cpp needs GCC {MIN_GCC_MAJOR} or newer (13 or newer is recommended). Please install a newer g++."
                ),
            ));
        return None;
    }
    match toolchain.target.os {
            TargetOs::Cygwin => toolchain.problems.push(codes::warning(
                codes::CYGWIN,
                format!(
                    "{shown} is a Cygwin/MSYS compiler: the programs it builds only run where cygwin1.dll is available. MSYS2's UCRT64 g++ builds normal Windows programs."
                ),
            )),
            TargetOs::LegacyMinGw => toolchain.problems.push(codes::warning(
                codes::LEGACY_MINGW,
                format!(
                    "{shown} is the old MinGW from mingw.org, which is outdated. Install MSYS2 (UCRT64) or WinLibs for a current g++."
                ),
            )),
            TargetOs::Linux | TargetOs::MinGw | TargetOs::Other => {}
        }
    toolchain.capabilities.cc1plus = cc1plus
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file());
    if toolchain.capabilities.cc1plus.is_none() {
        toolchain.problems.push(codes::error(
                codes::BROKEN_INSTALL,
                format!(
                    "The installation of {shown} is incomplete: its C++ compiler program (cc1plus) is missing. Reinstall the compiler (for example the g++ package)."
                ),
            ));
        return None;
    }

    Some(version)
}

/// The hardening flag groups to probe, one group per capability, in the
/// order [`Hardening`] is filled from (spec §7.4.3).
fn hardening_groups(platform: Platform, version: GccVersion) -> Vec<Vec<&'static str>> {
    let fortify = if version.major >= 12 {
        "-D_FORTIFY_SOURCE=3"
    } else {
        "-D_FORTIFY_SOURCE=2"
    };
    match platform {
        Platform::Linux => vec![
            vec!["-O2", "-U_FORTIFY_SOURCE", fortify],
            vec!["-fstack-protector-strong"],
            vec!["-fstack-clash-protection"],
            vec!["-fcf-protection"],
            vec!["-fPIE", "-pie"],
            vec!["-Wl,-z,relro,-z,now"],
            vec!["-Wl,-z,noexecstack"],
        ],
        Platform::Windows => vec![
            vec!["-fstack-protector-strong"],
            vec!["-Wl,--dynamicbase,--nxcompat,--high-entropy-va"],
        ],
    }
}

/// Records the round-2 answers (all but hardening) in the toolchain.
fn record_answers(toolchain: &mut Toolchain, results: &[(Check, Answer)]) {
    let get = |wanted: Check| {
        results
            .iter()
            .find(|(check, _)| *check == wanted)
            .map(|(_, answer)| *answer)
    };
    let ok = |wanted: Check| matches!(get(wanted), Some(Answer::Bool(true)));
    let sanitizer = |wanted: Check| match get(wanted) {
        Some(Answer::Sanitizer(run)) => run,
        _ => SanitizerRun::Fails,
    };

    let caps = &mut toolchain.capabilities;
    caps.hello_world = ok(Check::Hello);
    let spelling = |first: &'static str, second: &'static str| {
        if ok(Check::Std(first)) {
            Some(first.to_owned())
        } else if ok(Check::Std(second)) {
            Some(second.to_owned())
        } else {
            None
        }
    };
    caps.standards = Standards {
        cpp17: ok(Check::Std("c++17")).then(|| String::from("c++17")),
        cpp20: ok(Check::Std("c++20")).then(|| String::from("c++20")),
        cpp23: spelling("c++23", "c++2b"),
        cpp26: spelling("c++26", "c++2c"),
    };
    caps.library = LibraryFeatures {
        format: ok(Check::Format),
        print: ok(Check::Print),
        stacktrace: ok(Check::Stacktrace),
        ranges: ok(Check::Ranges),
        jthread: ok(Check::Jthread),
        contains: ok(Check::Contains),
        designated_initializers: ok(Check::Designated),
    };
    caps.diagnostics = Some(if ok(Check::AddOutputSarif) {
        DiagnosticsFormat::AddOutputSarif
    } else if ok(Check::SarifFile) {
        DiagnosticsFormat::SarifFile
    } else if ok(Check::Json) {
        DiagnosticsFormat::Json
    } else {
        DiagnosticsFormat::Plain
    });
    let address = sanitizer(Check::AddressUndefined);
    caps.sanitizers = Sanitizers {
        address_undefined: address != SanitizerRun::Fails,
        undefined: sanitizer(Check::Undefined) != SanitizerRun::Fails,
        undefined_trap: sanitizer(Check::UndefinedTrap) != SanitizerRun::Fails,
        leak_detection: address == SanitizerRun::Works,
    };
    if address == SanitizerRun::WorksWithoutLeakDetection {
        toolchain.problems.push(codes::info(
                codes::LEAKS_UNAVAILABLE,
                "AddressSanitizer works here, but its leak detection does not (this happens in some containers and under debuggers), so debug builds run without leak checks.",
            ));
    }
    caps.static_link = ok(Check::Static);
}

/// A round-2 answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Bool(bool),
    Sanitizer(SanitizerRun),
}

/// A unit of work for [`parallel`].
type Job<'a, T> = Box<dyn FnOnce() -> T + Send + 'a>;

/// Runs jobs on up to `workers` threads and returns their results in order
/// (`None` only if a job could not run at all). If no thread can be started,
/// the jobs run on the calling thread.
fn parallel<'a, T: Send>(workers: usize, jobs: Vec<Job<'a, T>>) -> Vec<Option<T>> {
    let count = jobs.len();
    let queue: Mutex<VecDeque<(usize, Job<'a, T>)>> = Mutex::new(jobs.into_iter().enumerate().collect());
    let results: Mutex<Vec<Option<T>>> = Mutex::new((0..count).map(|_| None).collect());
    let work = || {
        loop {
            let next = queue.lock().unwrap_or_else(PoisonError::into_inner).pop_front();
            let Some((index, job)) = next else {
                break;
            };
            let value = job();
            if let Some(slot) = results
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get_mut(index)
            {
                *slot = Some(value);
            }
        }
    };
    std::thread::scope(|scope| {
        let mut started = 0;
        for _ in 0..workers.clamp(1, count.max(1)) {
            if std::thread::Builder::new()
                .name(String::from("b2c-probe"))
                .spawn_scoped(scope, work)
                .is_ok()
            {
                started += 1;
            }
        }
        if started == 0 {
            work();
        }
    });
    results.into_inner().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_keeps_order_and_runs_everything() {
        let jobs: Vec<Job<'_, usize>> = (0_usize..20)
            .map(|i| {
                Box::new(move || {
                    std::thread::sleep(Duration::from_millis((20 - i) as u64));
                    i * 2
                }) as Job<'_, usize>
            })
            .collect();
        let results = parallel(4, jobs);
        assert_eq!(results, (0..20).map(|i| Some(i * 2)).collect::<Vec<_>>());
        assert!(parallel::<u8>(4, Vec::new()).is_empty());
    }

    #[test]
    fn standards_spelling() {
        let standards = Standards {
            cpp17: Some("c++17".into()),
            cpp20: Some("c++20".into()),
            cpp23: Some("c++2b".into()),
            cpp26: None,
        };
        assert_eq!(standards.spelling(CppStandard::Cpp23), Some("c++2b"));
        assert_eq!(standards.spelling(CppStandard::Cpp26), None);
    }

    #[test]
    fn missing_compiler_is_an_error() {
        let error = probe(Path::new("/nonexistent/g++"), &ProbeOptions::default()).unwrap_err();
        assert!(matches!(error, ProbeError::Unreadable { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_is_not_gcc_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("g++");
        std::fs::write(&fake, "#!/bin/sh\necho 'not a compiler'\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let toolchain = probe(&fake, &ProbeOptions::default()).unwrap();
        assert_eq!(toolchain.kind, CompilerKind::Unknown);
        assert!(!toolchain.is_usable());
        assert_eq!(toolchain.problems[0].code.0, codes::UNKNOWN_COMPILER);
    }

    #[cfg(unix)]
    #[test]
    fn clang_masquerading_as_gcc_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("g++");
        std::fs::write(
            &fake,
            "#!/bin/sh\ncase \"$1\" in\n  -dumpfullversion|-dumpversion) echo 18.1.3 ;;\n  -dumpmachine) echo x86_64-pc-linux-gnu ;;\n  --version) echo 'Ubuntu clang version 18.1.3' ;;\nesac\n",
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let toolchain = probe(&fake, &ProbeOptions::default()).unwrap();
        assert_eq!(toolchain.kind, CompilerKind::Clang);
        assert_eq!(toolchain.problems[0].code.0, codes::CLANG);
        assert!(!toolchain.is_usable());
    }

    #[cfg(unix)]
    #[test]
    fn old_gcc_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("g++");
        std::fs::write(
            &fake,
            "#!/bin/sh\ncase \"$1\" in\n  -dumpfullversion) echo 9.4.0 ;;\n  -dumpmachine) echo x86_64-linux-gnu ;;\n  --version) echo 'g++ (GCC) 9.4.0' ;;\nesac\n",
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let toolchain = probe(&fake, &ProbeOptions::default()).unwrap();
        assert_eq!(toolchain.kind, CompilerKind::Gcc);
        assert_eq!(toolchain.version.map(|v| v.major), Some(9));
        assert_eq!(toolchain.problems[0].code.0, codes::TOO_OLD);
        assert!(toolchain.problems[0].message.contains("GCC 9.4.0"));
    }

    #[cfg(unix)]
    #[test]
    fn a_hanging_compiler_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("g++");
        std::fs::write(&fake, "#!/bin/sh\nexec sleep 30\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let options = ProbeOptions {
            timeout: Duration::from_millis(300),
            ..ProbeOptions::default()
        };
        let start = std::time::Instant::now();
        let toolchain = probe(&fake, &options).unwrap();
        assert!(start.elapsed() < Duration::from_secs(10));
        assert_eq!(toolchain.problems[0].code.0, codes::NOT_RUNNABLE);
        assert!(
            toolchain.problems[0].message.contains("did not answer"),
            "{}",
            toolchain.problems[0].message
        );
    }
}
