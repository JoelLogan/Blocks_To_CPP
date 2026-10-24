# Dependencies

The desktop app's dependencies and why each one is there. Paths are relative to `apps/desktop/`.
A new dependency gets a row here, in the pull request that adds it, covering its purpose,
maintenance, licence, size and transitive count
([CONTRIBUTING.md](../../CONTRIBUTING.md#rules-for-every-change),
[§8.9](../spec/08-security.md#89-supply-chain)); `cargo deny` and pnpm's workspace settings
(`pnpm-workspace.yaml`, explained in [packages/README.md](../../packages/README.md)) enforce the
policy.

Every dependency is pinned to an exact version and was at least 7 days old when added, as the
workspace policy requires ([§8.9](../spec/08-security.md#89-supply-chain)). The tables list the
npm packages of the app and of the frontend packages (`packages/*` use the same development tools
at the same versions), and the crates that the desktop shell (milestone M0) and milestone M2 added
to the Cargo workspace.

- [npm packages](#npm-packages)
- [Crates](#crates)
- [Tools](#tools)
- [Notes](#notes)

## npm packages

| Package                                  | Version (released)   | Licence           | Why                                                                                                                                                                                                                                                                                                                                             |
| ---------------------------------------- | -------------------- | ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `react`, `react-dom`                     | 19.3.1 (2026-10-14)  | MIT               | UI framework ([§2.1](../spec/02-architecture.md#21-technology-stack))                                                                                                                                                                                                                                                                           |
| `blockly`                                | 12.5.1 (2026-03-20)  | Apache-2.0        | The block editor and its Zelos renderer ([ADR-0002](../adr/0002-block-editor-blockly.md))                                                                                                                                                                                                                                                       |
| `@blockly/continuous-toolbox`            | 7.0.9 (2026-04-09)   | Apache-2.0        | The Scratch-style continuous toolbox (§4.2): category bubbles and one scrolling flyout; Blockly team, npm provenance, no dependencies or install scripts, peer `blockly` ^12 (7.0.9 is the last release for Blockly 12; 13.x needs Blockly 13)                                                                                                  |
| `@tauri-apps/api`                        | 2.12.4 (2026-10-14)  | Apache-2.0 OR MIT | `invoke` for IPC; same minor version as the `tauri` crate                                                                                                                                                                                                                                                                                       |
| `@tauri-apps/cli` (dev)                  | 2.12.3 (2026-10-14)  | Apache-2.0 OR MIT | `tauri dev`, `tauri build`, `tauri icon`                                                                                                                                                                                                                                                                                                        |
| `vite` (dev)                             | 8.3.4 (2026-10-08)   | MIT               | Dev server and bundler                                                                                                                                                                                                                                                                                                                          |
| `@vitejs/plugin-react` (dev)             | 6.1.2 (2026-10-05)   | MIT               | JSX and React Fast Refresh for Vite                                                                                                                                                                                                                                                                                                             |
| `typescript` (dev)                       | 5.9.3 (2025-09-30)   | Apache-2.0        | Type checking; TypeScript 5 as in §2.1 (typescript-eslint 8 supports up to 6.0)                                                                                                                                                                                                                                                                 |
| `@types/react`, `@types/react-dom` (dev) | 19.3.0 (2026-09-09)  | MIT               | Types for React                                                                                                                                                                                                                                                                                                                                 |
| `eslint` (dev)                           | 10.12.0 (2026-10-02) | MIT               | Linting                                                                                                                                                                                                                                                                                                                                         |
| `@eslint/js` (dev)                       | 10.0.1 (2026-02-06)  | MIT               | ESLint's recommended rules (no dependencies)                                                                                                                                                                                                                                                                                                    |
| `typescript-eslint` (dev)                | 8.71.2 (2026-10-12)  | MIT               | `strict-type-checked` and `stylistic-type-checked` rules                                                                                                                                                                                                                                                                                        |
| `eslint-plugin-react-hooks` (dev)        | 5.2.0 (2025-02-28)   | MIT               | Rules of Hooks and effect dependencies                                                                                                                                                                                                                                                                                                          |
| `eslint-plugin-no-unsanitized` (dev)     | 4.1.5 (2026-02-19)   | MPL-2.0           | Flags HTML-injection sinks (§8.8)                                                                                                                                                                                                                                                                                                               |
| `eslint-plugin-jsx-a11y` (dev)           | 6.10.2 (2024-10-26)  | MIT               | Accessibility rules for JSX, the strict set (§9.1, WCAG 2.2 AA in §4.8)                                                                                                                                                                                                                                                                         |
| `prettier` (dev)                         | 3.9.10 (2026-10-10)  | MIT               | Formatting                                                                                                                                                                                                                                                                                                                                      |
| `vitest` (dev)                           | 4.1.12 (2026-10-15)  | MIT               | Unit and component tests (§9.2); uses the app's Vite configuration                                                                                                                                                                                                                                                                              |
| `@vitest/coverage-v8` (dev)              | 4.1.12 (2026-10-15)  | MIT               | Line coverage and the 75% frontend gate (§9.2)                                                                                                                                                                                                                                                                                                  |
| `happy-dom` (dev)                        | 20.14.5 (2026-09-12) | MIT               | The DOM the tests run in, headless; Blockly runs in it. The isolation tests parse `index.html` with it. Its advisory GHSA-834v-8h66-3h79 is excepted until 21.0.4 is old enough (see the notes)                                                                                                                                                 |
| `@testing-library/react` (dev)           | 16.3.3 (2026-08-27)  | MIT               | Renders components in tests and finds elements by role and name                                                                                                                                                                                                                                                                                 |
| `@testing-library/dom` (dev)             | 10.4.2 (2026-09-13)  | MIT               | The DOM queries behind `@testing-library/react` (its peer dependency)                                                                                                                                                                                                                                                                           |
| `axe-core` (dev)                         | 4.14.0 (2026-10-05)  | MPL-2.0           | Automated accessibility checks in component tests (`src/test/axe.ts`); no dependencies                                                                                                                                                                                                                                                          |
| `@testing-library/user-event` (dev)      | 14.6.8 (2026-10-13)  | MIT               | Types and tabs like a person in the keyboard tests (`src/app/layout/accessibility.test.tsx`); no dependencies                                                                                                                                                                                                                                   |
| `selenium-webdriver` (dev)               | 4.51.0 (2026-10-09)  | Apache-2.0        | The end-to-end tests' WebDriver client (`e2e/`, [ADR-0009](../adr/0009-e2e-tooling-and-test-seams.md)); WebdriverIO fails `trustPolicy: no-downgrade`. npm provenance; brings `ws`, `tmp`, `jszip` (with `lie`, `pako`, `readable-stream` 2 and their helpers) and `@bazel/runfiles`, all development-only, none with an install script         |
| `@types/selenium-webdriver` (dev)        | 4.35.7 (2026-09-14)  | MIT               | Types for `selenium-webdriver` (it ships none)                                                                                                                                                                                                                                                                                                  |
| `pixelmatch` (dev)                       | 7.2.0 (2026-04-29)   | ISC               | The visual diff's pixel comparison (`e2e/visual/`); no install script. 8.0.0 changes what the threshold measures (see the notes)                                                                                                                                                                                                                |
| `pngjs` (dev)                            | 7.0.0 (2023-02-20)   | MIT               | Reads and writes the visual diff's PNG screenshots and baselines; no dependencies                                                                                                                                                                                                                                                               |
| `@types/pngjs` (dev)                     | 6.0.5 (2024-05-02)   | MIT               | Types for `pngjs`                                                                                                                                                                                                                                                                                                                               |
| `@types/node` (dev)                      | 24.19.2 (2026-10-09) | MIT               | Node.js types for the end-to-end harness only (`e2e/tsconfig.json`); the app's own TypeScript configuration loads none. The same version as `b2c-core-wasm` and `catalog-gen`                                                                                                                                                                   |
| `markdownlint-cli2` (root, dev)          | 0.23.3 (2026-09-20)  | MIT               | The Markdown lint of `ci.yml`'s `lint` job (`.markdownlint-cli2.jsonc`); a devDependency of the repository root, not of the app. 87 development-only packages, none with an install script. `pnpm-workspace.yaml` overrides three of them to fixed releases: `smol-toml` 1.9.0, `katex` 0.18.2 and `markdown-it` 15.0.3 (with `argparse` 3.0.3) |
| `zustand`                                | 5.0.15 (2026-08-13)  | MIT               | The app's state store (`src/app/store/`); no dependencies                                                                                                                                                                                                                                                                                       |
| `@radix-ui/react-dialog`                 | 1.3.1 (2026-10-12)   | MIT               | Accessible, focus-trapped dialogs (§4.8), also for Blockly's prompts                                                                                                                                                                                                                                                                            |
| `@radix-ui/react-tabs`                   | 1.2.1 (2026-10-12)   | MIT               | The bottom dock's tabs: ARIA tab semantics and arrow-key navigation                                                                                                                                                                                                                                                                             |
| `@radix-ui/react-tooltip`                | 1.4.1 (2026-10-12)   | MIT               | Hints on the toolbar buttons, shown on hover and on keyboard focus                                                                                                                                                                                                                                                                              |
| `@codemirror/view`                       | 6.43.14 (2026-10-07) | MIT               | The read-only C++ view of the code panel (§4.3): decorations, gutters, no workers                                                                                                                                                                                                                                                               |
| `@codemirror/state`                      | 6.7.6 (2026-09-22)   | MIT               | CodeMirror's editor state (state fields and effects for highlights and diagnostics)                                                                                                                                                                                                                                                             |
| `@codemirror/language`                   | 6.13.1 (2026-10-07)  | MIT               | Syntax highlighting from the Lezer parse tree (`HighlightStyle`)                                                                                                                                                                                                                                                                                |
| `@codemirror/lang-cpp`                   | 6.0.3 (2025-06-19)   | MIT               | The Lezer C++ grammar (`@lezer/cpp`)                                                                                                                                                                                                                                                                                                            |
| `@lezer/highlight`                       | 1.2.5 (2026-09-28)   | MIT               | Highlighting tags for the code panel's colours (already a dependency of CodeMirror)                                                                                                                                                                                                                                                             |
| `@xterm/xterm`                           | 5.5.0 (2024-04-05)   | MIT               | The console (§4.5) with its DOM renderer; no dependencies                                                                                                                                                                                                                                                                                       |
| `@xterm/addon-fit`                       | 0.10.0 (2024-04-05)  | MIT               | Fits the terminal to the console panel                                                                                                                                                                                                                                                                                                          |

## Crates

| Crate                                        | Version (released)   | Licence           | Why                                                                                                                                                                                                                                                                                                                                                                                 |
| -------------------------------------------- | -------------------- | ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tauri`                                      | 2.12.4 (2026-10-14)  | Apache-2.0 OR MIT | The desktop shell ([ADR-0001](../adr/0001-desktop-shell-tauri.md))                                                                                                                                                                                                                                                                                                                  |
| `tauri-build` (build)                        | 2.7.3 (2026-10-14)   | Apache-2.0 OR MIT | Embeds the configuration, capabilities and Windows resources. 2.7.3 replaced its own module that links the C runtime statically into Windows MSVC builds with the `static_vcruntime` crate 3.0.0 (2025-11-25; MIT OR Apache-2.0 OR Zlib, no dependencies, build time only)                                                                                                          |
| `tauri-plugin-dialog`                        | 2.8.3 (2026-10-14)   | Apache-2.0 OR MIT | Native open, save and message dialogs, from Rust only (no JavaScript permission). Default features off except `gtk3`: rfd 0.16.0's GTK 3 backend on Linux, no XDG portal and no D-Bus. Brings `tauri-plugin-fs` 2.7.1 (for its `FilePath` type; the fs plugin is not registered)                                                                                                    |
| `rfd` (Windows only)                         | 0.16.0               | MIT               | The Windows trust dialog with _Stay in Restricted Mode_ as its first and default button (08 §8.3.1). No default features, `common-controls-v6`; already in the lockfile through `tauri-plugin-dialog` with the same features, so no new code is compiled                                                                                                                            |
| `tracing-subscriber`                         | 0.3.23 (2026-03-13)  | MIT               | The span registry under the app's own JSON-lines log layer. Features `registry` and `std` only: no `fmt`, ANSI, `log` bridge or `EnvFilter`                                                                                                                                                                                                                                         |
| `notify` (through `b2c-app`)                 | 8.2.0 (2025-08-03)   | CC0-1.0           | The file watcher of open projects (05 §5.10): inotify on Linux, `ReadDirectoryChangesW` on Windows, one non-recursive watch per project folder. No default features (no macOS FSEvents). Its events only wake the backend's own 300 ms debounce and SHA-256 check                                                                                                                   |
| `tracing` (through `b2c-build`, `b2c-app`)   | 0.1.44 (2025-12-18)  | MIT               | Structured logging with a span per build and run session (09 §9.1, 08 §8.11). No default features (`std` only): no `log` bridge and no attribute macros. Already in the lockfile through Tauri                                                                                                                                                                                      |
| `getrandom` (through `b2c-ipc`, `b2c-store`) | 0.4.3 (2026-06-17)   | MIT OR Apache-2.0 | 128 bits from the operating system's random number generator for every opaque IPC ID and recovery ID (02 §2.5.4). Already in the lockfile through `tempfile`; Tauri still brings 0.3.4 into the desktop app                                                                                                                                                                         |
| `ts-rs` (`b2c-ipc`, feature `ts` only)       | 12.0.1 (2026-01-31)  | MIT               | Generates the TypeScript IPC types of `packages/ipc-types` from the Rust types (ADR-0007). Only the generation test turns the feature on, so no app build compiles it. Brings `ts-rs-macros` 12.0.1 (MIT) and `termcolor` 1.4.1 (Unlicense OR MIT)                                                                                                                                  |
| `wasm-bindgen` (through `b2c-core-wasm`)     | 0.2.129 (2026-09-25) | MIT OR Apache-2.0 | The JavaScript glue of the WebAssembly core the editor runs (ADR-0003, ADR-0010). Pinned exactly, because the wasm-bindgen CLI that writes the glue must be the same version. Already in the lockfile through Tauri                                                                                                                                                                 |
| `libc` (through `b2c-process`, Unix only)    | 0.2.190 (2026-10-02) | MIT OR Apache-2.0 | Resets the signal mask and dispositions in a program's process before `exec`, which `rustix` does not offer. No default features. Already in the lockfile through `tempfile` and `getrandom`                                                                                                                                                                                        |
| `criterion` (dev, `b2c-core-wasm`)           | 0.7.0 (2025-07-25)   | Apache-2.0 OR MIT | The native pipeline benchmark (`crates/b2c-core-wasm/benches/pipeline`). No default features (no plotters, no rayon). 0.8 adds `alloca`, whose build script needs a C compiler for the Windows target of the clippy check. Brings `anes`, `cast`, `ciborium` (with `-io` and `-ll`), `criterion-plot`, `crunchy`, `either`, `half`, `itertools` 0.13, `oorandom` and `tinytemplate` |

## Tools

Programs the build and the tests run, which are not workspace dependencies. Their versions are
pinned where they are installed, and [`BUILDING.md`](../../BUILDING.md) installs the ones a
build needs.

- **wasm-bindgen CLI** (MIT OR Apache-2.0): writes the WebAssembly core's JavaScript glue; it must
  be the version of the `wasm-bindgen` crate in `Cargo.lock`, and the core's build checks that
  ([`packages/b2c-core-wasm/README.md`](../../packages/b2c-core-wasm/README.md#build)).
- **binaryen's `wasm-opt`** (Apache-2.0, optional): makes the WebAssembly core smaller.
- **`tauri-driver`** (Apache-2.0 OR MIT): the end-to-end tests' WebDriver server, installed with
  `cargo install --locked` at the version the `e2e` job of `.github/workflows/desktop.yml` pins
  ([e2e/README.md](../../apps/desktop/e2e/README.md#running-them-locally-linux)). It is a tool,
  not a workspace crate.

## Notes

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
  registry packages. `@codemirror/language` 6.13 brings `@codemirror/streamparser` 6.0.0 (MIT, by
  CodeMirror's author, no dependencies), which now holds the `StringStream` that `language`
  re-exports; the code panel uses no stream parser, so the bundle leaves it out. Like every
  `@codemirror` package it has no npm provenance; being the package's first release, it cannot
  fail `trustPolicy: no-downgrade`.
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
  them, with `@types/node` 24.19.2, whose `undici-types` passes the policy. (`happy-dom` brings
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
  install script, and every version was at least 7 days old when added (dialog 1.3.1, tabs 1.2.1,
  tooltip 1.4.1 and the primitives updated with them came out on 2026-10-12). They work with
  `freezePrototype: true` (`src/app/freeze-prototype.test.tsx` checks this under Vitest; the
  end-to-end tests check the release build). Since `react-tooltip` 1.3 an open tooltip also
  describes its control, which already has a description with the same text, so `Hint`
  (`src/app/ui/Hint.tsx`) leaves the control's own description out while the hint is shown and a
  screen reader reads the text once (`Hint.test.tsx`).
- **Test tooling** (`vitest`, `@vitest/coverage-v8`, `happy-dom`, Testing Library, `axe-core`) adds
  about 70 development-only packages; none of it reaches the app bundle. Vitest 4.1.12 fixes
  GHSA-qxg6-89mg-rqv8 and GHSA-xp26-3454-mjgp in `@vitest/mocker`.
- **`happy-dom` 20.14.5** has GHSA-834v-8h66-3h79 (high: host code execution through an
  unsanitised import alias), which no 20.x release fixes. The fix is 21.0.4, a new major
  (published 2026-10-20 18:05 UTC, so old enough on 2026-10-27); 21.0 no longer loads CSS files
  and iframe pages by default, which the tests do not appear to rely on. Until then the advisory is
  a tracked exception in `osv-scanner.toml` and `auditConfig` (review by 2026-11-03): happy-dom is
  never shipped and runs only our own tests.
- **`pixelmatch` 7.2.0**, not 8.0.0 (2026-10-06): 8.0 measures a pixel's colour difference as an
  OKLab HyAB distance instead of a YIQ one, so the visual diff's threshold of 0.1
  (`e2e/visual/compare.ts`) would mean something else. It moves as a change of its own, with the
  baselines checked against the new measure. 8.0.0 was also published from its author's own npm
  account, where 7.1.1 and 7.2.0 came from Mapbox's release accounts; no version has provenance.
- **`tauri` features:** `wry`, `compression`, `isolation`, `common-controls-v6` and `x11`.
  Left out: `devtools` (no built-in developer tools in release builds; WebKitGTK's remote
  inspector is a separate path, see 08 §8.8), `dynamic-acl` (capabilities are fixed
  at build time), `tray-icon`, and `dbus` (unused, and it would need libdbus to build).
- **`rfd` 0.16.0**, not 0.17: `tauri-plugin-dialog` 2.8.3, the newest on 2026-10-24, still
  requires `rfd` 0.16, and the Windows trust dialog uses the plugin's copy, so `rfd` moves when the
  plugin does. On Windows it brings `windows-sys` 0.60.2, which `notify` uses too.
- **`lazy_static` 1.5.0** (through `tracing-subscriber`'s `sharded-slab`): 1.5.1 was less than 7
  days old when it was added. It is old enough now (2026-10-01), but it changes no code and, being
  a transitive crate, is in no Dependabot pull request, so it waits for a later Cargo update.
- **`proptest` 1.12** (test-only, every crate's property tests) moved to `rand` 0.10 and brings
  `rand` 0.10.3, `rand_core` 0.10.1, `rand_xorshift` 0.5.0, `chacha20` 0.10.2, `core_detect` 1.0.0,
  `bit-set` 0.11.1 and `bit-vec` 0.10.1 (all MIT OR Apache-2.0); `rand_chacha`, `ppv-lite86` and
  `rand` and `rand_core` 0.9 leave the lockfile. `borsh` and `borsh-derive` 1.8.1 (MIT OR
  Apache-2.0, and Apache-2.0) and `cfg_aliases` 0.2.2 (MIT) are listed in `Cargo.lock` only because
  `bit-vec` names them in an optional feature that nothing turns on, so they are never built
  (`cargo tree --all-features --target all -i borsh` finds nothing).
- **`notify` 8.2.0**, the newest stable release (9.0 is still a release candidate). Without
  default features it brings `inotify`, `inotify-sys`, `mio`, `walkdir`, `notify-types`,
  `bitflags` and `log` on Linux (most already in the lockfile through Tauri), and `walkdir` and
  `windows-sys` 0.60 on Windows.
- Cargo has no minimum release age, so `Cargo.lock` holds Tauri's own crates and the crates it
  brought in at their newest versions that were at least 7 days old. Keep it that way when
  updating them, with `cargo update -p <crate> --precise <version>`. On 2026-10-24 that was
  `tauri` 2.12.4 with `tauri-build`, `tauri-codegen` and `tauri-macros` 2.7.3, `tauri-runtime` and
  `tauri-runtime-wry` 2.12.2, `tauri-utils` 2.10.2 and `tauri-plugin-fs` 2.7.1 (all 2026-10-14):
  `tauri` 2.12.5 with the 2.7.4 crates and `tauri-runtime-wry` 2.12.3 came out on 2026-10-21, and
  `wry` 0.57.1 and `tao` 0.37.2 on 2026-10-23, so `wry` 0.57.0 and `tao` 0.37.1 stay. A plain
  `cargo update -p tauri --precise 2.12.4` also takes `tauri-codegen` and `tauri-macros` 2.7.4 and
  `tauri-runtime-wry` 2.12.3, which are set back with `--precise` too; `tauri` 2.12.4 needs
  `swift-rs` 1.0.9 (2026-10-10, macOS only). Other crates are updated the same way: `tokio` is on
  1.53.3 (2026-10-13), which fixes RUSTSEC-2026-0386, because 1.54.0 was yanked and 1.54.1 came
  out on 2026-10-21. A `--precise` update can also move unrelated crates between the `windows-sys`
  (or `syn`) versions already in the lockfile, so read the `Cargo.lock` diff and keep only the
  intended changes. `fuzz/Cargo.lock` gets the same versions, since `cargo check --locked`
  of the fuzz targets fails once the workspace asks for a newer one.
