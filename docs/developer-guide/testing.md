# Testing the desktop app

How the desktop app is tested, and where each kind of test lives. The testing strategy is
[09 §9.2](../spec/09-quality-and-delivery.md#92-testing-strategy); the commands CI runs, for every
part of the repository, are listed once in [CONTRIBUTING.md](../../CONTRIBUTING.md#checks). Paths
are relative to `apps/desktop/`.

- [Before the first test run](#before-the-first-test-run)
- [Unit and component tests](#unit-and-component-tests)
- [The isolation hook's tests](#the-isolation-hooks-tests)
- [The desktop crate's tests](#the-desktop-crates-tests)
- [The e2e-hooks feature](#the-e2e-hooks-feature)
- [End-to-end tests](#end-to-end-tests)

## Before the first test run

Set up your machine with [`BUILDING.md`](../../BUILDING.md) first. The editor imports the
WebAssembly core (`packages/b2c-core-wasm`), so build it once before the first `dev`, `build` or
test run, and again after changing the Rust crates it contains: `pnpm wasm:build`, with the tools
[`BUILDING.md`](../../BUILDING.md#linux-debian-and-ubuntu) installs. A production or end-to-end
build of the frontend stops when the core is older than its source, and the dev server and Vitest
warn.

## Unit and component tests

Tests sit next to the code they test (`*.test.ts`, `*.test.tsx`); `src/test/` holds the setup and
the shared helpers, such as `expectNoAxeViolations` for the accessibility check every panel and
dialog test runs. [`packages/README.md`](../../packages/README.md) describes the frontend packages
and the template they follow; each package has its own tests.

- `pnpm --filter @blocks2cpp/desktop run test` runs Vitest (happy-dom), headless, then
  `test:isolation`; `test:coverage` is the same with the 75% line gate
  ([09 §9.2](../spec/09-quality-and-delivery.md#92-testing-strategy)).
- happy-dom lays nothing out (every rectangle is 0 by 0), so a test that needs a place on the
  screen stubs it, and what depends on the real layout is checked end to end.
- To run some files only:
  `pnpm --filter @blocks2cpp/desktop exec vitest run src/editor/sync/drag.test.ts`.
- `B2C_REQUIRE_WASM=1` makes the tests that need the WebAssembly core fail, rather than skip,
  when it has not been built; CI sets it.

## The isolation hook's tests

`pnpm --filter @blocks2cpp/desktop run test:isolation` runs the isolation hook's validator tests
(`node --test`, `src-tauri/isolation-tests/`) against the allowlist generated from the IPC
contract ([security settings](desktop-security.md#security-settings)).

## The desktop crate's tests

`src-tauri/tests/` holds the Rust tests of the desktop crate: command consistency, security
settings, mock-runtime IPC, logging, end-to-end seams and Windows hardening (`hardening.rs`).
`cargo test --locked -p blocks2cpp-desktop` runs them (with `B2C_REQUIRE_GXX=1` they fail without
g++ instead of skipping), and again with `--features e2e-hooks`. The crate needs the webview
libraries, so it is not a default member of the workspace
([architecture](desktop-architecture.md#the-desktop-crate-in-the-cargo-workspace)).

## The e2e-hooks feature

`cargo build -p blocks2cpp-desktop --features e2e-hooks` (debug builds only: a `compile_error!`
refuses it otherwise) builds the app with its end-to-end seams, which read `B2C_E2E_ROOT` (every
machine-local folder, the web view's data included), `B2C_E2E_WEBVIEW_DATA` (another web view data
folder), `B2C_E2E_DIALOGS`, `B2C_E2E_TOOLCHAIN_DIRS` and `B2C_E2E_WINDOW_SIZE`
([ADR-0009](../adr/0009-e2e-tooling-and-test-seams.md)). `B2C_LOG=debug` makes the log more
detailed. `pnpm build:e2e` builds the frontend with the test hook `window.__B2C_E2E__`
(`src/e2e/`); no other build contains it, and CI checks that a release build contains neither
the hook nor the backend's `B2C_E2E_*` variables.

## End-to-end tests

End-to-end tests (selenium-webdriver, `tauri-driver`, Vitest) live in
[`e2e/`](../../apps/desktop/e2e/README.md): `pnpm --filter @blocks2cpp/desktop run e2e` runs them
against the app built with
`tauri build --debug --no-bundle --features e2e-hooks --config '{"build":{"beforeBuildCommand":"pnpm build:e2e"}}'`
(on Linux under `xvfb-run -a`, with `webkit2gtk-driver`); the e2e README has the steps, the
drivers and the settings. `tauri-driver` is a tool, not a workspace crate
([dependencies](dependencies.md#tools)). The guessing-game exit test is
[`e2e/specs/exit/guessing-game.e2e.ts`](../../apps/desktop/e2e/specs/exit/guessing-game.e2e.ts).

- Elements the tests find carry a `data-testid`; the stable ones are listed under
  [Test IDs](../../apps/desktop/e2e/README.md#test-ids) in the e2e README.
- The webview benchmarks are in [`e2e/bench/`](../../apps/desktop/e2e/bench/README.md), and the
  visual diff in [`e2e/visual/`](../../apps/desktop/e2e/visual/README.md).
- What no automated test can judge (screen readers, first-time users, the toolchain matrix) is
  checked by hand with the [manual test protocols](../manual-tests/README.md).
