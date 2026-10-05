//! Starting the backend and the app-level commands
//! (`docs/spec/01-overview.md` N2, `docs/spec/02-architecture.md` §2.5–§2.7,
//! `docs/spec/07-toolchain-build-run.md` §7.2–§7.3): a cold start that never
//! probes, background discovery with its event, the folders discovery never
//! searches, toolchain selection and manual compilers, settings, help links,
//! the app info, clearing the build cache and shutting down.

// Test code: helpers fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use b2c_app::{Backend, BackendConfig, Services, StartError};
use b2c_build::toolchains::DiscoveryScope;
use b2c_ipc::dto::{
    AppEvent, NoticeReason, OpenHelpLinkRequest, Platform, SettingsPatch, ToolchainSelectRequest,
};
use b2c_ipc::{IpcError, LinkId, decode};
use b2c_store::Dirs;
use common::{Compiler, FakeDialogs, TestApp, TestProber, app_sink, wait_events};
use serde_json::json;

#[test]
fn startup_never_probes_and_discovery_reports_when_done() {
    let prober = TestProber::held();
    let app = TestApp::with(Compiler::Spy, Arc::clone(&prober));
    let events = app_sink();
    app.backend.app_subscribe(events.clone());
    // The first list answers at once, from the (empty) cache, while the
    // background discovery waits in its first probe.
    let started = Instant::now();
    let list = app.backend.toolchain_list().unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(list.toolchains.is_empty());
    assert!(list.discovering);
    let deadline = Instant::now() + Duration::from_secs(30);
    while prober.calls() == 0 {
        assert!(Instant::now() < deadline, "discovery never probed");
        std::thread::sleep(Duration::from_millis(5));
    }
    prober.release();
    let all = wait_events(&events, |events| {
        events
            .iter()
            .any(|event| matches!(event, AppEvent::ToolchainsUpdated { .. }))
    });
    let Some(AppEvent::ToolchainsUpdated {
        toolchains,
        discovering,
    }) = all.last()
    else {
        panic!("{all:?}");
    };
    assert!(!discovering);
    assert_eq!(toolchains.len(), 1);
    assert!(toolchains[0].usable);
    assert_eq!(
        toolchains[0].display_path,
        app.root().join("bin/g++").display().to_string()
    );
    assert_eq!(app.backend.toolchain_list().unwrap().toolchains, *toolchains);
}

#[test]
fn discovery_never_searches_the_cache_or_the_current_directory() {
    let app = TestApp::new();
    let excluded = app.backend.excluded_folders();
    assert!(excluded.contains(&app.dirs.cache), "{excluded:?}");
    assert!(excluded.contains(&app.root().join("cwd")), "{excluded:?}");

    // A g++ in the current directory is never found.
    #[cfg(unix)]
    {
        let root = tempfile::tempdir().unwrap();
        let root_path = b2c_toolchain::paths::canonical(root.path()).unwrap();
        let cwd = root_path.join("cwd");
        common::write_script(&cwd, "g++", "exit 0");
        let dialogs = FakeDialogs::new();
        let backend = Backend::start_with(
            BackendConfig {
                dirs: Dirs::under_root(&root_path.join("app")),
                app_version: "0.1.0",
                discovery_scope: DiscoveryScope::Only(vec![cwd.clone()]),
                cwd: Some(cwd),
            },
            dialogs.clone(),
            Services {
                prober: TestProber::new(),
                opener: Arc::new(common::FakeOpener::default()),
            },
        )
        .unwrap();
        let list = backend.toolchain_rescan().unwrap();
        assert!(list.toolchains.is_empty(), "{:?}", list.toolchains);
        // The folders of open projects are excluded too.
        let projects = root_path.join("projects");
        std::fs::create_dir_all(&projects).unwrap();
        let project = projects.join("game.b2c");
        std::fs::write(&project, common::example("hello_world")).unwrap();
        dialogs.will_open(&project);
        backend.project_open_dialog().unwrap();
        assert!(backend.excluded_folders().contains(&projects));
    }
}

#[test]
fn relative_folders_are_refused() {
    let mut dirs = Dirs::under_root(std::path::Path::new("/tmp/b2c-test-root"));
    dirs.cache = std::path::PathBuf::from("relative/cache");
    let error = Backend::start(
        BackendConfig {
            dirs,
            app_version: "0.1.0",
            discovery_scope: DiscoveryScope::Only(Vec::new()),
            cwd: None,
        },
        FakeDialogs::new(),
    )
    .unwrap_err();
    assert_eq!(error, StartError::RelativeFolder("cache"));
    let error = Backend::start(
        BackendConfig {
            dirs: Dirs::under_root(std::path::Path::new("/tmp/b2c-test-root")),
            app_version: "0.1.0",
            discovery_scope: DiscoveryScope::Only(Vec::new()),
            cwd: Some(std::path::PathBuf::from("here")),
        },
        FakeDialogs::new(),
    )
    .unwrap_err();
    assert_eq!(error, StartError::RelativeFolder("current"));
}

#[test]
fn selecting_a_toolchain_persists() {
    let app = TestApp::with(Compiler::Spy, TestProber::new());
    app.wait_discovery();
    let list = app.backend.toolchain_list().unwrap();
    assert_eq!(list.toolchains.len(), 1);
    assert!(!list.toolchains[0].selected);
    let id = list.toolchains[0].id.clone();
    app.backend
        .toolchain_select(ToolchainSelectRequest {
            toolchain_id: id.clone(),
        })
        .unwrap();
    let list = app.backend.toolchain_list().unwrap();
    assert!(list.toolchains[0].selected);
    assert_eq!(
        app.backend.settings_get().unwrap().settings.toolchain.selected_id,
        Some(id.clone())
    );
    let file: serde_json::Value =
        serde_json::from_slice(&std::fs::read(app.dirs.config.join("settings.json")).unwrap()).unwrap();
    assert_eq!(file["toolchain"]["selectedId"], id.as_str());
    // settings_update can never change it.
    assert!(decode::<SettingsPatch>(json!({"toolchain": {"selectedId": null}})).is_err());
}

#[cfg(unix)]
#[test]
fn compilers_added_by_hand_are_checked() {
    use b2c_ipc::dto::ToolchainAddDialogResponse;

    let app = TestApp::new();
    app.wait_discovery();
    // Cancelled.
    assert_eq!(
        app.backend.toolchain_add_dialog().unwrap(),
        ToolchainAddDialogResponse::Cancelled
    );
    // A batch file is never accepted.
    let bat = common::write_script(&app.root().join("tools"), "g++.bat", "exit 0");
    app.dialogs.will_pick_compiler(&bat);
    let IpcError::ToolchainRejected { diagnostics } = app.backend.toolchain_add_dialog().unwrap_err() else {
        panic!("expected toolchainRejected");
    };
    assert!(
        diagnostics.iter().any(|d| d.code == "B2C-T1002"),
        "{diagnostics:?}"
    );
    assert!(app.backend.toolchain_list().unwrap().toolchains.is_empty());
    // A g++ is probed and kept as manual.
    let gxx = common::write_script(&app.root().join("tools"), "g++", "exit 0");
    app.dialogs.will_pick_compiler(&gxx);
    let ToolchainAddDialogResponse::Ok { toolchain } = app.backend.toolchain_add_dialog().unwrap() else {
        panic!("cancelled");
    };
    assert_eq!(toolchain.source, b2c_ipc::dto::ToolchainSource::Manual);
    assert!(!toolchain.selected);
    assert_eq!(app.backend.toolchain_list().unwrap().toolchains, [toolchain]);
}

#[test]
fn setup_info_reports_the_platform() {
    let app = TestApp::new();
    app.wait_discovery();
    let info = app.backend.toolchain_setup_info().unwrap();
    assert_eq!(info.platform, Platform::current());
    assert!(info.no_usable_toolchain);
    if cfg!(windows) {
        assert_eq!(info.distro, None);
    }
}

#[test]
fn settings_round_trip_and_notices_reach_the_first_subscriber() {
    let root = tempfile::tempdir().unwrap();
    let root_path = b2c_toolchain::paths::canonical(root.path()).unwrap();
    let dirs = Dirs::under_root(&root_path.join("app"));
    std::fs::create_dir_all(&dirs.config).unwrap();
    std::fs::write(
        dirs.config.join("settings.json"),
        br#"{"format": "blocks2cpp/settings", "formatVersion": 1, "console": {"scrollbackLines": 5}}"#,
    )
    .unwrap();
    let backend = Backend::start_with(
        BackendConfig {
            dirs: dirs.clone(),
            app_version: "0.1.0",
            discovery_scope: DiscoveryScope::Only(Vec::new()),
            cwd: Some(root_path.clone()),
        },
        FakeDialogs::new(),
        Services {
            prober: TestProber::new(),
            opener: Arc::new(common::FakeOpener::default()),
        },
    )
    .unwrap();
    let got = backend.settings_get().unwrap();
    assert_eq!(got.settings.console.scrollback_lines, 10_000);
    assert_eq!(got.notices.len(), 1);
    assert_eq!(got.notices[0].key, "console.scrollbackLines");
    assert_eq!(got.notices[0].reason, NoticeReason::InvalidValue);
    let events = app_sink();
    backend.app_subscribe(events.clone());
    assert_eq!(
        events.events(),
        [AppEvent::SettingsNotice {
            notices: got.notices.clone()
        }]
    );

    let patch = decode::<SettingsPatch>(
        json!({"codeStyle": {"indentWidth": 2}, "run": {"onErrors": "showProblems"}}),
    )
    .unwrap();
    let updated = backend.settings_update(patch).unwrap().settings;
    assert_eq!(updated.code_style.indent_width, b2c_ipc::dto::IndentWidth::Two);
    assert_eq!(updated.run.on_errors, b2c_ipc::dto::OnErrors::ShowProblems);
    assert_eq!(backend.settings_get().unwrap().settings, updated);
    let file = std::fs::read(dirs.config.join("settings.json")).unwrap();
    // A load and save round trip is byte-stable.
    backend.settings_update(SettingsPatch::default()).unwrap();
    assert_eq!(std::fs::read(dirs.config.join("settings.json")).unwrap(), file);
}

#[test]
fn help_links_open_fixed_https_urls_only() {
    let app = TestApp::new();
    for link in LinkId::ALL {
        app.backend
            .open_help_link(OpenHelpLinkRequest { link_id: *link })
            .unwrap();
    }
    let urls = app.opener.urls();
    assert_eq!(urls.len(), LinkId::ALL.len());
    for url in &urls {
        assert!(url.starts_with("https://"), "{url}");
    }
    assert!(decode::<OpenHelpLinkRequest>(json!({"linkId": "https://example.com"})).is_err());
    assert!(decode::<OpenHelpLinkRequest>(json!({"linkId": "msys2Install", "url": "x"})).is_err());
}

#[test]
fn app_info_reports_the_versions() {
    let app = TestApp::new();
    let info = app.backend.app_info();
    assert_eq!(info.app_version, "0.1.0");
    assert_eq!(info.ipc_version, b2c_ipc::IPC_VERSION);
    assert_eq!(info.platform, Platform::current());
    assert_eq!(info.catalog_version, b2c_build::CATALOG_VERSION);
}

#[test]
fn clearing_an_empty_cache_frees_nothing() {
    let app = TestApp::new();
    let cleared = app.backend.build_cache_clear().unwrap();
    assert_eq!(cleared.freed_bytes, 0);
    assert_eq!(cleared.skipped_in_use, 0);
}

#[test]
fn recovery_is_not_available_yet() {
    let app = TestApp::new();
    assert_eq!(app.backend.recovery_list().unwrap_err(), IpcError::Internal);
}

#[test]
fn shutdown_is_idempotent_and_ends_sessions() {
    let app = TestApp::new();
    app.backend.app_quit().unwrap();
    app.backend.shutdown();
    let created = app
        .backend
        .project_new(b2c_ipc::dto::ProjectNewRequest {
            template: b2c_ipc::dto::Template::HelloWorld,
        })
        .unwrap();
    let error = app
        .backend
        .build_start(
            b2c_ipc::dto::BuildStartRequest {
                handle: created.handle,
                document: created.document,
                config: b2c_ipc::dto::BuildConfig::Debug,
            },
            Arc::new(b2c_ipc::sink::testing::RecordingSink::new()),
        )
        .unwrap_err();
    assert_eq!(error, IpcError::Internal, "no build starts after shutdown");
}

#[test]
fn startup_prunes_builds_unused_for_30_days() {
    let root = tempfile::tempdir().unwrap();
    let root_path = b2c_toolchain::paths::canonical(root.path()).unwrap();
    let dirs = Dirs::under_root(&root_path.join("app"));
    let entry = |name: &str, days: u64| {
        let folder = dirs.builds().join("prj_old-00000001").join(name);
        std::fs::create_dir_all(folder.join("out")).unwrap();
        std::fs::write(folder.join("out").join("main"), vec![0_u8; 100]).unwrap();
        let lock = std::fs::File::create(folder.join("lock")).unwrap();
        lock.set_modified(std::time::SystemTime::now() - Duration::from_hours(24 * days))
            .unwrap();
        folder
    };
    let old = entry("debug-00000001", 31);
    let recent = entry("debug-00000002", 1);
    let sandbox = dirs.sandbox().join("prj_old-00000001");
    std::fs::create_dir_all(&sandbox).unwrap();
    let _backend = Backend::start_with(
        BackendConfig {
            dirs: dirs.clone(),
            app_version: "0.1.0",
            discovery_scope: DiscoveryScope::Only(Vec::new()),
            cwd: Some(root_path.clone()),
        },
        FakeDialogs::new(),
        Services {
            prober: TestProber::new(),
            opener: Arc::new(common::FakeOpener::default()),
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_mins(1);
    while old.exists() {
        assert!(Instant::now() < deadline, "the old build was not pruned");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(recent.exists());
    assert!(sandbox.exists(), "sandboxes are never pruned");
}
