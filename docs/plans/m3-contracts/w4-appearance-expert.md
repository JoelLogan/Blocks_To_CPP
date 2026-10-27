# M3 contracts: W4, Appearance and expert mode

The first pieces of wave W4 ([next steps §3.4](../next-steps.md#34-waves))
that need no new blocks: the Appearance setting (F-1 of the
[M2 polish plan](../m2-polish-plan.md#future-list), with the console's
colours, U14d), the expert-mode switch (Friendly and C++ labels, *Show
advanced blocks*; Q8 and M3-D4) and the service that says what a mode, a
C++ standard and the selected toolchain allow (feature gating, 06 §6.10).
It is written as W2 builds the language blocks on `ccr-01f82d9b-v70u20`;
the places that apply the switch inside W2's files are a proposal (§6), to
be made after W2's integration step.

## 1. Scope

### 1.1 In this slice

1. **Appearance**: a machine setting, *Follow the system* (the default),
   *Light* or *Dark*, on the Settings page. It applies at once to the whole
   window: the app's colour tokens, the controls the webview draws itself
   (`color-scheme`), the Blockly theme (blocks, toolbox, flyout) and the
   console (its xterm theme, U14d). *Follow the system* follows a change of
   the system's scheme while the app runs, as M2 does.
2. **The `[data-theme]` token refactor** F-1 asks for: every
   `@media (prefers-color-scheme: dark)` block of the app's style sheets
   becomes a `:root[data-theme='dark']` rule, so one attribute on `<html>`
   decides the theme; `cssAudit` reads the new rules, and a test keeps
   `prefers-color-scheme` out of the style sheets.
3. **Expert mode**: two machine settings, *Block labels* (Friendly or C++)
   and *Show advanced blocks*, with one *Expert mode* switch on the Settings
   page that turns both on or off together (Q8: "one switch to expert
   mode"); each can also be set on its own.
4. **Gating service**: a pure module that answers, for the editor's mode, the
   project's C++ standard and the selected toolchain's probed capabilities,
   whether a block, a dropdown option, a type or a category may be offered,
   and why not; and which label template a block shows. With its own tests.
5. **Tests**: Rust (store, IPC, app), the Settings page and the appearance
   module (Vitest, axe), the contrast of every token pair in both themes
   (the existing `styles.test.ts` and `layout.test.ts` checks, now reading
   `[data-theme]`), and an E2E flow that switches the appearance and checks
   the page, the canvas and the console.

### 1.2 Not in this slice

* Applying the switch inside W2's files (block labels in
  `packages/blockly-ext/src/blocks`, the toolbox's contents in
  `apps/desktop/src/editor/toolbox/`): §6, *Needs from W2*.
* `E0703` from the analyser and codegen's fallbacks (06 §6.10): they belong
  with the blocks that need them (*format … with*, W4 later; W2's C++20 and
  C++17 forms already choose by standard). The gating service gives the
  editor the same answer the analyser will, from the same table (§3.4).
* A *View* menu with the two toggles (03 §3.5.2 names *View → Block
  labels*): the app has one main menu and no *View* menu in M2. Decision
  W4A-5 below.
* The native title bar's theme (Tauri's `Window::setTheme`): decision
  W4A-4.
* High Contrast themes and UI scaling (M5, F-2).

## 2. Design

### 2.1 Where the settings live

Three new keys in `settings.json`, in two new sections, written like the
other sections (always, with their defaults), validated value by value:

```json
{
  "appearance": { "theme": "system" },
  "editor": { "blockLabels": "friendly", "showAdvanced": false }
}
```

| Key | Values | Default |
| --- | --- | --- |
| `appearance.theme` | `system`, `light` or `dark` | `system` |
| `editor.blockLabels` | `friendly` or `cpp` | `friendly` |
| `editor.showAdvanced` | `true` or `false` | `false` |

`settings_update` accepts `appearance` and `editor`, each partial, with
unknown keys refused as for the other sections. The format version stays 1:
the keys are additive, an older version keeps them as unknown sections and
writes them back unchanged (05 §5.9).

*Alternatives considered.* (a) Under `ui` (`ui.appearance`,
`ui.blockLabels`): `ui` is the window's own state, absent until first set,
and its keys are written only once set; preferences the person chooses on
the Settings page sit better with the other sections, which always show
their value in the file. (b) One `editor.expertMode` boolean: Q8 asks for
one switch, but 03 §3.5.2 and 04 §4.2 name two toggles, and a teacher may
want C++ labels without the advanced blocks; the switch sets both keys in
one patch, so one boolean would only lose that choice.

### 2.2 How the theme is applied

* `apps/desktop/src/app/appearance/` holds the logic: `resolveTheme(choice,
  systemDark)` gives `light` or `dark`; `applyTheme(root, theme)` sets
  `data-theme` on `<html>`; `startAppearance(store, media)` follows the
  setting in the app's store and the system's scheme (`matchMedia`), and
  keeps the resolved theme in a small store that the editor and the console
  subscribe to (`appearance.theme()`, `appearance.subscribe()`, and the hook
  `useResolvedTheme()`).
* `main.tsx` applies the system's theme before the first render, so a
  person on *Follow the system* never sees a flash; a person who chose the
  other theme sees the system's until `settings_get` answers (the first
  read at start-up, before a project opens). *Alternative:* keep the last
  resolved theme in `localStorage` to avoid that flash: one more store of a
  machine setting outside `settings.json`, against 05 §5.9's rule that only
  the backend keeps them, so not taken.
* CSS: `:root` holds the light tokens and `color-scheme: light`;
  `:root[data-theme='dark']` the dark ones and `color-scheme: dark`. Until the
  app has set the attribute, `@media (prefers-color-scheme: dark)
  :root:not([data-theme])` repeats the dark tokens (a test keeps the two
  sets equal), so a dark system shows no light frame before the bundle runs. The
  main menu's list, which sets its own `color-scheme` to escape the
  toolbar's dark one, uses the token `--b2c-color-scheme`.
* The Blockly theme: `EditorWorkspace` calls `workspace.setTheme()` with
  `b2cTheme(theme)` when the resolved theme changes, as it does today on a
  system change (the toolbox's category colours follow, `toolbox/category.ts`).
* The console (U14d): `panels.css` gains light console tokens (a light
  background, dark text, a visible cursor and selection), and the dark ones
  move under `[data-theme='dark']`. `ConsolePanel` reads them as now and
  sets `term.options.theme` again when the resolved theme changes.
  *Alternative:* keep the console dark in both themes, as M2 does: F-1 and
  U14d ask for it to follow the setting, so it follows (decision W4A-3).
* Blockly's injected CSS in `packages/blockly-ext/src/icons/css.ts` (the
  diagnostic badges' colours) moves to `:root[data-theme='dark']
  .blocklySvg`.

*Alternative considered for the whole mechanism:* only call Tauri's
`Window::setTheme` and keep the media queries, since the webviews report the
window's theme as `prefers-color-scheme`. It changes the title bar too, but
it needs a new window capability (`core:window:allow-set-theme`), WebKitGTK's
answer depends on the GTK theme in use, and happy-dom and the E2E harness
could not check it; the attribute is deterministic and testable everywhere.

### 2.3 Expert mode in the app

* The app's store already holds the settings (`state.settings.value`); the
  editor's mode is a selector, `editorMode(state)`, in
  `apps/desktop/src/editor/gating/`: `{ labels, showAdvanced, standard,
  toolchain }`, where `showAdvanced` is the machine setting **or** the open
  project's `project.options.showAdvanced` (decision W4A-2), `standard` is the
  project's `language.standard` (the machine's `newProject.standard` without
  a project), and `toolchain` the probed capabilities of the toolchain a build
  would use (`selectedToolchain`, or `null` before discovery).
* `modeKey(mode)` is a short string that changes exactly when what the
  toolbox or the labels show can change, for the toolbox plugin's rebuild
  check (§6.2).
* The Settings page gets an *Editor* section: the *Expert mode* switch (a
  checkbox, mixed when the two settings differ), then *Block labels*
  (Friendly, C++) and *Show advanced blocks*. It says that both modes give
  the same C++ and that blocks already in a project stay.

### 2.4 The gating service

`apps/desktop/src/editor/gating/gate.ts` (pure, no Blockly):

```ts
export type LabelMode = 'friendly' | 'cpp';
export interface EditorMode {
  readonly labels: LabelMode;
  readonly showAdvanced: boolean;
  readonly standard: CppStandard;
  readonly toolchain: ToolchainCapabilities | null;
}
/** What a block, option, type or category needs. */
export interface Requirement {
  readonly advanced?: true;
  readonly standard?: CppStandard;            // the oldest standard that has it
  readonly capability?: 'stdFormat';          // a probed toolchain capability
  readonly fallback?: true;                   // codegen has an older form
}
export type Gate =
  | { readonly allowed: true }
  | { readonly allowed: false; readonly reason: 'advanced' }
  | { readonly allowed: false; readonly reason: 'standard'; readonly needs: CppStandard }
  | { readonly allowed: false; readonly reason: 'toolchain'; readonly needs: 'stdFormat' };

export function gate(requirement: Requirement | undefined, mode: EditorMode): Gate;
export function blockGate(blockType: string, mode: EditorMode): Gate;
export function optionGate(blockType: string, field: string, value: string, mode: EditorMode): Gate;
export function categoryGate(category: string, mode: EditorMode): Gate;
export function typeGate(typeKey: string, mode: EditorMode): Gate;
export function gateText(gate: Gate): string | null;   // "Needs C++20 or newer.", …
```

* A requirement with `fallback` is always allowed (codegen writes the older
  form); without one, a standard older than `standard`, or a toolchain that
  lacks the capability, refuses it. An unknown toolchain (`null`, discovery
  not finished) refuses nothing: the build reports it (`E0703`) rather than
  the toolbox hiding blocks that may well work.
* The requirements live in one table, `REQUIREMENTS` (`gating/table.ts`),
  keyed `block:<id>`, `option:<id>.<FIELD>=<value>`, `category:<id>` and
  `type:<key>`. In this slice it holds the rows the plans name: `var.change`
  `OP` = `and`, `or`, `xor`, `shl`, `shr` (advanced, w2.md §1.9);
  *format … with* (`text.format`, C++20 and `stdFormat`, W4 later); the
  advanced categories of 03 §3.7 (*Memory*, *Concurrency*, *Generics*, *Raw
  C++*), which have no blocks yet. A test checks that every `block:` and
  `option:` key names a block, field and option of the catalog, or is listed
  as waiting for its block, so a renamed block cannot leave a dead row.
  *Alternative:* the requirement as catalog metadata (`advanced = true`,
  `requires = { standard = "c++20", capability = "stdFormat" }` in the block's
  TOML), read by `b2c-lang` for `E0703` too. That is the right end state, but
  `catalog/` and `b2c-catalog` are W2's; §6.4 proposes it, and the table then
  becomes generated.
* Labels (`gating/labels.ts`): `blockLabelParts(def, mode)` gives the parts a
  block is laid out from (the C++ template's only when it names every field
  and value input, §6.1); `labelTemplate(def, mode)` gives `def.label.friendly` or
  `def.label.cpp`; `labelParts(template)` splits a template into text and
  `%NAME` parts as the catalog generator does for the Friendly label, but
  keeping punctuation next to a part (`%LIST.push_back(%VALUE);` gives
  `LIST`, `.push_back(`, `VALUE`, `);`). `optionLabel(blockType, field,
  value, friendly, mode)` gives an option's C++ text where the table
  `CPP_OPTION_LABELS` has one (the operator menus: `+=`, `-=`, `<`, `&&`, …),
  otherwise the Friendly text.

## 3. Files

### 3.1 Owned by this slice (new)

* `apps/desktop/src/app/appearance/` (`appearance.ts`, `index.ts`, tests).
* `apps/desktop/src/editor/gating/` (`gate.ts`, `table.ts`, `labels.ts`,
  `mode.ts`, `index.ts`, `gating.test.ts`).
* `apps/desktop/e2e/specs/flows/appearance.e2e.ts`.
* This contract.

### 3.2 Changed (outside W2's paths)

* Rust: `crates/b2c-store/src/settings.rs`, `crates/b2c-ipc/src/dto/settings.rs`
  (and its tests and snapshots), `crates/b2c-app/src/settings.rs` (and its
  tests); `packages/ipc-types/src/generated/` regenerated.
* CSS: `apps/desktop/src/app/app.css`, `apps/desktop/src/panels/panels.css`,
  `apps/desktop/src/editor/sync/drag.css`, `apps/desktop/src/app/layout/MainMenu.css`,
  `packages/blockly-ext/src/icons/css.ts`.
* TS: `apps/desktop/src/main.tsx`, `apps/desktop/src/app/layout/cssAudit.ts`
  and the tests that read it, `apps/desktop/src/editor/EditorWorkspace.tsx`,
  `apps/desktop/src/panels/console/ConsolePanel.tsx`, the settings feature
  (`SettingsPage.tsx` gains the *Appearance* and *Blocks* sections beside
  its others, `texts.ts`, tests), `apps/desktop/src/app/panels.tsx` (the
  console's `theme` prop), the test fixtures of the settings
  (`apps/desktop/src/app/testing/fixtures.ts`).
* The visual harness's checks that read `prefers-color-scheme` read the
  page's `data-theme` too (`e2e/visual/settle.ts`), so a scene still checks
  the theme it is taken in.

## 4. IPC and project format

* **IPC** (`b2c-ipc`, mirrored in `packages/ipc-types`): `Settings` gains
  `appearance: AppearanceSettings { theme: Theme }` and `editor:
  EditorSettings { blockLabels: BlockLabels, showAdvanced: boolean }`;
  `SettingsPatch` gains `appearance?: { theme?: Theme }` and `editor?: {
  blockLabels?: BlockLabels, showAdvanced?: boolean }`, with their schemas
  (unknown keys and other values refused, `invalidRequest`). `Theme` is
  `"system" | "light" | "dark"`, `BlockLabels` `"friendly" | "cpp"`. No new
  command; the isolation hook's generated allowlist and its samples gain the
  two patch objects (`apps/desktop/src-tauri/isolation/allowlist.generated.js`,
  `isolation-tests/samples.generated.json`), so the hook refuses any other
  value before the backend sees it.
* **Project format:** none. The project's `options.showAdvanced` is read, not
  written.

## 5. Security, accessibility, platforms

* **Security.** Two enums and a boolean, validated twice (the IPC schema and
  the store), never paths or text. Nothing reaches a project file. The theme
  is an attribute value from a closed set; no style is built from data.
* **Accessibility.** The Settings controls are native radio buttons and
  checkboxes in fieldsets with legends, as the page's others; the mixed
  state of *Expert mode* is the native `indeterminate` (announced as mixed).
  Every token pair the contrast tests check is checked in both themes, now
  selected by `[data-theme]`, and the new light console tokens are checked
  (4.5:1 text, 3:1 cursor). The appearance does not change focus or move
  anything; reduced motion is unaffected.
* **Windows.** WebView2 follows `color-scheme` and the attribute like
  WebKitGTK; with *Light* or *Dark*, Windows' own setting no longer decides
  the page (the visual diff's dark scenes on Windows become possible, §7).
  The title bar keeps the system's theme (W4A-4).

## 6. Needs from W2

To be applied after W2's integration step, by the coordinator or by this
slice on a later branch. Each item names the function, its call sites and
the tests.

### 6.1 Labels in the C++ mode (`packages/blockly-ext/src/blocks`)

* `blocks/layout.ts`: `layoutFor(def)` takes the label parts as a parameter,
  `layoutFor(def, parts = def.labelParts)`, so it can lay a block out from the
  C++ template's parts; nothing else changes in it.
* `blocks/register.ts`: the block's `init` reads the label mode from a
  service, `services.ts` gaining `labelMode(): LabelMode` (default
  `'friendly'`), and calls `layoutFor(def, blockLabelParts(def, mode))`;
  `blockLabelParts`, `labelParts` (with its `cpp` option, which reads a
  C++ template's `{ … }` as text, not as a repeated group) and `hasFullCppLabel` move from
  `apps/desktop/src/editor/gating/labels.ts` into `blocks/` (or a shared
  `text/` module) so the package does not import the app. Dropdown option
  texts go through `optionLabel` the same way (`CPP_OPTION_LABELS` covers
  every option of the operator and function menus; a test keeps it so).
* **The catalog's C++ labels.** A C++ template is laid out only when it names
  every field and value input (`hasFullCppLabel`); otherwise the block keeps
  its Friendly layout, with the options' C++ texts. At `df4b4f5` these
  templates leave out a field or a value input, so they stand for one form
  of the block only (statement inputs left out are fine: `{ … }` stands for
  them): `coll.for_each` (`MODE`), `coll.sort` (`ORDER`),
  `control.for_range` (`DIRECTION`, `TO`, `STEP`), `control.while` (`MODE`),
  `io.ask` (`MODE`, `PROMPT`), `io.print` (`SEP`, `NEWLINE`, `STREAM`),
  `logic.operation` (`OP`), `math.fn`, `math.fn_two`, `math.minmax` (`FN`),
  `text.case` (`CASE`), `text.find` (`OP`), `text.to_number` (`TO`),
  `var.declare` (`CONST`). Proposal for W2: either rewrite each so it names
  its parts (`%FN(%VALUE)` with the C++ option texts `std::round`, …), or keep
  them as documentation; the fallback keeps both working.
* `apps/desktop/src/editor/` (the editor's attach code, not W2's): on a mode
  change, rebuild the visible module's blocks without an undo step and
  without marking the project changed: serialise the module
  (`Blockly.serialization.workspaces.save`), clear and load inside
  `withoutEvents`, as the module switch in `editor/sync/session.ts` does, then
  refresh the toolbox. A label change never changes the document.
* Tests: a block laid out from `%LIST.push_back(%VALUE);` has its inputs in
  the C++ order and serialises to the same state as the Friendly one; a
  mode switch keeps the selection, the view and the undo stack, and the
  document's hash; every catalog block's C++ template has parts whose names
  are the block's fields and inputs (a test over `BLOCK_DEFS`, like the
  Friendly one in `b2c-catalog`).

### 6.2 The toolbox (`apps/desktop/src/editor/toolbox/`)

* `contents.ts`: `entryItems(category, mode)` skips an entry whose
  `blockGate(entry.block, mode)` is not allowed; `isShownCategory(category,
  mode)` also requires `categoryGate(category.id, mode).allowed`;
  `toolboxDefinition(mode)` and `initialToolboxDefinition(mode)` pass it on.
  The *change* block's operator menu lists an option only when
  `optionGate('var.change', 'OP', value, mode)` allows it, unless the block
  already holds that value (a loaded project keeps what it has).
* `plugin.ts`: the contents context gains `mode: () => EditorMode` (from
  `editorMode(store.getState())`); the rebuild check compares
  `modeKey(mode)` with the last one, so a mode, standard or toolchain change
  rebuilds the toolbox once, outside a drag, as the dynamic categories do.
* `toolbox.css`: nothing.
* Tests: in `contents.test.ts`, an advanced-only entry is absent with
  `showAdvanced` off and present with it on; a category whose gate refuses
  it is not shown; in `plugin.test.ts`, switching the mode rebuilds the
  toolbox once and keeps the selected category.

### 6.3 Field menus (`packages/blockly-ext/src/fields`)

* The dropdowns of catalog blocks filter their options with a service,
  `services.ts` gaining `optionAllowed(blockType, field, value): boolean`
  (default: all), keeping the current value. Test: `var.change`'s menu hides
  the bitwise options until *Show advanced blocks* is on.

### 6.4 The catalog (`catalog/`, `crates/b2c-catalog`, `packages/catalog-gen`)

* Optional, when W2 extends the schema next: `advanced = true` on a block or
  a dropdown option, and `requires = { standard = "c++20", capability =
  "stdFormat", fallback = true }`, written into `catalog.json` and the
  generated `catalog.ts`; `gating/table.ts` then reads them and keeps only
  the categories and types. The analyser's `E0703` reads the same keys
  (06 §6.10).

## 7. Tests

* **Rust.** `b2c-store`: the new keys are read, validated one by one (an
  invalid `appearance.theme` is reset with a notice naming it; the other
  values stay), updated, round-tripped byte for byte, and the default file
  is canonical. `b2c-ipc`: the patch schema accepts each new key, refuses
  unknown keys and other values; the snapshot of `SettingsPatch::sample()`
  and of `Settings`. `b2c-app`: `settings_update` with each new key returns it
  and `settings_get` after a restart reads it.
* **Vitest.** `appearance.test.ts`: resolution (system, light, dark with the
  system light and dark), the attribute and `color-scheme` on `<html>`, a
  system change followed only with *Follow the system*, a settings change
  applied at once, unsubscribing. `gating/*.test.ts`: every reason of `gate`,
  fallbacks, an unknown toolchain, `modeKey`, the table's keys against the
  catalog, `labelParts` on every catalog block's C++ template, `optionLabel`.
  The Settings page: the Appearance radio group and the Editor section save
  their patches, the switch's mixed state, a refused update brings the saved
  value back, axe on the page. `styles.test.ts`/`layout.test.ts`: the token
  pairs in both themes from `[data-theme]` rules, the new console pairs, and
  no `prefers-color-scheme` in the style sheets. `EditorWorkspace.test.tsx`
  and `ConsolePanel.test.tsx`: the Blockly theme and the terminal's theme
  follow the resolved theme.
* **E2E.** `specs/flows/appearance.e2e.ts`: open Settings, choose *Dark*: the
  page's `data-theme` is `dark`, the canvas's background is the dark canvas
  colour and the console's background the dark console colour; *Light*: the
  light ones; restart: the choice is kept; *Follow the system*: the page
  follows the system's scheme again. The expert-mode switch saves both
  settings (the visible effect waits for §6).
* **Visual diff.** The scenes this slice changes, for the coordinator to
  rebaseline: `chrome-console-scrolled` and `chrome-console-two-runs`
  (light, Linux and Windows: the console becomes light, U14d); possibly
  `chrome-window-smallest` (light) if its bottom dock shows the console. The
  dark scenes are unchanged. Taking dark scenes on Windows by setting
  `appearance.theme` in the profile is a follow-up that needs new baselines.

## 8. Spec sections to change later (docs/spec/ is W2's)

* [03 §3.5.2](../../spec/03-block-language.md#352-friendly-and-c-display-modes):
  the toggle is on the Settings page (*Editor*), and the *Expert mode*
  switch; *In M2* note.
* [03 §3.7](../../spec/03-block-language.md#37-block-catalog-by-category):
  *View → Show advanced blocks* becomes the Settings page's.
* [04 §4.2](../../spec/04-user-interface.md#42-toolbox) *Beginner /
  Advanced*: shown when the machine setting or the project asks for it
  (W4A-2); gated blocks hidden by standard and toolchain.
* [04 §4.8](../../spec/04-user-interface.md#48-accessibility): the
  Appearance setting; the console follows the theme.
* [04 §4.12](../../spec/04-user-interface.md#412-settings-page): the
  Appearance and Editor sections.
* [05 §5.9](../../spec/05-project-format.md#59-machine-local-data): the
  example and the key table gain `appearance.theme`, `editor.blockLabels`,
  `editor.showAdvanced`; `settings_update` accepts `appearance` and `editor`.
* [06 §6.10](../../spec/06-compiler-pipeline.md#610-includes-and-support-helpers):
  the editor hides what the gate refuses, from the same requirements.
* [09 §9.2](../../spec/09-quality-and-delivery.md#92-testing-strategy): the
  visual diff's Windows dark scenes with the Appearance setting.
* [10 §10.1](../../spec/10-roadmap.md#101-milestones) M3: what landed.

## 9. Decisions for the owner

| ID | Question | Recommendation |
| --- | --- | --- |
| W4A-1 | Where the keys live | New sections `appearance` and `editor`, always written (§2.1) |
| W4A-2 | *Show advanced blocks*: machine or project? | Shown when the machine setting **or** the project's `options.showAdvanced` is on: M3-D4 makes it a machine setting, and a project that asks for advanced blocks (a teacher's) still shows them; the project option is edited in W3's project settings dialog |
| W4A-3 | Does the console follow the theme? | Yes (F-1, U14d): light console in the light theme |
| W4A-4 | The title bar's theme | Leave it with the system for now; a follow-up sets it with Tauri's `setTheme` and its capability, checked on both systems |
| W4A-5 | A *View* menu | Not in M3: the toggles live on the Settings page; a *View* section of the main menu comes with M5's command palette |
| W4A-6 | Blocks already in a project that the mode or the standard would hide | Kept, shown and generated as they are; only the toolbox and the menus hide them, and the build reports what the standard or toolchain cannot compile (`E0703`) |
| W4A-7 | A label-mode change | Rebuilds the visible blocks without an undo step and without changing the project (§6.1) |

## 10. Status

On `m3/w4-appearance-expert`, from `df4b4f5`, on 2026-10-27.

**Done.**

* The contract (`2ec2c27`; later commits correct it as the code was
  written).
* The settings (`3ada984`): `appearance.theme`, `editor.blockLabels` and
  `editor.showAdvanced` in `b2c-store`, `b2c-ipc` and `b2c-app`, with tests;
  the TypeScript types, the isolation allowlist and its samples regenerated.
* Appearance (`fcf6d96`): the `[data-theme]` refactor of every style sheet
  and of the badges' CSS, `cssAudit` reading `:root[data-theme='dark']`, a test
  that no style sheet uses `prefers-color-scheme`, `src/app/appearance/`, the
  Blockly theme and the console following the theme, light console tokens
  (U14d), and the Settings page's *Appearance* and *Blocks* sections (the
  expert-mode switch and its two settings), with tests and axe.
* Gating (`583aeee`): `src/editor/gating/`, with its tests (§2.4, §6.1's
  C++-label finding).
* E2E (`a346951`): `specs/flows/appearance.e2e.ts`. It found a real bug,
  fixed in the same commit: WebKitGTK gives stale computed values inside the
  editor while a page hides it, so the console now reads its tokens again when
  it is shown.

**Not done, and why.**

* Everything in §6 (labels, toolbox, menus, catalog): W2's files, to apply
  after W2's integration. Until then the *Blocks* settings are saved and read
  but change nothing in the editor.
* W4A-4 (title bar), W4A-5 (*View* menu): decisions for the owner.
* The spec sections of §8: `docs/spec/` is W2's.

**Checks.** Locally: `cargo fmt`, workspace clippy (Linux, and the Windows
target with CI's clang settings), `cargo doc`, the whole workspace's
`cargo test` with `B2C_REQUIRE_GXX=1`, the four `tools/check-*.py`,
markdownlint; `pnpm -r` typecheck, lint and format; desktop Vitest (2151
tests) and `test:isolation`; blockly-ext's tests; the E2E flows
`appearance`, `settings`, `console` and `chrome` under xvfb. The WASM core
was built without wasm-opt.

**CI on `a346951`.** CI: green. Desktop: green except the visual-diff step on
both systems. The E2E flows pass on Linux and Windows, `appearance` included.

**Visual diff, for the coordinator to rebaseline.** Changed by this branch
(the console is light in the light theme, U14d): `chrome-console-scrolled`,
`chrome-console-two-runs`, `chrome-window-smallest` and `chrome-menu-open`
(72,000 px each, the console in the bottom dock), all light, on Linux and
expectedly on Windows. Not this branch's: the toolbox scenes
(`chrome-toolbox-math`, `-text`, `-io`, `-functions`, and in the dark theme
also `-logic` and `-loops`), which W2's blocks changed; the base branch's
Desktop runs fail the same step. No baseline was updated.

**Merge notes.** No file of W2's list is touched. Files outside this
contract's own new ones: §3.2's list, plus
`apps/desktop/e2e/specs/flows/README.md`, `e2e/specs/flows/chrome.e2e.ts`,
`e2e/visual/canvas.visual.e2e.ts` and `e2e/visual/settle.ts` (they read the
page's `data-theme`). Expected conflicts: `packages/ipc-types/src/generated/`
and the isolation allowlist if W2 or W3 changed the IPC (regenerate with
`B2C_UPDATE_IPC=1 cargo test -p b2c-ipc --features ts --test generate`);
`settings.test.tsx` and `SettingsPage.tsx` if W3's settings dialog edits
them; `app.css` and `panels.css` if anyone added dark tokens in an
`@media (prefers-color-scheme: dark)` block, which must move to
`:root[data-theme='dark']` (the new style test fails otherwise).

### 10.1 The coordinator's review (seven findings)

Each fix has a test that fails without it.

1. **[major] Console selection colour** (`c492ecf`). Fixed: the tokens are
   `rgba(26, 86, 219, 0.22)` and `rgba(138, 180, 255, 0.35)`, which xterm.js
   parses. Tests: `ConsolePanel.test.tsx` reads the colour xterm.js resolved
   (it was `rgba(255, 255, 255, 0.3)` with the old form, checked), and
   `styles.test.ts` holds every `--b2c-console-*` token in both themes to
   xterm.js's formats.
2. **[major] ANSI colours on the light console** (`c492ecf`). Fixed with
   `minimumContrastRatio: 4.5` on the terminal, which covers the 16 colours,
   256-colour, true colour and dim text in both themes. Test: the option.
3. **[minor] `{ … }` read as a repeat part** (`91f185f`). Fixed:
   `labelParts(template, { cpp: true })` reads a `…` that is the whole of a
   `{ … }` as text, and `blockLabelParts` uses it; §6.1 says so. Tests:
   `control.forever` and `control.repeat` exactly, a repeated group outside
   braces kept, and over `BLOCK_DEFS`, no repeat part in a full C++ layout of
   a block without a repeated part.
4. **[minor] First-paint flash on a dark system** (`7ffd6d9`). Fixed with the
   `:root:not([data-theme])` fallback in `app.css`, `panels.css` and
   `drag.css` (§2.2). Tests: the media query appears only around that
   selector, and each sheet's fallback equals its dark tokens.
5. **[minor] AWAITED rows never re-checked** (`91f185f`). Fixed: each AWAITED
   row's target must not exist yet, so the test fails when it lands; and every
   option of `var.change`'s `OP` beyond `add`, `sub`, `mul`, `div`, `mod` and
   `append` must have a row.
6. **[nit] blockly-ext's CSS not scanned** (`7ffd6d9`). Fixed: `icons/css.ts`
   exports its sheet as `DIAGNOSTIC_CSS` (the only change there, besides
   `icons/index.ts`'s export); `styles.test.ts` checks it has no media query
   of the scheme and has the `[data-theme='dark']` rule. The badges need no
   fallback: Blockly is injected after the attribute is set.
7. **[nit] The advanced refusal text** (`91f185f`). Now *An advanced block:
   turn on “Show advanced blocks” in Settings to use it.*

Checks after the fixes: `pnpm -r` typecheck, lint and format; desktop Vitest
(2161 tests) and `test:isolation`; blockly-ext's tests (396); the E2E flows
`appearance`, `console`, `settings` and `chrome` under xvfb; markdownlint. No
Rust changed since the full Rust checks above. The merge notes gain
`packages/blockly-ext/src/icons/css.ts` and `icons/index.ts` (W2's package:
one exported constant). The visual-diff list is unchanged: the fallback
shows the same colours as the attribute.

**CI on `f642acf`** (the review fixes). CI: green. Desktop: every job green
except the visual-diff step on Linux and Windows (the E2E flows pass on both,
`appearance` included); the scenes are those listed above. OSV-Scanner fails
with no lockfile changed on this branch since `df4b4f5`: `Cargo.lock` holds
`block-buffer 0.10.4`, the advisory GHSA-qwgh-2vcv-g2f7 that the coordinating
branch excepts in `c22ae1c`, not this branch's change.
