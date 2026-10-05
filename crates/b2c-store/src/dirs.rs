//! The app's per-user folders (02 §2.7) and creating them safely (08 §8.6).
//!
//! | Folder | Windows | Linux |
//! |--------|---------|-------|
//! | `config`: settings, recent projects | `%APPDATA%\Blocks2Cpp` | `$XDG_CONFIG_HOME/blocks2cpp` |
//! | `machine`: trust store, toolchains | `%LOCALAPPDATA%\Blocks2Cpp` | `$XDG_CONFIG_HOME/blocks2cpp` |
//! | `cache`: `builds/`, `sandbox/` | `%LOCALAPPDATA%\Blocks2Cpp` | `$XDG_CACHE_HOME/blocks2cpp` |
//! | `recovery`: autosave snapshots | `%LOCALAPPDATA%\Blocks2Cpp\recovery` | `$XDG_STATE_HOME/blocks2cpp/recovery` |
//! | `logs` | `%LOCALAPPDATA%\Blocks2Cpp\logs` | `$XDG_STATE_HOME/blocks2cpp/logs` |
//!
//! On Linux (and other Unix systems) an `XDG_*` variable that is unset,
//! empty or not an absolute path is ignored, as the XDG Base Directory
//! specification says, and `~/.config`, `~/.cache` or `~/.local/state` is
//! used instead. Tauri's path resolver is not used: it names the folders
//! after the bundle identifier rather than `Blocks2Cpp`/`blocks2cpp`.
//!
//! Every folder is created by [`ensure_private_dir`]: one level at a time,
//! owner-only (`0700`) on Unix, and never through a link.

use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::error::StoreError;
use crate::fs_checks::is_link_or_reparse_point;

/// The app's folder name on Windows.
#[cfg(windows)]
const APP_FOLDER: &str = "Blocks2Cpp";
/// The app's folder name on Linux and other Unix systems.
#[cfg(not(windows))]
const APP_FOLDER: &str = "blocks2cpp";

/// The app's per-user folders (see the module documentation). Computing
/// them creates nothing; [`Dirs::ensure`] or [`ensure_private_dir`] does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    /// Roaming settings and the recent projects list.
    pub config: PathBuf,
    /// Machine-local data: the trust store and the toolchain list.
    pub machine: PathBuf,
    /// The cache root: build folders ([`Dirs::builds`]) and program sandboxes
    /// ([`Dirs::sandbox`]).
    pub cache: PathBuf,
    /// Autosave and crash-recovery snapshots.
    pub recovery: PathBuf,
    /// Log files.
    pub logs: PathBuf,
}

impl Dirs {
    /// The folders for this user, from the environment.
    ///
    /// # Errors
    /// [`StoreError::Invalid`] when the variables the folders derive from
    /// are missing or not absolute paths: `APPDATA` and `LOCALAPPDATA` on
    /// Windows; on Linux `HOME`, for each XDG variable that is not usable.
    pub fn from_env() -> Result<Self, StoreError> {
        Self::from_lookup(&|name| std::env::var_os(name))
    }

    /// [`Dirs::from_env`] with the environment supplied by the caller (for
    /// tests, which must not change the real environment).
    pub(crate) fn from_lookup(env: &dyn Fn(&str) -> Option<OsString>) -> Result<Self, StoreError> {
        #[cfg(windows)]
        {
            let roaming = absolute_var(env, "APPDATA")
                .ok_or(StoreError::Invalid("APPDATA is not set to an absolute path"))?
                .join(APP_FOLDER);
            let local = absolute_var(env, "LOCALAPPDATA")
                .ok_or(StoreError::Invalid("LOCALAPPDATA is not set to an absolute path"))?
                .join(APP_FOLDER);
            Ok(Self {
                config: roaming,
                machine: local.clone(),
                cache: local.clone(),
                recovery: local.join("recovery"),
                logs: local.join("logs"),
            })
        }
        #[cfg(not(windows))]
        {
            let config = xdg_dir(env, "XDG_CONFIG_HOME", &[".config"])?;
            let state = xdg_dir(env, "XDG_STATE_HOME", &[".local", "state"])?;
            Ok(Self {
                machine: config.clone(),
                config,
                cache: xdg_dir(env, "XDG_CACHE_HOME", &[".cache"])?,
                recovery: state.join("recovery"),
                logs: state.join("logs"),
            })
        }
    }

    /// Every folder below one root, for end-to-end tests and test fixtures:
    /// `<root>/config`, `<root>/machine`, `<root>/cache`,
    /// `<root>/state/recovery` and `<root>/state/logs`.
    pub fn under_root(root: &Path) -> Self {
        Self {
            config: root.join("config"),
            machine: root.join("machine"),
            cache: root.join("cache"),
            recovery: root.join("state").join("recovery"),
            logs: root.join("state").join("logs"),
        }
    }

    /// The build cache: `<cache>/builds`.
    pub fn builds(&self) -> PathBuf {
        self.cache.join("builds")
    }

    /// The program sandboxes: `<cache>/sandbox`.
    pub fn sandbox(&self) -> PathBuf {
        self.cache.join("sandbox")
    }

    /// Creates every folder that does not exist yet with
    /// [`ensure_private_dir`].
    ///
    /// # Errors
    /// As [`ensure_private_dir`], for the first folder that fails.
    pub fn ensure(&self) -> Result<(), StoreError> {
        for dir in [
            &self.config,
            &self.machine,
            &self.cache,
            &self.recovery,
            &self.logs,
        ] {
            ensure_private_dir(dir)?;
        }
        Ok(())
    }
}

/// The cache root alone (the `cache` of [`Dirs::from_env`]), for the
/// command-line tool: `%LOCALAPPDATA%\Blocks2Cpp` on Windows,
/// `$XDG_CACHE_HOME/blocks2cpp` or `~/.cache/blocks2cpp` elsewhere. `None`
/// when the variables it derives from are missing or not absolute.
pub fn cache_root_from_env() -> Option<PathBuf> {
    cache_root_from_lookup(&|name| std::env::var_os(name))
}

/// [`cache_root_from_env`] with the environment supplied by the caller.
pub(crate) fn cache_root_from_lookup(env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        absolute_var(env, "LOCALAPPDATA").map(|local| local.join(APP_FOLDER))
    }
    #[cfg(not(windows))]
    {
        xdg_dir(env, "XDG_CACHE_HOME", &[".cache"]).ok()
    }
}

/// The value of `name` as a path, if it is set to an absolute path.
fn absolute_var(env: &dyn Fn(&str) -> Option<OsString>, name: &str) -> Option<PathBuf> {
    env(name).map(PathBuf::from).filter(|path| path.is_absolute())
}

/// `$<variable>/blocks2cpp`, or `$HOME/<fallback...>/blocks2cpp` when the
/// variable is unset, empty or relative (XDG Base Directory specification).
#[cfg(not(windows))]
fn xdg_dir(
    env: &dyn Fn(&str) -> Option<OsString>,
    variable: &str,
    fallback: &[&str],
) -> Result<PathBuf, StoreError> {
    if let Some(base) = absolute_var(env, variable) {
        return Ok(base.join(APP_FOLDER));
    }
    let mut dir =
        absolute_var(env, "HOME").ok_or(StoreError::Invalid("HOME is not set to an absolute path"))?;
    dir.extend(fallback);
    dir.push(APP_FOLDER);
    Ok(dir)
}

/// Makes sure `path` is an existing, private, real folder.
///
/// * Missing levels are created one at a time with a non-recursive
///   `create_dir`, owner-only (`0700`) on Unix; on Windows they inherit the
///   user-profile ACL of their parent. Creating a level never follows a
///   link: a link that appears at a level being created is refused.
/// * Ancestors that already existed are the user's own layout (a home
///   folder or `~/.config` may well be a link) and are used as they are.
/// * `path` itself must be a real folder: a symbolic link, a junction or
///   another reparse point, or a file, is refused. On Unix it is made
///   `0700` if it is not already; that fails unless it belongs to this user,
///   so a folder planted by someone else is refused too.
///
/// # Errors
/// [`StoreError::Invalid`] when `path` is not absolute or has `.` or `..`
/// parts; [`StoreError::Link`] for a link or non-folder; [`StoreError::Io`]
/// when a folder cannot be inspected, created or made private.
pub fn ensure_private_dir(path: &Path) -> Result<(), StoreError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(StoreError::Invalid(
            "a private folder must be an absolute path without '.' or '..' parts",
        ));
    }
    // Collect the levels that do not exist yet, deepest first.
    let mut missing = Vec::new();
    let mut existing = path;
    loop {
        match fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(existing);
                existing = existing
                    .parent()
                    .ok_or(StoreError::Invalid("the folder's drive or root does not exist"))?;
            }
            // A file where a folder should be, somewhere above.
            Err(error) if error.kind() == std::io::ErrorKind::NotADirectory => {
                return Err(StoreError::link(existing));
            }
            Err(source) => return Err(StoreError::io("inspect the folder", existing, source)),
        }
    }
    if !missing.is_empty() && !fs::metadata(existing).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(StoreError::link(existing));
    }
    for level in missing.iter().rev() {
        create_level(level)?;
    }
    make_private(path)
}

/// Creates one folder level, or accepts one that appeared meanwhile if it
/// is a real folder.
fn create_level(path: &Path) -> Result<(), StoreError> {
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    match builder.create(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => check_real_dir(path),
        Err(source) => Err(StoreError::io("create the folder", path, source)),
    }
}

/// Fails unless `path` is a real folder (not a link or reparse point).
fn check_real_dir(path: &Path) -> Result<(), StoreError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|source| StoreError::io("inspect the folder", path, source))?;
    if is_link_or_reparse_point(&metadata) || !metadata.is_dir() {
        return Err(StoreError::link(path));
    }
    Ok(())
}

/// Checks that `path` is a real folder and, on Unix, makes it `0700`.
fn make_private(path: &Path) -> Result<(), StoreError> {
    check_real_dir(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let metadata = fs::symlink_metadata(path)
            .map_err(|source| StoreError::io("inspect the folder", path, source))?;
        if metadata.permissions().mode() & 0o7777 != 0o700 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .map_err(|source| StoreError::io("make the folder private", path, source))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn lookup(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
            .collect();
        move |name| vars.get(name).cloned()
    }

    #[cfg(not(windows))]
    #[test]
    fn xdg_variables_are_used_when_absolute() {
        let env = lookup(&[
            ("HOME", "/home/ada"),
            ("XDG_CONFIG_HOME", "/xdg/config"),
            ("XDG_CACHE_HOME", "/xdg/cache"),
            ("XDG_STATE_HOME", "/xdg/state"),
        ]);
        let dirs = Dirs::from_lookup(&env).unwrap();
        assert_eq!(
            dirs,
            Dirs {
                config: PathBuf::from("/xdg/config/blocks2cpp"),
                machine: PathBuf::from("/xdg/config/blocks2cpp"),
                cache: PathBuf::from("/xdg/cache/blocks2cpp"),
                recovery: PathBuf::from("/xdg/state/blocks2cpp/recovery"),
                logs: PathBuf::from("/xdg/state/blocks2cpp/logs"),
            }
        );
        assert_eq!(dirs.builds(), PathBuf::from("/xdg/cache/blocks2cpp/builds"));
        assert_eq!(dirs.sandbox(), PathBuf::from("/xdg/cache/blocks2cpp/sandbox"));
        assert_eq!(
            cache_root_from_lookup(&env),
            Some(PathBuf::from("/xdg/cache/blocks2cpp"))
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn home_fallbacks_replace_unset_empty_and_relative_values() {
        for (config, cache, state) in [("", "", ""), ("relative/config", "./cache", "state")] {
            let mut vars = vec![("HOME", "/home/ada")];
            for (name, value) in [
                ("XDG_CONFIG_HOME", config),
                ("XDG_CACHE_HOME", cache),
                ("XDG_STATE_HOME", state),
            ] {
                vars.push((name, value));
            }
            let dirs = Dirs::from_lookup(&lookup(&vars)).unwrap();
            assert_eq!(dirs.config, PathBuf::from("/home/ada/.config/blocks2cpp"));
            assert_eq!(dirs.machine, dirs.config);
            assert_eq!(dirs.cache, PathBuf::from("/home/ada/.cache/blocks2cpp"));
            assert_eq!(
                dirs.recovery,
                PathBuf::from("/home/ada/.local/state/blocks2cpp/recovery")
            );
            assert_eq!(dirs.logs, PathBuf::from("/home/ada/.local/state/blocks2cpp/logs"));
        }
        let unset = Dirs::from_lookup(&lookup(&[("HOME", "/home/ada")])).unwrap();
        assert_eq!(unset.cache, PathBuf::from("/home/ada/.cache/blocks2cpp"));
    }

    #[cfg(not(windows))]
    #[test]
    fn a_missing_or_relative_home_is_an_error() {
        assert!(matches!(
            Dirs::from_lookup(&lookup(&[])),
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            Dirs::from_lookup(&lookup(&[("HOME", "home/ada")])),
            Err(StoreError::Invalid(_))
        ));
        assert_eq!(cache_root_from_lookup(&lookup(&[("HOME", "")])), None);
        // With every XDG variable absolute, HOME is not needed.
        let env = lookup(&[
            ("XDG_CONFIG_HOME", "/c"),
            ("XDG_CACHE_HOME", "/k"),
            ("XDG_STATE_HOME", "/s"),
        ]);
        assert!(Dirs::from_lookup(&env).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn windows_folders_follow_appdata_and_localappdata() {
        let env = lookup(&[
            ("APPDATA", r"C:\Users\Ada\AppData\Roaming"),
            ("LOCALAPPDATA", r"C:\Users\Ada\AppData\Local"),
            ("XDG_CONFIG_HOME", r"C:\ignored"),
        ]);
        let dirs = Dirs::from_lookup(&env).unwrap();
        assert_eq!(
            dirs.config,
            PathBuf::from(r"C:\Users\Ada\AppData\Roaming\Blocks2Cpp")
        );
        assert_eq!(
            dirs.machine,
            PathBuf::from(r"C:\Users\Ada\AppData\Local\Blocks2Cpp")
        );
        assert_eq!(dirs.cache, dirs.machine);
        assert_eq!(
            dirs.recovery,
            PathBuf::from(r"C:\Users\Ada\AppData\Local\Blocks2Cpp\recovery")
        );
        assert_eq!(
            dirs.logs,
            PathBuf::from(r"C:\Users\Ada\AppData\Local\Blocks2Cpp\logs")
        );
        assert_eq!(
            dirs.builds(),
            PathBuf::from(r"C:\Users\Ada\AppData\Local\Blocks2Cpp\builds")
        );
        assert_eq!(
            cache_root_from_lookup(&env),
            Some(PathBuf::from(r"C:\Users\Ada\AppData\Local\Blocks2Cpp"))
        );
        for vars in [
            &[("LOCALAPPDATA", r"C:\Users\Ada\AppData\Local")][..],
            &[("APPDATA", r"relative"), ("LOCALAPPDATA", r"C:\x")][..],
            &[("APPDATA", r"C:\x")][..],
        ] {
            assert!(matches!(
                Dirs::from_lookup(&lookup(vars)),
                Err(StoreError::Invalid(_))
            ));
        }
    }

    #[test]
    fn under_root_puts_everything_below_the_root() {
        let dirs = Dirs::under_root(Path::new("/e2e"));
        assert_eq!(dirs.config, Path::new("/e2e").join("config"));
        assert_eq!(dirs.machine, Path::new("/e2e").join("machine"));
        assert_eq!(dirs.cache, Path::new("/e2e").join("cache"));
        assert_eq!(dirs.recovery, Path::new("/e2e").join("state").join("recovery"));
        assert_eq!(dirs.logs, Path::new("/e2e").join("state").join("logs"));
    }

    #[test]
    fn ensure_creates_every_level() {
        let root = tempfile::tempdir().unwrap();
        let dirs = Dirs::under_root(&root.path().join("a").join("b"));
        dirs.ensure().unwrap();
        for dir in [
            &dirs.config,
            &dirs.machine,
            &dirs.cache,
            &dirs.recovery,
            &dirs.logs,
        ] {
            assert!(dir.is_dir(), "{}", dir.display());
        }
        // Again: nothing to do.
        dirs.ensure().unwrap();
    }

    #[test]
    fn relative_and_dotted_paths_are_refused() {
        let root = tempfile::tempdir().unwrap();
        for path in [
            Path::new("relative/dir"),
            &root.path().join("a").join("..").join("b"),
        ] {
            assert!(
                matches!(ensure_private_dir(path), Err(StoreError::Invalid(_))),
                "{}",
                path.display()
            );
        }
    }

    #[test]
    fn a_file_in_the_way_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("file");
        fs::write(&file, "x").unwrap();
        assert!(matches!(ensure_private_dir(&file), Err(StoreError::Link { .. })));
        assert!(matches!(
            ensure_private_dir(&file.join("below")),
            Err(StoreError::Link { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn folders_are_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = tempfile::tempdir().unwrap();
        let dirs = Dirs::under_root(&root.path().join("new"));
        dirs.ensure().unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o7777;
        for dir in [
            root.path().join("new"),
            root.path().join("new").join("state"),
            dirs.config.clone(),
            dirs.recovery.clone(),
            dirs.logs.clone(),
        ] {
            assert_eq!(mode(&dir), 0o700, "{}", dir.display());
        }
        // An existing folder of ours with wider permissions is tightened.
        fs::set_permissions(&dirs.cache, fs::Permissions::from_mode(0o755)).unwrap();
        ensure_private_dir(&dirs.cache).unwrap();
        assert_eq!(mode(&dirs.cache), 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn links_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let link = root.path().join("blocks2cpp");
        std::os::unix::fs::symlink(elsewhere.path(), &link).unwrap();
        assert!(matches!(ensure_private_dir(&link), Err(StoreError::Link { .. })));
        // A link to a file, and a dangling link, too.
        let file_link = root.path().join("file-link");
        std::os::unix::fs::symlink(root.path().join("missing"), &file_link).unwrap();
        assert!(matches!(
            ensure_private_dir(&file_link),
            Err(StoreError::Link { .. })
        ));
        // Nothing was created through the link.
        assert_eq!(fs::read_dir(elsewhere.path()).unwrap().count(), 0);
        // An existing ancestor that is a link is the user's own layout.
        let below = link.join("config");
        ensure_private_dir(&below).unwrap();
        assert!(elsewhere.path().join("config").is_dir());
    }
}
