//! `toolchains.json`: the toolchain list kept between runs
//! (`docs/spec/05-project-format.md` §5.9).
//!
//! ```json
//! { "format": "blocks2cpp/toolchains", "formatVersion": 1,
//!   "toolchains": [ { "source": "path", "foundAs": "/usr/bin/g++", "probe": { … } } ] }
//! ```
//!
//! `source` is how the toolchain was found (`path`, `wellKnown` or `manual`,
//! the IPC `Toolchain.source` values), `foundAs` the path it was found as
//! before links were resolved (the IPC `displayPath`), and `probe` the
//! probed [`Toolchain`] with its fingerprint.
//!
//! The file is a cache, so reading it never fails: a missing, unreadable,
//! oversized ([`MAX_STORE_BYTES`]) or malformed file, one with another
//! `format` or `formatVersion`, and the bare array that M1 wrote, all count
//! as empty, and discovery fills the list again. Each entry is checked on its
//! own, so one bad entry costs only itself. It is written atomically and
//! owner-only through [`b2c_store::write_atomic`].

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use b2c_ipc::ToolchainId;
use b2c_ipc::dto::ToolchainSource;
use b2c_toolchain::probe::Toolchain;
use serde::{Deserialize, Serialize};

/// The file name, in the machine folder (`b2c_store::Dirs::machine`).
pub const STORE_FILE: &str = "toolchains.json";
/// The `format` tag of the file.
pub const STORE_FORMAT: &str = "blocks2cpp/toolchains";
/// The `formatVersion` this version reads and writes.
pub const STORE_FORMAT_VERSION: u32 = 1;
/// The largest file read (4 MiB; an entry is a few kilobytes).
pub const MAX_STORE_BYTES: u64 = 4 * 1024 * 1024;
/// The most toolchains kept: up to [`MAX_DISCOVERED`] found by discovery and
/// up to [`MAX_MANUAL`] added by hand.
pub const MAX_TOOLCHAINS: usize = MAX_DISCOVERED + MAX_MANUAL;
/// The most compilers one discovery probes and keeps. Real computers have a
/// handful; the bound keeps a pathological `PATH` from costing minutes.
pub const MAX_DISCOVERED: usize = 64;
/// The most manually added compilers kept; adding one more forgets the
/// oldest.
pub const MAX_MANUAL: usize = 64;

/// One toolchain in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Entry {
    /// Its ID, derived from the canonical driver path.
    pub(super) id: ToolchainId,
    /// How it was found.
    pub(super) source: ToolchainSource,
    /// The path it was found as (for display).
    pub(super) found_as: PathBuf,
    /// The probe result. Its problems start with the warnings about the
    /// compiler's location (legacy MinGW, network folder).
    pub(super) probe: Toolchain,
}

impl Entry {
    /// An entry for a probed toolchain.
    pub(super) fn new(source: ToolchainSource, found_as: PathBuf, probe: Toolchain) -> Self {
        Self {
            id: ToolchainId::for_driver(probe.path()),
            source,
            found_as,
            probe,
        }
    }

    /// The canonical driver path: the entry's identity.
    pub(super) fn path(&self) -> &Path {
        self.probe.path()
    }

    /// Whether the user added it by hand.
    pub(super) fn is_manual(&self) -> bool {
        self.source == ToolchainSource::Manual
    }
}

/// An entry as stored.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredEntry {
    source: ToolchainSource,
    found_as: PathBuf,
    probe: Toolchain,
}

/// The file's top level as read: entries are checked one at a time.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredFile {
    format: String,
    format_version: u32,
    toolchains: Vec<serde_json::Value>,
}

/// Reads the list from `path` (see the module documentation); never fails.
pub(super) fn load(path: &Path) -> Vec<Entry> {
    match b2c_store::read_bounded(path, MAX_STORE_BYTES) {
        Ok(bytes) => parse(&bytes),
        Err(b2c_store::ReadError::NotFound) => Vec::new(),
        Err(error) => {
            tracing::debug!(path = %path.display(), %error, "toolchains.json cannot be read; starting empty");
            Vec::new()
        }
    }
}

/// The entries of a `toolchains.json` text. Anything that is not the
/// current format gives an empty list; entries that are invalid, repeated
/// (by canonical path) or beyond the limits are left out.
pub(super) fn parse(bytes: &[u8]) -> Vec<Entry> {
    let Ok(file) = serde_json::from_slice::<StoredFile>(bytes) else {
        return Vec::new();
    };
    if file.format != STORE_FORMAT || file.format_version != STORE_FORMAT_VERSION {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    let (mut discovered, mut manual) = (0_usize, 0_usize);
    let mut entries = Vec::new();
    for value in file.toolchains {
        let Ok(stored) = serde_json::from_value::<StoredEntry>(value) else {
            continue;
        };
        if !stored.found_as.is_absolute() || !stored.probe.path().is_absolute() {
            continue;
        }
        let count = if stored.source == ToolchainSource::Manual {
            (&mut manual, MAX_MANUAL)
        } else {
            (&mut discovered, MAX_DISCOVERED)
        };
        if *count.0 >= count.1 || !seen.insert(stored.probe.path().to_path_buf()) {
            continue;
        }
        *count.0 += 1;
        entries.push(Entry::new(stored.source, stored.found_as, stored.probe));
    }
    entries
}

/// The `toolchains.json` text for `entries`. An entry that cannot be stored
/// (a path that is not valid Unicode) is left out; it is found again by the
/// next discovery.
pub(super) fn render(entries: &[Entry]) -> Vec<u8> {
    let toolchains: Vec<serde_json::Value> = entries
        .iter()
        .filter_map(|entry| {
            let stored = StoredEntry {
                source: entry.source,
                found_as: entry.found_as.clone(),
                probe: entry.probe.clone(),
            };
            serde_json::to_value(&stored).ok()
        })
        .collect();
    let file = serde_json::json!({
        "format": STORE_FORMAT,
        "formatVersion": STORE_FORMAT_VERSION,
        "toolchains": toolchains,
    });
    let mut text = serde_json::to_vec_pretty(&file).unwrap_or_default();
    text.push(b'\n');
    text
}

/// Writes the list to `path` atomically and owner-only, creating its private
/// folder first. Failing to save costs only probing again next time, so the
/// error is logged (the path at debug level only) and otherwise ignored.
pub(super) fn save(path: &Path, entries: &[Entry]) {
    let result = path
        .parent()
        .ok_or(b2c_store::StoreError::Invalid("the toolchain list has no folder"))
        .and_then(b2c_store::ensure_private_dir)
        .and_then(|()| b2c_store::write_atomic(path, &render(entries), b2c_store::Backup::None));
    if let Err(error) = result {
        tracing::warn!(%error, "the toolchain list could not be saved");
        tracing::debug!(path = %path.display(), "the toolchain list that could not be saved");
    }
}

#[cfg(test)]
mod tests {
    use b2c_toolchain::fingerprint::Fingerprint;
    use b2c_toolchain::probe::{Capabilities, CompilerKind, PROBE_FORMAT};
    use b2c_toolchain::target::{GccVersion, Target};

    use super::*;

    fn probe(path: &str) -> Toolchain {
        Toolchain {
            format: PROBE_FORMAT,
            fingerprint: Fingerprint {
                path: PathBuf::from(path),
                size: 1,
                modified_ns: 2,
                sha256: "0".repeat(64),
            },
            kind: CompilerKind::Gcc,
            version: GccVersion::parse("13.3.0"),
            version_text: String::from("g++ 13.3.0"),
            target: Target::parse("x86_64-linux-gnu"),
            capabilities: Capabilities::default(),
            problems: Vec::new(),
        }
    }

    fn entry(source: ToolchainSource, path: &str) -> Entry {
        Entry::new(source, PathBuf::from(path), probe(path))
    }

    #[cfg(unix)]
    #[test]
    fn entries_round_trip_in_order() {
        let entries = vec![
            entry(ToolchainSource::Path, "/usr/bin/g++-13"),
            entry(ToolchainSource::WellKnown, "/usr/local/bin/g++"),
            entry(ToolchainSource::Manual, "/opt/gcc/bin/g++"),
        ];
        let text = render(&entries);
        assert_eq!(parse(&text), entries);
        let json: serde_json::Value = serde_json::from_slice(&text).unwrap();
        assert_eq!(json["format"], STORE_FORMAT);
        assert_eq!(json["formatVersion"], STORE_FORMAT_VERSION);
        assert_eq!(json["toolchains"][1]["source"], "wellKnown");
        assert_eq!(json["toolchains"][2]["foundAs"], "/opt/gcc/bin/g++");
        assert!(json["toolchains"][0]["probe"]["fingerprint"].is_object());
    }

    #[test]
    fn other_formats_count_as_empty() {
        let probe_json = serde_json::to_value(probe("/usr/bin/g++")).unwrap();
        for text in [
            // The bare array of M1.
            serde_json::json!([probe_json]),
            serde_json::json!({"format": "blocks2cpp/toolchains", "formatVersion": 2, "toolchains": []}),
            serde_json::json!({"format": "blocks2cpp/settings", "formatVersion": 1, "toolchains": []}),
            serde_json::json!({"format": "blocks2cpp/toolchains", "toolchains": []}),
            serde_json::json!("toolchains"),
        ] {
            assert!(parse(text.to_string().as_bytes()).is_empty(), "{text}");
        }
        assert!(parse(b"{ not json").is_empty());
        assert!(parse(b"").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn bad_entries_cost_only_themselves() {
        let good = serde_json::to_value(StoredEntry {
            source: ToolchainSource::Path,
            found_as: PathBuf::from("/usr/bin/g++"),
            probe: probe("/usr/bin/g++"),
        })
        .unwrap();
        let mut unknown_key = good.clone();
        unknown_key["extra"] = serde_json::json!(1);
        let mut bad_source = good.clone();
        bad_source["source"] = serde_json::json!("discovered");
        let mut relative = good.clone();
        relative["foundAs"] = serde_json::json!("bin/g++");
        let mut relative_probe = good.clone();
        relative_probe["probe"]["fingerprint"]["path"] = serde_json::json!("g++");
        let mut other = good.clone();
        other["probe"]["fingerprint"]["path"] = serde_json::json!("/usr/bin/g++-14");
        let file = serde_json::json!({
            "format": STORE_FORMAT,
            "formatVersion": 1,
            "toolchains": [unknown_key, bad_source, relative, relative_probe, good, good.clone(), 42, other],
        });
        let entries = parse(file.to_string().as_bytes());
        let paths: Vec<_> = entries.iter().map(|entry| entry.path().to_path_buf()).collect();
        assert_eq!(
            paths,
            [PathBuf::from("/usr/bin/g++"), PathBuf::from("/usr/bin/g++-14")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_counts_are_bounded() {
        let mut entries: Vec<Entry> = (0..=MAX_DISCOVERED)
            .map(|n| entry(ToolchainSource::Path, &format!("/d/{n}/g++")))
            .collect();
        entries.extend((0..=MAX_MANUAL).map(|n| entry(ToolchainSource::Manual, &format!("/m/{n}/g++"))));
        let parsed = parse(&render(&entries));
        assert_eq!(parsed.len(), MAX_TOOLCHAINS);
        assert_eq!(
            parsed.iter().filter(|entry| entry.is_manual()).count(),
            MAX_MANUAL
        );
        assert!(!parsed.iter().any(|entry| entry.path() == Path::new("/d/64/g++")));
    }

    #[test]
    fn ids_follow_the_canonical_path() {
        let a = entry(ToolchainSource::Path, "/usr/bin/g++-13");
        assert_eq!(a.id, ToolchainId::for_driver(Path::new("/usr/bin/g++-13")));
        assert!(!a.is_manual());
        assert!(entry(ToolchainSource::Manual, "/x/g++").is_manual());
    }

    #[cfg(unix)]
    #[test]
    fn saving_creates_a_private_folder_and_an_owner_only_file() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let path = root.join("machine").join(STORE_FILE);
        let entries = vec![entry(ToolchainSource::Path, "/usr/bin/g++")];
        save(&path, &entries);
        assert_eq!(load(&path), entries);
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&root.join("machine")), 0o700);
        // No temporary files are left behind.
        let names: Vec<_> = std::fs::read_dir(root.join("machine"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [STORE_FILE]);
    }

    #[test]
    fn unreadable_files_count_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&dir.path().join("missing.json")).is_empty());
        // A folder in its place.
        assert!(load(dir.path()).is_empty());
        // An oversized file is refused without being parsed.
        let big = dir.path().join("big.json");
        let file = std::fs::File::create(&big).unwrap();
        file.set_len(MAX_STORE_BYTES + 1).unwrap();
        assert!(load(&big).is_empty());
    }
}
