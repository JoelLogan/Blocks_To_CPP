# Desktop app security settings

How the desktop app hardens its webview, its IPC and, on Windows, its process. Paths are relative
to `apps/desktop/`. The threat model and the rules every change follows are in
[spec chapter 8](../spec/08-security.md); a new IPC command follows
[Adding an IPC command](recipes.md#adding-an-ipc-command).

## Security settings

These implement
[docs/spec/08-security.md §8.8](../spec/08-security.md#88-webview-and-ipc-hardening):

- **Content Security Policy** exactly as specified, in `tauri.conf.json`. At run time Tauri adds
  hashes for its own scripts to `script-src` and the origin of the isolation frame to
  `default-src`. Tauri would also add hashes to `style-src`, which makes browsers ignore
  `'unsafe-inline'` and blocks the stylesheet that Blockly injects (the tracked exception in
  §8.8); `dangerousDisableAssetCspModification: ["style-src"]` keeps `style-src` exactly as
  specified.
- **No remote content.** Blockly's images and cursors are bundled (`vite.config.ts` copies them
  into `blockly-media/`) instead of loaded from Blockly's default web server, sounds are off, and
  fonts are the system's. The build inlines no assets as `data:` URLs.
- **Isolation pattern.** `src-tauri/isolation/index.js` runs in a sandboxed iframe and drops every
  IPC message whose command and arguments are not on its allowlist (`allowlist.generated.js`,
  generated from the command table in `crates/b2c-ipc`), before the backend sees it. The backend
  checks every request again.
- **Capabilities.** The `main` window may call exactly the commands of the IPC contract, one
  `allow-…` permission each: no core or plugin permissions, and no `fs`, `shell`, `http`,
  `process`, `opener` or `dialog` permission (the native dialogs are raised from Rust).
  `build.rs` declares the app's commands, so each one needs an explicit permission.
- **Navigation.** The window may navigate only within the app (and to the dev server during
  development), and requests to open new windows are denied (`src-tauri/src/window.rs`).
- `withGlobalTauri: false` (no `window.__TAURI__`), `freezePrototype: true`
  (`Object.prototype` is frozen against prototype pollution), and Tauri's developer tools are only
  in debug builds: the `devtools` Cargo feature is off. WebKitGTK's remote Web Inspector can still
  be turned on through the app's environment, also in release builds
  ([08 §8.8](../spec/08-security.md#88-webview-and-ipc-hardening)).
- **Frontend rules** (ESLint): no `innerHTML`, `outerHTML`, `insertAdjacentHTML`,
  `document.write`, `eval`, `new Function`, string arguments to `setTimeout`/`setInterval`, or
  `dangerouslySetInnerHTML`; `eslint-plugin-no-unsanitized` checks the remaining DOM sinks.
- **Trusted Types, report only.** Every build adds the report-only header
  `Content-Security-Policy-Report-Only: require-trusted-types-for 'script'` to the app's own HTML
  (08 §8.8), and logs _"added the Trusted Types report-only policy"_ at debug level when it does.
  The end-to-end tests collect the reports ([testing](testing.md#end-to-end-tests)).

## Windows hardening

`blocks2cpp_desktop::run()` first restricts where DLLs are loaded from (`harden_dll_search`: the
system folder and the app's own; a failure is logged and start-up continues). The application
manifest (`src-tauri/windows-app-manifest.xml`, linked into every executable of the crate, tests
included) declares Common Controls v6 and `longPathAware`, so paths beyond 260 characters work
where Windows allows them, and CI checks the release executable's embedded manifest with
[`tools/check-windows-manifest.ps1`](../../tools/check-windows-manifest.ps1)
(`B2C_CHECK_EXE=<exe> cargo test -p blocks2cpp-desktop --test hardening a_given_executable` does
the same check on any system).
