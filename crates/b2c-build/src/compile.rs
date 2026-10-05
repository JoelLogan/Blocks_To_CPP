//! Building a project: the front end, then g++ (`docs/spec/07-toolchain-build-run.md` §7.5).
//!
//! [`run_build_job`] is the one implementation of a build. The app's build
//! sessions ([`crate::session::BuildSessions`]) run it on a thread with a
//! cancellation token and stream its events; the command-line tool runs it
//! through the synchronous [`build`].
//!
//! A build:
//!
//! 1. runs the front end on the project bytes (load, resolve, analyse,
//!    generate; 06 §6.1) and stops with *project errors* if any stage
//!    reported an error, before any compiler is chosen or started;
//! 2. checks the toolchain's fingerprint again and probes a changed compiler
//!    once more (`B2C-T1009`, 07 §7.2, 08 §8.5);
//! 3. prepares `<cache>/builds/<project>/<config>-<hash8>/`, waits for its
//!    lock (or for cancellation) and marks it as used;
//! 4. writes the generated files that changed, the source map and, for IDE
//!    builds, the init unit (07 §7.6.3);
//! 5. is *up to date* when nothing was written and the build manifest
//!    records exactly this build's inputs and still matches the executable;
//! 6. otherwise deletes the manifest and compiles: a single translation
//!    unit (plus the init unit) in one compile-and-link invocation, several
//!    in parallel (at most `min(available_parallelism, 8)` at once) and then
//!    a link once all have succeeded;
//! 7. maps every compiler and linker message to the blocks through the source
//!    map (07 §7.5.3), sorted by translation unit, so the order never
//!    depends on which compiler finished first;
//! 8. on success records the manifest atomically; on failure or
//!    cancellation deletes the executable and the objects it was making.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Instant;

use b2c_ipc::dto::{BuildConfig, BuildStage};
use b2c_ir::diag::Related;
use b2c_ir::source_map::{GeneratedProject, Position, SourceMap};
use b2c_ir::{DiagSource, Diagnostic, Location, Severity};
use b2c_model::{BuildConfiguration, Document};
use b2c_process::CancelToken;
use b2c_toolchain::command::{BuildInputs, CommandPlan, CompilerCommand};
use b2c_toolchain::diagnostics::{CompilerMessage, MessageOrigin, MessageSeverity, ParsedOutput, SourcePos};
use b2c_toolchain::env::{CompilerEnv, HostEnv, compiler_env};
use b2c_toolchain::flags::{ExtraFlags, ValidDefine};
use b2c_toolchain::probe::{ProbeError, ProbeOptions, Toolchain, probe};
use sha2::{Digest as _, Sha256};

use crate::build_dir::{BuildDir, BuildDirError, is_device_name, write_generated_tracked, write_if_changed};
use crate::frontend::{FrontendOptions, run_frontend};
use crate::ide;
use crate::manifest::{self, ManifestInputs, ManifestStep, ManifestToolchain, StepKind};
use crate::session::{RecordOutcome, ToolchainForBuild};
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
    /// `debug` or `release`: the first part of the build folder's name.
    pub fn name(self) -> &'static str {
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

impl From<BuildConfig> for Configuration {
    fn from(config: BuildConfig) -> Self {
        match config {
            BuildConfig::Debug => Self::Debug,
            BuildConfig::Release => Self::Release,
        }
    }
}

/// What to build and where (the command-line tool's synchronous build).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildRequest {
    /// The configuration.
    pub configuration: Configuration,
    /// The compiler.
    pub toolchain: ToolchainChoice,
    /// The build cache folder (see [`crate::default_cache_root`]).
    pub cache_root: PathBuf,
    /// Options for generating C++. The command-line tool always uses the
    /// default indent width of 4 (07 §7.9).
    pub frontend: FrontendOptions,
    /// Link the IDE init unit (07 §7.6.3). The command-line tool never does,
    /// so its builds use their own build folders.
    pub ide: bool,
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
    /// The build manifest could not be written or removed.
    #[error("could not record the build: {0}")]
    Manifest(String),
    /// The build was cancelled. (The synchronous [`build`] is never
    /// cancelled; sessions report cancellation as an outcome.)
    #[error("the build was cancelled")]
    Cancelled,
    /// Something that cannot happen for a valid project did.
    #[error("the build failed unexpectedly: {0} (this is a bug in Blocks2Cpp)")]
    Internal(&'static str),
}

/// The project needs library profiles, which this version cannot build
/// (docs/reference/diagnostics/generator.md).
pub const LIBRARIES_UNSUPPORTED: &str = "B2C-E0702";

/// Compiler messages that are about the compiler run itself, not a line.
const COMPILER_FAILED: &str = "C:failed";
/// The compiler hit its time or memory limit.
const COMPILER_LIMIT: &str = "C:limit";
/// The compiler crashed (an internal compiler error).
const COMPILER_CRASHED: &str = "C:crashed";

/// The most translation units compiled at once (07 §7.5.2).
const MAX_PARALLEL_COMPILES: usize = 8;

/// The most related locations kept for one compiler message.
const MAX_RELATED: usize = 16;

/// The longest raw compiler text kept for one message (the "Show C++
/// compiler message" view, 04 §4.4).
const MAX_RAW_BYTES: usize = 16 * 1024;

/// Builds a project: generates C++, then compiles and links it with g++ in
/// the build cache. Nothing is compiled again when the build folder's
/// manifest shows the program is up to date. The compiler is chosen only
/// when the project has no errors.
///
/// # Errors
/// [`BuildError`] when the build folder cannot be prepared, the compiler
/// cannot be started or the result cannot be recorded. Problems with the
/// project or the toolchain are reported in the [`BuildReport`] instead.
pub fn build(project: &[u8], request: &BuildRequest) -> Result<BuildReport, BuildError> {
    let input = JobInput {
        build_id: None,
        document: project,
        configuration: request.configuration,
        frontend: &request.frontend,
        ide: request.ide,
        cache_root: &request.cache_root,
    };
    let choose = || match select(&request.toolchain, &request.cache_root) {
        Selected::Usable(toolchain, notes) => ToolchainForBuild::Ready { toolchain, notes },
        Selected::Unusable(diagnostics) => ToolchainForBuild::Unavailable { diagnostics },
    };
    let mut diagnostics = Vec::new();
    let result = run_build_job(&input, choose, &CancelToken::new(), &mut |event| {
        if let JobEvent::Diagnostics(items) = event {
            diagnostics.extend(items);
        }
    });
    let outcome = match result.outcome {
        RecordOutcome::Built | RecordOutcome::UpToDate => match result.executable {
            Some(executable) => BuildOutcome::Built { executable },
            None => return Err(BuildError::Internal("a successful build has no executable")),
        },
        RecordOutcome::ProjectErrors => BuildOutcome::ProjectErrors,
        RecordOutcome::ToolchainProblem => BuildOutcome::ToolchainProblem,
        RecordOutcome::Cancelled => return Err(BuildError::Cancelled),
        RecordOutcome::Failed => {
            return Err(result
                .failure
                .unwrap_or(BuildError::Internal("a failed build gave no reason")));
        }
    };
    Ok(BuildReport {
        document: result.document,
        diagnostics,
        outcome,
    })
}

/// What a build reports while it runs; sessions turn these into
/// `b2c_ipc::BuildEvent`s.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum JobEvent {
    /// Progress through a stage.
    Progress {
        /// The stage.
        stage: BuildStage,
        /// Steps done.
        done: u32,
        /// Steps in the stage.
        total: u32,
    },
    /// Diagnostics of one stage, never empty.
    Diagnostics(Vec<Diagnostic>),
}

/// The inputs of one build.
#[derive(Debug, Clone, Copy)]
pub(crate) struct JobInput<'a> {
    /// The session's build ID, for the log.
    pub(crate) build_id: Option<&'a str>,
    /// The project file's bytes (untrusted).
    pub(crate) document: &'a [u8],
    /// Debug or release.
    pub(crate) configuration: Configuration,
    /// How C++ is generated.
    pub(crate) frontend: &'a FrontendOptions,
    /// Whether to link the IDE init unit.
    pub(crate) ide: bool,
    /// The cache root (`builds/` is created below it).
    pub(crate) cache_root: &'a Path,
}

/// The result of [`run_build_job`]: everything a build record holds.
#[derive(Debug)]
pub(crate) struct JobResult {
    /// How it ended.
    pub(crate) outcome: RecordOutcome,
    /// Why, when the outcome is [`RecordOutcome::Failed`].
    pub(crate) failure: Option<BuildError>,
    /// The content hash of the project, when it loaded.
    pub(crate) project_hash: Option<[u8; 32]>,
    /// `<config>-<hash8>`, once the toolchain was known (otherwise empty).
    pub(crate) config_key: String,
    /// The build folder, once it was prepared.
    pub(crate) build_dir: Option<PathBuf>,
    /// The executable, after a successful build.
    pub(crate) executable: Option<PathBuf>,
    /// The project with catalog defaults filled in, when it loaded.
    pub(crate) document: Option<Document>,
    /// Whether the program was built with a sanitizer.
    pub(crate) sanitizers: bool,
    /// Whether the toolchain's leak detection works.
    pub(crate) leak_detection: bool,
    /// The toolchain's `bin` folder.
    pub(crate) toolchain_bin: Option<PathBuf>,
}

/// Runs one build (see the module documentation). `toolchain` is called
/// only when the project has no errors. Every event goes to `emit`; the
/// outcome is in the result. Never panics; cancelling `cancel` stops the
/// build at the next step and kills a running compiler (after a 2 s grace).
pub(crate) fn run_build_job(
    input: &JobInput<'_>,
    toolchain: impl FnOnce() -> ToolchainForBuild,
    cancel: &CancelToken,
    emit: &mut dyn FnMut(JobEvent),
) -> JobResult {
    let span = tracing::info_span!(
        "build",
        build_id = input.build_id.unwrap_or(""),
        config = input.configuration.name(),
        ide = input.ide,
        toolchain = tracing::field::Empty,
    );
    let _entered = span.enter();
    let started = Instant::now();
    let mut result = JobResult {
        outcome: RecordOutcome::Failed,
        failure: None,
        project_hash: None,
        config_key: String::new(),
        build_dir: None,
        executable: None,
        document: None,
        sanitizers: false,
        leak_detection: false,
        toolchain_bin: None,
    };
    let mut reporter = Reporter { emit };
    let ended = run_job(input, toolchain, cancel, &mut reporter, &mut result);
    match ended {
        Ok(outcome) => result.outcome = outcome,
        Err(error) => {
            tracing::warn!(error = %error_kind(&error), "the build failed");
            tracing::debug!(error = %error, "the build failure in full");
            result.outcome = RecordOutcome::Failed;
            result.failure = Some(error);
        }
    }
    if !matches!(result.outcome, RecordOutcome::Built | RecordOutcome::UpToDate) {
        result.executable = None;
    }
    tracing::info!(
        outcome = result.outcome.as_str(),
        duration_ms = elapsed_ms(started),
        "build finished"
    );
    result
}

/// A short description of a build failure without paths, for the log at
/// information level.
fn error_kind(error: &BuildError) -> &'static str {
    match error {
        BuildError::BuildDir(BuildDirError::NotADirectory { .. }) => {
            "a build folder is a link or not a folder"
        }
        BuildError::BuildDir(BuildDirError::NotAFile { .. }) => "a build file is a link or not a file",
        BuildError::BuildDir(BuildDirError::BadFileName { .. }) => "a generated file name was refused",
        BuildError::BuildDir(BuildDirError::BadConfigKey { .. }) => "a configuration key was refused",
        BuildError::BuildDir(BuildDirError::Io { action, .. }) => action,
        BuildError::Process(_) => "the compiler could not be started",
        BuildError::Manifest(_) => "the build manifest could not be updated",
        BuildError::Cancelled => "cancelled",
        BuildError::Internal(reason) => reason,
    }
}

/// Milliseconds since `started`, saturating.
pub(crate) fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Sends events, dropping empty diagnostics batches.
struct Reporter<'e> {
    emit: &'e mut dyn FnMut(JobEvent),
}

impl Reporter<'_> {
    fn progress(&mut self, stage: BuildStage, done: usize, total: usize) {
        (self.emit)(JobEvent::Progress {
            stage,
            done: u32::try_from(done).unwrap_or(u32::MAX),
            total: u32::try_from(total).unwrap_or(u32::MAX),
        });
    }

    fn diagnostics(&mut self, items: Vec<Diagnostic>) {
        if !items.is_empty() {
            (self.emit)(JobEvent::Diagnostics(items));
        }
    }
}

/// The front end and the choice of toolchain; then [`compile_project`].
fn run_job(
    input: &JobInput<'_>,
    toolchain: impl FnOnce() -> ToolchainForBuild,
    cancel: &CancelToken,
    reporter: &mut Reporter<'_>,
    result: &mut JobResult,
) -> Result<RecordOutcome, BuildError> {
    if cancel.is_cancelled() {
        return Ok(RecordOutcome::Cancelled);
    }
    reporter.progress(BuildStage::Generate, 0, 1);
    let generate_started = Instant::now();
    let frontend = run_frontend(input.document, input.frontend);
    result.project_hash = frontend.project_hash;
    let errors = frontend.has_errors();
    tracing::info!(
        stage = "generate",
        duration_ms = elapsed_ms(generate_started),
        diagnostics = frontend.diagnostics.len(),
        errors,
        "generated C++"
    );
    reporter.diagnostics(frontend.diagnostics);
    reporter.progress(BuildStage::Generate, 1, 1);
    let document = frontend.document;
    let outcome = match (document.as_ref(), frontend.generated) {
        (Some(document), Some(generated)) if !errors => {
            with_toolchain(input, document, &generated, toolchain, cancel, reporter, result)
        }
        _ => Ok(RecordOutcome::ProjectErrors),
    };
    result.document = document;
    outcome
}

/// Everything after the front end: choosing and checking the toolchain,
/// then [`compile_project`].
fn with_toolchain(
    input: &JobInput<'_>,
    document: &Document,
    generated: &GeneratedProject,
    toolchain: impl FnOnce() -> ToolchainForBuild,
    cancel: &CancelToken,
    reporter: &mut Reporter<'_>,
    result: &mut JobResult,
) -> Result<RecordOutcome, BuildError> {
    if cancel.is_cancelled() {
        return Ok(RecordOutcome::Cancelled);
    }
    let (toolchain, mut notes) = match toolchain() {
        ToolchainForBuild::Ready { toolchain, notes } => (toolchain, notes),
        ToolchainForBuild::Unavailable { diagnostics } => {
            reporter.diagnostics(diagnostics);
            return Ok(RecordOutcome::ToolchainProblem);
        }
    };
    let toolchain = match recheck_toolchain(toolchain, cancel) {
        Recheck::Current(toolchain) => toolchain,
        Recheck::Changed(toolchain) => {
            notes.push(b2c_toolchain::codes::toolchain_changed(toolchain.path()));
            notes.extend(toolchain.problems.iter().cloned());
            if !toolchain.is_usable() {
                reporter.diagnostics(notes);
                return Ok(RecordOutcome::ToolchainProblem);
            }
            toolchain
        }
        Recheck::Unusable(problem) => {
            notes.push(problem);
            reporter.diagnostics(notes);
            return Ok(RecordOutcome::ToolchainProblem);
        }
        Recheck::Cancelled => return Ok(RecordOutcome::Cancelled),
    };
    tracing::Span::current().record(
        "toolchain",
        toolchain
            .version
            .map_or_else(|| String::from("unknown"), |version| version.to_string())
            .as_str(),
    );
    result.leak_detection = toolchain.capabilities.sanitizers.leak_detection;
    result.toolchain_bin = Some(toolchain.bin_dir().to_path_buf());

    let defines = project_defines(document, &mut notes);
    if b2c_ir::has_errors(&notes) {
        reporter.diagnostics(notes);
        return Ok(RecordOutcome::ProjectErrors);
    }
    let settings = input.configuration.settings(document);
    let key = config_key(
        input.configuration,
        &toolchain,
        settings,
        document,
        input.frontend.indent(),
        input.ide,
    );
    result.config_key.clone_from(&key);
    let project = Project {
        document,
        generated,
        toolchain: &toolchain,
        defines: &defines,
        settings,
        key: &key,
    };
    compile_project(input, &project, notes, cancel, reporter, result)
}

/// The outcome of checking a toolchain's fingerprint before a build.
enum Recheck {
    /// Unchanged since it was probed.
    Current(Box<Toolchain>),
    /// Changed, and probed again (it may now be unusable).
    Changed(Box<Toolchain>),
    /// Changed, and it could not be probed again.
    Unusable(Diagnostic),
    /// Cancelled while probing.
    Cancelled,
}

/// Checks the toolchain's fingerprint (path, size, time and SHA-256) and
/// probes a changed compiler again (07 §7.2, 08 §8.5).
fn recheck_toolchain(toolchain: Box<Toolchain>, cancel: &CancelToken) -> Recheck {
    if toolchain.is_current() {
        return Recheck::Current(toolchain);
    }
    tracing::info!("the compiler changed since it was checked; checking it again");
    let options = ProbeOptions {
        cancel: Some(cancel.clone()),
        ..ProbeOptions::default()
    };
    match probe(toolchain.path(), &options) {
        Ok(probed) => Recheck::Changed(Box::new(probed)),
        Err(ProbeError::Cancelled) => Recheck::Cancelled,
        Err(error) => Recheck::Unusable(Diagnostic::error(
            b2c_toolchain::codes::NOT_RUNNABLE,
            DiagSource::Toolchain,
            Location::project(),
            format!(
                "The compiler {} changed since it was last checked and could not be checked again: {error}.",
                toolchain.path().display()
            ),
        )),
    }
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

/// What [`compile_project`] builds.
struct Project<'a> {
    document: &'a Document,
    generated: &'a GeneratedProject,
    toolchain: &'a Toolchain,
    defines: &'a [ValidDefine],
    settings: &'a BuildConfiguration,
    key: &'a str,
}

/// One planned compiler step.
struct Step {
    kind: StepKind,
    command: CompilerCommand,
}

/// Prepares the build folder, decides whether the program is up to date,
/// and otherwise compiles it.
fn compile_project(
    input: &JobInput<'_>,
    project: &Project<'_>,
    mut notes: Vec<Diagnostic>,
    cancel: &CancelToken,
    reporter: &mut Reporter<'_>,
    result: &mut JobResult,
) -> Result<RecordOutcome, BuildError> {
    let dir = BuildDir::create(input.cache_root, &project.document.project.id, project.key)?;
    tracing::debug!(build_dir = %dir.root().display(), "build folder");
    result.build_dir = Some(dir.root().to_path_buf());
    let Some(_lock) = dir.try_lock_until(cancel)? else {
        reporter.diagnostics(notes);
        return Ok(RecordOutcome::Cancelled);
    };
    if let Err(error) = dir.touch() {
        tracing::debug!(error = %error, "could not mark the build folder as used");
    }

    let extra = ExtraFlags::none();
    let mut inputs = BuildInputs::new(
        project.toolchain,
        project.settings,
        project.document.project.language,
        dir.gen_dir(),
        &extra,
    );
    inputs.defines = project.defines;
    let plan = match CommandPlan::new(&inputs) {
        Ok(plan) => plan,
        Err(diagnostic) => {
            notes.push(diagnostic);
            reporter.diagnostics(notes);
            return Ok(RecordOutcome::ToolchainProblem);
        }
    };
    notes.extend(plan.notes().iter().cloned());
    reporter.diagnostics(notes);
    result.sanitizers = plan
        .compile_flags_for_key()
        .iter()
        .any(|flag| flag.to_string_lossy().starts_with("-fsanitize="));

    let prepared = prepare(
        input,
        project,
        &dir,
        &plan,
        result.project_hash.unwrap_or_default(),
    )?;
    if prepared.up_to_date {
        result.executable = Some(prepared.executable);
        return Ok(RecordOutcome::UpToDate);
    }
    // A failed or interrupted build must never look up to date.
    manifest::remove(dir.root()).map_err(|error| manifest_error(&error))?;
    let outcome = compile_and_record(project, &dir, &prepared, cancel, reporter);
    if outcome
        .as_ref()
        .is_ok_and(|outcome| *outcome == RecordOutcome::Built)
    {
        result.executable = Some(prepared.executable);
    }
    outcome
}

/// A manifest file that could not be removed.
fn manifest_error(error: &std::io::Error) -> BuildError {
    BuildError::Manifest(error.kind().to_string())
}

/// A build folder with its files written and its steps planned.
struct Prepared {
    steps: Vec<Step>,
    manifest_inputs: ManifestInputs,
    executable: PathBuf,
    up_to_date: bool,
}

/// Writes the generated files, the source map and (for IDE builds) the init
/// unit, plans the compiler steps, and decides whether the program is up to
/// date.
fn prepare(
    input: &JobInput<'_>,
    project: &Project<'_>,
    dir: &BuildDir,
    plan: &CommandPlan,
    project_hash: [u8; 32],
) -> Result<Prepared, BuildError> {
    // A build whose generated files change must never look up to date, even
    // if it is interrupted before it compiles: drop the manifest first.
    let pending = files_differ(dir, project.generated, input.ide);
    if pending {
        manifest::remove(dir.root()).map_err(|error| manifest_error(&error))?;
    }
    let written = write_generated_tracked(&dir.gen_dir(), project.generated)?;
    if let Ok(json) = serde_json::to_vec_pretty(&project.generated.source_map) {
        write_if_changed(&dir.root().join("sourcemap.json"), &json)?;
    }
    let main_count = written.sources.len();
    if main_count == 0 {
        return Err(BuildError::Internal("the generated project has no source file"));
    }
    let mut sources = written.sources;
    let mut changed = pending || written.changed;
    if input.ide {
        let (path, ide_changed) = ide::write_init_unit(dir)?;
        changed |= ide_changed;
        sources.push(path);
    }

    let executable = dir.out_dir().join(format!(
        "{}{}",
        program_name(project.document),
        project.toolchain.platform().exe_suffix()
    ));
    let steps = plan_steps(plan, &sources, main_count, dir, &executable);
    let manifest_inputs = ManifestInputs {
        project_hash,
        toolchain: ManifestToolchain::new(project.toolchain),
        ide: input.ide,
        steps: steps
            .iter()
            .map(|step| ManifestStep::new(step.kind, &step.command))
            .collect(),
        executable_name: executable
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    let up_to_date = !changed
        && manifest::read(dir.root()).is_some_and(|recorded| {
            recorded.records(&manifest_inputs)
                && manifest::digest_file(&executable).is_ok_and(|digest| recorded.executable.matches(&digest))
        });
    Ok(Prepared {
        steps,
        manifest_inputs,
        executable,
        up_to_date,
    })
}

/// Runs the compiler steps and, when they succeed, records the manifest.
/// Removes what the steps wrote unless the build succeeded.
fn compile_and_record(
    project: &Project<'_>,
    dir: &BuildDir,
    prepared: &Prepared,
    cancel: &CancelToken,
    reporter: &mut Reporter<'_>,
) -> Result<RecordOutcome, BuildError> {
    let env = compiler_env(
        project.toolchain.platform(),
        project.toolchain.bin_dir(),
        &dir.tmp_dir(),
        &HostEnv::from_process(&[]),
    );
    let context = StepContext {
        dir,
        env: &env,
        source_map: &project.generated.source_map,
        cancel,
    };
    let outcome = match run_steps(&context, &prepared.steps, reporter) {
        // Cancelled after the last step: the build is still cancelled.
        Ok(StepResult::Succeeded) if cancel.is_cancelled() => Ok(StepResult::Cancelled),
        other => other,
    };
    let recorded = match outcome {
        Ok(StepResult::Succeeded) => {
            restrict_permissions(&prepared.executable);
            manifest::digest_file(&prepared.executable)
                .map_err(|_| BuildError::Internal("the compiler reported success but wrote no program"))
                .and_then(|digest| {
                    manifest::write(dir.root(), &prepared.manifest_inputs.finish(&digest))
                        .map_err(|error| BuildError::Manifest(error.to_string()))
                })
                .map(|()| RecordOutcome::Built)
        }
        Ok(StepResult::Crashed) => Ok(RecordOutcome::ToolchainProblem),
        Ok(StepResult::Cancelled) => Ok(RecordOutcome::Cancelled),
        Ok(StepResult::Failed) => Ok(RecordOutcome::ProjectErrors),
        Err(error) => Err(error),
    };
    if !matches!(recorded, Ok(RecordOutcome::Built)) {
        remove_outputs(&prepared.steps);
    }
    recorded
}

/// Whether writing the generated files (and, for IDE builds, the init unit)
/// would change anything on disk.
fn files_differ(dir: &BuildDir, generated: &GeneratedProject, ide: bool) -> bool {
    let differs = |path: &Path, contents: &[u8]| {
        std::fs::symlink_metadata(path)
            .ok()
            .filter(std::fs::Metadata::is_file)
            .and_then(|_| std::fs::read(path).ok())
            .is_none_or(|existing| existing != contents)
    };
    generated
        .files
        .iter()
        .any(|file| differs(&dir.gen_dir().join(&file.path), file.contents.as_bytes()))
        || (ide
            && differs(
                &dir.ide_dir().join(ide::INIT_UNIT_FILE),
                ide::INIT_UNIT_SOURCE.as_bytes(),
            ))
}

/// Deletes what the steps write (objects and the executable), best effort:
/// a running program on Windows cannot be deleted, and its manifest is
/// already gone.
fn remove_outputs(steps: &[Step]) {
    for step in steps {
        if let Some(output) = output_of(&step.command) {
            let _ = std::fs::remove_file(output);
        }
    }
}

/// The `-o` argument of a compiler command.
fn output_of(command: &CompilerCommand) -> Option<PathBuf> {
    let position = command.args.iter().position(|arg| arg == "-o")?;
    command.args.get(position + 1).map(PathBuf::from)
}

/// `<config>-<hash8>`: one build folder per configuration, toolchain and set
/// of build settings, so switching between them never mixes objects. The
/// hash covers the toolchain's SHA-256 and canonical path, the build
/// configuration, the language settings, the defines, the indent width and
/// the IDE flag (07 §7.5.1).
fn config_key(
    configuration: Configuration,
    toolchain: &Toolchain,
    settings: &BuildConfiguration,
    document: &Document,
    indent_width: u8,
    ide: bool,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(toolchain.fingerprint.sha256.as_bytes());
    hasher.update(toolchain.path().as_os_str().as_encoded_bytes());
    hasher.update(serde_json::to_vec(settings).unwrap_or_default());
    hasher.update(serde_json::to_vec(&document.project.language).unwrap_or_default());
    hasher.update(serde_json::to_vec(&document.project.build.defines).unwrap_or_default());
    hasher.update([indent_width, u8::from(ide)]);
    let digest = hasher.finalize();
    format!(
        "{}-{}",
        configuration.name(),
        b2c_model::hex(digest.get(..4).unwrap_or_default())
    )
}

/// The executable's file name: the main module's name, which the loader
/// already restricts to `[a-z][a-z0-9_-]{0,63}`. A name that Windows
/// reserves for a device (`con`, `nul`, …) and anything unexpected become
/// `program`.
fn program_name(document: &Document) -> String {
    document
        .modules
        .first()
        .map(|module| module.name.as_str())
        .filter(|name| {
            !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
                && !is_device_name(name)
        })
        .map_or_else(|| String::from("program"), str::to_owned)
}

/// The compiler steps. `sources` are the generated sources (the first
/// `main_count`) followed by the IDE init unit, if any. One generated source
/// is compiled and linked with the init unit in one invocation; several are
/// compiled one by one (the init unit too) and then linked.
fn plan_steps(
    plan: &CommandPlan,
    sources: &[PathBuf],
    main_count: usize,
    dir: &BuildDir,
    executable: &Path,
) -> Vec<Step> {
    if main_count == 1 {
        return vec![Step {
            kind: StepKind::CompileAndLink,
            command: plan.compile_and_link_many(sources, executable),
        }];
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
    let mut steps: Vec<Step> = sources
        .iter()
        .zip(&objects)
        .map(|(source, object)| Step {
            kind: StepKind::Compile,
            command: plan.compile(source, object),
        })
        .collect();
    steps.push(Step {
        kind: StepKind::Link,
        command: plan.link(&objects, executable),
    });
    steps
}

/// How one compiler step (or a group of them) ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepResult {
    Succeeded,
    /// It failed; the recorded diagnostics say why.
    Failed,
    /// The compiler crashed, so the compiler, not the project, is at fault.
    Crashed,
    /// The build was cancelled while it ran.
    Cancelled,
}

impl StepResult {
    /// The worse of two results, for a group of parallel steps: a crash
    /// outranks a failure, and cancellation outranks both.
    fn worst(self, other: Self) -> Self {
        let rank = |result: Self| match result {
            Self::Succeeded => 0,
            Self::Failed => 1,
            Self::Crashed => 2,
            Self::Cancelled => 3,
        };
        if rank(other) > rank(self) { other } else { self }
    }
}

/// What every step needs.
struct StepContext<'a> {
    dir: &'a BuildDir,
    env: &'a CompilerEnv,
    source_map: &'a SourceMap,
    cancel: &'a CancelToken,
}

/// Runs the steps: one compile-and-link, or the compiles in parallel and
/// then the link. Reports progress and one diagnostics batch per stage.
fn run_steps(
    context: &StepContext<'_>,
    steps: &[Step],
    reporter: &mut Reporter<'_>,
) -> Result<StepResult, BuildError> {
    if let [single] = steps {
        reporter.progress(BuildStage::Compile, 0, 1);
        let report = run_step(context, single)?;
        reporter.diagnostics(report.diagnostics);
        if report.result == StepResult::Succeeded {
            reporter.progress(BuildStage::Compile, 1, 1);
            reporter.progress(BuildStage::Link, 1, 1);
        }
        return Ok(report.result);
    }
    let Some((link, compiles)) = steps.split_last() else {
        return Err(BuildError::Internal("a build without compiler steps"));
    };
    let compile_result = run_parallel(context, compiles, reporter)?;
    if compile_result != StepResult::Succeeded {
        return Ok(compile_result);
    }
    if context.cancel.is_cancelled() {
        return Ok(StepResult::Cancelled);
    }
    reporter.progress(BuildStage::Link, 0, 1);
    let report = run_step(context, link)?;
    reporter.diagnostics(report.diagnostics);
    if report.result == StepResult::Succeeded {
        reporter.progress(BuildStage::Link, 1, 1);
    }
    Ok(report.result)
}

/// Compiles the translation units in parallel, at most
/// `min(available_parallelism, 8)` at once. Progress is reported as each one
/// finishes; the diagnostics are reported once, in translation-unit order.
fn run_parallel(
    context: &StepContext<'_>,
    steps: &[Step],
    reporter: &mut Reporter<'_>,
) -> Result<StepResult, BuildError> {
    let total = steps.len();
    reporter.progress(BuildStage::Compile, 0, total);
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .clamp(1, MAX_PARALLEL_COMPILES)
        .min(total);
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel::<(usize, Result<StepReport, BuildError>)>();
    let mut reports: Vec<Option<Result<StepReport, BuildError>>> = (0..total).map(|_| None).collect();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let sender = sender.clone();
            let next = &next;
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::SeqCst);
                    let Some(step) = steps.get(index) else {
                        break;
                    };
                    let report = if context.cancel.is_cancelled() {
                        Ok(StepReport {
                            result: StepResult::Cancelled,
                            diagnostics: Vec::new(),
                        })
                    } else {
                        run_step(context, step)
                    };
                    if sender.send((index, report)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        let mut done = 0;
        for (index, report) in &receiver {
            done += 1;
            reporter.progress(BuildStage::Compile, done, total);
            if let Some(slot) = reports.get_mut(index) {
                *slot = Some(report);
            }
        }
    });
    let mut worst = StepResult::Succeeded;
    let mut diagnostics = Vec::new();
    let mut first_error = None;
    for report in reports {
        match report {
            Some(Ok(report)) => {
                worst = worst.worst(report.result);
                diagnostics.extend(report.diagnostics);
            }
            Some(Err(error)) => {
                first_error.get_or_insert(error);
            }
            None => worst = worst.worst(StepResult::Cancelled),
        }
    }
    reporter.diagnostics(diagnostics);
    match first_error {
        Some(error) => Err(error),
        None => Ok(worst),
    }
}

/// How one step ended and what it reported.
struct StepReport {
    result: StepResult,
    diagnostics: Vec<Diagnostic>,
}

/// Runs one compiler step and maps its messages.
fn run_step(context: &StepContext<'_>, step: &Step) -> Result<StepReport, BuildError> {
    let started = Instant::now();
    let report = run_step_inner(context, &step.command)?;
    tracing::info!(
        step = step_name(step.kind),
        duration_ms = elapsed_ms(started),
        result = step_result_name(report.result),
        diagnostics = report.diagnostics.len(),
        "compiler step finished"
    );
    Ok(report)
}

fn step_name(kind: StepKind) -> &'static str {
    match kind {
        StepKind::CompileAndLink => "compileAndLink",
        StepKind::Compile => "compile",
        StepKind::Link => "link",
    }
}

fn step_result_name(result: StepResult) -> &'static str {
    match result {
        StepResult::Succeeded => "succeeded",
        StepResult::Failed => "failed",
        StepResult::Crashed => "crashed",
        StepResult::Cancelled => "cancelled",
    }
}

fn run_step_inner(context: &StepContext<'_>, step: &CompilerCommand) -> Result<StepReport, BuildError> {
    let Some(mut run) = run_once(context, step)? else {
        return Ok(StepReport {
            result: StepResult::Cancelled,
            diagnostics: Vec::new(),
        });
    };
    if run.crashed() {
        // Some GCC releases crash only while writing SARIF or JSON
        // diagnostics; plain text avoids that code.
        if let Some(plain) = step.with_plain_diagnostics() {
            match run_once(context, &plain)? {
                Some(again) => run = again,
                None => {
                    return Ok(StepReport {
                        result: StepResult::Cancelled,
                        diagnostics: Vec::new(),
                    });
                }
            }
        }
    }
    let crashed = run.crashed();
    let explained = run.explained();
    let StepRun { captured, mut mapped } = run;
    if captured.cancelled {
        return Ok(StepReport {
            result: StepResult::Cancelled,
            diagnostics: Vec::new(),
        });
    }
    if captured.timed_out || captured.too_many_processes || captured.out_of_memory {
        mapped.push(Diagnostic::error(
            COMPILER_LIMIT,
            DiagSource::Compiler,
            Location::project(),
            "The compiler ran out of time or memory while building this program.",
        ));
        return Ok(StepReport {
            result: StepResult::Failed,
            diagnostics: mapped,
        });
    }
    if captured.status.success() {
        return Ok(StepReport {
            result: StepResult::Succeeded,
            diagnostics: mapped,
        });
    }
    if explained {
        return Ok(StepReport {
            result: StepResult::Failed,
            diagnostics: mapped,
        });
    }
    let mut diagnostic = if crashed {
        Diagnostic::error(
            COMPILER_CRASHED,
            DiagSource::Compiler,
            Location::project(),
            "The compiler crashed (an internal compiler error in g++) while building this program. This is a bug in g++, not in your project; try a different g++ version.",
        )
    } else {
        Diagnostic::error(
            COMPILER_FAILED,
            DiagSource::Compiler,
            Location::project(),
            format!(
                "The compiler stopped without explaining why ({}).",
                captured.status.describe()
            ),
        )
    };
    diagnostic.raw = Some(cap_raw(String::from_utf8_lossy(&captured.stderr).into_owned()));
    mapped.push(diagnostic);
    Ok(StepReport {
        result: if crashed {
            StepResult::Crashed
        } else {
            StepResult::Failed
        },
        diagnostics: mapped,
    })
}

/// One run of a compiler step: its output and its messages, mapped to
/// blocks.
struct StepRun {
    captured: b2c_process::Captured,
    mapped: Vec<Diagnostic>,
}

impl StepRun {
    /// Whether any message is an error.
    fn explained(&self) -> bool {
        self.mapped
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    }

    /// Whether the compiler crashed without reporting an error first.
    fn crashed(&self) -> bool {
        // A compiler stopped by a limit is not a crash, even when g++ reports
        // the killed cc1plus as an "internal compiler error: Killed".
        !self.captured.status.success()
            && !self.captured.timed_out
            && !self.captured.too_many_processes
            && !self.captured.out_of_memory
            && !self.captured.cancelled
            && !self.explained()
            && String::from_utf8_lossy(&self.captured.stderr).contains("internal compiler error")
    }
}

/// Runs a step once; `None` when the build was cancelled before it started.
fn run_once(context: &StepContext<'_>, step: &CompilerCommand) -> Result<Option<StepRun>, BuildError> {
    let mut command = step.process_command(&context.dir.diag_dir(), context.env, None)?;
    command.cancel_token(context.cancel);
    let captured = match b2c_process::run_captured(&command) {
        Ok(captured) => captured,
        Err(b2c_process::ProcessError::Cancelled) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let parsed = step.read_diagnostics(&context.dir.diag_dir(), &captured.stderr);
    let dirs = MapDirs {
        gen_dir: context.dir.gen_dir(),
        ide_dir: context.dir.ide_dir(),
    };
    let mapped = map_messages(&parsed, context.source_map, &dirs);
    Ok(Some(StepRun { captured, mapped }))
}

/// The folders whose files compiler messages can point into.
struct MapDirs {
    gen_dir: PathBuf,
    ide_dir: PathBuf,
}

/// Turns g++ and linker messages into diagnostics on the blocks that
/// produced the code (spec §7.5.3).
///
/// Generated code should never fail to compile when the analyser found no
/// errors, so a compiler error is labelled as a probable bug in Blocks2Cpp,
/// and so is any message about the IDE init unit.
fn map_messages(parsed: &ParsedOutput, source_map: &SourceMap, dirs: &MapDirs) -> Vec<Diagnostic> {
    let mut diagnostics: Vec<Diagnostic> = parsed
        .messages
        .iter()
        .filter(|message| message.severity != MessageSeverity::Note)
        .map(|message| map_message(message, source_map, dirs))
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

/// Where a compiler position is: on a block, in a generated module (but not
/// in any block), or neither.
fn locate(position: &SourcePos, source_map: &SourceMap, gen_dir: &Path) -> Option<Location> {
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
    match range {
        Some(range) => Some(
            Location::block(Some(range.module.clone()), range.block.clone()).with_part(range.part.clone()),
        ),
        // An unmapped place in a generated file (an include, a helper)
        // belongs to that file's module (06 §6.9).
        None => source_map
            .files
            .iter()
            .find(|map| map.path == file)
            .and_then(|map| map.ranges.first())
            .map(|range| Location {
                module: Some(range.module.clone()),
                block: None,
                part: b2c_ir::Part::Whole,
            }),
    }
}

/// Include chains and notes that point into the blocks become related
/// locations (07 §7.5.3 step 2), each place once, at most [`MAX_RELATED`].
fn related_locations(message: &CompilerMessage, source_map: &SourceMap, gen_dir: &Path) -> Vec<Related> {
    let mut related: Vec<Related> = Vec::new();
    let mut add = |position: &SourcePos, text: &str| {
        if related.len() >= MAX_RELATED {
            return;
        }
        let Some(location) =
            locate(position, source_map, gen_dir).filter(|location| location.block.is_some())
        else {
            return;
        };
        if related.iter().any(|existing| existing.location == location) {
            return;
        }
        related.push(Related {
            location,
            message: text.to_owned(),
        });
    };
    for position in &message.included_from {
        add(position, "Included from here.");
    }
    for child in &message.children {
        if let Some(position) = &child.location {
            add(position, &child.message);
        }
    }
    related
}

fn map_message(message: &CompilerMessage, source_map: &SourceMap, dirs: &MapDirs) -> Diagnostic {
    let linker = message.origin == MessageOrigin::Linker;
    let in_ide_unit = message
        .location
        .as_ref()
        .is_some_and(|position| is_ide_file(&position.file, &dirs.ide_dir));
    let location = message
        .location
        .as_ref()
        .filter(|_| !in_ide_unit)
        .and_then(|position| locate(position, source_map, &dirs.gen_dir))
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
    let text = if in_ide_unit {
        format!(
            "g++ reported a problem in the helper code that Blocks2Cpp adds to the programs it runs, which \
             should not happen. This looks like a bug in Blocks2Cpp; please report it. It said: {}",
            message.message
        )
    } else if severity == Severity::Error {
        format!(
            "{} reported a problem in the generated C++, which should not happen for blocks without errors. This \
             looks like a bug in Blocks2Cpp; please report it with the project file. It said: {}",
            if linker { "The linker" } else { "g++" },
            message.message
        )
    } else {
        format!("g++ warns: {}", message.message)
    };
    Diagnostic {
        code: b2c_ir::DiagCode(code),
        severity,
        message: text,
        primary: location,
        related: if in_ide_unit {
            Vec::new()
        } else {
            related_locations(message, source_map, &dirs.gen_dir)
        },
        source: if linker {
            DiagSource::Linker
        } else {
            DiagSource::Compiler
        },
        raw: Some(raw_text(message)),
    }
}

/// `file:line:col: ` for a position.
fn position_prefix(position: &SourcePos) -> String {
    let mut out = format!("{}:{}", position.file, position.line);
    if let Some(column) = position.column {
        out.push(':');
        out.push_str(&column.to_string());
    }
    out
}

/// The original message, as g++ would have printed it: the include chain,
/// the message with its option, and its notes. At most [`MAX_RAW_BYTES`].
fn raw_text(message: &CompilerMessage) -> String {
    let mut out = String::new();
    for (index, position) in message.included_from.iter().rev().enumerate() {
        out.push_str(if index == 0 {
            "In file included from "
        } else {
            "                 from "
        });
        out.push_str(&position_prefix(position));
        out.push_str(":\n");
    }
    if let Some(function) = &message.function {
        if let Some(position) = &message.location {
            out.push_str(&position.file);
            out.push_str(": ");
        }
        out.push_str("In function '");
        out.push_str(function);
        out.push_str("':\n");
    }
    if let Some(position) = &message.location {
        out.push_str(&position_prefix(position));
        out.push_str(": ");
    }
    out.push_str(&message.message);
    if let Some(option) = &message.option {
        out.push_str(" [");
        out.push_str(option);
        out.push(']');
    }
    for child in &message.children {
        out.push('\n');
        if let Some(position) = &child.location {
            out.push_str(&position_prefix(position));
            out.push_str(": ");
        }
        out.push_str("note: ");
        out.push_str(&child.message);
    }
    cap_raw(out)
}

/// Cuts raw compiler text to at most [`MAX_RAW_BYTES`] on a character
/// boundary, marking the cut with `…`.
fn cap_raw(mut text: String) -> String {
    if text.len() > MAX_RAW_BYTES {
        let mut end = MAX_RAW_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('…');
    }
    text
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

/// Whether a compiler path is the IDE init unit (in `ide_dir`, or its bare
/// file name).
fn is_ide_file(reported: &str, ide_dir: &Path) -> bool {
    let path = Path::new(reported);
    if path.is_absolute() {
        path.starts_with(ide_dir)
    } else {
        reported == ide::INIT_UNIT_FILE
    }
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

    use super::*;

    fn killed_compiler(out_of_memory: bool) -> StepRun {
        StepRun {
            captured: b2c_process::Captured {
                status: b2c_process::ExitStatus::Signaled(9),
                stdout: Vec::new(),
                stderr: b"g++: internal compiler error: Killed signal terminated program cc1plus\n".to_vec(),
                stdout_truncated: false,
                stderr_truncated: false,
                duration: std::time::Duration::from_secs(1),
                timed_out: false,
                cancelled: false,
                too_many_processes: false,
                out_of_memory,
            },
            mapped: Vec::new(),
        }
    }

    #[test]
    fn a_compiler_stopped_for_memory_is_not_a_crash() {
        // g++ reports a cc1plus killed by the memory limit as an "internal
        // compiler error"; that is a limit (C:limit), not a g++ bug.
        assert!(killed_compiler(false).crashed());
        assert!(!killed_compiler(true).crashed());
    }

    fn gen_dir() -> PathBuf {
        PathBuf::from(if cfg!(windows) {
            r"C:\cache\gen"
        } else {
            "/cache/gen"
        })
    }

    fn dirs() -> MapDirs {
        MapDirs {
            gen_dir: gen_dir(),
            ide_dir: gen_dir().with_file_name("ide"),
        }
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
                ranges: vec![
                    range(7, 5, 40, "b003"),
                    range(7, 18, 30, "b004"),
                    range(12, 1, 20, "b009"),
                ],
            }],
        }
    }

    fn at(file: &Path, line: u32, column: Option<u32>) -> SourcePos {
        SourcePos {
            file: file.to_string_lossy().into_owned(),
            line,
            column,
        }
    }

    fn message(origin: MessageOrigin, severity: MessageSeverity, column: Option<u32>) -> CompilerMessage {
        let mut message = CompilerMessage::new(origin, severity, "something went wrong");
        message.location = Some(at(&gen_dir().join("main.cpp"), 7, column));
        message
    }

    #[test]
    fn compiler_errors_land_on_the_innermost_block() {
        let parsed = ParsedOutput {
            messages: vec![message(MessageOrigin::Compiler, MessageSeverity::Error, Some(20))],
            truncated: false,
        };
        let diagnostics = map_messages(&parsed, &source_map(), &dirs());
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
        assert!(diagnostic.related.is_empty());
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
        let diagnostics = map_messages(&parsed, &source_map(), &dirs());
        let codes: Vec<&str> = diagnostics.iter().map(|d| d.code.0.as_str()).collect();
        assert_eq!(codes, ["C:-Wunused-variable", "C:link", "C:truncated"]);
        assert_eq!(diagnostics[0].severity, Severity::Warning);
        assert!(diagnostics[0].message.starts_with("g++ warns: "));
        assert_eq!(
            diagnostics[0].primary.block.as_ref().map(BlockId::as_str),
            Some("b003")
        );
        assert_eq!(diagnostics[1].source, DiagSource::Linker);
        assert!(diagnostics[1].message.starts_with("The linker reported"));
        assert_eq!(diagnostics[1].primary, Location::project());
        assert_eq!(diagnostics[2].severity, Severity::Info);
    }

    /// A place in a generated file outside every block (an include line, a
    /// helper) belongs to that file's module.
    #[test]
    fn unmapped_places_in_generated_files_attach_to_the_module() {
        let mut error = message(MessageOrigin::Compiler, MessageSeverity::Error, Some(1));
        error.location = Some(at(&gen_dir().join("main.cpp"), 2, Some(1)));
        let diagnostics = map_messages(
            &ParsedOutput {
                messages: vec![error],
                truncated: false,
            },
            &source_map(),
            &dirs(),
        );
        assert_eq!(
            diagnostics[0].primary,
            Location {
                module: Some(ModuleId::new("mod_main").unwrap()),
                block: None,
                part: Part::Whole,
            }
        );
        // A generated file the source map does not know stays at the project.
        let mut other = message(MessageOrigin::Compiler, MessageSeverity::Error, Some(1));
        other.location = Some(at(&gen_dir().join("other.cpp"), 2, Some(1)));
        let diagnostics = map_messages(
            &ParsedOutput {
                messages: vec![other],
                truncated: false,
            },
            &source_map(),
            &dirs(),
        );
        assert_eq!(diagnostics[0].primary, Location::project());
    }

    /// Include chains and notes that point into blocks become related
    /// locations; notes elsewhere (system headers) do not, and each place is
    /// listed once.
    #[test]
    fn include_chains_and_notes_become_related_locations() {
        let mut error = message(MessageOrigin::Compiler, MessageSeverity::Error, Some(20));
        error.included_from = vec![at(&gen_dir().join("main.cpp"), 12, Some(3))];
        let mut required = CompilerMessage::new(
            MessageOrigin::Compiler,
            MessageSeverity::Note,
            "required from here",
        );
        required.location = Some(at(&gen_dir().join("main.cpp"), 7, Some(6)));
        let mut system = CompilerMessage::new(MessageOrigin::Compiler, MessageSeverity::Note, "candidate");
        system.location = Some(SourcePos {
            file: String::from("/usr/include/c++/13/ostream"),
            line: 100,
            column: Some(5),
        });
        let duplicate = required.clone();
        let unplaced = CompilerMessage::new(MessageOrigin::Compiler, MessageSeverity::Note, "no place");
        error.children = vec![required, system, duplicate, unplaced];
        let diagnostics = map_messages(
            &ParsedOutput {
                messages: vec![error],
                truncated: false,
            },
            &source_map(),
            &dirs(),
        );
        let related: Vec<(Option<&str>, &str)> = diagnostics[0]
            .related
            .iter()
            .map(|related| {
                (
                    related.location.block.as_ref().map(BlockId::as_str),
                    related.message.as_str(),
                )
            })
            .collect();
        assert_eq!(
            related,
            [
                (Some("b009"), "Included from here."),
                (Some("b003"), "required from here"),
            ]
        );
        let raw = diagnostics[0].raw.as_deref().unwrap();
        assert!(raw.starts_with("In file included from "), "{raw}");
        assert!(raw.contains("note: required from here"), "{raw}");
        assert!(raw.contains("ostream:100:5: note: candidate"), "{raw}");
        assert!(raw.ends_with("note: no place"), "{raw}");
    }

    #[test]
    fn related_locations_are_bounded() {
        let mut error = message(MessageOrigin::Compiler, MessageSeverity::Error, Some(20));
        error.children = (1..=40)
            .map(|column| {
                let mut note = CompilerMessage::new(MessageOrigin::Compiler, MessageSeverity::Note, "note");
                note.location = Some(at(&gen_dir().join("main.cpp"), 7, Some(column)));
                note
            })
            .collect();
        // Columns 5–17 and 30–39 map to b003, 18–29 to b004: two places.
        let related = related_locations(&error, &source_map(), &gen_dir());
        assert_eq!(related.len(), 2);

        let many = SourceMap {
            version: 1,
            files: vec![FileMap {
                path: String::from("main.cpp"),
                ranges: (1..=40)
                    .map(|line| MappedRange {
                        start: Position { line, column: 1 },
                        end: Position { line, column: 9 },
                        module: ModuleId::new("mod_main").unwrap(),
                        block: BlockId::new(&format!("b{line:03}")).unwrap(),
                        part: Part::Whole,
                    })
                    .collect(),
            }],
        };
        error.children = (1..=40)
            .map(|line| {
                let mut note = CompilerMessage::new(MessageOrigin::Compiler, MessageSeverity::Note, "note");
                note.location = Some(at(&gen_dir().join("main.cpp"), line, Some(2)));
                note
            })
            .collect();
        assert_eq!(related_locations(&error, &many, &gen_dir()).len(), MAX_RELATED);
    }

    /// Messages about the IDE init unit are reported as a bug in Blocks2Cpp
    /// at the project, whatever their severity.
    #[test]
    fn messages_in_the_ide_unit_are_blocks2cpp_bugs() {
        let ide_file = dirs().ide_dir.join("b2c_ide_init.cpp");
        let mut warning = message(MessageOrigin::Compiler, MessageSeverity::Warning, Some(3));
        warning.location = Some(at(&ide_file, 7, Some(20)));
        warning.option = Some(String::from("-Wunused"));
        let mut error = message(MessageOrigin::Compiler, MessageSeverity::Error, Some(3));
        error.location = Some(SourcePos {
            file: String::from("b2c_ide_init.cpp"),
            line: 7,
            column: Some(20),
        });
        let diagnostics = map_messages(
            &ParsedOutput {
                messages: vec![warning, error],
                truncated: false,
            },
            &source_map(),
            &dirs(),
        );
        for diagnostic in &diagnostics {
            assert!(diagnostic.message.contains("bug in Blocks2Cpp"), "{diagnostic:?}");
            assert!(diagnostic.message.contains("helper code"), "{diagnostic:?}");
            assert_eq!(diagnostic.primary, Location::project());
        }
        assert_eq!(diagnostics[0].severity, Severity::Warning);
        assert_eq!(diagnostics[0].code.0, "C:-Wunused");
        assert_eq!(diagnostics[1].severity, Severity::Error);
        assert_eq!(diagnostics[1].code.0, "C:error");
        assert!(!is_ide_file(
            &gen_dir().join("main.cpp").to_string_lossy(),
            &dirs().ide_dir
        ));
        assert!(!is_ide_file("main.cpp", &dirs().ide_dir));
    }

    #[test]
    fn raw_text_is_capped_on_a_character_boundary() {
        let mut long = message(MessageOrigin::Compiler, MessageSeverity::Error, Some(1));
        long.children = (0..100)
            .map(|_| CompilerMessage::new(MessageOrigin::Compiler, MessageSeverity::Note, &"é".repeat(400)))
            .collect();
        let raw = raw_text(&long);
        assert!(raw.len() <= MAX_RAW_BYTES + '…'.len_utf8());
        assert!(raw.ends_with('…'));
        assert_eq!(cap_raw(String::from("short")), "short");
        let exact = "a".repeat(MAX_RAW_BYTES);
        assert_eq!(cap_raw(exact.clone()), exact);
    }

    #[test]
    fn functions_appear_in_the_raw_text() {
        let mut warning = message(MessageOrigin::Compiler, MessageSeverity::Warning, Some(9));
        warning.function = Some(String::from("int main()"));
        warning.option = Some(String::from("-Wunused-variable"));
        let raw = raw_text(&warning);
        assert!(raw.contains(": In function 'int main()':\n"), "{raw}");
        assert!(
            raw.ends_with(":7:9: something went wrong [-Wunused-variable]"),
            "{raw}"
        );
    }

    #[test]
    fn generated_file_names_are_recognised() {
        let gen_dir = gen_dir();
        let inside = gen_dir.join("main.cpp");
        assert_eq!(
            generated_file_name(inside.to_str().unwrap(), &gen_dir),
            Some(String::from("main.cpp"))
        );
        assert_eq!(
            generated_file_name("main.cpp", &gen_dir),
            Some(String::from("main.cpp"))
        );
        assert_eq!(
            generated_file_name("/usr/include/c++/13/iostream", &gen_dir),
            None
        );
        assert_eq!(generated_file_name("sub/main.cpp", &gen_dir), None);
    }

    #[test]
    fn the_worst_step_result_wins() {
        use StepResult::{Cancelled, Crashed, Failed, Succeeded};
        let all = [Succeeded, Failed, Crashed, Cancelled];
        for (i, first) in all.iter().enumerate() {
            for (j, second) in all.iter().enumerate() {
                assert_eq!(first.worst(*second), all[i.max(j)], "{first:?} {second:?}");
            }
        }
    }

    #[test]
    fn outputs_are_the_dash_o_arguments() {
        let command = CompilerCommand {
            program: PathBuf::from("/usr/bin/g++"),
            args: vec!["-c".into(), "a.cpp".into(), "-o".into(), "/b/out/a.o".into()],
            format: b2c_toolchain::probe::DiagnosticsFormat::Plain,
            sarif_files: Vec::new(),
        };
        assert_eq!(output_of(&command), Some(PathBuf::from("/b/out/a.o")));
        let no_output = CompilerCommand {
            args: vec!["-c".into(), "a.cpp".into(), "-o".into()],
            ..command
        };
        assert_eq!(output_of(&no_output), None);
    }

    fn document() -> Document {
        b2c_model::load(include_bytes!("../../../examples/hello_world.b2c")).unwrap()
    }

    #[test]
    fn program_names_avoid_device_names() {
        let mut document = document();
        assert_eq!(program_name(&document), document.modules[0].name);
        for (name, expected) in [
            ("con", "program"),
            ("nul", "program"),
            ("com1", "program"),
            ("lpt9", "program"),
            ("console", "console"),
            ("Bad Name", "program"),
            ("", "program"),
        ] {
            name.clone_into(&mut document.modules[0].name);
            assert_eq!(program_name(&document), expected, "{name}");
        }
        document.modules.clear();
        assert_eq!(program_name(&document), "program");
    }

    fn fake_toolchain() -> Toolchain {
        use b2c_toolchain::fingerprint::Fingerprint;
        use b2c_toolchain::probe::{
            Capabilities, CompilerKind, DiagnosticsFormat, Hardening, LibraryFeatures, PROBE_FORMAT,
            Sanitizers, Standards,
        };
        use b2c_toolchain::target::{GccVersion, Target};
        Toolchain {
            format: PROBE_FORMAT,
            fingerprint: Fingerprint {
                path: PathBuf::from("/usr/bin/g++"),
                size: 1,
                modified_ns: 2,
                sha256: "a".repeat(64),
            },
            kind: CompilerKind::Gcc,
            version: Some(GccVersion {
                major: 13,
                minor: 3,
                patch: 0,
            }),
            version_text: String::from("g++ 13.3.0"),
            target: Target::parse("x86_64-linux-gnu"),
            capabilities: Capabilities {
                cc1plus: None,
                hello_world: true,
                standards: Standards {
                    cpp17: Some("c++17".into()),
                    cpp20: Some("c++20".into()),
                    cpp23: Some("c++23".into()),
                    cpp26: None,
                },
                library: LibraryFeatures::default(),
                diagnostics: Some(DiagnosticsFormat::SarifFile),
                sanitizers: Sanitizers {
                    address_undefined: true,
                    undefined: true,
                    undefined_trap: true,
                    leak_detection: true,
                },
                hardening: Hardening {
                    fhardened: false,
                    fortify_source: true,
                    stack_protector_strong: true,
                    stack_clash_protection: true,
                    cf_protection: true,
                    pie: true,
                    relro_now: true,
                    noexecstack: true,
                    windows_aslr_dep: false,
                },
                static_link: false,
            },
            problems: Vec::new(),
        }
    }

    /// Every input of the folder name changes it; the rest of the document
    /// does not.
    #[test]
    fn config_keys_cover_indent_width_and_ide() {
        let toolchain = fake_toolchain();
        let document = document();
        let settings = Configuration::Debug.settings(&document);
        let key = |indent: u8, ide: bool| {
            config_key(Configuration::Debug, &toolchain, settings, &document, indent, ide)
        };
        let base = key(4, false);
        assert!(base.starts_with("debug-"), "{base}");
        assert_eq!(base.len(), "debug-".len() + 8);
        assert!(
            base["debug-".len()..]
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        assert_eq!(key(4, false), base);
        assert_ne!(key(2, false), base);
        assert_ne!(key(4, true), base);
        assert_ne!(key(2, true), key(4, true));
        let release = config_key(
            Configuration::Release,
            &toolchain,
            Configuration::Release.settings(&document),
            &document,
            4,
            false,
        );
        assert!(release.starts_with("release-"), "{release}");

        let mut other_toolchain = fake_toolchain();
        other_toolchain.fingerprint.sha256 = "b".repeat(64);
        assert_ne!(
            config_key(
                Configuration::Debug,
                &other_toolchain,
                settings,
                &document,
                4,
                false
            ),
            base
        );
        let mut renamed = document.clone();
        "Renamed".clone_into(&mut renamed.project.name);
        assert_eq!(
            config_key(Configuration::Debug, &toolchain, settings, &renamed, 4, false),
            base
        );
    }

    #[test]
    fn configurations_come_from_the_ipc_values() {
        assert_eq!(Configuration::from(BuildConfig::Debug), Configuration::Debug);
        assert_eq!(Configuration::from(BuildConfig::Release), Configuration::Release);
        assert_eq!(Configuration::Debug.name(), "debug");
        assert_eq!(Configuration::Release.name(), "release");
    }
}
