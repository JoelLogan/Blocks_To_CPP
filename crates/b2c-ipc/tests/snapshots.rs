//! Snapshots of the JSON of every error variant, every channel event and every
//! response, each also checked to read back into the same value. A change to any
//! of these is a change to the IPC contract (bump `IPC_VERSION` when it breaks).

// Test code: unwrap/expect/panic are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::fmt::{Debug, Write as _};

use b2c_ipc::diag::{DiagSource, Diagnostic, Location, Part, Severity};
use b2c_ipc::dto::BuildCacheSettings;
use b2c_ipc::dto::{
    AppEvent, AppInfo, BuildCacheClearResponse, BuildEvent, BuildOutcome, BuildStage, BuildStartResponse,
    CodeStyle, ConsoleSettings, Containment, CppStandard, Crash, Distro, Empty, ExitStatus, IndentWidth,
    NewProjectSettings, NoticeReason, OnErrors, Platform, ProjectNewResponse, ProjectOpenDialogResponse,
    ProjectOpened, ProjectReloadResponse, ProjectSaveAsDialogResponse, ProjectSaveResponse, ProjectSavedAs,
    RecentEntry, RecentListResponse, RecoveryListResponse, RecoveryRestoreResponse, RestrictedReason,
    RunEvent, RunMode, RunSettings, RunStartResponse, SanitizerKind, SanitizerReport, SanitizerTool,
    Settings, SettingsGetResponse, SettingsNotice, SettingsUpdateResponse, SnapshotInfo, Toolchain,
    ToolchainAddDialogResponse, ToolchainCapabilities, ToolchainListResponse, ToolchainSettings,
    ToolchainSetupInfo, ToolchainSource, Trust, TrustResponse, TrustSource, TrustState,
};
use b2c_ipc::{BuildId, Handle, InvalidReason, IoKind, IpcError, RecentId, RunId, SnapshotId, ToolchainId};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Serialises `value`, checks that it reads back unchanged, and returns its JSON.
fn round_trip<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) -> Value {
    let json = serde_json::to_value(value).unwrap();
    let back: T = serde_json::from_value(json.clone()).unwrap_or_else(|e| panic!("{value:?}: {e}"));
    assert_eq!(&back, value);
    json
}

/// `value` with every object's keys inserted in sorted order, so the snapshot does
/// not depend on whether `serde_json` keeps insertion order (its `preserve_order`
/// feature, which another crate in a build may enable).
fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            Value::Object(keys.into_iter().map(|k| (k.clone(), sorted(&map[k]))).collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

fn render(values: &[(&str, Value)]) -> String {
    let mut out = String::new();
    for (label, value) in values {
        let json = serde_json::to_string_pretty(&sorted(value)).unwrap();
        writeln!(out, "--- {label}\n{json}").unwrap();
    }
    out
}

fn problem() -> Diagnostic {
    Diagnostic {
        code: "B2C-T1004".into(),
        severity: Severity::Error,
        message: "This compiler is too old: Blocks2Cpp needs GCC 11 or later.".into(),
        primary: Location {
            module: None,
            block: None,
            part: Part::Whole,
        },
        related: Vec::new(),
        source: DiagSource::Toolchain,
        raw: None,
    }
}

fn block_error() -> Diagnostic {
    Diagnostic {
        code: "B2C-E0201".into(),
        severity: Severity::Error,
        message: "The variable 'score' does not exist here.".into(),
        primary: Location {
            module: Some("mod_main".into()),
            block: Some("b012".into()),
            part: Part::Field { name: "VAR".into() },
        },
        related: Vec::new(),
        source: DiagSource::Analyser,
        raw: None,
    }
}

fn trusted() -> Trust {
    Trust {
        state: TrustState::Trusted,
        source: Some(TrustSource::CreatedHere),
        restricted_reason: None,
        mark_of_the_web: false,
    }
}

fn restricted() -> Trust {
    Trust {
        state: TrustState::Restricted,
        source: None,
        restricted_reason: Some(RestrictedReason::ChangedOutside),
        mark_of_the_web: true,
    }
}

/// A toolchain as the probe reports a working g++ 13.
fn probed_toolchain() -> Toolchain {
    Toolchain {
        id: ToolchainId::example(),
        version: Some("13.3.0".into()),
        target: Some("x86_64-linux-gnu".into()),
        flavor: None,
        display_path: "/usr/bin/g++-13".into(),
        source: ToolchainSource::Path,
        usable: true,
        selected: true,
        capabilities: ToolchainCapabilities {
            standards: vec![CppStandard::Cpp17, CppStandard::Cpp20, CppStandard::Cpp23],
            std_format: true,
            sanitizers: true,
            sarif: true,
        },
        problems: Vec::new(),
    }
}

fn unusable_toolchain() -> Toolchain {
    Toolchain {
        id: ToolchainId::parse("tc_fedcba9876543210").unwrap(),
        version: Some("9.4.0".into()),
        target: Some("x86_64-w64-mingw32".into()),
        flavor: Some("MSYS2 UCRT64".into()),
        display_path: "C:\\msys64\\ucrt64\\bin\\g++.exe".into(),
        source: ToolchainSource::WellKnown,
        usable: false,
        selected: false,
        capabilities: ToolchainCapabilities {
            standards: vec![CppStandard::Cpp17],
            std_format: false,
            sanitizers: false,
            sarif: false,
        },
        problems: vec![problem()],
    }
}

fn settings() -> Settings {
    Settings {
        format_version: 1,
        code_style: CodeStyle {
            indent_width: IndentWidth::Four,
        },
        run: RunSettings {
            on_errors: OnErrors::DisableRun,
        },
        console: ConsoleSettings {
            scrollback_lines: 10_000,
        },
        toolchain: ToolchainSettings {
            selected_id: Some(ToolchainId::example()),
        },
        new_project: NewProjectSettings {
            standard: CppStandard::Cpp20,
        },
        build_cache: BuildCacheSettings {
            max_bytes: 2 * 1024 * 1024 * 1024,
        },
    }
}

fn every_error() -> Vec<IpcError> {
    vec![
        IpcError::invalid(InvalidReason::UnknownField, Some("runOptions.colour")),
        IpcError::invalid(InvalidReason::Malformed, None),
        IpcError::too_large(33_554_432),
        IpcError::UnknownHandle,
        IpcError::UnknownBuild,
        IpcError::UnknownRun,
        IpcError::UnknownRecent,
        IpcError::UnknownSnapshot,
        IpcError::UnknownToolchain,
        IpcError::InvalidDocument {
            diagnostics: vec![block_error()],
        },
        IpcError::NewerFormat {
            needs: Some("3.1.0".into()),
        },
        IpcError::NewerFormat { needs: None },
        IpcError::Restricted,
        IpcError::ChangedOnDisk,
        IpcError::NoPath,
        IpcError::NotFound,
        IpcError::StaleBuild,
        IpcError::BuildNotSuccessful,
        IpcError::ProjectErrors { count: 3 },
        IpcError::NotRunning,
        IpcError::RateLimited,
        IpcError::Busy,
        IpcError::TooManyHandles,
        IpcError::TooManySessions,
        IpcError::ToolchainRejected {
            diagnostics: vec![problem()],
        },
        IpcError::Io {
            kind: IoKind::PermissionDenied,
        },
        IpcError::Internal,
    ]
}

#[test]
fn errors() {
    let errors = every_error();
    let codes: BTreeSet<&str> = errors.iter().map(IpcError::code).collect();
    let all: BTreeSet<&str> = IpcError::CODES.iter().copied().collect();
    assert_eq!(codes, all, "every variant is covered");
    assert_eq!(all.len(), IpcError::CODES.len(), "CODES lists each code once");
    let values: Vec<(&str, Value)> = errors
        .iter()
        .map(|error| {
            let json = round_trip(error);
            assert_eq!(json["code"], error.code());
            (error.code(), json)
        })
        .collect();
    insta::assert_snapshot!("errors", render(&values));
    for reason in InvalidReason::ALL {
        round_trip(&IpcError::invalid(*reason, Some("x")));
    }
    for kind in IoKind::ALL {
        round_trip(&IpcError::Io { kind: *kind });
    }
}

#[test]
fn events() {
    let handle = Handle::example();
    let app = [
        AppEvent::ProjectChangedOnDisk {
            handle,
            deleted: true,
        },
        AppEvent::ToolchainsUpdated {
            toolchains: vec![probed_toolchain()],
            discovering: false,
        },
        AppEvent::SettingsNotice {
            notices: vec![SettingsNotice {
                key: "console.scrollbackLines".into(),
                reason: NoticeReason::InvalidValue,
            }],
        },
        AppEvent::CloseRequested,
    ];
    let build = [
        BuildEvent::Progress {
            stage: BuildStage::Compile,
            done: 1,
            total: 3,
        },
        BuildEvent::Diagnostics {
            items: vec![block_error()],
        },
        BuildEvent::finished(BuildOutcome::Built, Some(&[0x5a; 32]), 1834),
        BuildEvent::finished(BuildOutcome::ProjectErrors, None, 12),
    ];
    let run = [
        RunEvent::Started {
            containment: Containment::ProcessGroupOnly,
            mode: RunMode::Pty,
            ide_helpers: true,
        },
        RunEvent::Skipped {
            lines: 52_113,
            after_seq: 40,
        },
        RunEvent::Exit {
            after_seq: 41,
            elapsed_ms: 2_500,
            status: ExitStatus::Exited { code: 0 },
            crash: None,
            sanitizer: None,
            message: "Finished (exit code 0)".into(),
        },
        RunEvent::Exit {
            after_seq: 2,
            elapsed_ms: 31,
            status: ExitStatus::Signaled { signal: 6 },
            crash: Some(Crash::Aborted),
            sanitizer: Some(SanitizerReport {
                tool: SanitizerTool::Address,
                kind: SanitizerKind::new("heap-buffer-overflow").unwrap(),
            }),
            message: "Crashed: heap-buffer-overflow (AddressSanitizer)".into(),
        },
        RunEvent::Exit {
            after_seq: 0,
            elapsed_ms: 4,
            status: ExitStatus::Exception {
                ntstatus: 0xC000_00FD,
            },
            crash: Some(Crash::StackOverflow),
            sanitizer: None,
            message: "Crashed: stack overflow".into(),
        },
        RunEvent::Exit {
            after_seq: 9,
            elapsed_ms: 900,
            status: ExitStatus::Stopped,
            crash: None,
            sanitizer: None,
            message: "Stopped".into(),
        },
    ];
    let mut values = Vec::new();
    for event in &app {
        values.push(("AppEvent", round_trip(event)));
    }
    for event in &build {
        values.push(("BuildEvent", round_trip(event)));
    }
    for event in &run {
        values.push(("RunEvent", round_trip(event)));
    }
    insta::assert_snapshot!("events", render(&values));
    for crash in Crash::ALL {
        assert_eq!(round_trip(crash), crash.as_str());
    }
}

#[test]
fn toolchains() {
    let values = [
        ("probed", round_trip(&probed_toolchain())),
        ("unusable", round_trip(&unusable_toolchain())),
    ];
    insta::assert_snapshot!("toolchains", render(&values));
}

#[test]
#[allow(clippy::too_many_lines, reason = "one entry per response type")]
fn responses() {
    let handle = Handle::example();
    let document = "{\"format\":\"blocks2cpp/project\"}".to_owned();
    let opened = ProjectOpened {
        handle: handle.clone(),
        document: document.clone(),
        trust: restricted(),
        file_name: "guess.b2c".into(),
        migrated_from: None,
    };
    let values = [
        (
            "AppInfo",
            round_trip(&AppInfo {
                app_version: "0.1.0".into(),
                ipc_version: b2c_ipc::IPC_VERSION,
                platform: Platform::Linux,
                catalog_version: "1.0.0".into(),
            }),
        ),
        ("Empty", round_trip(&Empty {})),
        (
            "ProjectNewResponse",
            round_trip(&ProjectNewResponse {
                handle: handle.clone(),
                document: document.clone(),
                trust: trusted(),
            }),
        ),
        (
            "ProjectOpenDialogResponse",
            round_trip(&ProjectOpenDialogResponse::Cancelled),
        ),
        (
            "ProjectOpenDialogResponse",
            round_trip(&ProjectOpenDialogResponse::Ok(opened.clone())),
        ),
        (
            "ProjectOpened",
            round_trip(&ProjectOpened {
                migrated_from: Some(1),
                ..opened
            }),
        ),
        (
            "ProjectReloadResponse",
            round_trip(&ProjectReloadResponse {
                document: document.clone(),
                trust: trusted(),
                migrated_from: None,
            }),
        ),
        (
            "ProjectSaveResponse",
            round_trip(&ProjectSaveResponse {
                saved_at: "2026-10-05T09:15:00Z".into(),
                hash: "ab".repeat(32),
            }),
        ),
        (
            "ProjectSaveAsDialogResponse",
            round_trip(&ProjectSaveAsDialogResponse::Cancelled),
        ),
        (
            "ProjectSaveAsDialogResponse",
            round_trip(&ProjectSaveAsDialogResponse::Ok(ProjectSavedAs {
                handle: handle.clone(),
                saved_at: "2026-10-05T09:15:00Z".into(),
                hash: "cd".repeat(32),
                file_name: "copy.b2c".into(),
            })),
        ),
        (
            "RecentListResponse",
            round_trip(&RecentListResponse {
                entries: vec![RecentEntry {
                    recent_id: RecentId::example(),
                    project_name: "Guessing Game".into(),
                    display_path: "~/Projects/guess.b2c".into(),
                    last_opened_at: "2026-10-04T18:00:00Z".into(),
                }],
            }),
        ),
        (
            "RecoveryListResponse",
            round_trip(&RecoveryListResponse {
                snapshots: vec![SnapshotInfo {
                    snapshot_id: SnapshotId::example(),
                    project_name: "Untitled".into(),
                    saved_at: "2026-10-04T18:01:00Z".into(),
                    has_path: false,
                }],
            }),
        ),
        (
            "RecoveryRestoreResponse",
            round_trip(&RecoveryRestoreResponse {
                handle: handle.clone(),
                document: document.clone(),
                trust: trusted(),
                file_name: None,
            }),
        ),
        (
            "TrustResponse",
            round_trip(&TrustResponse { trust: restricted() }),
        ),
        (
            "ToolchainListResponse",
            round_trip(&ToolchainListResponse {
                toolchains: vec![probed_toolchain()],
                discovering: true,
            }),
        ),
        (
            "ToolchainAddDialogResponse",
            round_trip(&ToolchainAddDialogResponse::Cancelled),
        ),
        (
            "ToolchainAddDialogResponse",
            round_trip(&ToolchainAddDialogResponse::Ok {
                toolchain: unusable_toolchain(),
            }),
        ),
        (
            "ToolchainSetupInfo",
            round_trip(&ToolchainSetupInfo {
                platform: Platform::Linux,
                no_usable_toolchain: true,
                distro: Some(Distro {
                    id: "ubuntu".into(),
                    id_like: vec!["debian".into()],
                }),
            }),
        ),
        (
            "ToolchainSetupInfo",
            round_trip(&ToolchainSetupInfo {
                platform: Platform::Windows,
                no_usable_toolchain: false,
                distro: None,
            }),
        ),
        (
            "BuildStartResponse",
            round_trip(&BuildStartResponse {
                build_id: BuildId::example(),
            }),
        ),
        (
            "BuildCacheClearResponse",
            round_trip(&BuildCacheClearResponse {
                freed_bytes: 734_003_200,
                skipped_in_use: 1,
            }),
        ),
        (
            "RunStartResponse",
            round_trip(&RunStartResponse {
                run_id: RunId::example(),
            }),
        ),
        (
            "SettingsGetResponse",
            round_trip(&SettingsGetResponse {
                settings: settings(),
                notices: vec![SettingsNotice {
                    key: "buildCache.maxBytes".into(),
                    reason: NoticeReason::NewerVersion,
                }],
            }),
        ),
        (
            "SettingsUpdateResponse",
            round_trip(&SettingsUpdateResponse { settings: settings() }),
        ),
    ];
    insta::assert_snapshot!("responses", render(&values));
}
