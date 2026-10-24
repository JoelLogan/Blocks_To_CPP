//! The recent projects list: `recent.json` in the configuration folder
//! (05 §5.9; 02 §2.5 `project_open_recent`; 04 §4.10 start page).
//!
//! ```json
//! {
//!   "format": "blocks2cpp/recent",
//!   "formatVersion": 1,
//!   "entries": [
//!     {
//!       "id": "rc_0123456789abcdef0123456789abcdef",
//!       "path": "/home/ada/projects/game.b2c",
//!       "projectName": "Guessing game",
//!       "lastOpenedAt": "2026-10-05T14:03:27.512Z"
//!     }
//!   ]
//! }
//! ```
//!
//! * The UI only ever sees the opaque `rc_` IDs; paths stay in the backend
//!   (02 §2.5 "Opaque handles"). An entry keeps its ID when it is opened
//!   again, so the start page's references stay valid.
//! * Newest first, at most [`MAX_RECENT`] entries. Opening a project moves
//!   it to the front.
//! * The file is read with a 1 MiB bound. A corrupt file is an empty list;
//!   an invalid entry (bad ID, relative or oversized path, bad timestamp,
//!   duplicate) is dropped and the others are kept. Each change is written
//!   atomically before it is applied in memory.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::atomic::{Backup, write_atomic};
use crate::dirs::ensure_private_dir;
use crate::error::StoreError;
use crate::ids::{is_hex_id, random_hex_id};
use crate::read::read_bounded;
use crate::time::{parse_rfc3339_utc, rfc3339_utc};

/// The recent list's file name in the configuration folder.
pub const RECENT_FILE: &str = "recent.json";
/// The `format` value of the recent list.
pub const RECENT_FORMAT: &str = "blocks2cpp/recent";
/// The `formatVersion` this version writes.
pub const RECENT_FORMAT_VERSION: u64 = 1;
/// The largest recent list that is read, in bytes (1 MiB).
pub const MAX_RECENT_BYTES: u64 = 1024 * 1024;
/// How many projects the list keeps.
pub const MAX_RECENT: usize = 10;
/// The prefix of recent-entry IDs.
pub const RECENT_ID_PREFIX: &str = "rc_";
/// The longest path that is recorded, in bytes of UTF-8. With the name
/// limit this keeps a full list far below [`MAX_RECENT_BYTES`], even with
/// every character escaped.
pub const MAX_RECENT_PATH_BYTES: usize = 8 * 1024;
/// The longest project name that is recorded, in bytes of UTF-8; longer
/// names are shortened (they are for display only).
pub const MAX_RECENT_NAME_BYTES: usize = 1024;

/// One recently opened project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentEntry {
    /// The opaque ID the UI uses: `rc_` and 32 lower-case hexadecimal
    /// digits.
    pub id: String,
    /// The project's canonical path. Never sent to the webview except for
    /// display.
    pub path: PathBuf,
    /// The project's name when it was last opened.
    pub project_name: String,
    /// When it was last opened (RFC 3339, UTC).
    pub last_opened_at: String,
}

/// The file as written.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecentFile<'a> {
    format: &'static str,
    format_version: u64,
    entries: &'a [RecentEntry],
}

/// The file as read: entries are checked one by one.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecentFileIn {
    format: String,
    format_version: u64,
    entries: Vec<serde_json::Value>,
}

/// `recent.json`: the list in memory, saved on every change.
#[derive(Debug)]
pub struct RecentStore {
    dir: PathBuf,
    path: PathBuf,
    entries: Mutex<Vec<RecentEntry>>,
}

impl RecentStore {
    /// Loads `<config_dir>/recent.json`. Never fails: a missing or corrupt
    /// file is an empty list, and invalid entries are dropped.
    pub fn open(config_dir: &Path) -> Self {
        let path = config_dir.join(RECENT_FILE);
        let entries = load(&path);
        Self {
            dir: config_dir.to_path_buf(),
            path,
            entries: Mutex::new(entries),
        }
    }

    /// The recent list's path (for the debug log).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The entries, newest first (at most [`MAX_RECENT`]).
    pub fn list(&self) -> Vec<RecentEntry> {
        self.lock().clone()
    }

    /// Records that the project at `path` (its canonical path) was opened
    /// now: an existing entry for the path moves to the front and keeps its
    /// ID; otherwise a new entry is added at the front, and the oldest is
    /// dropped beyond [`MAX_RECENT`]. Returns the entry's ID.
    ///
    /// `project_name` is shortened to [`MAX_RECENT_NAME_BYTES`].
    ///
    /// # Errors
    /// [`StoreError::PathNotUnicode`] for a path that is not valid Unicode;
    /// [`StoreError::Invalid`] for a relative path or one longer than
    /// [`MAX_RECENT_PATH_BYTES`]; otherwise as [`write_atomic`] (the list in
    /// memory is then unchanged).
    pub fn touch(&self, path: &Path, project_name: &str) -> Result<String, StoreError> {
        let text = path.to_str().ok_or(StoreError::PathNotUnicode)?;
        if !path.is_absolute() {
            return Err(StoreError::Invalid(
                "only absolute paths are recorded in the recent list",
            ));
        }
        if text.len() > MAX_RECENT_PATH_BYTES {
            return Err(StoreError::Invalid(
                "the path is too long to record in the recent list",
            ));
        }
        let mut entries = self.lock();
        let mut next = entries.clone();
        let existing = next.iter().position(|entry| same_path(&entry.path, path));
        let id = match existing {
            Some(index) => next.remove(index).id,
            None => random_hex_id(RECENT_ID_PREFIX)?,
        };
        next.insert(
            0,
            RecentEntry {
                id: id.clone(),
                path: path.to_path_buf(),
                project_name: shorten(project_name, MAX_RECENT_NAME_BYTES).to_owned(),
                last_opened_at: rfc3339_utc(SystemTime::now()),
            },
        );
        next.truncate(MAX_RECENT);
        self.save(&next)?;
        *entries = next;
        Ok(id)
    }

    /// The path recorded under `id`, if there is one.
    pub fn path_of(&self, id: &str) -> Option<PathBuf> {
        self.lock()
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.path.clone())
    }

    /// Removes the entry `id`; whether there was one.
    ///
    /// # Errors
    /// As [`write_atomic`] (the list in memory is then unchanged).
    pub fn remove(&self, id: &str) -> Result<bool, StoreError> {
        let mut entries = self.lock();
        let Some(index) = entries.iter().position(|entry| entry.id == id) else {
            return Ok(false);
        };
        let mut next = entries.clone();
        next.remove(index);
        self.save(&next)?;
        *entries = next;
        Ok(true)
    }

    /// Writes `entries` to the file.
    fn save(&self, entries: &[RecentEntry]) -> Result<(), StoreError> {
        let file = RecentFile {
            format: RECENT_FORMAT,
            format_version: RECENT_FORMAT_VERSION,
            entries,
        };
        let mut bytes = serde_json::to_vec_pretty(&file)
            .map_err(|_| StoreError::Invalid("the recent list could not be serialised"))?;
        bytes.push(b'\n');
        ensure_private_dir(&self.dir)?;
        write_atomic(&self.path, &bytes, Backup::None)
    }

    /// The list, also after a panic elsewhere while it was locked: it is
    /// only ever replaced as a whole, so it is always consistent.
    fn lock(&self) -> MutexGuard<'_, Vec<RecentEntry>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Reads the list, keeping only valid entries.
fn load(path: &Path) -> Vec<RecentEntry> {
    let Ok(bytes) = read_bounded(path, MAX_RECENT_BYTES) else {
        return Vec::new();
    };
    let Ok(file) = serde_json::from_slice::<RecentFileIn>(&bytes) else {
        return Vec::new();
    };
    if file.format != RECENT_FORMAT || file.format_version < 1 {
        return Vec::new();
    }
    let mut entries: Vec<RecentEntry> = Vec::new();
    for value in file.entries {
        let Ok(entry) = serde_json::from_value::<RecentEntry>(value) else {
            continue;
        };
        let valid = is_hex_id(&entry.id, RECENT_ID_PREFIX)
            && entry.path.is_absolute()
            && entry
                .path
                .to_str()
                .is_some_and(|text| text.len() <= MAX_RECENT_PATH_BYTES)
            && entry.project_name.len() <= MAX_RECENT_NAME_BYTES
            && parse_rfc3339_utc(&entry.last_opened_at).is_some();
        let duplicate = entries
            .iter()
            .any(|seen| seen.id == entry.id || same_path(&seen.path, &entry.path));
        if valid && !duplicate {
            entries.push(entry);
        }
        if entries.len() == MAX_RECENT {
            break;
        }
    }
    entries
}

/// Whether two canonical paths name the same project: equal, ignoring case
/// on Windows (whose file systems normally ignore it).
fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        match (a.to_str(), b.to_str()) {
            (Some(a), Some(b)) => a.to_lowercase() == b.to_lowercase(),
            _ => a == b,
        }
    } else {
        a == b
    }
}

/// `text` cut to at most `max` bytes at a character boundary.
fn shorten(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn project(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{name}.b2c"))
    }

    #[test]
    fn a_missing_file_is_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecentStore::open(dir.path());
        assert!(store.list().is_empty());
        assert_eq!(store.path_of("rc_00000000000000000000000000000000"), None);
    }

    #[test]
    fn opening_moves_an_entry_to_the_front_and_keeps_its_id() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecentStore::open(dir.path());
        let first = store.touch(&project(dir.path(), "a"), "A").unwrap();
        let second = store.touch(&project(dir.path(), "b"), "B").unwrap();
        assert!(is_hex_id(&first, RECENT_ID_PREFIX));
        assert_ne!(first, second);
        let names: Vec<String> = store.list().into_iter().map(|entry| entry.project_name).collect();
        assert_eq!(names, ["B", "A"]);
        let again = store.touch(&project(dir.path(), "a"), "A renamed").unwrap();
        assert_eq!(again, first);
        let list = store.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, first);
        assert_eq!(list[0].project_name, "A renamed");
        assert_eq!(list[1].id, second);
        assert!(parse_rfc3339_utc(&list[0].last_opened_at).is_some());
        assert_eq!(store.path_of(&first), Some(project(dir.path(), "a")));
    }

    #[test]
    fn the_list_is_capped_and_survives_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecentStore::open(dir.path());
        let ids: Vec<String> = (0..12)
            .map(|index| {
                store
                    .touch(&project(dir.path(), &index.to_string()), "P")
                    .unwrap()
            })
            .collect();
        let list = store.list();
        assert_eq!(list.len(), MAX_RECENT);
        assert_eq!(list[0].id, ids[11]);
        assert_eq!(list[9].id, ids[2]);
        assert_eq!(store.path_of(&ids[0]), None);
        let reloaded = RecentStore::open(dir.path());
        assert_eq!(reloaded.list(), list);
    }

    #[test]
    fn entries_can_be_removed() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecentStore::open(dir.path());
        let id = store.touch(&project(dir.path(), "a"), "A").unwrap();
        assert!(store.remove(&id).unwrap());
        assert!(!store.remove(&id).unwrap());
        assert!(!store.remove("rc_unknown").unwrap());
        assert_eq!(store.path_of(&id), None);
        assert!(RecentStore::open(dir.path()).list().is_empty());
    }

    #[test]
    fn only_absolute_unicode_paths_are_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecentStore::open(dir.path());
        assert!(matches!(
            store.touch(Path::new("relative.b2c"), "x"),
            Err(StoreError::Invalid(_))
        ));
        let long = dir.path().join("a".repeat(MAX_RECENT_PATH_BYTES));
        assert!(matches!(store.touch(&long, "x"), Err(StoreError::Invalid(_))));
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt as _;
            let bad = dir.path().join(std::ffi::OsStr::from_bytes(b"\xff.b2c"));
            assert!(matches!(store.touch(&bad, "x"), Err(StoreError::PathNotUnicode)));
        }
        assert!(store.list().is_empty());
        assert!(!dir.path().join(RECENT_FILE).exists());
    }

    #[test]
    fn long_names_are_shortened_at_a_character_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecentStore::open(dir.path());
        let name = "é".repeat(MAX_RECENT_NAME_BYTES);
        store.touch(&project(dir.path(), "a"), &name).unwrap();
        let stored = &store.list()[0].project_name;
        assert_eq!(stored.len(), MAX_RECENT_NAME_BYTES);
        assert!(name.starts_with(stored.as_str()));
        assert_eq!(shorten("abc", 2), "ab");
        assert_eq!(shorten("aé", 2), "a");
    }

    #[test]
    fn corrupt_files_and_invalid_entries_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(RECENT_FILE);
        for contents in [
            "not json",
            "[]",
            r#"{"format": "blocks2cpp/settings", "formatVersion": 1, "entries": []}"#,
        ] {
            fs::write(&path, contents).unwrap();
            assert!(RecentStore::open(dir.path()).list().is_empty(), "{contents}");
        }
        let good = |id: &str, path: &str| serde_json::json!({"id": id, "path": path, "projectName": "P", "lastOpenedAt": "2026-10-05T14:03:27.512Z"});
        let id = |digit: char| format!("rc_{}", digit.to_string().repeat(32));
        let absolute = |name: &str| dir.path().join(name).to_str().unwrap().to_owned();
        let file = serde_json::json!({
            "format": "blocks2cpp/recent",
            "formatVersion": 1,
            "entries": [
                good(&id('1'), &absolute("one.b2c")),
                good("rc_short", &absolute("two.b2c")),
                good(&id('3'), "relative.b2c"),
                good(&id('1'), &absolute("duplicate-id.b2c")),
                good(&id('4'), &absolute("one.b2c")),
                {"id": id('5'), "path": absolute("five.b2c"), "projectName": "P", "lastOpenedAt": "yesterday"},
                {"id": id('6'), "path": absolute("six.b2c")},
                42,
                good(&id('7'), &absolute("seven.b2c")),
            ],
        });
        fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
        let ids: Vec<String> = RecentStore::open(dir.path())
            .list()
            .into_iter()
            .map(|entry| entry.id)
            .collect();
        assert_eq!(ids, [id('1'), id('7')]);
    }

    #[test]
    fn oversized_files_are_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let mut huge =
            br#"{"format": "blocks2cpp/recent", "formatVersion": 1, "entries": [], "x": ""#.to_vec();
        huge.resize(usize::try_from(MAX_RECENT_BYTES).unwrap() + 1, b'a');
        huge.extend_from_slice(b"\"}");
        fs::write(dir.path().join(RECENT_FILE), huge).unwrap();
        assert!(RecentStore::open(dir.path()).list().is_empty());
    }

    #[test]
    fn a_full_list_with_the_longest_values_stays_readable() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecentStore::open(dir.path());
        // Every character escaped as \u00XX in JSON: the worst case.
        let name = "\u{1}".repeat(MAX_RECENT_NAME_BYTES);
        for index in 0..MAX_RECENT {
            let mut path = dir.path().join(index.to_string());
            let room = MAX_RECENT_PATH_BYTES - path.to_str().unwrap().len() - 1;
            path.push("\u{1}".repeat(room));
            store.touch(&path, &name).unwrap();
        }
        let size = fs::metadata(dir.path().join(RECENT_FILE)).unwrap().len();
        assert!(size <= MAX_RECENT_BYTES, "{size}");
        assert_eq!(RecentStore::open(dir.path()).list(), store.list());
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let store = RecentStore::open(dir.path());
        store.touch(&project(dir.path(), "a"), "A").unwrap();
        let mode = fs::metadata(store.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
