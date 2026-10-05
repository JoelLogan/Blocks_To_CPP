//! Native dialogs (`docs/spec/02-architecture.md` §2.5.1 "Dialogs",
//! `docs/spec/08-security.md` §8.3).
//!
//! Every path the backend acts on comes from one of these dialogs, from the
//! recent list or from the backend's own folders: no request carries a path.
//! The desktop adapter implements [`Dialogs`] with the operating system's
//! dialogs (and, in end-to-end builds only, with a script); tests use a fake.
//!
//! The trust dialog is the only way to trust a project. Its text is built
//! here ([`trust_dialog_text`]) from the backend's own parsed document, never
//! from webview text. Anything taken from the project (its name, its library
//! names) is untrusted: [`display_text`] makes invisible and reordering
//! characters visible and cuts long text, so a project cannot disguise what
//! the dialog says.
//!
//! One native dialog is open at a time ([`DialogGate`]); a second request
//! gets `busy`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use b2c_ipc::IpcError;

use crate::limits::{MAX_DIALOG_LIBRARIES, MAX_DIALOG_TEXT_CHARS, MAX_SUGGESTED_STEM_CHARS};

/// The native dialogs the backend raises. Every method blocks until the user
/// answers; the adapter makes them callable from the command threads (not
/// only the main thread). A dialog the user cancels returns `None` (or
/// [`TrustChoice::StayRestricted`]).
pub trait Dialogs: Send + Sync {
    /// "Open project": a `*.b2c` file to open. The path need not be canonical;
    /// the backend canonicalises it.
    fn open_project(&self) -> Option<PathBuf>;

    /// "Save project as", starting with `suggested_file_name` (for example
    /// `Guessing Game.b2c`). Returns the file to write, with its extension (an
    /// implementation adds `.b2c` when the user typed none). It may not exist
    /// yet; when it does, the dialog has asked whether to replace it.
    fn save_project_as(&self, suggested_file_name: &str) -> Option<PathBuf>;

    /// "Choose g++ manually…": the compiler executable to add.
    fn pick_compiler(&self) -> Option<PathBuf>;

    /// The trust dialog (see [`trust_dialog_text`] for its text). Closing it
    /// counts as [`TrustChoice::StayRestricted`].
    fn confirm_trust(&self, prompt: &TrustPrompt) -> TrustChoice;
}

/// What the trust dialog shows (`docs/spec/08-security.md` §8.3): the
/// security summary of the latest document the backend received for the
/// project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustPrompt {
    /// The project's name, already made safe to show ([`display_text`]).
    pub project_name: String,
    /// The folder of the project file, for display (made safe to show); empty
    /// for a project that was never saved.
    pub folder_display: String,
    /// How many Raw C++ blocks the project has.
    pub raw_cpp_blocks: usize,
    /// The library requirements, made safe to show: at most
    /// [`MAX_DIALOG_LIBRARIES`] names, followed by one entry `and N more`
    /// when the project has more.
    pub libraries: Vec<String>,
    /// How many file-system blocks the project has.
    pub file_system_blocks: usize,
    /// Whether the file came from the Internet (Mark of the Web), which adds
    /// a stronger warning.
    pub mark_of_the_web: bool,
}

/// The user's answer to the trust dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrustChoice {
    /// Trust this project file (recorded with its security hash).
    TrustProject,
    /// Trust every project in the project file's folder and below it.
    TrustFolder,
    /// Keep the project in Restricted Mode (also when the dialog is closed).
    StayRestricted,
}

impl TrustChoice {
    /// The button labels, in the order the dialog shows them.
    pub const LABELS: [(Self, &'static str); 3] = [
        (Self::TrustProject, "Trust this project"),
        (Self::TrustFolder, "Trust everything in this folder"),
        (Self::StayRestricted, "Stay in Restricted Mode"),
    ];

    /// The button label of this choice.
    pub fn label(self) -> &'static str {
        match self {
            Self::TrustProject => "Trust this project",
            Self::TrustFolder => "Trust everything in this folder",
            Self::StayRestricted => "Stay in Restricted Mode",
        }
    }
}

/// The sentence every trust dialog contains (`docs/spec/08-security.md` §8.3).
pub const TRUST_WARNING: &str = "Running this project lets it do anything a program on your computer can do.";

/// The stronger warning for files with the Mark of the Web.
pub const INTERNET_WARNING: &str = "This file was downloaded from the Internet. Projects from the Internet are \
     a common way to spread harmful programs: trust it only if you know who made it and where it came from.";

/// The English title and body of the trust dialog for `prompt`
/// (`docs/spec/08-security.md` §8.3). The body explains what trusting means,
/// lists what the project contains that can reach beyond the program (Raw
/// C++, libraries, file-system blocks), adds [`INTERNET_WARNING`] for a file
/// with the Mark of the Web, and names the three choices.
pub fn trust_dialog_text(prompt: &TrustPrompt) -> (String, String) {
    let title = String::from("Trust this project?");
    let mut body = String::new();
    if prompt.mark_of_the_web {
        body.push_str("Warning: ");
        body.push_str(INTERNET_WARNING);
        body.push_str("\n\n");
    }
    body.push_str(TRUST_WARNING);
    body.push_str(
        " It can read, change or delete your files and use the network. Only trust projects from \
         people you trust.\n\n",
    );
    let _ = writeln!(body, "Project: {}", prompt.project_name);
    if prompt.folder_display.is_empty() {
        body.push_str("Folder: not saved yet\n");
    } else {
        let _ = writeln!(body, "Folder: {}", prompt.folder_display);
    }
    body.push_str("\nThis project contains:\n");
    let _ = writeln!(body, "• Raw C++ blocks: {}", prompt.raw_cpp_blocks);
    if prompt.libraries.is_empty() {
        body.push_str("• Libraries: none\n");
    } else {
        let _ = writeln!(body, "• Libraries: {}", prompt.libraries.join(", "));
    }
    let _ = writeln!(body, "• File-system blocks: {}", prompt.file_system_blocks);
    let _ = write!(
        body,
        "\n{}: build and run this project.\n{}: build and run every project in this folder and \
         below it.\n{}: keep editing and viewing the C++ without building or running it.",
        TrustChoice::TrustProject.label(),
        TrustChoice::TrustFolder.label(),
        TrustChoice::StayRestricted.label(),
    );
    (title, body)
}

/// `text` made safe to show in a native dialog: control characters,
/// invisible and reordering characters (bidi controls, zero-width
/// characters, line separators) become visible placeholders such as
/// `⟨U+202E⟩`, and text longer than `max_chars` is cut and ends with `…`.
pub fn display_text(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (count, c) in text.chars().enumerate() {
        if count >= max_chars {
            out.push('…');
            return out;
        }
        if c.is_control() || b2c_ir::text::is_invisible(c) {
            let _ = write!(out, "⟨U+{:04X}⟩", u32::from(c));
        } else {
            out.push(c);
        }
    }
    out
}

/// The file name the save dialog suggests: the current file's name when the
/// project has one, otherwise the project's name made into a safe file name
/// (letters, digits, spaces, `-`, `_` and `.`; at most
/// [`MAX_SUGGESTED_STEM_CHARS`] characters) with `.b2c`, or `project.b2c`.
pub fn suggested_file_name(current: Option<&Path>, project_name: &str) -> String {
    if let Some(name) = current.and_then(Path::file_name).and_then(|name| name.to_str()) {
        let safe = display_text(name, MAX_DIALOG_TEXT_CHARS);
        if safe == name {
            return safe;
        }
    }
    let mut stem: String = project_name
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
        .filter(|&c| !b2c_ir::text::is_invisible(c))
        .take(MAX_SUGGESTED_STEM_CHARS)
        .collect();
    // No leading or trailing dots or spaces (Windows drops them; a leading
    // dot hides the file on Linux).
    let trimmed = stem.trim_matches(|c| c == '.' || c == ' ');
    stem = trimmed.to_owned();
    if stem.is_empty() || is_device_name(&stem) {
        stem = String::from("project");
    }
    format!("{stem}.b2c")
}

/// Whether `stem` is a Windows device name (`CON`, `NUL`, `COM1`, …), which
/// cannot be a file name there.
fn is_device_name(stem: &str) -> bool {
    let upper = stem
        .split('.')
        .next()
        .unwrap_or(stem)
        .trim_end()
        .to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            upper
                .strip_prefix(prefix)
                .is_some_and(|n| n.len() == 1 && n.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// The library list of a [`TrustPrompt`]: the first [`MAX_DIALOG_LIBRARIES`]
/// names made safe to show, then `and N more` for the rest.
pub(crate) fn dialog_libraries(libraries: &[String]) -> Vec<String> {
    let mut shown: Vec<String> = libraries
        .iter()
        .take(MAX_DIALOG_LIBRARIES)
        .map(|name| display_text(name, MAX_DIALOG_TEXT_CHARS))
        .collect();
    let more = libraries.len().saturating_sub(MAX_DIALOG_LIBRARIES);
    if more > 0 {
        shown.push(format!("and {more} more"));
    }
    shown
}

/// Allows one native dialog at a time.
#[derive(Debug, Default)]
pub(crate) struct DialogGate {
    open: AtomicBool,
}

impl DialogGate {
    /// Claims the right to show a dialog until the guard is dropped.
    ///
    /// # Errors
    /// [`IpcError::Busy`] while another dialog is open.
    pub(crate) fn enter(&self) -> Result<DialogGuard<'_>, IpcError> {
        self.open
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| DialogGuard { gate: self })
            .map_err(|_| IpcError::Busy)
    }
}

/// An open dialog; releases the [`DialogGate`] when dropped.
#[derive(Debug)]
pub(crate) struct DialogGuard<'a> {
    gate: &'a DialogGate,
}

impl Drop for DialogGuard<'_> {
    fn drop(&mut self) {
        self.gate.open.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn prompt() -> TrustPrompt {
        TrustPrompt {
            project_name: String::from("Guessing Game"),
            folder_display: String::from("/home/ada/games"),
            raw_cpp_blocks: 2,
            libraries: vec![String::from("sqlite3"), String::from("curl")],
            file_system_blocks: 1,
            mark_of_the_web: false,
        }
    }

    #[test]
    fn the_text_explains_lists_and_offers_three_choices() {
        let (title, body) = trust_dialog_text(&prompt());
        assert_eq!(title, "Trust this project?");
        assert!(body.contains("Running this project lets it do anything a program on your computer can do"));
        assert!(body.contains("Project: Guessing Game"));
        assert!(body.contains("Folder: /home/ada/games"));
        assert!(body.contains("Raw C++ blocks: 2"));
        assert!(body.contains("Libraries: sqlite3, curl"));
        assert!(body.contains("File-system blocks: 1"));
        for (_, label) in TrustChoice::LABELS {
            assert!(body.contains(label), "{label}");
        }
        assert!(!body.contains("Internet"));
    }

    #[test]
    fn the_mark_of_the_web_comes_first() {
        let mut motw = prompt();
        motw.mark_of_the_web = true;
        motw.libraries.clear();
        motw.folder_display.clear();
        let (_, body) = trust_dialog_text(&motw);
        assert!(
            body.starts_with("Warning: This file was downloaded from the Internet."),
            "{body}"
        );
        assert!(body.contains("Libraries: none"));
        assert!(body.contains("Folder: not saved yet"));
        motw.libraries = dialog_libraries(&(0..13).map(|i| format!("l{i}")).collect::<Vec<_>>());
        let (_, body) = trust_dialog_text(&motw);
        assert!(body.contains("l8, l9, and 3 more"), "{body}");
    }

    #[test]
    fn labels_are_consistent() {
        for (choice, label) in TrustChoice::LABELS {
            assert_eq!(choice.label(), label);
        }
    }

    #[test]
    fn display_text_shows_hidden_characters_and_cuts() {
        assert_eq!(display_text("plain", 10), "plain");
        assert_eq!(display_text("a\u{202E}b", 10), "a⟨U+202E⟩b");
        assert_eq!(display_text("line\nbreak", 20), "line⟨U+000A⟩break");
        assert_eq!(display_text("zero\u{200B}width", 20), "zero⟨U+200B⟩width");
        assert_eq!(display_text("abcdef", 3), "abc…");
        assert_eq!(display_text("abc", 3), "abc");
        assert_eq!(display_text("", 3), "");
    }

    #[test]
    fn library_lists_are_bounded() {
        let many: Vec<String> = (0..15).map(|i| format!("lib{i}")).collect();
        let shown = dialog_libraries(&many);
        assert_eq!(shown.len(), MAX_DIALOG_LIBRARIES + 1);
        assert_eq!(shown.last().map(String::as_str), Some("and 5 more"));
        let ten: Vec<String> = (0..10).map(|i| format!("lib{i}")).collect();
        assert_eq!(dialog_libraries(&ten), ten);
        assert_eq!(dialog_libraries(&[String::from("x\u{2066}y")]), ["x⟨U+2066⟩y"]);
    }

    #[test]
    fn suggested_names() {
        assert_eq!(
            suggested_file_name(Some(Path::new("/home/ada/game.b2c")), "Other"),
            "game.b2c"
        );
        assert_eq!(suggested_file_name(None, "Guessing Game"), "Guessing Game.b2c");
        assert_eq!(suggested_file_name(None, "../../etc/passwd"), "etcpasswd.b2c");
        assert_eq!(
            suggested_file_name(None, "a/b\\c:d*e?f\"g<h>i|j"),
            "abcdefghij.b2c"
        );
        assert_eq!(suggested_file_name(None, "..."), "project.b2c");
        assert_eq!(suggested_file_name(None, ""), "project.b2c");
        assert_eq!(suggested_file_name(None, "CON"), "project.b2c");
        assert_eq!(suggested_file_name(None, "com1"), "project.b2c");
        assert_eq!(suggested_file_name(None, "com10"), "com10.b2c");
        assert_eq!(suggested_file_name(None, "x\u{202E}y"), "xy.b2c");
        let long = "a".repeat(200);
        assert_eq!(
            suggested_file_name(None, &long).len(),
            MAX_SUGGESTED_STEM_CHARS + ".b2c".len()
        );
        // A current file name with hidden characters is not suggested as is.
        assert_eq!(
            suggested_file_name(Some(Path::new("/p/a\u{202E}b.b2c")), "Game"),
            "Game.b2c"
        );
    }

    #[test]
    fn one_dialog_at_a_time() {
        let gate = DialogGate::default();
        let first = gate.enter().unwrap();
        assert_eq!(gate.enter().unwrap_err(), IpcError::Busy);
        drop(first);
        let again = gate.enter().unwrap();
        drop(again);
    }

    proptest! {
        #[test]
        fn display_text_never_shows_hidden_characters(text in "\\PC{0,40}|[\\x00-\\x1f\\u{200b}-\\u{202e}a-z]{0,40}", max in 0_usize..50) {
            let shown = display_text(&text, max);
            prop_assert!(shown.chars().all(|c| !c.is_control() && !b2c_ir::text::is_invisible(c)));
            prop_assert!(shown.chars().filter(|&c| c == '⟨').count() <= max);
        }

        #[test]
        fn suggested_names_are_plain(name in "\\PC{0,100}") {
            let file = suggested_file_name(None, &name);
            let stem = file.strip_suffix(".b2c");
            prop_assert!(stem.is_some());
            let stem = stem.unwrap_or_default();
            prop_assert!(!stem.is_empty());
            prop_assert!(stem.chars().count() <= MAX_SUGGESTED_STEM_CHARS);
            prop_assert!(stem.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.')));
            prop_assert!(!stem.starts_with('.') && !stem.ends_with('.'));
        }
    }
}
