//! The build directory and safe file writing (`docs/spec/07-toolchain-build-run.md` §7.5.1).
//!
//! ```text
//! <cache>/builds/<project folder>/<config>-<optionsHash8>/
//! ├── gen/    generated sources (rewritten only when their content changes)
//! ├── diag/   compiler working directory (SARIF files land here)
//! ├── out/    the final executable
//! ├── tmp/    private TMPDIR/TEMP for the compiler
//! └── lock    held by the build that is using the folder
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
//! module names, never from user text.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use b2c_ir::ids::ProjectId;
use b2c_ir::source_map::{FileKind, GeneratedProject};
use sha2::{Digest as _, Sha256};

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
    /// A generated file had a name the build directory does not accept.
    #[error("the generator produced an unexpected file name {name:?} (this is a bug in Blocks2Cpp)")]
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

/// The per-user cache root: `%LOCALAPPDATA%\Blocks2Cpp\cache` on Windows,
/// `$XDG_CACHE_HOME/blocks2cpp` or `~/.cache/blocks2cpp` elsewhere.
///
/// Returns `None` when the relevant environment variables are missing or not
/// absolute paths.
pub fn default_cache_root() -> Option<PathBuf> {
    let absolute = |value: std::ffi::OsString| {
        let path = PathBuf::from(value);
        path.is_absolute().then_some(path)
    };
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .and_then(absolute)
            .map(|base| base.join("Blocks2Cpp").join("cache"))
    } else if let Some(base) = std::env::var_os("XDG_CACHE_HOME").and_then(absolute) {
        Some(base.join("blocks2cpp"))
    } else {
        std::env::var_os("HOME")
            .and_then(absolute)
            .map(|home| home.join(".cache").join("blocks2cpp"))
    }
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
        for sub in [dir.gen_dir(), dir.diag_dir(), dir.out_dir(), dir.tmp_dir()] {
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
    /// program is up to date until it has written the new build stamp, so the
    /// executable always matches its stamp. The lock is released when the
    /// returned file is dropped.
    ///
    /// # Errors
    /// Fails when the lock file cannot be opened or locked.
    pub fn lock(&self) -> Result<fs::File, BuildDirError> {
        let path = self.root.join("lock");
        let io = |action: &'static str| {
            let path = path.clone();
            move |source| BuildDirError::Io { action, path, source }
        };
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(io("open the lock file"))?;
        file.lock().map_err(io("lock"))?;
        Ok(file)
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
/// # Errors
/// Fails on an unexpected file name, when a target is a link, or on an I/O
/// error.
pub fn write_generated_files(dir: &Path, project: &GeneratedProject) -> Result<Vec<PathBuf>, BuildDirError> {
    let mut sources = Vec::new();
    for file in &project.files {
        if !is_generated_file_name(&file.path) {
            return Err(BuildDirError::BadFileName {
                name: file.path.clone(),
            });
        }
        let path = dir.join(&file.path);
        write_if_changed(&path, file.contents.as_bytes())?;
        if file.kind == FileKind::Source {
            sources.push(path);
        }
    }
    Ok(sources)
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
/// by `.cpp` or `.hpp`.
fn is_generated_file_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".cpp").or_else(|| name.strip_suffix(".hpp")) else {
        return false;
    };
    !stem.is_empty()
        && stem.len() <= 64
        && stem
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
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
    replace_if_changed(path, contents, false)
}

/// [`write_if_changed`] for a program: the file is made executable (on Unix,
/// `rwxr-xr-x`).
///
/// # Errors
/// As [`write_if_changed`].
pub fn write_executable(path: &Path, contents: &[u8]) -> Result<(), BuildDirError> {
    replace_if_changed(path, contents, true)
}

fn replace_if_changed(path: &Path, contents: &[u8], executable: bool) -> Result<(), BuildDirError> {
    let existing_permissions = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(BuildDirError::NotAFile {
                path: path.to_path_buf(),
            });
        }
        Ok(metadata) => {
            let same_len = u64::try_from(contents.len()).is_ok_and(|len| len == metadata.len());
            if same_len && fs::read(path).is_ok_and(|existing| existing == contents) {
                return Ok(());
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
    Ok(())
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
        for sub in [dir.gen_dir(), dir.diag_dir(), dir.out_dir(), dir.tmp_dir()] {
            assert!(sub.is_dir(), "{}", sub.display());
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
        dir.write_generated(&project).unwrap();
        let path = dir.gen_dir().join("main.cpp");
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        dir.write_generated(&project).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
        dir.write_generated(&generated("main.cpp", "int main() { return 1; }\n"))
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "int main() { return 1; }\n");
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
        other.try_lock().unwrap();
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
