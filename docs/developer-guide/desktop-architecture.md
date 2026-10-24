# Desktop app architecture

The desktop editor: a React + TypeScript frontend with a
[Blockly](https://github.com/RaspberryPiFoundation/blockly) workspace, running in a
[Tauri 2](https://v2.tauri.app) shell (WebView2 on Windows, WebKitGTK on Linux). See
[docs/spec/02-architecture.md](../spec/02-architecture.md) for the architecture and
[docs/spec/04-user-interface.md](../spec/04-user-interface.md) for the user interface. This page
describes how the app is built inside; the code map is in the
[app's README](../../apps/desktop/README.md#code-map). Paths are relative to `apps/desktop/`
unless they start with `crates/` or `packages/`.

- [What it contains](#what-it-contains)
- [The backend](#the-backend-milestone-m2)
- [The app shell](#the-app-shell-milestone-m2)
- [The block editor](#the-block-editor-milestone-m2)
- [Projects](#projects-milestone-m2)
- [Build and run](#build-and-run-milestone-m2)
- [Toolchain, settings and trust](#toolchain-settings-and-trust-milestone-m2)
- [Recovery and outside changes](#recovery-and-outside-changes-milestone-m2)
- [The desktop crate in the Cargo workspace](#the-desktop-crate-in-the-cargo-workspace)
- [Blockly seams (ADR-0014)](#blockly-seams-adr-0014)

## What it contains

The desktop editor of milestone M2 ([roadmap](../spec/10-roadmap.md#101-milestones)): the start
page, the block editor with its toolbox, the live C++ panel, Problems, the console and the build
output, the toolchain and settings pages, and Restricted Mode, on top of the M0 shell (the window,
the Content Security Policy, the isolation pattern and the capabilities of the
[security settings](desktop-security.md)). [Getting started](../user-guide/getting-started.md)
shows the app from a user's side; the sections below describe how it is built.

## The backend (milestone M2)

`src-tauri/` is a thin adapter over `b2c_app::Backend` (`crates/b2c-app`), which holds the
project, build, run, toolchain, settings, trust and recovery services:

- **Commands** (`src-tauri/src/commands/`): one async adapter per command of the IPC contract
  (`crates/b2c-ipc`, 36 commands): it decodes the request, calls the backend on Tauri's blocking
  pool and returns its response or error. Build and run output stream to the editor through
  channels (`channels.rs`). [Adding an IPC command](recipes.md#adding-an-ipc-command) lists the
  places a new command touches.
- **Native dialogs** (`dialogs.rs`): _Open_, _Save as_, _Choose g++_ and the trust confirmation
  are native dialogs raised by the backend (tauri-plugin-dialog, from Rust only). The editor
  cannot show or answer them, and it never names a path to open or save.
- **Window** (`window.rs`): the navigation rules, and closing with unsaved changes asks the
  editor first; quitting stops every build and running program.
- **Logs** (`logging.rs`): JSON-lines files (`blocks2cpp.log`, five files of 5 MiB) in the app's
  log folder, with no project content; `B2C_LOG=debug` adds paths. A panic is logged before the
  app exits.

## The app shell (milestone M2)

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
  (`layout/MainMenu.tsx`) lists only the commands something has registered; its _Undo_ and
  _Redo_ are enabled from the block editor's undo and redo stacks. The project name
  (`layout/ProjectTitle.tsx`) is a button that renames the project in place and provides
  `project.rename`; the editor applies the name (`EditorHandle.updateProjectInfo`).
- **Window banners** (`banners.tsx`): features register banners with `registerBanner`; `App`
  shows them under the toolbar in a polite live region. The analysis feature
  (`src/features/analysis/`) adds one while the latest change could not be checked
  (`syncFailed`) or the compiler core was restarted (`trapRecovered`).
- **Dialogs** (`dialogs/`): alerts, confirmations, prompts and choices on Radix's accessible
  dialog; Blockly's own prompts go through them too, and Blockly's pop-ups (drop-downs, field
  editors, menus, tooltips) close before a dialog opens, the console's link dialog included
  (`dialogs.beforeModal()`), and when a full-window page replaces the editor
  (`blocklyPopupsPlugin`).
- **Shortcuts** (`shortcuts.ts`): `F5` Run, `Shift+F5` Stop, `Ctrl+B` Build, `Ctrl+S` Save,
  everywhere except in a dialog or inside an element marked `data-b2c-shortcuts="off"`; `Ctrl+Z`
  and `Ctrl+Y` (or `Ctrl+Shift+Z`) undo and redo the block editor's changes (`edit.undo`,
  `edit.redo`) while it is shown, except in text fields, Blockly's own areas (Blockly handles
  them there) and the console while a program runs. The reload keys (`Ctrl+R`, `Ctrl+Shift+R`,
  `Ctrl+F5`) are cancelled everywhere, and the webview's own context menu (which offers _Reload_)
  appears only over editable text, the console and selected text.

## The block editor (milestone M2)

`src/editor/` turns Blockly into the Blocks2Cpp editor
([§2.4.1](../spec/02-architecture.md#241-editing-in-the-webview-no-ipc),
[§5.4](../spec/05-project-format.md#54-block-nodes)):

- **Workspace** (`EditorWorkspace.tsx`): Zelos, the b2c theme (following the system colour
  scheme), the `b2c_checker` connection checker and the `b2c_previewer` connection previewer,
  sounds off, the project format's zoom range (0.1–4.0), a trash can that keeps nothing
  (`maxTrashcanContents: 0`; Undo brings deleted blocks back) and a starting category toolbox that
  the toolbox plugin replaces. It installs the editor services, starts an editing session,
  publishes the `EditorHandle` and attaches `EDITOR_PLUGINS`.
- **Canvas controls** (`view/`): _Zoom out_, _Zoom in_, _Reset zoom_ (to 100 %, showing the
  zoom now) and _Show all blocks_ (scrolls to the blocks, keeps the zoom) as named HTML buttons
  at the canvas's top right, placed as a Blockly positionable component so they never cover the
  toolbox; `Ctrl+=`, `Ctrl+-` and `Ctrl+0` on the canvas (the keyboard plugin) and `Ctrl` +
  wheel zoom too. Blockly's own zoom controls are off.
- **Sync** (`sync/`): `loadModule` builds a module's canvas from a loaded document and
  `readModule` reads it back (statements as arrays, loose stacks as `stack`, positions as whole
  numbers within ±10⁷). Both are iterative. Blocks the editor cannot show faithfully (unknown
  types, values a field would change, inputs the mutator state lacks, misplaced shapes, newer
  versions) become placeholders that keep their data verbatim, so saving an unchanged project is
  byte-identical. Blockly's own serialisation and variable model are never persisted.
- **Preview** (`preview/`): 50 ms after the last change the session reads the canvas and runs the
  core's `update()` once (one load: the analysis, C++, the content hash and the hash of the
  canonical text, which `dirty` compares with the saved text's); the text itself is made only to
  save, build or autosave; stale results are dropped; a WASM trap replaces the core and sets the
  notice `trapRecovered`; a document that does not load keeps the last preview and sets
  `syncFailed`.
- **Duplicates** (`sync/duplicates.ts`): blocks Blockly creates (duplicate, paste, toolbox drags,
  redo) get fresh symbol IDs where theirs are already declared, so B2C-E0114/E0115 cannot occur.
- **Undo** (`sync/undo.ts`): Blockly records a change for undo only when it delivers the change's
  events, in a task after the next animation frame. Every undo and redo (Blockly's keys and canvas
  menu items, whose actions the session replaces, and `EditorSession.undo` behind `edit.undo` and
  `edit.redo`) therefore acts after the next frame and a task, so a `Ctrl+Z` right after a change
  undoes that change; undos act in the order they were asked for.
- **Opening** (`load.ts`): `openDocumentInEditor` loads backend text through the core's loader
  (never `JSON.parse`), sets the store and shows the editor.
- **Saving**: call `editor.currentDocument()`; it reads the canvas now and captures the viewports
  (which are never part of the live document, so scrolling does not mark the project dirty).
  During a block drag it returns the canvas as it was before the drag: the editor injects its own
  block dragger (`sync/drag.ts`) so the session hears of a drag before Blockly moves anything.
  The dragger also gives each dragged block a drag strategy that previews no connection while the
  pointer is over the toolbox or the trash can, so a drop there deletes only the dragged blocks
  (Blockly 12 would connect them first and delete the blocks below too). Code that shows or
  resizes the editor container relies on the editor's resize observer (or calls
  `EditorSession.resize()`), never `Blockly.svgResize` directly, so a view the user has not moved
  stays unchanged for saving.
- **Toolbox** (`toolbox/`, the `toolboxPlugin`): the catalog's categories in a Scratch-style
  continuous toolbox (category bubbles and one scrolling flyout), with presets and shadows, plus
  the dynamic Program, Variables (with _Make a variable_) and Functions (with _Your functions_)
  categories, which follow the project's variables and functions and the module shown, not the
  selection. A block taken from the toolbox gets a free default name, or a variable visible where
  it is dropped, as part of the drop (`landing.ts`).
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
  changes nothing and lists the loader's codes. One paste is one undo step. A paste must also keep
  the saved (canonical) project text within 32 MiB. Paste, duplicate and cut are all or nothing:
  Blockly's undo events serialise statement chains recursively, so a chain too long for the
  engine's stack is refused with a notice and nothing changes (`clipboard/chains.ts`).
- **Menus** (`menus/`, the `menusPlugin`): Blockly's own block and canvas menu items without
  _Inline Inputs_ (never saved), and a _Delete N blocks_ that counts the blocks it deletes (not
  their expression slots), asks with _Cancel_ as the default and deletes in one undo step; _Clean
  up blocks_ stays. Blockly's items are put back when the last editor detaches.
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
  They load no Blockly: Problems' block paths (`when program starts › repeat until › if`) use the
  block catalog the diagnostics plugin provides (`diagnostics/catalog.ts`).

## Projects (milestone M2)

`src/features/project/` (the `projectFeature`) is the project lifecycle
([§4.10](../spec/04-user-interface.md#410-project-lifecycle-ux)):

- **Start page** (`StartPage.tsx`, the `start` screen): _New project_ from the bundled templates
  (_Empty_, _Hello World_), _Open…_ (the backend's native dialog; the webview never names a path),
  the recent projects (newest first, at most 10, each with a remove button; an entry whose file is
  gone offers to remove itself), and the sections other features add with
  `registerStartPageSection(id, Component, {order})` (the recovery offer). A file that does not
  load shows its `B2C-E01xx` problems there (`E0108` reads _made with a newer version of
  Blocks2Cpp_, naming no version from the file) and nothing opens.
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

## Build and run (milestone M2)

`src/features/build-run/` (the `buildRunFeature`) builds and runs the open project
([§4.5](../spec/04-user-interface.md#45-console-integrated-terminal),
[02 §2.4.2–§2.4.3](../spec/02-architecture.md#242-building-ipc-to-the-backend)):

- **Build** (`build.start`, `Ctrl+B`) sends `build_start` with the project's canonical BDM text
  (read from the canvas through the core's `canonical()` at that moment, else the store's
  document) and the session's Debug/Release choice. Its channel fills `store.build` (progress,
  diagnostics, one `finished`) and the Build output tab: progress, toolchain notes
  (`B2C-T1011`–`T1013`), the compiler's own text and how the build ended. A `C:` error from blocks
  that are not Raw C++ is labelled _This looks like a bug in Blocks2Cpp_. A cancelled build puts
  the previous diagnostics back; an up-to-date build keeps the previous compiler warnings of the
  same content.
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
  `── New run ──` separator after resetting the terminal's modes and scrolls the console to the
  end; _Clear_ empties it, keeping only the prompt line of a running program. The console scrolls
  the wheel by whole lines itself (`panels/console/wheel.ts`), refits at most once a frame
  (`refit.ts`), and gives screen readers its output except during floods (`screenReader.ts`).
- Every channel message is checked against the contract before use (`channel.ts`); user-facing
  error text comes from `messages.ts`, never from the backend.

## Toolchain, settings and trust (milestone M2)

- **Toolchain page** (`src/features/toolchain/`, the `toolchainSetup` screen,
  [§4.6](../spec/04-user-interface.md#46-toolchain-setup-experience)): one page that is the setup
  page while no compiler can build (what a compiler is; on Windows MSYS2 with
  `pacman -S mingw-w64-ucrt-x86_64-gcc` in the _MSYS2 UCRT64_ shell, or WinLibs through winget;
  on Linux the `apt`, `dnf` or `pacman` command chosen from `toolchain_setup_info`'s
  distribution, all three when unknown; copy buttons; the MSYS2 and WinLibs links through
  `open_help_link`) and the toolchain list otherwise (version, target, flavour, location,
  capabilities, health checks with the reason of each rejection, _Select as default_). _I
  installed it → Rescan_ and _Choose g++ manually…_ are always there. The page opens by itself
  once when discovery ends without a usable compiler, and from the status bar.
- **Settings page** (`src/features/settings/`, the `settings` screen,
  [§4.12](../spec/04-user-interface.md#412-settings-page)): indent width, _Run on errors_,
  console scrollback, a link to the toolchain page and _Clear build cache_ (in-app confirmation,
  then the space freed and the builds kept). Every change is a partial `settings_update`, saved at once
  and applied live through the store. The feature also remembers the editor's layout
  (`layout.ts`): the shell hands each change of the toolbox's width or a dock's size or
  visibility to it (`app/layout/rememberedLayout.ts`), and it writes them as `ui.layout` 500 ms
  after the last one. Notices about `settings.json` are listed on the page, and a dismissible
  window banner points to them. Other features add sections with `registerSettingsSection`.
- **Restricted Mode** (`src/features/trust/`,
  [08 §8.3](../spec/08-security.md#83-workspace-trust-and-restricted-mode)): a persistent banner
  under the toolbar while the open project is restricted (why: no trust record or changed outside
  the app; a stronger warning for a file from the Internet), with _Trust…_ (`trust_grant`, the
  backend's native dialog). The Settings page's _This project_ section says why a project is
  trusted and offers _Revoke trust_ when the trust comes from the project's own record.

## Recovery and outside changes (milestone M2)

- **Autosave** (`src/features/recovery/autosave.ts`): while the open project has unsaved
  changes, its canonical text (made by the core's `canonical()` when a snapshot is due) goes to
  the backend as a recovery snapshot (`recovery_save`) every 30 s and when the window loses focus;
  never twice the same text, one call at a time, failures logged by code only. The backend keeps
  snapshots in its recovery folder, never next to the project, and deletes them on a clean save
  or close.
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

## The desktop crate in the Cargo workspace

The desktop crate (`blocks2cpp-desktop`, `src-tauri/`) is a member of the Cargo workspace but not
a default member, so `cargo build`, `cargo test` and `cargo clippy` at the root build only the
compiler crates and the CLI and need no webview libraries. Name the crate
(`-p blocks2cpp-desktop`) to build the app. Its feature `e2e-hooks` adds the end-to-end seams
([testing](testing.md#the-e2e-hooks-feature)).

## Blockly seams (ADR-0014)

[ADR-0014](../adr/0014-extending-blockly.md) lets the editor use the public and protected members
of documented Blockly and plugin classes, components replaced through `Blockly.registry`, and
`BlockSvg.setDragStrategy`; private members are not used, and nothing is patched. Each subclass of
a Blockly or plugin class is listed here with the upstream version it was checked against: the
TypeScript compiler checks the overrides on every upgrade, and the list says what else to look at
(a public setting written for a while, a protected helper, a workaround of an upstream defect).
Add or change an entry in the pull request that adds or changes the subclass.

Checked against Blockly 12.5.1 and `@blockly/continuous-toolbox` 7.0.9. The upstream reports named
below are drafts in [`docs/plans/upstream/`](../plans/upstream/README.md), not filed yet (polish
plan F-8); once upstream fixes a defect, the workaround goes.

**Dragging and placing blocks** (03 §3.3 "Placing blocks"):

- **`B2cBlockDragStrategy`** (`src/editor/sync/drag.ts`) extends
  `Blockly.dragging.BlockDragStrategy`.
  - Uses: overrides `drag` (with the pointer event that `BlockSvg.drag` passes) and the protected
    `getSearchRadius`; writes the public `Blockly.config.currentConnectionPreference` for the
    length of each move, because Blockly reads it in a private method, and puts it back in a
    `finally`; installed on each dragged block with `BlockSvg.setDragStrategy`.
  - Why: no place is previewed while the pointer is over a delete area, so a drop on the toolbox
    or the trash can deletes only the dragged blocks (W1-0a; report:
    [dropping on a delete area](../plans/upstream/blockly-drop-on-delete-area-deletes-stack.md));
    a snap radius that follows the zoom, and the editor's preview preference (W5-1).
- **`B2cBlockDragger`** (`src/editor/sync/drag.ts`) extends `Blockly.dragging.Dragger`, injected
  as the workspace's `blockDragger`.
  - Uses: overrides `onDragStart` and `onDragEnd`; calls `onDrag` and the protected
    `wouldDeleteDraggable` and `shouldReturnToStart`.
  - Why: the editing session hears of a drag before Blockly moves anything; a deleting drop gets a
    last move first (Blockly's gesture can end without one); the canvas records its drag targets
    again at a drag start when the record is stale (report:
    [records while hidden](../plans/upstream/blockly-flyout-setvisible-records-hidden.md)); a wrap
    is finished in the drop's event group (W5-2).
- **`B2cConnectionChecker`** (`packages/blockly-ext/src/checker/checker.ts`) extends
  `Blockly.ConnectionChecker`, registered as the `CONNECTION_CHECKER` `b2c_checker`.
  - Uses: overrides `doDragChecks` and `canConnectWithReason`; reads the public
    `Blockly.common.draggingConnections` (`checker/wrap.ts`).
  - Why: the structural rules and, while dragging, the type rule of 03 §3.5.3; the wrap pair of an
    empty C-block (W5-2).
- **`B2cConnectionPreviewer`** (`packages/blockly-ext/src/checker/previewer.ts`) extends
  `Blockly.InsertionMarkerPreviewer`, registered as the `CONNECTION_PREVIEWER` `b2c_previewer`.
  - Uses: overrides `previewReplacement`, `previewConnection` and `hidePreview`; makes its own
    marker with the protected `serializeBlockToInsertionMarker` (the base class makes its markers
    in private methods); calls the public `renderManagement.triggerQueuedRenders`.
  - Why: the wrap preview (W5-2): the statements go into the marker's mouth, and back exactly
    where they were when the preview is hidden, never disposed with the marker. The marker is
    drawn before it is disposed, which works around Blockly's render queue keeping disposed
    blocks for good (report:
    [render queue](../plans/upstream/blockly-render-queue-retains-disposed-blocks.md)); Blockly's
    own markers and deleted blocks still leak until Blockly fixes it.

**The toolbox** (04 §4.2), registered under names of ours and chosen per workspace through the
injection options (`src/editor/toolbox/register.ts`); the plugin's own registration is not used:

- **`B2cContinuousToolbox`** (`src/editor/toolbox/continuous.ts`) extends `ContinuousToolbox`
  (`@blockly/continuous-toolbox`), registered as the `TOOLBOX` `b2c_continuous_toolbox`.
  - Uses: overrides `refreshSelection`, `selectCategoryByName`, `render` and `dispose`.
  - Why: the dynamic categories are built only when what they depend on changed, and a refresh
    waits while a block is dragged, a field editor or a menu is open, or the flyout scrolls to a
    category.
- **`B2cContinuousFlyout`** (`src/editor/toolbox/continuous.ts`) extends `ContinuousFlyout`
  (`@blockly/continuous-toolbox`), registered as the `FLYOUTS_VERTICAL_TOOLBOX`
  `b2c_continuous_flyout`; the plugin's `ContinuousMetrics` is registered as is, as the
  `METRICS_MANAGER` `b2c_continuous_metrics`.
  - Uses: overrides `getFlyoutScale`, `position`, `dispose`, `scrollTo`, `createBlock` and the
    plugin's protected `getRecyclableInflater`; re-implements Blockly's protected
    `VerticalFlyout.reflowInternal_` instead of wrapping it; sets the plugin's protected
    `scrollAnimationFraction`.
  - Why: a fixed block scale and a width the user chooses (W3-1); no recycled blocks; the scroll
    to a chosen category goes half a unit past its heading, so the category stays selected (W1-7;
    report: [category selection](../plans/upstream/continuous-toolbox-category-selection.md));
    the canvas's record of its drag targets is marked stale after each layout (report:
    [records while hidden](../plans/upstream/blockly-flyout-setvisible-records-hidden.md)); a
    block taken from it is settled where it is dropped.
- **`B2cToolboxCategory`** (`src/editor/toolbox/category.ts`) extends `Blockly.ToolboxCategory`,
  registered as the `TOOLBOX_ITEM` `b2c_category`.
  - Uses: overrides the protected `createContainer_`, `createIconDom_` and `addColourBorder_`.
  - Why: the category's bubble with its icon, and its name below it.
- **`B2cBlockInflater`** (`src/editor/toolbox/inflater.ts`) extends `Blockly.BlockFlyoutInflater`,
  registered as the `FLYOUT_INFLATER` `b2c_block`.
  - Uses: overrides `load` and `getType`.
  - Why: the toolbox's own block items (fresh symbol IDs, labels of call arguments).
- **`StartValueChange`** (`src/editor/toolbox/reshape.ts`) extends `Blockly.Events.BlockBase`.
  - Uses: overrides `isNull` and `run`.
  - Why: Blockly records no event when a shadow is replaced, so without it Undo would put back a
    variable's type but keep the new start value.

**Blocks** (`packages/blockly-ext`):

- **The custom fields** (`src/fields/`) extend `Blockly.FieldTextInput`, `FieldDropdown` and
  `FieldCheckbox`, registered with `Blockly.fieldRegistry`.
  - Uses: the static `fromJson`; the protected `configure_`, `doClassValidation_`,
    `doValueUpdate_`, `getText_`, `trimOptions`, `showEditor_` and `widgetCreate_`; `saveState`
    and `loadState`.
  - Why: the seven fields of the block language, each reviewed with the
    [custom field review checklist](../security/custom-field-review-checklist.md).
- **`DiagnosticIcon`** (`src/icons/diagnosticIcon.ts`) extends `Blockly.icons.Icon`, registered
  with `Blockly.icons.registry`.
  - Uses: overrides `getType`, `getWeight`, `getSize`, `initView`, `isShownWhenCollapsed`,
    `updateCollapsed`, `setOffsetInBlock`, `onLocationChange`, `onNodeFocus` and `onClick`.
  - Why: the diagnostic badge on blocks.
- **`MutatorButton`** and **`MirrorLabel`** (`src/mutators/`) extend `Blockly.FieldImage` and
  `Blockly.FieldLabel`; the mutators are registered with `Blockly.Extensions.registerMutator`.
  - Uses: override `initView` and the protected `getText_`.
  - Why: the inline ⊕ and ⊖ buttons, and labels that repeat a field's text.
