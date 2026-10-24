# Blocks2Cpp desktop app

The desktop editor of Blocks2Cpp: a React + TypeScript frontend with a
[Blockly](https://github.com/RaspberryPiFoundation/blockly) workspace, running in a
[Tauri 2](https://v2.tauri.app) shell (WebView2 on Windows, WebKitGTK on Linux). Its backend is
`b2c_app::Backend` (`crates/b2c-app`) behind the IPC contract of `crates/b2c-ipc`, and the editor
runs the compiler core as WebAssembly (`packages/b2c-core-wasm`) for its live C++ view.

- **Build and start it:** [`BUILDING.md`](../../BUILDING.md), the one place with the build steps.
- **Use it:** [Getting started](../../docs/user-guide/getting-started.md).
- **Change it:** the [developer guide](../../docs/developer-guide/README.md) has the
  [architecture](../../docs/developer-guide/desktop-architecture.md) (with the list of Blockly
  seams), the [security settings](../../docs/developer-guide/desktop-security.md), the
  [dependencies](../../docs/developer-guide/dependencies.md),
  [testing](../../docs/developer-guide/testing.md) and
  [recipes](../../docs/developer-guide/recipes.md). The checks are listed in
  [CONTRIBUTING.md](../../CONTRIBUTING.md#checks), the end-to-end tests in [`e2e/`](e2e/README.md).
- **Design:** [02 Architecture](../../docs/spec/02-architecture.md),
  [04 User interface](../../docs/spec/04-user-interface.md) and
  [08 Security](../../docs/spec/08-security.md).

## Code map

| Path                                   | Contents                                                                                                                                                                   |
| -------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `index.html`, `src/main.tsx`           | Entry point (no inline scripts)                                                                                                                                            |
| `src/app/`                             | The app shell: start-up, state, registries, window, banners, dialogs                                                                                                       |
| `src/features/`                        | The features, installed at start-up (`index.ts`): project, build and run, toolchain, settings, trust, recovery, external changes                                           |
| `src/editor/`                          | The block editor: workspace, BDM ⇄ Blockly sync, live preview, services, module switcher, toolbox, diagnostics on blocks, highlighting, clipboard                          |
| `src/panels/`                          | The C++ code panel, Problems, the console and Build output                                                                                                                 |
| `src/lib/ipc.ts`                       | The typed client for the backend's commands                                                                                                                                |
| `src/lib/timing.ts`                    | The User Timing spans (`b2c:preview`, …) that the long-task recorder and the benchmarks read, and the end-to-end hook gives                                                |
| `src/e2e/`                             | The end-to-end test hook `window.__B2C_E2E__`, built only in Vite mode `e2e` (`pnpm build:e2e`) and absent from every other build                                          |
| `e2e/`                                 | The end-to-end tests (selenium-webdriver, `tauri-driver`, Vitest), the webview benchmarks (`bench/`) and the visual diff (`visual/`); see [`e2e/README.md`](e2e/README.md) |
| `src/test/`, `vitest.config.ts`        | Test setup and shared test helpers; the Vitest settings                                                                                                                    |
| `vite.config.ts`, `vite/`              | The Vite settings and their plugins: `vite/wasmCore.ts` stops a production build whose WebAssembly core is missing or out of date, and warns in the dev server             |
| `src-tauri/src/main.rs`                | Entry point; calls `blocks2cpp_desktop::run()`                                                                                                                             |
| `src-tauri/src/lib.rs`                 | Start-up (folders, log, panic hook, backend, window), the command list, shutdown on exit                                                                                   |
| `src-tauri/src/commands/`              | One thin async adapter per IPC command (decode, call `b2c_app::Backend` on the blocking pool)                                                                              |
| `src-tauri/src/channels.rs`            | Tauri channels as the backend's event and byte sinks                                                                                                                       |
| `src-tauri/src/dialogs.rs`             | Native open, save, choose-g++ and trust dialogs (tauri-plugin-dialog, Rust API only)                                                                                       |
| `src-tauri/src/window.rs`              | The editor window, navigation rules, closing with unsaved changes                                                                                                          |
| `src-tauri/src/logging.rs`             | JSON-lines log files (5 × 5 MiB), `B2C_LOG`, panic hook                                                                                                                    |
| `src-tauri/src/e2e.rs`                 | End-to-end seams, feature `e2e-hooks` only (never in release builds)                                                                                                       |
| `src-tauri/tests/`                     | Command consistency, security settings, mock-runtime IPC, logging, end-to-end seams, Windows hardening (`hardening.rs`)                                                    |
| `src-tauri/windows-app-manifest.xml`   | The Windows application manifest (Common Controls v6, `longPathAware`), linked into every executable of the crate, tests included                                          |
| `src-tauri/tauri.conf.json`            | App, window, security (CSP, isolation) and bundle settings                                                                                                                 |
| `src-tauri/capabilities/`              | What the window may call in the backend                                                                                                                                    |
| `src-tauri/permissions/autogenerated/` | Permissions for our commands, generated by `build.rs`                                                                                                                      |
| `src-tauri/isolation/`                 | The isolation application that checks every IPC message                                                                                                                    |
| `src-tauri/isolation-tests/`           | Node tests of the isolation validator and hook                                                                                                                             |
| `src-tauri/icons/`                     | App icons; `icon.svg` is the source                                                                                                                                        |
