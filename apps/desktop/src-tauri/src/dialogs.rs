//! The native dialogs the backend raises (`docs/spec/02-architecture.md`
//! §2.5.1, `docs/spec/08-security.md` §8.3 and §8.8): open project, save
//! project as, choose g++, and the trust dialog.
//!
//! They use `tauri-plugin-dialog`'s Rust API only, except the Windows trust
//! dialog, which uses the plugin's dialog library (`rfd`) directly so that
//! its default button is **Stay in Restricted Mode**. The plugin is
//! registered from Rust and the capability grants the webview none of its
//! commands, so only the backend can show a dialog, and the webview can
//! neither fake nor answer one. On Linux the dialogs are GTK 3 dialogs (never
//! the XDG portal, so no D-Bus); on Windows they are the system's common
//! dialogs and Task Dialogs.
//!
//! *Choose g++* starts in a system folder with no file chosen, never in the
//! folder used last (usually the open project's own).
//!
//! Every method blocks the calling command thread (never the main thread)
//! until the user answers: the plugin shows the dialog on the main thread and
//! a channel brings the answer back. When the dialog cannot be shown (the
//! plugin is missing, the event loop has ended), the answer is "cancelled"
//! (or Stay in Restricted Mode), never a panic.
//!
//! The trust dialog's text comes from [`trust_dialog_text`], built by the
//! backend from its own parsed copy of the project.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{SyncSender, sync_channel};

use b2c_app::{Dialogs, TrustChoice, TrustPrompt, display_text, trust_dialog_text};
use tauri::{AppHandle, Manager as _, Runtime};
use tauri_plugin_dialog::{
    Dialog, FileDialogBuilder, FilePath, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};

use crate::window::MAIN_WINDOW;

/// The extension of project files.
pub(crate) const PROJECT_EXTENSION: &str = "b2c";

/// The name of the file filter for project files.
const PROJECT_FILTER: &str = "Blocks2Cpp project";

/// The most characters of a file name shown in the "replace?" question.
const MAX_SHOWN_NAME_CHARS: usize = 120;

/// The native dialogs, shown through `tauri-plugin-dialog` with the editor
/// window as their parent.
pub(crate) struct NativeDialogs<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> NativeDialogs<R> {
    /// Dialogs of the app `app`, which must have the dialog plugin registered
    /// ([`crate::configure`] does).
    pub(crate) fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }

    /// The plugin's dialog API, or `None` (logged) when the plugin is not
    /// registered.
    fn plugin(&self) -> Option<Dialog<R>> {
        let dialog = self
            .app
            .try_state::<Dialog<R>>()
            .map(|state| state.inner().clone());
        if dialog.is_none() {
            tracing::error!("the dialog plugin is not registered; the dialog counts as cancelled");
        }
        dialog
    }

    /// A file dialog titled `title`, in front of the editor window.
    fn file_dialog(&self, title: &str) -> Option<FileDialogBuilder<R>> {
        let builder = self.plugin()?.file().set_title(title);
        Some(match self.app.get_webview_window(MAIN_WINDOW) {
            Some(window) => builder.set_parent(&window),
            None => builder,
        })
    }

    /// Asks a yes/no question with two custom buttons; `true` for the first.
    fn confirm(&self, title: &str, message: &str, yes: &str, no: &str) -> bool {
        let Some(dialog) = self.plugin() else {
            return false;
        };
        let mut builder = dialog
            .message(message)
            .title(title)
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                yes.to_owned(),
                no.to_owned(),
            ));
        if let Some(window) = self.app.get_webview_window(MAIN_WINDOW) {
            builder = builder.parent(&window);
        }
        let answer = wait(|reply| builder.show_with_result(move |result| send(&reply, result)));
        match answer {
            Some(MessageDialogResult::Custom(label)) => label == yes,
            Some(MessageDialogResult::Ok | MessageDialogResult::Yes) => true,
            _ => false,
        }
    }
}

impl<R: Runtime> Dialogs for NativeDialogs<R> {
    fn open_project(&self) -> Option<PathBuf> {
        let builder = self
            .file_dialog("Open project")?
            .add_filter(PROJECT_FILTER, &[PROJECT_EXTENSION]);
        let picked = wait(|reply| builder.pick_file(move |path| send(&reply, path))).flatten()?;
        local_path(picked)
    }

    fn save_project_as(&self, suggested_file_name: &str) -> Option<PathBuf> {
        let builder = self
            .file_dialog("Save project as")?
            .set_file_name(suggested_file_name)
            .add_filter(PROJECT_FILTER, &[PROJECT_EXTENSION]);
        let picked = wait(|reply| builder.save_file(move |path| send(&reply, path))).flatten()?;
        let (path, added) = with_project_extension(local_path(picked)?);
        // The dialog asked about replacing the name the user typed; when we
        // added the extension, the file we write may be another one.
        if added && path.symlink_metadata().is_ok() {
            let shown = path
                .file_name()
                .map(|name| display_text(&name.to_string_lossy(), MAX_SHOWN_NAME_CHARS))
                .unwrap_or_default();
            let replace = self.confirm(
                "Replace file?",
                &format!("A file named “{shown}” already exists. Replacing it overwrites what it contains."),
                "Replace",
                "Cancel",
            );
            if !replace {
                return None;
            }
        }
        Some(path)
    }

    fn pick_compiler(&self) -> Option<PathBuf> {
        // No file name is filled in, and the dialog starts in a folder where
        // compilers are installed, never in the one used last, which is
        // usually the folder a project was just opened from: a "g++" that a
        // downloaded project ships must not be one Enter away
        // (`docs/spec/08-security.md` §8.5). The backend refuses a compiler
        // inside an open project's folder anyway, before running it.
        let mut builder = self.file_dialog("Choose g++")?;
        if let Some(folder) = compiler_start_folder() {
            builder = builder.set_directory(folder);
        }
        // The common dialog filters by extension only, so on Windows it shows
        // programs; the backend refuses anything that is not a usable g++
        // (B2C-T1002).
        if cfg!(windows) {
            builder = builder.add_filter("g++ (g++.exe)", &["exe"]);
        }
        let picked = wait(|reply| builder.pick_file(move |path| send(&reply, path))).flatten()?;
        local_path(picked)
    }

    fn confirm_trust(&self, prompt: &TrustPrompt) -> TrustChoice {
        let (title, body) = trust_dialog_text(prompt);
        #[cfg(windows)]
        {
            self.windows_trust_dialog(&title, &body)
        }
        #[cfg(not(windows))]
        {
            self.plugin_trust_dialog(title, body)
        }
    }
}

impl<R: Runtime> NativeDialogs<R> {
    /// The trust dialog through the plugin (Linux: a GTK 3 message dialog).
    /// Its buttons are [`TrustChoice::LABELS`] in order; see
    /// [`trust_choice`] for how its answers are read.
    #[cfg(not(windows))]
    fn plugin_trust_dialog(&self, title: String, body: String) -> TrustChoice {
        let Some(dialog) = self.plugin() else {
            return TrustChoice::StayRestricted;
        };
        let [project, folder, restricted] = TrustChoice::LABELS.map(|(_, label)| label.to_owned());
        let mut builder = dialog
            .message(body)
            .title(title)
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::YesNoCancelCustom(
                project, folder, restricted,
            ));
        if let Some(window) = self.app.get_webview_window(MAIN_WINDOW) {
            builder = builder.parent(&window);
        }
        let answer = wait(|reply| builder.show_with_result(move |result| send(&reply, result)));
        answer.map_or(TrustChoice::StayRestricted, |result| trust_choice(&result))
    }

    /// Windows: the trust dialog through the plugin's own dialog library
    /// (`rfd`) directly, the fallback `docs/spec/08-security.md` §8.3.1
    /// allows. The Task Dialog makes its first button the default, which
    /// Enter (also a held or repeated one) presses, so the first button is
    /// **Stay in Restricted Mode**; Escape and closing the dialog stay
    /// restricted too. The plugin cannot show that order: it reports Escape
    /// as the third button, which would then be a trust choice.
    ///
    /// The dialog runs on this command thread, modal to the editor window,
    /// as the plugin runs its own on a thread of its own.
    #[cfg(windows)]
    fn windows_trust_dialog(&self, title: &str, body: &str) -> TrustChoice {
        let [project, folder, restricted] = TrustChoice::LABELS.map(|(_, label)| label.to_owned());
        let mut dialog = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title(title)
            .set_description(body)
            .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                restricted, project, folder,
            ));
        if let Some(window) = self.app.get_webview_window(MAIN_WINDOW) {
            dialog = dialog.set_parent(&window);
        }
        windows_trust_choice(&dialog.show().into())
    }
}

/// Shows a dialog with `show`, which must eventually send the answer on the
/// sender it is given (or drop it), and waits for that answer. `None` when
/// the dialog could not be shown or the answer never came.
fn wait<T>(show: impl FnOnce(SyncSender<T>)) -> Option<T> {
    let (sender, receiver) = sync_channel(1);
    show(sender);
    receiver.recv().ok()
}

/// Sends a dialog's answer; nobody is waiting only when the command thread
/// already gave up, so a failure is ignored.
fn send<T>(reply: &SyncSender<T>, answer: T) {
    let _ = reply.send(answer);
}

/// The local path of a picked file (a `file:` URL becomes a path; other URLs,
/// which only mobile systems return, count as cancelled).
fn local_path(picked: FilePath) -> Option<PathBuf> {
    match picked.into_path() {
        Ok(path) => Some(path),
        Err(error) => {
            tracing::warn!(%error, "the dialog returned a location that is not a local file");
            None
        }
    }
}

/// The file to save to: `picked` as chosen when it has an extension,
/// otherwise with `.b2c` added; the flag says whether it was added.
pub(crate) fn with_project_extension(picked: PathBuf) -> (PathBuf, bool) {
    if Path::new(&picked).extension().is_some() || picked.file_name().is_none() {
        return (picked, false);
    }
    let mut name = picked.into_os_string();
    name.push(".");
    name.push(PROJECT_EXTENSION);
    (PathBuf::from(name), true)
}

/// Where the *Choose g++* dialog starts: on Windows the system drive's root
/// (where MSYS2, TDM-GCC, `MinGW` and Strawberry Perl install their
/// compilers), elsewhere `/usr/bin`. `None` when that folder does not exist;
/// the dialog then starts where the system decides, still with no file
/// chosen.
pub(crate) fn compiler_start_folder() -> Option<PathBuf> {
    let folder = if cfg!(windows) {
        let mut root = std::env::var_os("SystemDrive").unwrap_or_else(|| "C:".into());
        root.push("\\");
        PathBuf::from(root)
    } else {
        PathBuf::from("/usr/bin")
    };
    (folder.is_absolute() && folder.is_dir()).then_some(folder)
}

/// The choice whose button has `label` (compared exactly), if any.
fn trust_button(label: &str) -> Option<TrustChoice> {
    TrustChoice::LABELS
        .iter()
        .find(|(_, shown)| *shown == label)
        .map(|(choice, _)| *choice)
}

/// The trust choice for the button the user pressed in the plugin's dialog
/// (Linux). The buttons are, in order, [`TrustChoice::LABELS`]; the GTK
/// backend reports them as Yes, No and Cancel rather than by label, and
/// Escape or closing the dialog as Cancel. Cancel, or anything unexpected,
/// means Stay in Restricted Mode.
#[cfg(any(not(windows), test))]
pub(crate) fn trust_choice(result: &MessageDialogResult) -> TrustChoice {
    match result {
        MessageDialogResult::Yes => TrustChoice::TrustProject,
        MessageDialogResult::No => TrustChoice::TrustFolder,
        MessageDialogResult::Custom(label) => trust_button(label).unwrap_or(TrustChoice::StayRestricted),
        MessageDialogResult::Ok | MessageDialogResult::Cancel => TrustChoice::StayRestricted,
    }
}

/// The trust choice for the button the user pressed in the Windows dialog,
/// whose buttons are Stay in Restricted Mode, Trust this project and Trust
/// everything in this folder, in that order. The Task Dialog reports each by
/// its label; only a trust button's own label grants trust. Escape, closing
/// the dialog (Cancel) and anything else mean Stay in Restricted Mode.
#[cfg(any(windows, test))]
pub(crate) fn windows_trust_choice(result: &MessageDialogResult) -> TrustChoice {
    match result {
        MessageDialogResult::Custom(label) => trust_button(label).unwrap_or(TrustChoice::StayRestricted),
        MessageDialogResult::Yes
        | MessageDialogResult::No
        | MessageDialogResult::Ok
        | MessageDialogResult::Cancel => TrustChoice::StayRestricted,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn each_trust_button_maps_to_its_choice() {
        for (choice, label) in TrustChoice::LABELS {
            assert_eq!(
                trust_choice(&MessageDialogResult::Custom(label.to_owned())),
                choice
            );
        }
        assert_eq!(trust_choice(&MessageDialogResult::Yes), TrustChoice::TrustProject);
        assert_eq!(trust_choice(&MessageDialogResult::No), TrustChoice::TrustFolder);
    }

    #[test]
    fn closing_or_anything_unexpected_stays_restricted() {
        for result in [
            MessageDialogResult::Cancel,
            MessageDialogResult::Ok,
            MessageDialogResult::Custom(String::new()),
            MessageDialogResult::Custom(String::from("Trust this project ")),
            MessageDialogResult::Custom(String::from("trust this project")),
        ] {
            assert_eq!(trust_choice(&result), TrustChoice::StayRestricted, "{result:?}");
            assert_eq!(
                windows_trust_choice(&result),
                TrustChoice::StayRestricted,
                "{result:?}"
            );
        }
    }

    /// The Windows dialog's buttons are Stay in Restricted Mode (the default
    /// button), Trust this project and Trust everything in this folder. Only
    /// a trust button's own label grants trust: Escape and closing (Cancel),
    /// and the plain Yes, No and Ok, never do.
    #[test]
    fn on_windows_only_a_trust_buttons_label_grants_trust() {
        for (choice, label) in TrustChoice::LABELS {
            assert_eq!(
                windows_trust_choice(&MessageDialogResult::Custom(label.to_owned())),
                choice
            );
        }
        for result in [
            MessageDialogResult::Yes,
            MessageDialogResult::No,
            MessageDialogResult::Ok,
            MessageDialogResult::Cancel,
        ] {
            assert_eq!(
                windows_trust_choice(&result),
                TrustChoice::StayRestricted,
                "{result:?}"
            );
        }
    }

    #[test]
    fn the_compiler_dialog_starts_in_a_system_folder() {
        let folder = compiler_start_folder();
        if cfg!(windows) {
            let folder = folder.unwrap();
            assert!(
                folder.is_dir() && folder.parent().is_none(),
                "{}",
                folder.display()
            );
        } else {
            assert_eq!(folder.as_deref(), Some(Path::new("/usr/bin")));
        }
    }

    #[test]
    fn the_extension_is_added_only_when_none_was_typed() {
        let (path, added) = with_project_extension(PathBuf::from("/home/ada/game"));
        assert_eq!((path.as_path(), added), (Path::new("/home/ada/game.b2c"), true));
        let (path, added) = with_project_extension(PathBuf::from("/home/ada/game.b2c"));
        assert_eq!((path.as_path(), added), (Path::new("/home/ada/game.b2c"), false));
        let (path, added) = with_project_extension(PathBuf::from("/home/ada/game.v2"));
        assert_eq!((path.as_path(), added), (Path::new("/home/ada/game.v2"), false));
        let (path, added) = with_project_extension(PathBuf::from("/home/ada/My Game"));
        assert_eq!(
            (path.as_path(), added),
            (Path::new("/home/ada/My Game.b2c"), true)
        );
        let (path, added) = with_project_extension(PathBuf::from("/"));
        assert_eq!((path.as_path(), added), (Path::new("/"), false));
    }

    #[test]
    fn waiting_for_a_dialog_that_never_answers_is_a_cancel() {
        assert_eq!(wait::<u8>(drop), None);
        assert_eq!(wait(|reply| send(&reply, 7_u8)), Some(7));
        // An answer that comes from another thread, as the plugin's does.
        let answer = wait(|reply| {
            std::thread::spawn(move || send(&reply, String::from("answer")));
        });
        assert_eq!(answer.as_deref(), Some("answer"));
    }
}
