//! The build directory and safe file writing (`docs/spec/07-toolchain-build-run.md` §7.5.1).
//!
//! ```text
//! <cache>/builds/<project folder>/<config>-<optionsHash8>/
//! ├── gen/    generated sources (rewritten only when their content changes)
//! ├── ide/    the IDE-only init unit (never exported)
//! ├── diag/   compiler working directory (SARIF files land here)
//! ├── out/    the final executable
//! ├── tmp/    private TMPDIR/TEMP for the compiler
//! ├── build-manifest.json   what the executable was built from (crate::manifest)
//! └── lock    held by the build that is using the folder; its modification
//!             time is the folder's last use (cache eviction, crate::cache)
//! ```
//!
//! The project folder is the project ID in lower case plus a hash of its
//! exact spelling (`prj_hello-1a2b3c4d`), so IDs that differ only in case
//! never share a folder on a case-insensitive file system, and no ID can name
//! a Windows device (`CON`, `NUL`, `COM1`, …).
//!
//! Directories below the cache root are created one level at a time with
//! `create_dir`, never following a symbolic link or junction, and are
//! owner-only on Unix. File names come only from validated IDs and generated
//! module names, never from user text, and a generated file name that
//! Windows reserves for a device (`con.cpp`, `nul.hpp`, `com1.cpp`, …) is
//! refused on every system, so a project builds the same everywhere
//! (08 §8.6).

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use b2c_ir::ids::ProjectId;
use b2c_ir::source_map::{FileKind, GeneratedProject};
use b2c_process::CancelToken;
use sha2::{Digest as _, Sha256};

/// The lock file's name inside a build folder (also used by `crate::cache`).
const LOCK_FILE: &str = "lock";

/// How long [`BuildDir::try_lock_until`] waits between attempts.
const LOCK_POLL: Duration = Duration::from_millis(20);

/// The stem of the IDE init unit (`ide/b2c_ide_init.cpp`, 07 §7.6.3). A
/// generated file may not take it, so their diagnostics files never clash.
pub(crate) const IDE_INIT_STEM: &str = "b2c_ide_init";

/// A problem preparing the build directory.
#[derive(Debug, thiserror::Error)]
pub enum BuildDirError {
    /// A path that should be a directory is a link, a file or something else.
    #[error("refusing to use {path}: it is a link or not a directory")]
    NotADirectory {
        /// The offending path.
        path: PathBuf,
    },
    /// A path that should be a regular file is a link or something else.
    #[error("refusing to replace {path}: it is a link or not a regular file")]
    NotAFile {
        /// The offending path.
        path: PathBuf,
    },
    /// A generated file had a name the build directory does not accept: not
    /// a plain generated name (a bug in Blocks2Cpp), the IDE init unit's
    /// name, or a name Windows reserves for a device (a module named `con`,
    /// `nul`, `com1`, …).
    #[error(
        "the file name {name:?} cannot be used in the build folder (it is reserved, for example for a Windows device, or not a generated name)"
    )]
    BadFileName {
        /// The rejected name.
        name: String,
    },
    /// A configuration key had characters the build directory does not accept.
    #[error("invalid build configuration key {key:?}")]
    BadConfigKey {
        /// The rejected key.
        key: String,
    },
    /// An I/O operation failed.
    #[error("could not {action} {path}: {source}")]
    Io {
        /// What was being done.
        action: &'static str,
        /// The path involved.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
}

/// The per-user cache root that the app and the command-line tool share
/// (02 §2.7): `%LOCALAPPDATA%\Blocks2Cpp` on Windows, `$XDG_CACHE_HOME/blocks2cpp`
/// or `~/.cache/blocks2cpp` elsewhere. Builds go to its `builds/` folder.
/// (Before M2, Windows used `%LOCALAPPDATA%\Blocks2Cpp\cache`; that folder is
/// abandoned, not migrated.)
///
/// This is [`b2c_store::dirs::cache_root_from_env`]: `None` when the
/// variables it derives from are missing, not absolute or have `..` parts.
pub fn default_cache_root() -> Option<PathBuf> {
    b2c_store::dirs::cache_root_from_env()
}

/// A prepared build directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildDir {
    root: PathBuf,
}

impl BuildDir {
    /// Creates (or reuses) `<cache_root>/builds/<project folder>/<config_key>/`
    /// and its subdirectories.
    ///
    /// `config_key` must match `[a-z0-9-]{1,64}` (for example
    /// `debug-1a2b3c4d`).
    ///
    /// # Errors
    /// Fails when a directory cannot be created, or when any path below the
    /// cache root exists but is a link or not a directory.
    pub fn create(cache_root: &Path, project: &ProjectId, config_key: &str) -> Result<Self, BuildDirError> {
        let key_ok = !config_key.is_empty()
            && config_key.len() <= 64
            && config_key
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !key_ok {
            return Err(BuildDirError::BadConfigKey {
                key: config_key.to_owned(),
            });
        }
        fs::create_dir_all(cache_root).map_err(|source| BuildDirError::Io {
            action: "create the cache folder",
            path: cache_root.to_path_buf(),
            source,
        })?;
        ensure_plain_dir(cache_root)?;

        let mut root = cache_root.to_path_buf();
        for component in ["builds", &project_folder(project), config_key] {
            root.push(component);
            create_private_dir(&root)?;
        }
        let dir = Self { root };
        for sub in [
            dir.gen_dir(),
            dir.ide_dir(),
            dir.diag_dir(),
            dir.out_dir(),
            dir.tmp_dir(),
        ] {
            create_private_dir(&sub)?;
        }
        Ok(dir)
    }

    /// The build directory itself.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Generated sources.
    pub fn gen_dir(&self) -> PathBuf {
        self.root.join("gen")
    }

    /// The IDE-only files: the init unit of IDE builds (07 §7.6.3). Nothing
    /// here is ever shown in the code view or exported.
    pub fn ide_dir(&self) -> PathBuf {
        self.root.join("ide")
    }

    /// The compiler's working directory (diagnostics files land here).
    pub fn diag_dir(&self) -> PathBuf {
        self.root.join("diag")
    }

    /// The final executable's folder.
    pub fn out_dir(&self) -> PathBuf {
        self.root.join("out")
    }

    /// A private temporary folder for the compiler.
    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// Takes the folder's lock, waiting while another build (in this or
    /// another process) holds it. A build holds it from checking whether the
    /// program is up to date until it has recorded the result in its build
    /// manifest, so the executable always matches its manifest. The lock is
    /// released when the returned file is dropped.
    ///
    /// # Errors
    /// Fails when the lock file is a link or not a regular file, or cannot
    /// be opened or locked.
    pub fn lock(&self) -> Result<fs::File, BuildDirError> {
        let file = self.open_lock()?;
        file.lock().map_err(|source| BuildDirError::Io {
            action: "lock",
            path: self.lock_path(),
            source,
        })?;
        Ok(file)
    }

    /// Like [`Self::lock`], but gives up when `cancel` is cancelled: it tries
    /// every 20 ms and returns `Ok(None)` once the token is cancelled. A
    /// cancelled build therefore never waits for another one to finish.
    ///
    /// # Errors
    /// As [`Self::lock`].
    pub fn try_lock_until(&self, cancel: &CancelToken) -> Result<Option<fs::File>, BuildDirError> {
        let file = self.open_lock()?;
        loop {
            if cancel.is_cancelled() {
                return Ok(None);
            }
            match file.try_lock() {
                Ok(()) => return Ok(Some(file)),
                Err(fs::TryLockError::WouldBlock) => std::thread::sleep(LOCK_POLL),
                Err(fs::TryLockError::Error(source)) => {
                    return Err(BuildDirError::Io {
                        action: "lock",
                        path: self.lock_path(),
                        source,
                    });
                }
            }
        }
    }

    /// Marks the folder as used now: sets the lock file's modification time,
    /// which cache eviction reads as the folder's last use (07 §7.5.1).
    /// Creates the lock file if it is missing.
    ///
    /// # Errors
    /// As [`Self::lock`], or when the time cannot be set.
    pub fn touch(&self) -> Result<(), BuildDirError> {
        self.open_lock()?
            .set_modified(SystemTime::now())
            .map_err(|source| BuildDirError::Io {
                action: "set the last use of",
                path: self.lock_path(),
                source,
            })
    }

    /// The lock file.
    fn lock_path(&self) -> PathBuf {
        self.root.join(LOCK_FILE)
    }

    /// Opens (creating it if needed) the lock file for writing, refusing a
    /// link or anything but a regular file in its place.
    fn open_lock(&self) -> Result<fs::File, BuildDirError> {
        let path = self.lock_path();
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(BuildDirError::NotAFile { path });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(BuildDirError::Io {
                    action: "inspect",
                    path,
                    source,
                });
            }
        }
        fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|source| BuildDirError::Io {
                action: "open the lock file",
                path,
                source,
            })
    }

    /// Writes every generated file into [`Self::gen_dir`]; see
    /// [`write_generated_files`].
    ///
    /// # Errors
    /// Fails on an unexpected file name or an I/O error.
    pub fn write_generated(&self, project: &GeneratedProject) -> Result<Vec<PathBuf>, BuildDirError> {
        write_generated_files(&self.gen_dir(), project)
    }
}

/// Writes every generated file into `dir` (which must exist), rewriting a
/// file only when its content changed (so timestamps stay stable), and
/// returns the paths of the source files to compile, in order.
///
/// Every name is checked before anything is written: a name that is not
/// `[a-z0-9_-]{1,64}` plus `.cpp` or `.hpp`, that Windows reserves for a
/// device (`con.cpp`, `com1.hpp`, …) or that is the IDE init unit's
/// (`b2c_ide_init.cpp`) is refused.
///
/// # Errors
/// Fails on such a file name, when a target is a link, or on an I/O error.
pub fn write_generated_files(dir: &Path, project: &GeneratedProject) -> Result<Vec<PathBuf>, BuildDirError> {
    write_generated_tracked(dir, project).map(|written| written.sources)
}

/// What [`write_generated_tracked`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Written {
    /// The source files to compile, in order.
    pub(crate) sources: Vec<PathBuf>,
    /// Whether any file was created or rewritten (an unchanged file is left
    /// alone).
    pub(crate) changed: bool,
}

/// [`write_generated_files`], also reporting whether anything changed on
/// disk.
pub(crate) fn write_generated_tracked(
    dir: &Path,
    project: &GeneratedProject,
) -> Result<Written, BuildDirError> {
    if let Some(file) = project
        .files
        .iter()
        .find(|file| !is_generated_file_name(&file.path))
    {
        return Err(BuildDirError::BadFileName {
            name: file.path.clone(),
        });
    }
    let mut written = Written {
        sources: Vec::new(),
        changed: false,
    };
    for file in &project.files {
        let path = dir.join(&file.path);
        written.changed |= replace_if_changed(&path, file.contents.as_bytes(), false)?;
        if file.kind == FileKind::Source {
            written.sources.push(path);
        }
    }
    Ok(written)
}

/// Creates (or reuses) `<cache_root>/sandbox/<project folder>/`, the working
/// directory for programs whose project chooses the sandbox folder
/// (spec §7.6.2). Created like the build directory: one level at a time,
/// owner-only on Unix, never through a link.
///
/// # Errors
/// Fails when a folder cannot be created, or when a path below the cache
/// root exists but is a link or not a directory.
pub fn sandbox_dir(cache_root: &Path, project: &ProjectId) -> Result<PathBuf, BuildDirError> {
    fs::create_dir_all(cache_root).map_err(|source| BuildDirError::Io {
        action: "create the cache folder",
        path: cache_root.to_path_buf(),
        source,
    })?;
    ensure_plain_dir(cache_root)?;
    let mut dir = cache_root.to_path_buf();
    for component in ["sandbox", &project_folder(project)] {
        dir.push(component);
        create_private_dir(&dir)?;
    }
    Ok(dir)
}

/// The folder name for a project's builds and sandbox (see the module
/// documentation): `[a-z0-9_]{1,32}-[0-9a-f]{8}`.
fn project_folder(project: &ProjectId) -> String {
    let digest = Sha256::digest(project.as_str().as_bytes());
    let hash = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
    format!("{}-{hash:08x}", project.as_str().to_ascii_lowercase())
}

/// Whether `name` is a plain generated file name: `[a-z0-9_-]{1,64}` followed
/// by `.cpp` or `.hpp`, whose stem is neither a Windows device name nor the
/// IDE init unit's.
fn is_generated_file_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".cpp").or_else(|| name.strip_suffix(".hpp")) else {
        return false;
    };
    !stem.is_empty()
        && stem.len() <= 64
        && stem
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        && !is_device_name(stem)
        && stem != IDE_INIT_STEM
}

/// Whether Windows reserves `stem` for a device, so that a file named
/// `<stem>` or `<stem>.<anything>` would open the device instead
/// (`CON`, `PRN`, `AUX`, `NUL`, `COM0`–`COM9`, `LPT0`–`LPT9`; compared
/// without regard to case).
pub(crate) fn is_device_name(stem: &str) -> bool {
    let lower = stem.to_ascii_lowercase();
    match lower.as_bytes() {
        b"con" | b"prn" | b"aux" | b"nul" => true,
        [b'c', b'o', b'm', digit] | [b'l', b'p', b't', digit] => digit.is_ascii_digit(),
        _ => false,
    }
}

/// The permissions for a file replacing one with `existing` permissions
/// (`None` to keep the temporary file's owner-only ones).
fn new_permissions(existing: Option<fs::Permissions>, executable: bool) -> Option<fs::Permissions> {
    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt as _;
        return Some(fs::Permissions::from_mode(0o755));
    }
    #[cfg(not(unix))]
    let _ = executable;
    existing
}

/// Fails unless `path` is a real directory (not a link to one).
fn ensure_plain_dir(path: &Path) -> Result<(), BuildDirError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| BuildDirError::Io {
        action: "inspect",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(BuildDirError::NotADirectory {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

/// A non-recursive directory builder; owner-only (`0700`) on Unix. On Windows
/// new folders inherit the per-user cache folder's ACL.
fn private_dir_builder() -> fs::DirBuilder {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder
    }
    #[cfg(not(unix))]
    {
        fs::DirBuilder::new()
    }
}

/// Creates one directory level (owner-only on Unix), or checks that an
/// existing one is a real directory.
fn create_private_dir(path: &Path) -> Result<(), BuildDirError> {
    match private_dir_builder().create(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => ensure_plain_dir(path),
        Err(source) => Err(BuildDirError::Io {
            action: "create the folder",
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Atomically replaces `path` with `contents` unless it already holds exactly
/// those bytes. Refuses to read through or replace a link.
///
/// # Errors
/// Fails when `path` is a link or not a regular file, or on an I/O error.
pub fn write_if_changed(path: &Path, contents: &[u8]) -> Result<(), BuildDirError> {
    replace_if_changed(path, contents, false).map(drop)
}

/// [`write_if_changed`] for a program: the file is made executable (on Unix,
/// `rwxr-xr-x`).
///
/// # Errors
/// As [`write_if_changed`].
pub fn write_executable(path: &Path, contents: &[u8]) -> Result<(), BuildDirError> {
    replace_if_changed(path, contents, true).map(drop)
}

/// Replaces `path` unless it already holds `contents`; returns whether it
/// wrote the file.
pub(crate) fn replace_if_changed(
    path: &Path,
    contents: &[u8],
    executable: bool,
) -> Result<bool, BuildDirError> {
    let existing_permissions = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(BuildDirError::NotAFile {
                path: path.to_path_buf(),
            });
        }
        Ok(metadata) => {
            let same_len = u64::try_from(contents.len()).is_ok_and(|len| len == metadata.len());
            if same_len && fs::read(path).is_ok_and(|existing| existing == contents) {
                return Ok(false);
            }
            Some(metadata.permissions())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(BuildDirError::Io {
                action: "inspect",
                path: path.to_path_buf(),
                source,
            });
        }
    };

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let io = |action: &'static str| {
        let path = path.to_path_buf();
        move |source| BuildDirError::Io { action, path, source }
    };
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(io("create a temporary file next to"))?;
    temp.write_all(contents).map_err(io("write"))?;
    // The temporary file is owner-only; keep the replaced file's
    // permissions, or make a program executable.
    if let Some(permissions) = new_permissions(existing_permissions, executable) {
        temp.as_file()
            .set_permissions(permissions)
            .map_err(io("set permissions on"))?;
    }
    temp.as_file().sync_all().map_err(io("write"))?;
    temp.persist(path).map_err(|error| BuildDirError::Io {
        action: "replace",
        path: path.to_path_buf(),
        source: error.error,
    })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use b2c_ir::source_map::{GeneratedFile, SourceMap};

    use super::*;

    fn project_id() -> ProjectId {
        ProjectId::new("prj_test").unwrap()
    }

    fn generated(path: &str, contents: &str) -> GeneratedProject {
        GeneratedProject {
            files: vec![GeneratedFile {
                path: path.to_owned(),
                kind: FileKind::Source,
                contents: contents.to_owned(),
            }],
            source_map: SourceMap::default(),
        }
    }

    #[test]
    fn creates_the_layout_and_writes_sources() {
        let cache = tempfile::tempdir().unwrap();
        let dir = BuildDir::create(cache.path(), &project_id(), "debug-0123abcd").unwrap();
        let folder = project_folder(&project_id());
        assert!(folder.starts_with("prj_test-"), "{folder}");
        assert!(dir.root().ends_with(format!("builds/{folder}/debug-0123abcd")));
        for (sub, name) in [
            (dir.gen_dir(), "gen"),
            (dir.ide_dir(), "ide"),
            (dir.diag_dir(), "diag"),
            (dir.out_dir(), "out"),
            (dir.tmp_dir(), "tmp"),
        ] {
            assert!(sub.is_dir(), "{}", sub.display());
            assert_eq!(sub, dir.root().join(name));
        }
        let sources = dir
            .write_generated(&generated("main.cpp", "int main() {}\n"))
            .unwrap();
        assert_eq!(sources, vec![dir.gen_dir().join("main.cpp")]);
        assert_eq!(fs::read_to_string(&sources[0]).unwrap(), "int main() {}\n");
        // Reusing the directory works.
        BuildDir::create(cache.path(), &project_id(), "debug-0123abcd").unwrap();
    }

    #[test]
    fn unchanged_files_are_not_rewritten() {
        let cache = tempfile::tempdir().unwrap();
        let dir = BuildDir::create(cache.path(), &project_id(), "debug").unwrap();
        let project = generated("main.cpp", "int main() {}\n");
        assert!(write_generated_tracked(&dir.gen_dir(), &project).unwrap().changed);
        let path = dir.gen_dir().join("main.cpp");
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let again = write_generated_tracked(&dir.gen_dir(), &project).unwrap();
        assert!(!again.changed);
        assert_eq!(again.sources, vec![path.clone()]);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
        let changed = write_generated_tracked(
            &dir.gen_dir(),
            &generated("main.cpp", "int main() { return 1; }\n"),
        )
        .unwrap();
        assert!(changed.changed);
        assert_eq!(fs::read_to_string(&path).unwrap(), "int main() { return 1; }\n");
    }

    /// One changed file among unchanged ones counts as a change, and headers
    /// are written but not compiled.
    #[test]
    fn any_changed_file_is_reported() {
        let folder = tempfile::tempdir().unwrap();
        let project = |header: &str| GeneratedProject {
            files: vec![
                GeneratedFile {
                    path: String::from("main.cpp"),
                    kind: FileKind::Source,
                    contents: String::from("int main() {}\n"),
                },
                GeneratedFile {
                    path: String::from("main.hpp"),
                    kind: FileKind::Header,
                    contents: header.to_owned(),
                },
            ],
            source_map: SourceMap::default(),
        };
        let first = write_generated_tracked(folder.path(), &project("// a\n")).unwrap();
        assert!(first.changed);
        assert_eq!(first.sources, vec![folder.path().join("main.cpp")]);
        assert!(
            !write_generated_tracked(folder.path(), &project("// a\n"))
                .unwrap()
                .changed
        );
        assert!(
            write_generated_tracked(folder.path(), &project("// b\n"))
                .unwrap()
                .changed
        );
        assert_eq!(
            fs::read_to_string(folder.path().join("main.hpp")).unwrap(),
            "// b\n"
        );
    }

    /// Windows device names (any case) are refused as generated file stems
    /// on every system, and so is the IDE init unit's name; names that only
    /// look similar are fine. Nothing is written when one name is bad.
    #[test]
    fn device_and_reserved_names_are_refused() {
        for stem in [
            "con", "prn", "aux", "nul", "com0", "com1", "com9", "lpt0", "lpt1", "lpt9", "CON", "Nul", "cOm3",
            "LPT4",
        ] {
            assert!(is_device_name(stem), "{stem}");
        }
        for stem in [
            "co",
            "conn",
            "con1",
            "com",
            "comx",
            "com10",
            "lpt",
            "lpt10",
            "nul_",
            "auxiliary",
            "main",
            "",
            "com-",
            "lpta",
        ] {
            assert!(!is_device_name(stem), "{stem}");
        }
        let folder = tempfile::tempdir().unwrap();
        for name in [
            "con.cpp",
            "nul.hpp",
            "com1.cpp",
            "lpt9.hpp",
            "aux.cpp",
            "prn.cpp",
            "b2c_ide_init.cpp",
        ] {
            let project = GeneratedProject {
                files: vec![
                    GeneratedFile {
                        path: String::from("main.cpp"),
                        kind: FileKind::Source,
                        contents: String::new(),
                    },
                    GeneratedFile {
                        path: name.to_owned(),
                        kind: FileKind::Source,
                        contents: String::new(),
                    },
                ],
                source_map: SourceMap::default(),
            };
            assert!(
                matches!(
                    write_generated_files(folder.path(), &project),
                    Err(BuildDirError::BadFileName { name: refused }) if refused == name
                ),
                "{name}"
            );
            assert!(!folder.path().join("main.cpp").exists(), "{name}");
        }
        for name in ["con1.cpp", "com10.hpp", "console.cpp", "b2c_ide_init2.cpp"] {
            write_generated_files(folder.path(), &generated(name, "")).unwrap();
            assert!(folder.path().join(name).is_file(), "{name}");
        }
    }

    #[test]
    fn rejects_bad_file_names_and_keys() {
        let cache = tempfile::tempdir().unwrap();
        let dir = BuildDir::create(cache.path(), &project_id(), "debug").unwrap();
        for name in [
            "../main.cpp",
            "Main.cpp",
            "main.exe",
            ".cpp",
            "a/b.cpp",
            "a\\b.cpp",
            "main.cpp/",
        ] {
            assert!(
                matches!(
                    dir.write_generated(&generated(name, "")),
                    Err(BuildDirError::BadFileName { .. })
                ),
                "{name}"
            );
        }
        for key in ["", "Debug", "../x", "a b", &"a".repeat(65)] {
            assert!(matches!(
                BuildDir::create(cache.path(), &project_id(), key),
                Err(BuildDirError::BadConfigKey { .. })
            ));
        }
    }

    /// Generated names may use lower-case letters, digits, `_` and `-`, with
    /// a stem of up to 64 characters; only sources are returned for compiling.
    #[test]
    fn accepts_every_generated_file_name_shape() {
        let folder = tempfile::tempdir().unwrap();
        let longest = format!("{}.cpp", "a".repeat(64));
        let names = [
            ("main.cpp", FileKind::Source),
            ("b2c_runtime.hpp", FileKind::Header),
            ("module-2.cpp", FileKind::Source),
            ("x9_y-z.hpp", FileKind::Header),
            (longest.as_str(), FileKind::Source),
        ];
        let project = GeneratedProject {
            files: names
                .iter()
                .map(|&(path, kind)| GeneratedFile {
                    path: path.to_owned(),
                    kind,
                    contents: String::new(),
                })
                .collect(),
            source_map: SourceMap::default(),
        };
        let sources = write_generated_files(folder.path(), &project).unwrap();
        let expected: Vec<PathBuf> = ["main.cpp", "module-2.cpp", longest.as_str()]
            .iter()
            .map(|name| folder.path().join(name))
            .collect();
        assert_eq!(sources, expected);
        assert!(folder.path().join("b2c_runtime.hpp").is_file());
        assert!(folder.path().join("x9_y-z.hpp").is_file());

        let too_long = format!("{}.cpp", "a".repeat(65));
        assert!(matches!(
            write_generated_files(folder.path(), &generated(&too_long, "")),
            Err(BuildDirError::BadFileName { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinked_directories_and_files() {
        let cache = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), cache.path().join("builds")).unwrap();
        assert!(matches!(
            BuildDir::create(cache.path(), &project_id(), "debug"),
            Err(BuildDirError::NotADirectory { .. })
        ));

        let cache = tempfile::tempdir().unwrap();
        let dir = BuildDir::create(cache.path(), &project_id(), "debug").unwrap();
        let target = elsewhere.path().join("victim.txt");
        fs::write(&target, "keep me").unwrap();
        std::os::unix::fs::symlink(&target, dir.gen_dir().join("main.cpp")).unwrap();
        assert!(
            dir.write_generated(&generated("main.cpp", "int main() {}\n"))
                .is_err()
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "keep me");
    }

    #[test]
    fn sandbox_folders_are_per_project() {
        let cache = tempfile::tempdir().unwrap();
        let first = sandbox_dir(cache.path(), &project_id()).unwrap();
        assert!(first.is_dir());
        assert!(first.ends_with(format!("sandbox/{}", project_folder(&project_id()))));
        assert_eq!(sandbox_dir(cache.path(), &project_id()).unwrap(), first);
        let other = sandbox_dir(cache.path(), &ProjectId::new("prj_other").unwrap()).unwrap();
        assert_ne!(other, first);
    }

    #[test]
    fn project_folders_never_clash_or_name_devices() {
        let folder = |id: &str| project_folder(&ProjectId::new(id).unwrap());
        // IDs that differ only in case share a folder on Windows and macOS
        // unless the hash tells them apart.
        assert_ne!(folder("prj_A"), folder("prj_a"));
        assert!(folder("prj_A").starts_with("prj_a-"));
        // `CON`, `NUL`, `COM1` and so on are devices on Windows, also with an
        // extension; a folder name with a suffix is not.
        for device in ["CON", "nul", "COM1", "LPT9", "AUX", "PRN"] {
            let name = folder(device);
            assert_eq!(name.len(), device.len() + 9, "{name}");
            assert!(name.starts_with(&format!("{}-", device.to_ascii_lowercase())));
            assert!(
                name.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
            );
        }
        let cache = tempfile::tempdir().unwrap();
        let dir = BuildDir::create(cache.path(), &ProjectId::new("CON").unwrap(), "debug").unwrap();
        assert!(dir.gen_dir().is_dir());
    }

    #[test]
    fn the_lock_is_exclusive() {
        let cache = tempfile::tempdir().unwrap();
        let dir = BuildDir::create(cache.path(), &project_id(), "debug").unwrap();
        let held = dir.lock().unwrap();
        let other = fs::OpenOptions::new()
            .write(true)
            .open(dir.root().join("lock"))
            .unwrap();
        assert!(other.try_lock().is_err());
        drop(held);
        take_released_lock(&other);
    }

    /// Takes a lock that was just released. A process another test starts at
    /// that moment holds a copy of the descriptor, and with it the lock, from
    /// its fork until its exec closes it (the file is close-on-exec), so the
    /// lock can stay taken for a moment after the release.
    fn take_released_lock(file: &fs::File) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match file.try_lock() {
                Ok(()) => return,
                Err(fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("the released lock stays taken: {error:?}"),
            }
        }
    }

    /// `try_lock_until` takes a free lock at once, waits for a held one, and
    /// gives up when its token is cancelled.
    #[test]
    fn try_lock_until_waits_and_can_be_cancelled() {
        let cache = tempfile::tempdir().unwrap();
        let dir = BuildDir::create(cache.path(), &project_id(), "debug").unwrap();
        let cancel = CancelToken::new();
        let held = dir.try_lock_until(&cancel).unwrap().expect("a free lock");
        let other = fs::OpenOptions::new()
            .write(true)
            .open(dir.root().join("lock"))
            .unwrap();
        assert!(other.try_lock().is_err());

        // A waiter gets the lock when it is released.
        let waiter = {
            let dir = dir.clone();
            let cancel = cancel.clone();
            std::thread::spawn(move || dir.try_lock_until(&cancel).map(|lock| lock.is_some()))
        };
        std::thread::sleep(Duration::from_millis(100));
        assert!(!waiter.is_finished());
        drop(held);
        assert!(waiter.join().unwrap().unwrap());

        // A cancelled waiter gives up without the lock.
        let held = dir.lock().unwrap();
        let waiter = {
            let dir = dir.clone();
            let cancel = cancel.clone();
            std::thread::spawn(move || dir.try_lock_until(&cancel).map(|lock| lock.is_some()))
        };
        std::thread::sleep(Duration::from_millis(100));
        assert!(!waiter.is_finished());
        cancel.cancel();
        assert!(!waiter.join().unwrap().unwrap());
        drop(held);
        // An already cancelled token never takes the lock, even a free one.
        assert!(dir.try_lock_until(&cancel).unwrap().is_none());
        take_released_lock(&other);
    }

    /// `touch` creates the lock file when needed and moves its modification
    /// time to now, which is what cache eviction reads.
    #[test]
    fn touch_marks_the_folder_as_used() {
        let cache = tempfile::tempdir().unwrap();
        let dir = BuildDir::create(cache.path(), &project_id(), "debug").unwrap();
        let lock = dir.root().join("lock");
        assert!(!lock.exists());
        dir.touch().unwrap();
        assert!(lock.is_file());
        let old = SystemTime::now() - Duration::from_hours(24 * 40);
        fs::OpenOptions::new()
            .write(true)
            .open(&lock)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(fs::metadata(&lock).unwrap().modified().unwrap(), old);
        let before = SystemTime::now() - Duration::from_secs(1);
        dir.touch().unwrap();
        let touched = fs::metadata(&lock).unwrap().modified().unwrap();
        assert!(touched >= before, "{touched:?} < {before:?}");
        // Touching does not need the lock, so a running build does not block it.
        let _held = dir.lock().unwrap();
        dir.touch().unwrap();
    }

    /// A link or folder where the lock file belongs is refused by every lock
    /// operation, and the link's target is never created or changed.
    #[test]
    fn lock_files_are_never_links_or_folders() {
        let cache = tempfile::tempdir().unwrap();
        let dir = BuildDir::create(cache.path(), &project_id(), "debug").unwrap();
        let lock = dir.root().join("lock");
        fs::create_dir(&lock).unwrap();
        let cancel = CancelToken::new();
        assert!(matches!(dir.lock(), Err(BuildDirError::NotAFile { .. })));
        assert!(matches!(
            dir.try_lock_until(&cancel),
            Err(BuildDirError::NotAFile { .. })
        ));
        assert!(matches!(dir.touch(), Err(BuildDirError::NotAFile { .. })));
        fs::remove_dir(&lock).unwrap();
        #[cfg(unix)]
        {
            let elsewhere = tempfile::tempdir().unwrap();
            let target = elsewhere.path().join("victim");
            std::os::unix::fs::symlink(&target, &lock).unwrap();
            assert!(matches!(dir.lock(), Err(BuildDirError::NotAFile { .. })));
            assert!(matches!(dir.touch(), Err(BuildDirError::NotAFile { .. })));
            assert!(!target.exists());
        }
    }

    /// When the lock file cannot even be looked at (here: its folder is a
    /// file), the step that failed is "inspect", and nothing is opened.
    #[test]
    fn a_lock_that_cannot_be_inspected_is_reported() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("not-a-folder");
        fs::write(&file, "").unwrap();
        let dir = BuildDir { root: file };
        let below_file = dir.lock();
        #[cfg(unix)]
        assert!(
            matches!(
                below_file,
                Err(BuildDirError::Io {
                    action: "inspect",
                    ..
                })
            ),
            "{below_file:?}"
        );
        #[cfg(not(unix))]
        assert!(below_file.is_err());
        assert!(dir.touch().is_err());
        assert!(dir.try_lock_until(&CancelToken::new()).is_err());
    }

    #[test]
    fn programs_are_written_executable_and_never_through_links() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("hello");
        write_executable(&path, b"program").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"program");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o755);
            let victim = folder.path().join("victim.txt");
            fs::write(&victim, "keep me").unwrap();
            let link = folder.path().join("link");
            std::os::unix::fs::symlink(&victim, &link).unwrap();
            assert!(matches!(
                write_executable(&link, b"program"),
                Err(BuildDirError::NotAFile { .. })
            ));
            assert_eq!(fs::read_to_string(&victim).unwrap(), "keep me");
        }
    }

    /// Set in the environment of the child process that reports
    /// [`default_cache_root`] (see [`cache_root_with`]).
    const CACHE_ROOT_PROBE: &str = "B2C_TEST_CACHE_ROOT_PROBE";

    /// Runs this test binary again with exactly the variables `env` (nothing
    /// is inherited; changing this process's environment would race with
    /// other tests), running only [`cache_root_probe`], and returns the cache
    /// root the child found.
    fn cache_root_with(env: &[(&str, &str)]) -> Option<PathBuf> {
        let exe = std::env::current_exe().unwrap();
        let mut command = b2c_process::Command::new(exe, std::env::temp_dir()).unwrap();
        command
            .args(["--exact", "build_dir::tests::cache_root_probe", "--nocapture"])
            .env(CACHE_ROOT_PROBE, "1")
            .envs(env.iter().copied())
            .timeout(std::time::Duration::from_mins(1));
        // Variables the child needs to run at all (Windows) or that a
        // coverage run uses to collect the child's counters.
        for name in ["SystemRoot", "LLVM_PROFILE_FILE"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        let captured = b2c_process::run_captured(&command).unwrap();
        let stdout = String::from_utf8_lossy(&captured.stdout);
        assert!(
            captured.status.success(),
            "{stdout}{}",
            String::from_utf8_lossy(&captured.stderr)
        );
        let reported = stdout
            .lines()
            .find_map(|line| line.strip_prefix("cache root: "))
            .unwrap_or_else(|| panic!("no cache root reported:\n{stdout}"));
        serde_json::from_str(reported).unwrap()
    }

    /// Not a check of its own: in the child process started by
    /// [`cache_root_with`] it prints [`default_cache_root`]. In an ordinary
    /// test run it does nothing.
    #[test]
    fn cache_root_probe() {
        if std::env::var_os(CACHE_ROOT_PROBE).is_some() {
            println!(
                "cache root: {}",
                serde_json::to_string(&default_cache_root()).unwrap()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn cache_root_follows_xdg_cache_home_then_home() {
        let root = cache_root_with;
        assert_eq!(
            root(&[("XDG_CACHE_HOME", "/xdg/cache"), ("HOME", "/home/ada")]),
            Some(PathBuf::from("/xdg/cache/blocks2cpp"))
        );
        // A relative or empty XDG_CACHE_HOME is ignored, as the XDG Base
        // Directory specification says.
        for xdg in ["cache", ""] {
            assert_eq!(
                root(&[("XDG_CACHE_HOME", xdg), ("HOME", "/home/ada")]),
                Some(PathBuf::from("/home/ada/.cache/blocks2cpp")),
                "{xdg:?}"
            );
        }
        assert_eq!(
            root(&[("HOME", "/home/ada")]),
            Some(PathBuf::from("/home/ada/.cache/blocks2cpp"))
        );
        // A value with a `..` part is ignored like a relative one (02 §2.7).
        assert_eq!(
            root(&[("XDG_CACHE_HOME", "/xdg/../cache"), ("HOME", "/home/ada")]),
            Some(PathBuf::from("/home/ada/.cache/blocks2cpp"))
        );
        // No usable variable: no cache root (and never one relative to the
        // working directory). LOCALAPPDATA is for Windows only.
        assert_eq!(root(&[("HOME", "ada")]), None);
        assert_eq!(root(&[("XDG_CACHE_HOME", "cache"), ("HOME", "")]), None);
        assert_eq!(root(&[("LOCALAPPDATA", "/local")]), None);
        assert_eq!(root(&[]), None);
    }

    #[cfg(windows)]
    #[test]
    fn cache_root_follows_localappdata() {
        let root = cache_root_with;
        // The cache root is the app's local folder itself (02 §2.7); the
        // `cache` folder of earlier versions is no longer used.
        assert_eq!(
            root(&[("LOCALAPPDATA", r"C:\Users\Ada\AppData\Local")]),
            Some(PathBuf::from(r"C:\Users\Ada\AppData\Local\Blocks2Cpp"))
        );
        assert_eq!(root(&[("LOCALAPPDATA", r"AppData\Local")]), None);
        assert_eq!(root(&[("LOCALAPPDATA", r"C:\Users\..\Local")]), None);
        assert_eq!(
            root(&[("XDG_CACHE_HOME", r"C:\xdg"), ("HOME", r"C:\Users\Ada")]),
            None
        );
        assert_eq!(root(&[]), None);
    }

    /// Only identical bytes leave a file alone: a change that keeps the
    /// length is written, and a replaced file keeps its permissions.
    #[test]
    fn same_length_changes_are_written() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("main.cpp");
        write_if_changed(&path, b"int a = 1;\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        }
        write_if_changed(&path, b"int b = 2;\n").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"int b = 2;\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o640);
        }
    }

    /// A folder or a file where the other belongs is refused, and other
    /// errors are reported by the step that failed.
    #[test]
    fn wrong_kinds_and_failed_steps_are_reported() {
        let cache = tempfile::tempdir().unwrap();
        let file = cache.path().join("file");
        fs::write(&file, "").unwrap();
        assert!(matches!(
            ensure_plain_dir(&file),
            Err(BuildDirError::NotADirectory { .. })
        ));
        assert!(matches!(
            create_private_dir(&file),
            Err(BuildDirError::NotADirectory { .. })
        ));
        // Below a file, creating the folder itself fails (it is not an
        // existing folder to check).
        assert!(matches!(
            create_private_dir(&file.join("sub")),
            Err(BuildDirError::Io {
                action: "create the folder",
                ..
            })
        ));
        // A folder where a generated file belongs is not replaced.
        assert!(matches!(
            write_if_changed(cache.path(), b"x"),
            Err(BuildDirError::NotAFile { .. })
        ));
        // Below a file, looking at the target fails (on Unix with "not a
        // directory"; Windows reports such a path as not found).
        let below_file = write_if_changed(&file.join("main.cpp"), b"x");
        #[cfg(unix)]
        assert!(
            matches!(
                below_file,
                Err(BuildDirError::Io {
                    action: "inspect",
                    ..
                })
            ),
            "{below_file:?}"
        );
        #[cfg(not(unix))]
        assert!(below_file.is_err());

        // A file where the `builds` folder belongs.
        fs::write(cache.path().join("builds"), "").unwrap();
        assert!(matches!(
            BuildDir::create(cache.path(), &project_id(), "debug"),
            Err(BuildDirError::NotADirectory { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn directories_are_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let cache = tempfile::tempdir().unwrap();
        let dir = BuildDir::create(cache.path(), &project_id(), "debug").unwrap();
        for path in [dir.root().to_path_buf(), dir.gen_dir(), dir.out_dir()] {
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o700);
        }
    }
}
