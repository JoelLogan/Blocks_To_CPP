//! Toolchains: `toolchain_list`, `toolchain_rescan`, `toolchain_add_dialog`,
//! `toolchain_select` and `toolchain_setup_info`
//! (`docs/spec/07-toolchain-build-run.md` §7.2–§7.3,
//! `docs/spec/04-user-interface.md` §4.6).
//!
//! The list is [`b2c_build::toolchains::ToolchainRegistry`]: loaded from
//! `toolchains.json` at startup without probing, so `toolchain_list` answers
//! at once, while discovery runs in the background and ends with a
//! `toolchainsUpdated` app event. Discovery never searches the build cache,
//! the process's current directory or the open projects' folders
//! ([`Backend::excluded_folders`]), with the folder rules of
//! [`b2c_toolchain::discovery::Exclusions`]. A compiler whose probe timed out
//! is probed once more; `toolchainsUpdated` is also sent when that starts and
//! when it ends, so the setup page can say "Checking g++ again…"
//! ([`Backend::watch_toolchain_rechecks`]).

use std::sync::{Arc, Weak};

use b2c_ipc::IpcError;
use b2c_ipc::dto::{
    AppEvent, Empty, ToolchainAddDialogResponse, ToolchainListResponse, ToolchainSelectRequest,
    ToolchainSetupInfo,
};

use b2c_toolchain::discovery::ExcludedFolder;

use crate::backend::{Backend, command_span};
use crate::errors::store_error;

// Commands take their request by value, as the adapter decodes it, so every
// command method has the same shape whether or not it keeps the request.
#[allow(clippy::needless_pass_by_value)]
impl Backend {
    /// The folders discovery never searches (`docs/spec/07-toolchain-build-run.md`
    /// §7.2): the build cache, the process's current directory (that folder
    /// only) and the folders of the open projects (each with its tree, except
    /// a project folder that is the home folder or a root).
    pub fn excluded_folders(&self) -> Vec<ExcludedFolder> {
        let mut excluded = vec![ExcludedFolder::cache(self.cache_root())];
        excluded.extend(self.cwd.clone().map(ExcludedFolder::current_dir));
        excluded.extend(self.projects.folders().into_iter().map(ExcludedFolder::project));
        excluded
    }

    /// Sends `toolchainsUpdated` whenever a compiler whose probe timed out
    /// starts or ends being probed once more, so the setup page can say so
    /// while it runs (07 §7.3). Called once when the backend starts.
    pub(crate) fn watch_toolchain_rechecks(self: &Arc<Self>) {
        let weak: Weak<Self> = Arc::downgrade(self);
        self.toolchains.on_progress(move || {
            if let Some(backend) = weak.upgrade() {
                backend.send_toolchains_updated();
            }
        });
    }

    /// Sends `toolchainsUpdated` with the current list.
    pub(crate) fn send_toolchains_updated(&self) {
        let list = self.toolchains.list(self.selected_toolchain().as_ref());
        tracing::debug!(
            toolchains = list.toolchains.len(),
            skipped = list.skipped.len(),
            rechecking = list.rechecking,
            "toolchain list updated"
        );
        self.events.send(AppEvent::ToolchainsUpdated {
            toolchains: list.toolchains,
            discovering: list.discovering,
            skipped: list.skipped,
            rechecking: list.rechecking,
        });
    }

    /// `toolchain_list`: the toolchains as known now, without probing;
    /// `discovering` says whether background discovery is still running.
    ///
    /// # Errors
    /// None today; the `Result` keeps the command shape.
    pub fn toolchain_list(&self) -> Result<ToolchainListResponse, IpcError> {
        let _span = command_span("toolchain_list");
        Ok(self.toolchains.list(self.selected_toolchain().as_ref()))
    }

    /// `toolchain_rescan`: discovers and probes again ("I installed it →
    /// Rescan"), blocking until done, and returns the new list.
    ///
    /// # Errors
    /// None today; the `Result` keeps the command shape.
    pub fn toolchain_rescan(&self) -> Result<ToolchainListResponse, IpcError> {
        let _span = command_span("toolchain_rescan");
        let excluded = self.excluded_folders();
        Ok(self
            .toolchains
            .rescan(&excluded, self.selected_toolchain().as_ref()))
    }

    /// The folders a compiler picked by hand may not lie in, because it would
    /// be run (probed) at once: the open projects' folders, where a
    /// downloaded project could ship its own "g++" (08 §8.5), with the same
    /// folder rules as discovery, and the build cache. The current directory
    /// is not among them, unlike for discovery: a compiler chosen by its full
    /// path does not depend on the current directory.
    fn manual_compiler_excluded(&self) -> Vec<ExcludedFolder> {
        let mut excluded = vec![ExcludedFolder::cache(self.cache_root())];
        excluded.extend(self.projects.folders().into_iter().map(ExcludedFolder::project));
        excluded
    }

    /// `toolchain_add_dialog`: the user picks a g++ executable in the native
    /// file dialog; it is checked (`B2C-T1002`: never `.bat` or `.cmd`, on
    /// Windows only `g++.exe`, never inside an open project's folder or the
    /// build cache), canonicalised, probed and kept as a manual toolchain,
    /// even when it fails its health checks (`usable: false`). Nothing a
    /// check refuses is ever run.
    ///
    /// # Errors
    /// [`IpcError::Busy`] while another dialog is open;
    /// [`IpcError::ToolchainRejected`] with the diagnostics when the file is
    /// not an acceptable g++ or cannot be probed (nothing is added).
    pub fn toolchain_add_dialog(&self) -> Result<ToolchainAddDialogResponse, IpcError> {
        let _span = command_span("toolchain_add_dialog");
        let picked = {
            let _dialog = self.dialog_gate.enter()?;
            self.dialogs.pick_compiler()
        };
        let Some(path) = picked else {
            return Ok(ToolchainAddDialogResponse::Cancelled);
        };
        let excluded = self.manual_compiler_excluded();
        let mut toolchain = self
            .toolchains
            .add_explicit(&path, &excluded)
            .map_err(|diagnostics| IpcError::ToolchainRejected {
                diagnostics: b2c_ipc::diag::convert_all(&diagnostics),
            })?;
        toolchain.selected = self.selected_toolchain().as_ref() == Some(&toolchain.id);
        Ok(ToolchainAddDialogResponse::Ok { toolchain })
    }

    /// `toolchain_select`: makes a toolchain of the list the default for
    /// builds (saved in the settings). It is checked again before each build;
    /// when it cannot be used, the build falls back to discovery order with
    /// the warning `B2C-T1022`.
    ///
    /// # Errors
    /// [`IpcError::UnknownToolchain`] for an ID that is not in the list;
    /// [`IpcError::Io`] when the settings cannot be saved.
    pub fn toolchain_select(&self, request: ToolchainSelectRequest) -> Result<Empty, IpcError> {
        let _span = command_span("toolchain_select");
        if self.toolchains.get(&request.toolchain_id).is_none() {
            return Err(IpcError::UnknownToolchain);
        }
        self.settings
            .set_selected_toolchain(Some(request.toolchain_id.as_str()))
            .map_err(|error| store_error("select a toolchain", &error))?;
        Ok(Empty {})
    }

    /// `toolchain_setup_info`: what the setup page needs to suggest an install
    /// command: the platform, whether no usable toolchain is known, and on
    /// Linux the distribution from `os-release` (read with a 64 KiB bound).
    ///
    /// # Errors
    /// None today; the `Result` keeps the command shape.
    pub fn toolchain_setup_info(&self) -> Result<ToolchainSetupInfo, IpcError> {
        let _span = command_span("toolchain_setup_info");
        Ok(self.toolchains.setup_info())
    }
}
