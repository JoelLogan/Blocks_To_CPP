//! End-to-end test seams (feature `e2e-hooks`,
//! [ADR-0009](../../../docs/adr/0009-e2e-tooling-and-test-seams.md)).
//!
//! The feature is off by default and cannot be compiled without debug
//! assertions, so it never reaches a release build. With it, the app reads
//! three environment variables, each optional:
//!
//! * [`ROOT_ENV`] (`B2C_E2E_ROOT`): every machine-local folder goes under this
//!   absolute path ([`b2c_store::Dirs::under_root`]), so each test starts from
//!   a fresh profile.
//! * [`DIALOGS_ENV`] (`B2C_E2E_DIALOGS`): the path of a JSON [`DialogScript`]
//!   that answers the open, save-as, pick-compiler and trust dialogs in
//!   order, and cancels once a list is exhausted.
//! * [`TOOLCHAIN_DIRS_ENV`] (`B2C_E2E_TOOLCHAIN_DIRS`): an OS path list (`:`
//!   on Linux, `;` on Windows) of the only folders toolchain discovery
//!   searches; an empty value means no compilers.
//!
//! On Windows it also passes on the browser arguments of
//! [`WEBVIEW2_ARGS_ENV`], which a `WebDriver` for `WebView2` needs
//! ([`webview2_browser_args`]); release builds ignore that variable.
//!
//! Only the native dialogs and the folders are replaced: commands, IPC, the
//! isolation hook, trust and the real g++ are exercised as in the product.

#[cfg(not(debug_assertions))]
compile_error!(
    "the `e2e-hooks` feature is for end-to-end tests only and cannot be built without debug assertions"
);

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use b2c_app::{Dialogs, TrustChoice, TrustPrompt};
use b2c_store::{ReadError, read_bounded};
use serde::Deserialize;

/// The variable naming the test root folder.
pub const ROOT_ENV: &str = "B2C_E2E_ROOT";

/// The variable naming the dialog script.
pub const DIALOGS_ENV: &str = "B2C_E2E_DIALOGS";

/// The variable listing the folders toolchain discovery searches.
pub const TOOLCHAIN_DIRS_ENV: &str = "B2C_E2E_TOOLCHAIN_DIRS";

/// The largest dialog script, in bytes.
pub const MAX_SCRIPT_BYTES: u64 = 1024 * 1024;

/// The most answers in one list of a dialog script.
pub const MAX_SCRIPT_ANSWERS: usize = 1000;

/// The most folders in [`TOOLCHAIN_DIRS_ENV`].
pub const MAX_TOOLCHAIN_DIRS: usize = 64;

/// Why the end-to-end settings cannot be used. The app does not start with
/// invalid settings, so a test never runs against the user's own profile by
/// mistake.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum E2eError {
    /// A variable that must hold an absolute path holds something else.
    #[error("{0} must be an absolute path")]
    NotAbsolute(&'static str),
    /// The dialog script cannot be read.
    #[error("the dialog script cannot be read ({0})")]
    ScriptUnreadable(#[source] ReadError),
    /// The dialog script is not valid.
    #[error("the dialog script is not valid ({0})")]
    ScriptInvalid(String),
    /// [`TOOLCHAIN_DIRS_ENV`] lists more than [`MAX_TOOLCHAIN_DIRS`] folders.
    #[error("{TOOLCHAIN_DIRS_ENV} lists more than {MAX_TOOLCHAIN_DIRS} folders")]
    TooManyToolchainDirs,
}

/// The answer the script gives to one trust dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScriptedTrust {
    /// Trust this project.
    TrustProject,
    /// Trust everything in this folder.
    TrustFolder,
    /// Stay in Restricted Mode.
    StayRestricted,
}

impl From<ScriptedTrust> for TrustChoice {
    fn from(answer: ScriptedTrust) -> Self {
        match answer {
            ScriptedTrust::TrustProject => Self::TrustProject,
            ScriptedTrust::TrustFolder => Self::TrustFolder,
            ScriptedTrust::StayRestricted => Self::StayRestricted,
        }
    }
}

/// The answers to the native dialogs, in order (JSON, every key optional):
///
/// ```json
/// {
///   "open": ["/abs/path/game.b2c", null],
///   "saveAs": ["/abs/path/copy.b2c"],
///   "pickCompiler": ["/usr/bin/g++"],
///   "trust": ["trustProject", "stayRestricted"]
/// }
/// ```
///
/// `null` cancels that dialog; an exhausted list cancels every later one (the
/// trust dialog: Stay in Restricted Mode). Paths must be absolute.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase", default)]
pub struct DialogScript {
    /// Answers to "Open project".
    pub open: Vec<Option<PathBuf>>,
    /// Answers to "Save project as".
    pub save_as: Vec<Option<PathBuf>>,
    /// Answers to "Choose g++".
    pub pick_compiler: Vec<Option<PathBuf>>,
    /// Answers to the trust dialog.
    pub trust: Vec<ScriptedTrust>,
}

impl DialogScript {
    /// Parses and checks a script.
    ///
    /// # Errors
    /// [`E2eError::ScriptInvalid`] for JSON that is not a script (unknown
    /// keys included), a list longer than [`MAX_SCRIPT_ANSWERS`], or a
    /// relative path.
    pub fn parse(json: &[u8]) -> Result<Self, E2eError> {
        let invalid = |error: serde_json::Error| E2eError::ScriptInvalid(error.to_string());
        let value: serde_json::Value = serde_json::from_slice(json).map_err(invalid)?;
        // serde would also read a struct from an array; a script is an object.
        if !value.is_object() {
            return Err(E2eError::ScriptInvalid(String::from("a script is a JSON object")));
        }
        let script: Self = serde_json::from_value(value).map_err(invalid)?;
        let lists = [
            ("open", &script.open),
            ("saveAs", &script.save_as),
            ("pickCompiler", &script.pick_compiler),
        ];
        for (name, answers) in lists {
            if answers.len() > MAX_SCRIPT_ANSWERS {
                return Err(E2eError::ScriptInvalid(format!("{name} has too many answers")));
            }
            if answers.iter().flatten().any(|path| !path.is_absolute()) {
                return Err(E2eError::ScriptInvalid(format!("{name} has a relative path")));
            }
        }
        if script.trust.len() > MAX_SCRIPT_ANSWERS {
            return Err(E2eError::ScriptInvalid(String::from(
                "trust has too many answers",
            )));
        }
        Ok(script)
    }

    /// Reads and parses the script file at `path` (at most
    /// [`MAX_SCRIPT_BYTES`]).
    ///
    /// # Errors
    /// [`E2eError::ScriptUnreadable`], or as [`DialogScript::parse`].
    pub fn read(path: &Path) -> Result<Self, E2eError> {
        let bytes = read_bounded(path, MAX_SCRIPT_BYTES).map_err(E2eError::ScriptUnreadable)?;
        Self::parse(&bytes)
    }
}

/// The variable through which `msedgedriver` gives a `WebView2` app the browser
/// arguments a `WebDriver` session needs, `--remote-debugging-port` among them.
pub const WEBVIEW2_ARGS_ENV: &str = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS";

/// The browser arguments wry gives `WebView2` when the app sets none.
const WRY_BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// The switch whose comma-separated lists are merged.
const DISABLE_FEATURES: &str = "--disable-features=";

/// The `WebView2` browser arguments of an end-to-end build: wry's own defaults
/// and those of `extra` (the value of [`WEBVIEW2_ARGS_ENV`]), or `None` when
/// `extra` is unset or blank, so wry's defaults apply as in the product.
///
/// `WebView2` lets the arguments an app sets through its API take precedence
/// over the variable, and wry always sets some, so without this the remote
/// debugging port `msedgedriver` asks for never opens and no `WebDriver`
/// session starts (seen on windows-2025 with `WebView2` 153). Release builds
/// never read the variable, so it cannot turn remote debugging on there.
/// The `--disable-features` lists of both are merged into one switch,
/// because Chromium keeps only the last of a repeated switch.
pub fn webview2_browser_args(extra: Option<&str>) -> Option<String> {
    let extra = extra.map(str::trim).filter(|extra| !extra.is_empty())?;
    let mut disabled: Vec<&str> = Vec::new();
    let mut others: Vec<&str> = Vec::new();
    for argument in WRY_BROWSER_ARGS
        .split_whitespace()
        .chain(extra.split_whitespace())
    {
        match argument.strip_prefix(DISABLE_FEATURES) {
            Some(features) => {
                for feature in features.split(',').filter(|feature| !feature.is_empty()) {
                    if !disabled.contains(&feature) {
                        disabled.push(feature);
                    }
                }
            }
            None => others.push(argument),
        }
    }
    let mut arguments = format!("{DISABLE_FEATURES}{}", disabled.join(","));
    for argument in others {
        arguments.push(' ');
        arguments.push_str(argument);
    }
    Some(arguments)
}

/// The end-to-end settings from the environment; `None` fields keep the
/// normal behaviour.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct E2eOptions {
    /// The test root ([`ROOT_ENV`]).
    pub root: Option<PathBuf>,
    /// The dialog script ([`DIALOGS_ENV`]).
    pub dialogs: Option<DialogScript>,
    /// The only folders toolchain discovery searches ([`TOOLCHAIN_DIRS_ENV`]).
    pub toolchain_dirs: Option<Vec<PathBuf>>,
}

impl E2eOptions {
    /// The settings from the process's environment.
    ///
    /// # Errors
    /// As [`E2eOptions::from_lookup`].
    pub fn from_env() -> Result<Self, E2eError> {
        Self::from_lookup(&|name| std::env::var_os(name))
    }

    /// The settings from the variables `lookup` returns (tests pass their own
    /// instead of changing the process's environment).
    ///
    /// # Errors
    /// [`E2eError::NotAbsolute`] for a relative root, script path or
    /// toolchain folder; [`E2eError::TooManyToolchainDirs`]; the script's own
    /// errors.
    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<OsString>) -> Result<Self, E2eError> {
        let root = lookup(ROOT_ENV).map(PathBuf::from);
        if root.as_deref().is_some_and(|root| !root.is_absolute()) {
            return Err(E2eError::NotAbsolute(ROOT_ENV));
        }
        let dialogs = match lookup(DIALOGS_ENV).map(PathBuf::from) {
            Some(path) if !path.is_absolute() => return Err(E2eError::NotAbsolute(DIALOGS_ENV)),
            Some(path) => Some(DialogScript::read(&path)?),
            None => None,
        };
        let toolchain_dirs = match lookup(TOOLCHAIN_DIRS_ENV) {
            Some(list) => {
                let dirs: Vec<PathBuf> = std::env::split_paths(&list)
                    .filter(|dir| !dir.as_os_str().is_empty())
                    .collect();
                if dirs.len() > MAX_TOOLCHAIN_DIRS {
                    return Err(E2eError::TooManyToolchainDirs);
                }
                if dirs.iter().any(|dir| !dir.is_absolute()) {
                    return Err(E2eError::NotAbsolute(TOOLCHAIN_DIRS_ENV));
                }
                Some(dirs)
            }
            None => None,
        };
        Ok(Self {
            root,
            dialogs,
            toolchain_dirs,
        })
    }
}

/// [`Dialogs`] answered from a [`DialogScript`], in order.
#[derive(Debug, Default)]
pub struct ScriptedDialogs {
    open: Mutex<VecDeque<Option<PathBuf>>>,
    save_as: Mutex<VecDeque<Option<PathBuf>>>,
    pick_compiler: Mutex<VecDeque<Option<PathBuf>>>,
    trust: Mutex<VecDeque<ScriptedTrust>>,
}

impl ScriptedDialogs {
    /// Dialogs that give `script`'s answers.
    pub fn new(script: DialogScript) -> Self {
        Self {
            open: Mutex::new(script.open.into()),
            save_as: Mutex::new(script.save_as.into()),
            pick_compiler: Mutex::new(script.pick_compiler.into()),
            trust: Mutex::new(script.trust.into()),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Dialogs for ScriptedDialogs {
    fn open_project(&self) -> Option<PathBuf> {
        lock(&self.open).pop_front().flatten()
    }

    fn save_project_as(&self, _suggested_file_name: &str) -> Option<PathBuf> {
        lock(&self.save_as).pop_front().flatten()
    }

    fn pick_compiler(&self) -> Option<PathBuf> {
        lock(&self.pick_compiler).pop_front().flatten()
    }

    fn confirm_trust(&self, _prompt: &TrustPrompt) -> TrustChoice {
        lock(&self.trust)
            .pop_front()
            .map_or(TrustChoice::StayRestricted, TrustChoice::from)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn webview2_arguments_add_the_drivers_to_wrys_own() {
        assert_eq!(webview2_browser_args(None), None);
        assert_eq!(webview2_browser_args(Some("  ")), None);
        // What msedgedriver 153 sets, shortened.
        let driver = "--allow-pre-commit-input --disable-features=IgnoreDuplicateNavs,Prewarm \
                      --enable-automation --remote-debugging-port=0 --test-type=webdriver";
        assert_eq!(
            webview2_browser_args(Some(driver)).as_deref(),
            Some(
                "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,IgnoreDuplicateNavs,Prewarm \
                 --allow-pre-commit-input --enable-automation --remote-debugging-port=0 --test-type=webdriver"
            )
        );
        // A feature named twice is disabled once.
        assert_eq!(
            webview2_browser_args(Some("--disable-features=msWebOOUI,,Prewarm")).as_deref(),
            Some("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,Prewarm")
        );
    }

    fn absolute(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    fn prompt() -> TrustPrompt {
        TrustPrompt {
            project_name: String::from("Game"),
            folder_display: String::new(),
            raw_cpp_blocks: 0,
            libraries: Vec::new(),
            file_system_blocks: 0,
            mark_of_the_web: false,
        }
    }

    #[test]
    fn scripts_answer_in_order_and_then_cancel() {
        let game = absolute("game.b2c");
        let json = serde_json::json!({
            "open": [game, null],
            "saveAs": [absolute("copy.b2c")],
            "trust": ["trustFolder", "trustProject", "stayRestricted"],
        });
        let dialogs = ScriptedDialogs::new(DialogScript::parse(json.to_string().as_bytes()).unwrap());
        assert_eq!(dialogs.open_project(), Some(game));
        assert_eq!(dialogs.open_project(), None);
        assert_eq!(dialogs.open_project(), None);
        assert_eq!(dialogs.save_project_as("x.b2c"), Some(absolute("copy.b2c")));
        assert_eq!(dialogs.save_project_as("x.b2c"), None);
        assert_eq!(dialogs.pick_compiler(), None);
        assert_eq!(dialogs.confirm_trust(&prompt()), TrustChoice::TrustFolder);
        assert_eq!(dialogs.confirm_trust(&prompt()), TrustChoice::TrustProject);
        assert_eq!(dialogs.confirm_trust(&prompt()), TrustChoice::StayRestricted);
        assert_eq!(dialogs.confirm_trust(&prompt()), TrustChoice::StayRestricted);
    }

    #[test]
    fn invalid_scripts_are_refused() {
        for bad in [
            r#"{"opne": []}"#,
            r#"{"open": ["relative/game.b2c"]}"#,
            r#"{"trust": ["trustEverything"]}"#,
            r#"{"open": "game.b2c"}"#,
            "[]",
            "not json",
        ] {
            assert!(
                matches!(
                    DialogScript::parse(bad.as_bytes()),
                    Err(E2eError::ScriptInvalid(_))
                ),
                "{bad}"
            );
        }
        let many = serde_json::json!({ "trust": vec!["trustProject"; MAX_SCRIPT_ANSWERS + 1] });
        assert!(DialogScript::parse(many.to_string().as_bytes()).is_err());
        assert_eq!(DialogScript::parse(b"{}").unwrap(), DialogScript::default());
    }

    #[test]
    fn options_come_from_the_variables() {
        let root = tempfile::tempdir().unwrap();
        let script = root.path().join("dialogs.json");
        std::fs::write(&script, br#"{"trust": ["trustProject"]}"#).unwrap();
        let separator = if cfg!(windows) { ";" } else { ":" };
        let first = absolute("tc-a");
        let second = absolute("tc-b");
        let mut variables = HashMap::new();
        variables.insert(ROOT_ENV, root.path().as_os_str().to_owned());
        variables.insert(DIALOGS_ENV, script.as_os_str().to_owned());
        variables.insert(
            TOOLCHAIN_DIRS_ENV,
            OsString::from(format!("{}{separator}{}", first.display(), second.display())),
        );
        let options = E2eOptions::from_lookup(&|name| variables.get(name).cloned()).unwrap();
        assert_eq!(options.root.as_deref(), Some(root.path()));
        assert_eq!(options.dialogs.unwrap().trust, [ScriptedTrust::TrustProject]);
        assert_eq!(options.toolchain_dirs, Some(vec![first, second]));

        let none = E2eOptions::from_lookup(&|_| None).unwrap();
        assert_eq!(none, E2eOptions::default());

        let empty =
            E2eOptions::from_lookup(&|name| (name == TOOLCHAIN_DIRS_ENV).then(OsString::new)).unwrap();
        assert_eq!(empty.toolchain_dirs, Some(Vec::new()));
    }

    #[test]
    fn relative_or_unreadable_settings_are_refused() {
        let relative = |wanted: &'static str| {
            move |name: &str| (name == wanted).then(|| OsString::from("relative/path"))
        };
        assert!(matches!(
            E2eOptions::from_lookup(&relative(ROOT_ENV)),
            Err(E2eError::NotAbsolute(ROOT_ENV))
        ));
        assert!(matches!(
            E2eOptions::from_lookup(&relative(DIALOGS_ENV)),
            Err(E2eError::NotAbsolute(DIALOGS_ENV))
        ));
        assert!(matches!(
            E2eOptions::from_lookup(&relative(TOOLCHAIN_DIRS_ENV)),
            Err(E2eError::NotAbsolute(TOOLCHAIN_DIRS_ENV))
        ));
        let missing = absolute("no-such-dialog-script.json");
        assert!(matches!(
            E2eOptions::from_lookup(&|name| (name == DIALOGS_ENV).then(|| missing.clone().into_os_string())),
            Err(E2eError::ScriptUnreadable(_))
        ));
    }
}
