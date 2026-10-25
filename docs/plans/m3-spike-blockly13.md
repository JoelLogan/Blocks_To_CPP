# M3 spike: Blockly 13, continuous-toolbox 13 and xterm.js 6.1

The spike of M3-3 that owner decision M3-D7 asks for before the upgrade is scheduled
([next steps §3.3](next-steps.md#33-decisions-before-m3-starts), wave W0, §3.6 step 4). It was
run on 2026-10-24 and 25 in a separate worktree at `22c2532` (main, M2 merged); nothing on the branch
changed. The app stays on Blockly 12.5.1, `@blockly/continuous-toolbox` 7.0.9 and xterm.js 5.5.0.
The worktree's changes are kept as a patch (see [The patch](#the-patch)).

- [Verdict](#verdict)
- [Versions and policy](#versions-and-policy)
- [What changed in Blockly 13 that touches the app](#what-changed-in-blockly-13-that-touches-the-app)
- [Changes made in the spike](#changes-made-in-the-spike)
- [Checks and unit tests](#checks-and-unit-tests)
- [End-to-end flows](#end-to-end-flows)
- [Visual diff](#visual-diff)
- [Benchmarks](#benchmarks)
- [Trusted Types and the CSP](#trusted-types-and-the-csp)
- [xterm.js 6.1](#xtermjs-61)
- [Remaining work](#remaining-work)
- [The patch](#the-patch)

## Verdict

**Not clean: the upgrade waits until after M3**, as M3-D7 says for this case. M3 keeps Blockly
12.5.1 and continuous-toolbox 7.0.9, and W4 (the editor wave) and W5 build on them.

Blockly 13 is not a small step from 12. It brings keyboard navigation and screen-reader support
into core: its own shortcuts on the arrow keys, Enter, Space, `M` and `T`, a stateless navigator
in place of the line cursor the editor's keyboard module is built on, its own ARIA roles and
live region, a focus target for the workspace and a focusable trash can. Its block drag now
also serves keyboard moves, copies a toolbox block inside the drag strategy instead of the
flyout, and draws its own insertion preview without a marker block. The app compiles and runs on
it after the changes below (the guessing game exit test passes with real drags from the
toolbox), but:

- 4 of 105 Linux end-to-end tests fail, among them a toolbox that no longer shows the chosen
  category as selected, a canvas Tab order with a new stop, and zoom levels Blockly now rounds;
- the visual diff cannot take its scenes (they wait for the selected category), and with that
  wait relaxed the canvas looks different: every expression slot has a yellow outline and the
  boolean slots lose their hexagonal ends;
- 79 of 2,021 desktop unit tests still fail, and axe finds two critical problems in the editor
  that come from Blockly 13's new ARIA roles;
- the gated drag benchmark at 5,000 blocks is 16.7 % slower in a local A/B (the selection and
  edit benchmarks are faster);
- the keyboard module's port to the new navigator is mechanical and not reviewed, and Blockly
  13's own keyboard navigation is switched off rather than adopted, which is a product decision
  ([Remaining work](#remaining-work)).

The verdict rests on "not clean", which is M3-D7's test, not on the size of what is left. What is
left is about 9 to 13 working days of work and review beyond the 2-day spike, and most of it is in
the editor, where W4 works.

## Versions and policy

| Package | Now | Tried | Released | Age on 2026-10-24 |
| --- | --- | --- | --- | --- |
| `blockly` | 12.5.1 (2026-03-20) | 13.4.0 | 2026-10-13 20:47 UTC | 11.1 days |
| `@blockly/continuous-toolbox` | 7.0.9 (2026-04-09) | 13.4.0 | 2026-10-13 20:44 UTC | 11.1 days |
| `@xterm/xterm` | 5.5.0 (2024-04-05) | 6.1.0-beta.315 (probe only) | 2026-10-13 17:51 UTC | 11.3 days |
| `@xterm/addon-fit` | 0.10.0 (2024-04-05) | 0.12.0-beta.311 (probe only) | 2026-10-13 17:52 UTC | 11.3 days |

Blockly 13.0.0 came out on 2026-06-15; the first 13.x continuous toolbox, 13.1.0, on 2026-06-30.
Continuous-toolbox 13.4.0 asks for `blockly` ^13.2.0. xterm.js has no stable 6.1 yet: npm's
`latest` is 6.0.0 (2025-12-22) and addon-fit's 0.11.0.

The two Blockly packages installed under the unchanged policy of `pnpm-workspace.yaml`
(`minimumReleaseAge` 10080, `minimumReleaseAgeStrict`, `trustPolicy: no-downgrade`,
`blockExoticSubdeps`, `allowBuilds: {}` with `strictDepBuilds`): no exception, override or
allowed build was added. Both have npm provenance, as before.

- Blockly 13 makes `jsdom` a peer dependency (`>=27.4.0 <31.0.0`, required) where 12.5.1
  depended on `jsdom` 26.1.0, as the [dependency notes](../developer-guide/dependencies.md#notes)
  expected. pnpm installs the peer, `jsdom` 30.1.3; the browser bundle still never includes it.
  The lockfile's packages go from 536 to 534: 30 added and 32 removed, the two Blockly packages
  and the rest jsdom's tree (`jsdom` 26.1.0 → 30.1.3 and its dependencies).
- Every added package was 7.6 days old or more (the youngest, `@asamuzakjp/css-color` 7.1.7 and
  `@asamuzakjp/dom-selector` 10.0.4, were published on 2026-10-17); 15 of the 30 carry npm
  provenance and 15 do not, and pnpm's `no-downgrade` check passed for all of them.
- `jsdom` 30.1.3 declares `node: ^22.22.2 || ^24.15.0 || >=26.0.0`; this machine has Node.js
  22.22.0 and `.node-version` says `22`. pnpm only warns, and the frontend tests do not load jsdom
  (they run in happy-dom), but CI should install 22.22.2 or later before the upgrade lands.
- The `blockly` and continuous-toolbox rows of the [dependency table](../developer-guide/dependencies.md#npm-packages)
  and the note on Blockly 12 change with the upgrade.

**Release notes.** The npm packages carry no changelog. The GitHub release pages and API of
`RaspberryPiFoundation/blockly` answered 403 through the session's proxy ("GitHub access to this
repository is not enabled for this session"); this was not retried or worked around. The changes
below were found by comparing the type declarations of 12.5.1 and 13.4.0 (182 files under
`core/` differ), by reading the 13.4.0 bundle where a behaviour mattered, and by the compiler and
the tests.

## What changed in Blockly 13 that touches the app

Paths with line numbers are at `22c2532`; `core/…` is the `blockly` 13.4.0 package's
declarations.

1. **Keyboard navigation in core.** `WorkspaceSvg.getCursor`, `LineCursor`, `Marker` and the
   marker manager are gone; `getNavigator()` (`core/workspace_svg.d.ts:1105`) returns a
   stateless `Navigator` (`getNextNode`, `getPreviousNode`, `getInNode`, `getOutNode`,
   `getFirstNode`, `getLastNode`), and focus is the only state. The editor's keyboard module
   uses the cursor at `apps/desktop/src/editor/keyboard/navigation.ts:160` and `:306`.
2. **Blockly's own shortcuts.** Loading Blockly 13 registers its keyboard navigation and
   screen-reader shortcuts (`registerKeyboardNavigationShortcuts`, `registerScreenReaderShortcuts`,
   `core/shortcut_items.d.ts:212`, `:216`): 34 shortcuts by default, where 12 had eight. They take
   the arrow keys, Enter, Space, `M` and `T`, so registering the editor's shortcuts
   (`keyboard/shortcuts.ts:115-123`, registered at `:154`) throws `Shortcut named "b2c_kb_down"
   collides with shortcuts "down,move_down"`: in the app the keyboard plugin would fail to
   attach. Blockly's set also has `D` (duplicate), `X` (disconnect), `C` (clean up) and `I`
   (information), which would act on the canvas past the editing session and the clipboard
   plugin's validated copy and paste.
3. **Dragging.** `Dragger` takes only the draggable and has no `workspace` any more; `onDragStart`
   returns the draggable it ends up dragging; `wouldDeleteDraggable` and `shouldReturnToStart`
   take a screen `Coordinate` instead of the pointer event; a given-up drag calls the new
   `onDragRevert` (`core/dragging/dragger.d.ts:16`, `:37`, `:41`, `:46`). `BlockDragStrategy`
   takes keyboard events too and ends with a `DragDisposition` (`core/dragging/block_drag_strategy.d.ts:220`).
   `B2cBlockDragger` used all of these (`apps/desktop/src/editor/sync/drag.ts:355`, `:372-383`,
   `:402-424`, `:440`).
4. **Copying a toolbox block.** `Flyout.createBlock`, `isBlockCreatable`, `serializeBlock`,
   `isDragTowardWorkspace` and `dragAngleRange_` are gone. The drag strategy makes the copy in
   the protected `getTargetBlock` (`core/dragging/block_drag_strategy.d.ts:85`) and starts the
   drag on the copy, with the copy's own strategy. The toolbox's landing note hooked
   `createBlock` (`apps/desktop/src/editor/toolbox/continuous.ts:683-687`), and the keyboard's
   add from the toolbox called it (`keyboard/navigation.ts:426`).
5. **Nullable connections.** `BlockSvg.previousConnection`, `nextConnection` and
   `outputConnection` are typed `RenderedConnection | null` (`core/block_svg.d.ts:105`), as they
   always were at run time: `packages/blockly-ext/src/checker/previewer.ts:156` and eleven places
   in tests and test helpers no longer compiled.
6. **Messages at inject.** `inject` builds the workspace's ARIA label from `Blockly.Msg` and
   fails without a locale. The app sets English before it injects
   (`apps/desktop/src/editor/services/install.ts:41`); `blockly-ext`'s tests and two desktop
   test helpers did not.
7. **Late CSS.** `Css.register` no longer throws after Blockly has styled the document; it keeps
   the rules but never adds them to a document already styled. `registerB2cCss` relied on the
   throw for its own `<style>` fallback (`packages/blockly-ext/src/blocks/css.ts:42-51`).
8. **Insertion previews.** For the built-in renderers Blockly draws its own preview as a bare
   `<g class="blocklyInsertionMarker">` on the canvas (`InsertionMarkerPreviewer.useFastInsertionMarkers`,
   `core/insertion_marker_previewer.d.ts:24`), not as a marker block in the list. The wrap
   previewer still makes its own marker block and works; two of its tests looked for Blockly's
   marker block (`packages/blockly-ext/src/checker/wrap.test.ts:293`, `:353`). Blockly's own
   markers can no longer be what the render-queue report
   ([`upstream/blockly-render-queue-retains-disposed-blocks.md`](upstream/blockly-render-queue-retains-disposed-blocks.md))
   says leaks; deleted blocks are to be checked again.
9. **Focus.** `BlockSvg.bringToFront` now gives the focus back to the node that had it
   (`core/block_svg.d.ts:486`; the reordering itself moved to an animation frame), and throws
   when that node's tree is no longer registered. The workspace takes the focus through a
   separate `<rect>` focus target, not its `<g>`; the trash can is a focusable button ("Trash,
   currently empty") and so a Tab stop; Blockly adds a polite live region of its own
   (`#blocklyAriaAnnounce`, `role="status"`) and ARIA roles on the canvas
   (`.blocklyBlockCanvas[role="listbox"]`); a toolbox category takes part in focus
   (`ToolboxCategory.onNodeFocus`).
10. **Drag targets.** `BlockDragStrategy.startDrag` records the drag targets at every drag start
    (read in the 13.4.0 bundle). That covers what the stale-record workaround
    (`apps/desktop/src/editor/sync/dragTargets.ts`) and the report
    [`upstream/blockly-flyout-setvisible-records-hidden.md`](upstream/blockly-flyout-setvisible-records-hidden.md)
    are about, but it also reads every target's place (a forced layout) at each drag start,
    which the workaround avoided.
11. **Zoom.** `WorkspaceSvg.setScale` now rounds the scale to three decimals
    (`Math.round(scale * 1000) / 1000` in the 13.4.0 bundle): 0.521 for 0.5208…, 1.555 for
    1.5552, which the zoom tests and the placement flow compare exactly.

**The seams of [ADR-0014](../adr/0014-extending-blockly.md)** (the list in
[desktop-architecture.md](../developer-guide/desktop-architecture.md#blockly-seams-adr-0014)):
the drag strategy, the dragger and `B2cContinuousFlyout` needed changes (3, 4); the connection
checker, the connection previewer, `B2cContinuousToolbox`, `B2cToolboxCategory`,
`B2cBlockInflater`, `StartValueChange`, the seven custom fields, `DiagnosticIcon`,
`MutatorButton` and `MirrorLabel` compile unchanged against 13.4.0 and their tests pass. The
fields, the icon and the category gained ARIA methods in 13 (`getAriaTypeName`, `getAriaValue`,
`computeAriaLabel`, `recomputeAriaContext`, `onNodeFocus`) that the custom fields do not
override: what a screen reader hears for them is not reviewed against the
[custom field checklist](../security/custom-field-review-checklist.md). Continuous-toolbox 13.4.0
differs from 7.0.9 only by a keyboard navigator for the toolbox (`ContinuousToolboxNavigator`)
and `ContinuousFlyout.headerForCategory`. No private member is used; nothing is patched.

## Changes made in the spike

The smallest changes that make the workspace typecheck, lint and run, in the worktree only
(paths are the worktree's):

| File | Change | Why |
| --- | --- | --- |
| `apps/desktop/package.json`, `packages/blockly-ext/package.json`, `pnpm-lock.yaml` | `blockly` 13.4.0, `@blockly/continuous-toolbox` 13.4.0; jsdom's tree | The upgrade ([Versions and policy](#versions-and-policy)) |
| `apps/desktop/src/editor/sync/drag.ts` | The dragger keeps its canvas itself (`canvasOf`, `:430`), turns the pointer into a `Coordinate` (`pointerPoint`, `:439`), returns Blockly's draggable from `onDragStart` (`:488`), tells the listener on `onDragRevert` too (`:550`); the strategy passes keyboard events through (`:392`), overrides `getTargetBlock` (`:347`) to give a toolbox copy the editor's strategy and tell the toolbox, tells the session of a copy's drag in `startDrag` (`:334`), and offers `copyFromFlyout` (`:368`) for the keyboard; `handleToolboxCopies` (`:125`) replaces the flyout hook | Items 3 and 4 |
| `apps/desktop/src/editor/toolbox/continuous.ts`, `toolbox/plugin.ts:336` | `createBlock` override removed; the toolbox plugin registers `noteCreatedFromToolbox` with `handleToolboxCopies` | Item 4 |
| `apps/desktop/src/editor/keyboard/navigation.ts` | The cursor calls become `Navigator` calls plus `focusNode` (`:26`, `:196`); the toolbox's stops are found by stepping the navigator (`flyoutStep`, `:35`); the add from the toolbox uses `copyFromFlyout` (`:471`) | Items 1 and 4 |
| `apps/desktop/src/editor/keyboard/shortcuts.ts` | While the editor's keys are registered, every Blockly shortcut beyond 12's eight is taken out (`setBlocklyAside`, `:168`) and put back with the last editor (`restoreBlockly`, `:182`) | Item 2: keeps the keyboard exactly as with Blockly 12 |
| `apps/desktop/src/editor/keyboard/flyoutItems.ts:72` | Cast removed | Item 5 (lint: unnecessary assertion) |
| `packages/blockly-ext/src/blocks/css.ts:44` | Falls back to its own `<style>` when Blockly's `blockly-common-style` is already in the document | Item 7 |
| `packages/blockly-ext/src/checker/previewer.ts:148` | `markerPrevious` checked for `null` | Item 5 |
| `packages/blockly-ext/test/setup.ts:12`, `apps/desktop/src/test/setup.ts:21` | English messages set before any test injects a workspace, as the app does | Item 6 |
| `packages/blockly-ext/src/checker/wrap.test.ts:123`, `:305`, `:308` | Blockly's own preview is looked for as its `<g class="blocklyInsertionMarker">` | Item 8 |
| `packages/blockly-ext/src/mutators/rendered.test.ts`, `apps/desktop/src/editor/diagnostics/testing.ts`, `keyboard/mover.test.ts`, `keyboard/targets.test.ts`, `sync/wrapSweep.test.ts`, `toolbox/core.test.ts`, `toolbox/plugin.test.ts`, `sync/drag.test.ts` | Null checks for connections; `copyFromFlyout` for `flyout.createBlock`; `endDrag(undefined, DragDisposition.COMMIT)` | Items 4 and 5 |

22 files, 590 lines added and 313 removed (446 of the changed lines are the lockfile). None of it
is reviewed, and the keyboard port is only as good as its tests below.

## Checks and unit tests

All commands from the worktree's root, on 2026-10-24. "First run" is before the runtime changes
(`blockly-ext`'s two type fixes were in).

| Command | Blockly 12.5.1 | Blockly 13.4.0, first run | Blockly 13.4.0, after the changes |
| --- | --- | --- | --- |
| `pnpm -r --if-present run typecheck` | pass | 30 errors (2 in `blockly-ext`, 28 in the app) | pass |
| `pnpm -r --no-bail --if-present run lint` | (CI) | not run | pass (after the cast in `flyoutItems.ts`) |
| `pnpm -r --if-present run format:check` | (CI) | — | pass |
| `pnpm --filter @blocks2cpp/blockly-ext run test` | (CI) | 84 of 313 failed | 313 passed |
| `pnpm --filter @blocks2cpp/desktop exec vitest run` | (CI) | 269 of 2,021 failed | 79 failed, 1,941 passed, 1 skipped |
| `… exec vitest run --config e2e/vitest.e2e.config.ts --project support` | — | — | 137 passed, 2 skipped |
| `… exec vitest run --config e2e/bench/vitest.bench.config.ts --project bench-unit` | — | — | 146 passed |
| `… exec vitest run --config e2e/visual/vitest.visual.config.ts --project visual-unit` | — | — | 131 passed |
| `pnpm --filter @blocks2cpp/desktop run test:isolation` | — | — | 35 passed |

The 79 desktop failures left, by cause:

- **58 in `sync/drag.test.ts`, `sync/dragWrap.test.ts` and `sync/wrapSweep.test.ts`**: every
  pointer drag on a canvas the editing session loaded throws `Attempted to focus unregistered
  node` from `BlockSvg.bringToFront` (item 9): the node that had the focus is a block disposed
  when the session loaded the document. A probe with a plain workspace did not reproduce it, so
  the session's load is involved. Whether the app can meet it (a drag soon after a document is
  loaded) is not known; the end-to-end flows, which drag after opening projects, did not.
- **9 in the keyboard module** (`keyboard/plugin.test.ts`, `navigation.test.ts`,
  `zoomKeys.test.ts`): the canvas's labels and descriptions are set on the workspace's `<g>`
  while the focus goes to Blockly's focus `<rect>`; `keyboardNavigationController.setIsActive`
  throws after the workspace is disposed; axe reports `aria-required-children` (critical) on
  `.blocklyBlockCanvas[role="listbox"]` and `aria-valid-attr-value` (critical) on four elements
  with Blockly's generated IDs (`#blockly-2w9` …); on an empty canvas the navigator's
  `getFirstNode` gives `undefined` (it is typed `| null`) and the port passes it to `focusNode`;
  and the zoom rounding (item 11).
- **8 in the window's layout** (`app/layout/tabOrder.test.tsx`, `editor/EditorWorkspace.test.tsx`,
  `editor/view/CanvasControls.test.tsx`): the trash can is a new Tab stop before the canvas
  controls, and Testing Library finds two `role="status"` regions (the app's and Blockly's).
- **4 others**: `app/dialogs/dialogs.test.tsx` (the focus given back after a dialog goes to the
  `<rect>`; `Blockly.dialog.alert` no longer reaches `window.alert`), `editor/view/zoom.test.ts`
  (item 11) and `editor/sync/session.test.ts` (after a reload Blockly 13 selects block `b` of the
  new canvas, but the editor's selection tracking does not follow it and the store keeps no
  selection, which FA-8's test checks).

## End-to-end flows

Built as `e2e/README.md` describes: the WebAssembly core
(`WASM_OPT=…/binaryen-version_133/bin/wasm-opt CARGO_TARGET_DIR=/home/user/Blocks_To_CPP/target pnpm wasm:build`),
the frontend (`pnpm --filter @blocks2cpp/desktop run build:e2e`) and the app
(`CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/home/user/Blocks_To_CPP/target/e2e cargo build --locked -p blocks2cpp-desktop --features e2e-hooks,tauri/custom-protocol`),
once with each Blockly. The PR job's set (flows and exit, not the nightly security specs) on the
Blockly 13 build:

```sh
WEBKIT_DISABLE_DMABUF_RENDERER=1 B2C_E2E_APP=<the Blockly 13 app> B2C_E2E_TOOLCHAIN_DIRS=/usr/bin \
  B2C_E2E_ARTIFACTS=<scratch> xvfb-run -a pnpm --filter @blocks2cpp/desktop run e2e specs/flows specs/exit
```

**100 passed, 4 failed, 1 skipped** (105 tests in 33 files, 498 s). The exit test (the guessing
game assembled with real drags from the toolbox, built, played and stopped) passed, and so did
dragging a block from every toolbox category, the drop on the toolbox while a place is
previewed, wrapping, the clipboard keys, save and reload, crash recovery, and the console, build
and run flows: the drag and toolbox port (items 3 and 4) holds in the webview.

| Test | Failure | Cause |
| --- | --- | --- |
| `editor.e2e.ts` "keeps the chosen category selected at every zoom level" | No category row is drawn selected (`.blocklyToolboxSelected`) after a click on *Program* | A click on a category no longer leaves it selected in the app (item 9: categories take part in focus); to be found |
| `canvas-controls.e2e.ts` "zoom from the keyboard on the canvas …" | Tab from the canvas does not reach *Zoom out* within 10 s | The trash can is a Tab stop first (item 9) |
| `placement.e2e.ts` "snaps a block dropped off its connection into an empty C-block at 90 % and at 52 %" | Waits for zoom 0.5208…; the canvas is at 0.521 | Item 11 |
| `tray-delete.e2e.ts` "deletes it anywhere over the categories …" | Over the trash can the lid does not open (the drop is still a delete: the toolbox is not marked and its column not tinted, as expected) | The trash can's lid in Blockly 13; to be found |

The four spec files pass on the Blockly 12.5.1 build of the same commit on this machine (10 of 10
tests), so each failure comes with Blockly 13.

## Visual diff

`pnpm --filter @blocks2cpp/desktop exec vitest run --config e2e/visual/vitest.visual.config.ts --project visual`
under `xvfb-run`, with each build. Local screenshots are only comparable with each other, so the
two builds were also compared with each other (pixelmatch at 0.1, as `compare.ts` does).

- **Blockly 12.5.1** against the committed Linux baselines: every scene passes (at most 0.057 %)
  except the four console scenes (19.1 %, 10.0 %, 3.3 %, 2.2 %), where this machine's console
  header has a *Process group only* notice (no cgroup here) that wraps it onto two lines. That is
  this machine, not Blockly, and the same numbers come back with Blockly 13.
- **Blockly 13.4.0** took no scene: each waits until a click on *Program* leaves *Program* the
  only category drawn selected, and none is (as in `editor.e2e.ts`). With that wait skipped for
  one local run (an environment switch in `e2e/visual/settle.ts`, reverted, not in the patch):

| Scene (light; dark alike) | 13.4.0 against 12.5.1, this machine | What changed |
| --- | ---: | --- |
| `canvas-guessing-game` | 3.42 % | Every expression slot (the shadows of `random integer from`, `0`, the texts, `guess == secret`) has a thick yellow outline, the colour Zelos uses for the selected block; the boolean slots lose their hexagonal ends and are drawn round; *Program* is not highlighted in the toolbox |
| `chrome-window-smallest`, `chrome-menu-open` | 2.13 %, 1.15 % | The same slot outlines on the program behind |
| `chrome-toolbox-loops`, `-io`, `-functions` | 1.69–1.86 % | The category column is scrolled about 2 px further; the flyout's blocks are the same |
| `chrome-toolbox-program`, `-variables` | 0.05 % | Under the 0.5 % threshold; not inspected |
| the toolbar, `-math`, `-logic`, `-text`, `-control`, the console scenes | 0 | — |

The slot outlines and round boolean slots are **not acceptable** as they are: the hexagon is how
the editor shows a `bool` slot, and an outline on every slot reads as a selection. They come
from how Blockly 13's Zelos draws a shadow that is one field (a "full block field"); the cause in
the app's shadows (`applyShape`, the expression field) is not found yet. Once fixed, new
baselines come from CI's *visual-baselines* run, never from here.

## Benchmarks

The gated webview benchmarks, as a local A/B with the repository's own tools (the harness of the
worktree, whose `e2e/` is that of `22c2532`):

```sh
AB_XVFB=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 B2C_E2E_TOOLCHAIN_DIRS=/usr/bin \
  apps/desktop/e2e/bench/ab-rounds.sh <12.5.1 app> <13.4.0 app> <out> \
  preview.bench select.bench drag.bench cold-start.bench
python3 tools/bench-compare.py ab --os linux --base <out>/base --head <out>/head \
  --accepted apps/desktop/e2e/bench/accepted.json --accepted-base apps/desktop/e2e/bench/accepted.json \
  --runs <out>/ab-runs.tsv
```

Both apps are the E2E builds above (debug backend), Linux under xvfb, 2 rounds of 5 samples in
alternating runs (base, head, head, base), 10 samples per side, 2026-10-24/25; the machine's load
was 0.5–1.2 on 4 cores when checked. Every benchmark ran in every round on both sides.

| Metric | Gated | 12.5.1 | 13.4.0 | Change |
| --- | --- | ---: | ---: | ---: |
| `webview.cold-start` | yes | 1,797 ms | 1,817.5 ms | +1.1 % |
| `webview.preview-1000.p95` | yes | 82.5 ms | 85 ms | +3.0 % |
| `webview.edit-1000.p95-all` | yes | 1,512 ms (p95 of 100 edits) | 1,328 ms | −12.2 % |
| `webview.select-1000.max-task` | yes | 801 ms | 510.5 ms | −36.3 % |
| `webview.drag-5000.frame-p95-all` | yes | 48 ms (p95 of 1,294 frames) | 56 ms (of 1,274) | **+16.7 %** |
| `webview.edit-1000.p95` | no | 1,493.5 ms | 1,335 ms | −10.6 % |
| `webview.drag-5000.frame-p95` | no | 47 ms | 53.5 ms | +13.8 % |

- **The drag frames are over the 10 % gate**, so the A/B job would fail the upgrade as it is. Not
  profiled. Blockly 13's block drag does more at its start: it records the drag targets (item 10)
  and caches every connection pair that the connection checker allows between the dragged block
  and the rest of the canvas (`cacheAllConnectionPairs`, for keyboard moves and announcements:
  each connection on the canvas against each of the dragged block's, through
  `B2cConnectionChecker`), which at 5,000 blocks may be one long frame per drag. To be measured,
  then fixed or recorded in `accepted.json`.
- The selection gain fits Blockly 13 doing `bringToFront`'s reordering in an animation frame
  (item 9), the work the P-3 spike found to be most of a click (0.3–0.9 s for some targets); not
  profiled either.
- One local run of 2 rounds. On this shared machine the same two builds have moved by more than
  the gate between runs before (polish plan, W4b's regression check), so CI's A/B job on both
  systems decides; the native pipeline benchmark does not involve Blockly and was not run.

## Trusted Types and the CSP

In the 47 reports the end-to-end run wrote (`trusted-types.jsonl`), the report-only Trusted Types
policy was active every time and **no violation** was reported. Blockly 13.4.0's bundle has no
`innerHTML`, `insertAdjacentHTML`, `eval` or `new Function`, as 12.5.1's, and uses `DOMParser`
in 4 places where 12.5.1 used it in 10. Its styles are still injected as a `<style>` element
(`blockly-common-style`), which `style-src 'unsafe-inline'` already covers. No CSP change is
needed.

## xterm.js 6.1

Tried only to learn the two things M3-3 depends on; the app stays on xterm.js 5.5.0 and addon-fit
0.10.0 until a stable 6.1 is 7 days old.

- **The frozen prototype.** Loaded in Node.js with happy-dom and `Object.prototype` frozen, as
  Tauri's `freezePrototype` leaves it: 5.5.0 loads and opens a terminal with addon-fit 0.10.0;
  6.0.0 fails while loading with `TypeError: Cannot assign to read only property 'toString' of
  object '#<Object>'` (the failure the [dependency notes](../developer-guide/dependencies.md#notes)
  describe); **6.1.0-beta.315 loads**, and addon-fit 0.12.0-beta.311 loads with it. Opening a
  6.1 terminal fails in happy-dom with or without the freeze (`value must not be falsy`: 6.1
  measures glyphs on a 2D canvas, and happy-dom's `getContext('2d')` returns `null`), so the
  console's unit tests and `src/panels/frozen-prototype.test.tsx` will need a canvas stand-in.
- **The wheel (U14b).** Not tried in the webview. 6.1's viewport (`src/browser/Viewport.ts`)
  scrolls through VS Code's scrollable element: it keeps the scroll position in its own model,
  and a render moves it only when the buffer's position was changed by something else
  (`ydisp !== this._latestYDisp`). The 5.5 behaviour that `panels/console/wheel.ts` works around,
  every render setting the viewport element's `scrollTop` and taking the next scroll event as its
  own, is gone by design. Whether the wheel then works while output streams at device scale 2 is
  to be checked by hand (WebKitGTK at 125 % and 150 %, and WebView2) before `wheel.ts` goes.
- The console uses none of the options 6.x removed (`windowsMode`, `fastScrollModifier`,
  `overviewRulerWidth`) or the old viewport's classes; `attachCustomWheelEventHandler` and
  `scrollLines` remain.

## Remaining work

For the upgrade after M3, with the effort of each in working days:

1. **Decide on keyboard navigation** (owner): keep the editor's own (with Blockly's shortcuts
   set aside, as in the spike) or adopt Blockly 13's, which now covers moves, the toolbox and
   screen readers. Either way, review and test the port to `Navigator` (empty canvas,
   wrap-around, the toolbox's stops). 3–5 days.
2. **Focus and ARIA**: the drag-time focus error after a load; labels on the focus `<rect>`; the
   trash can's Tab stop; one live region instead of two; Blockly's `listbox` role and axe's two
   critical problems; the category that a click no longer selects; the custom fields' new ARIA
   methods against the checklist. 2–3 days.
3. **Rendering**: the slot outlines and round boolean slots; then CI visual baselines on both
   systems. 1–2 days.
4. **The rest of the tests**: zoom rounding, the trash can's lid, the dialogs' alert, the
   selection tracking after a reload (FA-8), the 58 drag tests once 2 is done. 1 day.
5. **The drag benchmark**: profile the 5,000-block drag, then fix it (for example by keeping
   Blockly's connection-pair cache out of pointer drags, if a documented seam allows) or record it
   in `accepted.json` with the owner's agreement. By then M3's typed slots
   ([ADR-0018](../adr/0018-slot-typing.md)) make each pair the cache checks a type check, where
   Blockly 12 asks the checker only about connections within the snap radius; so measure
   `webview.drag-typed-1000.frame-p95-all` (added in W1) on both builds as well. 1 day.
6. **Records**: the seams list and its checked versions, ADR-0014's consequences, the dependency
   rows and notes, `dragTargets.ts` and its upstream report (item 10), the render-queue report
   (item 8), and an A/B run on both systems in CI. 1 day.

About 9–13 days in all, and the Windows E2E job may add work that Linux cannot show. Separately,
**xterm.js 6.1** once stable and 7 days old: a canvas stand-in for the unit tests, the manual
wheel check, then removing `wheel.ts` (about 1 day).

## The patch

The worktree's diff against `22c2532` is kept as
[`m3-spike/blockly13.patch`](m3-spike/blockly13.patch) (21 files, 930 lines), without its
`pnpm-lock.yaml` hunk. It pins `blockly` and `@blockly/continuous-toolbox` at 13.4.0. It applies
cleanly to `22c2532` and to `e49e61b` (checked with `git apply --check`), but it is for reading,
not for applying: it is the spike's state, with the failures above still open, and W4 will change
the same editor files (`sync/drag.ts`, `keyboard/navigation.ts`, `keyboard/shortcuts.ts`,
`toolbox/continuous.ts`). The upgrade after M3:

- bumps both pins to the 13.x that is current then and at least 7 days old;
- checks Blockly's `jsdom` peer range and the Node.js floor again (22.22.2 for `jsdom` 30.1.3,
  [Versions and policy](#versions-and-policy));
- runs `pnpm install` under the policy, which writes the lockfile again;
- makes the changes again by hand where W4 has changed the same files, with the patch as the
  guide.

The screenshots and logs were not kept.
