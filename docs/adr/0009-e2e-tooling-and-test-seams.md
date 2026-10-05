# ADR-0009: End-to-end tooling under the pnpm policy, and the test seams

* Status: Accepted (confirmed by the owner on 2026-10-05)
* Date: 2026-10-05

## Context

M2's exit criterion is that a first-time user builds and runs the guessing
game, and end-to-end tests must cover that flow on Windows and Linux
([10 §10.1](../spec/10-roadmap.md#101-milestones)). The spec planned
WebdriverIO with `tauri-driver`
([09 §9.2](../spec/09-quality-and-delivery.md#92-testing-strategy)).

Every npm package must pass the workspace's supply-chain policy
([08 §8.9](../spec/08-security.md#89-supply-chain)): at least 7 days old,
`trustPolicy: no-downgrade` (no version may carry weaker publishing evidence
than earlier versions), no exotic transitive sources, and no install scripts
unless allowlisted. A trial install during M2 planning showed that
WebdriverIO and its `@wdio/*` packages fail `no-downgrade` through their
dependency chain, and so does `eslint-plugin-react` (through `semver@6.3.1`).

End-to-end tests also need control over things a real user decides through
native dialogs (which file to open, whether to trust a project) and over the
machine (which compilers exist, where settings live). None of that may be
reachable in a release build.

## Options considered

For the driver library:

1. **WebdriverIO.** The planned choice, with a Tauri example. It fails the
   policy, and exempting it would weaken the policy for a large dependency
   tree.
2. **selenium-webdriver (chosen).** The Selenium project's own JavaScript
   client for W3C WebDriver, with a modest dependency tree that installs under
   the policy. It has no test runner of its own.
3. **A hand-written WebDriver HTTP client.** No dependency, but more code to
   maintain for no security gain.

For the runner: Vitest, which the frontend unit tests already use, run with a
separate config (one worker, forked processes, long timeouts).

For the dialogs and the machine: mocking the IPC layer would not test the real
backend, and driving native dialogs through OS automation is fragile and
differs per platform. A compile-time test seam in the desktop crate is
chosen instead.

## Decision

* The E2E suite uses **selenium-webdriver** with
  **Vitest** as its runner, driving **`tauri-driver`** (installed with
  `cargo install --locked` at a pinned version). Linux uses `WebKitWebDriver`
  (the distribution's `webkit2gtk-driver`) under `xvfb`. Windows uses
  `msedgedriver` matching the installed WebView2 version, downloaded from
  Microsoft's official endpoint and version-checked. WebdriverIO is
  reconsidered when its dependency chain passes the policy again.
* **`eslint-plugin-react` is not used**, for the same policy
  reason. Its one security rule, `react/no-danger`, is replaced by a
  `no-restricted-syntax` rule on `dangerouslySetInnerHTML`; `react-hooks`,
  `jsx-a11y`, `no-unsanitized` and the `no-restricted-*` bans stay
  ([09 §9.1](../spec/09-quality-and-delivery.md#91-engineering-standards)).
* **The `e2e-hooks` Cargo feature** of `blocks2cpp-desktop`, off by default.
  A `compile_error!` stops it in any build without debug assertions, so it
  cannot reach a release. With it, the app reads:
  * `B2C_E2E_ROOT`: all machine-local folders go under this root
    (`b2c_store::Dirs::under_root`), so each test starts from a fresh profile;
  * `B2C_E2E_DIALOGS`: a JSON script that answers the open, save-as,
    pick-compiler and trust dialogs in order, and cancels once exhausted;
  * `B2C_E2E_TOOLCHAIN_DIRS`: the only folders toolchain discovery searches
    (an empty value means no compilers).
* **A frontend hook** (`window.__B2C_E2E__`: readiness, inserting blocks,
  reading the document and the console, selecting a block, and the Trusted
  Types counts of [08 §8.8](../spec/08-security.md#88-webview-and-ipc-hardening))
  is installed only when Vite builds in mode `e2e`, and is tree-shaken out of
  every other build.
* **CI asserts** that the release binary and bundle contain neither the
  strings `B2C_E2E_DIALOGS` nor `__B2C_E2E__`.
* The tests read the console through xterm.js's DOM renderer (`.xterm-rows`)
  and type through `.xterm-helper-textarea`. Stable `data-testid` attributes
  mark the toolbar, status bar, console header, Problems and banners.

## Consequences

* The E2E tests exercise the real backend, real IPC (with the isolation hook,
  the CSP and `freezePrototype` active) and the real g++, with only the
  native dialogs and the machine's folders scripted.
* Drag and drop on Blockly's SVG canvas goes through Selenium's pointer
  actions in small steps; the rest of a program can be inserted through the
  hook, so a test does not depend on dragging every block.
* Driver versions must track the webviews: `msedgedriver` follows WebView2,
  which updates itself, so the Windows job resolves the version on each run.
* Without `eslint-plugin-react`, rules such as `react/jsx-key` are not
  enforced; TypeScript's strict checks and React's own warnings in tests
  cover the most common mistakes.
* 09 §9.1 and §9.2 describe this tooling.
