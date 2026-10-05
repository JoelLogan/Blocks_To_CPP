# Blocks2Cpp desktop app

The desktop editor: a React + TypeScript frontend with a [Blockly](https://github.com/RaspberryPiFoundation/blockly)
workspace, running in a [Tauri 2](https://v2.tauri.app) shell (WebView2 on Windows, WebKitGTK on
Linux). See [docs/spec/02-architecture.md](../../docs/spec/02-architecture.md) for the
architecture and [docs/spec/04-user-interface.md](../../docs/spec/04-user-interface.md) for the
user interface.

## What it contains (milestone M0)

The shell of the main window ([§4.1](../../docs/spec/04-user-interface.md#41-main-window-layout)):

- a top bar with the app name
- an empty Blockly workspace with the Scratch-style **Zelos** renderer, a grid, zoom controls
  and a trash can, in light and dark colours that follow the system setting
- placeholders for the toolbox (left), the C++ panel (right) and the console, problems and build
  output dock (bottom)
- a status bar with the version, which the frontend asks the backend for over IPC (the
  `app_version` command), so the shell shows that IPC works under the security settings below

The backend is deliberately thin and has no other commands yet. Toolbox, blocks, the live C++
view, building and running arrive with milestone M2
([roadmap](../../docs/spec/10-roadmap.md)).

### The app shell (milestone M2)

`src/app/` is the shell the M2 features plug into:

- **Start-up** (`bootstrap.ts`, `Root.tsx`): `app_info` first; when the backend's `ipcVersion`
  differs from `IPC_VERSION` in `@blocks2cpp/ipc-types`, the window shows a blocking error and
  nothing else runs. Then `app_subscribe` (once), the settings and the toolchain list, the
  shortcuts, Blockly's dialog overrides and the features (`src/features/index.ts`).
- **State** (`store/`): one Zustand store with a slice per concern (project, analysis, build, run,
  toolchains, settings, ui) and setters under `actions`.
- **Registries**: commands (`commands.ts`), full-window pages (`screens.ts`), the app event bus
  (`events.ts`), editor plugins (`editorPlugins.ts`) and the editor handle (`editor-types.ts`).
  Features receive all of them in a `FeatureContext` (`features.ts`).
- **Window**: the toolbar (project name and `•`, Debug/Release, ► Run, ■ Stop, Build, Settings),
  the docks (resizable by pointer and keyboard, collapsible), the status bar, and the run gate
  (`runGate.ts`) that explains why Run and Build are held back.
- **Dialogs** (`dialogs/`): alerts, confirmations, prompts and choices on Radix's accessible
  dialog; Blockly's own prompts go through them too.
- **Shortcuts** (`shortcuts.ts`): `F5` Run, `Shift+F5` Stop, `Ctrl+B` Build, `Ctrl+S` Save,
  everywhere except in a dialog or inside an element marked `data-b2c-shortcuts="off"`.

### The block editor (milestone M2)

`src/editor/` turns Blockly into the Blocks2Cpp editor
([§2.4.1](../../docs/spec/02-architecture.md), [§5.4](../../docs/spec/05-project-format.md)):

- **Workspace** (`EditorWorkspace.tsx`): Zelos, the b2c theme (following the system colour
  scheme), the `b2c_checker` connection checker, sounds off, the project format's zoom range
  (0.1–4.0) and a starting category toolbox that the toolbox plugin replaces. It installs the
  editor services, starts an editing session, publishes the `EditorHandle` and attaches
  `EDITOR_PLUGINS`.
- **Sync** (`sync/`): `loadModule` builds a module's canvas from a loaded document and
  `readModule` reads it back (statements as arrays, loose stacks as `stack`, positions as whole
  numbers within ±10⁷). Both are iterative. Blocks the editor cannot show faithfully (unknown
  types, values a field would change, inputs the mutator state lacks, misplaced shapes, newer
  versions) become placeholders that keep their data verbatim, so saving an unchanged project is
  byte-identical. Blockly's own serialisation and variable model are never persisted.
- **Preview** (`preview/`): 50 ms after the last change the session reads the canvas, runs the
  core's `canonical()` (project text, hash, dirty) and `preview()` (analysis, C++); stale results
  are dropped; a WASM trap replaces the core and sets the notice `trapRecovered`; a document that
  does not load keeps the last preview and sets `syncFailed`.
- **Duplicates** (`sync/duplicates.ts`): blocks Blockly creates (duplicate, paste, toolbox drags,
  redo) get fresh symbol IDs where theirs are already declared, so B2C-E0114/E0115 cannot occur.
- **Opening** (`load.ts`): `openDocumentInEditor` loads backend text through the core's loader
  (never `JSON.parse`), sets the store and shows the editor.
- **Saving**: call `editor.currentDocument()`; it reads the canvas now and captures the viewports
  (which are never part of the live document, so scrolling does not mark the project dirty).
- **Toolbox** (`toolbox/`, the `toolboxPlugin`): the catalog's categories in a Scratch-style
  continuous toolbox (category bubbles and one scrolling flyout), with presets and shadows, plus
  the dynamic Variables (with _Make a variable_), Loops and My Blocks categories, which follow the
  scope at the selected block.
- **Diagnostics on blocks** (`diagnostics/`, the `diagnosticsPlugin`): every diagnostic with a
  block shows on it as blockly-ext's `b2c_diagnostic` badge (✖ error, ⚠ warning, ℹ info, each
  with its own shape; the most serious wins), a severity outline (solid, dashed or dotted, so
  colour is never the only signal) and a mark on the field, input or token range it points at.
  A block inside a collapsed block shows on the collapsed block; one kept in a placeholder shows
  on the placeholder. The last build's compiler and linker messages show the same way, dimmed and
  marked "from the last build" once the project has changed, and are dropped when their block is
  deleted. Badges are display only: no Blockly events, no undo entries, never saved.
- **Two-way highlighting** (`highlight/`): the hovered and selected blocks go to the store
  (`ui.hoverBlock`, `ui.selection`) and the C++ tab highlights their code; a click in the code or
  on a problem selects the block (or the collapsed block around it) and scrolls to it or centres
  it.
- **Dock panels** (`src/app/panels.tsx`): the C++ tab and Problems are connected to the store.
  They load no Blockly: Problems' block paths (`main › repeat until › if`) use the block catalog
  the diagnostics plugin provides (`diagnostics/catalog.ts`).

## Layout

| Path                                   | Contents                                                                                                                               |
| -------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `index.html`, `src/main.tsx`           | Entry point (no inline scripts)                                                                                                        |
| `src/app/`                             | The app shell: start-up, state, registries, window, dialogs                                                                            |
| `src/features/`                        | The features, installed at start-up (`index.ts`)                                                                                       |
| `src/editor/`                          | The block editor: workspace, BDM ⇄ Blockly sync, live preview, services, module switcher, toolbox, diagnostics on blocks, highlighting |
| `src/panels/`                          | The C++ code panel, Problems, the console and Build output                                                                             |
| `src/lib/ipc.ts`                       | The typed client for the backend's commands                                                                                            |
| `src/test/`, `vitest.config.ts`        | Test setup and shared test helpers; the Vitest settings                                                                                |
| `src-tauri/src/main.rs`                | The Rust shell: window, navigation rules, commands                                                                                     |
| `src-tauri/tauri.conf.json`            | App, window, security (CSP, isolation) and bundle settings                                                                             |
| `src-tauri/capabilities/`              | What the window may call in the backend                                                                                                |
| `src-tauri/permissions/autogenerated/` | Permissions for our commands, generated by `build.rs`                                                                                  |
| `src-tauri/isolation/`                 | The isolation application that checks every IPC message                                                                                |
| `src-tauri/icons/`                     | App icons; `icon.svg` is the source                                                                                                    |

## Requirements

- Node.js 22.13 or later and pnpm 11 (the exact version is pinned in the root `package.json`;
  see [site/README.md](../../site/README.md#build-locally) for Corepack)
- Rust: the toolchain pinned in [`rust-toolchain.toml`](../../rust-toolchain.toml), installed
  automatically by `rustup`
- **Linux:** the WebKitGTK and GTK development packages. On Debian or Ubuntu:

  ```sh
  sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev \
    libsoup-3.0-dev libgtk-3-dev librsvg2-dev
  ```

  Other distributions: see [Tauri's prerequisites](https://v2.tauri.app/start/prerequisites/).

- **Windows:** the Microsoft C++ Build Tools (MSVC) and WebView2, which Windows 10 and 11 already
  have.

## Develop

From the repository root:

```sh
pnpm install --frozen-lockfile
pnpm --filter @blocks2cpp/b2c-core-wasm build   # the WebAssembly core the editor imports
pnpm desktop:dev          # the app, with the frontend served by Vite (hot reload)
```

The editor imports the WebAssembly core (`packages/b2c-core-wasm`), so build it once before the
first `dev`, `build` or test run, and again after changing the Rust crates it contains. Without
binaryen's `wasm-opt`, set `B2C_SKIP_WASM_OPT=1` (see that package's README).

`pnpm --filter @blocks2cpp/desktop dev` starts only the Vite dev server
(<http://localhost:1420>), so the frontend can be opened in a browser. There is no backend there:
the window starts without one (development builds only), and calls to it fail.

In `tauri dev` the window loads the page from the Vite dev server, which sends no Content Security
Policy. To check the app as it ships, with the CSP and the embedded frontend, build it (below);
`--debug` keeps the build quick.

Checks (CI runs them all, see [`.github/workflows/desktop.yml`](../../.github/workflows/desktop.yml)):

```sh
pnpm desktop:check                    # TypeScript, ESLint and Prettier, in every frontend package
python3 tools/check-package-layering.py                           # which package may use which
pnpm --filter @blocks2cpp/desktop run test                        # Vitest (happy-dom), headless, then test:isolation
pnpm --filter @blocks2cpp/desktop run test:coverage               # the same, with the 75% line gate
pnpm --filter @blocks2cpp/desktop run test:isolation              # the isolation hook's validator (node --test)
pnpm --filter @blocks2cpp/desktop run build                       # frontend build
cargo clippy --locked -p blocks2cpp-desktop --all-targets -- -D warnings
cargo test --locked -p blocks2cpp-desktop
```

Tests sit next to the code they test (`*.test.ts`, `*.test.tsx`); `src/test/` holds the setup and
the shared helpers, such as `expectNoAxeViolations` for the accessibility check every panel and
dialog test runs. [`packages/README.md`](../../packages/README.md) describes the frontend packages
and the template they follow.

The desktop crate is a member of the Cargo workspace but not a default member, so `cargo build`,
`cargo test` and `cargo clippy` at the root build only the compiler crates and the CLI and need
no webview libraries. Name the crate (`-p blocks2cpp-desktop`) to build the app.

## Build

```sh
pnpm desktop:build                                               # release build and installers
pnpm --filter @blocks2cpp/desktop tauri build --no-bundle        # the executable only
pnpm --filter @blocks2cpp/desktop tauri build --debug --no-bundle
```

The executable is written to `target/release/` (or `target/debug/`) at the repository root.
Installers are configured for Windows (MSI and NSIS) and Linux (AppImage, `.deb` and `.rpm`) in
`tauri.conf.json`. Signing and release builds come with the release workflow (milestone M6).

On Linux, if the window stays blank (some GPU drivers, virtual machines or a virtual display), start
the app with `WEBKIT_DISABLE_DMABUF_RENDERER=1`
([§2.8](../../docs/spec/02-architecture.md#28-cross-platform-considerations)).

## Security settings

These implement [docs/spec/08-security.md §8.8](../../docs/spec/08-security.md#88-webview-and-ipc-hardening):

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
  IPC message whose command and arguments are not on its allowlist, before the backend sees it.
- **Capabilities.** The `main` window may call `app_version` and nothing else: no core or plugin
  permissions, and no `fs`, `shell`, `http`, `process` or `dialog` plugins. `build.rs` declares
  the app's commands, so each one needs an explicit permission.
- **Navigation.** The window may navigate only within the app (and to the dev server during
  development), and requests to open new windows are denied (`src-tauri/src/main.rs`).
- `withGlobalTauri: false` (no `window.__TAURI__`), `freezePrototype: true`
  (`Object.prototype` is frozen against prototype pollution), and the web inspector is only in
  debug builds: the `devtools` Cargo feature is off.
- **Frontend rules** (ESLint): no `innerHTML`, `outerHTML`, `insertAdjacentHTML`,
  `document.write`, `eval`, `new Function`, string arguments to `setTimeout`/`setInterval`, or
  `dangerouslySetInnerHTML`; `eslint-plugin-no-unsanitized` checks the remaining DOM sinks.

### Adding an IPC command

1. Write the command in `src-tauri/src/` with `#[tauri::command]`, validate its input, and add it
   to `generate_handler!` in `main.rs`.
2. Add its name to `AppManifest::commands` in `build.rs`, and its `allow-…` permission to
   `capabilities/main-window.json`.
3. Add it, with a check of its arguments, to `ALLOWED_COMMANDS` in `isolation/index.js`.
4. Call it only through a typed function in `src/lib/ipc.ts`.

## Dependencies

Every dependency is pinned to an exact version and was at least 7 days old when added, as the
workspace policy requires ([§8.9](../../docs/spec/08-security.md#89-supply-chain)).

| Package                                  | Version (released)   | Licence           | Why                                                                                                                                                                                                                                            |
| ---------------------------------------- | -------------------- | ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `react`, `react-dom`                     | 19.3.0 (2026-09-09)  | MIT               | UI framework ([§2.1](../../docs/spec/02-architecture.md#21-technology-stack))                                                                                                                                                                  |
| `blockly`                                | 12.5.1 (2026-03-20)  | Apache-2.0        | The block editor and its Zelos renderer ([ADR-0002](../../docs/adr/0002-block-editor-blockly.md))                                                                                                                                              |
| `@blockly/continuous-toolbox`            | 7.0.9 (2026-04-09)   | Apache-2.0        | The Scratch-style continuous toolbox (§4.2): category bubbles and one scrolling flyout; Blockly team, npm provenance, no dependencies or install scripts, peer `blockly` ^12 (7.0.9 is the last release for Blockly 12; 13.x needs Blockly 13) |
| `@tauri-apps/api`                        | 2.12.0 (2026-09-26)  | Apache-2.0 OR MIT | `invoke` for IPC; same minor version as the `tauri` crate                                                                                                                                                                                      |
| `@tauri-apps/cli` (dev)                  | 2.12.0 (2026-09-26)  | Apache-2.0 OR MIT | `tauri dev`, `tauri build`, `tauri icon`                                                                                                                                                                                                       |
| `vite` (dev)                             | 8.3.1 (2026-09-24)   | MIT               | Dev server and bundler                                                                                                                                                                                                                         |
| `@vitejs/plugin-react` (dev)             | 6.1.1 (2026-08-28)   | MIT               | JSX and React Fast Refresh for Vite                                                                                                                                                                                                            |
| `typescript` (dev)                       | 5.9.3 (2025-09-30)   | Apache-2.0        | Type checking; TypeScript 5 as in §2.1 (typescript-eslint 8 supports up to 6.0)                                                                                                                                                                |
| `@types/react`, `@types/react-dom` (dev) | 19.3.0 (2026-09-09)  | MIT               | Types for React                                                                                                                                                                                                                                |
| `eslint` (dev)                           | 10.11.0 (2026-09-18) | MIT               | Linting                                                                                                                                                                                                                                        |
| `@eslint/js` (dev)                       | 10.0.1 (2026-02-06)  | MIT               | ESLint's recommended rules (no dependencies)                                                                                                                                                                                                   |
| `typescript-eslint` (dev)                | 8.70.1 (2026-09-21)  | MIT               | `strict-type-checked` and `stylistic-type-checked` rules                                                                                                                                                                                       |
| `eslint-plugin-react-hooks` (dev)        | 5.2.0 (2025-02-28)   | MIT               | Rules of Hooks and effect dependencies                                                                                                                                                                                                         |
| `eslint-plugin-no-unsanitized` (dev)     | 4.1.5 (2026-02-19)   | MPL-2.0           | Flags HTML-injection sinks (§8.8)                                                                                                                                                                                                              |
| `eslint-plugin-jsx-a11y` (dev)           | 6.10.2 (2024-10-26)  | MIT               | Accessibility rules for JSX, the strict set (§9.1, WCAG 2.2 AA in §4.8)                                                                                                                                                                        |
| `prettier` (dev)                         | 3.9.9 (2026-09-23)   | MIT               | Formatting                                                                                                                                                                                                                                     |
| `vitest` (dev)                           | 4.1.11 (2026-08-18)  | MIT               | Unit and component tests (§9.2); uses the app's Vite configuration                                                                                                                                                                             |
| `@vitest/coverage-v8` (dev)              | 4.1.11 (2026-08-18)  | MIT               | Line coverage and the 75% frontend gate (§9.2)                                                                                                                                                                                                 |
| `happy-dom` (dev)                        | 20.14.5 (2026-09-12) | MIT               | The DOM the tests run in, headless; Blockly runs in it                                                                                                                                                                                         |
| `@testing-library/react` (dev)           | 16.3.3 (2026-08-27)  | MIT               | Renders components in tests and finds elements by role and name                                                                                                                                                                                |
| `@testing-library/dom` (dev)             | 10.4.2 (2026-09-13)  | MIT               | The DOM queries behind `@testing-library/react` (its peer dependency)                                                                                                                                                                          |
| `axe-core` (dev)                         | 4.13.0 (2026-08-05)  | MPL-2.0           | Automated accessibility checks in component tests (`src/test/axe.ts`); no dependencies                                                                                                                                                         |
| `zustand`                                | 5.0.15 (2026-08-13)  | MIT               | The app's state store (`src/app/store/`); no dependencies                                                                                                                                                                                      |
| `@radix-ui/react-dialog`                 | 1.1.23 (2026-07-24)  | MIT               | Accessible, focus-trapped dialogs (§4.8), also for Blockly's prompts                                                                                                                                                                           |
| `@radix-ui/react-tabs`                   | 1.1.21 (2026-07-24)  | MIT               | The bottom dock's tabs: ARIA tab semantics and arrow-key navigation                                                                                                                                                                            |
| `@radix-ui/react-tooltip`                | 1.2.16 (2026-07-24)  | MIT               | Hints on the toolbar buttons, shown on hover and on keyboard focus                                                                                                                                                                             |
| `@codemirror/view`                       | 6.43.13 (2026-09-22) | MIT               | The read-only C++ view of the code panel (§4.3): decorations, gutters, no workers                                                                                                                                                              |
| `@codemirror/state`                      | 6.7.6 (2026-09-22)   | MIT               | CodeMirror's editor state (state fields and effects for highlights and diagnostics)                                                                                                                                                            |
| `@codemirror/language`                   | 6.12.4 (2026-06-25)  | MIT               | Syntax highlighting from the Lezer parse tree (`HighlightStyle`)                                                                                                                                                                               |
| `@codemirror/lang-cpp`                   | 6.0.3 (2025-06-19)   | MIT               | The Lezer C++ grammar (`@lezer/cpp`)                                                                                                                                                                                                           |
| `@lezer/highlight`                       | 1.2.4 (2026-09-24)   | MIT               | Highlighting tags for the code panel's colours (already a dependency of CodeMirror)                                                                                                                                                            |
| `@xterm/xterm`                           | 5.5.0 (2024-04-05)   | MIT               | The console (§4.5) with its DOM renderer; no dependencies                                                                                                                                                                                      |
| `@xterm/addon-fit`                       | 0.10.0 (2024-04-05)  | MIT               | Fits the terminal to the console panel                                                                                                                                                                                                         |

| Crate                 | Version (released)  | Licence           | Why                                                                        |
| --------------------- | ------------------- | ----------------- | -------------------------------------------------------------------------- |
| `tauri`               | 2.12.0 (2026-09-26) | Apache-2.0 OR MIT | The desktop shell ([ADR-0001](../../docs/adr/0001-desktop-shell-tauri.md)) |
| `tauri-build` (build) | 2.7.0 (2026-09-26)  | Apache-2.0 OR MIT | Embeds the configuration, capabilities and Windows resources               |

Notes:

- **xterm.js 5.5.0, not 6.0.0:** xterm.js 6.0.0 assigns `toString` to a plain object while it
  loads. With Tauri's `freezePrototype: true` that assignment throws (the property is read-only
  on the frozen `Object.prototype`, and modules are strict), so the frontend would fail to start
  in release builds. 5.5.0 works with a frozen prototype; `src/panels/frozen-prototype.test.tsx`
  freezes `Object.prototype` before loading every panel and fails if an upgrade brings the
  problem back.
- **CodeMirror 6 and xterm.js** add about 740 kB minified (about 218 kB gzip) to the bundle. Both
  inject their styles at run time, which the `style-src 'unsafe-inline'` exception of §8.8
  already covers. Nothing is loaded from the network, and neither starts a worker. The
  `@codemirror/*` and `@lezer/*` packages declare a `prepare` script, which pnpm never runs for
  registry packages.
- **`eslint-plugin-react-hooks` 5.2.0**, not 7.x: versions 6 and 7 depend on `@babel/core`, which
  depends on `semver@6.3.1`, and `trustPolicy: no-downgrade` rejects that version (it was
  published without the provenance that earlier `semver` releases have). Version 5.2.0 has no
  dependencies. It declares ESLint up to 9 as its peer, so pnpm warns about an unmet peer, but
  both of its rules work with ESLint 10. Move to 7.x (which adds the React Compiler rules) once
  that chain is resolved.
- **Blockly 12.5.1**, the newest 12.x, as the specification names Blockly 12. Blockly 12 depends
  on `jsdom` (about 40 packages) for use under Node.js; the browser bundle never includes it.
  Blockly 13 makes `jsdom` a peer dependency, so it would still be installed.
- **No `@types/node`:** `@types/node` 22 depends on `undici-types@6.21.0`, which the trust policy
  also rejects, so `vite.config.ts` reads Blockly's media through Rolldown's plugin file-system API
  instead of `node:fs`. (`happy-dom` brings `@types/node` 26 with `undici-types` 8 for its own
  types, which pass the policy; the app's TypeScript configuration still loads no Node.js types.)
- **`eslint-plugin-jsx-a11y` 6.10.2** is the newest release and declares ESLint up to 9 as its
  peer, so pnpm warns about an unmet peer as for `eslint-plugin-react-hooks`. Its rules use only
  the parts of the rule API that ESLint 10 kept, and the lint fails on JSX accessibility problems
  as it should. It adds about 110 development-only packages to the lockfile, mostly small
  ECMAScript polyfills (`es-abstract` and its helpers).
- **No `eslint-plugin-react`**, which §9.1 names: it depends on `semver@6.3.1`, which
  `trustPolicy: no-downgrade` rejects (checked with 7.37.5, the newest version). The one security rule
  needed from it, `react/no-danger`, is a `no-restricted-syntax` rule on `dangerouslySetInnerHTML`
  in `eslint.config.js` instead.
- **Radix UI** (`@radix-ui/react-dialog`, `react-tabs`, `react-tooltip`) brings 41 runtime packages:
  its own primitives, `@floating-ui/*` for positioning hints, `react-remove-scroll` and
  `aria-hidden` for modal dialogs, and `tslib`. All are MIT except `tslib` (0BSD), none has an
  install script, and every version was at least 70 days old when added. They work with
  `freezePrototype: true` (`src/app/freeze-prototype.test.tsx` checks this under Vitest; the
  end-to-end tests check the release build).
- **Test tooling** (`vitest`, `@vitest/coverage-v8`, `happy-dom`, Testing Library, `axe-core`) adds
  about 70 development-only packages; none of it reaches the app bundle.
- **`tauri` features:** `wry`, `compression`, `isolation`, `common-controls-v6` and `x11`.
  Left out: `devtools` (no web inspector in release builds), `dynamic-acl` (capabilities are fixed
  at build time), `tray-icon`, and `dbus` (unused, and it would need libdbus to build).
- Cargo has no minimum release age, so `Cargo.lock` holds Tauri's own crates and the crates it
  brought in at their newest versions that were at least 7 days old. Keep it that way when
  updating them, with `cargo update -p <crate> --precise <version>`.

## Icons

`src-tauri/icons/icon.svg` is the source. To regenerate the other icons:

```sh
pnpm --filter @blocks2cpp/desktop tauri icon src-tauri/icons/icon.svg -o /tmp/b2c-icons
cp /tmp/b2c-icons/{32x32.png,128x128.png,128x128@2x.png,icon.png,icon.ico} apps/desktop/src-tauri/icons/
```

Only these five are used (`bundle.icon` in `tauri.conf.json`): the macOS, Windows Store and
mobile icons that `tauri icon` also writes are not needed.
