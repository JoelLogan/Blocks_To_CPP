//! The editor window: opening it with the navigation rules the configuration
//! file cannot express (`docs/spec/08-security.md` §8.8), and closing it with
//! unsaved changes (`docs/spec/02-architecture.md` §2.6).

use std::sync::Arc;

use b2c_app::Backend;
use tauri::webview::NewWindowResponse;
use tauri::{App, Manager as _, Runtime, Url, WebviewWindowBuilder, Window, WindowEvent};

/// The label of the editor window, as in `tauri.conf.json` and `capabilities/`.
pub(crate) const MAIN_WINDOW: &str = "main";

/// Opens the editor window from its entry in `tauri.conf.json` (which has
/// `"create": false`), adding the navigation rules that the configuration
/// file cannot express: the window may only show the app itself, and it can
/// never open another window.
pub(crate) fn open_main_window<R: Runtime>(app: &App<R>) -> tauri::Result<()> {
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
/// development, the dev server. Everything else, such as a link to a web
/// page, is refused; help links open in the system browser through
/// `open_help_link`.
pub(crate) fn is_app_url(url: &Url, dev_url: Option<&Url>) -> bool {
    let bundled = match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" | "https" => url.host_str() == Some("tauri.localhost"),
        _ => false,
    };
    bundled || dev_url.is_some_and(|dev| dev.origin() == url.origin())
}

/// Window events: when the editor window is asked to close while a project
/// has unsaved changes, the backend keeps it open and sends `closeRequested`;
/// the frontend asks Save / Don't save / Cancel and then calls `app_quit`
/// (`docs/spec/02-architecture.md` §2.6).
pub(crate) fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event
        && window.label() == MAIN_WINDOW
        && let Some(backend) = window.try_state::<Arc<Backend>>()
        && keep_open(&backend)
    {
        api.prevent_close();
    }
}

/// Whether the window must stay open: the backend asked the frontend about
/// unsaved changes instead of allowing the close. (The backend allows it
/// when nothing is dirty, or when no frontend listens to answer.)
fn keep_open(backend: &Backend) -> bool {
    !backend.request_close()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use b2c_app::{BackendConfig, Dialogs, TrustChoice, TrustPrompt};
    use b2c_build::toolchains::DiscoveryScope;
    use b2c_ipc::dto::{AppEvent, ProjectNewRequest, ProjectSetDirtyRequest, Template};
    use b2c_ipc::sink::testing::RecordingSink;
    use b2c_store::Dirs;

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
            "isolation://localhost/",
            "ipc://localhost/",
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

    /// Dialogs that are never shown in these tests.
    struct NoDialogs;

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

    #[test]
    fn the_window_stays_open_only_while_the_frontend_is_asked_about_unsaved_changes() {
        let root = tempfile::tempdir().unwrap();
        let backend = Backend::start(
            BackendConfig {
                dirs: Dirs::under_root(root.path()),
                app_version: crate::APP_VERSION,
                discovery_scope: DiscoveryScope::Only(Vec::new()),
                cwd: Some(root.path().to_path_buf()),
            },
            Arc::new(NoDialogs),
        )
        .unwrap();
        let events = Arc::new(RecordingSink::<AppEvent>::new());
        backend.app_subscribe(events.clone());
        let project = backend
            .project_new(ProjectNewRequest {
                template: Template::Empty,
            })
            .unwrap();

        // Nothing unsaved: the window closes.
        assert!(!keep_open(&backend));

        backend
            .project_set_dirty(ProjectSetDirtyRequest {
                handle: project.handle.clone(),
                dirty: true,
            })
            .unwrap();
        events.take();
        assert!(keep_open(&backend));
        assert_eq!(events.take(), [AppEvent::CloseRequested]);

        // Without a frontend to ask, the window can always be closed.
        events.close();
        assert!(!keep_open(&backend));
        backend.shutdown();
    }
}
