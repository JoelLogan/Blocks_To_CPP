//! What a build session leaves behind: its [`BuildRecord`]
//! (`docs/spec/07-toolchain-build-run.md` §7.5.4, §7.6.1).

use std::path::PathBuf;

use b2c_ipc::BuildId;
use b2c_model::Document;

use crate::manifest;

/// How a build ended (the `finished` event's outcome, 02 §2.5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordOutcome {
    /// The program was built.
    Built,
    /// Nothing changed since the last successful build; the cached program
    /// is current.
    UpToDate,
    /// The analyser or the compiler found errors in the project.
    ProjectErrors,
    /// The toolchain is missing or unusable.
    ToolchainProblem,
    /// The build was cancelled.
    Cancelled,
    /// An internal failure; the details are in the log.
    Failed,
}

impl RecordOutcome {
    /// Whether the build left a runnable program ([`Self::Built`] or
    /// [`Self::UpToDate`]).
    pub fn is_success(self) -> bool {
        matches!(self, Self::Built | Self::UpToDate)
    }

    /// The outcome's IPC spelling (`built`, `upToDate`, …), for the log.
    pub fn as_str(self) -> &'static str {
        b2c_ipc::dto::BuildOutcome::from(self).as_str()
    }
}

impl From<RecordOutcome> for b2c_ipc::dto::BuildOutcome {
    fn from(outcome: RecordOutcome) -> Self {
        match outcome {
            RecordOutcome::Built => Self::Built,
            RecordOutcome::UpToDate => Self::UpToDate,
            RecordOutcome::ProjectErrors => Self::ProjectErrors,
            RecordOutcome::ToolchainProblem => Self::ToolchainProblem,
            RecordOutcome::Cancelled => Self::Cancelled,
            RecordOutcome::Failed => Self::Failed,
        }
    }
}

/// Why a build's executable may no longer be run ([`BuildRecord::verify_executable`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StaleReason {
    /// The build did not succeed, so there is no executable.
    #[error("the build did not produce a program")]
    NotBuilt,
    /// The build folder has no valid manifest any more (a later build of
    /// the same folder is running or failed, or the cache was cleared).
    #[error("the build folder no longer records this build")]
    ManifestMissing,
    /// The manifest describes a different build (another project version,
    /// an IDE build instead of a command-line one, or another executable).
    #[error("the build folder records a different build")]
    ManifestMismatch,
    /// The executable is missing or cannot be read.
    #[error("the program is missing")]
    ExecutableMissing,
    /// The executable's size or SHA-256 differs from the manifest.
    #[error("the program was changed after it was built")]
    ExecutableChanged,
}

/// The record of a build, kept until its project closes
/// ([`super::BuildSessions::forget_project`]). `run_start` uses it to find
/// the program and to check that it still matches what was built.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildRecord {
    /// The build.
    pub build_id: BuildId,
    /// The project it belongs to (the caller's key, such as the handle).
    pub project_key: String,
    /// How it ended.
    pub outcome: RecordOutcome,
    /// The content hash of the document that was built (05 §5.11), when it
    /// loaded.
    pub project_hash: Option<[u8; 32]>,
    /// The build folder's `<config>-<hash8>`; empty when the build stopped
    /// before the toolchain was known.
    pub config_key: String,
    /// The build folder, once it was prepared.
    pub build_dir: Option<PathBuf>,
    /// The program, when the outcome is [`RecordOutcome::Built`] or
    /// [`RecordOutcome::UpToDate`].
    pub executable: Option<PathBuf>,
    /// The document that was built, with catalog defaults filled in (its run
    /// arguments and working directory are what `run_start` uses in M2).
    pub document: Option<Document>,
    /// Whether the program was built with AddressSanitizer or
    /// UndefinedBehaviorSanitizer (their options are set when it runs).
    pub sanitizers: bool,
    /// Whether the toolchain's leak detection works (`detect_leaks=0`
    /// otherwise, `B2C-T1021`).
    pub leak_detection: bool,
    /// The toolchain's `bin` folder when the program is linked dynamically
    /// against the compiler's runtime DLLs: only on Windows, when static
    /// linking does not work with the toolchain (`B2C-T1013`). A run puts
    /// it first on `PATH` (`RunEnvOptions::toolchain_bin`, 07 §7.6.2).
    /// `None` for a statically linked program, on Linux, and when the build
    /// stopped before the toolchain was known.
    pub toolchain_bin: Option<PathBuf>,
    /// Whether the program contains the IDE init unit.
    pub ide: bool,
}

impl BuildRecord {
    /// Checks that the program can still be run as this build: the build
    /// succeeded, its folder's manifest still records this build (the same
    /// project hash, IDE flag and executable name), and the executable still
    /// has the recorded size and SHA-256 (07 §7.6.1: `staleBuild`
    /// otherwise). The manifest is read again and the executable hashed
    /// again on every call.
    ///
    /// # Errors
    /// The [`StaleReason`] when the program must not be run.
    pub fn verify_executable(&self) -> Result<(), StaleReason> {
        let (true, Some(dir), Some(executable), Some(project_hash)) = (
            self.outcome.is_success(),
            self.build_dir.as_ref(),
            self.executable.as_ref(),
            self.project_hash.as_ref(),
        ) else {
            return Err(StaleReason::NotBuilt);
        };
        let recorded = manifest::read(dir).ok_or(StaleReason::ManifestMissing)?;
        let name_matches = executable
            .file_name()
            .is_some_and(|name| name.to_string_lossy() == recorded.executable.name.as_str());
        if recorded.project_hash != b2c_model::hex(project_hash) || recorded.ide != self.ide || !name_matches
        {
            return Err(StaleReason::ManifestMismatch);
        }
        let digest = manifest::digest_file(executable).map_err(|_| StaleReason::ExecutableMissing)?;
        if recorded.executable.matches(&digest) {
            Ok(())
        } else {
            Err(StaleReason::ExecutableChanged)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_map_to_the_ipc_values() {
        let all = [
            (RecordOutcome::Built, "built", true),
            (RecordOutcome::UpToDate, "upToDate", true),
            (RecordOutcome::ProjectErrors, "projectErrors", false),
            (RecordOutcome::ToolchainProblem, "toolchainProblem", false),
            (RecordOutcome::Cancelled, "cancelled", false),
            (RecordOutcome::Failed, "failed", false),
        ];
        for (outcome, text, success) in all {
            assert_eq!(outcome.as_str(), text);
            assert_eq!(b2c_ipc::dto::BuildOutcome::from(outcome).as_str(), text);
            assert_eq!(outcome.is_success(), success, "{text}");
        }
    }

    fn record() -> BuildRecord {
        BuildRecord {
            build_id: BuildId::example(),
            project_key: String::from("ph_x"),
            outcome: RecordOutcome::Built,
            project_hash: Some([1; 32]),
            config_key: String::from("debug-00000000"),
            build_dir: None,
            executable: None,
            document: None,
            sanitizers: false,
            leak_detection: false,
            toolchain_bin: None,
            ide: true,
        }
    }

    #[test]
    fn records_without_a_program_are_not_built() {
        assert_eq!(record().verify_executable(), Err(StaleReason::NotBuilt));
        let folder = tempfile::tempdir().unwrap();
        let mut failed = record();
        failed.outcome = RecordOutcome::Failed;
        failed.build_dir = Some(folder.path().to_path_buf());
        failed.executable = Some(folder.path().join("main"));
        assert_eq!(failed.verify_executable(), Err(StaleReason::NotBuilt));
        let mut built = failed.clone();
        built.outcome = RecordOutcome::UpToDate;
        assert_eq!(built.verify_executable(), Err(StaleReason::ManifestMissing));
        built.project_hash = None;
        assert_eq!(built.verify_executable(), Err(StaleReason::NotBuilt));
    }
}
