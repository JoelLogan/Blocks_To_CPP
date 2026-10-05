//! Helpers shared by the build session tests: the real g++, fake compilers,
//! example projects and event recording.

// Test helpers fail the test by panicking; not every test file uses every
// helper.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use b2c_build::{BuildJob, Configuration, FrontendOptions, ToolchainForBuild};
use b2c_ipc::dto::{BuildEvent, BuildOutcome};
use b2c_ipc::sink::testing::RecordingSink;
use b2c_toolchain::fingerprint::Fingerprint;
use b2c_toolchain::probe::{ProbeOptions, Toolchain, probe};

/// The repository root.
pub(crate) fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The bytes of `examples/<name>.b2c`.
pub(crate) fn example(name: &str) -> Vec<u8> {
    std::fs::read(repo_root().join("examples").join(format!("{name}.b2c"))).unwrap()
}

/// `examples/<name>.b2c` as JSON, to change before building.
pub(crate) fn example_json(name: &str) -> serde_json::Value {
    serde_json::from_slice(&example(name)).unwrap()
}

/// JSON back to project bytes.
pub(crate) fn bytes(document: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec_pretty(document).unwrap()
}

/// The g++ on `PATH`, if any; panics when there is none but
/// `B2C_REQUIRE_GXX` is set (as in CI).
pub(crate) fn gxx() -> Option<PathBuf> {
    let name = if cfg!(windows) { "g++.exe" } else { "g++" };
    let found = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    });
    assert!(
        found.is_some() || std::env::var_os("B2C_REQUIRE_GXX").is_none(),
        "B2C_REQUIRE_GXX is set but g++ was not found"
    );
    found.map(|path| path.canonicalize().unwrap())
}

/// The probed g++ on `PATH` (probed once per test binary), or `None`
/// without one.
pub(crate) fn real_toolchain() -> Option<Toolchain> {
    static PROBED: OnceLock<Option<Toolchain>> = OnceLock::new();
    PROBED
        .get_or_init(|| {
            let path = gxx()?;
            let toolchain = probe(&path, &ProbeOptions::default()).unwrap();
            assert!(toolchain.is_usable(), "{:#?}", toolchain.problems);
            Some(toolchain)
        })
        .clone()
}

/// A ready toolchain for a build.
pub(crate) fn ready(toolchain: Toolchain) -> ToolchainForBuild {
    ToolchainForBuild::Ready {
        toolchain: Box::new(toolchain),
        notes: Vec::new(),
    }
}

/// A build of `document` with the given toolchain: debug, IDE helpers on,
/// indent width 4.
pub(crate) fn job(project_key: &str, document: Vec<u8>, toolchain: ToolchainForBuild) -> BuildJob {
    BuildJob {
        project_key: project_key.to_owned(),
        document,
        configuration: Configuration::Debug,
        toolchain,
        frontend: FrontendOptions::default(),
        ide: true,
    }
}

/// Writes an executable shell script and returns its canonical path.
#[cfg(unix)]
pub(crate) fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let path = dir.join(name);
    // Written next to its final name and renamed, so no process ever runs a
    // half-written script.
    let temp = dir.join(format!(".{name}.tmp"));
    std::fs::write(&temp, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::rename(&temp, &path).unwrap();
    path.canonicalize().unwrap()
}

/// The real toolchain, but with its driver replaced by `script` (which
/// usually ends by running the real g++). Its fingerprint is the script's,
/// so it is current until the script changes.
pub(crate) fn fake_toolchain(real: &Toolchain, script: &Path) -> Toolchain {
    let mut fake = real.clone();
    fake.fingerprint = Fingerprint::compute(script).unwrap();
    fake
}

/// A recording event sink.
pub(crate) fn sink() -> Arc<RecordingSink<BuildEvent>> {
    Arc::new(RecordingSink::new())
}

/// Waits for the `finished` event (at most `timeout`) and returns every
/// event so far.
pub(crate) fn wait_finished(sink: &RecordingSink<BuildEvent>, timeout: Duration) -> Vec<BuildEvent> {
    let started = Instant::now();
    loop {
        let events = sink.events();
        if events
            .iter()
            .any(|event| matches!(event, BuildEvent::Finished { .. }))
        {
            return events;
        }
        assert!(
            started.elapsed() < timeout,
            "no finished event after {timeout:?}: {events:#?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The outcome and project hash of the one `finished` event, which must be
/// the last event.
pub(crate) fn outcome(events: &[BuildEvent]) -> (BuildOutcome, Option<String>) {
    let finished: Vec<&BuildEvent> = events
        .iter()
        .filter(|event| matches!(event, BuildEvent::Finished { .. }))
        .collect();
    assert_eq!(finished.len(), 1, "{events:#?}");
    match events.last() {
        Some(BuildEvent::Finished {
            outcome,
            project_hash,
            ..
        }) => (*outcome, project_hash.clone()),
        other => panic!("the last event is not finished: {other:?}"),
    }
}

/// Every diagnostic in the events.
pub(crate) fn diagnostics(events: &[BuildEvent]) -> Vec<b2c_ipc::diag::Diagnostic> {
    events
        .iter()
        .filter_map(|event| match event {
            BuildEvent::Diagnostics { items } => Some(items.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// The progress events as `(stage, done, total)`.
pub(crate) fn progress(events: &[BuildEvent]) -> Vec<(String, u32, u32)> {
    events
        .iter()
        .filter_map(|event| match event {
            BuildEvent::Progress { stage, done, total } => Some((stage.as_str().to_owned(), *done, *total)),
            _ => None,
        })
        .collect()
}

/// How long a build may take in these tests (CI machines are slow, and a
/// changed compiler is probed again).
pub(crate) const BUILD_TIMEOUT: Duration = Duration::from_mins(3);
