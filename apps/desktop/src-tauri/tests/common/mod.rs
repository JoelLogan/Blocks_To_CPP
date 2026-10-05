//! Helpers shared by the desktop adapter's integration tests: the app on
//! Tauri's mock runtime with a channel recorder, a backend whose folders are
//! all in a temporary directory, IPC calls, and the system g++.
//!
//! Tests that compile need g++ on `PATH`; without one they return early,
//! unless `B2C_REQUIRE_GXX` is set (as in CI).

// Test helpers fail the test by panicking; not every test file uses every
// helper.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    dead_code,
    unreachable_pub,
    missing_docs
)]

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use b2c_app::{Backend, BackendConfig, Dialogs, Services, TrustChoice, TrustPrompt, UrlOpener};
use b2c_build::toolchains::{DiscoveryScope, RealProber};
use b2c_store::Dirs;
use serde_json::Value;
use tauri::Manager as _;
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;

/// The app's own version, as the backend is started with.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The repository root.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The g++ on `PATH`, if any; panics when there is none but
/// `B2C_REQUIRE_GXX` is set (as in CI).
pub fn gxx() -> Option<PathBuf> {
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

/// Dialogs that cancel everything (the trust dialog: Stay in Restricted
/// Mode).
pub struct NoDialogs;

impl Dialogs for NoDialogs {
    fn open_project(&self) -> Option<PathBuf> {
        None
    }
    fn save_project_as(&self, _suggested_file_name: &str) -> Option<PathBuf> {
        None
    }
    fn pick_compiler(&self) -> Option<PathBuf> {
        None
    }
    fn confirm_trust(&self, _prompt: &TrustPrompt) -> TrustChoice {
        TrustChoice::StayRestricted
    }
}

/// An opener that counts help links instead of starting a browser.
#[derive(Default)]
pub struct NoBrowser {
    pub opened: AtomicUsize,
}

impl UrlOpener for NoBrowser {
    fn open(&self, _url: &'static str) -> io::Result<()> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// Starts a backend whose folders are all under `root`, searching only
/// `toolchain_dirs` for compilers, with no browser.
pub fn backend(root: &Path, toolchain_dirs: Vec<PathBuf>, dialogs: Arc<dyn Dialogs>) -> Arc<Backend> {
    Backend::start_with(
        BackendConfig {
            dirs: Dirs::under_root(root),
            app_version: "0.1.0",
            discovery_scope: DiscoveryScope::Only(toolchain_dirs),
            cwd: Some(root.to_path_buf()),
        },
        dialogs,
        Services {
            prober: Arc::new(RealProber),
            opener: Arc::new(NoBrowser::default()),
        },
    )
    .unwrap()
}

/// Every message a channel received, in the order it was sent.
pub type Recorded = Arc<Mutex<Vec<(u32, InvokeResponseBody)>>>;

/// The app as `blocks2cpp_desktop::configure` builds it, on the mock
/// runtime, with a backend and the main window.
pub struct Harness {
    pub app: tauri::App<MockRuntime>,
    pub window: tauri::WebviewWindow<MockRuntime>,
    pub backend: Arc<Backend>,
    pub recorded: Recorded,
    pub root: tempfile::TempDir,
}

impl Harness {
    /// The app with a fresh backend. With `record`, every channel message is
    /// recorded instead of being sent to the (absent) webview.
    pub fn new(toolchain_dirs: Vec<PathBuf>, dialogs: Arc<dyn Dialogs>, record: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let backend = backend(root.path(), toolchain_dirs, dialogs);
        Self::with_backend(backend, root, record)
    }

    /// The app around an existing backend.
    pub fn with_backend(backend: Arc<Backend>, root: tempfile::TempDir, record: bool) -> Self {
        let recorded: Recorded = Arc::default();
        let mut builder = blocks2cpp_desktop::configure(mock_builder());
        if record {
            let sink = Arc::clone(&recorded);
            builder = builder.channel_interceptor(move |_webview, callback, _index, body| {
                sink.lock().unwrap().push((callback.0, body.clone()));
                true
            });
        }
        let app = builder.build(mock_context(noop_assets())).unwrap();
        app.manage(Arc::clone(&backend));
        let window = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .unwrap();
        Self {
            app,
            window,
            backend,
            recorded,
            root,
        }
    }

    /// Calls `command` with `args` through the app's IPC, as the webview
    /// does; returns the response, or the error as JSON.
    pub fn invoke(&self, command: &str, args: Value) -> Result<Value, Value> {
        self.invoke_with(command, InvokeBody::Json(args))
    }

    /// Calls `command` with a raw body.
    pub fn invoke_with(&self, command: &str, body: InvokeBody) -> Result<Value, Value> {
        let url = if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        };
        get_ipc_response(
            &self.window,
            InvokeRequest {
                cmd: command.to_owned(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: url.parse().unwrap(),
                body,
                headers: tauri::http::HeaderMap::new(),
                invoke_key: INVOKE_KEY.to_owned(),
            },
        )
        .map(|body| body.deserialize::<Value>().unwrap())
    }

    /// The messages channel `id` received so far.
    pub fn messages(&self, id: u32) -> Vec<InvokeResponseBody> {
        self.recorded
            .lock()
            .unwrap()
            .iter()
            .filter(|(channel, _)| *channel == id)
            .map(|(_, body)| body.clone())
            .collect()
    }

    /// The JSON messages channel `id` received so far.
    pub fn events(&self, id: u32) -> Vec<Value> {
        self.messages(id)
            .into_iter()
            .map(|body| match body {
                InvokeResponseBody::Json(json) => serde_json::from_str(&json).unwrap(),
                InvokeResponseBody::Raw(_) => panic!("channel {id} carries JSON"),
            })
            .collect()
    }

    /// Waits until channel `id` has a JSON message of `kind`.
    pub fn wait_for(&self, id: u32, kind: &str) -> Vec<Value> {
        wait_until(Duration::from_mins(3), || {
            let events = self.events(id);
            events.iter().any(|event| event["kind"] == kind).then_some(events)
        })
        .unwrap_or_else(|| panic!("no {kind} event on channel {id}: {:?}", self.events(id)))
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.backend.shutdown();
    }
}

/// Polls `check` until it returns a value, for at most `limit`.
pub fn wait_until<T>(limit: Duration, mut check: impl FnMut() -> Option<T>) -> Option<T> {
    let start = Instant::now();
    loop {
        if let Some(value) = check() {
            return Some(value);
        }
        if start.elapsed() > limit {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The Hello World template's document with its printed text replaced.
pub fn with_printed_text(document: &str, text: &str) -> String {
    let mut value: Value = serde_json::from_str(document).unwrap();
    let item = &mut value["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"][0]["inputs"]["ITEM0"]
        ["expr"][0]["str"];
    assert!(item.is_string(), "the Hello World template changed shape: {item}");
    *item = Value::from(text);
    serde_json::to_string_pretty(&value).unwrap()
}
