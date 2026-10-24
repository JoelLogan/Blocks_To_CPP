//! Finding g++ on this computer (spec §7.2).
//!
//! Every input comes from a [`DiscoveryEnv`], so discovery is testable with
//! a fake file system; [`DiscoveryEnv::from_process`] fills it from the real
//! environment.
//!
//! **Never searched:** the folders in [`DiscoveryEnv::excluded`] (the open
//! projects' folders, the build cache and the current directory; the current
//! directory is added by [`DiscoveryEnv::from_process`]), relative `PATH`
//! entries (empty, `.`, `bin`, and on Windows `C:bin` or `\bin`) and, on
//! Windows, network (UNC) folders. This prevents *binary planting*, where a
//! project folder ships its own `g++`.
//!
//! How much of an excluded folder is excluded depends on what it is
//! ([`ExcludedKind`]): a project folder or the build cache with its whole
//! tree, the current directory as that folder only. A project folder that is
//! the user's home folder or a file-system or drive root is excluded as that
//! folder only too, so that saving a project there never hides the
//! compilers installed below it (`~/.local/bin`, `/usr/bin`).
//! [`Exclusions`] applies the same rule to a compiler chosen by hand and at
//! build time. A compiler a search finds but skips because of where it is
//! is reported in [`Discovery::skipped`] with the reason; nothing skipped is
//! ever run.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use b2c_ir::Diagnostic;
use serde::{Deserialize, Serialize};

use crate::codes;
use crate::target::Platform;

/// Highest and lowest versioned name tried on Linux (`g++-16` … `g++-11`).
const NEWEST_VERSIONED: u32 = 16;
const OLDEST_VERSIONED: u32 = 11;

/// Everything discovery looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryEnv {
    /// Which rules apply.
    pub platform: Platform,
    /// `PATH` entries in order, unfiltered (relative ones are skipped).
    pub path: Vec<PathBuf>,
    /// The file-system root the well-known locations are under: `/` on
    /// Linux, the system drive (`C:\`) on Windows. Tests point it at a
    /// temporary folder.
    pub root: PathBuf,
    /// Windows: `%USERPROFILE%` (Scoop installs).
    pub user_profile: Option<PathBuf>,
    /// Windows: `%ProgramData%` (`Chocolatey` installs).
    pub program_data: Option<PathBuf>,
    /// Windows: `%LOCALAPPDATA%` (`WinGet` installs).
    pub local_app_data: Option<PathBuf>,
    /// Windows: MSYS2 installation folders found elsewhere, for example from
    /// the uninstall registry key's `InstallLocation`; their `ucrt64`,
    /// `mingw64` and `mingw32` `bin` folders are searched.
    pub msys2_roots: Vec<PathBuf>,
    /// Further folders to search after the well-known ones.
    pub extra_dirs: Vec<PathBuf>,
    /// Folders never searched, nor accepted as a location of a found
    /// compiler: the open projects' folders, the build cache, the current
    /// directory. How much of each is excluded depends on its kind
    /// ([`Exclusions`]).
    pub excluded: Vec<ExcludedFolder>,
    /// The user's home folder ([`home_folder`]): a project folder that is
    /// this folder is excluded as that folder only.
    pub home: Option<PathBuf>,
}

impl DiscoveryEnv {
    /// Reads `PATH`, `%USERPROFILE%`, `%ProgramData%`, `%LOCALAPPDATA%`,
    /// `%SystemDrive%` and the home folder ([`home_folder`]) from this
    /// process's environment. The current directory is added to `excluded`
    /// ([`ExcludedKind::CurrentDir`]).
    pub fn from_process(mut excluded: Vec<ExcludedFolder>) -> Self {
        let platform = Platform::host();
        if let Ok(current) = std::env::current_dir() {
            excluded.push(ExcludedFolder::current_dir(current));
        }
        let path = std::env::var_os("PATH")
            .map(|value| std::env::split_paths(&value).collect())
            .unwrap_or_default();
        let var = |name: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
        };
        let root = match platform {
            Platform::Linux => PathBuf::from("/"),
            Platform::Windows => {
                let mut drive: OsString = std::env::var_os("SystemDrive").unwrap_or_else(|| "C:".into());
                drive.push("\\");
                PathBuf::from(drive)
            }
        };
        Self {
            platform,
            path,
            root,
            user_profile: var("USERPROFILE"),
            program_data: var("ProgramData"),
            local_app_data: var("LOCALAPPDATA"),
            msys2_roots: Vec::new(),
            extra_dirs: Vec::new(),
            excluded,
            home: home_folder(),
        }
    }
}

/// The user's home folder: `HOME` on Linux, `%USERPROFILE%` on Windows, or
/// `None` when that is not set to an absolute path.
pub fn home_folder() -> Option<PathBuf> {
    home_folder_in(Platform::host(), |name| std::env::var_os(name))
}

/// [`home_folder`] on `platform`, reading the environment variable it names
/// through `var`.
fn home_folder_in(platform: Platform, var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let name = match platform {
        Platform::Linux => "HOME",
        Platform::Windows => "USERPROFILE",
    };
    var(name).map(PathBuf::from).filter(|path| path.is_absolute())
}

/// What kind of folder discovery never searches (spec §7.2). The kind
/// decides how much of the folder is excluded ([`Exclusions`]) and what a
/// skipped compiler's reason says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExcludedKind {
    /// An open project's folder, where a downloaded project could ship its
    /// own "g++": excluded with its whole tree, except that the user's home
    /// folder or a file-system or drive root is excluded as that folder
    /// only.
    Project,
    /// The build cache: excluded with its whole tree.
    Cache,
    /// The process's current directory: excluded as that folder only, so
    /// that starting a program from `/` or from the home folder never hides
    /// the compilers installed below it.
    CurrentDir,
}

/// A folder discovery never searches; a compiler in it is never run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcludedFolder {
    /// The folder, as given (it is compared in canonical form).
    pub path: PathBuf,
    /// What it is.
    pub kind: ExcludedKind,
}

impl ExcludedFolder {
    /// An open project's folder.
    pub fn project(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            kind: ExcludedKind::Project,
        }
    }

    /// The build cache.
    pub fn cache(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            kind: ExcludedKind::Cache,
        }
    }

    /// The process's current directory.
    pub fn current_dir(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            kind: ExcludedKind::CurrentDir,
        }
    }
}

/// Excluded folders ready for matching (spec §7.2): each in canonical form
/// (as given when it does not exist), with how much of it is excluded.
///
/// * A project folder is excluded with its whole tree, unless it is the
///   user's home folder or a root (a folder without a parent: `/`, `C:\`),
///   which is excluded as that folder only.
/// * The build cache is excluded with its whole tree.
/// * The current directory is excluded as that folder only.
///
/// "That folder only" means the files directly in it, not its subfolders.
/// Discovery, a compiler picked by hand and the check before a build all
/// use this one rule.
///
/// ```
/// use std::path::Path;
/// use b2c_toolchain::discovery::{ExcludedFolder, ExcludedKind, Exclusions};
///
/// let home = Path::new("/home/ada");
/// let exclusions = Exclusions::new(
///     &[ExcludedFolder::project(home), ExcludedFolder::project("/home/ada/game")],
///     Some(home),
/// );
/// assert_eq!(exclusions.holding(Path::new("/home/ada/.local/bin/g++")), None);
/// assert_eq!(exclusions.holding(Path::new("/home/ada/g++")), Some(ExcludedKind::Project));
/// assert_eq!(exclusions.holding(Path::new("/home/ada/game/bin/g++")), Some(ExcludedKind::Project));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exclusions {
    rules: Vec<Exclusion>,
}

/// One folder of [`Exclusions`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct Exclusion {
    /// Canonical (or as given when it does not exist).
    folder: PathBuf,
    kind: ExcludedKind,
    /// Whether its subfolders are excluded too.
    tree: bool,
}

impl Exclusions {
    /// The rules for `excluded`; `home` is the user's home folder
    /// ([`home_folder`]), if known.
    pub fn new(excluded: &[ExcludedFolder], home: Option<&Path>) -> Self {
        let home = home.map(canonical_or_given);
        let rules = excluded
            .iter()
            .map(|excluded| {
                let folder = canonical_or_given(&excluded.path);
                let tree = match excluded.kind {
                    ExcludedKind::Project => folder.parent().is_some() && home.as_ref() != Some(&folder),
                    ExcludedKind::Cache => true,
                    ExcludedKind::CurrentDir => false,
                };
                Exclusion {
                    folder,
                    kind: excluded.kind,
                    tree,
                }
            })
            .collect();
        Self { rules }
    }

    /// The kind of the first excluded folder that holds the file `path`
    /// (canonical): a file inside a folder excluded with its tree, or
    /// directly in a folder excluded as that folder only.
    pub fn holding(&self, path: &Path) -> Option<ExcludedKind> {
        self.rules
            .iter()
            .find(|rule| {
                if rule.tree {
                    path.starts_with(&rule.folder)
                } else {
                    path.parent() == Some(rule.folder.as_path())
                }
            })
            .map(|rule| rule.kind)
    }

    /// The kind of the first excluded folder that covers every file
    /// directly in the folder `dir` (canonical): `dir` is inside a folder
    /// excluded with its tree, or is a folder excluded as that folder only.
    pub fn covering_folder(&self, dir: &Path) -> Option<ExcludedKind> {
        self.rules
            .iter()
            .find(|rule| {
                if rule.tree {
                    dir.starts_with(&rule.folder)
                } else {
                    dir == rule.folder
                }
            })
            .map(|rule| rule.kind)
    }
}

/// `path` in canonical form, or as given when that fails (it does not
/// exist): a folder that is gone is still compared by its name.
fn canonical_or_given(path: &Path) -> PathBuf {
    crate::paths::canonical(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Why a search skipped a compiler it found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// It is in an excluded folder of this kind.
    Excluded(ExcludedKind),
    /// It is on a network (UNC) folder (Windows).
    NetworkFolder,
}

/// A compiler a search found but skipped because of where it is. It is
/// never run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skipped {
    /// Canonical absolute path.
    pub path: PathBuf,
    /// The path it was found as (for display).
    pub found_as: PathBuf,
    /// Why it was skipped.
    pub reason: SkipReason,
}

/// What a search found ([`search`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Discovery {
    /// The candidates, in search order.
    pub candidates: Vec<Candidate>,
    /// The compilers found but skipped because of where they are, in search
    /// order, deduplicated by canonical path. A compiler that was also found
    /// somewhere it may be used is a candidate and not listed here.
    pub skipped: Vec<Skipped>,
}

/// How a candidate was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSource {
    /// A `PATH` entry.
    Path,
    /// A well-known installation folder.
    WellKnown,
    /// [`DiscoveryEnv::extra_dirs`] or [`DiscoveryEnv::msys2_roots`].
    Extra,
    /// Chosen explicitly (settings, `b2c build --toolchain`).
    Explicit,
}

/// A g++ found on this computer, not yet probed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// Canonical absolute path (symbolic links resolved): what is run.
    pub path: PathBuf,
    /// The path it was found as (for display), e.g. `/usr/bin/g++`.
    pub found_as: PathBuf,
    /// How it was found.
    pub source: CandidateSource,
    /// Warnings about the location (legacy MinGW, network path).
    pub warnings: Vec<Diagnostic>,
}

/// Finds every g++ candidate, in search order, deduplicated by canonical
/// path (spec §7.2), and the compilers it skips because of where they are:
///
/// * **Linux:** absolute `PATH` entries, then `/usr/bin`, `/usr/local/bin`,
///   `/opt/rh/gcc-toolset-*/root/usr/bin` (newest first) and
///   `/home/linuxbrew/.linuxbrew/bin`; in each folder `g++`, then `g++-16`
///   … `g++-11`. Only executable regular files count.
/// * **Windows:** absolute `PATH` entries, then `C:\msys64\{ucrt64,mingw64,
///   mingw32}\bin`, the [`DiscoveryEnv::msys2_roots`], Scoop
///   (`%USERPROFILE%\scoop\apps\{gcc,mingw,mingw-winlibs}\current\bin`),
///   `Chocolatey`, `WinGet` `WinLibs`, `C:\TDM-GCC-64\bin`, `C:\Strawberry\c\bin`
///   and the legacy `C:\MinGW\bin`; only files named exactly `g++.exe`
///   (never `.bat` or `.cmd`).
///
/// A compiler whose folder or canonical path lies in an excluded folder
/// ([`Exclusions`]), or whose canonical path is on a network folder, is
/// skipped and listed in [`Discovery::skipped`]: its file is looked at, but
/// it is never run. Folders on a network share and relative folders are not
/// looked into at all.
///
/// ```no_run
/// use b2c_toolchain::discovery::{DiscoveryEnv, ExcludedFolder, search};
///
/// let project_folder = ExcludedFolder::project("/home/ada/game");
/// let found = search(&DiscoveryEnv::from_process(vec![project_folder]));
/// for candidate in &found.candidates {
///     println!("{} ({})", candidate.found_as.display(), candidate.path.display());
/// }
/// for skipped in &found.skipped {
///     println!("skipped {} ({:?})", skipped.found_as.display(), skipped.reason);
/// }
/// ```
pub fn search(env: &DiscoveryEnv) -> Discovery {
    let exclusions = Exclusions::new(&env.excluded, env.home.as_deref());
    let mut seen = HashSet::new();
    let mut skipped_seen = HashSet::new();
    let mut found = Discovery::default();
    for (dir, source) in search_dirs(env) {
        if !dir.is_absolute() || is_network_path(&dir) {
            continue;
        }
        let Ok(canonical_dir) = crate::paths::canonical(&dir) else {
            continue;
        };
        // An excluded folder is looked into only to say what it skipped.
        let folder_excluded = exclusions.covering_folder(&canonical_dir);
        for name in file_names(env.platform) {
            let candidate = dir.join(&name);
            let Some(path) = accept(&candidate, env.platform) else {
                continue;
            };
            let skip = folder_excluded
                .or_else(|| exclusions.holding(&path))
                .map(SkipReason::Excluded)
                .or_else(|| is_network_path(&path).then_some(SkipReason::NetworkFolder));
            if let Some(reason) = skip {
                if skipped_seen.insert(path.clone()) {
                    found.skipped.push(Skipped {
                        path,
                        found_as: candidate,
                        reason,
                    });
                }
                continue;
            }
            if !seen.insert(path.clone()) {
                continue;
            }
            let warnings = location_warnings(&path, env.platform);
            found.candidates.push(Candidate {
                path,
                found_as: candidate,
                source,
                warnings,
            });
        }
    }
    found.skipped.retain(|skipped| !seen.contains(&skipped.path));
    found
}

/// The candidates of [`search`], without the skipped compilers.
pub fn discover(env: &DiscoveryEnv) -> Vec<Candidate> {
    search(env).candidates
}

/// Checks a g++ path chosen explicitly (machine settings or
/// `b2c build --toolchain`): it must be absolute and an executable regular
/// file (an `.exe` on Windows). Any file name is allowed, since the user
/// chose it; the probe then checks that it really is GCC.
///
/// # Errors
/// A `B2C-T1002` diagnostic saying what is wrong.
pub fn explicit_candidate(path: &Path, platform: Platform) -> Result<Candidate, Diagnostic> {
    let refuse = |why: &str| {
        codes::error(
            codes::BAD_TOOLCHAIN_PATH,
            format!("The compiler {} cannot be used: {why}.", path.display()),
        )
    };
    if !path.is_absolute() {
        return Err(refuse("give its full (absolute) path"));
    }
    if platform == Platform::Windows
        && !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return Err(refuse(
            "only .exe programs can be used (never .bat or .cmd scripts)",
        ));
    }
    let canonical = crate::paths::canonical(path).map_err(|_| refuse("the file does not exist"))?;
    let Some(canonical) = accept(&canonical, platform) else {
        return Err(refuse("it is not an executable program file"));
    };
    let mut warnings = location_warnings(&canonical, platform);
    if is_network_path(&canonical) {
        warnings.push(codes::warning(
            codes::NETWORK_PATH,
            format!(
                "The compiler {} is on a network folder. Builds will be slower, and anyone who can change that folder can change what runs on this computer.",
                canonical.display()
            ),
        ));
    }
    Ok(Candidate {
        path: canonical,
        found_as: path.to_path_buf(),
        source: CandidateSource::Explicit,
        warnings,
    })
}

/// Folders to search, in order.
fn search_dirs(env: &DiscoveryEnv) -> Vec<(PathBuf, CandidateSource)> {
    let mut dirs: Vec<(PathBuf, CandidateSource)> = env
        .path
        .iter()
        // Relative entries (including "" and ".") would resolve against the
        // current directory: never searched.
        .filter(|entry| entry.is_absolute())
        .map(|entry| (entry.clone(), CandidateSource::Path))
        .collect();
    let root = &env.root;
    match env.platform {
        Platform::Linux => {
            for dir in ["usr/bin", "usr/local/bin"] {
                dirs.push((root.join(dir), CandidateSource::WellKnown));
            }
            for toolset in gcc_toolsets(&root.join("opt/rh")) {
                dirs.push((toolset.join("root/usr/bin"), CandidateSource::WellKnown));
            }
            dirs.push((
                root.join("home/linuxbrew/.linuxbrew/bin"),
                CandidateSource::WellKnown,
            ));
        }
        Platform::Windows => {
            for flavour in ["ucrt64", "mingw64", "mingw32"] {
                dirs.push((
                    root.join("msys64").join(flavour).join("bin"),
                    CandidateSource::WellKnown,
                ));
            }
            for msys in &env.msys2_roots {
                for flavour in ["ucrt64", "mingw64", "mingw32"] {
                    dirs.push((msys.join(flavour).join("bin"), CandidateSource::Extra));
                }
            }
            if let Some(profile) = &env.user_profile {
                for app in ["gcc", "mingw", "mingw-winlibs"] {
                    dirs.push((
                        profile
                            .join("scoop")
                            .join("apps")
                            .join(app)
                            .join("current")
                            .join("bin"),
                        CandidateSource::WellKnown,
                    ));
                }
            }
            if let Some(data) = &env.program_data {
                dirs.push((
                    data.join("chocolatey")
                        .join("lib")
                        .join("mingw")
                        .join("tools")
                        .join("install")
                        .join("mingw64")
                        .join("bin"),
                    CandidateSource::WellKnown,
                ));
            }
            if let Some(local) = &env.local_app_data {
                for package in winlibs_packages(&local.join("Microsoft").join("WinGet").join("Packages")) {
                    dirs.push((package.join("mingw64").join("bin"), CandidateSource::WellKnown));
                }
            }
            for dir in ["TDM-GCC-64\\bin", "Strawberry\\c\\bin", "MinGW\\bin"] {
                let mut path = root.clone();
                for part in dir.split('\\') {
                    path.push(part);
                }
                dirs.push((path, CandidateSource::WellKnown));
            }
        }
    }
    dirs.extend(
        env.extra_dirs
            .iter()
            .map(|dir| (dir.clone(), CandidateSource::Extra)),
    );
    dirs
}

/// `gcc-toolset-N` folders under `/opt/rh`, newest first.
fn gcc_toolsets(opt_rh: &Path) -> Vec<PathBuf> {
    let mut toolsets: Vec<(u32, PathBuf)> = read_dir_names(opt_rh)
        .into_iter()
        .filter_map(|name| {
            let version = name.strip_prefix("gcc-toolset-")?.parse().ok()?;
            Some((version, opt_rh.join(&name)))
        })
        .collect();
    toolsets.sort_by_key(|(version, _)| std::cmp::Reverse(*version));
    toolsets.into_iter().map(|(_, path)| path).collect()
}

/// `BrechtSanders.WinLibs.*` folders, sorted by name.
fn winlibs_packages(packages: &Path) -> Vec<PathBuf> {
    let mut names: Vec<String> = read_dir_names(packages)
        .into_iter()
        .filter(|name| name.starts_with("BrechtSanders.WinLibs."))
        .collect();
    names.sort();
    names.into_iter().map(|name| packages.join(name)).collect()
}

fn read_dir_names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// File names tried in each folder.
fn file_names(platform: Platform) -> Vec<String> {
    match platform {
        Platform::Linux => std::iter::once(String::from("g++"))
            .chain(
                (OLDEST_VERSIONED..=NEWEST_VERSIONED)
                    .rev()
                    .map(|v| format!("g++-{v}")),
            )
            .collect(),
        Platform::Windows => vec![String::from("g++.exe")],
    }
}

/// Canonicalises a candidate and checks that it is an executable regular
/// file. Returns the canonical path.
fn accept(candidate: &Path, platform: Platform) -> Option<PathBuf> {
    let canonical = crate::paths::canonical(candidate).ok()?;
    let metadata = std::fs::metadata(&canonical).ok()?;
    if !metadata.is_file() {
        return None;
    }
    match platform {
        Platform::Linux => is_executable(&metadata).then_some(canonical),
        Platform::Windows => canonical
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
            .then_some(canonical),
    }
}

/// Whether the file may be run: on Unix, an execute bit is set; elsewhere
/// every file counts (Windows decides by the extension, checked by the
/// caller).
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        true
    }
}

/// Windows: whether the path is on a network share (`\\server\share`).
fn is_network_path(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        matches!(
            path.components().next(),
            Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..))
        )
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

/// Warnings about where a compiler lives.
fn location_warnings(path: &Path, platform: Platform) -> Vec<Diagnostic> {
    let mut warnings = Vec::new();
    // The legacy installer puts g++ in `C:\MinGW\bin`; MinGW-w64 flavours
    // use `mingw64` or `mingw32` folders.
    let parent_names: Vec<String> = path
        .ancestors()
        .skip(1)
        .take(2)
        .filter_map(Path::file_name)
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .collect();
    if platform == Platform::Windows && parent_names == ["bin", "mingw"] {
        warnings.push(codes::warning(
            codes::LEGACY_MINGW,
            format!(
                "{} looks like the old MinGW from mingw.org, which is outdated. Install MSYS2 (UCRT64) or WinLibs for a current g++.",
                path.display()
            ),
        ));
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates an executable file.
    fn tool(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[cfg(unix)]
    fn linux_env(root: &Path) -> DiscoveryEnv {
        DiscoveryEnv {
            platform: Platform::Linux,
            path: Vec::new(),
            root: root.to_path_buf(),
            user_profile: None,
            program_data: None,
            local_app_data: None,
            msys2_roots: Vec::new(),
            extra_dirs: Vec::new(),
            excluded: Vec::new(),
            home: None,
        }
    }

    fn names(found: &[Candidate], root: &Path) -> Vec<String> {
        found
            .iter()
            // `/`-separated on every host, so expectations read the same.
            .map(|c| {
                c.found_as
                    .strip_prefix(root)
                    .unwrap()
                    .display()
                    .to_string()
                    .replace('\\', "/")
            })
            .collect()
    }

    #[cfg(unix)]
    #[test]
    fn linux_search_order_and_versioned_names() {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::paths::canonical(dir.path()).unwrap();
        tool(&root.join("usr/bin/g++-13"));
        tool(&root.join("usr/bin/g++"));
        tool(&root.join("usr/local/bin/g++-12"));
        tool(&root.join("opt/rh/gcc-toolset-12/root/usr/bin/g++"));
        tool(&root.join("opt/rh/gcc-toolset-14/root/usr/bin/g++"));
        tool(&root.join("home/linuxbrew/.linuxbrew/bin/g++-15"));
        tool(&root.join("mybin/g++"));
        let mut env = linux_env(&root);
        env.path = vec![root.join("mybin")];
        let found = discover(&env);
        assert_eq!(
            names(&found, &root),
            [
                "mybin/g++",
                "usr/bin/g++",
                "usr/bin/g++-13",
                "usr/local/bin/g++-12",
                "opt/rh/gcc-toolset-14/root/usr/bin/g++",
                "opt/rh/gcc-toolset-12/root/usr/bin/g++",
                "home/linuxbrew/.linuxbrew/bin/g++-15",
            ]
        );
        assert_eq!(found[0].source, CandidateSource::Path);
        assert_eq!(found[1].source, CandidateSource::WellKnown);
    }

    #[cfg(unix)]
    #[test]
    fn relative_path_entries_are_never_searched() {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::paths::canonical(dir.path()).unwrap();
        tool(&root.join("project/g++"));
        let mut env = linux_env(&root);
        env.path = vec![
            PathBuf::new(),
            PathBuf::from("."),
            PathBuf::from("project"),
            PathBuf::from("./project"),
        ];
        assert!(discover(&env).is_empty());
    }

    /// A relative folder from any source (here [`DiscoveryEnv::extra_dirs`])
    /// would resolve against the current directory, which a project can
    /// control: it is skipped even when it leads to a real compiler.
    #[cfg(unix)]
    #[test]
    fn relative_extra_folders_are_never_searched() {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::paths::canonical(dir.path()).unwrap();
        tool(&root.join("bin/g++"));
        // `../../…/<root>/bin`, relative to the current directory.
        let current = crate::paths::canonical(&std::env::current_dir().unwrap()).unwrap();
        let mut relative: PathBuf = current.components().skip(1).map(|_| "..").collect();
        relative.push(root.strip_prefix("/").unwrap().join("bin"));
        assert!(relative.is_relative());
        assert!(relative.join("g++").is_file(), "{}", relative.display());

        let mut env = linux_env(&root);
        env.extra_dirs = vec![relative];
        assert!(discover(&env).is_empty());
        // The same folder given as an absolute path is searched.
        env.extra_dirs = vec![root.join("bin")];
        let found = discover(&env);
        assert_eq!(names(&found, &root), ["bin/g++"]);
        assert_eq!(found[0].source, CandidateSource::Extra);
    }

    #[cfg(unix)]
    #[test]
    fn excluded_folders_are_never_searched_or_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::paths::canonical(dir.path()).unwrap();
        tool(&root.join("project/bin/g++"));
        let mut env = linux_env(&root);
        env.path = vec![root.join("project/bin")];
        env.excluded = vec![ExcludedFolder::project(root.join("project"))];
        let found = search(&env);
        assert!(found.candidates.is_empty());
        // Listed as skipped, with the reason.
        assert_eq!(
            found.skipped,
            [Skipped {
                path: root.join("project/bin/g++"),
                found_as: root.join("project/bin/g++"),
                reason: SkipReason::Excluded(ExcludedKind::Project),
            }]
        );

        // A symlink in a searched folder that points into the project.
        std::fs::create_dir_all(root.join("usr/bin")).unwrap();
        std::os::unix::fs::symlink(root.join("project/bin/g++"), root.join("usr/bin/g++")).unwrap();
        let found = search(&env);
        assert!(found.candidates.is_empty());
        // One file, found twice: listed once, as first found.
        assert_eq!(found.skipped.len(), 1);
        assert_eq!(found.skipped[0].found_as, root.join("project/bin/g++"));
    }

    /// Spec §7.2 (W3-10): started from `/`, discovery found no g++ at all,
    /// because the current directory was excluded with its whole tree. The
    /// current directory, and a project folder that is a root, are now
    /// excluded as that folder only.
    #[cfg(unix)]
    #[test]
    fn excluding_the_root_folder_does_not_exclude_what_is_below_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::paths::canonical(dir.path()).unwrap();
        tool(&root.join("usr/bin/g++"));
        let mut env = linux_env(&root);
        for excluded in [
            ExcludedFolder::current_dir("/"),
            ExcludedFolder::project("/"),
            ExcludedFolder::current_dir(&root),
        ] {
            env.excluded = vec![excluded.clone()];
            let found = search(&env);
            assert_eq!(names(&found.candidates, &root), ["usr/bin/g++"], "{excluded:?}");
            assert!(found.skipped.is_empty(), "{excluded:?}");
        }
    }

    /// The home folder comes from `HOME` on Linux and `%USERPROFILE%` on
    /// Windows (never the other one), and only as an absolute path.
    #[test]
    fn the_home_folder_is_read_from_the_platform_s_variable() {
        // Absolute on the host that runs the test (a Windows path is not
        // absolute on Linux, and the reverse).
        let absolute = |name: &str| std::env::temp_dir().join(name);
        let (home, profile) = (absolute("home-ada"), absolute("profile-ada"));
        let both = |name: &str| match name {
            "HOME" => Some(home.clone().into_os_string()),
            "USERPROFILE" => Some(profile.clone().into_os_string()),
            _ => None,
        };
        assert_eq!(home_folder_in(Platform::Linux, both), Some(home.clone()));
        assert_eq!(home_folder_in(Platform::Windows, both), Some(profile));

        let relative = |_: &str| Some(OsString::from("ada"));
        assert_eq!(home_folder_in(Platform::Linux, relative), None);
        assert_eq!(home_folder_in(Platform::Windows, relative), None);
        let unset = |_: &str| None;
        assert_eq!(home_folder_in(Platform::Linux, unset), None);
        assert_eq!(home_folder_in(Platform::Windows, unset), None);

        // The process's own environment (set on CI runners and desktops).
        let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        let expected = std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute());
        assert_eq!(home_folder(), expected);
    }

    /// Spec §7.2 (W3-10): started from the home folder, or with a project
    /// saved directly in it, discovery skipped `~/.local/bin`. Only the files
    /// directly in the home folder are skipped now, while a project folder
    /// below it is still excluded with its whole tree.
    #[cfg(unix)]
    #[test]
    fn the_home_folder_is_excluded_as_that_folder_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::paths::canonical(dir.path()).unwrap();
        let home = root.join("home/ada");
        tool(&home.join(".local/bin/g++"));
        tool(&home.join("g++"));
        tool(&home.join("game/bin/g++"));
        let mut env = linux_env(&root);
        env.path = vec![home.join(".local/bin"), home.clone(), home.join("game/bin")];
        env.home = Some(home.clone());
        let skipped = |found: &Discovery| -> Vec<(String, SkipReason)> {
            found
                .skipped
                .iter()
                .map(|skipped| {
                    let relative = skipped.found_as.strip_prefix(&root).unwrap();
                    (relative.display().to_string(), skipped.reason)
                })
                .collect()
        };

        env.excluded = vec![
            ExcludedFolder::project(&home),
            ExcludedFolder::project(home.join("game")),
        ];
        let found = search(&env);
        assert_eq!(names(&found.candidates, &root), ["home/ada/.local/bin/g++"]);
        let project = SkipReason::Excluded(ExcludedKind::Project);
        assert_eq!(
            skipped(&found),
            [
                (String::from("home/ada/g++"), project),
                (String::from("home/ada/game/bin/g++"), project)
            ]
        );

        env.excluded = vec![ExcludedFolder::current_dir(&home)];
        let found = search(&env);
        assert_eq!(
            names(&found.candidates, &root),
            ["home/ada/.local/bin/g++", "home/ada/game/bin/g++"]
        );
        assert_eq!(
            skipped(&found),
            [(
                String::from("home/ada/g++"),
                SkipReason::Excluded(ExcludedKind::CurrentDir)
            )]
        );

        // Not known as the home folder: an ordinary project folder, excluded
        // with its whole tree.
        env.home = None;
        env.excluded = vec![ExcludedFolder::project(&home)];
        let found = search(&env);
        assert!(found.candidates.is_empty());
        assert_eq!(found.skipped.len(), 3);
    }

    /// A compiler skipped in one place but found where it may be used is a
    /// candidate, not a skipped one.
    #[cfg(unix)]
    #[test]
    fn a_compiler_found_elsewhere_is_not_listed_as_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::paths::canonical(dir.path()).unwrap();
        tool(&root.join("usr/bin/g++"));
        std::fs::create_dir_all(root.join("game/bin")).unwrap();
        std::os::unix::fs::symlink(root.join("usr/bin/g++"), root.join("game/bin/g++")).unwrap();
        let mut env = linux_env(&root);
        env.path = vec![root.join("game/bin")];
        env.excluded = vec![ExcludedFolder::project(root.join("game"))];
        let found = search(&env);
        assert_eq!(names(&found.candidates, &root), ["usr/bin/g++"]);
        assert!(found.skipped.is_empty(), "{:?}", found.skipped);
        assert_eq!(discover(&env), found.candidates);
    }

    /// The rules on plain paths (Unix spellings; a folder that does not
    /// exist is compared as given).
    #[cfg(unix)]
    #[test]
    fn exclusions_follow_the_folder_rules() {
        let home = Path::new("/home/ada");
        let exclusions = Exclusions::new(
            &[
                ExcludedFolder::project("/"),
                ExcludedFolder::project(home),
                ExcludedFolder::project("/p/proj"),
                ExcludedFolder::cache("/home/ada/.cache/blocks2cpp"),
                ExcludedFolder::current_dir("/work"),
            ],
            Some(home),
        );
        let project = Some(ExcludedKind::Project);
        for (path, expected) in [
            ("/usr/bin/g++", None),
            ("/g++", project),
            ("/home/ada/.local/bin/g++", None),
            ("/home/ada/g++", project),
            ("/p/proj/bin/g++", project),
            ("/p/proj/g++", project),
            ("/p/project2/g++", None),
            (
                "/home/ada/.cache/blocks2cpp/builds/x/g++",
                Some(ExcludedKind::Cache),
            ),
            ("/work/g++", Some(ExcludedKind::CurrentDir)),
            ("/work/bin/g++", None),
        ] {
            assert_eq!(exclusions.holding(Path::new(path)), expected, "{path}");
        }
        for (folder, expected) in [
            ("/", project),
            ("/usr/bin", None),
            ("/home/ada", project),
            ("/home/ada/.local/bin", None),
            ("/p/proj/bin", project),
            ("/work", Some(ExcludedKind::CurrentDir)),
            ("/work/bin", None),
        ] {
            assert_eq!(
                exclusions.covering_folder(Path::new(folder)),
                expected,
                "{folder}"
            );
        }
        assert_eq!(Exclusions::default().holding(Path::new("/g++")), None);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_canonicalised_and_deduplicated() {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::paths::canonical(dir.path()).unwrap();
        tool(&root.join("usr/bin/x86_64-linux-gnu-g++-13"));
        std::os::unix::fs::symlink("x86_64-linux-gnu-g++-13", root.join("usr/bin/g++")).unwrap();
        std::os::unix::fs::symlink("x86_64-linux-gnu-g++-13", root.join("usr/bin/g++-13")).unwrap();
        let mut env = linux_env(&root);
        env.path = vec![root.join("usr/bin"), root.join("usr/bin")];
        let found = discover(&env);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, root.join("usr/bin/x86_64-linux-gnu-g++-13"));
        assert_eq!(found[0].found_as, root.join("usr/bin/g++"));
    }

    #[cfg(unix)]
    #[test]
    fn non_executables_and_directories_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::paths::canonical(dir.path()).unwrap();
        std::fs::create_dir_all(root.join("usr/bin/g++")).unwrap();
        std::fs::create_dir_all(root.join("usr/local/bin")).unwrap();
        std::fs::write(root.join("usr/local/bin/g++"), b"data").unwrap();
        assert!(discover(&linux_env(&root)).is_empty());
    }

    #[test]
    fn windows_well_known_locations() {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::paths::canonical(dir.path()).unwrap();
        let profile = root.join("Users/ada");
        let local = profile.join("AppData/Local");
        let program_data = root.join("ProgramData");
        tool(&root.join("msys64/ucrt64/bin/g++.exe"));
        tool(&root.join("msys64/mingw64/bin/g++.bat"));
        tool(&root.join("msys64/mingw64/bin/g++.cmd"));
        tool(&root.join("D/msys2/ucrt64/bin/g++.exe"));
        tool(&profile.join("scoop/apps/mingw-winlibs/current/bin/g++.exe"));
        tool(&program_data.join("chocolatey/lib/mingw/tools/install/mingw64/bin/g++.exe"));
        tool(
            &local.join("Microsoft/WinGet/Packages/BrechtSanders.WinLibs.POSIX.UCRT_abc/mingw64/bin/g++.exe"),
        );
        tool(&root.join("TDM-GCC-64/bin/g++.exe"));
        tool(&root.join("Strawberry/c/bin/g++.exe"));
        tool(&root.join("MinGW/bin/g++.exe"));
        tool(&root.join("MinGW/bin/gcc.exe"));
        let env = DiscoveryEnv {
            platform: Platform::Windows,
            path: vec![PathBuf::from("relative"), PathBuf::new()],
            root: root.clone(),
            user_profile: Some(profile.clone()),
            program_data: Some(program_data.clone()),
            local_app_data: Some(local.clone()),
            msys2_roots: vec![root.join("D/msys2")],
            extra_dirs: Vec::new(),
            excluded: Vec::new(),
            home: None,
        };
        let found = discover(&env);
        assert_eq!(
            names(&found, &root),
            [
                "msys64/ucrt64/bin/g++.exe",
                "D/msys2/ucrt64/bin/g++.exe",
                "Users/ada/scoop/apps/mingw-winlibs/current/bin/g++.exe",
                "ProgramData/chocolatey/lib/mingw/tools/install/mingw64/bin/g++.exe",
                "Users/ada/AppData/Local/Microsoft/WinGet/Packages/BrechtSanders.WinLibs.POSIX.UCRT_abc/mingw64/bin/g++.exe",
                "TDM-GCC-64/bin/g++.exe",
                "Strawberry/c/bin/g++.exe",
                "MinGW/bin/g++.exe",
            ]
        );
        // Only the legacy MinGW gets a warning.
        let warned: Vec<_> = found.iter().filter(|c| !c.warnings.is_empty()).collect();
        assert_eq!(warned.len(), 1);
        assert_eq!(warned[0].warnings[0].code.0, codes::LEGACY_MINGW);
    }

    /// Network shares (`\\server\share`, also in the verbatim `\\?\UNC\` form)
    /// are never searched; drive paths are ordinary folders.
    #[cfg(windows)]
    #[test]
    fn windows_network_paths_are_recognised() {
        for network in [r"\\server\share\bin\g++.exe", r"\\?\UNC\server\share\bin\g++.exe"] {
            assert!(is_network_path(Path::new(network)), "{network}");
        }
        for local in [r"C:\msys64\ucrt64\bin\g++.exe", r"\\?\C:\msys64\bin", r"\bin"] {
            assert!(!is_network_path(Path::new(local)), "{local}");
        }
    }

    #[test]
    fn explicit_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::paths::canonical(dir.path()).unwrap();
        let gxx = root.join("custom/x86_64-linux-gnu-g++-14");
        tool(&gxx);
        let candidate = explicit_candidate(&gxx, Platform::Linux).unwrap();
        assert_eq!(candidate.source, CandidateSource::Explicit);
        assert_eq!(candidate.path, gxx);

        for bad in [Path::new("g++"), &root.join("missing/g++"), &root.join("custom")] {
            let error = explicit_candidate(bad, Platform::Linux).unwrap_err();
            assert_eq!(error.code.0, codes::BAD_TOOLCHAIN_PATH, "{}", bad.display());
        }
        let script = root.join("custom/g++.bat");
        tool(&script);
        assert!(explicit_candidate(&script, Platform::Windows).is_err());
    }

    /// On Windows only `.exe` programs can be chosen (in any letter case);
    /// a script is refused for being one, before the file is even looked at.
    #[test]
    fn explicit_windows_paths_must_be_exe_programs() {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::paths::canonical(dir.path()).unwrap();
        let exe = root.join("w64/bin/x86_64-w64-mingw32-g++.EXE");
        tool(&exe);
        let candidate = explicit_candidate(&exe, Platform::Windows).unwrap();
        assert_eq!(candidate.path, exe);
        assert_eq!(candidate.found_as, exe);
        assert_eq!(candidate.source, CandidateSource::Explicit);

        let script = root.join("w64/bin/g++.bat");
        tool(&script);
        for refused in [script, root.join("missing/g++.cmd"), root.join("w64/bin/g++")] {
            let error = explicit_candidate(&refused, Platform::Windows).unwrap_err();
            assert_eq!(error.code.0, codes::BAD_TOOLCHAIN_PATH);
            assert!(
                error.message.contains("only .exe programs can be used"),
                "{}: {}",
                refused.display(),
                error.message
            );
        }
    }
}
