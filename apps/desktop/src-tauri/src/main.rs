//! The Blocks2Cpp desktop app: a thin Tauri shell around the block editor
//! (docs/spec/02-architecture.md §2.3). It holds no business logic. Later
//! milestones add IPC commands that adapt to `b2c-build` and `b2c-toolchain`.
//!
//! Hardening (docs/spec/08-security.md §8.8): the Content Security Policy,
//! the isolation pattern and the capabilities are set in `tauri.conf.json`,
//! `isolation/` and `capabilities/`. The editor window may navigate only within
//! the app, and it cannot open new windows.

// Release builds are GUI apps on Windows: no console window next to the editor.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write as _;
use std::process::ExitCode;

use tauri::webview::NewWindowResponse;
use tauri::{App, Url, WebviewWindowBuilder};

/// The label of the editor window, as in `tauri.conf.json` and `capabilities/`.
const MAIN_WINDOW: &str = "main";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // There is no window to show this in. If stderr is gone too, the
            // exit code is all that is left.
            let _ = writeln!(std::io::stderr().lock(), "Blocks2Cpp could not start: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Starts the app and runs it until the last window closes.
fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_version])
        .setup(|app| Ok(open_main_window(app)?))
        .run(tauri::generate_context!())
}

/// Opens the editor window from its entry in `tauri.conf.json` (which has
/// `"create": false`), adding the navigation rules that the configuration
/// file cannot express.
fn open_main_window(app: &App) -> tauri::Result<()> {
    let config = app.config();
    let window = config
        .app
        .windows
        .iter()
        .find(|window| window.label == MAIN_WINDOW)
        .ok_or(tauri::Error::WindowNotFound)?;
    // In development the frontend comes from the Vite dev server.
    let dev_url = if cfg!(dev) {
        config.build.dev_url.clone()
    } else {
        None
    };
    WebviewWindowBuilder::from_config(app.handle(), window)?
        .on_navigation(move |url| is_app_url(url, dev_url.as_ref()))
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .build()?;
    Ok(())
}

/// Whether `url` belongs to the app's own frontend: the bundled assets
/// (`tauri://localhost` on Linux, `http://tauri.localhost` on Windows) or, in
/// development, the dev server. Everything else, such as a link to a web page,
/// is refused (help links will open in the system browser through a
/// dedicated command).
fn is_app_url(url: &Url, dev_url: Option<&Url>) -> bool {
    let bundled = match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" | "https" => url.host_str() == Some("tauri.localhost"),
        _ => false,
    };
    bundled || dev_url.is_some_and(|dev| dev.origin() == url.origin())
}

/// Returns the app's version. A trivial command: it lets the editor check
/// that IPC works under the Content Security Policy and the isolation pattern.
#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn bundled_frontend_is_allowed() {
        assert!(is_app_url(&url("tauri://localhost/index.html"), None));
        assert!(is_app_url(&url("http://tauri.localhost/"), None));
        assert!(is_app_url(&url("https://tauri.localhost/assets/app.js"), None));
    }

    #[test]
    fn other_sites_are_refused() {
        for other in [
            "https://example.com/",
            "http://localhost:1420/",
            "https://tauri.localhost.example.com/",
            "tauri://example.com/",
            "file:///etc/passwd",
            "data:text/html,hello",
            "javascript:alert(1)",
            "about:blank",
        ] {
            assert!(!is_app_url(&url(other), None), "{other} should be refused");
        }
    }

    #[test]
    fn dev_server_is_allowed_only_when_configured() {
        let dev = url("http://localhost:1420");
        assert!(is_app_url(&url("http://localhost:1420/src/main.tsx"), Some(&dev)));
        assert!(!is_app_url(&url("http://localhost:1421/"), Some(&dev)));
        assert!(!is_app_url(&url("https://localhost:1420/"), Some(&dev)));
        assert!(!is_app_url(&url("http://localhost:1420/"), None));
    }

    #[test]
    fn app_version_matches_the_package() {
        assert_eq!(app_version(), env!("CARGO_PKG_VERSION"));
    }
}
