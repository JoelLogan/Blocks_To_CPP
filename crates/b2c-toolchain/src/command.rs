//! From build options to `argv` (spec §7.4).
//!
//! The command line is built only from closed enums (the project's
//! [`BuildConfiguration`] and [`Language`]), the probed [`Toolchain`],
//! validated defines and library profiles, validated machine-local extra
//! flags, and backend-chosen paths. Options the toolchain cannot provide are
//! dropped with an info note ([`CommandPlan::notes`]); they never fail the
//! build. Commands are `Vec<OsString>` passed to the OS directly, never
//! through a shell.

use std::ffi::OsString;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use b2c_ir::Diagnostic;
use b2c_ir::sast::CppStandard;
use b2c_model::{BuildConfiguration, Language, Optimization, Sanitizer, WarningLevel};
use b2c_process::{Command, Limits, ProcessError};

use crate::codes;
use crate::diagnostics::{self, MAX_INPUT_BYTES, ParsedOutput};
use crate::env::CompilerEnv;
use crate::flags::{ExtraFlags, LibraryProfile, Subsystem, ValidDefine};
use crate::probe::{DiagnosticsFormat, Toolchain};
use crate::target::Platform;

/// Default timeout for one compiler invocation (spec §7.5.2; configurable
/// 10–600 s).
pub const DEFAULT_COMPILE_TIMEOUT: Duration = Duration::from_mins(2);
/// Memory limit for one compiler invocation: 4 GiB (spec §7.5.2).
pub const COMPILER_MEMORY_LIMIT: u64 = 4 * 1024 * 1024 * 1024;
/// Process limit for one compiler invocation (spec §7.5.2).
pub const COMPILER_PROCESS_LIMIT: u32 = 32;
/// Cap for the compiler's captured output: 4 MiB (spec §7.5.2).
pub const COMPILER_OUTPUT_CAP: usize = 4 * 1024 * 1024;

/// Flags that keep the compiler's output stable and machine-readable.
const STABLE_OUTPUT: [&str; 3] = [
    "-fdiagnostics-color=never",
    "-fdiagnostics-urls=never",
    "-fmessage-length=0",
];

/// Extra warnings of the `strict` level (spec §7.4.2).
const STRICT_WARNINGS: [&str; 10] = [
    "-Wshadow",
    "-Wconversion",
    "-Wsign-conversion",
    "-Wold-style-cast",
    "-Wnon-virtual-dtor",
    "-Woverloaded-virtual",
    "-Wnull-dereference",
    "-Wdouble-promotion",
    "-Wformat=2",
    "-Wimplicit-fallthrough",
];

/// The limits for one compiler invocation: the timeout, 4 GiB of memory, 32
/// processes and 4 MiB of captured output per stream.
pub fn compiler_limits(timeout: Duration) -> Limits {
    Limits {
        timeout: Some(timeout),
        stdout_cap: COMPILER_OUTPUT_CAP,
        stderr_cap: COMPILER_OUTPUT_CAP,
        memory: Some(COMPILER_MEMORY_LIMIT),
        processes: Some(COMPILER_PROCESS_LIMIT),
        grace: None,
    }
}

/// How Windows programs are linked (a machine setting; Linux programs are
/// always linked dynamically).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkMode {
    /// `-static`, so the `.exe` runs anywhere (the default, spec §7.4.3).
    #[default]
    Static,
    /// Dynamic: the program needs the toolchain's DLLs to run.
    Dynamic,
}

/// Everything the command line is built from.
#[derive(Debug, Clone)]
pub struct BuildInputs<'a> {
    /// The probed toolchain.
    pub toolchain: &'a Toolchain,
    /// The project's configuration (debug or release).
    pub configuration: &'a BuildConfiguration,
    /// The project's language settings.
    pub language: Language,
    /// Validated project defines.
    pub defines: &'a [ValidDefine],
    /// Resolved library profiles.
    pub libraries: &'a [LibraryProfile],
    /// The build's `gen/` folder (`-iquote`).
    pub gen_dir: PathBuf,
    /// Windows link mode.
    pub link_mode: LinkMode,
    /// The program uses threads (`-pthread`).
    pub threads: bool,
    /// Validated machine-local extra flags.
    pub extra: &'a ExtraFlags,
    /// IDE trace builds only: the header passed with `-include`.
    pub trace_header: Option<PathBuf>,
}

impl<'a> BuildInputs<'a> {
    /// Inputs with no defines, libraries, threads or trace header, and the
    /// default link mode.
    pub fn new(
        toolchain: &'a Toolchain,
        configuration: &'a BuildConfiguration,
        language: Language,
        gen_dir: PathBuf,
        extra: &'a ExtraFlags,
    ) -> Self {
        Self {
            toolchain,
            configuration,
            language,
            defines: &[],
            libraries: &[],
            gen_dir,
            link_mode: LinkMode::default(),
            threads: false,
            extra,
            trace_header: None,
        }
    }
}

/// The resolved flags for one build, from which each step's command is made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPlan {
    program: PathBuf,
    format: DiagnosticsFormat,
    /// Every compile flag (before the source file).
    compile: Vec<OsString>,
    /// Flags needed both when compiling and when linking.
    both: Vec<OsString>,
    /// Link-only flags (before `-o`).
    link: Vec<OsString>,
    /// `-L`/`-l` flags (after the objects).
    libs: Vec<OsString>,
    notes: Vec<Diagnostic>,
}

/// One compiler invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerCommand {
    /// The compiler driver (canonical absolute path).
    pub program: PathBuf,
    /// The arguments.
    pub args: Vec<OsString>,
    /// How diagnostics come back for this step.
    pub format: DiagnosticsFormat,
    /// The SARIF file the compiler writes into its working directory, if
    /// any (a file name, not a path).
    pub sarif_file: Option<String>,
}

impl CommandPlan {
    /// Resolves the flags for `inputs` (spec §7.4.1–7.4.3).
    ///
    /// # Errors
    /// `B2C-T1010` if the toolchain does not support the project's C++
    /// standard, or `B2C-T1006` if it was not probed successfully.
    pub fn new(inputs: &BuildInputs<'_>) -> Result<Self, Diagnostic> {
        let toolchain = inputs.toolchain;
        let caps = &toolchain.capabilities;
        let shown = toolchain.path().display();
        let Some(format) = caps.diagnostics else {
            return Err(codes::error(
                codes::BROKEN_INSTALL,
                format!("The compiler {shown} has not been checked successfully, so it cannot be used."),
            ));
        };
        let standard = inputs.language.standard;
        let Some(spelling) = caps.standards.spelling(standard) else {
            return Err(codes::error(
                codes::STANDARD_UNSUPPORTED,
                format!(
                    "This project uses {}, which {shown} does not support. Choose an older C++ standard in the project settings or install a newer g++.",
                    standard_name(standard)
                ),
            ));
        };
        let platform = toolchain.platform();
        let major = toolchain.version.map_or(0, |version| version.major);
        let config = inputs.configuration;
        let mut plan = Self {
            program: toolchain.path().to_path_buf(),
            format,
            compile: Vec::new(),
            both: Vec::new(),
            link: Vec::new(),
            libs: Vec::new(),
            notes: Vec::new(),
        };

        // §7.4.1 base flags.
        let std = if inputs.language.gnu_extensions {
            spelling.replacen("c++", "gnu++", 1)
        } else {
            spelling.to_owned()
        };
        plan.compile_flag(format!("-std={std}"));
        plan.compile_flags(["-finput-charset=UTF-8", "-fexec-charset=UTF-8"]);
        plan.compile_flags(STABLE_OUTPUT);
        match format {
            // The file name is per source; added by each step.
            DiagnosticsFormat::AddOutputSarif => {}
            DiagnosticsFormat::SarifFile => plan.compile_flag("-fdiagnostics-format=sarif-file"),
            DiagnosticsFormat::Json => plan.compile_flag("-fdiagnostics-format=json"),
            DiagnosticsFormat::Plain => plan.compile_flag("-fdiagnostics-plain-output"),
        }
        if major >= 12 {
            plan.compile_flag("-Wbidi-chars=any");
        }

        plan.warning_flags(config);
        let debug_like = plan.optimisation_flags(config);
        let address = plan.sanitizers(platform, inputs);
        if config.hardening {
            plan.hardening(platform, major, debug_like, address, inputs);
        }
        if platform == Platform::Windows {
            match inputs.link_mode {
                LinkMode::Static if caps.static_link => plan.link.push("-static".into()),
                LinkMode::Static => plan.notes.push(codes::info(
                    codes::STATIC_DROPPED,
                    format!(
                        "Static linking does not work with {shown}, so the program is linked dynamically and needs the compiler's DLLs to run."
                    ),
                )),
                LinkMode::Dynamic => {}
            }
        }

        plan.compile_flag("-pipe");
        if inputs.threads {
            plan.both_flag("-pthread");
        }
        plan.compile_flag("-iquote");
        plan.compile.push(inputs.gen_dir.clone().into_os_string());
        for define in inputs.defines {
            plan.compile_flag(define.argument());
        }
        plan.library_flags(inputs);
        if platform == Platform::Windows
            && inputs
                .libraries
                .iter()
                .any(|library| library.subsystem() == Subsystem::Windows)
        {
            plan.link.push("-mwindows".into());
        }
        plan.compile.extend(inputs.extra.compile().iter().cloned());
        plan.link.extend(inputs.extra.link().iter().cloned());
        if let Some(header) = &inputs.trace_header {
            plan.compile_flag("-include");
            plan.compile.push(header.clone().into_os_string());
        }
        Ok(plan)
    }

    /// §7.4.2: the warning flags of the configuration's warning level.
    fn warning_flags(&mut self, config: &BuildConfiguration) {
        self.compile_flag("-Wall");
        if matches!(config.warnings, WarningLevel::Helpful | WarningLevel::Strict) {
            self.compile_flags(["-Wextra", "-Wpedantic"]);
        }
        if config.warnings == WarningLevel::Strict {
            self.compile_flags(STRICT_WARNINGS);
        }
        if config.warnings_as_errors {
            self.compile_flag("-Werror");
        }
    }

    /// §7.4.3: optimisation, debug information and assertion flags. Returns
    /// whether this is a debug-like (unoptimised) configuration.
    fn optimisation_flags(&mut self, config: &BuildConfiguration) -> bool {
        let debug_like = matches!(config.optimization, Optimization::None | Optimization::Debug);
        self.compile_flag(match config.optimization {
            Optimization::None => "-O0",
            Optimization::Debug => "-Og",
            Optimization::Speed => "-O2",
            Optimization::Size => "-Os",
        });
        if config.debug_info {
            self.compile_flag("-g");
        }
        if config.debug_info || !config.sanitizers.is_empty() {
            self.compile_flag("-fno-omit-frame-pointer");
        }
        if debug_like {
            self.compile_flag("-D_GLIBCXX_ASSERTIONS");
        } else {
            self.compile_flag("-DNDEBUG");
        }
        debug_like
    }

    /// §7.4.4: include folders, compile flags, library folders and link
    /// libraries of every resolved library profile.
    fn library_flags(&mut self, inputs: &BuildInputs<'_>) {
        for library in inputs.libraries {
            for dir in library.include_dirs() {
                self.compile_flag("-isystem");
                self.compile.push(dir.clone().into_os_string());
            }
            self.compile_flags(library.pkg_config().compile.iter().map(String::as_str));
            for dir in library.lib_dirs() {
                self.libs.push("-L".into());
                self.libs.push(dir.clone().into_os_string());
            }
            for name in library.link() {
                self.libs.push(format!("-l{}", name.as_str()).into());
            }
            self.libs
                .extend(library.pkg_config().link.iter().map(OsString::from));
        }
    }

    /// Info notes about options that were left out (`B2C-T1011`–`T1013`).
    /// Show them once per build.
    pub fn notes(&self) -> &[Diagnostic] {
        &self.notes
    }

    /// Every compile flag (no sources, objects or output paths), for the
    /// object cache key (spec §7.5.1).
    pub fn compile_flags_for_key(&self) -> &[OsString] {
        &self.compile
    }

    /// The diagnostics format compile steps use.
    pub fn diagnostics_format(&self) -> DiagnosticsFormat {
        self.format
    }

    /// `-c <source> -o <object>`.
    pub fn compile(&self, source: &Path, object: &Path) -> CompilerCommand {
        let (mut args, sarif_file) = self.compile_head(source);
        args.extend(["-c".into(), source.as_os_str().to_os_string()]);
        args.extend(["-o".into(), object.as_os_str().to_os_string()]);
        CompilerCommand {
            program: self.program.clone(),
            args,
            format: self.format,
            sarif_file,
        }
    }

    /// Links objects into an executable. Linker messages are plain text.
    pub fn link(&self, objects: &[PathBuf], output: &Path) -> CompilerCommand {
        let mut args: Vec<OsString> = STABLE_OUTPUT.iter().map(OsString::from).collect();
        args.push("-fdiagnostics-plain-output".into());
        args.extend(self.both.iter().cloned());
        args.extend(self.link.iter().cloned());
        args.extend(["-o".into(), output.as_os_str().to_os_string()]);
        args.extend(objects.iter().map(|object| object.as_os_str().to_os_string()));
        args.extend(self.libs.iter().cloned());
        CompilerCommand {
            program: self.program.clone(),
            args,
            format: DiagnosticsFormat::Plain,
            sarif_file: None,
        }
    }

    /// Compiles and links a single translation unit in one invocation.
    pub fn compile_and_link(&self, source: &Path, output: &Path) -> CompilerCommand {
        let (mut args, sarif_file) = self.compile_head(source);
        args.extend(self.link.iter().cloned());
        args.push(source.as_os_str().to_os_string());
        args.extend(["-o".into(), output.as_os_str().to_os_string()]);
        args.extend(self.libs.iter().cloned());
        CompilerCommand {
            program: self.program.clone(),
            args,
            format: self.format,
            sarif_file,
        }
    }

    /// The compile flags plus the per-source diagnostics flag.
    fn compile_head(&self, source: &Path) -> (Vec<OsString>, Option<String>) {
        let mut args = self.compile.clone();
        let sarif_file = match self.format {
            DiagnosticsFormat::AddOutputSarif => {
                let name = sarif_name(source);
                args.push(format!("-fdiagnostics-add-output=sarif:version=2.1,file={name}").into());
                Some(name)
            }
            // GCC 13–14 name the file after the source.
            DiagnosticsFormat::SarifFile => Some(sarif_name(source)),
            DiagnosticsFormat::Json | DiagnosticsFormat::Plain => None,
        };
        (args, sarif_file)
    }

    fn compile_flag(&mut self, flag: impl Into<OsString>) {
        self.compile.push(flag.into());
    }

    fn compile_flags<'f>(&mut self, flags: impl IntoIterator<Item = &'f str>) {
        self.compile.extend(flags.into_iter().map(OsString::from));
    }

    fn both_flag(&mut self, flag: &str) {
        self.compile.push(flag.into());
        self.both.push(flag.into());
    }

    /// Adds sanitizer flags (spec §7.4.3). Returns whether AddressSanitizer
    /// is on.
    fn sanitizers(&mut self, platform: Platform, inputs: &BuildInputs<'_>) -> bool {
        let wanted = &inputs.configuration.sanitizers;
        let sanitizers = &inputs.toolchain.capabilities.sanitizers;
        let want_address = wanted.contains(&Sanitizer::Address);
        let want_undefined = wanted.contains(&Sanitizer::Undefined);
        let mut address = false;
        let mut undefined_done = false;
        if want_address {
            if platform == Platform::Windows {
                self.notes.push(codes::info(
                    codes::SANITIZER_DROPPED,
                    "AddressSanitizer is not available for g++ on Windows, so debug builds do not check memory accesses.",
                ));
            } else if sanitizers.address_undefined {
                address = true;
                if want_undefined {
                    self.both_flag("-fsanitize=address,undefined");
                    self.both_flag("-fno-sanitize-recover=undefined");
                    undefined_done = true;
                } else {
                    self.both_flag("-fsanitize=address");
                }
            } else {
                self.notes.push(codes::info(
                    codes::SANITIZER_DROPPED,
                    "AddressSanitizer does not work with this compiler, so debug builds do not check memory accesses.",
                ));
            }
        }
        if want_undefined && !undefined_done {
            if platform == Platform::Linux && sanitizers.undefined {
                self.both_flag("-fsanitize=undefined");
                self.both_flag("-fno-sanitize-recover=undefined");
            } else if sanitizers.undefined_trap {
                self.both_flag("-fsanitize=undefined");
                self.both_flag("-fsanitize-undefined-trap-on-error");
                if platform == Platform::Linux {
                    self.notes.push(codes::info(
                        codes::SANITIZER_DROPPED,
                        "The undefined-behaviour checker's library is missing, so it stops the program without a detailed report.",
                    ));
                }
            } else {
                self.notes.push(codes::info(
                    codes::SANITIZER_DROPPED,
                    "The undefined-behaviour checker (UBSan) does not work with this compiler, so debug builds run without it.",
                ));
            }
        }
        address
    }

    /// Adds hardening flags (spec §7.4.3), each gated on the probe.
    fn hardening(
        &mut self,
        platform: Platform,
        major: u32,
        debug_like: bool,
        address: bool,
        inputs: &BuildInputs<'_>,
    ) {
        let hardening = &inputs.toolchain.capabilities.hardening;
        let mut dropped: Vec<&str> = Vec::new();
        match platform {
            Platform::Linux if hardening.fhardened => self.both_flag("-fhardened"),
            Platform::Linux => {
                // glibc only fortifies optimised code (and warns otherwise),
                // and fortified functions hide errors from AddressSanitizer.
                if !debug_like && !address {
                    if hardening.fortify_source {
                        self.compile_flag("-U_FORTIFY_SOURCE");
                        self.compile_flag(if major >= 12 {
                            "-D_FORTIFY_SOURCE=3"
                        } else {
                            "-D_FORTIFY_SOURCE=2"
                        });
                    } else {
                        dropped.push("-D_FORTIFY_SOURCE");
                    }
                    self.compile_flag("-D_GLIBCXX_ASSERTIONS");
                }
                for (supported, flag) in [
                    (hardening.stack_protector_strong, "-fstack-protector-strong"),
                    (hardening.stack_clash_protection, "-fstack-clash-protection"),
                    (hardening.cf_protection, "-fcf-protection"),
                ] {
                    if supported {
                        self.both_flag(flag);
                    } else {
                        dropped.push(flag);
                    }
                }
                if hardening.pie {
                    self.compile_flag("-fPIE");
                    self.link.push("-pie".into());
                } else {
                    dropped.push("-fPIE -pie");
                }
                for (supported, flag) in [
                    (hardening.relro_now, "-Wl,-z,relro,-z,now"),
                    (hardening.noexecstack, "-Wl,-z,noexecstack"),
                ] {
                    if supported {
                        self.link.push(flag.into());
                    } else {
                        dropped.push(flag);
                    }
                }
            }
            Platform::Windows => {
                if hardening.stack_protector_strong {
                    self.both_flag("-fstack-protector-strong");
                } else {
                    dropped.push("-fstack-protector-strong");
                }
                if hardening.windows_aslr_dep {
                    self.link
                        .push("-Wl,--dynamicbase,--nxcompat,--high-entropy-va".into());
                } else {
                    dropped.push("-Wl,--dynamicbase,--nxcompat,--high-entropy-va");
                }
            }
        }
        if !dropped.is_empty() {
            self.notes.push(codes::info(
                codes::HARDENING_DROPPED,
                format!(
                    "These hardening options are not supported by this compiler and were left out: {}.",
                    dropped.join(", ")
                ),
            ));
        }
    }
}

impl CompilerCommand {
    /// A `b2c-process` command running this step in `working_dir` (the
    /// build's `diag/` folder) with the allowlisted environment and
    /// [`compiler_limits`] (`timeout` defaults to
    /// [`DEFAULT_COMPILE_TIMEOUT`]).
    ///
    /// It also removes a SARIF file left in `working_dir` by an earlier run,
    /// so [`CompilerCommand::read_diagnostics`] never reads stale results.
    ///
    /// # Errors
    /// [`ProcessError`] if the program or working directory is not
    /// absolute.
    pub fn process_command(
        &self,
        working_dir: &Path,
        env: &CompilerEnv,
        timeout: Option<Duration>,
    ) -> Result<Command, ProcessError> {
        if let Some(name) = &self.sarif_file {
            let _ = std::fs::remove_file(working_dir.join(name));
        }
        let mut command = Command::new(&self.program, working_dir)?;
        command
            .args(self.args.iter().cloned())
            .envs(env.vars.iter().cloned())
            .limits(compiler_limits(timeout.unwrap_or(DEFAULT_COMPILE_TIMEOUT)));
        Ok(command)
    }

    /// Parses this step's diagnostics: the SARIF file in `working_dir` (read
    /// with a size bound, never through a symbolic link) and standard error.
    pub fn read_diagnostics(&self, working_dir: &Path, stderr: &[u8]) -> ParsedOutput {
        let sarif = self
            .sarif_file
            .as_ref()
            .and_then(|name| read_bounded(&working_dir.join(name)));
        let mut parsed = diagnostics::parse_output(self.format, stderr, sarif.as_deref());
        if sarif.as_ref().is_some_and(|bytes| bytes.len() > MAX_INPUT_BYTES) {
            parsed.truncated = true;
        }
        parsed
    }
}

/// Reads at most [`MAX_INPUT_BYTES`] + 1 bytes of a regular file (not a
/// symbolic link); a longer file is then refused by the parser.
fn read_bounded(path: &Path) -> Option<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(
        u64::try_from(MAX_INPUT_BYTES)
            .unwrap_or(u64::MAX)
            .saturating_add(1),
    )
    .read_to_end(&mut bytes)
    .ok()?;
    Some(bytes)
}

/// The SARIF file name for a source: `<file name>.sarif` (what GCC 13–14
/// write), or a fixed name when the file name is unusual.
fn sarif_name(source: &Path) -> String {
    match source.file_name().and_then(|name| name.to_str()) {
        Some(name)
            if !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) =>
        {
            format!("{name}.sarif")
        }
        _ => String::from("diagnostics.sarif"),
    }
}

fn standard_name(standard: CppStandard) -> &'static str {
    match standard {
        CppStandard::Cpp17 => "C++17",
        CppStandard::Cpp20 => "C++20",
        CppStandard::Cpp23 => "C++23",
        CppStandard::Cpp26 => "C++26",
    }
}
