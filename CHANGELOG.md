# Changelog

All notable changes to Blocks2Cpp are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) (0.x until 1.0).

## [Unreleased]

### Added

- Technical specification, architecture decision records and security policy.
- Specification website published with GitHub Pages.
- Rust workspace with the shared contracts of the compiler pipeline: validated
  IDs, the diagnostics model, injection-safe C++ text encoders, the Semantic AST,
  source maps, the project file model and the milestone-M1 block catalog.
- Project loading with every validation rule of the format, canonical saving
  and content hashing; the block catalog and its resolve stage.
- The analyser: lowering of the 32 milestone-M1 blocks, expression slots,
  scoping, types, flow checks and lints, with 46 documented diagnostics.
- C++ generation: readable, deterministic code with source maps and support
  helpers for random numbers and safe input.
- Toolchain discovery and probing, safe compiler commands and environment,
  and parsing of g++ diagnostics in SARIF, JSON and text.
- Process control that stops whole process trees (process groups on Linux,
  Job Objects on Windows).
- The `b2c` command-line tool: `check`, `generate`, `build`, `run`,
  `toolchains`, `migrate` and `fmt`.
- Fifteen example projects with golden tests, a malicious-project regression
  suite, fuzz targets, and CI on Linux (GCC 11, 13 and 15) and Windows (MSYS2)
  with security scanning.
- The desktop app shell (milestone M0): a Tauri 2 window with an empty Blockly
  canvas (Zelos renderer) and placeholder panels, hardened with the specified
  Content Security Policy, the isolation pattern and minimal capabilities, and
  built and checked in CI on Linux and Windows.
- More CI quality gates: API docs built with warnings denied, Markdown lint,
  an external link check, line-coverage gates (80% for all Rust code, 90% for
  the compiler crates), a size budget for the WebAssembly core, checks that
  generated files are up to date, and fuzz targets for the g++ diagnostics
  parsers, seeded with recorded GCC 11 and GCC 13 output.
- Milestone M2 foundations: the IPC contract between the editor and the
  backend (crate `b2c-ipc`, with generated TypeScript types and an isolation
  allowlist), machine-local settings and recent-project stores, pseudo-terminal
  sessions for running programs (openpty on Linux, ConPTY on Windows), the
  editor's scope query, the WebAssembly core for the live C++ preview, the
  catalog export that generates the toolbox and the block reference, and
  frontend test tooling with coverage and accessibility checks.
- Pasting blocks is validated like opening a file (new codes `B2C-E0138` and
  `B2C-E0139`), and loose block stacks are saved intact (`stack`).
- Nightly fuzzing with a kept corpus, and weekly mutation testing and
  dependency-health reports.
- Desktop editor building blocks (milestone M2): Blockly blocks generated from
  the catalog with Zelos shapes, light and dark themes and custom fields that
  show text literally; a type-aware connection checker and the variadic
  mutators; the app shell (start-up, state store, layout, toolbar, Run gating,
  accessible dialogs); the C++ code panel with source-map highlighting, the
  Problems list, the console and the build output; and build-cache eviction
  (2 GiB limit by default, 30-day pruning, Clear build cache).
- Builds run as cancellable sessions with progress, record a build manifest
  instead of a build stamp, compile modules in parallel, and link a small IDE
  unit (UTF-8 console on Windows) into app builds only; the Windows cache root
  is `%LOCALAPPDATA%\Blocks2Cpp`.
- Run sessions for the editor's console: terminal or pipe mode, batched output
  with flood protection, rate-limited input, friendly exit messages and
  sanitizer report summaries.
- Stronger containment: compilers and programs run in cgroup v2 scopes on
  Linux when a user service manager is available (otherwise a process group
  with an RSS watchdog), stale scopes are cleaned up at start-up, and captured
  runs on Windows inherit only an explicit list of handles. Compilers stopped
  for memory now report a limit, not a compiler crash.
- The app's toolchain list: background discovery that never delays start-up,
  compilers added by hand, a remembered choice that is checked again before
  each build (new warning `B2C-T1022` when it falls back), and the Linux
  distribution for setup instructions.
- Machine-local trust store (`trust.json`) with project and folder trust,
  safe across several app instances; the Windows Mark-of-the-Web check; and
  size-rotating local logs (5 × 5 MiB).
- The WebAssembly core answers the editor's scope, type and conversion
  questions and makes and checks clipboard payloads, with fresh IDs and
  references bound again where blocks are pasted.
- More tests from mutation testing: the mutants it found missed in the
  project-file parser, the decoder and the toolchain code are now caught, and
  the few equivalent ones are excluded with a reason each.
- The desktop app's backend (crate `b2c-app`): every IPC command without
  Tauri, with project open, save and close, workspace trust, builds and runs,
  toolchains, settings and the recent list, and start-up work that never
  delays the first window.
- Recovery snapshots for unsaved work, kept per app instance with locks so
  several instances never take each other's snapshots.
- The block editor: the Blockly canvas reads and writes the project format
  faithfully (unknown or unrepresentable blocks become placeholders that keep
  their data, so an unchanged project saves byte for byte), with a live C++
  preview 50 ms after each change, fresh symbol IDs for duplicated and pasted
  blocks, and a module switcher.
- The editor's toolbox: the catalog's categories in a continuous, Scratch-style
  toolbox with presets, and Program, Variables and Functions categories built
  from the project.
- Problems on blocks: a badge with a shape per severity, an outline and a mark
  on the exact part, for live and last-build diagnostics (dimmed once the
  project changed), and two-way highlighting between blocks, the C++ view and
  the Problems list.
- Review fixes: the scope query answers at disabled blocks; pasting into a
  statement list unstacks loose stacks; numbers JavaScript cannot hold exactly
  are refused on load, and free-form floats are saved in JavaScript's form; the
  console no longer traps the keyboard (Ctrl+Tab leaves it while a program
  runs) and shows dropped output; dialogs and docks keep keyboard focus; long
  expression tooltips are cut; and the editor reads the WebAssembly core anew
  after a crash recovery.
- Review fixes: on Windows, trust compares only ASCII letter case in paths,
  so a lookalike folder name is never trusted; a build returns its ID at once
  and can be cancelled while it waits for the compiler search or runs the
  front end; the program that was just built survives cache eviction; ended
  runs release their terminals; and dynamically linked Windows programs find
  the compiler's DLLs.
- The desktop app speaks the full M2 IPC contract: 36 commands for projects,
  recent files, recovery, trust, toolchains, builds, runs, settings and help
  links, with build and run output streamed through channels. Open, Save As,
  Choose g++ and the trust confirmation are native dialogs raised by the
  backend, which the editor cannot show or answer. Closing the window with
  unsaved changes asks first; quitting stops every build and running program.
  Local JSON-lines logs (five files of 5 MiB) hold no project content.
- Projects in the editor: a start page with templates, _Open…_ and the recent
  projects, _Save_ and _Save as…_, unsaved-changes prompts, and one project
  per window.
- Build, Run, Stop and Run again from the toolbar and shortcuts, with the
  build output, the interactive console (acknowledged output, rate-limited
  input, resizing) and friendly exit messages.
- The toolchain page (setup instructions while no compiler works, the list of
  compilers with their health checks, rescan and choosing g++ by hand), the
  Settings page, and the Restricted Mode banner with _Trust…_ and _Revoke
  trust_.
- Autosave of unsaved changes into recovery snapshots every 30 seconds and
  when the window loses focus; after a crash the start page offers to restore
  or discard them. A file watcher notices when the open project's file changes
  on disk (the app's own saves never trigger it), and the editor offers
  _Reload_ (which re-checks trust) or _Keep mine (save as…)_.
- Copy, cut, paste and duplicate blocks through a validated clipboard
  format (with the C++ as plain text for other programs): pastes get fresh
  IDs, re-bind references where they land, and are refused with the
  loader's codes when they are malformed or would break the format's limits.
- Review fixes (security): a crash-recovery snapshot is trusted only by the
  trust record of its own path, so a link swapped in after a crash restores
  it in Restricted Mode; _Choose g++ manually…_ refuses a compiler inside an
  open project's folder or the build cache before running it, and its dialog
  starts in a system folder; on Windows the trust dialog's default button is
  _Stay in Restricted Mode_; the window navigates only within its own
  platform's app origin; and the webview can no longer be reloaded by key or
  by its context menu.
- Review fixes: edits made while a save runs stay unsaved, so closing the
  window asks first and their recovery snapshot is kept; closing a project or
  quitting while its program or build starts leaves nothing running; Reload
  deletes the snapshot of the changes it discards and waits for a save in
  progress; restoring a snapshot waits for and holds back New, Open and Save;
  autosave never races a save; a canvas that cannot be read back is shown in
  a banner and is never built silently; opening a project leaves no label
  updates to undo, and saving an untouched project keeps its bytes,
  viewport included; Delete then Undo, selecting from Problems and saving
  during a drag keep the blocks exactly; opening another project resets the
  console; the _lines skipped_ marker appears where output was dropped; the
  status bar says _Saved_ for a file just opened; and pastes, duplicates and
  cuts of very long statement chains, or pastes that would make the file
  larger than 32 MiB, are refused whole instead of half done.
- Keyboard use of the block editor: the arrow keys move between blocks and
  their parts, _M_ moves a block to a place chosen with the arrow keys, _T_
  opens the toolbox, and copy, paste, duplicate, delete, undo and the block
  menu act on the keyboard focus. A live region announces what the keyboard
  reaches, the Tab order runs toolbar, toolbox, canvas, then the docks, focus
  returns where it was when a dialog closes, Blockly's animations follow the
  reduced-motion setting, and the colours and focus rings meet WCAG 2.2 AA
  contrast in the style sheets (a manual screen-reader checklist covers the
  real webviews).
- End-to-end tests drive the real app on Linux and Windows (selenium-webdriver
  and `tauri-driver`): the guessing game is assembled from the _Empty_
  template with real drags, run, answered until _Correct!_ and stopped. CI
  checks that release builds contain no test hooks.
- Desktop hardening: on Windows the app and the CLI restrict where DLLs are
  loaded from before anything else, and the app's manifest allows long paths
  (checked in CI); the app's pages carry a report-only Trusted Types policy;
  and on Linux without cgroups the compiler's memory limit is set before it
  starts, so processes it forks at once are limited too.
- Review fixes: keyboard moves offer only the places a pointer drag accepts,
  so they can no longer make a connection the type rule refuses; adding a
  block from the toolbox with the keyboard is one undo step, and Ctrl+Z after
  Escape no longer brings the cancelled block back; the keyboard focus no
  longer falls out of the editor when the focused block is deleted or the
  toolbox's blocks are rebuilt; dialogs give the focus back to the block
  canvas; with reduced motion on, choosing a toolbox category jumps to it; a
  compiler that runs out of memory under the address-space limit is reported
  as such (`C:limit`) instead of as a crash; and the Windows application
  manifest has no XML declaration, which MinGW builds embedded after a space.
- End-to-end harness fixes: on Linux, stopping a test kills the app and
  everything it started even when the WebDriver session did not end; a
  driver that exits at once fails the test at once with its output; a
  refused block insertion leaves no undo step; and the msedgedriver download
  is checked against Microsoft's exact signer and the exact version.
- End-to-end flows on Linux and Windows: Restricted Mode and trust, save and
  reload, crash recovery, Stop, exit decoding, closing the app while a
  program runs, a 10,000,000-line output flood, the toolchain setup page,
  external changes, the clipboard, settings after a restart, the variable
  menus, the connection checker, selecting a block from its code, drags from
  every toolbox category, and a build and a run with input.
- Nightly end-to-end security tests in the real app on Linux and Windows:
  every file of the malicious-project suite, build and run refused in
  Restricted Mode, IPC abuse and messages around the isolation frame, markup
  shown as text, blocked navigation and new windows, and trust changes made
  outside the app.
- Benchmarks with a 10% regression gate: the compiler pipeline at 1,000
  blocks (criterion), and in the app the cold start, the live preview at
  1,000 blocks and dragging in a 5,000-block workspace; a visual diff of the
  block canvas against a baseline per system; and nightly end-to-end,
  security and benchmark jobs on both systems.
- The user guide's getting-started page (from the first start to a guessing
  game built from blocks), and the protocols of M2's manual checks: screen
  readers, and a usability session with first-time users together with the
  review that nothing of later milestones has an entry point in M2.
- `BUILDING.md`: one place with the steps to build and run Blocks2Cpp from
  source on Debian, Ubuntu and Windows (Fedora and Arch untested). It
  recommends the release build for using and judging the app, and its
  troubleshooting covers blank windows and Wayland.
- CI checks the relative links and heading anchors of every Markdown file,
  and the documentation references in the build scripts' messages
  (`node site/build.mjs --check-repo-markdown`).
- Decision records: ADR-0012 (loose blocks are parked: one warning, never
  generated, never blocking Run), ADR-0013 (`formatVersion` is the lowest
  version whose keys cover the file; an unknown key from a newer app is
  `B2C-E0108`) and ADR-0014 (the allowed Blockly seams and patched
  dependencies), all accepted on 2026-10-10. ADR-0011 and ADR-0002 are
  partly superseded.
- A/B benchmark jobs (`.github/workflows/bench-ab-run.yml`), started by
  `bench-ab-pr.yml` for the `bench-ab` label on a pull request (and each push
  while it has the label) and by `bench-ab.yml` for a manual run, which
  measures the commit it is dispatched on against one of its ancestors (by
  default where it branched off the default branch). They build the base and
  the head and run the chosen webview benchmarks against both in alternating
  runs on one runner, 10 samples per side. A gated metric more than 10% worse
  than the base fails (`tools/bench-compare.py ab`). On a pull request the head
  is the pull request merged into its base branch and the base is that branch,
  and every benchmark but cold start runs; the selection benchmarks' rounds
  start where the round before stopped, so both sides click the same targets
  as a nightly run.
- `apps/desktop/e2e/bench/accepted.json` records intended slowdowns. The
  nightly comparison restarts an accepted metric's history instead of
  failing, and an A/B run passes a metric that its head newly accepts. The
  benchmark history file is now format version 2; version 1 is still read.
- Benchmarks `webview.select-1000.max-task` (gated: the longest task after a
  click that selects a block at 1,000 blocks, 8 scopes in turn),
  `webview.guessing-game.select.max-task` and
  `webview.guessing-game.edit.max-task` (reported only). A click fails the
  run unless the toolbox then lists the clicked block's scope, so every
  sample includes the toolbox's rebuild.
- Benchmarks of a program's output in the console:
  `webview.console-200000.run` (gated: from a click on Run until the console
  says Finished, for 200,000 lines, the median of 3 runs per sample) and
  `webview.console-200000.max-task` (reported only).
- Every end-to-end flow reports its longest main-thread tasks, named with the
  app's User Timing spans that overlap them (`long-tasks-<launch>.json` per
  test, `long-tasks.md` in the E2E job summaries). Report only; it never
  fails a test.
- User Timing spans for the preview pipeline and its phases, each rebuild of
  the toolbox's flyout, block selection and the code panel (`b2c:preview`,
  `b2c:toolbox-refresh`, `b2c:select`, `b2c:code-panel`). The benchmarks
  print each span's time per edit or click beside their metrics.
- The end-to-end test hook's `scrollCanvasBy(dx, dy)` and `timings()` (the
  latest duration of each User Timing span).
- Manual test: an M2 performance profile protocol
  (`docs/manual-tests/m2-performance-profile.md`). It records the owner's
  system, re-measures the first-start layout (U6) and lag (U11) in a release
  build, profiles clicks that change nothing, and checks whether GTK reports
  a primary monitor under Wayland. A release build is profiled unchanged
  through WebKitGTK's remote inspector.
- The toolbox's block list can be resized with a splitter at its right edge
  (pointer or keyboard). By default it is as wide as its blocks, at most
  320 px, and it narrows to leave the canvas at least 200 px.
- The toolbox's width, the panels' sizes and whether each panel is hidden are
  remembered on this computer (`settings.json` `ui.layout`), written as soon
  as a change ends, so closing the window right after it keeps it.
- Canvas controls: _Zoom out_, _Zoom in_, _Reset zoom_ (back to 100 %; the
  button shows the zoom now) and _Show all blocks_ (scrolls to the blocks
  without changing the zoom) are named buttons at the canvas's top right,
  reachable with Tab right after the canvas. `Ctrl+=`, `Ctrl+-` and `Ctrl+0`
  zoom from the keyboard, on the canvas and on the controls, and announce the
  new zoom (04 §4.1, §4.7).
- Rename the project by clicking its name in the top bar, or with ≡ _Rename
  project…_. Enter or moving away keeps the name and Escape cancels; `Ctrl+S`,
  `Ctrl+B` and `F5` keep it before they save, build or run. A name
  has 1–100 characters on one line and is checked with the project-text
  rules. Renaming is an unsaved change that the C++ header comment follows,
  and it never renames the file (04 §4.1, D12).
- _Undo_ and _Redo_ in the ≡ menu, greyed out when there is nothing to undo
  or redo. `Ctrl+Z`, `Ctrl+Y` and `Ctrl+Shift+Z` now undo and redo the block
  editor's changes also when the focus is outside the canvas (Problems,
  toolbar, docks), but not in text fields, dialogs, the ≡ menu or a running
  program's console.
- The "Your latest change was not checked" banner has an _Undo last change_
  button; when the undo lets the canvas load, the focus goes to the workspace.
- Screen readers can read the console: the rows on screen line by line, and
  new output as it arrives, afresh for each run (not during an output flood).
- The toolchain page lists the compilers a search skipped and why (inside an
  open project's folder, the build cache, the folder the app was started
  from, or a network folder).

- Blocks of an older version or of a retired type are upgraded when a
  project, a recovery snapshot or pasted blocks are loaded. The project then
  has unsaved changes, and saving keeps the file as it was as `.b2c.bak`
  (ADR-0015).
- `b2c migrate` applies block upgrades; `--in-place` keeps the original as
  `.b2c.bak`, and nothing is written when a block cannot be upgraded
  (`B2C-E0603`).
- Blocks left loose on the canvas are parked (ADR-0012). They are kept and
  shown with a ⚠ `B2C-W0505` warning ("This block isn't attached to 'when
  program starts' or a function, so it doesn't run."), are never generated,
  no longer disable ▶ Run, and no longer make `b2c check` fail. A loose block
  you disable gives no warning. `B2C-E0604` now covers only placements the
  editor cannot produce. A block straight from the toolbox whose variable or
  name is not chosen yet (an _ask_) is parked the same way: the block catalog
  no longer reports the empty choice (`B2C-E0606`), and the analyser asks for
  it (`B2C-E0430`, "Choose a variable in this block.") once the block is
  attached.

- Scratch-style wrapping: an empty C-block dragged onto a statement takes its
  place and wraps that statement and everything below it in the list. The
  grey preview shows the C-block around them, and one Undo takes the drop
  back. A block moved with the keyboard is still inserted (03 §3.3). A
  counted loop from the toolbox that wraps statements counts with a name they
  do not declare (`j` around `create i` or a `for i` loop).
- End-to-end flows `placement` (drops off target into a C-block at 90 % and
  52 % zoom; drops on the toolbox's flyout, after moving over it and in one
  go with the jump, and on the trash can while a place is previewed, each
  deleting only the dragged block and undone with Ctrl+Z; _Show all blocks_
  keeping the zoom) and `placement-wrap` (wrapping previewed, dropped, undone
  and redone, and at zooms 0.5, 0.9 and 1.5 at the Windows runner's window
  size). `B2C_E2E_WRAP_SWEEP` turns the zoom test into a measurement.
- The plan records the render-management spike (P-3): with a local patch of
  Blockly 12.5.1, appending to a 500-statement chain draws 4 blocks instead
  of 503, and `edit-1000.p95` falls 34 % in a local A/B. Clicks are
  unchanged: their cost is `BlockSvg.bringToFront`. The patch and two
  upstream report drafts are in `docs/plans/upstream/`; the app still ships
  Blockly unchanged, and the patch is M3-2.
- A developer guide (`docs/developer-guide/`) for contributors: the desktop
  app's architecture with the list of Blockly seams (ADR-0014), its security
  settings, the dependency table, testing and recipes. The desktop app's
  README is now a short code map, and its test IDs are listed in the
  end-to-end README.
- A manual test protocol for the toolchain matrix
  (`docs/manual-tests/m2-toolchains.md`): the owner's Debian g++ cell for M2,
  and the other compilers of 07 §7.1 to run or defer to M3.
- `tools/check-moved-text.py`, which checks that text moved out of a split
  document was moved, not lost (`--self-test`).
- Building the app with an out-of-date WebAssembly core now stops with the
  command that fixes it (`pnpm wasm:build`); `pnpm app:build` builds the core
  and then the app. The dev server warns about an out-of-date core, and a
  development build will not start when its core has another block catalog
  than the backend.
- The M2 threat-model review is recorded in
  `docs/security/threat-model-reviews.md`, one entry per milestone (08
  §8.13), with its findings and accepted residuals.
- Tests: the nightly IPC-abuse test checks the backend's debug log for
  `app_quit` after the malformed messages, and mutates objects nested in a
  request (`settings_update`'s `ui.layout`, for example) as well as its top
  level. The isolation-hook tests send `constructor`, `prototype` and
  `__proto__` keys, extra keys and other payloads to all 11 commands that
  take no arguments. The security specs' README records the 2026-10-06
  Windows `app_quit` incident and its likely cause.
- A visual diff of the window's chrome (the M2 polish plan's W7-1), beside
  the block canvas's: in the smallest window (800×560), the whole window, the
  toolbar's Debug/Release drop-down closed and hovered, the ≡ menu open over
  the editor, the toolbox after a click on each category, and the console
  after two runs (scrolled up 4 lines, and with the bottom panel at its
  tallest). Light on both systems and dark on Linux (`GTK_THEME=Adwaita:dark`),
  one launch per theme. Each scene waits for fonts, animations and two
  identical screenshots in a row, parks the pointer, hides the caret and
  WebKitGTK's overlay scroll bars, and masks the toolchain, the elapsed time
  and the app version in the C++ header. Every scene is listed in the
  summary; the candidates start without baselines until they are committed
  from CI (W7-2). The canvas screenshot now sets its window through the
  window-size seam.

### Changed

- The specification gains goal N12 (interaction latency, met in M5); N4 is
  measured on the compiler core's update, and G3 says the preview follows
  within 100 ms of the last change. Places where the text described something
  other than the code are corrected, and the roadmap places the M2 polish
  wave inside M2, before the usability sessions.
- A whole number written with a leading zero (`010`) gets its own message:
  "Write `10`, not `010`: in C++ a number that starts with 0 is octal."
- A project saved by a newer Blocks2Cpp with a key this version does not know
  is reported as "made with a newer version of Blocks2Cpp" (`B2C-E0108`)
  instead of "unknown key" (`B2C-E0110`). No `B2C-E0108` message names a
  version taken from the file any more.
- The toolbox offers each block once: _repeat until_ and _if … else_ are
  chosen on the block (the loop's _while_/_until_ menu, ⊕ for _else_).
  `b2c-catalog` rejects a second toolbox entry for a block and an entry label
  that is a category name.
- Functions: the _My Blocks_ heading is now _Your functions_, with a hint
  while the module has no function. Only the shown module's functions are
  offered, under a module heading only when the project has more than one
  module. A call to a known function shows ⊖/⊕ only to bring its arguments
  back in line with the definition.
- Block labels name every field: "create … constant ☐", "print … new line ☑
  to [the console ▾]" and "character 'a'"; the character field takes one
  printable ASCII character. `b2c-catalog` checks that every friendly label
  shows each field and fixed input, with words next to each checkbox.
- Variable menus leave out what cannot be changed there: loop counters and
  read-only parameters (as well as constants) for _set_ and _ask_, and text
  and true/false variables for _change_. A chosen variable stays shown, and
  the Variables category presets blocks only to variables their menus offer.
- The trash can keeps nothing; Undo brings deleted blocks back. The block
  menu no longer offers _Inline/External Inputs_, which were never saved. The
  canvas menu's _Delete N blocks_ counts the blocks it deletes (not
  expression slots), always asks with _Cancel_ as the default, and one Undo
  brings them all back; the block menu's _Delete_ counts the same way.
- ▶ Run and Build stay enabled while the first compiler search runs. The
  build waits for it (at most 30 s) and the build output says so; if the
  search finds no g++, the setup page opens.
- The README's _Get Blocks2Cpp_ section links to `BUILDING.md` instead of
  listing commands. The getting-started guide and the manual-test protocols
  ask for a release build made with it, and the build errors for a missing
  WebAssembly core or its tools point to it.
- Dependabot groups minor and patch updates per ecosystem and opens each
  major update on its own. The Tauri crates and packages, which must move
  together, have groups of their own. New majors of Blockly and its plugins,
  xterm.js (with its add-ons' minor versions), TypeScript and Vitest wait for
  a scheduled migration, and those of `@types/node` for the Node.js runtime.
- Dependencies updated to releases at least 7 days old: Tauri 2.12.1 (crate,
  `@tauri-apps/api` and `@tauri-apps/cli`) with the dialog plugin 2.8.1,
  `getrandom` 0.4, Vite 8.3.2, ESLint 10.12, typescript-eslint 8.71 and other
  patch releases, and the MSYS2 setup action 2.33.0.
- Dependencies updated on 2026-10-15, again to releases at least 7 days old:
  `toml` 1.1.7, CodeMirror's view 6.43.14 and language 6.13.1, Radix's dialog
  1.2, tabs 1.1.22 and tooltip 1.3, Vite 8.3.4, axe-core 4.14 and other patch
  releases, and the Node.js setup, artifact and CodeQL actions. Tauri 2.12.2
  and later, `toml_parser` 1.1.5, `selenium-webdriver` 4.51 and `@tauri-apps`
  2.12.2 and later were too new; `pixelmatch` 8 changes what the visual diff's
  threshold measures and moves on its own.
- Dependencies updated on 2026-10-24, again to releases at least 7 days old:
  Tauri 2.12.4 (crate and `@tauri-apps/api`; `@tauri-apps/cli` 2.12.3) with
  the dialog plugin 2.8.3, `proptest` 1.12, `toml` 1.1.8, Radix's dialog 1.3,
  tabs 1.2 and tooltip 1.4, React 19.3.1, Vitest 4.1.12 and other patch
  releases, and the CodeQL, OSV-Scanner, Scorecard and zizmor actions (zizmor
  1.31.1). Vitest 4.1.12 and `tokio` 1.53.3 fix advisories, an override moves
  `markdown-it` (through markdownlint-cli2) to 15.0.3, and the `braces`
  exception is gone. `happy-dom`'s advisory is a tracked exception until its
  fix, 21.0.4, is 7 days old on 2026-10-27; Tauri 2.12.5, `wry` 0.57.1 and
  typescript-eslint 8.72 were too new.
- `webview.edit-1000.p95` is now gated and measured with the C++ panel shown,
  until the page has finished what the edit started. It is redefined in
  `accepted.json`, so its history restarts.
- Each edit does less work in the editor: call blocks are labelled from the
  preview's symbols instead of one scope query each, the preview stores an
  edit in two store updates, autosave no longer compares the saved text on
  every update, and the Problems list and the C++ panel stay idle while
  hidden and catch up when shown. A hidden panel still starts afresh when
  another project is opened or the project is closed, so the C++ view keeps
  no selection or scroll position of the previous project.
- The security specification (08 §8.8) notes that WebKitGTK's remote Web
  Inspector (`WEBKIT_INSPECTOR_SERVER`, `WEBKIT_INSPECTOR_HTTP_SERVER`) can
  inspect release builds, and what it exposes while it is on.
- The window fits the screen: Tauri's `preventOverflow`, plus a fallback
  where the system names no primary monitor (Wayland) that fits the window
  to the monitor it opens on, less room for panels and the title bar (GTK
  knows no work area there), and logs which monitor was used.
- The C++ and bottom panels start at a share of the window (30 % of its
  width, 280–480 px; 28 % of its height, 120–280 px), follow the window until
  resized, and never take more than 65 % of the editor's area (the window
  less the toolbar, any banner and the status bar). The C++ panel is hidden
  when the window is too small for it and shown again when it fits.
- `Ctrl` + mouse wheel and a pinch now zoom the canvas around the pointer;
  the plain wheel still scrolls. Blockly's own zoom buttons are gone,
  including the reset that went back to 90 %.
- When the canvas goes past a project limit (blocks nested too deeply,
  `B2C-E0104`; over 32 MiB, `B2C-E0101`; over 100,000 blocks, `B2C-E0121`),
  the banner and Save name the limit and how to get back within it, instead
  of calling it a bug in Blocks2Cpp.
- _Clear_ empties the console completely once the program has ended; while a
  program runs, the line it waits at stays.
- The console refits at most once a frame while a panel or the window is
  resized, and rewraps long lines once the width has settled.
- The current directory, and a project folder that is the home folder or a
  root, are now excluded from compiler discovery as that folder only. This
  applies to a rescan, the check before a build (`B2C-T1002`) and _Choose g++
  manually…_. The desktop app now works in an empty owner-only folder,
  `<cache>/cwd`; the CLI keeps the caller's folder. The project dialogs start
  in the project's own folder, or in the documents or home folder, never in
  the app's working folder.
- The A/B benchmark job runs each benchmark on its own. A benchmark the base
  build cannot run (its end-to-end hook lacks a method the benchmark calls)
  is skipped with a notice, and a benchmark that fails is left out of the
  comparison; the job summary lists both. Only a gated regression or a
  benchmark that failed on the head fails the job, and a run that left a
  benchmark out says it is not conclusive for it. Before, the first failure
  stopped every benchmark of the run.
- `apps/desktop/e2e/bench/accepted.json` (format version 2) gives each entry
  a kind: `redefined` restarts a metric's nightly history only; `accepted`,
  an intended slowdown, also passes an A/B run. A redefinition no longer
  waives an A/B difference, because both builds are measured with the head's
  benchmarks. The two W2-1 entries for `webview.edit-1000.p95` are
  redefinitions.
- End-to-end harness: the window gets 60 s to start, as long as a page load
  may take, and a timeout says how far the start got (the page never loaded,
  it loaded too late, or the app never became ready). End-to-end builds keep
  the web view's data under the test's `B2C_E2E_ROOT`, or in the folder
  `B2C_E2E_WEBVIEW_DATA` names, instead of the app's local data folder that
  every launch shared: each launch of a flow test gets a folder of its own,
  and the cold-start benchmark's starts share one, as a person's do.

- A project or clipboard payload from a newer block catalog reports unknown
  fields, inputs, settings and options as `B2C-E0602` (made by a newer
  version) and keeps the block unchanged. Before, they were `B2C-E0605`.
- The IPC responses of `project_open_dialog`, `project_open_recent`,
  `project_reload` and `recovery_restore` gain `blocksUpgraded`.
- Unknown or damaged blocks stay errors wherever they are, including blocks
  left on the canvas (a Raw C++ block, a block from a missing pack, a newer
  block version).
- `b2c check` and builds run the analyser even when the block catalog
  reports errors, so the command line lists the same problems as the app's
  Problems panel. `b2c check` exits with 0 when there are only warnings.
  Each problem is listed once, in the app too: the analyser no longer repeats
  as `B2C-E0430` a field or input the catalog rejected (a dropdown value that
  is not an option was two errors), nor reports the choices and inputs of a
  block kept unchanged because a newer version made it (`B2C-E0602`) or it
  is too old to upgrade (`B2C-E0603`), whose advice to choose the setting
  again would have replaced the newer value.
- Catalog messages name blocks by their label (This "print" block …)
  instead of their catalog ID ("io.print"), and Problems' block paths and
  the canvas's keyboard announcements name the program block _when program
  starts_ instead of _main_.
- The compiler core's `symbols_in_scope` export and `CoreWasm.symbolsInScope`
  answer `null` (was `[]`) for a block the last analysis has no position for;
  new `Analysis::scope_at`, `scope_after` and `parked_scope`. The editor's
  scope resolver keeps its fallback only for blocks the latest analysis has
  not seen; an empty answer is final, and a block's menus ask the core once.

- The Variables category no longer follows the selected block. It offers
  _Make a variable_ (which now asks for a name and then a type: `int`,
  `double`, `bool`, `char` or `string`), the _create_ block, one _set_ block,
  one _change_ block while a number or character variable exists, and a getter
  for every variable of the module, grouped by the function that declares it
  (at most 50).
- A block taken from the toolbox, with the pointer or the keyboard, gets a
  free default name where it is put. A _set_ or _change_ block whose variable
  is not visible there takes the first fitting variable that is, as part of
  the same drop (one Undo removes it). A getter keeps its variable and shows
  `B2C-E0203` when that variable is out of scope.
- The Program category offers _when program starts_ only while the project
  has none, and otherwise says where the program starts.
- In the toolbox's blocks, ↓ and ↑ also stop on _Make a variable_, on the
  Variables category's group headings and on notes (a two-line note is read
  as one), so _Make a variable_ can be pressed from the keyboard; a getter is
  announced with its group ("block “i”, in greet").
- The toolbox's list of blocks is rebuilt only when what Program, Variables
  and Functions show changes (the module's variables and functions, whether
  the project has _when program starts_, the modules). It is never rebuilt
  during a drag, a field edit, an open menu or a category scroll, and it keeps
  the category in view and the selected category.
- A block still in the toolbox lists every variable of the module in its
  menu, grouped like the category. Choosing another variable in the toolbox's
  _set_, or another type in its _create_, changes the start value there as on
  the canvas, so the block taken out reads `set word to ""`, not
  `set word to 0` (which the analyser reported as `B2C-E0301`).
- The _change_ block has a menu: _by_, _down by_, _times_, _divided by_ and
  _mod_ (`+=`, `-=`, `*=`, `/=`, `%=`), so the Variables category offers one
  block for all of them. The block catalog is now 1.2.0. A project saved with
  it and opened in an older version reports the menu as made by a newer
  version (`B2C-E0602`) and keeps it.

- Placing blocks: a dragged block snaps from about 44 screen pixels at every
  zoom (48 to 96 canvas units; Blockly used 28 units at any zoom, about 25 px
  at 90 % and 14 px at 50 %). While a statement is dragged, its grey preview
  stays on a place until another place is 20 units nearer. A dragged value
  always goes to the nearest slot, so it can be dropped into the first slot
  of a block that sits in a slot. Bumping is unchanged.
- Starting a drag no longer reads the whole canvas when the live preview is
  up to date; it reuses the project's document (about 10 ms saved per drag
  start at 1,000 blocks).
- Each edit loads and analyses the document once in the WebAssembly core
  (`update()`), and the canonical text is made only when a save, build,
  autosave, open or paste needs it. At 1,000 blocks the preview task's p95
  falls from 113 to 92 ms in the app's benchmark (a local A/B), with about
  17 ms less core work per edit (`canonical` and `analyse` took 58–62 ms,
  `update` takes 42–44 ms; 36 ms less in the package's Vitest measurement).
- The core's `preview()` export is now `update()`, which also returns
  `textHash` (SHA-256 of the canonical text); `canonical()` returns
  `textHash` too. Unsaved changes are detected by that hash, so moving a
  block still counts as a change.
- The User Timing spans `b2c:preview:canonical` and `b2c:preview:analyse` are
  replaced by `b2c:preview:update`.
- The README says what works today (M2) apart from the goals for 1.0, with a
  short status note. CONTRIBUTING sets up through `BUILDING.md`, lists every
  check CI runs in one place, names the 14 crates and says where things are
  documented. `BUILDING.md` builds with `pnpm app:build` and has the tools of
  the end-to-end tests.
- CI checks that the tool versions and Debian packages in `BUILDING.md` match
  the repository's pins (`Cargo.lock`, `rust-toolchain.toml`, `package.json`,
  `.node-version`, the workflows), and that no other document repeats them.
  The declared minimum Node.js is now 22.14 (`package.json` engines), as
  `BUILDING.md` already required.
- New project files get the permissions any new file of the user gets (the
  umask applies, usually `0644`) instead of `0600`; a saved project keeps its
  permissions, and machine-local files and recovery snapshots stay
  owner-only (decision Q13; 05 §5.10, 08 §8.6).
- Problems' _Learn more_ is marked ↗, and its tooltip and accessible
  description say it opens the diagnostics reference in your web browser.
- The status bar's compiler has its full path in its tooltip and accessible
  name, with hidden or reordering characters shown as placeholders (08
  §8.5).

### Removed

- The separate update block (`[score ▾] [*= ▾] (2)`, `var.update`) is
  retired. Projects that use it open with the _change_ block in its place,
  with the same operator, value and C++. The project is marked changed, and
  saving keeps the original as `.b2c.bak`. `b2c migrate` rewrites such files.

### Fixed

- On Linux, a project whose name, folder or library names hold a `%` (a
  project named `%s%s%n`, say) could crash the app, or worse, as its trust
  dialog opened: the dialog library (`rfd` 0.16.0, through the dialog plugin)
  handed the text to GTK as a printf format. The _Replace file?_ question did
  the same with a file name. Every `%` is now doubled for GTK, so it shows as
  typed, until `rfd` 0.16.1, which fixes this, is old enough to use.
- Every Release build with Ubuntu's GCC 14 or newer showed a warning
  (`-Whardened`: `_FORTIFY_SOURCE` is not enabled by `-fhardened`), since
  Ubuntu's compiler defines `_FORTIFY_SOURCE` itself. The compiler check now
  counts `-fhardened` as available only when an optimised build with it says
  nothing; elsewhere Release builds use the separate hardening options, as
  with GCC 13. Remembered compilers are checked again once.
- A save asked for while the previous save was still finishing was dropped
  without a word: after the first _Save as…_ of a new project, on a slow
  disk, an edit saved at once with Ctrl+S or ≡ _Save_ stayed unsaved. Such a
  save now runs once the one before it has saved, and writes what the editor
  shows then; several asked for meanwhile make one save. After a cancelled
  _Save as…_ or a failed save it does nothing more, so the dialog or the
  message does not come twice, and it never saves a project opened after it
  was asked for. A held key still saves once.
- Choosing another module, or a problem in another module (also the first
  error that F5 or Ctrl+B selects), while a block is dragged no longer
  switches modules in the middle of the drag, which lost the dragged block
  and could lose the statements a drag preview held; the switch is made once
  the block is dropped.
- While the canvas cannot be read back as a project (the "Your latest change
  was not checked" banner), the editor stays on its module. Switching cleared
  the undo history the banner's _Undo last change_ needs, and kept the module
  in a form that no save, build or recovery snapshot could write.
- A save during which an edit was made and undone no longer marks the undone
  document as saved: the project stays unsaved, since the file holds the
  edit.
- Dropping a block on the toolbox or the trash can while a place was
  previewed no longer deletes the blocks below that place. Only the dragged
  blocks are deleted, no place is previewed while the pointer is there, and
  the dragged blocks are shown faded (over the toolbox, the toolbox is tinted
  too).
- A block dropped anywhere on the toolbox is deleted again. After the
  toolbox's list of blocks was rebuilt (opening a project, making a variable),
  a drop on the categories or the open list deleted nothing, showed no faded
  blocks and still previewed places, until the window or a dock was resized;
  only the trash can and the window's left edge deleted. The pointer decides,
  at any height: a drop with the pointer left of the toolbox's right edge
  deletes, also over the toolbar or the bottom dock, and while the pointer is
  there the whole toolbox, its categories included, is tinted. Over the trash
  can the dragged blocks are faded and the trash can opens its lid, but the
  toolbox is not tinted. The places where a drop deletes are recorded in the
  animation frame after the toolbox's list of blocks changes or moves (or when
  a drag starts before that frame), so rebuilding the list after an edit no
  longer makes the page lay itself out an extra time, and a drag no longer
  measures them again when nothing moved.
- A negative number typed into a block (`change by -1`) was rejected as "not
  a number" (`B2C-E0310`). A typed `-` or `+` is now a sign, and
  `-2147483648` is generated as `-2147483647 - 1`.
- _Keep asking until valid_ reads the whole line: `3.5` or `12abc` for a
  whole number asks again, a text answer asks again after an empty line, and
  a character answer must be one character. A _simple_ ask for a number,
  character or true/false value drops the rest of its line, and a failed
  one is cleared, so a later _keep asking_ ask reads the next answer.
- `join` writes decimal numbers as `print` does (`2.5`, not `2.500000`),
  through the new support helper `b2c::to_text`.
- A category chosen in the toolbox stays selected at every zoom level.
- A block left loose on the canvas, a block inside a disabled statement and
  a block the live analysis has not seen yet no longer empty the block's own
  variable menu; it lists what is visible at the end of `main` (the module's
  functions without `main`) or at the nearest analysed statement before or
  around them.
- The toolbox shows a renamed variable's or function's new name at once.
- The `≡` main menu is drawn over the block toolbox and can be clicked across
  its full width. Opening a dialog (the console's link dialog too) or a
  full-window page such as Settings first closes the block editor's
  drop-downs, field editors, context menus and tooltips, which were drawn
  over it.
- The Debug/Release drop-down is readable in the light GTK theme: the app
  draws it with the toolbar's colours and its own chevron, and the toolbar
  is 40 px tall again.
- The toolchain link in the status bar no longer shows a button edge; a
  page's heading shows its focus ring only after keyboard use; a long
  project name is cut off before the unsaved-changes marker, which stays in
  view.
- A second run no longer writes over the first run's output in the console.
  Each run starts below a "── New run ──" line, with the terminal's modes
  reset and the cursor where the last run left it. On Windows, where each
  program's pseudoconsole repaints the screen from the top, the console
  first scrolls what it shows into the scrollback.
- The console's terminal is fitted inside its box, so the bottom dock no
  longer scrolls by a few pixels. A console header taller than a small bottom
  dock (an open notice) scrolls by itself, so Stop, Run again and Clear stay
  reachable.
- _Copied_ (C++ panel, setup page, link dialog) disappears after 3 s and is
  announced on every copy; _Could not copy_ stays until the next attempt.
  The C++ panel's confirmation no longer moves the code.
- A project opened from the start page, or a template, shows its blocks
  centred and clear of the toolbox instead of against its edge.
- A plain Save now updates the project's name in the recent-projects list and
  moves it to the front. Before, only Save as did, so a renamed project kept
  its old name on the start page.
- A new run scrolls the console to its output when you had scrolled up to
  read an earlier run; earlier output stays above the _New run_ line.
- The mouse wheel scrolls the console while a program prints, also at
  display scales such as 125 %, 150 % and 200 %.
- Started from `/`, the app found no g++, and started from the home folder
  (or with a project saved directly in it) it skipped `~/.local/bin`.
- A compiler whose check timed out (for example on the first start after a
  restore) was reported as a broken installation (`B2C-T1006`) and stayed
  that way, even across Rescan. It is now checked once more with 30 s per
  test, and the page says "Checking g++ again…" while that runs. If it still
  times out, it is reported as `B2C-T1023`, with a hint to rescan, and
  checked again the next time it is needed. A test of an optional capability
  (sanitizers, hardening, static linking) that times out only turns that
  capability off. Toolchain records saved by older versions are checked again
  once, without a note that the compiler changed.
- `tools/bench-compare.py` writes UTF-8 on Windows too. Before, "§9.2" in the
  A/B summary came out as a code-page byte in the job log.
- The scope query answers parked blocks (loose, in a loose stack, or nested
  in one) with what is visible at the end of `main`'s body (the module's
  functions without `main`), and blocks inside a disabled statement like that
  statement, so a loose block's variable menu lists `main`'s variables
  straight from the analysis.
- Pasting on the canvas, or into or directly after a parked block, binds
  references to the variables visible at the end of `main`; pasting into or
  after a block inside a disabled statement binds them as at that statement.
- A block just dropped on the canvas, before the next preview, now sees the
  end of the _when program starts_ that runs, as the compiler does, not of a
  disabled one above it; _Make a variable_ with nothing selected inserts there
  too. Without _when program starts_, a new loose block sees the module's
  functions.
- Saving in the app no longer lowers the block catalog a file names
  (`generator.catalog`). A project from a newer catalog saved in an older
  app named the older catalog afterwards, so its newer blocks turned from
  `B2C-E0602` ("Update Blocks2Cpp") into `B2C-E0605` ("Remove it").
- `b2c migrate` names this version and its block catalog in `generator` when
  it upgraded blocks, as a save in the app does (never an older catalog than
  the file named).

- The compiler core keeps answering scope queries for the document on screen
  after an edit whose document fails to load.
- The C++ tab shows the code of the module on the canvas and follows the
  module switcher; a file you pick in the tab stays until you show another
  module.
- After a module switch, a reload or closing the project, the C++ tab no
  longer highlights the code of a block that is gone or no longer selected.
- Build output follows each new build from its start again, even after you
  scrolled up in the last one.
- Run is no longer stuck at "starting" when the backend answers without a
  valid run ID; the console header goes back and a dialog says the program
  could not start.
- The weekly mutation job no longer runs its runners out of memory. Each test
  process runs under a 2 GiB resident-memory cap
  (`tools/memory-capped-runner.py`, the job's cargo target runner), so a
  mutant that makes a loop endless and allocating is caught within seconds.
  A mutant's tests now time out after 1.5 times the unmutated tests' time
  instead of 5 times.
- The weekly `cargo-geiger` job keeps each crate's report: cargo-geiger exits
  with 1 whenever it warns, and the job dropped every report as a failure.
- The end-to-end harness gives `tauri-driver` and the native driver two
  different ports: it could hand out the same free port twice, and the test
  then failed before the app started.
- _Save as…_ no longer suggests a Windows device name written with a
  superscript digit (`COM¹.b2c`, `LPT².b2c`): the suggestion uses the device
  rule of the project file's path checks.
- Contributor docs: CONTRIBUTING's cargo-deny line runs as written (its
  options come before `check`), and its Rust checks set
  `RUSTFLAGS='-D warnings'` as CI does; BUILDING.md's Develop section tells
  Windows users to set `B2C_SKIP_WASM_OPT` first, and says which pinned
  versions the build checks. The toolchain protocol's Rescan step works on
  Debian (`apt remove`, since the app always searches `/usr/bin` and reads
  the path only at start), and its TDM-GCC cell expects the compiler to be
  flagged too old (GCC 10.3).
- `tools/check-build-docs.py` also reports a binaryen release right after the
  tool's name and a Node.js minimum written in lower case (`engines.node`);
  `tools/check-moved-text.py` reports a missing input file instead of
  crashing, and the dropped list of the desktop README's split is committed.
- The toolbox's first category (_Program_) is selected when the editor
  opens, every time. Drawing the toolbox's categories again kept the old
  selected row as the selection, so the new rows were drawn unselected and,
  depending on timing, no category was highlighted until the flyout was
  scrolled or a category chosen.
- The end-to-end flow `selection` reads the toolbox's getter IDs in one call
  of the test hook (its new `flyoutBlockIds`). It read them one call at a
  time, and the toolbox's delayed rebuild after a new variable could run
  between two calls: the new variable's getter came from the new flyout
  while the others still had their old IDs, and the test failed although
  the app was right.
