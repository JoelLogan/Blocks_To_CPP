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

Milestone M2 ([roadmap](../../docs/spec/10-roadmap.md)) turns the shell into the editor: the
sections below describe what it adds.

### The backend (milestone M2)

`src-tauri/` is a thin adapter over `b2c_app::Backend` (`crates/b2c-app`), which holds the
project, build, run, toolchain, settings, trust and recovery services:

- **Commands** (`src-tauri/src/commands/`): one async adapter per command of the IPC contract
  (`crates/b2c-ipc`, 36 commands): it decodes the request, calls the backend on Tauri's blocking
  pool and returns its response or error. Build and run output stream to the editor through
  channels (`channels.rs`).
- **Native dialogs** (`dialogs.rs`): _Open_, _Save as_, _Choose g++_ and the trust confirmation
  are native dialogs raised by the backend (tauri-plugin-dialog, from Rust only). The editor
  cannot show or answer them, and it never names a path to open or save.
- **Window** (`window.rs`): the navigation rules, and closing with unsaved changes asks the
  editor first; quitting stops every build and running program.
- **Logs** (`logging.rs`): JSON-lines files (`blocks2cpp.log`, five files of 5 MiB) in the app's
  log folder, with no project content; `B2C_LOG=debug` adds paths. A panic is logged before the
  app exits.

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
- **Window**: the toolbar (the `≡` main menu, project name and `•`, Debug/Release, ► Run, ■ Stop,
  Build, Settings), the docks (resizable by pointer and keyboard, collapsible), the status bar, and
  the run gate (`runGate.ts`) that explains why Run and Build are held back. The main menu
  (`layout/MainMenu.tsx`) lists only the project commands a feature has registered.
- **Window banners** (`banners.tsx`): features register banners with `registerBanner`; `App`
  shows them under the toolbar in a polite live region. The analysis feature
  (`src/features/analysis/`) adds one while the latest change could not be checked
  (`syncFailed`) or the compiler core was restarted (`trapRecovered`).
- **Dialogs** (`dialogs/`): alerts, confirmations, prompts and choices on Radix's accessible
  dialog; Blockly's own prompts go through them too.
- **Shortcuts** (`shortcuts.ts`): `F5` Run, `Shift+F5` Stop, `Ctrl+B` Build, `Ctrl+S` Save,
  everywhere except in a dialog or inside an element marked `data-b2c-shortcuts="off"`. The
  reload keys (`Ctrl+R`, `Ctrl+Shift+R`, `Ctrl+F5`) are cancelled everywhere, and the webview's
  own context menu (which offers _Reload_) appears only over editable text, the console and
  selected text.

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
  During a block drag it returns the canvas as it was before the drag: the editor injects its own
  block dragger (`sync/drag.ts`) so the session hears of a drag before Blockly moves anything.
  Code that shows or resizes the editor container relies on the editor's resize observer (or
  calls `EditorSession.resize()`), never `Blockly.svgResize` directly, so a view the user has not
  moved stays unchanged for saving.
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
- **Clipboard** (`clipboard/`, the `clipboardPlugin`): copy, cut, paste and duplicate (`Ctrl+C`,
  `Ctrl+X`, `Ctrl+V`, `Ctrl+D`, the block and canvas menus, the webview's copy, cut and paste
  events, and the commands `edit.copy`, `edit.cut` and `edit.paste`) go through the core's
  validated format: `application/x-blocks2cpp+json` plus the C++ as `text/plain`, with an in-app
  copy as a fallback. A paste is validated like a project file, gets fresh IDs, re-binds
  references at its target and must keep the project within the format's limits; a refused paste
  changes nothing and lists the loader's codes. One paste is one undo step.
  A paste must also keep the saved (canonical) project text within 32 MiB.
  Paste, duplicate and cut are all or nothing: Blockly's undo events
  serialise statement chains recursively, so a chain too long for the
  engine's stack is refused with a notice and nothing changes
  (`clipboard/chains.ts`).
- **Keyboard** (`keyboard/`, the `keyboardPlugin`, attached last): keyboard navigation of the
  canvas and the toolbox on Blockly 12's core focus manager, cursor and shortcut registry (not
  `@blockly/keyboard-navigation`, which would replace the validated clipboard and drag blocks past
  the editing session; see 04 §4.7). The arrow keys move between blocks and their parts, `Enter`
  edits a field, `M` picks a block up so the arrow keys choose where it goes, `T` opens the
  toolbox, and `Escape` cancels; `keyboard/help.ts` is the whole key map. A polite live region
  announces the block or place the keyboard reaches, the canvas and the toolbox's blocks have
  names and describe their keys, and Blockly's own animations follow the reduced-motion setting.
  A move offers only the places a pointer drag could connect to, an add from the toolbox is one
  undo step, and the keyboard focus stays in the editor when the focused block or the toolbox's
  blocks go away (`keepFocus.ts`).
- **Two-way highlighting** (`highlight/`): the hovered and selected blocks go to the store
  (`ui.hoverBlock`, `ui.selection`) and the C++ tab highlights their code; a click in the code or
  on a problem selects the block (or the collapsed block around it) and scrolls to it or centres
  it.
- **Dock panels** (`src/app/panels.tsx`): the C++ tab and Problems are connected to the store.
  They load no Blockly: Problems' block paths (`main › repeat until › if`) use the block catalog
  the diagnostics plugin provides (`diagnostics/catalog.ts`).

### Projects (milestone M2)

`src/features/project/` (the `projectFeature`) is the project lifecycle
([§4.10](../../docs/spec/04-user-interface.md)):

- **Start page** (`StartPage.tsx`, the `start` screen): _New project_ from the bundled templates
  (_Empty_, _Hello World_), _Open…_ (the backend's native dialog; the webview never names a path),
  the recent projects (newest first, at most 10, each with a remove button; an entry whose file is
  gone offers to remove itself), and the sections other features add with
  `registerStartPageSection(id, Component, {order})` (the recovery offer). A file that does not
  load shows its `B2C-E01xx` problems there (`E0108` reads _made with a newer version of
  Blocks2Cpp (needs ≥ X)_) and nothing opens.
- **Commands**: `project.new` (asks for the template), `project.open`, `project.save` (`Ctrl+S`;
  a project without a file, or a backend `noPath`, goes to _Save as…_), `project.saveAs` and
  `project.close`, all reachable from the toolbar's `≡` main menu.
- **One project per window**: opening or creating another project first asks _Save_, _Don't
  save_ or _Cancel_; the previous project is closed (`project_close`) only once the new one is
  shown.
- **Saving** writes the editor's document (`EditorHandle.currentDocument()`, viewports captured)
  with `generator` set to this app and catalog, serialised by the core's canonical writer, so an
  unchanged project saves byte for byte the same. A file changed on disk is never overwritten:
  the save emits `project:changedOnDisk` for the external-change feature.
- **Unsaved changes**: the store's `dirty` flag is reported with `project_set_dirty` (one call at
  a time, latest value wins); the window's `closeRequested` asks _Save_, _Don't save_ or _Cancel_
  and then calls `app_quit`. An untouched new project has no unsaved changes.
- Operations run one at a time, in order; every backend error becomes a sentence for the user.
  Recovery (restore, autosave) and the external-change feature (reload) use the same queue
  through `features/project/link.ts` (`ProjectLink`), which `src/features/index.ts` creates and
  passes to the three features.

### Build and run (milestone M2)

`src/features/build-run/` (the `buildRunFeature`) builds and runs the open project
([§4.5](../../docs/spec/04-user-interface.md), [02 §2.4.2–§2.4.3](../../docs/spec/02-architecture.md)):

- **Build** (`build.start`, `Ctrl+B`) sends `build_start` with the project's canonical BDM text
  (read from the canvas through the core's `canonical()` at that moment, else the store's text)
  and the session's Debug/Release choice. Its channel fills `store.build` (progress, diagnostics,
  one `finished`) and the Build output tab: progress, toolchain notes (`B2C-T1011`–`T1013`), the
  compiler's own text and how the build ended. A `C:` error from blocks that are not Raw C++ is
  labelled _This looks like a bug in Blocks2Cpp_. A cancelled build puts the previous diagnostics
  back; an up-to-date build keeps the previous compiler warnings of the same content.
- **Run** (`run.start`, `F5`, and `run.again`) stops a running program first (its _Stopped_
  shows), builds unless the last successful build has the same content hash, configuration and
  compiler, then calls `run_start` with the console's size. `staleBuild` from the backend builds
  once more and retries. A failed build shows the first error (or the Build output).
- **Stop** (`run.stop`, `Shift+F5`) cancels the running build (`build_cancel`, also once a late
  build ID arrives) and stops the running program (`run_stop`, also once a late run ID arrives).
- **The console** (`src/app/panels.tsx`, `ConnectedConsolePanel`) attaches its terminal to the
  feature's `consoleBridge`. Output batches are written in order and acknowledged with `run_ack`
  at most every 100 ms, once xterm has processed them; `skipped` and `exit` are applied only after
  their `afterSeq` batches are written. Typed text goes to `run_input` as base64 chunks of at most
  64 KiB, within the backend's 200 calls and 1 MiB a second (with retries on `rateLimited`); the
  fitted size goes to `run_resize`. The header shows the state, the exit text, the elapsed time
  and the notices _Running with IDE helpers_ and _Process group only_. A new run writes a dim
  `── New run ──` separator after resetting the terminal's modes.
- Every channel message is checked against the contract before use (`channel.ts`); user-facing
  error text comes from `messages.ts`, never from the backend.

### Toolchain, settings and trust (milestone M2)

- **Toolchain page** (`src/features/toolchain/`, the `toolchainSetup` screen,
  [§4.6](../../docs/spec/04-user-interface.md)): one page that is the setup page while no
  compiler can build (what a compiler is; on Windows MSYS2 with
  `pacman -S mingw-w64-ucrt-x86_64-gcc` in the _MSYS2 UCRT64_ shell, or WinLibs through winget;
  on Linux the `apt`, `dnf` or `pacman` command chosen from `toolchain_setup_info`'s
  distribution, all three when unknown; copy buttons; the MSYS2 and WinLibs links through
  `open_help_link`) and the toolchain list otherwise (version, target, flavour, location,
  capabilities, health checks with the reason of each rejection, _Select as default_). _I
  installed it → Rescan_ and _Choose g++ manually…_ are always there. The page opens by itself
  once when discovery ends without a usable compiler, and from the status bar.
- **Settings page** (`src/features/settings/`, the `settings` screen,
  [§4.12](../../docs/spec/04-user-interface.md)): indent width, _Run on errors_, console
  scrollback, a link to the toolchain page and _Clear build cache_ (in-app confirmation, then the
  space freed and the builds kept). Every change is a partial `settings_update`, saved at once
  and applied live through the store. Notices about `settings.json` are listed on the page, and a
  dismissible window banner points to them. Other features add sections with
  `registerSettingsSection`.
- **Restricted Mode** (`src/features/trust/`,
  [08 §8.3](../../docs/spec/08-security.md)): a persistent banner under the toolbar while the
  open project is restricted (why: no trust record or changed outside the app; a stronger
  warning for a file from the Internet), with _Trust…_ (`trust_grant`, the backend's native
  dialog). The Settings page's _This project_ section says why a project is trusted and offers
  _Revoke trust_ when the trust comes from the project's own record.

### Recovery and outside changes (milestone M2)

- **Autosave** (`src/features/recovery/autosave.ts`): while the open project has unsaved
  changes, its canonical text goes to the backend as a recovery snapshot (`recovery_save`) every
  30 s and when the window loses focus; never twice the same text, one call at a time, failures
  logged by code only. The backend keeps snapshots in its recovery folder, never next to the
  project, and deletes them on a clean save or close.
- **Restore or discard** (`RecoveryOffer.tsx`, a start page section): at start-up the snapshots
  of instances that are no longer running (`recovery_list`) are listed with _Restore_ and
  _Discard_. Restore starts the compiler core first, then `recovery_restore`; the document goes
  through the core's loader (`openDocumentInEditor`) and opens with unsaved changes and the trust
  state the backend decided (08 §8.3.1). An open project with unsaved changes is settled first
  (_Save_, _Don't save_, _Cancel_) and closed once the restored one is shown. Discard asks first.
- **Changed on disk** (`src/features/external-change/`): the backend's file watcher reports a
  change to the open project file (`projectChangedOnDisk`), or a save is refused with
  `changedOnDisk`; the editor asks _Reload_, _Keep mine (save as…)_ or _Not now_. Reload
  (`project_reload`) replaces the workspace, clears undo and takes the reloaded file's trust
  state (an outside change to trust-relevant content shows Restricted Mode); it is not offered
  for a deleted or moved file. Keep mine runs `project.saveAs`.

## Layout

| Path                                   | Contents                                                                                                                                          |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `index.html`, `src/main.tsx`           | Entry point (no inline scripts)                                                                                                                   |
| `src/app/`                             | The app shell: start-up, state, registries, window, banners, dialogs                                                                              |
| `src/features/`                        | The features, installed at start-up (`index.ts`): project, build and run, toolchain, settings, trust, recovery, external changes                  |
| `src/editor/`                          | The block editor: workspace, BDM ⇄ Blockly sync, live preview, services, module switcher, toolbox, diagnostics on blocks, highlighting, clipboard |
| `src/panels/`                          | The C++ code panel, Problems, the console and Build output                                                                                        |
| `src/lib/ipc.ts`                       | The typed client for the backend's commands                                                                                                       |
| `src/e2e/`                             | The end-to-end test hook `window.__B2C_E2E__`, built only in Vite mode `e2e` (`pnpm build:e2e`) and absent from every other build                 |
| `e2e/`                                 | The end-to-end tests (selenium-webdriver, `tauri-driver`, Vitest); see [`e2e/README.md`](e2e/README.md)                                           |
| `src/test/`, `vitest.config.ts`        | Test setup and shared test helpers; the Vitest settings                                                                                           |
| `src-tauri/src/main.rs`                | Entry point; calls `blocks2cpp_desktop::run()`                                                                                                    |
| `src-tauri/src/lib.rs`                 | Start-up (folders, log, panic hook, backend, window), the command list, shutdown on exit                                                          |
| `src-tauri/src/commands/`              | One thin async adapter per IPC command (decode, call `b2c_app::Backend` on the blocking pool)                                                     |
| `src-tauri/src/channels.rs`            | Tauri channels as the backend's event and byte sinks                                                                                              |
| `src-tauri/src/dialogs.rs`             | Native open, save, choose-g++ and trust dialogs (tauri-plugin-dialog, Rust API only)                                                              |
| `src-tauri/src/window.rs`              | The editor window, navigation rules, closing with unsaved changes                                                                                 |
| `src-tauri/src/logging.rs`             | JSON-lines log files (5 × 5 MiB), `B2C_LOG`, panic hook                                                                                           |
| `src-tauri/src/e2e.rs`                 | End-to-end seams, feature `e2e-hooks` only (never in release builds)                                                                              |
| `src-tauri/tests/`                     | Command consistency, security settings, mock-runtime IPC, logging, end-to-end seams, Windows hardening (`hardening.rs`)                           |
| `src-tauri/windows-app-manifest.xml`   | The Windows application manifest (Common Controls v6, `longPathAware`), linked into every executable of the crate, tests included                 |
| `src-tauri/tauri.conf.json`            | App, window, security (CSP, isolation) and bundle settings                                                                                        |
| `src-tauri/capabilities/`              | What the window may call in the backend                                                                                                           |
| `src-tauri/permissions/autogenerated/` | Permissions for our commands, generated by `build.rs`                                                                                             |
| `src-tauri/isolation/`                 | The isolation application that checks every IPC message                                                                                           |
| `src-tauri/isolation-tests/`           | Node tests of the isolation validator and hook                                                                                                    |
| `src-tauri/icons/`                     | App icons; `icon.svg` is the source                                                                                                               |

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
cargo test --locked -p blocks2cpp-desktop                         # B2C_REQUIRE_GXX=1: fail without g++
cargo test --locked -p blocks2cpp-desktop --features e2e-hooks
```

`cargo build -p blocks2cpp-desktop --features e2e-hooks` (debug builds only: a `compile_error!`
refuses it otherwise) builds the app with its end-to-end seams, which read `B2C_E2E_ROOT`,
`B2C_E2E_DIALOGS` and `B2C_E2E_TOOLCHAIN_DIRS` ([ADR-0009](../../docs/adr/0009-e2e-tooling-and-test-seams.md)). `B2C_LOG=debug`
makes the log more detailed.

End-to-end tests (selenium-webdriver, `tauri-driver`, Vitest) live in [`e2e/`](e2e/README.md):
`pnpm --filter @blocks2cpp/desktop run e2e` runs them against the app built with
`tauri build --debug --no-bundle --features e2e-hooks --config '{"build":{"beforeBuildCommand":"pnpm build:e2e"}}'`
(on Linux under `xvfb-run -a`, with `webkit2gtk-driver`). `pnpm build:e2e` builds the frontend
with the test hook `window.__B2C_E2E__` (`src/e2e/`); no other build contains it. `tauri-driver`
2.1.0 (Apache-2.0 OR MIT) is a tool, installed with `cargo install tauri-driver --locked --version
2.1.0`, not a workspace crate. The guessing-game exit test is
[`e2e/specs/exit/guessing-game.e2e.ts`](e2e/specs/exit/guessing-game.e2e.ts).

Elements the tests find carry a `data-testid`; never rename or remove one. The stable ones are
`window-banners`, `main-menu`, `toolbar-run`, `toolbar-stop`, `toolbar-config`,
`toolbar-settings`, `project-name`, `module-switch`, `workspace`, `right-dock-toggle`,
`bottom-dock-toggle`, `dock-tab-console`, `dock-tab-problems`, `dock-tab-buildOutput`, the
`status-*` items (`status-save`, `status-config`, `status-standard`, `status-toolchain`,
`status-restricted`), `start-page`, `template-empty`, `template-helloWorld`, `start-open`,
`recent-item`, `recovery-offer`, `recovery-item`, the `restricted-banner*` parts, `code-panel`,
`problems-panel`, `problems-summary`, `problem-row`, the `console-*` parts (`console-panel`,
`console-header`, `console-state`, `console-elapsed`, `console-stop`, `console-run-again`,
`console-clear`, `console-terminal`, `console-notice`), `build-output-panel`,
`build-output-line`, `keyboard-announcer`, `app-dialog`, and the settings and toolchain pages'
`settings-*` and `toolchain-*` IDs.

**Windows hardening.** `blocks2cpp_desktop::run()` first restricts where DLLs are loaded from
(`harden_dll_search`: the system folder and the app's own; a failure is logged and start-up
continues). The application manifest declares `longPathAware`, so paths beyond 260 characters
work where Windows allows them, and CI checks the release executable's embedded manifest with
[`tools/check-windows-manifest.ps1`](../../tools/check-windows-manifest.ps1)
(`B2C_CHECK_EXE=<exe> cargo test -p blocks2cpp-desktop --test hardening a_given_executable` does
the same check on any system). Every build adds the report-only header
`Content-Security-Policy-Report-Only: require-trusted-types-for 'script'` to the app's own HTML
(08 §8.8), and logs _"added the Trusted Types report-only policy"_ at debug level when it does.

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
  (`Object.prototype` is frozen against prototype pollution), and the web inspector is only in
  debug builds: the `devtools` Cargo feature is off.
- **Frontend rules** (ESLint): no `innerHTML`, `outerHTML`, `insertAdjacentHTML`,
  `document.write`, `eval`, `new Function`, string arguments to `setTimeout`/`setInterval`, or
  `dangerouslySetInnerHTML`; `eslint-plugin-no-unsanitized` checks the remaining DOM sinks.

### Adding an IPC command

1. Add the request and response types and the `COMMANDS` entry in `crates/b2c-ipc`, then
   regenerate (`B2C_UPDATE_IPC=1 cargo test -p b2c-ipc --features ts --test generate`): the
   TypeScript client, the isolation allowlist and the isolation samples follow.
2. Implement the method on `b2c_app::Backend`.
3. Add a thin adapter in `src-tauri/src/commands/<area>.rs` and its path to `generate_handler!`
   in `src/lib.rs`.
4. Add `allow-<name>` (underscores become dashes) to `capabilities/main-window.json`; `build.rs`
   takes the command names from `b2c_ipc::COMMAND_NAMES` and writes
   `permissions/autogenerated/<name>.toml` (commit it).
5. `cargo test -p blocks2cpp-desktop --test consistency` checks that all five places agree.

## Dependencies

Every dependency is pinned to an exact version and was at least 7 days old when added, as the
workspace policy requires ([§8.9](../../docs/spec/08-security.md#89-supply-chain)).

| Package                                  | Version (released)   | Licence           | Why                                                                                                                                                                                                                                                                                                                                                                              |
| ---------------------------------------- | -------------------- | ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `react`, `react-dom`                     | 19.3.0 (2026-09-09)  | MIT               | UI framework ([§2.1](../../docs/spec/02-architecture.md#21-technology-stack))                                                                                                                                                                                                                                                                                                    |
| `blockly`                                | 12.5.1 (2026-03-20)  | Apache-2.0        | The block editor and its Zelos renderer ([ADR-0002](../../docs/adr/0002-block-editor-blockly.md))                                                                                                                                                                                                                                                                                |
| `@blockly/continuous-toolbox`            | 7.0.9 (2026-04-09)   | Apache-2.0        | The Scratch-style continuous toolbox (§4.2): category bubbles and one scrolling flyout; Blockly team, npm provenance, no dependencies or install scripts, peer `blockly` ^12 (7.0.9 is the last release for Blockly 12; 13.x needs Blockly 13)                                                                                                                                   |
| `@tauri-apps/api`                        | 2.12.0 (2026-09-26)  | Apache-2.0 OR MIT | `invoke` for IPC; same minor version as the `tauri` crate                                                                                                                                                                                                                                                                                                                        |
| `@tauri-apps/cli` (dev)                  | 2.12.0 (2026-09-26)  | Apache-2.0 OR MIT | `tauri dev`, `tauri build`, `tauri icon`                                                                                                                                                                                                                                                                                                                                         |
| `vite` (dev)                             | 8.3.1 (2026-09-24)   | MIT               | Dev server and bundler                                                                                                                                                                                                                                                                                                                                                           |
| `@vitejs/plugin-react` (dev)             | 6.1.1 (2026-08-28)   | MIT               | JSX and React Fast Refresh for Vite                                                                                                                                                                                                                                                                                                                                              |
| `typescript` (dev)                       | 5.9.3 (2025-09-30)   | Apache-2.0        | Type checking; TypeScript 5 as in §2.1 (typescript-eslint 8 supports up to 6.0)                                                                                                                                                                                                                                                                                                  |
| `@types/react`, `@types/react-dom` (dev) | 19.3.0 (2026-09-09)  | MIT               | Types for React                                                                                                                                                                                                                                                                                                                                                                  |
| `eslint` (dev)                           | 10.11.0 (2026-09-18) | MIT               | Linting                                                                                                                                                                                                                                                                                                                                                                          |
| `@eslint/js` (dev)                       | 10.0.1 (2026-02-06)  | MIT               | ESLint's recommended rules (no dependencies)                                                                                                                                                                                                                                                                                                                                     |
| `typescript-eslint` (dev)                | 8.70.1 (2026-09-21)  | MIT               | `strict-type-checked` and `stylistic-type-checked` rules                                                                                                                                                                                                                                                                                                                         |
| `eslint-plugin-react-hooks` (dev)        | 5.2.0 (2025-02-28)   | MIT               | Rules of Hooks and effect dependencies                                                                                                                                                                                                                                                                                                                                           |
| `eslint-plugin-no-unsanitized` (dev)     | 4.1.5 (2026-02-19)   | MPL-2.0           | Flags HTML-injection sinks (§8.8)                                                                                                                                                                                                                                                                                                                                                |
| `eslint-plugin-jsx-a11y` (dev)           | 6.10.2 (2024-10-26)  | MIT               | Accessibility rules for JSX, the strict set (§9.1, WCAG 2.2 AA in §4.8)                                                                                                                                                                                                                                                                                                          |
| `prettier` (dev)                         | 3.9.9 (2026-09-23)   | MIT               | Formatting                                                                                                                                                                                                                                                                                                                                                                       |
| `vitest` (dev)                           | 4.1.11 (2026-08-18)  | MIT               | Unit and component tests (§9.2); uses the app's Vite configuration                                                                                                                                                                                                                                                                                                               |
| `@vitest/coverage-v8` (dev)              | 4.1.11 (2026-08-18)  | MIT               | Line coverage and the 75% frontend gate (§9.2)                                                                                                                                                                                                                                                                                                                                   |
| `happy-dom` (dev)                        | 20.14.5 (2026-09-12) | MIT               | The DOM the tests run in, headless; Blockly runs in it. The isolation tests parse `index.html` with it                                                                                                                                                                                                                                                                           |
| `@testing-library/react` (dev)           | 16.3.3 (2026-08-27)  | MIT               | Renders components in tests and finds elements by role and name                                                                                                                                                                                                                                                                                                                  |
| `@testing-library/dom` (dev)             | 10.4.2 (2026-09-13)  | MIT               | The DOM queries behind `@testing-library/react` (its peer dependency)                                                                                                                                                                                                                                                                                                            |
| `axe-core` (dev)                         | 4.13.0 (2026-08-05)  | MPL-2.0           | Automated accessibility checks in component tests (`src/test/axe.ts`); no dependencies                                                                                                                                                                                                                                                                                           |
| `@testing-library/user-event` (dev)      | 14.6.7 (2026-09-02)  | MIT               | Types and tabs like a person in the keyboard tests (`src/app/layout/accessibility.test.tsx`); no dependencies                                                                                                                                                                                                                                                                    |
| `selenium-webdriver` (dev)               | 4.49.0 (2026-09-09)  | Apache-2.0        | The end-to-end tests' WebDriver client (`e2e/`, [ADR-0009](../../docs/adr/0009-e2e-tooling-and-test-seams.md)); WebdriverIO fails `trustPolicy: no-downgrade`. npm provenance; brings `ws`, `tmp`, `jszip` (with `lie`, `pako`, `readable-stream` 2 and their helpers) and `@bazel/runfiles`, all development-only, none with an install script. 4.50.0 was less than 7 days old |
| `@types/selenium-webdriver` (dev)        | 4.35.7 (2026-09-14)  | MIT               | Types for `selenium-webdriver` (it ships none)                                                                                                                                                                                                                                                                                                                                   |
| `pixelmatch` (dev)                       | 7.2.0 (2026-04-29)   | ISC               | The visual diff's pixel comparison (`e2e/visual/`); no install script. 8.0.0 was less than 7 days old                                                                                                                                                                                                                                                                            |
| `pngjs` (dev)                            | 7.0.0 (2023-02-20)   | MIT               | Reads and writes the visual diff's PNG screenshots and baselines; no dependencies                                                                                                                                                                                                                                                                                                |
| `@types/pngjs` (dev)                     | 6.0.5 (2024-05-02)   | MIT               | Types for `pngjs`                                                                                                                                                                                                                                                                                                                                                                |
| `@types/node` (dev)                      | 24.19.0              | MIT               | Node.js types for the end-to-end harness only (`e2e/tsconfig.json`); the app's own TypeScript configuration loads none. The same version as `b2c-core-wasm` and `catalog-gen`                                                                                                                                                                                                    |
| `zustand`                                | 5.0.15 (2026-08-13)  | MIT               | The app's state store (`src/app/store/`); no dependencies                                                                                                                                                                                                                                                                                                                        |
| `@radix-ui/react-dialog`                 | 1.1.23 (2026-07-24)  | MIT               | Accessible, focus-trapped dialogs (§4.8), also for Blockly's prompts                                                                                                                                                                                                                                                                                                             |
| `@radix-ui/react-tabs`                   | 1.1.21 (2026-07-24)  | MIT               | The bottom dock's tabs: ARIA tab semantics and arrow-key navigation                                                                                                                                                                                                                                                                                                              |
| `@radix-ui/react-tooltip`                | 1.2.16 (2026-07-24)  | MIT               | Hints on the toolbar buttons, shown on hover and on keyboard focus                                                                                                                                                                                                                                                                                                               |
| `@codemirror/view`                       | 6.43.13 (2026-09-22) | MIT               | The read-only C++ view of the code panel (§4.3): decorations, gutters, no workers                                                                                                                                                                                                                                                                                                |
| `@codemirror/state`                      | 6.7.6 (2026-09-22)   | MIT               | CodeMirror's editor state (state fields and effects for highlights and diagnostics)                                                                                                                                                                                                                                                                                              |
| `@codemirror/language`                   | 6.12.4 (2026-06-25)  | MIT               | Syntax highlighting from the Lezer parse tree (`HighlightStyle`)                                                                                                                                                                                                                                                                                                                 |
| `@codemirror/lang-cpp`                   | 6.0.3 (2025-06-19)   | MIT               | The Lezer C++ grammar (`@lezer/cpp`)                                                                                                                                                                                                                                                                                                                                             |
| `@lezer/highlight`                       | 1.2.4 (2026-09-24)   | MIT               | Highlighting tags for the code panel's colours (already a dependency of CodeMirror)                                                                                                                                                                                                                                                                                              |
| `@xterm/xterm`                           | 5.5.0 (2024-04-05)   | MIT               | The console (§4.5) with its DOM renderer; no dependencies                                                                                                                                                                                                                                                                                                                        |
| `@xterm/addon-fit`                       | 0.10.0 (2024-04-05)  | MIT               | Fits the terminal to the console panel                                                                                                                                                                                                                                                                                                                                           |

| Crate                        | Version (released)  | Licence           | Why                                                                                                                                                                                                                                                                              |
| ---------------------------- | ------------------- | ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tauri`                      | 2.12.0 (2026-09-26) | Apache-2.0 OR MIT | The desktop shell ([ADR-0001](../../docs/adr/0001-desktop-shell-tauri.md))                                                                                                                                                                                                       |
| `tauri-build` (build)        | 2.7.0 (2026-09-26)  | Apache-2.0 OR MIT | Embeds the configuration, capabilities and Windows resources                                                                                                                                                                                                                     |
| `tauri-plugin-dialog`        | 2.8.0 (2026-09-26)  | Apache-2.0 OR MIT | Native open, save and message dialogs, from Rust only (no JavaScript permission). Default features off except `gtk3`: rfd 0.16.0's GTK 3 backend on Linux, no XDG portal and no D-Bus. Brings `tauri-plugin-fs` 2.6.0 (for its `FilePath` type; the fs plugin is not registered) |
| `rfd` (Windows only)         | 0.16.0              | MIT               | The Windows trust dialog with _Stay in Restricted Mode_ as its first and default button (08 §8.3.1). No default features, `common-controls-v6`; already in the lockfile through `tauri-plugin-dialog` with the same features, so no new code is compiled                         |
| `tracing-subscriber`         | 0.3.23 (2026-03-13) | MIT               | The span registry under the app's own JSON-lines log layer. Features `registry` and `std` only: no `fmt`, ANSI, `log` bridge or `EnvFilter`                                                                                                                                      |
| `notify` (through `b2c-app`) | 8.2.0 (2025-08-03)  | CC0-1.0           | The file watcher of open projects (05 §5.10): inotify on Linux, `ReadDirectoryChangesW` on Windows, one non-recursive watch per project folder. No default features (no macOS FSEvents). Its events only wake the backend's own 300 ms debounce and SHA-256 check                |

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
- **Node.js types only for the end-to-end harness:** `@types/node` 22 depends on
  `undici-types@6.21.0`, which the trust policy also rejects, so the app's TypeScript configuration
  loads no Node.js types and `vite.config.ts` reads Blockly's media through Rolldown's plugin
  file-system API instead of `node:fs`. Only the end-to-end harness (`e2e/tsconfig.json`) loads
  them, with `@types/node` 24.19.0, whose `undici-types` passes the policy. (`happy-dom` brings
  `@types/node` 26 with `undici-types` 8 for its own types, which pass too.)
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
- **`tauri-plugin-dialog` 2.8.0, `tauri-plugin` 2.7.0, `lazy_static` 1.5.0**: the newer 2.8.1,
  2.7.1 and 1.5.1 were less than 7 days old when added. `rfd` 0.16.0 on Windows brings
  `windows-sys` 0.60.2, which `notify` uses too.
- **`notify` 8.2.0**, the newest stable release (9.0 is still a release candidate). Without
  default features it brings `inotify`, `inotify-sys`, `mio`, `walkdir`, `notify-types`,
  `bitflags` and `log` on Linux (most already in the lockfile through Tauri), and `walkdir` and
  `windows-sys` 0.60 on Windows.
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
