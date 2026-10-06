//! The editor window: opening it with the navigation rules and the response
//! header that the configuration file cannot express
//! (`docs/spec/08-security.md` §8.8), and closing it with unsaved changes
//! (`docs/spec/02-architecture.md` §2.6).

use std::borrow::Cow;
use std::sync::Arc;

use b2c_app::Backend;
use tauri::http::header::{CONTENT_SECURITY_POLICY_REPORT_ONLY, CONTENT_TYPE};
use tauri::http::{HeaderMap, HeaderValue, Response, Uri};
use tauri::webview::NewWindowResponse;
use tauri::{App, Manager as _, Runtime, Url, WebviewWindowBuilder, Window, WindowEvent};

/// The label of the editor window, as in `tauri.conf.json` and `capabilities/`.
pub(crate) const MAIN_WINDOW: &str = "main";

/// The policy of the Trusted Types trial (`docs/spec/08-security.md` §8.8),
/// sent as `Content-Security-Policy-Report-Only`: a string passed to a DOM
/// sink that can run script (`innerHTML`, a script element's `src`,
/// `eval`, …) raises a `securitypolicyviolation` event, which the frontend
/// counts, but is never blocked. The enforced policy (`tauri.conf.json`) is
/// unchanged; enforcement comes once Blockly runs cleanly under this one.
pub(crate) const TRUSTED_TYPES_REPORT_ONLY: &str = "require-trusted-types-for 'script'";

/// Opens the editor window from its entry in `tauri.conf.json` (which has
/// `"create": false`), adding what the configuration file cannot express:
/// the window may only show the app itself, at this platform's app origin,
/// it can never open another window, and the app's HTML carries the
/// report-only Trusted Types policy ([`add_trusted_types_report_only`]; in
/// every build, so the trial sees what users run).
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
    let builder = WebviewWindowBuilder::from_config(app.handle(), window)?
        .on_navigation(move |url| is_app_url(url, dev_url.as_ref()))
        .on_new_window(|_, _| NewWindowResponse::Deny)
        // Tauri calls this for every response of its `tauri` protocol, which
        // serves the bundled frontend (not the dev server).
        .on_web_resource_request(|request, response| {
            add_trusted_types_report_only(request.uri(), response);
        });
    // End-to-end builds pass on a WebDriver's browser arguments, which
    // WebView2 would otherwise drop in favour of wry's (see the function).
    #[cfg(all(windows, feature = "e2e-hooks"))]
    let builder =
        match crate::e2e::webview2_browser_args(std::env::var(crate::e2e::WEBVIEW2_ARGS_ENV).ok().as_deref())
        {
            Some(arguments) => builder.additional_browser_args(&arguments),
            None => builder,
        };
    builder.build()?;
    Ok(())
}

/// Adds the Trusted Types trial's report-only policy
/// ([`TRUSTED_TYPES_REPORT_ONLY`]) to `response` when it is an HTML document
/// (`Content-Type: text/html`, any parameters) of the app's own frontend
/// ([`is_app_resource`]). Other responses (scripts, styles, images, the
/// WebAssembly core) are left alone: only a document's policy governs its
/// page, and a worker script keeps the policy Tauri gives it. Another
/// report-only policy already on the response stays (each is applied on its
/// own), and the trial's is never added twice.
///
/// Logged at debug level with the resource's path (never project content),
/// so an end-to-end run (`B2C_LOG=debug`) can confirm the policy reached the
/// main document.
pub(crate) fn add_trusted_types_report_only(uri: &Uri, response: &mut Response<Cow<'static, [u8]>>) {
    if !is_app_resource(uri) || !is_html(response.headers()) {
        return;
    }
    let headers = response.headers_mut();
    let present = headers
        .get_all(CONTENT_SECURITY_POLICY_REPORT_ONLY)
        .iter()
        .any(|value| value == TRUSTED_TYPES_REPORT_ONLY);
    if !present {
        headers.append(
            CONTENT_SECURITY_POLICY_REPORT_ONLY,
            HeaderValue::from_static(TRUSTED_TYPES_REPORT_ONLY),
        );
        tracing::debug!(path = uri.path(), "added the Trusted Types report-only policy");
    }
}

/// Whether a request that Tauri's `tauri` protocol answers is for the app's
/// own frontend. Tauri passes the protocol's own form of the URL,
/// `tauri://localhost/…`, on every platform (on Windows, wry turns the
/// `http://tauri.localhost/…` that `WebView2` requested back into it); this
/// platform's app origin ([`APP_ORIGIN`]) is accepted as well, in case a
/// later wry stops translating it. Nothing else is: never another host, port
/// or scheme.
fn is_app_resource(uri: &Uri) -> bool {
    let at = |(scheme, host): (&str, &str)| {
        uri.scheme_str() == Some(scheme) && uri.host() == Some(host) && uri.port().is_none()
    };
    at(("tauri", "localhost")) || at(APP_ORIGIN)
}

/// Whether the response is an HTML document: its `Content-Type` is
/// `text/html`, ignoring letter case and parameters such as `charset`.
fn is_html(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("text/html"))
}

/// The origin of the bundled frontend on this platform: Tauri serves it as
/// `http://tauri.localhost` on Windows (`WebView2`, `useHttpsScheme` off) and
/// as `tauri://localhost` elsewhere (`WebKitGTK`).
const APP_ORIGIN: (&str, &str) = if cfg!(windows) {
    ("http", "tauri.localhost")
} else {
    ("tauri", "localhost")
};

/// Whether `url` belongs to the app's own frontend: the bundled assets, at
/// exactly this platform's app origin ([`APP_ORIGIN`]), or, in development,
/// the dev server. Everything else is refused (`docs/spec/08-security.md`
/// §8.8): a link to a web page, and also the other platform's spelling of
/// the app origin, which here is not the app but a network address (on Linux
/// `http://tauri.localhost` is whatever listens on the loopback interface;
/// on Windows `https://tauri.localhost` is not served by the app). Help links
/// open in the system browser through `open_help_link`.
pub(crate) fn is_app_url(url: &Url, dev_url: Option<&Url>) -> bool {
    let (scheme, host) = APP_ORIGIN;
    let bundled = url.scheme() == scheme && url.host_str() == Some(host) && url.port().is_none();
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

    /// This platform's app origin, and the other platform's (which is not
    /// the app here).
    const fn origins() -> (&'static str, &'static str) {
        if cfg!(windows) {
            ("http://tauri.localhost", "tauri://localhost")
        } else {
            ("tauri://localhost", "http://tauri.localhost")
        }
    }

    #[test]
    fn bundled_frontend_is_allowed() {
        let (app, _) = origins();
        for path in ["/", "/index.html", "/assets/app.js?v=1#top"] {
            let page = format!("{app}{path}");
            assert!(is_app_url(&url(&page), None), "{page} should be allowed");
        }
    }

    /// 08 §8.8: only this platform's app origin is the app. The other
    /// platform's spelling, and the same host under another scheme or port,
    /// are network addresses (or nothing) here.
    #[test]
    fn other_sites_are_refused() {
        let (_, other_platform) = origins();
        for other in [
            other_platform,
            "https://tauri.localhost/",
            "https://tauri.localhost/assets/app.js",
            "http://tauri.localhost:8080/",
            "tauri://localhost:1420/",
            "https://example.com/",
            "http://localhost:1420/",
            "http://localhost/",
            "https://tauri.localhost.example.com/",
            "http://tauri.localhost.example.com/",
            "tauri://example.com/",
            "tauri://tauri.localhost/",
            "file:///etc/passwd",
            "data:text/html,hello",
            "javascript:alert(1)",
            "about:blank",
            "isolation://localhost/",
            "ipc://localhost/",
            "http://ipc.localhost/",
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

    /// A response as Tauri's `tauri` protocol builds it: the content type,
    /// and for HTML the enforced policy.
    fn asset(content_type: &str) -> Response<Cow<'static, [u8]>> {
        let mut builder = Response::builder()
            .header("Access-Control-Allow-Origin", "tauri://localhost")
            .header(CONTENT_TYPE, content_type);
        if content_type.to_ascii_lowercase().starts_with("text/html") {
            builder = builder.header("Content-Security-Policy", "default-src 'none'");
        }
        builder.body(Cow::Borrowed(&b"<!doctype html>"[..])).unwrap()
    }

    /// The report-only policies on `response`.
    fn report_only(response: &Response<Cow<'static, [u8]>>) -> Vec<String> {
        response
            .headers()
            .get_all(CONTENT_SECURITY_POLICY_REPORT_ONLY)
            .iter()
            .map(|value| value.to_str().unwrap().to_owned())
            .collect()
    }

    fn uri(text: &str) -> Uri {
        text.parse().unwrap()
    }

    #[test]
    fn the_app_html_gets_the_report_only_trusted_types_policy() {
        let (app, _) = origins();
        for page in [
            "tauri://localhost/".to_owned(),
            "tauri://localhost/index.html".to_owned(),
            "tauri://localhost/index.html?x=1#y".to_owned(),
            format!("{app}/"),
            format!("{app}/index.html"),
        ] {
            for content_type in [
                "text/html",
                "text/html; charset=utf-8",
                "TEXT/HTML ;charset=UTF-8",
            ] {
                let mut response = asset(content_type);
                add_trusted_types_report_only(&uri(&page), &mut response);
                assert_eq!(
                    report_only(&response),
                    ["require-trusted-types-for 'script'"],
                    "{page} ({content_type})"
                );
                // The enforced policy is untouched.
                assert_eq!(
                    response.headers()["Content-Security-Policy"],
                    "default-src 'none'"
                );
            }
        }
    }

    #[test]
    fn other_resources_get_no_policy() {
        for content_type in [
            "text/javascript",
            "text/css",
            "application/wasm",
            "application/json",
            "image/svg+xml",
            "text/plain",
            "text/htmlx",
            "application/xhtml+xml",
            "",
        ] {
            let mut response = asset(content_type);
            add_trusted_types_report_only(&uri("tauri://localhost/assets/app.js"), &mut response);
            assert!(report_only(&response).is_empty(), "{content_type:?}");
        }
        let mut no_type = Response::new(Cow::Borrowed(&b""[..]));
        add_trusted_types_report_only(&uri("tauri://localhost/"), &mut no_type);
        assert!(report_only(&no_type).is_empty());
    }

    #[test]
    fn html_of_other_origins_gets_no_policy() {
        let (_, other_platform) = origins();
        let mut pages = vec![
            "tauri://localhost:1420/".to_owned(),
            "tauri://example.com/".to_owned(),
            "https://tauri.localhost/".to_owned(),
            "http://localhost:1420/".to_owned(),
            "https://example.com/".to_owned(),
            "isolation://localhost/index.html".to_owned(),
            "/index.html".to_owned(),
        ];
        // On Windows the other platform's origin is `tauri://localhost`, the
        // protocol's own form of every request, which is the app's there too.
        if !cfg!(windows) {
            pages.push(format!("{other_platform}/"));
        }
        for page in pages {
            let mut response = asset("text/html");
            add_trusted_types_report_only(&uri(&page), &mut response);
            assert!(report_only(&response).is_empty(), "{page}");
        }
    }

    #[test]
    fn the_policy_is_added_once_and_keeps_other_report_only_policies() {
        let mut response = asset("text/html");
        response.headers_mut().append(
            CONTENT_SECURITY_POLICY_REPORT_ONLY,
            HeaderValue::from_static("img-src 'none'"),
        );
        add_trusted_types_report_only(&uri("tauri://localhost/"), &mut response);
        add_trusted_types_report_only(&uri("tauri://localhost/"), &mut response);
        assert_eq!(
            report_only(&response),
            ["img-src 'none'", "require-trusted-types-for 'script'"]
        );
    }

    /// The policy is report-only, and not also in the enforced policy of
    /// `tauri.conf.json` (whose exact text `tests/consistency.rs` checks
    /// against the spec).
    #[test]
    fn the_enforced_policy_does_not_require_trusted_types() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let csp = config["app"]["security"]["csp"].as_str().unwrap();
        assert!(!csp.contains("trusted-types"), "{csp}");
        assert_eq!(TRUSTED_TYPES_REPORT_ONLY, "require-trusted-types-for 'script'");
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
