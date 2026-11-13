//! The per-unit object cache (`docs/plans/m3-contracts/w3-modules.md` §1.6,
//! `modules.md` §5.3).
//!
//! A build of several translation units compiles each one to
//! `obj/<stem>-<key12>.o`, with the unit's compiler messages beside it in
//! `obj/<stem>-<key12>.messages.json`. The key is a SHA-256 over everything
//! that decides the object: the toolchain's fingerprint, the compile argv
//! without its source and output paths (so every flag, the C++ standard and
//! the `-iquote` folder), the unit's text, and the text of every generated
//! header in path order (`b2c_support.hpp` among them). A unit whose two
//! files exist is not compiled again; its messages are read back and mapped
//! with the current source map, so its warnings are still reported.
//!
//! The compiler writes `tmp/<stem>.o` and the build writes
//! `tmp/<stem>.messages.json`, in the build folder's private `tmp/`; both
//! are renamed into `obj/` under their keyed names only after the unit
//! compiled. (The temporary object keeps the source's stem because GCC 13
//! and 14 name a unit's SARIF file after its output: `-o main.tmp.o` would
//! give `main.tmp.cpp.sarif`, not the `main.cpp.sarif` the build reads.) A
//! failed or cancelled build removes only its temporaries (by their absolute
//! paths), so the objects it did compile are kept and fixing one unit's
//! error recompiles only that unit. After a successful link, `obj/` keeps
//! only that build's files ([`prune`]).
//!
//! The build folder's lock is held throughout, so no other build uses the
//! same temporary names at the same time.

use std::fs;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use b2c_ir::source_map::{FileKind, GeneratedProject};
use b2c_toolchain::command::CompilerCommand;
use b2c_toolchain::diagnostics::ParsedOutput;
use b2c_toolchain::probe::Toolchain;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::build_dir::BuildDirError;
use crate::manifest::ManifestToolchain;

/// The largest messages file that is read back; a larger one is a miss.
pub(crate) const MAX_MESSAGES_BYTES: u64 = 4 * 1024 * 1024;

/// How many hexadecimal digits of the key name a unit's files.
const KEY_DIGITS: usize = 12;

/// The format of the messages file; a file of another format is a miss.
const MESSAGES_FORMAT: u32 = 1;

/// The suffix of a cached object (after `<stem>-<key12>`).
const OBJECT_SUFFIX: &str = ".o";

/// The suffix of a cached messages file (after `<stem>-<key12>`).
const MESSAGES_SUFFIX: &str = ".messages.json";

/// What every unit's key of one build shares: the toolchain's fingerprint
/// and the generated headers.
#[derive(Debug, Clone)]
pub(crate) struct KeyBase {
    digest: [u8; 32],
}

impl KeyBase {
    /// The shared part of the keys of a build of `generated` with
    /// `toolchain`: its fingerprint (path, size, modification time, SHA-256,
    /// version and target) and every generated header, in path order.
    pub(crate) fn new(toolchain: &Toolchain, generated: &GeneratedProject) -> Self {
        let mut hasher = Sha256::new();
        field(&mut hasher, b"blocks2cpp object cache 1");
        field(
            &mut hasher,
            &serde_json::to_vec(&ManifestToolchain::new(toolchain)).unwrap_or_default(),
        );
        let mut headers: Vec<_> = generated
            .files
            .iter()
            .filter(|file| file.kind == FileKind::Header)
            .collect();
        headers.sort_by(|a, b| a.path.cmp(&b.path));
        field(&mut hasher, &(headers.len() as u64).to_le_bytes());
        for header in headers {
            field(&mut hasher, header.path.as_bytes());
            field(&mut hasher, header.contents.as_bytes());
        }
        Self {
            digest: hasher.finalize().into(),
        }
    }

    /// The key (64 hexadecimal digits) of the unit whose text is `unit`,
    /// compiled by `command`: its argv counts without the source's and the
    /// output's paths (the values of `-c` and `-o`).
    pub(crate) fn unit_key(&self, command: &CompilerCommand, unit: &[u8]) -> String {
        let mut hasher = Sha256::new();
        field(&mut hasher, &self.digest);
        field(&mut hasher, command.program.as_os_str().as_encoded_bytes());
        let mut skip_next = false;
        for arg in &command.args {
            if std::mem::take(&mut skip_next) {
                continue;
            }
            skip_next = arg == "-c" || arg == "-o";
            field(&mut hasher, arg.as_encoded_bytes());
        }
        field(&mut hasher, unit);
        b2c_model::hex(&hasher.finalize())
    }
}

/// Adds one length-prefixed field, so no two sequences of fields hash alike.
fn field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

/// The messages file: a unit's parsed compiler output.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MessagesFile {
    format: u32,
    output: ParsedOutput,
}

/// One translation unit's files in `obj/`, all absolute paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CachedUnit {
    /// `obj/<stem>-<key12>.o`.
    pub(crate) object: PathBuf,
    /// `obj/<stem>-<key12>.messages.json`.
    pub(crate) messages: PathBuf,
    /// `tmp/<stem>.o`, which the compiler writes.
    pub(crate) temp_object: PathBuf,
    /// `tmp/<stem>.messages.json`, written before it is renamed.
    pub(crate) temp_messages: PathBuf,
}

impl CachedUnit {
    /// The files of the unit `stem` with `key` (at least [`KEY_DIGITS`]
    /// hexadecimal digits): cached in `obj_dir`, written first to
    /// `tmp_dir`.
    pub(crate) fn new(obj_dir: &Path, tmp_dir: &Path, stem: &str, key: &str) -> Self {
        let short = key.get(..KEY_DIGITS).unwrap_or(key);
        Self {
            object: obj_dir.join(format!("{stem}-{short}{OBJECT_SUFFIX}")),
            messages: obj_dir.join(format!("{stem}-{short}{MESSAGES_SUFFIX}")),
            temp_object: tmp_dir.join(temp_object_name(stem)),
            temp_messages: tmp_dir.join(format!("{stem}{MESSAGES_SUFFIX}")),
        }
    }

    /// The cached object's file name (`<stem>-<key12>.o`).
    pub(crate) fn object_name(&self) -> &std::ffi::OsStr {
        self.object.file_name().unwrap_or_default()
    }

    /// The unit's cached messages, when its object and messages are both
    /// regular files (never through a link) and the messages file is at most
    /// [`MAX_MESSAGES_BYTES`] and parses strictly; `None` (a miss) otherwise.
    pub(crate) fn lookup(&self) -> Option<ParsedOutput> {
        if !is_regular_file(&self.object) || !is_regular_file(&self.messages) {
            return None;
        }
        let file = fs::File::open(&self.messages).ok()?;
        if file.metadata().ok()?.len() > MAX_MESSAGES_BYTES {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(MAX_MESSAGES_BYTES + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() as u64 > MAX_MESSAGES_BYTES {
            return None;
        }
        let parsed: MessagesFile = serde_json::from_slice(&bytes).ok()?;
        (parsed.format == MESSAGES_FORMAT).then_some(parsed.output)
    }

    /// Records a unit the compiler just wrote to [`Self::temp_object`]:
    /// writes its messages to [`Self::temp_messages`], then renames the
    /// object and, last, the messages to their keyed names, so a messages
    /// file never stands beside an incomplete object.
    ///
    /// # Errors
    /// Fails when a file cannot be written or renamed.
    pub(crate) fn store(&self, output: &ParsedOutput) -> Result<(), BuildDirError> {
        let io = |action: &'static str, path: &Path| {
            let path = path.to_path_buf();
            move |source| BuildDirError::Io { action, path, source }
        };
        let json = serde_json::to_vec(&MessagesFile {
            format: MESSAGES_FORMAT,
            output: output.clone(),
        })
        .unwrap_or_default();
        let _ = fs::remove_file(&self.temp_messages);
        // `create_new` never opens through a link.
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.temp_messages)
            .map_err(io("create", &self.temp_messages))?;
        file.write_all(&json)
            .and_then(|()| file.sync_all())
            .map_err(io("write", &self.temp_messages))?;
        drop(file);
        fs::rename(&self.temp_object, &self.object).map_err(io("store the compiled object", &self.object))?;
        fs::rename(&self.temp_messages, &self.messages)
            .map_err(io("store the compiler messages", &self.messages))
    }

    /// Deletes the unit's temporaries by their absolute paths, best effort;
    /// its cached files stay.
    pub(crate) fn remove_temporaries(&self) {
        let _ = fs::remove_file(&self.temp_object);
        let _ = fs::remove_file(&self.temp_messages);
    }
}

/// `<stem>.o`, the object the compiler writes (in `tmp/`) for the unit
/// `stem`.
pub(crate) fn temp_object_name(stem: &str) -> String {
    format!("{stem}{OBJECT_SUFFIX}")
}

/// Whether `path` is a regular file (looked at with `symlink_metadata`, so
/// a link never is).
fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file() && !metadata.is_symlink())
}

/// Deletes the files of `obj_dir` that are not `keep`'s, so it holds only
/// the last successful build's objects and messages: every regular file
/// (never through a link) whose name is a cache file name
/// ([`is_cache_file_name`]). Anything else is left
/// alone. Best effort: a file that cannot be deleted stays and is only
/// logged; it is never reused for another key. Returns how many files it
/// deleted.
pub(crate) fn prune<'a>(obj_dir: &Path, keep: impl IntoIterator<Item = &'a CachedUnit>) -> usize {
    let kept: Vec<&std::ffi::OsStr> = keep
        .into_iter()
        .flat_map(|unit| [unit.object.file_name(), unit.messages.file_name()])
        .flatten()
        .collect();
    let Ok(entries) = fs::read_dir(obj_dir) else {
        return 0;
    };
    let mut deleted = 0;
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        if kept.contains(&name.as_os_str()) || !name.to_str().is_some_and(is_cache_file_name) {
            continue;
        }
        let path = entry.path();
        if !is_regular_file(&path) {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => deleted += 1,
            Err(error) => tracing::debug!(error = %error, "could not delete an old object"),
        }
    }
    deleted
}

/// Whether `name` is a name the cache gives a file: `<stem>-<key12>.o` or
/// `<stem>-<key12>.messages.json`, with a stem of `[a-z0-9_-]{1,64}`.
fn is_cache_file_name(name: &str) -> bool {
    let stem_ok = |stem: &str| {
        !stem.is_empty()
            && stem.len() <= 64
            && stem
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    };
    let keyed = |rest: &str| {
        rest.rsplit_once('-').is_some_and(|(stem, key)| {
            stem_ok(stem)
                && key.len() == KEY_DIGITS
                && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        })
    };
    name.strip_suffix(MESSAGES_SUFFIX)
        .or_else(|| name.strip_suffix(OBJECT_SUFFIX))
        .is_some_and(keyed)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::ffi::OsString;

    use b2c_toolchain::diagnostics::{CompilerMessage, MessageOrigin, MessageSeverity};
    use b2c_toolchain::probe::DiagnosticsFormat;

    use super::*;

    fn command(args: &[&str]) -> CompilerCommand {
        CompilerCommand {
            program: PathBuf::from("/usr/bin/g++"),
            args: args.iter().map(OsString::from).collect(),
            format: DiagnosticsFormat::Plain,
            sarif_files: Vec::new(),
        }
    }

    fn base() -> KeyBase {
        KeyBase { digest: [7; 32] }
    }

    fn warning() -> ParsedOutput {
        ParsedOutput {
            messages: vec![{
                let mut message = CompilerMessage::new(
                    MessageOrigin::Compiler,
                    MessageSeverity::Warning,
                    "unused variable 'x'",
                );
                message.option = Some(String::from("-Wunused-variable"));
                message
            }],
            truncated: false,
        }
    }

    /// The key ignores where the source and the object are, and nothing
    /// else: a flag, the program or the unit's text changes it.
    #[test]
    fn keys_ignore_only_the_source_and_output_paths() {
        let key = base().unit_key(
            &command(&["-std=c++17", "-c", "/a/main.cpp", "-o", "../obj/x.o"]),
            b"int x;",
        );
        assert_eq!(key.len(), 64);
        assert_eq!(
            key,
            base().unit_key(
                &command(&["-std=c++17", "-c", "/b/main.cpp", "-o", "y.o"]),
                b"int x;"
            )
        );
        for (args, text) in [
            (["-std=c++20", "-c", "/a/main.cpp", "-o", "x.o"], &b"int x;"[..]),
            (["-std=c++17", "-c", "/a/main.cpp", "-O2", "x.o"], b"int x;"),
            (["-std=c++17", "-c", "/a/main.cpp", "-o", "x.o"], b"int y;"),
        ] {
            assert_ne!(key, base().unit_key(&command(&args), text));
        }
        let mut other = command(&["-std=c++17", "-c", "/a/main.cpp", "-o", "x.o"]);
        other.program = PathBuf::from("/usr/bin/g++-14");
        assert_ne!(key, base().unit_key(&other, b"int x;"));
        let mut changed = base();
        changed.digest[0] = 8;
        assert_ne!(
            key,
            changed.unit_key(
                &command(&["-std=c++17", "-c", "/a/main.cpp", "-o", "x.o"]),
                b"int x;"
            )
        );
    }

    /// A stored unit is found again with its messages; a missing,
    /// oversized, malformed or linked messages file, or a missing object, is
    /// a miss.
    #[test]
    fn stored_units_are_found_and_anything_wrong_is_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let unit = CachedUnit::new(dir.path(), tmp.path(), "main", &"ab".repeat(32));
        assert_eq!(unit.object, dir.path().join("main-abababababab.o"));
        assert_eq!(unit.messages, dir.path().join("main-abababababab.messages.json"));
        assert_eq!(unit.temp_object, tmp.path().join("main.o"));
        assert_eq!(unit.temp_messages, tmp.path().join("main.messages.json"));
        assert_eq!(unit.object_name(), "main-abababababab.o");
        assert_eq!(unit.lookup(), None);
        fs::write(&unit.temp_object, "object").unwrap();
        unit.store(&warning()).unwrap();
        assert!(!unit.temp_object.exists() && !unit.temp_messages.exists());
        assert_eq!(fs::read_to_string(&unit.object).unwrap(), "object");
        assert_eq!(unit.lookup(), Some(warning()));

        let good = fs::read(&unit.messages).unwrap();
        let mut oversized = good.clone();
        oversized.resize(usize::try_from(MAX_MESSAGES_BYTES).unwrap() + 1, b' ');
        assert!(serde_json::from_slice::<MessagesFile>(&oversized).is_ok());
        for bad in [
            oversized,
            b"{".to_vec(),
            br#"{"format":1,"output":{"messages":[],"truncated":false},"more":1}"#.to_vec(),
            br#"{"format":2,"output":{"messages":[],"truncated":false}}"#.to_vec(),
        ] {
            fs::write(&unit.messages, &bad).unwrap();
            assert_eq!(unit.lookup(), None);
        }
        fs::write(&unit.messages, &good).unwrap();
        assert!(unit.lookup().is_some());
        fs::remove_file(&unit.object).unwrap();
        assert_eq!(unit.lookup(), None);
        fs::write(&unit.object, "object").unwrap();
        #[cfg(unix)]
        {
            let elsewhere = dir.path().join("elsewhere.json");
            fs::write(&elsewhere, &good).unwrap();
            fs::remove_file(&unit.messages).unwrap();
            std::os::unix::fs::symlink(&elsewhere, &unit.messages).unwrap();
            assert_eq!(unit.lookup(), None);
        }
    }

    /// Pruning deletes the cache's files that are not kept, and never
    /// another file or a link.
    #[test]
    fn pruning_keeps_the_build_s_files_and_anything_else() {
        let dir = tempfile::tempdir().unwrap();
        let kept = CachedUnit::new(dir.path(), dir.path(), "main", &"0".repeat(64));
        let old = CachedUnit::new(dir.path(), dir.path(), "main", &"1".repeat(64));
        let other = CachedUnit::new(dir.path(), dir.path(), "util", &"2".repeat(64));
        for path in [
            &kept.object,
            &kept.messages,
            &old.object,
            &old.messages,
            &other.object,
            &other.messages,
        ] {
            fs::write(path, "x").unwrap();
        }
        for name in ["notes.txt", "main.o", "main-0123.o", "Main-0123456789ab.o"] {
            fs::write(dir.path().join(name), "x").unwrap();
        }
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            let victim = outside.path().join("victim.o");
            fs::write(&victim, "keep me").unwrap();
            std::os::unix::fs::symlink(&victim, dir.path().join("util-0123456789ab.o")).unwrap();
            assert_eq!(prune(dir.path(), [&kept]), 4);
            assert_eq!(fs::read_to_string(&victim).unwrap(), "keep me");
            assert!(fs::symlink_metadata(dir.path().join("util-0123456789ab.o")).is_ok());
        }
        #[cfg(not(unix))]
        assert_eq!(prune(dir.path(), [&kept]), 4);
        assert!(kept.object.exists() && kept.messages.exists());
        for name in ["notes.txt", "main.o", "main-0123.o", "Main-0123456789ab.o"] {
            assert!(dir.path().join(name).exists(), "{name}");
        }
    }

    #[test]
    fn cache_file_names_are_recognised() {
        for name in [
            "main-0123456789ab.o",
            "my-mod-0123456789ab.messages.json",
            "b2c_ide_init-0123456789ab.o",
        ] {
            assert!(is_cache_file_name(name), "{name}");
        }
        for name in [
            "main.o",
            "main-0123456789AB.o",
            "-0123456789ab.o",
            "main-0123456789abc.o",
            "main.messages.json",
            "main.cpp",
            "../main-0123456789ab.o",
        ] {
            assert!(!is_cache_file_name(name), "{name}");
        }
    }
}
