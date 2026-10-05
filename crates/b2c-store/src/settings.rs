//! Machine settings: `settings.json` in the configuration folder (05 §5.9;
//! 02 §2.5 `settings_get` and `settings_update`; 10 §10.1 M2).
//!
//! ```json
//! {
//!   "format": "blocks2cpp/settings",
//!   "formatVersion": 1,
//!   "codeStyle": { "indentWidth": 4 },
//!   "run": { "onErrors": "disableRun" },
//!   "console": { "scrollbackLines": 10000 },
//!   "toolchain": { "selectedId": null },
//!   "newProject": { "standard": "c++20" },
//!   "buildCache": { "maxBytes": 2147483648 }
//! }
//! ```
//!
//! * The file is read with a 1 MiB bound and validated value by value. A
//!   missing file or value gives the default silently; an invalid value is
//!   reset to its default with an [`NoticeReason::InvalidValue`] notice, and
//!   the other values are kept; a file that is not valid JSON, not an object
//!   or not of this format gives the defaults and a
//!   [`NoticeReason::CorruptFile`] notice.
//! * Keys this version does not know (at the top level or inside a known
//!   section, such as the `lints` and `envPassthrough` of milestone M5) are
//!   kept and written back unchanged, so an older version never deletes what
//!   a newer one wrote. A newer `formatVersion` is read the same way (its
//!   known keys), gives a [`NoticeReason::NewerVersion`] notice, and is kept
//!   on save.
//! * Only [`SettingsPatch`] (code style, Run on errors, console) changes
//!   settings from the UI; its JSON form rejects unknown keys. The selected
//!   toolchain changes only through [`SettingsStore::set_selected_toolchain`].
//! * Every change is validated first, then written atomically (with the
//!   current file on disk as the base, so concurrent app instances do not
//!   undo each other's changes), and only then applied in memory. A mutex
//!   serialises updates. Nothing here ever comes from or goes into a project.
//! * Writing is canonical: keys in a fixed order, 2-space indentation, a
//!   trailing newline, so loading and saving an unchanged file gives the
//!   same bytes. The file is written only as a regular file in a private
//!   folder: a link at `settings.json` is refused, never followed
//!   ([`write_atomic`]).

use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use b2c_ir::sast::CppStandard;
use serde::ser::SerializeMap as _;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

use crate::atomic::{Backup, write_atomic};
use crate::dirs::ensure_private_dir;
use crate::error::{ReadError, StoreError};
use crate::ids::is_lower_hex;
use crate::read::read_bounded;

/// The settings file's name in the configuration folder.
pub const SETTINGS_FILE: &str = "settings.json";
/// The `format` value of the settings file.
pub const SETTINGS_FORMAT: &str = "blocks2cpp/settings";
/// The `formatVersion` this version writes.
pub const SETTINGS_FORMAT_VERSION: u64 = 1;
/// The largest settings file that is read, in bytes (1 MiB).
pub const MAX_SETTINGS_BYTES: u64 = 1024 * 1024;
/// The allowed indent widths, in spaces.
pub const INDENT_WIDTHS: [u8; 2] = [2, 4];
/// The allowed console scrollback, in lines.
pub const SCROLLBACK_LINES: RangeInclusive<u32> = 1_000..=100_000;
/// The allowed build cache size limit, in bytes (256 MiB to 1 TiB).
pub const BUILD_CACHE_BYTES: RangeInclusive<u64> = 268_435_456..=1_099_511_627_776;

/// All machine settings, with defaults filled in.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Settings {
    /// How generated code is laid out.
    pub code_style: CodeStyle,
    /// What Run does while the project has errors.
    pub run: RunSettings,
    /// The program console.
    pub console: ConsoleSettings,
    /// The toolchain chosen with `toolchain_select`.
    pub toolchain: ToolchainSettings,
    /// Defaults for new projects.
    pub new_project: NewProjectSettings,
    /// The build cache.
    pub build_cache: BuildCacheSettings,
}

/// How generated code is laid out (M2: indentation only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeStyle {
    /// Spaces per indentation level: 2 or 4 (default 4).
    pub indent_width: u8,
}

impl Default for CodeStyle {
    fn default() -> Self {
        Self { indent_width: 4 }
    }
}

/// What Run does while the project has analyser errors. Either way the
/// backend refuses to build a project with errors (07 §7.6.1); this only
/// changes the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OnErrors {
    /// Run is disabled, with a tooltip leading to the first error (default).
    #[default]
    DisableRun,
    /// Run stays enabled and opens the Problems panel at the first error.
    ShowProblems,
}

/// Run settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunSettings {
    /// What Run does while the project has errors.
    pub on_errors: OnErrors,
}

/// Program console settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsoleSettings {
    /// Lines kept in the console: 1,000 to 100,000 (default 10,000).
    pub scrollback_lines: u32,
}

impl Default for ConsoleSettings {
    fn default() -> Self {
        Self {
            scrollback_lines: 10_000,
        }
    }
}

/// The selected toolchain.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolchainSettings {
    /// The selected toolchain's ID (`tc_` and 16 lower-case hexadecimal
    /// digits), or `None` to use discovery order.
    pub selected_id: Option<String>,
}

/// Defaults for new projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NewProjectSettings {
    /// The C++ standard of new projects (default C++20, 10 §10.3 Q3).
    pub standard: CppStandard,
}

/// Build cache settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildCacheSettings {
    /// The size above which old builds are evicted: 256 MiB to 1 TiB
    /// (default 2 GiB).
    pub max_bytes: u64,
}

impl Default for BuildCacheSettings {
    fn default() -> Self {
        Self {
            max_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

/// A partial update from the Settings page. Its JSON form
/// (`{"codeStyle": {"indentWidth": 2}}`) rejects unknown keys at every
/// level, so only code style, Run on errors and the console can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
    /// Code style changes.
    #[serde(default)]
    pub code_style: Option<CodeStylePatch>,
    /// Run setting changes.
    #[serde(default)]
    pub run: Option<RunPatch>,
    /// Console setting changes.
    #[serde(default)]
    pub console: Option<ConsolePatch>,
}

/// Code style changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodeStylePatch {
    /// A new indent width (2 or 4).
    #[serde(default)]
    pub indent_width: Option<u8>,
}

/// Run setting changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunPatch {
    /// A new Run on errors choice.
    #[serde(default)]
    pub on_errors: Option<OnErrors>,
}

/// Console setting changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConsolePatch {
    /// A new scrollback length (1,000 to 100,000 lines).
    #[serde(default)]
    pub scrollback_lines: Option<u32>,
}

/// Something the user should be told about the settings file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsNotice {
    /// The dotted path of the value concerned (`codeStyle.indentWidth`), or
    /// the empty string when the notice is about the whole file.
    pub key: String,
    /// What happened.
    pub reason: NoticeReason,
}

/// Why a [`SettingsNotice`] was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeReason {
    /// The value was invalid and has been reset to its default.
    InvalidValue,
    /// The file could not be read or is not a settings file; every value is
    /// at its default.
    CorruptFile,
    /// The file was written by a newer version of Blocks2Cpp; the values
    /// this version knows were read, and the rest is kept unchanged.
    NewerVersion,
}

/// `settings.json`: the current settings in memory, saved on every change.
#[derive(Debug)]
pub struct SettingsStore {
    dir: PathBuf,
    path: PathBuf,
    state: Mutex<State>,
}

/// The settings plus what the file holds beyond them.
#[derive(Debug, Clone)]
struct State {
    settings: Settings,
    extras: Extras,
    /// The `formatVersion` to write: this version's, or a newer one read
    /// from the file.
    format_version: u64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            extras: Extras::default(),
            format_version: SETTINGS_FORMAT_VERSION,
        }
    }
}

/// Why the settings file gives no settings at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unusable {
    /// There is no file (yet).
    Missing,
    /// It cannot be read, is too large, or is not a settings file.
    Corrupt,
}

/// Keys this version does not know, written back unchanged.
#[derive(Debug, Clone, Default)]
struct Extras {
    top: BTreeMap<String, Value>,
    sections: BTreeMap<&'static str, BTreeMap<String, Value>>,
}

/// The known sections, in file order, with their known keys in file order.
const SECTIONS: [(&str, &[&str]); 6] = [
    ("codeStyle", &["indentWidth"]),
    ("run", &["onErrors"]),
    ("console", &["scrollbackLines"]),
    ("toolchain", &["selectedId"]),
    ("newProject", &["standard"]),
    ("buildCache", &["maxBytes"]),
];

impl SettingsStore {
    /// Loads `<config_dir>/settings.json`. Never fails: problems give
    /// defaults and notices (see the module documentation). Nothing is
    /// written until the first change.
    pub fn open(config_dir: &Path) -> (Self, Vec<SettingsNotice>) {
        let path = config_dir.join(SETTINGS_FILE);
        let (state, notices) = match load(&path) {
            Ok(loaded) => loaded,
            Err(Unusable::Missing) => (State::default(), Vec::new()),
            Err(Unusable::Corrupt) => (
                State::default(),
                vec![SettingsNotice {
                    key: String::new(),
                    reason: NoticeReason::CorruptFile,
                }],
            ),
        };
        let store = Self {
            dir: config_dir.to_path_buf(),
            path,
            state: Mutex::new(state),
        };
        (store, notices)
    }

    /// The settings file's path (for the debug log).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The current settings.
    pub fn get(&self) -> Settings {
        self.lock().settings.clone()
    }

    /// Validates and applies a partial update, saves it and returns the new
    /// settings.
    ///
    /// # Errors
    /// [`StoreError::Invalid`] when a value is out of range (nothing is
    /// written or changed); otherwise as [`write_atomic`] (the settings in
    /// memory stay as they were).
    pub fn update(&self, patch: &SettingsPatch) -> Result<Settings, StoreError> {
        if let Some(width) = patch.code_style.and_then(|style| style.indent_width)
            && !INDENT_WIDTHS.contains(&width)
        {
            return Err(StoreError::Invalid("the indent width must be 2 or 4 spaces"));
        }
        if let Some(lines) = patch.console.and_then(|console| console.scrollback_lines)
            && !SCROLLBACK_LINES.contains(&lines)
        {
            return Err(StoreError::Invalid(
                "the console scrollback must be 1,000 to 100,000 lines",
            ));
        }
        self.change(|settings| {
            if let Some(width) = patch.code_style.and_then(|style| style.indent_width) {
                settings.code_style.indent_width = width;
            }
            if let Some(on_errors) = patch.run.and_then(|run| run.on_errors) {
                settings.run.on_errors = on_errors;
            }
            if let Some(lines) = patch.console.and_then(|console| console.scrollback_lines) {
                settings.console.scrollback_lines = lines;
            }
        })
    }

    /// Records the toolchain chosen with `toolchain_select` (`None` to go
    /// back to discovery order), saves it and returns the new settings.
    ///
    /// # Errors
    /// [`StoreError::Invalid`] when `id` is not `tc_` followed by 16
    /// lower-case hexadecimal digits; otherwise as [`SettingsStore::update`].
    pub fn set_selected_toolchain(&self, id: Option<&str>) -> Result<Settings, StoreError> {
        if id.is_some_and(|id| !is_toolchain_id(id)) {
            return Err(StoreError::Invalid(
                "a toolchain ID is tc_ followed by 16 hexadecimal digits",
            ));
        }
        self.change(|settings| settings.toolchain.selected_id = id.map(str::to_owned))
    }

    /// Applies `edit` to the current file's settings (or to the ones in
    /// memory when the file cannot be read), writes the result and keeps it.
    fn change(&self, edit: impl FnOnce(&mut Settings)) -> Result<Settings, StoreError> {
        let mut state = self.lock();
        // Another app instance may have changed the file since it was
        // loaded: build on what is there now, unless it is gone or unusable.
        let mut next = match load(&self.path) {
            Ok((on_disk, _)) => on_disk,
            Err(_) => state.clone(),
        };
        edit(&mut next.settings);
        let bytes = to_json(&next)?;
        ensure_private_dir(&self.dir)?;
        write_atomic(&self.path, &bytes, Backup::None)?;
        *state = next;
        Ok(state.settings.clone())
    }

    /// The state, also after a panic elsewhere while it was locked: it is
    /// only ever replaced as a whole, so it is always consistent.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Whether `id` is a toolchain ID: `tc_` and 16 lower-case hexadecimal
/// digits.
pub fn is_toolchain_id(id: &str) -> bool {
    id.strip_prefix("tc_")
        .is_some_and(|digits| is_lower_hex(digits, 16))
}

/// Reads and validates the settings file: the settings with invalid values
/// reset, and a notice for each reset value (and for a newer version).
fn load(path: &Path) -> Result<(State, Vec<SettingsNotice>), Unusable> {
    let bytes = match read_bounded(path, MAX_SETTINGS_BYTES) {
        Ok(bytes) => bytes,
        Err(ReadError::NotFound) => return Err(Unusable::Missing),
        Err(_) => return Err(Unusable::Corrupt),
    };
    let Ok(Value::Object(mut root)) = serde_json::from_slice::<Value>(&bytes) else {
        return Err(Unusable::Corrupt);
    };
    if root.remove("format").as_ref().and_then(Value::as_str) != Some(SETTINGS_FORMAT) {
        return Err(Unusable::Corrupt);
    }
    let Some(format_version) = root
        .remove("formatVersion")
        .as_ref()
        .and_then(Value::as_u64)
        .filter(|version| *version >= 1)
    else {
        return Err(Unusable::Corrupt);
    };
    let mut notices = Vec::new();
    if format_version > SETTINGS_FORMAT_VERSION {
        notices.push(SettingsNotice {
            key: String::new(),
            reason: NoticeReason::NewerVersion,
        });
    }
    let mut state = State {
        format_version: format_version.max(SETTINGS_FORMAT_VERSION),
        ..State::default()
    };
    for (section, keys) in SECTIONS {
        let Some(value) = root.remove(section) else {
            continue;
        };
        let Value::Object(mut object) = value else {
            notices.push(SettingsNotice {
                key: section.to_owned(),
                reason: NoticeReason::InvalidValue,
            });
            continue;
        };
        for &key in keys {
            if let Some(value) = object.remove(key)
                && !apply(&mut state.settings, section, key, &value)
            {
                notices.push(SettingsNotice {
                    key: format!("{section}.{key}"),
                    reason: NoticeReason::InvalidValue,
                });
            }
        }
        if !object.is_empty() {
            state
                .extras
                .sections
                .insert(section, object.into_iter().collect());
        }
    }
    state.extras.top = root.into_iter().collect();
    Ok((state, notices))
}

/// Sets one known value if it is valid; whether it was.
fn apply(settings: &mut Settings, section: &str, key: &str, value: &Value) -> bool {
    match (section, key) {
        ("codeStyle", "indentWidth") => match value.as_u64().and_then(|width| u8::try_from(width).ok()) {
            Some(width) if INDENT_WIDTHS.contains(&width) => settings.code_style.indent_width = width,
            _ => return false,
        },
        ("run", "onErrors") => match OnErrors::deserialize(value) {
            Ok(on_errors) => settings.run.on_errors = on_errors,
            Err(_) => return false,
        },
        ("console", "scrollbackLines") => match value.as_u64().and_then(|lines| u32::try_from(lines).ok()) {
            Some(lines) if SCROLLBACK_LINES.contains(&lines) => settings.console.scrollback_lines = lines,
            _ => return false,
        },
        ("toolchain", "selectedId") => match value {
            Value::Null => settings.toolchain.selected_id = None,
            Value::String(id) if is_toolchain_id(id) => settings.toolchain.selected_id = Some(id.clone()),
            _ => return false,
        },
        ("newProject", "standard") => match CppStandard::deserialize(value) {
            Ok(standard) => settings.new_project.standard = standard,
            Err(_) => return false,
        },
        ("buildCache", "maxBytes") => match value.as_u64() {
            Some(bytes) if BUILD_CACHE_BYTES.contains(&bytes) => settings.build_cache.max_bytes = bytes,
            _ => return false,
        },
        _ => return false,
    }
    true
}

/// The canonical file contents for `state`.
///
/// # Errors
/// Only if `serde_json` fails to serialise plain values to memory, which
/// does not happen; the file is then left alone.
fn to_json(state: &State) -> Result<Vec<u8>, StoreError> {
    let unserialisable = |_| StoreError::Invalid("the settings could not be serialised");
    let settings = &state.settings;
    let known = |section: &str, key: &str| -> Result<Value, serde_json::Error> {
        Ok(match (section, key) {
            ("codeStyle", "indentWidth") => Value::from(settings.code_style.indent_width),
            ("run", "onErrors") => serde_json::to_value(settings.run.on_errors)?,
            ("console", "scrollbackLines") => Value::from(settings.console.scrollback_lines),
            ("toolchain", "selectedId") => settings
                .toolchain
                .selected_id
                .clone()
                .map_or(Value::Null, Value::from),
            ("newProject", "standard") => serde_json::to_value(settings.new_project.standard)?,
            ("buildCache", "maxBytes") => Value::from(settings.build_cache.max_bytes),
            _ => Value::Null,
        })
    };
    let mut top = Vec::new();
    top.push(("format", Entry::Plain(Value::from(SETTINGS_FORMAT))));
    top.push(("formatVersion", Entry::Plain(Value::from(state.format_version))));
    for (section, keys) in SECTIONS {
        let mut entries: Vec<(&str, Entry<'_>)> = Vec::new();
        for &key in keys {
            entries.push((key, Entry::Plain(known(section, key).map_err(unserialisable)?)));
        }
        if let Some(extra) = state.extras.sections.get(section) {
            entries.extend(
                extra
                    .iter()
                    .map(|(key, value)| (key.as_str(), Entry::Kept(value))),
            );
        }
        top.push((section, Entry::Object(entries)));
    }
    top.extend(
        state
            .extras
            .top
            .iter()
            .map(|(key, value)| (key.as_str(), Entry::Kept(value))),
    );
    let mut bytes = serde_json::to_vec_pretty(&Entry::Object(top)).map_err(unserialisable)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// A value to write, with object keys in a fixed order (`serde_json`'s own
/// map may sort its keys or keep their insertion order depending on its
/// features, so it is not used for the known keys).
enum Entry<'a> {
    Plain(Value),
    Kept(&'a Value),
    Object(Vec<(&'a str, Entry<'a>)>),
}

impl Serialize for Entry<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Plain(value) => value.serialize(serializer),
            Self::Kept(value) => value.serialize(serializer),
            Self::Object(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    const DEFAULT_FILE: &str = r#"{
  "format": "blocks2cpp/settings",
  "formatVersion": 1,
  "codeStyle": {
    "indentWidth": 4
  },
  "run": {
    "onErrors": "disableRun"
  },
  "console": {
    "scrollbackLines": 10000
  },
  "toolchain": {
    "selectedId": null
  },
  "newProject": {
    "standard": "c++20"
  },
  "buildCache": {
    "maxBytes": 2147483648
  }
}
"#;

    fn store_with(contents: Option<&[u8]>) -> (tempfile::TempDir, SettingsStore, Vec<SettingsNotice>) {
        let dir = tempfile::tempdir().unwrap();
        if let Some(contents) = contents {
            fs::write(dir.path().join(SETTINGS_FILE), contents).unwrap();
        }
        let (store, notices) = SettingsStore::open(dir.path());
        (dir, store, notices)
    }

    fn notice(key: &str, reason: NoticeReason) -> SettingsNotice {
        SettingsNotice {
            key: key.to_owned(),
            reason,
        }
    }

    #[test]
    fn a_missing_file_gives_the_defaults_quietly() {
        let (dir, store, notices) = store_with(None);
        assert!(notices.is_empty());
        assert_eq!(store.get(), Settings::default());
        assert_eq!(store.get().code_style.indent_width, 4);
        assert_eq!(store.get().console.scrollback_lines, 10_000);
        assert_eq!(store.get().build_cache.max_bytes, 2_147_483_648);
        assert_eq!(store.get().new_project.standard, CppStandard::Cpp20);
        assert_eq!(store.get().run.on_errors, OnErrors::DisableRun);
        assert!(!dir.path().join(SETTINGS_FILE).exists());
    }

    #[test]
    fn the_default_file_is_canonical() {
        let (dir, store, _) = store_with(None);
        store.update(&SettingsPatch::default()).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(SETTINGS_FILE)).unwrap(),
            DEFAULT_FILE
        );
    }

    #[test]
    fn corrupt_files_give_the_defaults_and_a_notice() {
        for contents in [
            &b"{ not json"[..],
            b"[]",
            b"null",
            b"{}",
            br#"{"format": "blocks2cpp/recent", "formatVersion": 1}"#,
            br#"{"format": "blocks2cpp/settings"}"#,
            br#"{"format": "blocks2cpp/settings", "formatVersion": 0}"#,
            br#"{"format": "blocks2cpp/settings", "formatVersion": "1"}"#,
            b"\xff\xfe",
        ] {
            let (_dir, store, notices) = store_with(Some(contents));
            assert_eq!(notices, [notice("", NoticeReason::CorruptFile)], "{contents:?}");
            assert_eq!(store.get(), Settings::default());
        }
    }

    #[test]
    fn oversized_files_are_not_read() {
        let mut huge = br#"{"format": "blocks2cpp/settings", "formatVersion": 1, "x": ""#.to_vec();
        huge.resize(usize::try_from(MAX_SETTINGS_BYTES).unwrap() + 10, b'a');
        huge.extend_from_slice(b"\"}");
        let (_dir, store, notices) = store_with(Some(&huge));
        assert_eq!(notices, [notice("", NoticeReason::CorruptFile)]);
        assert_eq!(store.get(), Settings::default());
    }

    #[test]
    fn one_invalid_value_resets_only_that_value() {
        let file = DEFAULT_FILE
            .replace("\"indentWidth\": 4", "\"indentWidth\": 3")
            .replace("10000", "500")
            .replace("disableRun", "showProblems");
        let (_dir, store, notices) = store_with(Some(file.as_bytes()));
        assert_eq!(
            notices,
            [
                notice("codeStyle.indentWidth", NoticeReason::InvalidValue),
                notice("console.scrollbackLines", NoticeReason::InvalidValue),
            ]
        );
        let settings = store.get();
        assert_eq!(settings.code_style.indent_width, 4);
        assert_eq!(settings.console.scrollback_lines, 10_000);
        assert_eq!(settings.run.on_errors, OnErrors::ShowProblems);
    }

    #[test]
    fn every_kind_of_invalid_value_is_caught() {
        for (from, to, key) in [
            (
                "\"indentWidth\": 4",
                "\"indentWidth\": 4.0",
                "codeStyle.indentWidth",
            ),
            (
                "\"indentWidth\": 4",
                "\"indentWidth\": 260",
                "codeStyle.indentWidth",
            ),
            ("\"disableRun\"", "\"sometimes\"", "run.onErrors"),
            ("\"disableRun\"", "true", "run.onErrors"),
            ("10000", "100001", "console.scrollbackLines"),
            ("10000", "-5", "console.scrollbackLines"),
            (
                "\"selectedId\": null",
                "\"selectedId\": \"tc_123\"",
                "toolchain.selectedId",
            ),
            (
                "\"selectedId\": null",
                "\"selectedId\": \"tc_0123456789ABCDEF\"",
                "toolchain.selectedId",
            ),
            ("\"c++20\"", "\"c++98\"", "newProject.standard"),
            ("\"c++20\"", "20", "newProject.standard"),
            ("2147483648", "1024", "buildCache.maxBytes"),
            ("2147483648", "1099511627777", "buildCache.maxBytes"),
            (
                "\"run\": {\n    \"onErrors\": \"disableRun\"\n  }",
                "\"run\": \"fast\"",
                "run",
            ),
        ] {
            let file = DEFAULT_FILE.replace(from, to);
            assert_ne!(file, DEFAULT_FILE, "{from}");
            let (_dir, store, notices) = store_with(Some(file.as_bytes()));
            assert_eq!(notices, [notice(key, NoticeReason::InvalidValue)], "{to}");
            assert_eq!(store.get(), Settings::default(), "{to}");
        }
    }

    #[test]
    fn valid_values_are_read() {
        let file = DEFAULT_FILE
            .replace("\"indentWidth\": 4", "\"indentWidth\": 2")
            .replace("null", "\"tc_0123456789abcdef\"")
            .replace("c++20", "c++23")
            .replace("2147483648", "268435456");
        let (_dir, store, notices) = store_with(Some(file.as_bytes()));
        assert!(notices.is_empty());
        let settings = store.get();
        assert_eq!(settings.code_style.indent_width, 2);
        assert_eq!(
            settings.toolchain.selected_id.as_deref(),
            Some("tc_0123456789abcdef")
        );
        assert_eq!(settings.new_project.standard, CppStandard::Cpp23);
        assert_eq!(settings.build_cache.max_bytes, 268_435_456);
    }

    #[test]
    fn updates_are_validated_saved_and_returned() {
        let (dir, store, _) = store_with(None);
        let patch: SettingsPatch = serde_json::from_str(
            r#"{"codeStyle": {"indentWidth": 2}, "console": {"scrollbackLines": 2000}}"#,
        )
        .unwrap();
        let settings = store.update(&patch).unwrap();
        assert_eq!(settings.code_style.indent_width, 2);
        assert_eq!(settings.console.scrollback_lines, 2000);
        assert_eq!(store.get(), settings);
        let patch: SettingsPatch = serde_json::from_str(r#"{"run": {"onErrors": "showProblems"}}"#).unwrap();
        let settings = store.update(&patch).unwrap();
        assert_eq!(settings.run.on_errors, OnErrors::ShowProblems);
        assert_eq!(settings.code_style.indent_width, 2);
        // A fresh load sees the same.
        let (reloaded, notices) = SettingsStore::open(dir.path());
        assert!(notices.is_empty());
        assert_eq!(reloaded.get(), settings);
    }

    #[test]
    fn invalid_updates_change_nothing() {
        let (dir, store, _) = store_with(Some(DEFAULT_FILE.as_bytes()));
        let file_path = dir.path().join(SETTINGS_FILE);
        for json in [
            r#"{"codeStyle": {"indentWidth": 3}}"#,
            r#"{"console": {"scrollbackLines": 999}}"#,
            r#"{"console": {"scrollbackLines": 100001}}"#,
        ] {
            let patch: SettingsPatch = serde_json::from_str(json).unwrap();
            assert!(
                matches!(store.update(&patch), Err(StoreError::Invalid(_))),
                "{json}"
            );
        }
        // Unknown keys and the keys the UI may not change are refused when
        // the patch is decoded, before the store sees it.
        for json in [
            r#"{"theme": "dark"}"#,
            r#"{"codeStyle": {"indentWidth": 2, "tabs": true}}"#,
            r#"{"toolchain": {"selectedId": null}}"#,
            r#"{"buildCache": {"maxBytes": 268435456}}"#,
            r#"{"run": {"onErrors": "never"}}"#,
        ] {
            assert!(serde_json::from_str::<SettingsPatch>(json).is_err(), "{json}");
        }
        assert_eq!(fs::read_to_string(&file_path).unwrap(), DEFAULT_FILE);
        assert_eq!(store.get(), Settings::default());
    }

    #[test]
    fn the_selected_toolchain_is_validated() {
        let (_dir, store, _) = store_with(None);
        let settings = store.set_selected_toolchain(Some("tc_00112233445566ff")).unwrap();
        assert_eq!(
            settings.toolchain.selected_id.as_deref(),
            Some("tc_00112233445566ff")
        );
        for bad in [
            "",
            "tc_",
            "tc_00112233445566f",
            "tc_00112233445566FF",
            "rc_00112233445566ff",
            "../x",
        ] {
            assert!(
                matches!(
                    store.set_selected_toolchain(Some(bad)),
                    Err(StoreError::Invalid(_))
                ),
                "{bad}"
            );
        }
        assert_eq!(
            store.set_selected_toolchain(None).unwrap().toolchain.selected_id,
            None
        );
    }

    #[test]
    fn a_load_save_round_trip_is_byte_stable_and_keeps_unknown_keys() {
        let file = r#"{
  "format": "blocks2cpp/settings",
  "formatVersion": 1,
  "codeStyle": {
    "indentWidth": 2,
    "braces": "allman"
  },
  "run": {
    "onErrors": "showProblems"
  },
  "console": {
    "scrollbackLines": 5000
  },
  "toolchain": {
    "selectedId": "tc_0123456789abcdef"
  },
  "newProject": {
    "standard": "c++17"
  },
  "buildCache": {
    "maxBytes": 1099511627776
  },
  "envPassthrough": [
    "LANG",
    "LC_ALL"
  ],
  "lints": {
    "B2C-W0510": "off"
  }
}
"#;
        let (dir, store, notices) = store_with(Some(file.as_bytes()));
        assert!(notices.is_empty());
        store.update(&SettingsPatch::default()).unwrap();
        let file_path = dir.path().join(SETTINGS_FILE);
        assert_eq!(fs::read_to_string(&file_path).unwrap(), file);
        // A change touches only its own value.
        let patch: SettingsPatch = serde_json::from_str(r#"{"codeStyle": {"indentWidth": 4}}"#).unwrap();
        store.update(&patch).unwrap();
        assert_eq!(
            fs::read_to_string(&file_path).unwrap(),
            file.replace("\"indentWidth\": 2", "\"indentWidth\": 4")
        );
    }

    #[test]
    fn newer_files_are_read_and_keep_their_version() {
        let file = DEFAULT_FILE
            .replace("\"formatVersion\": 1", "\"formatVersion\": 7")
            .replace("\"indentWidth\": 4", "\"indentWidth\": 2");
        let (dir, store, notices) = store_with(Some(file.as_bytes()));
        assert_eq!(notices, [notice("", NoticeReason::NewerVersion)]);
        assert_eq!(store.get().code_style.indent_width, 2);
        store.update(&SettingsPatch::default()).unwrap();
        assert_eq!(fs::read_to_string(dir.path().join(SETTINGS_FILE)).unwrap(), file);
    }

    #[test]
    fn updates_build_on_the_file_written_by_another_instance() {
        let (dir, first, _) = store_with(None);
        let (second, _) = SettingsStore::open(dir.path());
        let patch: SettingsPatch = serde_json::from_str(r#"{"codeStyle": {"indentWidth": 2}}"#).unwrap();
        first.update(&patch).unwrap();
        let patch: SettingsPatch = serde_json::from_str(r#"{"run": {"onErrors": "showProblems"}}"#).unwrap();
        let merged = second.update(&patch).unwrap();
        assert_eq!(merged.code_style.indent_width, 2);
        assert_eq!(merged.run.on_errors, OnErrors::ShowProblems);
    }

    #[test]
    fn a_corrupt_file_is_replaced_from_memory_on_the_next_change() {
        let (dir, store, _) = store_with(None);
        let patch: SettingsPatch = serde_json::from_str(r#"{"codeStyle": {"indentWidth": 2}}"#).unwrap();
        store.update(&patch).unwrap();
        fs::write(dir.path().join(SETTINGS_FILE), "garbage").unwrap();
        let settings = store.update(&SettingsPatch::default()).unwrap();
        assert_eq!(settings.code_style.indent_width, 2);
        let (reloaded, notices) = SettingsStore::open(dir.path());
        assert!(notices.is_empty());
        assert_eq!(reloaded.get().code_style.indent_width, 2);
    }

    #[test]
    fn the_config_folder_is_created_privately_on_first_save() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config").join("blocks2cpp");
        let (store, _) = SettingsStore::open(&config);
        store.update(&SettingsPatch::default()).unwrap();
        assert!(store.path().is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(fs::metadata(&config).unwrap().permissions().mode() & 0o777, 0o700);
            assert_eq!(
                fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn concurrent_updates_are_serialised() {
        let (_dir, store, _) = store_with(None);
        std::thread::scope(|scope| {
            for index in 0..8_u32 {
                let store = &store;
                scope.spawn(move || {
                    let patch = SettingsPatch {
                        console: Some(ConsolePatch {
                            scrollback_lines: Some(1_000 + index),
                        }),
                        ..SettingsPatch::default()
                    };
                    store.update(&patch).unwrap();
                });
            }
        });
        let (reloaded, notices) = SettingsStore::open(store.path().parent().unwrap());
        assert!(notices.is_empty());
        assert_eq!(reloaded.get(), store.get());
    }
}
