# End-to-end tests

The desktop app driven like a person drives it: real windows, real IPC through the isolation hook,
the Content Security Policy and `freezePrototype` active, the real backend and the real g++
([09 §9.2](../../../docs/spec/09-quality-and-delivery.md#92-testing-strategy),
[ADR-0009](../../../docs/adr/0009-e2e-tooling-and-test-seams.md)). Only the native dialogs and the
machine's folders are scripted, through the app's `e2e-hooks` feature.

- **Driver:** [selenium-webdriver](https://www.npmjs.com/package/selenium-webdriver) (WebdriverIO
  fails the pnpm supply-chain policy) talks to `tauri-driver`, which starts the platform's own
  driver: `WebKitWebDriver` on Linux, `msedgedriver` for WebView2 on Windows.
- **Runner:** Vitest, with its own configuration ([`vitest.e2e.config.ts`](vitest.e2e.config.ts)):
  one app at a time in a single forked worker, five minutes per test.
- **Specs:** `specs/**/*.e2e.ts`. [`specs/exit/guessing-game.e2e.ts`](specs/exit/guessing-game.e2e.ts)
  is the M2 exit criterion: the guessing game assembled from the Empty template (real drags for the
  program, a variable, a print and the loop; the test hook for the rest), F5, a binary search on the
  answers until _Correct!_, _Finished (exit code 0)_, then _Run again_ and _Stop_. A second test
  checks that more than 8 KiB of output in one batch (Tauri's channel fetch, through the isolation
  hook) reaches the console. [`specs/flows/`](specs/flows/README.md) are the breadth flows of 09
  §9.2 (Restricted Mode and trust, save and reload, crash recovery, Stop, exit decoding, closing
  the app, the output flood, the setup page, external changes, the clipboard, settings, the
  variable menus, the connection checker, code-to-block selection, dragging from every category,
  build and run with input); their launcher (`specs/flows/lib/launch.ts`) keeps one profile across
  launches. [`specs/security/`](specs/security/README.md) are the security tests of 08 §8.13,
  which run nightly (`nightly.yml`, `e2e-security`), not in the PR job.
- **Benchmarks and visual diff:** [`bench/`](bench/README.md) (cold start, the preview and edits
  at 1,000 blocks, selecting blocks at 1,000 blocks, the guessing game's selections and edits,
  dragging in a 5,000-block workspace; nightly, with the 10% gate, and in the A/B job on labelled
  pull requests) and
  [`visual/`](visual/README.md) (the canvas screenshot against per-system baselines). The PR job
  runs `pnpm --filter @blocks2cpp/desktop run e2e --exclude 'specs/security/**'` and then the visual
  diff; `support/global.ts` clears the Trusted Types report, `visual/global.ts` only rewrites its
  summary.

## Running them locally (Linux)

You need `WebKitWebDriver`, `xvfb`, g++, and `tauri-driver` at the pinned version;
[BUILDING.md](../../../BUILDING.md#end-to-end-tests) has the commands that install them.

Build the WebAssembly core, then the app under test. CI uses the Tauri CLI:

```sh
B2C_SKIP_WASM_OPT=1 pnpm --filter @blocks2cpp/b2c-core-wasm run build
pnpm --filter @blocks2cpp/desktop tauri build --debug --no-bundle --features e2e-hooks \
  --config '{"build":{"beforeBuildCommand":"pnpm build:e2e"}}'
```

Without the Tauri CLI, the same build is the e2e frontend and a debug build of the crate with the
embedded frontend (`tauri/custom-protocol`, which is what the CLI adds):

```sh
pnpm --filter @blocks2cpp/desktop run build:e2e
cargo build -p blocks2cpp-desktop --features e2e-hooks,tauri/custom-protocol
```

Then run the tests under a virtual display:

```sh
WEBKIT_DISABLE_DMABUF_RENDERER=1 xvfb-run -a pnpm --filter @blocks2cpp/desktop run e2e
```

`app-close` (in `specs/flows/`) also needs `python3` and libX11, both on Ubuntu runners, to send
the window its close request.

Only the harness's own unit tests, and those of the flow helpers (`specs/**/*.test.ts`); no app
needed:

```sh
pnpm --filter @blocks2cpp/desktop exec vitest run --config e2e/vitest.e2e.config.ts --project support
```

On Windows the tests run in CI (`windows-2025`): [`scripts/msedgedriver.ps1`](scripts/msedgedriver.ps1)
(PowerShell 7.3 or later) reads the installed WebView2 runtime's version, downloads the matching
`msedgedriver` from Microsoft, takes only `msedgedriver.exe` out of the archive (into a folder of its
own), checks that its signature is valid and Microsoft's (the exact signer and a Microsoft issuer)
and that it reports exactly that version, and sets `B2C_E2E_NATIVE_DRIVER`.

## Settings

| Variable                    | What it is                                                                                                                                                                                                                | Default                                                                            |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `B2C_E2E_APP`               | The app under test (absolute)                                                                                                                                                                                             | `target/debug/blocks2cpp-desktop[.exe]` under `CARGO_TARGET_DIR` or the repository |
| `B2C_E2E_TAURI_DRIVER`      | `tauri-driver`                                                                                                                                                                                                            | the one on `PATH`                                                                  |
| `B2C_E2E_NATIVE_DRIVER`     | `tauri-driver --native-driver` (absolute)                                                                                                                                                                                 | none: `tauri-driver` looks on `PATH`                                               |
| `B2C_E2E_TOOLCHAIN_DIRS`    | Where the app looks for g++ (the app's own variable)                                                                                                                                                                      | `/usr/bin` on Linux; required elsewhere                                            |
| `B2C_E2E_WINDOW_SIZE`       | The app window's size, `WIDTHxHEIGHT` in CSS pixels, for every launch (the app's own variable; a test's `windowSize` launch option wins)                                                                                  | none: the app's own size (1280 × 800, smaller on a smaller screen)                 |
| `B2C_E2E_ARTIFACTS`         | Screenshots, logs and the Trusted Types report (absolute)                                                                                                                                                                 | `blocks2cpp-e2e-artifacts` in the system's temporary folder                        |
| `B2C_E2E_NATIVE_DRIVER_LOG` | A log the native driver appends to for the whole run (absolute); a failed test keeps the part written while it ran, as `native-driver.log`                                                                                | none                                                                               |
| `B2C_E2E_WRAP_SWEEP`        | Any value turns `placement-wrap`'s zoom test into a measurement: drops on a 20-unit grid over ±60 units at zooms 0.5, 0.9 and 1.5, written to `wrap-sweep.json` in the test's artifact folder (about 13 minutes on Linux) | none: the test drops exactly and 10 units off                                      |

Each test gets its own temporary profile (`B2C_E2E_ROOT`), a dialog script (`B2C_E2E_DIALOGS`; by
default every native dialog is cancelled) and its own `tauri-driver` on free ports. A failed test
leaves a screenshot, the page, the console transcript, the project and the app's and the driver's
logs in a folder named after it. When no session starts (a missing or mismatched native driver),
the test fails as soon as `tauri-driver` exits, and its message quotes the driver's output and names
the saved copy of its log there.

When a test ends, the WebDriver session is ended (which closes the app) with a time limit, then
`tauri-driver` is stopped. On Linux `tauri-driver` does not close the app itself, so whatever the test
started that is still running then (the app, WebKit's processes, a compiler or program the app
started) is killed: the driver's descendants and every process with the test's `B2C_E2E_ROOT`
([`support/processes.ts`](support/processes.ts)). On Windows `tauri-driver`'s job object does this. After every test the Content Security Policy violations it saw
are appended to `trusted-types.jsonl`, and `trusted-types.md` summarises them when the run ends
(the CI job adds it to its summary): the Trusted Types trial of
[08 §8.8](../../../docs/spec/08-security.md#88-webview-and-ipc-hardening), which never fails a test.

From the window's readiness until the test ends, the long-task recorder
([`support/longtasks.ts`](support/longtasks.ts)) notes the page's main-thread tasks of 50 ms or more
(N12's limit), by a timer that sets itself again as soon as it runs: a gap between its beats is a
task, at most one beat (a few milliseconds) longer. For each launch of the app, the longest task,
the 10 longest with the `b2c:` User Timing marks and measures that overlap them, and the count go to
`long-tasks-<launch>.json` in the test's folder, a line on standard output and `long-tasks.jsonl`,
which `long-tasks.md` summarises when the run ends (the CI jobs add it to their summary). It is a
report ([09 §9.2](../../../docs/spec/09-quality-and-delivery.md#92-testing-strategy); N12 is met in
M5) and never fails a test; a page replaced during the test loses its record, which the report
says. The flows' launcher records each launch until it ends. The option `recordLongTasks: false` of
`launchApp` turns it off, as the benchmarks do. The app measures its preview pipeline (and each
phase of it), each rebuild of the toolbox's flyout (a task of its own, 100 ms after the change that
asked for it), the selection handler and the code panel's update as User Timing spans
([`src/lib/timing.ts`](../src/lib/timing.ts): `b2c:preview`, `b2c:toolbox-refresh`,
`b2c:select`, `b2c:code-panel`) and keeps only the latest entry of each name in the browser's
buffer, so the recorder observes the entries as they come (`PerformanceObserver`) to name the tasks
they overlap. The hook's `timings()` gives the latest duration of each span.

## Writing a test

`launchApp(context)` ([`support/app.ts`](support/app.ts)) starts the app for a test and closes it
when the test ends. Its option `windowSize` (also `launch({ windowSize })` of the flows' launcher)
sets the window's size through the app's `B2C_E2E_WINDOW_SIZE` (end-to-end builds only), so a test
lays the editor out alike on every system; WebDriver's `setRect` may not resize WebView2's web view.
The app still keeps the window on the screen. The helpers:

- [`support/ui.ts`](support/ui.ts): elements by `data-testid`, text also in hidden tabs, toolbox
  categories (`openCategory` returns once the flyout's scroll animation has ended, however many
  frames it takes), Blockly's dropdown menus and text editors, pointer clicks.
- [`support/editor.ts`](support/editor.ts) and [`support/canvas.ts`](support/canvas.ts): the open
  project, dragging blocks from the toolbox onto connections and around the canvas, panning the
  canvas, fields, Problems and the run gate.
- [`support/console.ts`](support/console.ts): the console header, the terminal's rows on screen
  (`.xterm-rows`) and typing into it (through its `.xterm-helper-textarea`).
- [`support/bdm.ts`](support/bdm.ts): reading the project and building block nodes to insert.
- [`support/hook.ts`](support/hook.ts): the app's test hook, `window.__B2C_E2E__`.

The hook exists only in the frontend built with `vite build --mode e2e`
([`src/e2e/`](../src/e2e/index.ts); its contract is [`src/e2e/contract.ts`](../src/e2e/contract.ts)).
It reports readiness, inserts blocks as the clipboard's paste does (one undo step; a refused
insertion changes nothing and leaves no undo step), reads the canonical document, the console
transcript (the terminal only has the rows on screen; the transcript also has the console's
separators and its "… N lines skipped" markers), what the terminal holds (its active buffer with
the scrollback, as the person sees it), the C++ of the code panel and the Trusted Types
counts, selects a block, and locates blocks, fields, connections and grab points on screen so that
the test can click and drag with real pointer input; it also scrolls the canvas without changing
the selection (`scrollCanvasBy`, for the benchmarks' clicks). CI checks that a release
build contains neither the hook nor the backend's `B2C_E2E_*` variables.

Find elements by their `data-testid`; the stable ones are listed under [Test IDs](#test-ids). Wait
for state with `waitFor` ([`support/wait.ts`](support/wait.ts)) rather than fixed delays, and keep
each test independent: it starts from a fresh profile.

## Test IDs

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
`build-output-line`, `keyboard-announcer`, `app-dialog`, `feature-page-back` (the settings and
toolchain pages' _Back_ button), and those pages' `settings-*` and `toolchain-*` IDs. The canvas
controls are `canvas-controls`, `canvas-zoom-out`, `canvas-zoom-in`, `canvas-zoom-reset` and
`canvas-show-all`.
