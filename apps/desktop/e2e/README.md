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
  hook) reaches the console.

## Running them locally (Linux)

You need `WebKitWebDriver` (Ubuntu and Debian: `sudo apt-get install webkit2gtk-driver`), `xvfb`,
g++, and `tauri-driver` at the pinned version (the `e2e` job in
[`desktop.yml`](../../../.github/workflows/desktop.yml) names it):

```sh
cargo install tauri-driver --locked --version 2.1.0
```

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

Only the harness's own unit tests (no app needed):

```sh
pnpm --filter @blocks2cpp/desktop exec vitest run --config e2e/vitest.e2e.config.ts --project support
```

On Windows the tests run in CI (`windows-2025`): [`scripts/msedgedriver.ps1`](scripts/msedgedriver.ps1)
reads the installed WebView2 runtime's version, downloads the matching `msedgedriver` from
Microsoft, checks its signature and version, and sets `B2C_E2E_NATIVE_DRIVER`.

## Settings

| Variable                 | What it is                                                | Default                                                                            |
| ------------------------ | --------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `B2C_E2E_APP`            | The app under test (absolute)                             | `target/debug/blocks2cpp-desktop[.exe]` under `CARGO_TARGET_DIR` or the repository |
| `B2C_E2E_TAURI_DRIVER`   | `tauri-driver`                                            | the one on `PATH`                                                                  |
| `B2C_E2E_NATIVE_DRIVER`  | `tauri-driver --native-driver` (absolute)                 | none: `tauri-driver` looks on `PATH`                                               |
| `B2C_E2E_TOOLCHAIN_DIRS` | Where the app looks for g++ (the app's own variable)      | `/usr/bin` on Linux; required elsewhere                                            |
| `B2C_E2E_ARTIFACTS`      | Screenshots, logs and the Trusted Types report (absolute) | `blocks2cpp-e2e-artifacts` in the system's temporary folder                        |

Each test gets its own temporary profile (`B2C_E2E_ROOT`), a dialog script (`B2C_E2E_DIALOGS`; by
default every native dialog is cancelled) and its own `tauri-driver` on free ports. A failed test
leaves a screenshot, the page, the console transcript, the project and the app's and the driver's
logs in a folder named after it. After every test the Content Security Policy violations it saw
are appended to `trusted-types.jsonl`, and `trusted-types.md` summarises them when the run ends
(the CI job adds it to its summary): the Trusted Types trial of
[08 §8.8](../../../docs/spec/08-security.md#88-webview-and-ipc-hardening), which never fails a test.

## Writing a test

`launchApp(context)` ([`support/app.ts`](support/app.ts)) starts the app for a test and closes it
when the test ends. The helpers:

- [`support/ui.ts`](support/ui.ts): elements by `data-testid`, text also in hidden tabs, toolbox
  categories, Blockly's dropdown menus and text editors, pointer clicks.
- [`support/editor.ts`](support/editor.ts) and [`support/canvas.ts`](support/canvas.ts): the open
  project, dragging blocks from the toolbox onto connections and around the canvas, panning the
  canvas, fields, Problems and the run gate.
- [`support/console.ts`](support/console.ts): the console header, the terminal's rows on screen
  (`.xterm-rows`) and typing into it (through its `.xterm-helper-textarea`).
- [`support/bdm.ts`](support/bdm.ts): reading the project and building block nodes to insert.
- [`support/hook.ts`](support/hook.ts): the app's test hook, `window.__B2C_E2E__`.

The hook exists only in the frontend built with `vite build --mode e2e`
([`src/e2e/`](../src/e2e/index.ts); its contract is [`src/e2e/contract.ts`](../src/e2e/contract.ts)).
It reports readiness, inserts blocks as the clipboard's paste does, reads the canonical document,
the console transcript (the terminal only has the rows on screen), the C++ of the code panel and
the Trusted Types counts, selects a block, and locates blocks, fields, connections and grab points
on screen so that the test can click and drag with real pointer input. CI checks that a release
build contains neither the hook nor the backend's `B2C_E2E_*` variables.

Find elements by their `data-testid`; the stable ones are listed in the app's README. Wait for
state with `waitFor` ([`support/wait.ts`](support/wait.ts)) rather than fixed delays, and keep
each test independent: it starts from a fresh profile.
