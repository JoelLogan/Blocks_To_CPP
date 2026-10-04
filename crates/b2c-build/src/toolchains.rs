//! Choosing a compiler: discovery, probing and the probe cache
//! (`docs/spec/07-toolchain-build-run.md` §7.2–7.3).
//!
//! Probing a g++ compiles and runs a few dozen tiny programs, so results are
//! kept in `<cache>/toolchains.json`, keyed by the compiler's canonical path.
//! A cached result is used only while the compiler's fingerprint (size,
//! modification time and SHA-256) is unchanged; otherwise it is probed again.

use std::path::{Path, PathBuf};

use b2c_ir::{DiagSource, Diagnostic, Location, Severity};
use b2c_toolchain::codes::NOT_RUNNABLE;
use b2c_toolchain::discovery::{Candidate, DiscoveryEnv, discover, explicit_candidate};
use b2c_toolchain::probe::{ProbeOptions, Toolchain, probe};
use b2c_toolchain::target::Platform;
use serde::Serialize;

/// The probe cache file inside the cache root.
const CACHE_FILE: &str = "toolchains.json";

/// The largest probe cache read (a few kilobytes per compiler).
const MAX_CACHE_BYTES: u64 = 4 * 1024 * 1024;

/// Which compiler to use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolchainChoice {
    /// The first usable g++ found on this computer.
    Auto,
    /// This g++ (an absolute path).
    Path(PathBuf),
}

/// Probes a compiler, turning a failure to check it at all into a
/// diagnostic.
fn probe_candidate(candidate: &Candidate) -> Result<Toolchain, Diagnostic> {
    probe(&candidate.path, &ProbeOptions::default()).map_err(|error| {
        Diagnostic::error(
            NOT_RUNNABLE,
            DiagSource::Toolchain,
            Location::project(),
            format!(
                "The compiler {} could not be checked: {error}.",
                candidate.found_as.display()
            ),
        )
    })
}

/// Probe results remembered between runs.
struct ProbeCache {
    file: PathBuf,
    entries: Vec<Toolchain>,
    changed: bool,
}

impl ProbeCache {
    /// Reads the cache; a missing, unreadable or malformed file is an empty
    /// cache.
    fn open(cache_root: &Path) -> Self {
        let file = cache_root.join(CACHE_FILE);
        let entries = std::fs::symlink_metadata(&file)
            .ok()
            .filter(|metadata| metadata.is_file() && metadata.len() <= MAX_CACHE_BYTES)
            .and_then(|_| std::fs::read(&file).ok())
            .and_then(|bytes| serde_json::from_slice::<Vec<Toolchain>>(&bytes).ok())
            .unwrap_or_default();
        Self {
            file,
            entries,
            changed: false,
        }
    }

    /// The probe result for `candidate`, from the cache when it is still
    /// current, otherwise by probing (and remembering) it.
    fn toolchain(&mut self, candidate: &Candidate) -> Result<Toolchain, Diagnostic> {
        if let Some(cached) = self
            .entries
            .iter()
            .find(|toolchain| toolchain.path() == candidate.path && toolchain.is_current())
        {
            return Ok(cached.clone());
        }
        let probed = probe_candidate(candidate)?;
        self.entries
            .retain(|toolchain| toolchain.path() != candidate.path);
        self.entries.push(probed.clone());
        self.changed = true;
        Ok(probed)
    }

    /// Writes the cache back if it changed. Failing to save it only means
    /// probing again next time, so errors are ignored.
    fn save(&self) {
        if !self.changed {
            return;
        }
        if let Some(parent) = self.file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_vec_pretty(&self.entries) {
            let _ = crate::build_dir::write_if_changed(&self.file, &json);
        }
    }
}

/// The result of choosing a toolchain.
pub(crate) enum Selected {
    /// A usable toolchain, plus warnings and notes about it.
    Usable(Box<Toolchain>, Vec<Diagnostic>),
    /// No usable toolchain; the diagnostics say why.
    Unusable(Vec<Diagnostic>),
}

/// The warnings and notes to show with a usable toolchain.
fn usable(candidate: &Candidate, toolchain: Toolchain) -> Selected {
    let mut notes = candidate.warnings.clone();
    notes.extend(toolchain.problems.iter().cloned());
    Selected::Usable(Box::new(toolchain), notes)
}

/// Chooses the toolchain for a build.
pub(crate) fn select(choice: &ToolchainChoice, cache_root: &Path) -> Selected {
    let mut cache = ProbeCache::open(cache_root);
    let selected = match choice {
        ToolchainChoice::Path(path) => match explicit_candidate(path, Platform::host()) {
            Err(diagnostic) => Selected::Unusable(vec![diagnostic]),
            Ok(candidate) => match cache.toolchain(&candidate) {
                Ok(toolchain) if toolchain.is_usable() => usable(&candidate, toolchain),
                Ok(toolchain) => {
                    let mut problems = candidate.warnings;
                    problems.extend(toolchain.problems);
                    Selected::Unusable(problems)
                }
                Err(problem) => Selected::Unusable(vec![problem]),
            },
        },
        ToolchainChoice::Auto => {
            let candidates = discover(&DiscoveryEnv::from_process(vec![cache_root.to_path_buf()]));
            // Why each compiler that was found was rejected, so the user can
            // fix the one they meant to use.
            let mut rejected = Vec::new();
            let mut chosen = None;
            for candidate in &candidates {
                match cache.toolchain(candidate) {
                    Ok(toolchain) if toolchain.is_usable() => {
                        chosen = Some(usable(candidate, toolchain));
                        break;
                    }
                    Ok(toolchain) => rejected.extend(
                        toolchain
                            .problems
                            .into_iter()
                            .filter(|problem| problem.severity == Severity::Error),
                    ),
                    Err(problem) => rejected.push(problem),
                }
            }
            chosen.unwrap_or_else(|| {
                let mut problems = vec![b2c_toolchain::codes::no_toolchain()];
                problems.extend(rejected);
                Selected::Unusable(problems)
            })
        }
    };
    cache.save();
    selected
}

/// One compiler found on this computer, for `b2c toolchains`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolchainReport {
    /// The path it was found as.
    pub path: PathBuf,
    /// Its GCC version, when known.
    pub version: Option<String>,
    /// Its target triple, when known.
    pub target: Option<String>,
    /// The C++ standards it supports (e.g. `c++20`).
    pub standards: Vec<String>,
    /// Whether builds can use it.
    pub usable: bool,
    /// Problems and notes about it.
    pub problems: Vec<Diagnostic>,
}

/// Finds and checks every compiler on this computer.
pub fn list_toolchains(cache_root: Option<&Path>) -> Vec<ToolchainReport> {
    let excluded: Vec<PathBuf> = cache_root.map(Path::to_path_buf).into_iter().collect();
    let candidates = discover(&DiscoveryEnv::from_process(excluded));
    let mut cache = cache_root.map(ProbeCache::open);
    let reports = candidates
        .iter()
        .map(|candidate| {
            let toolchain = match cache.as_mut() {
                Some(cache) => cache.toolchain(candidate),
                None => probe_candidate(candidate),
            };
            report(candidate, toolchain)
        })
        .collect();
    if let Some(cache) = &cache {
        cache.save();
    }
    reports
}

fn report(candidate: &Candidate, toolchain: Result<Toolchain, Diagnostic>) -> ToolchainReport {
    let mut problems = candidate.warnings.clone();
    let toolchain = match toolchain {
        Ok(toolchain) => toolchain,
        Err(problem) => {
            problems.push(problem);
            return ToolchainReport {
                path: candidate.found_as.clone(),
                version: None,
                target: None,
                standards: Vec::new(),
                usable: false,
                problems,
            };
        }
    };
    let standards = &toolchain.capabilities.standards;
    let supported = [
        &standards.cpp17,
        &standards.cpp20,
        &standards.cpp23,
        &standards.cpp26,
    ]
    .into_iter()
    .flatten()
    .cloned()
    .collect();
    problems.extend(toolchain.problems.iter().cloned());
    ToolchainReport {
        path: candidate.found_as.clone(),
        version: toolchain.version.map(|version| version.to_string()),
        target: Some(toolchain.target.triple.clone()).filter(|triple| !triple.is_empty()),
        standards: supported,
        usable: toolchain.is_usable(),
        problems,
    }
}
