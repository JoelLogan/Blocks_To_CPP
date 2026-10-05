//! Building and running: `build_start`, `build_cancel`, `build_cache_clear`,
//! `run_start`, `run_input`, `run_resize`, `run_stop` and `run_ack`
//! (`docs/spec/02-architecture.md` §2.4.2–§2.4.3 and §2.5,
//! `docs/spec/07-toolchain-build-run.md` §7.5–§7.6, `docs/spec/08-security.md`
//! §8.3 and §8.7).
//!
//! The backend never relies on the UI. `build_start` checks trust before
//! anything else touches the disk or starts a process, and generates the C++
//! itself from the document (never from C++ text). `run_start` checks again,
//! in this order (07 §7.6.1): the build belongs to an open project
//! (`unknownBuild`), the project is trusted now (`restricted`), the build
//! succeeded (`buildNotSuccessful`), it is of the latest document the backend
//! received for the project and its program still matches its build manifest
//! (`staleBuild`), and the analyser reports no errors (`projectErrors`).

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use b2c_build::cache;
use b2c_build::toolchains::Chosen;
use b2c_build::{
    BuildJob, BuildRecord, FrontendOptions, PtySize, RunEnvOptions, RunSpec, StaleReason, ToolchainForBuild,
    run_environment, run_frontend, sandbox_dir,
};
use b2c_ipc::dto::{
    BuildCacheClearResponse, BuildCancelRequest, BuildEvent, BuildStartRequest, BuildStartResponse, Empty,
    RunAckRequest, RunEvent, RunInputRequest, RunResizeRequest, RunStartRequest, RunStartResponse,
    RunStopRequest,
};
use b2c_ipc::{ByteSink, EventSink, Handle, IpcError};
use b2c_model::{Document, WorkingDirectory};
use b2c_toolchain::target::Platform;

use crate::backend::{Backend, command_span};
use crate::errors::io_error;
use crate::projects::lock;

/// The toolchain for a build, from the registry's choice.
fn for_build(chosen: Chosen) -> ToolchainForBuild {
    match chosen {
        Chosen::Ready { toolchain, notes } => ToolchainForBuild::Ready { toolchain, notes },
        Chosen::Unavailable { diagnostics } => ToolchainForBuild::Unavailable { diagnostics },
    }
}

/// The analyser's errors for `document`, as the build would count them.
fn analyser_errors(document: &Document, indent_width: u8) -> usize {
    let text = b2c_model::to_canonical_json(document);
    let options = FrontendOptions {
        indent_width,
        ..FrontendOptions::default()
    };
    run_frontend(text.as_bytes(), &options)
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == b2c_ir::Severity::Error)
        .count()
}

// Commands take their request by value, as the adapter decodes it, so every
// command method has the same shape whether or not it keeps the request.
#[allow(clippy::needless_pass_by_value)]
impl Backend {
    /// The generator options from the settings: the same as the editor's
    /// preview (banner, inline helpers, the code style's indent width).
    fn frontend_options(&self) -> FrontendOptions {
        FrontendOptions {
            indent_width: self.settings.get().code_style.indent_width,
            ..FrontendOptions::default()
        }
    }

    /// `build_start`: builds the document for the project on its own thread
    /// and returns the build's ID at once; `sink` gets the progress,
    /// diagnostics and exactly one `finished` event, last. The document
    /// becomes the project's latest document (which the trust dialog lists
    /// and runs are checked against), also when the build is refused.
    ///
    /// A project in Restricted Mode is refused before anything else happens:
    /// no build folder is created and no process starts. Otherwise the
    /// project's running program is stopped first (Windows locks a running
    /// program's file), its active build is cancelled, and the toolchain is
    /// chosen: the selected one checked again, else discovery order, never a
    /// compiler inside the project's folder. Choosing can wait for a running
    /// discovery or probe a changed compiler, so this can take seconds.
    ///
    /// # Errors
    /// The document's errors (`payloadTooLarge`, `invalidDocument`,
    /// `newerFormat`) before anything else; [`IpcError::UnknownHandle`];
    /// [`IpcError::Restricted`]; [`IpcError::Internal`] after shutdown or when
    /// no build thread can start.
    pub fn build_start(
        &self,
        request: BuildStartRequest,
        sink: Arc<dyn EventSink<BuildEvent>>,
    ) -> Result<BuildStartResponse, IpcError> {
        let _span = command_span("build_start");
        let document = b2c_ipc::parse_document(&request.document)?;
        let entry_ref = self.projects.get(&request.handle)?;
        let project_dir = {
            let mut entry = lock(&entry_ref);
            entry.set_latest(document, request.document.len());
            if !entry.trust.is_trusted() {
                tracing::info!("build refused: the project is in Restricted Mode");
                return Err(IpcError::Restricted);
            }
            entry.folder().map(PathBuf::from)
        };
        if self.is_shut_down() {
            return Err(IpcError::Internal);
        }
        let key = request.handle.to_string();
        self.runs.stop_project(&key);
        let selected = self.selected_toolchain();
        let toolchain = for_build(self.toolchains.choose(selected.as_ref(), project_dir.as_deref()));
        let job = BuildJob {
            project_key: key,
            document: request.document.into_bytes(),
            configuration: request.config.into(),
            toolchain,
            frontend: self.frontend_options(),
            ide: true,
        };
        let build_id = self.builds.start(job, sink)?;
        tracing::debug!(%build_id, "build started");
        Ok(BuildStartResponse { build_id })
    }

    /// `build_cancel`: cancels a build, killing the compiler's process tree.
    /// Cancelling a finished build does nothing.
    ///
    /// # Errors
    /// [`IpcError::UnknownBuild`].
    pub fn build_cancel(&self, request: BuildCancelRequest) -> Result<Empty, IpcError> {
        let _span = command_span("build_cancel");
        self.builds.cancel(&request.build_id)?;
        Ok(Empty {})
    }

    /// `build_cache_clear`: deletes the build cache except the folders a build
    /// or a running program is using; the sandboxes and the toolchain list are
    /// kept (`docs/spec/07-toolchain-build-run.md` §7.5.1).
    ///
    /// # Errors
    /// [`IpcError::Io`] when the cache cannot be read.
    pub fn build_cache_clear(&self) -> Result<BuildCacheClearResponse, IpcError> {
        let _span = command_span("build_cache_clear");
        let report = cache::clear(self.cache_root())
            .map_err(|error| io_error("clear the build cache", self.cache_root(), &error))?;
        tracing::info!(
            freed_bytes = report.freed_bytes,
            skipped = report.skipped_in_use,
            "build cache cleared"
        );
        Ok(BuildCacheClearResponse {
            freed_bytes: report.freed_bytes,
            skipped_in_use: u32::try_from(report.skipped_in_use).unwrap_or(u32::MAX),
        })
    }

    /// The record of a finished build of an open project, with its handle.
    fn build_of_open_project(
        &self,
        request: &RunStartRequest,
    ) -> Result<(Handle, Arc<BuildRecord>), IpcError> {
        let record = self
            .builds
            .record(&request.build_id)
            .ok_or(IpcError::UnknownBuild)?;
        let handle = Handle::parse(&record.project_key).map_err(|_| IpcError::UnknownBuild)?;
        Ok((handle, record))
    }

    /// `run_start`: runs the program of a successful build in a
    /// pseudo-terminal of `runOptions`' size, after checking the preconditions
    /// again (see the module documentation). The arguments and the working
    /// directory come from the built document: the project's folder, or its
    /// sandbox folder in the cache for `workingDirectory: sandbox` and for a
    /// project that was never saved. The environment is the user's, without
    /// the IDE's internal variables, plus `TERM` and the sanitizer options.
    /// `output` gets the output batches and `events` the `started`, `skipped`
    /// and exactly one `exit` event, last. A program of the same project that
    /// is still running is stopped first.
    ///
    /// # Errors
    /// [`IpcError::UnknownBuild`], [`IpcError::Restricted`],
    /// [`IpcError::BuildNotSuccessful`], [`IpcError::StaleBuild`] and
    /// [`IpcError::ProjectErrors`] as above; [`IpcError::TooManySessions`]
    /// with 8 programs running; [`IpcError::Io`] when the program cannot
    /// start; [`IpcError::Internal`] after shutdown.
    pub fn run_start(
        &self,
        request: RunStartRequest,
        output: Arc<dyn ByteSink>,
        events: Arc<dyn EventSink<RunEvent>>,
    ) -> Result<RunStartResponse, IpcError> {
        let _span = command_span("run_start");
        let (handle, record) = self.build_of_open_project(&request)?;
        let entry_ref = self.projects.get(&handle).map_err(|_| IpcError::UnknownBuild)?;
        let (latest, project_path) = {
            let entry = lock(&entry_ref);
            if !entry.trust.is_trusted() {
                tracing::info!("run refused: the project is in Restricted Mode");
                return Err(IpcError::Restricted);
            }
            (Arc::clone(&entry.latest_document), entry.path.clone())
        };
        if !record.outcome.is_success() {
            return Err(IpcError::BuildNotSuccessful);
        }
        if record.project_hash != Some(b2c_model::content_hash(&latest)) {
            tracing::debug!("run refused: the build is not of the latest document");
            return Err(IpcError::StaleBuild);
        }
        let (Some(build_dir), Some(executable), Some(built)) =
            (&record.build_dir, &record.executable, &record.document)
        else {
            return Err(IpcError::BuildNotSuccessful);
        };
        // The program's claim on its build folder, so eviction and *Clear
        // build cache* leave it alone while it runs; taken before the program
        // is checked. A build of the same folder holds it exclusively.
        let hold = cache::hold_for_run(build_dir)
            .inspect_err(|error| tracing::debug!(%error, "the build folder is busy"))
            .ok();
        record.verify_executable().map_err(|reason| {
            tracing::info!(%reason, "run refused: the program does not match its build");
            match reason {
                StaleReason::NotBuilt => IpcError::BuildNotSuccessful,
                _ => IpcError::StaleBuild,
            }
        })?;
        let errors = analyser_errors(&latest, self.frontend_options().indent_width);
        if errors > 0 {
            return Err(IpcError::ProjectErrors {
                count: u32::try_from(errors).unwrap_or(u32::MAX),
            });
        }
        if self.is_shut_down() {
            return Err(IpcError::Internal);
        }
        let working_dir = match (built.project.run.working_directory, project_path) {
            (WorkingDirectory::Project, Some(path)) => {
                path.parent().map(PathBuf::from).ok_or(IpcError::Internal)?
            }
            _ => sandbox_dir(self.cache_root(), &built.project.id).map_err(|error| {
                tracing::debug!(%error, "cannot prepare the sandbox folder");
                IpcError::Io {
                    kind: b2c_ipc::IoKind::Other,
                }
            })?,
        };
        let env = run_environment(
            std::env::vars_os(),
            &RunEnvOptions {
                platform: Platform::host(),
                sanitizers: record.sanitizers,
                leak_detection: record.leak_detection,
            },
        );
        let spec = RunSpec {
            project_key: record.project_key.clone(),
            executable: executable.clone(),
            args: built.project.run.args.iter().map(OsString::from).collect(),
            working_dir,
            env,
            size: PtySize {
                cols: request.run_options.cols,
                rows: request.run_options.rows,
            },
            scrollback_lines: self.settings.get().console.scrollback_lines,
            ide_helpers: record.ide,
            prefer_pty: true,
            hold,
        };
        let run_id = self.runs.start(spec, output, events)?;
        tracing::debug!(%run_id, "program started");
        Ok(RunStartResponse { run_id })
    }

    /// `run_input`: sends bytes to the program's terminal.
    ///
    /// # Errors
    /// [`IpcError::PayloadTooLarge`] over 64 KiB, [`IpcError::InvalidRequest`]
    /// for text that is not strict base64, [`IpcError::UnknownRun`],
    /// [`IpcError::NotRunning`], [`IpcError::RateLimited`] (200 calls or 1 MiB
    /// a second).
    pub fn run_input(&self, request: RunInputRequest) -> Result<Empty, IpcError> {
        let _span = command_span("run_input");
        let bytes = request.bytes()?;
        self.runs.input(&request.run_id, &bytes)?;
        Ok(Empty {})
    }

    /// `run_resize`: the program's terminal changed size.
    ///
    /// # Errors
    /// [`IpcError::InvalidRequest`] outside 2–1000 columns and 1–1000 rows,
    /// [`IpcError::UnknownRun`], [`IpcError::NotRunning`], [`IpcError::Io`].
    pub fn run_resize(&self, request: RunResizeRequest) -> Result<Empty, IpcError> {
        let _span = command_span("run_resize");
        self.runs.resize(&request.run_id, request.cols, request.rows)?;
        Ok(Empty {})
    }

    /// `run_stop`: stops the program and its whole process tree; the `exit`
    /// event follows with the status `stopped`.
    ///
    /// # Errors
    /// [`IpcError::UnknownRun`], [`IpcError::NotRunning`].
    pub fn run_stop(&self, request: RunStopRequest) -> Result<Empty, IpcError> {
        let _span = command_span("run_stop");
        self.runs.stop(&request.run_id)?;
        Ok(Empty {})
    }

    /// `run_ack`: the console has written every output batch up to `seq`
    /// (flow control).
    ///
    /// # Errors
    /// [`IpcError::InvalidRequest`] for a batch that was never sent,
    /// [`IpcError::UnknownRun`].
    pub fn run_ack(&self, request: RunAckRequest) -> Result<Empty, IpcError> {
        let _span = command_span("run_ack");
        self.runs.ack(&request.run_id, request.seq)?;
        Ok(Empty {})
    }
}
