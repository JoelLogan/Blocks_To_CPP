//! Building a project: the front end, then g++ (`docs/spec/07-toolchain-build-run.md` §7.5).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use b2c_ir::source_map::{GeneratedProject, Position, SourceMap};
use b2c_ir::{DiagSource, Diagnostic, Location, Severity};
use b2c_model::{BuildConfiguration, Document};
use b2c_toolchain::command::{BuildInputs, CommandPlan, CompilerCommand};
use b2c_toolchain::diagnostics::{CompilerMessage, MessageOrigin, MessageSeverity, ParsedOutput};
use b2c_toolchain::env::{CompilerEnv, HostEnv, compiler_env};
use b2c_toolchain::flags::{ExtraFlags, ValidDefine};
use b2c_toolchain::probe::Toolchain;
use sha2::{Digest as _, Sha256};

use crate::build_dir::{BuildDir, BuildDirError, write_if_changed};
use crate::frontend::{FrontendOptions, run_frontend};
use crate::toolchains::{Selected, ToolchainChoice, select};

/// Which of the project's build configurations to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Configuration {
    /// Fast to build, with run-time checks.
    Debug,
    /// Optimised.
    Release,
}

impl Configuration {
    fn name(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }

    fn settings(self, document: &Document) -> &BuildConfiguration {
        let configurations = &document.project.build.configurations;
        match self {
            Self::Debug => &configurations.debug,
            Self::Release => &configurations.release,
        }
    }
}

/// What to build and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildRequest {
    /// The configuration.
    pub configuration: Configuration,
    /// The compiler.
    pub toolchain: ToolchainChoice,
    /// The build cache folder (see [`crate::default_cache_root`]).
    pub cache_root: PathBuf,
    /// Options for generating C++.
    pub frontend: FrontendOptions,
}

/// How a build ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildOutcome {
    /// The program was built (or was already up to date).
    Built {
        /// The executable.
        executable: PathBuf,
    },
    /// The project has errors (in the diagnostics), including compiler
    /// errors.
    ProjectErrors,
    /// No usable compiler (the diagnostics say why).
    ToolchainProblem,
}

/// Everything a build produced.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildReport {
    /// The project, when it loaded (for naming blocks in messages).
    pub document: Option<Document>,
    /// Every diagnostic, from the front end, the toolchain and g++.
    pub diagnostics: Vec<Diagnostic>,
    /// How it ended.
    pub outcome: BuildOutcome,
}

/// A problem that stopped the build machinery itself (not the project).
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    /// The build folder could not be prepared.
    #[error(transparent)]
    BuildDir(#[from] BuildDirError),
    /// The compiler could not be started.
    #[error("could not start the compiler: {0}")]
    Process(#[from] b2c_process::ProcessError),
}

/// The project needs library profiles, which this version cannot build
/// (docs/reference/diagnostics/generator.md).
pub const LIBRARIES_UNSUPPORTED: &str = "B2C-E0702";

/// Compiler messages that are about the compiler run itself, not a line.
const COMPILER_FAILED: &str = "C:failed";
/// The compiler hit its time or memory limit.
const COMPILER_LIMIT: &str = "C:limit";

/// Builds a project: generates C++, then compiles and links it with g++ in
/// the build cache. Nothing is compiled again when the generated sources and
/// the compiler command are unchanged since the last successful build.
///
/// # Errors
/// [`BuildError`] when the build folder cannot be prepared or the compiler
/// cannot be started. Problems with the project or the toolchain are
/// reported in the [`BuildReport`] instead.
pub fn build(project: &[u8], request: &BuildRequest) -> Result<BuildReport, BuildError> {
    let frontend = run_frontend(project, &request.frontend);
    let mut diagnostics = frontend.diagnostics;
    let document = frontend.document;
    let Some(generated) = frontend.generated.filter(|_| !b2c_ir::has_errors(&diagnostics)) else {
        return Ok(BuildReport {
            document,
            diagnostics,
            outcome: BuildOutcome::ProjectErrors,
        });
    };
    let Some(document) = document else {
        return Ok(BuildReport {
            document: None,
            diagnostics,
            outcome: BuildOutcome::ProjectErrors,
        });
    };

    let toolchain = match select(&request.toolchain, &request.cache_root) {
        Selected::Usable(toolchain, notes) => {
            diagnostics.extend(notes);
            toolchain
        }
        Selected::Unusable(problems) => {
            diagnostics.extend(problems);
            return Ok(BuildReport {
                document: Some(document),
                diagnostics,
                outcome: BuildOutcome::ToolchainProblem,
            });
        }
    };
    let defines = project_defines(&document, &mut diagnostics);
    if b2c_ir::has_errors(&diagnostics) {
        return Ok(BuildReport {
            document: Some(document),
            diagnostics,
            outcome: BuildOutcome::ProjectErrors,
        });
    }
    let outcome = compile(
        &document,
        &generated,
        &toolchain,
        &defines,
        request,
        &mut diagnostics,
    )?;
    Ok(BuildReport {
        document: Some(document),
        diagnostics,
        outcome,
    })
}

/// The project's defines, checked for the command line; problems go to
/// `diagnostics`. Library profiles are not supported yet.
fn project_defines(document: &Document, diagnostics: &mut Vec<Diagnostic>) -> Vec<ValidDefine> {
    let mut defines = Vec::new();
    for define in &document.project.build.defines {
        match ValidDefine::new(define) {
            Ok(valid) => defines.push(valid),
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }
    if !document.project.build.libraries.is_empty() {
        diagnostics.push(Diagnostic::error(
            LIBRARIES_UNSUPPORTED,
            DiagSource::Generator,
            Location::project(),
            "This project uses libraries, which this version of Blocks2Cpp cannot build yet.",
        ));
    }
    defines
}

/// Prepares the build folder and runs the compiler steps, unless the
/// program is already up to date.
fn compile(
    document: &Document,
    generated: &GeneratedProject,
    toolchain: &Toolchain,
    defines: &[ValidDefine],
    request: &BuildRequest,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<BuildOutcome, BuildError> {
    let configuration = request.configuration.settings(document);
    let key = config_key(request.configuration, toolchain, configuration, document);
    let dir = BuildDir::create(&request.cache_root, &document.project.id, &key)?;
    let sources = dir.write_generated(generated)?;
    if let Ok(json) = serde_json::to_vec_pretty(&generated.source_map) {
        write_if_changed(&dir.root().join("sourcemap.json"), &json)?;
    }

    let extra = ExtraFlags::none();
    let mut inputs = BuildInputs::new(
        toolchain,
        configuration,
        document.project.language,
        dir.gen_dir(),
        &extra,
    );
    inputs.defines = defines;
    let plan = match CommandPlan::new(&inputs) {
        Ok(plan) => plan,
        Err(diagnostic) => {
            diagnostics.push(diagnostic);
            return Ok(BuildOutcome::ToolchainProblem);
        }
    };
    diagnostics.extend(plan.notes().iter().cloned());

    let executable = dir.out_dir().join(format!(
        "{}{}",
        program_name(document),
        toolchain.platform().exe_suffix()
    ));
    let steps = steps(&plan, &sources, &dir, &executable);
    let stamp = stamp(generated, &steps);
    let stamp_file = dir.root().join("build-stamp");
    if executable.is_file() && std::fs::read(&stamp_file).is_ok_and(|old| old == stamp.as_bytes()) {
        return Ok(BuildOutcome::Built { executable });
    }
    // A failed or interrupted build must never look up to date.
    let _ = std::fs::remove_file(&stamp_file);

    let env = compiler_env(
        toolchain.platform(),
        toolchain.bin_dir(),
        &dir.tmp_dir(),
        &HostEnv::from_process(&[]),
    );
    for step in &steps {
        if !run_step(step, &dir, &env, &generated.source_map, diagnostics)? {
            return Ok(BuildOutcome::ProjectErrors);
        }
    }
    restrict_permissions(&executable);
    write_if_changed(&stamp_file, stamp.as_bytes())?;
    Ok(BuildOutcome::Built { executable })
}

/// Runs one compiler step and records its messages. Returns whether it
/// succeeded.
fn run_step(
    step: &CompilerCommand,
    dir: &BuildDir,
    env: &CompilerEnv,
    source_map: &SourceMap,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<bool, BuildError> {
    let command = step.process_command(&dir.diag_dir(), env, None)?;
    let captured = b2c_process::run_captured(&command)?;
    let parsed = step.read_diagnostics(&dir.diag_dir(), &captured.stderr);
    let mapped = map_messages(&parsed, source_map, &dir.gen_dir());
    let explained = mapped
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error);
    diagnostics.extend(mapped);
    if captured.timed_out || captured.too_many_processes {
        diagnostics.push(Diagnostic::error(
            COMPILER_LIMIT,
            DiagSource::Compiler,
            Location::project(),
            "The compiler ran out of time or memory while building this program.",
        ));
        return Ok(false);
    }
    if captured.status.success() {
        return Ok(true);
    }
    if !explained {
        let mut diagnostic = Diagnostic::error(
            COMPILER_FAILED,
            DiagSource::Compiler,
            Location::project(),
            format!(
                "The compiler stopped without explaining why ({}).",
                captured.status.describe()
            ),
        );
        diagnostic.raw = Some(String::from_utf8_lossy(&captured.stderr).into_owned());
        diagnostics.push(diagnostic);
    }
    Ok(false)
}

/// `<config>-<hash8>`: one build folder per configuration, toolchain and
/// set of build settings, so switching between them never mixes objects.
fn config_key(
    configuration: Configuration,
    toolchain: &Toolchain,
    settings: &BuildConfiguration,
    document: &Document,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(toolchain.fingerprint.sha256.as_bytes());
    hasher.update(toolchain.path().as_os_str().as_encoded_bytes());
    hasher.update(serde_json::to_vec(settings).unwrap_or_default());
    hasher.update(serde_json::to_vec(&document.project.language).unwrap_or_default());
    hasher.update(serde_json::to_vec(&document.project.build.defines).unwrap_or_default());
    let digest = hasher.finalize();
    format!(
        "{}-{}",
        configuration.name(),
        hex(digest.get(..4).unwrap_or_default())
    )
}

/// The executable's file name: the main module's name, which is already
/// restricted to `[a-z][a-z0-9_-]{0,63}` by the loader.
fn program_name(document: &Document) -> String {
    document
        .modules
        .first()
        .map_or_else(|| String::from("program"), |module| module.name.clone())
}

/// The compiler steps: one compile-and-link for a single source file,
/// otherwise a compile per source and a link.
fn steps(plan: &CommandPlan, sources: &[PathBuf], dir: &BuildDir, executable: &Path) -> Vec<CompilerCommand> {
    if let [source] = sources {
        return vec![plan.compile_and_link(source, executable)];
    }
    let objects: Vec<PathBuf> = sources
        .iter()
        .map(|source| {
            let stem = source
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            dir.out_dir().join(format!("{stem}.o"))
        })
        .collect();
    let mut steps: Vec<CompilerCommand> = sources
        .iter()
        .zip(&objects)
        .map(|(source, object)| plan.compile(source, object))
        .collect();
    steps.push(plan.link(&objects, executable));
    steps
}

/// A digest of everything that decides the executable: the generated files
/// and every compiler command.
fn stamp(generated: &GeneratedProject, steps: &[CompilerCommand]) -> String {
    let mut hasher = Sha256::new();
    for file in &generated.files {
        hasher.update(file.path.as_bytes());
        hasher.update([0]);
        hasher.update(file.contents.as_bytes());
        hasher.update([0]);
    }
    for step in steps {
        hasher.update(step.program.as_os_str().as_encoded_bytes());
        for arg in &step.args {
            hasher.update([0]);
            hasher.update(arg.as_encoded_bytes());
        }
        hasher.update([1]);
    }
    hex(&hasher.finalize())
}

/// Lower-case hexadecimal.
fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// Turns g++ and linker messages into diagnostics on the blocks that
/// produced the code (spec §7.5.3).
///
/// Generated code should never fail to compile when the analyser found no
/// errors, so a compiler error is labelled as a probable bug in Blocks2Cpp.
fn map_messages(parsed: &ParsedOutput, source_map: &SourceMap, gen_dir: &Path) -> Vec<Diagnostic> {
    let mut diagnostics: Vec<Diagnostic> = parsed
        .messages
        .iter()
        .filter(|message| message.severity != MessageSeverity::Note)
        .map(|message| map_message(message, source_map, gen_dir))
        .collect();
    if parsed.truncated {
        diagnostics.push(Diagnostic::info(
            "C:truncated",
            DiagSource::Compiler,
            Location::project(),
            "The compiler reported more messages than can be shown; only the first ones are listed.",
        ));
    }
    diagnostics
}

fn map_message(message: &CompilerMessage, source_map: &SourceMap, gen_dir: &Path) -> Diagnostic {
    let linker = message.origin == MessageOrigin::Linker;
    let location = message
        .location
        .as_ref()
        .and_then(|position| {
            let file = generated_file_name(&position.file, gen_dir)?;
            let range = match position.column {
                Some(column) => source_map.lookup(
                    &file,
                    Position {
                        line: position.line,
                        column,
                    },
                ),
                None => source_map.lookup_line(&file, position.line),
            };
            range.map(|range| {
                Location::block(Some(range.module.clone()), range.block.clone()).with_part(range.part.clone())
            })
        })
        .unwrap_or_else(Location::project);
    let code = match &message.option {
        Some(option) => format!("C:{option}"),
        None if linker => String::from("C:link"),
        None => String::from("C:error"),
    };
    let severity = if message.severity.is_error() {
        Severity::Error
    } else {
        Severity::Warning
    };
    let text = if severity == Severity::Error {
        format!(
            "{} reported a problem in the generated C++, which should not happen for blocks without errors. This \
             looks like a bug in Blocks2Cpp; please report it with the project file. It said: {}",
            if linker { "The linker" } else { "g++" },
            message.message
        )
    } else {
        format!("g++ warns: {}", message.message)
    };
    let mut diagnostic = Diagnostic {
        code: b2c_ir::DiagCode(code),
        severity,
        message: text,
        primary: location,
        related: Vec::new(),
        source: if linker {
            DiagSource::Linker
        } else {
            DiagSource::Compiler
        },
        raw: None,
    };
    diagnostic.raw = Some(raw_text(message));
    diagnostic
}

/// The original message, as g++ would have printed it.
fn raw_text(message: &CompilerMessage) -> String {
    let mut out = String::new();
    if let Some(position) = &message.location {
        out.push_str(&position.file);
        out.push(':');
        out.push_str(&position.line.to_string());
        if let Some(column) = position.column {
            out.push(':');
            out.push_str(&column.to_string());
        }
        out.push_str(": ");
    }
    out.push_str(&message.message);
    if let Some(option) = &message.option {
        out.push_str(" [");
        out.push_str(option);
        out.push(']');
    }
    out
}

/// The generated file a compiler path refers to (`main.cpp`), if it is one
/// of ours: the path is inside `gen_dir`, or a bare file name.
fn generated_file_name(reported: &str, gen_dir: &Path) -> Option<String> {
    let path = Path::new(reported);
    let relative = if path.is_absolute() {
        path.strip_prefix(gen_dir).ok()?
    } else {
        path
    };
    let name = relative.to_str()?;
    (!name.contains(['/', '\\'])).then(|| name.to_owned())
}

/// Makes the program owner-only on Unix (spec §7.5.1: mode 0700).
fn restrict_permissions(executable: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = executable;
}

#[cfg(test)]
mod tests {
    use b2c_ir::source_map::{FileMap, MappedRange};
    use b2c_ir::{BlockId, ModuleId, Part};
    use b2c_toolchain::diagnostics::SourcePos;

    use super::*;

    fn gen_dir() -> PathBuf {
        PathBuf::from(if cfg!(windows) {
            r"C:\cache\gen"
        } else {
            "/cache/gen"
        })
    }

    fn source_map() -> SourceMap {
        let range = |line: u32, start: u32, end: u32, block: &str| MappedRange {
            start: Position { line, column: start },
            end: Position { line, column: end },
            module: ModuleId::new("mod_main").unwrap(),
            block: BlockId::new(block).unwrap(),
            part: Part::Whole,
        };
        SourceMap {
            version: 1,
            files: vec![FileMap {
                path: String::from("main.cpp"),
                ranges: vec![range(7, 5, 40, "b003"), range(7, 18, 30, "b004")],
            }],
        }
    }

    fn message(origin: MessageOrigin, severity: MessageSeverity, column: Option<u32>) -> CompilerMessage {
        let mut message = CompilerMessage::new(origin, severity, "something went wrong");
        message.location = Some(SourcePos {
            file: gen_dir().join("main.cpp").to_string_lossy().into_owned(),
            line: 7,
            column,
        });
        message
    }

    #[test]
    fn compiler_errors_land_on_the_innermost_block() {
        let parsed = ParsedOutput {
            messages: vec![message(MessageOrigin::Compiler, MessageSeverity::Error, Some(20))],
            truncated: false,
        };
        let diagnostics = map_messages(&parsed, &source_map(), &gen_dir());
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.code.0, "C:error");
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(diagnostic.source, DiagSource::Compiler);
        assert_eq!(
            diagnostic.primary.block.as_ref().map(BlockId::as_str),
            Some("b004")
        );
        assert!(diagnostic.message.contains("bug in Blocks2Cpp"));
        assert!(
            diagnostic
                .raw
                .as_deref()
                .is_some_and(|raw| raw.contains(":7:20: something went wrong"))
        );
    }

    #[test]
    fn line_only_messages_use_the_statement_and_unknown_places_the_project() {
        let mut warning = message(MessageOrigin::Compiler, MessageSeverity::Warning, None);
        warning.option = Some(String::from("-Wunused-variable"));
        let mut linker = CompilerMessage::new(
            MessageOrigin::Linker,
            MessageSeverity::Error,
            "undefined reference",
        );
        linker.location = Some(SourcePos {
            file: String::from("/usr/lib/crt1.o"),
            line: 1,
            column: None,
        });
        let parsed = ParsedOutput {
            messages: vec![
                warning,
                linker,
                CompilerMessage::new(MessageOrigin::Compiler, MessageSeverity::Note, "a note"),
            ],
            truncated: true,
        };
        let diagnostics = map_messages(&parsed, &source_map(), &gen_dir());
        let codes: Vec<&str> = diagnostics.iter().map(|d| d.code.0.as_str()).collect();
        assert_eq!(codes, ["C:-Wunused-variable", "C:link", "C:truncated"]);
        assert_eq!(diagnostics[0].severity, Severity::Warning);
        assert_eq!(
            diagnostics[0].primary.block.as_ref().map(BlockId::as_str),
            Some("b003")
        );
        assert_eq!(diagnostics[1].source, DiagSource::Linker);
        assert_eq!(diagnostics[1].primary, Location::project());
    }

    #[test]
    fn generated_file_names_are_recognised() {
        let gen_dir = if cfg!(windows) {
            Path::new(r"C:\cache\gen")
        } else {
            Path::new("/cache/gen")
        };
        let inside = gen_dir.join("main.cpp");
        assert_eq!(
            generated_file_name(inside.to_str().unwrap(), gen_dir),
            Some(String::from("main.cpp"))
        );
        assert_eq!(
            generated_file_name("main.cpp", gen_dir),
            Some(String::from("main.cpp"))
        );
        assert_eq!(generated_file_name("/usr/include/c++/13/iostream", gen_dir), None);
        assert_eq!(generated_file_name("sub/main.cpp", gen_dir), None);
    }
}
