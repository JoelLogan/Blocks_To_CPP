# 9. Quality, Testing, Documentation and Delivery

> Status: **Draft v0.1** · Related ADRs: [0007](../adr/0007-backend-crates-and-ipc-contract.md), [0009](../adr/0009-e2e-tooling-and-test-seams.md), [0010](../adr/0010-wasm-delivery-under-the-csp.md)

## 9.1 Engineering standards

### Rust

* Edition 2024. The toolchain is pinned in `rust-toolchain.toml`, and the MSRV
  is declared in the workspace `Cargo.toml` and checked in CI.
* `rustfmt` (checked). Clippy runs with `-D warnings` plus a curated
  `pedantic` subset and these lints at `deny`: `unwrap_used`, `expect_used`
  (outside tests), `panic`, `indexing_slicing` (in parsers/encoders), `todo`,
  `dbg_macro`, `print_stdout`, plus `disallowed_methods` (e.g.
  `std::process::Command::new` outside `b2c-process`, raw writes outside the
  emitter's token module).
* `#![forbid(unsafe_code)]` in every crate except `b2c-process`
  ([02 §2.3](02-architecture.md#23-repository-layout)). `b2c-core-wasm` uses
  `deny` instead, because the code `#[wasm_bindgen]` generates contains
  `unsafe`; hand-written `unsafe` is still not allowed there.
* Errors use `thiserror` enums per crate, with user-facing text produced by
  the i18n layer, never by `Display` of internal errors. No panics cross the
  IPC boundary: command handlers return typed errors.
* `#![warn(missing_docs)]` on all library crates. Each crate and module
  starts with `//!` docs that cite the spec sections it implements, and every
  public item has `///` docs. CI builds the docs with warnings as errors
  (`RUSTDOCFLAGS='-D warnings' cargo doc`), and doc examples compile
  (`no_run` where they would do I/O).
* Logging uses `tracing` with structured fields and spans per build/run
  session.

### TypeScript / frontend

* `strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`,
  `noImplicitOverride`, `noFallthroughCasesInSwitch`.
* ESLint (flat config): `typescript-eslint` *strict-type-checked*,
  `react-hooks`, `jsx-a11y`, `no-unsanitized`, and custom `no-restricted-*`
  rules from [08 §8.8](08-security.md#88-webview-and-ipc-hardening).
  No `any` and no non-null assertions without a justified disable comment.
  `eslint-plugin-react` is excluded because it fails the pnpm trust policy
  ([ADR-0009](../adr/0009-e2e-tooling-and-test-seams.md)); its
  `react/no-danger` rule is replaced by a `no-restricted-syntax` rule.
* Prettier (checked). IPC types are **generated** from Rust with `ts-rs`
  ([ADR-0007](../adr/0007-backend-crates-and-ipc-contract.md)); a CI check
  fails if they are stale. The same holds for the catalog's generated block
  definitions and block reference.
* Every workspace package follows one template (`packages/README.md`):
  private, named `@blocks2cpp/<name>`, exact dependency versions, the compiler
  flags above, the shared ESLint and Prettier setup, and the scripts
  `typecheck`, `lint`, `format:check`, `test` and `test:coverage`.

### General

* **Conventional Commits** (`feat:`, `fix:`, `docs:`, `sec:`, …), small
  focused PRs, squash merges.
* Branch protection on `main`: required status checks, at least one review,
  CODEOWNERS review for `crates/b2c-process/`, `crates/b2c-ipc/`,
  `crates/b2c-store/`, `crates/b2c-app/`, `apps/desktop/src-tauri/`,
  `.github/`, `catalog/`, and security-relevant encoders.
* **Definition of Done** for every PR:
  * [ ] Tests added or updated (unit + golden/E2E where behaviour is visible)
  * [ ] Docs updated (spec/user guide/reference); a new ADR for significant decisions
  * [ ] `CHANGELOG.md` entry for user-visible changes
  * [ ] Security checklist answered (new IPC command? new `unsafe`? new dependency? new process/file operation? new untrusted input?)
  * [ ] Accessibility checked for UI changes (keyboard path, labels, contrast)
  * [ ] No new warnings; all CI checks green

## 9.2 Testing strategy

| Level | Tooling | Scope / oracle |
| ------- | --------- | ---------------- |
| **Unit** | `cargo nextest`, Vitest | Every module. Encoders, parsers and validators aim for 100% branch coverage. |
| **Snapshot** | `insta` | Generated C++ for every catalog block in its default configuration, plus curated variants. Each snapshot is also compiled with `-fsyntax-only`. |
| **Golden end-to-end** | `b2c` CLI | `examples/*.b2c` → generate → compile → run with an stdin fixture → compare stdout, stderr and exit code with `tests/golden/`. Matrix: GCC 11, 13 and 15 on Linux (containers), MSYS2 UCRT64 GCC on Windows. |
| **Property-based** | `proptest` | (a) String-literal round trip: arbitrary text → program prints exactly those bytes. (b) **Differential "no generator bugs" oracle**: random well-typed block programs from a BDM generator; if the analyser reports no errors, g++ must accept the output. (c) Determinism: generate twice, and with shuffled top-level order → identical bytes. (d) BDM → Blockly → BDM round trip is the identity. |
| **Fuzzing** | `cargo-fuzz` (libFuzzer) | BDM parser, expression parser, type parser, catalog/pack loader, SARIF/JSON/text diagnostics parsers, GDB/MI parser, event-channel parser, comment/string encoders |
| **Mutation** | `cargo-mutants` (weekly) | Security-critical encoders and validators must kill all mutants |
| **Frontend components** | Vitest + Testing Library + axe-core | Panels, dialogs, custom Blockly fields. Automated accessibility checks. |
| **Desktop E2E** | selenium-webdriver + Vitest + `tauri-driver` (Windows + Linux) | Create project, Quick Insert, build, run with input, stop; open an untrusted project → Restricted Mode; trust flow; save/reload; crash recovery |
| **Visual diff** | WebDriver screenshots + pixelmatch | The canvas with a known project on both systems |
| **Security regression** | Custom harness | `tests/security/projects/*.b2c` with asserted outcomes ([08 §8.12](08-security.md#812-threat-summary)) |
| **Performance** | `criterion` + scripted workspace benchmarks | Codegen p95 at 1k blocks, drag frame time at 5k blocks, cold start. More than 10% regression fails CI on the benchmark job. |
| **Manual** | Per milestone | Screen readers (NVDA on Windows, Orca on Linux), high-contrast themes, real-world toolchain matrix (WinLibs, TDM, Scoop, Strawberry, distro GCCs), and sessions with first-time users where an exit criterion needs them; protocols and results in [`docs/manual-tests/`](../manual-tests/README.md) |

**Coverage gates:** core compiler crates ≥ 90% lines, all Rust ≥ 80%, and
frontend ≥ 75%. Measured with `cargo-llvm-cov` and Vitest coverage, and
reported on PRs.

Details of each level as M2 sets them up:

* **Coverage.** The core compiler crates (`b2c-ir`, `b2c-model`,
  `b2c-catalog`, `b2c-lang`, `b2c-codegen`) together must reach 90% of lines,
  and all default workspace members on Linux 80% (`cargo-llvm-cov`).
  `apps/desktop` and every package in `packages/` must reach 75% of lines
  (Vitest with the V8 coverage provider). Results go to the job summary; the
  workflows need no permission to write to pull requests. Tests that start
  our own binaries pass `LLVM_PROFILE_FILE` on explicitly, so those runs are
  counted.
* **Fuzzing.** The targets also cover the clipboard loader, the sanitizer
  report scanner and every diagnostics parser entry point. No target performs
  I/O; they call only pure parsing functions, so `fuzz/` may depend on
  `b2c-toolchain`. Diagnostics seeds come from fixtures recorded with GCC 13,
  GCC 11 (recorded locally) and GCC 15 (recorded in the pinned `gcc:15`
  container by a manually started job). Every pull request runs each target
  for 60 s, iterating over `cargo fuzz list`, so a new target needs no
  workflow change; the nightly run gives each target 30 minutes with a kept
  corpus.
* **Mutation testing** (`.cargo/mutants.toml`) covers `b2c-ir/src/text.rs`,
  `b2c-toolchain/src/{flags,env,discovery}.rs`, `b2c-model/src/json.rs` and
  `decode/`, `b2c-build/src/build_dir.rs` and `check_program` in
  `b2c-process/src/command.rs`, excluding the generated
  `reserved_names.rs`. Any missed mutant fails the job; timeouts and unviable
  mutants are reported only.
* **Desktop E2E** ([ADR-0009](../adr/0009-e2e-tooling-and-test-seams.md)):
  selenium-webdriver drives `tauri-driver` (installed with `cargo install
  --locked` at a pinned version), with Vitest as the runner; WebdriverIO
  fails the dependency policy. Linux uses `WebKitWebDriver` under `xvfb`,
  Windows `msedgedriver` matched to the installed WebView2. The tests run a
  debug build with the `e2e-hooks` feature, so the native dialogs are
  scripted and each test gets a fresh profile, while the CSP, the isolation
  hook and `freezePrototype` stay active. They read the console through the
  test hook's transcript (xterm.js's DOM renderer holds only the rows on
  screen) and find elements by `data-testid`. CI checks that
  release builds contain neither the hooks nor the frontend's test helper.
* **The M2 exit test** (`apps/desktop/e2e/specs/exit/`) assembles the
  guessing game from the *Empty* template on both systems: real drags for
  some blocks (the program, a variable, a print and a loop), a test helper
  for the rest, then a build, a binary search over *Too low!* and *Too
  high!* until *Correct!*, and *Finished (exit code 0)* in the console
  header. **The breadth flows** (`specs/flows/`) cover Restricted Mode and
  trust, save and reload, crash recovery, Stop (the button and `Shift+F5`),
  exit decoding, closing the app while a program runs (no process outlives
  it), a flood of 10,000,000 output lines, the setup page, an external change
  (also to a deleted file), the clipboard, settings after a restart, the
  variable menus, the connection checker, selecting a block from its code, a
  drag from every toolbox category, and a build and a run with input. Both
  run in the pull request E2E job on both systems; the security tests of
  [08 §8.13](08-security.md#813-continuous-security-process) run nightly.
  Typing in expression slots and Quick Insert are M3 features and are not
  tested in M2. *Without reading docs* is checked by a manual usability
  session with first-time users on both systems before M2 is declared done
  ([the protocol](../manual-tests/m2-usability.md)).
* **Visual diff.** The guessing game in a 1000×700 window at zoom 1.0, light
  theme, caret hidden, with the C++ and bottom panels folded, compared with
  pixelmatch (threshold 0.1); more than 0.5% of differing pixels fails. A
  native driver that cannot resize the window leaves the app at its own size
  with a warning, and the baseline's size check still applies. Each system
  has its own baseline in `apps/desktop/e2e/visual/baselines/{linux,windows}/`.
  The E2E job uploads its screenshots as candidates on every run (a manual
  run with the `visual-baselines` input only writes candidates); a person
  reviews them and commits the baselines. Until a system's baseline is
  committed, its visual diff passes with a warning. It runs in the pull
  request E2E job and nightly.
* **Benchmarks.** `criterion` measures the native pipeline (load, resolve,
  analyse, generate, and the whole preview) on a generated 1,000-block
  module. Through the E2E harness, the webview benchmarks measure cold start
  to the editor's readiness marker, the p95 of the preview pipeline's task at
  1,000 blocks (reading the canvas, `canonical()` and `preview()` in
  WebAssembly and storing the result, found by an in-page heartbeat) and the
  p95 frame time while dragging in a 5,000-block workspace (one sample is a
  drag away and back). The time from an edit to its C++ is reported too, not
  gated. Every timing is taken inside the page, never across WebDriver. A
  run takes the median of at least 10 samples and compares it with the
  baseline, the median of the last 5 successful nightly runs on the default
  branch for the same system, kept in the Actions cache per system
  (`tools/bench-compare.py`). More than 10% worse fails; until 5 baselines
  exist the comparison is only reported. M2 gates regressions; meeting the
  targets N2–N4 ([01 §1.4](01-overview.md#14-goals)) is part of M5.

## 9.3 Continuous integration

GitHub Actions workflows. All actions are SHA-pinned and least-privilege
([08 §8.9](08-security.md#89-supply-chain)).

| Workflow | Trigger | Jobs |
| ---------- | --------- | ------ |
| `ci.yml` | PR, push to any branch, weekly, manual | `lint` (rustfmt, clippy, also for the Windows target, markdownlint) · `test` (ubuntu, windows) · `test-cgroup` (the Linux cgroup path) · `gcc` (GCC 11 and 15 containers) · `wasm` (build + size budget) · `coverage` (the gates of §9.2) · `docs` (rustdoc, lychee link check, diagnostic codes documented, generated IPC types and catalog outputs up to date) · `deny` · `secrets` (gitleaks) · `security` (pnpm audit, security suite) · `fuzz` (60 s per target) |
| `desktop.yml` | PR and push touching the app, the crates, the packages or the Rust and Node configuration; manual (optionally making new visual-diff candidates) | `frontend` (`tsc --noEmit`, eslint, prettier, package layering) · `test-web` (Vitest with coverage, after building the WASM core; the unit tests of the E2E harness, the benchmarks and the visual diff; the benchmark comparison's self-test) · `app` (build, tests, release checks and start check on ubuntu and windows) · `e2e` (ubuntu, windows: the exit test and the breadth flows, then the visual diff; the Trusted Types summary, which is meaningful on WebView2) |
| `codeql.yml` | PR, push, weekly | CodeQL: JavaScript/TypeScript, Rust, GitHub Actions (code scanning, see below) |
| `osv-scanner.yml` | PR, push, daily | OSV-Scanner over every lockfile; fails on known vulnerabilities and uploads results to code scanning when available |
| `scorecard.yml` | Push to `main`, weekly, branch-protection changes | OpenSSF Scorecard (code scanning, see below) |
| `workflow-audit.yml` | PR and push touching `.github/`; weekly and manual (adding online audits) | zizmor (auditor persona) |
| `pages.yml` | PR and push touching `docs/`, `site/`, `LICENSE`, `SECURITY.md`, the Node/pnpm config or the workflow itself; manual | Build the specification website (fails on broken links or anchors, unpublished docs and raw HTML); deploy to GitHub Pages from the default branch |
| `nightly.yml` | Daily, manual | `fuzz` (30 minutes per target, one job per target, with a kept corpus) · `links` (lychee over all Markdown) · `e2e-app` (the app under test, built once per OS for the jobs below) · `e2e` (both OSes, every flow and the visual diff) · `e2e-security` (both OSes, [08 §8.13](08-security.md#813-continuous-security-process)) · `bench` (both OSes, with the 10% gate) |
| `weekly.yml` | Weekly, manual | `mutants` (`cargo-mutants`) · `dependency-health` (report) · `geiger` (`cargo-geiger`, report only) · `fuzz-cmin` (corpus minimisation) |
| `release.yml` | Signed tag `v*` | Build and bundle (Windows MSI/NSIS, Linux AppImage/deb/rpm), sign, SBOMs, provenance attestations, draft GitHub Release. Runs in the protected `release` environment. |

**Code scanning** (uploading CodeQL, OSV-Scanner and Scorecard results to the repository's
Security tab) needs a public repository or GitHub Code Security. The CodeQL and Scorecard jobs
and the OSV upload run when the repository is public, or when the repository variable
`B2C_CODE_SCANNING` is `true` (set it after enabling Code Security on a private repository).
The OSV scan itself always runs and fails on known vulnerabilities.

Notes on the jobs:

* The full OSV scan of the default branch runs daily in `osv-scanner.yml`, and
  OpenSSF Scorecard weekly in `scorecard.yml`; the nightly and weekly
  workflows do not repeat them.
* `test-cgroup` enables lingering for the runner's user, exports
  `XDG_RUNTIME_DIR` and sets `B2C_REQUIRE_CGROUP=1`, so the cgroup tests of
  `b2c-process` and `b2c-build` fail instead of skipping. Every other job
  exercises the fallback, and their cgroup tests skip with a logged reason.
* markdownlint uses `.markdownlint-cli2.jsonc`: the default rules, with line
  length (MD013) off, inline HTML (MD033) forbidden and repeated headings
  (MD024) allowed only in different sections. lychee is pinned, retries three
  times, accepts the status codes 200, 206 and 429, skips `localhost`,
  `tauri.localhost`, `ipc.localhost` and `example.com`, and uses no token; it
  runs when Markdown changes and on a schedule.
* The weekly dependency health report lists `cargo deny check advisories`
  (including unmaintained crates), `pnpm outdated -r`, the number of
  duplicate crate versions, and `osv-scanner.toml` exceptions that expire
  within 14 days.

**Implemented so far:** every workflow and job in the table except
`release.yml`, which arrives before the first signed release (M6). The
breadth flows and the visual diff in `desktop.yml`'s `e2e` job, and
`nightly.yml`'s `e2e-app`, `e2e`, `e2e-security` and `bench` jobs, came last
in M2: their tests pass on Linux, and their Windows runs are not yet
confirmed ([10 §10.1](10-roadmap.md#101-milestones)). The benchmark gate
only reports until five scheduled runs per system have been kept, and the
visual diff only warns until a person commits its baselines.

## 9.4 Documentation

Documentation is part of the product and is maintained **in the same PR** as
the code it describes.

| Doc | Location | Audience | Maintenance |
| ----- | ---------- | ---------- | ------------- |
| Specification | `docs/spec/` | Contributors | Living document with status and last-reviewed date per chapter |
| Specification website | `site/` → GitHub Pages | Everyone | **Generated** from `docs/spec/` and `docs/adr/` on every change; the build fails on broken links |
| ADRs | `docs/adr/` | Contributors | One per significant decision (template in `docs/adr/README.md`); superseded, never deleted |
| User guide + tutorials | `docs/user-guide/` (Markdown; the mdBook build and the bundled copy come with the offline help in M5) | Users, teachers | Updated with the features it describes; screenshots, once added, are regenerated by E2E tests. M2 has the [getting-started page](../user-guide/getting-started.md) |
| Manual test protocols | `docs/manual-tests/` | Testers, maintainers | One protocol per milestone check that cannot be automated (screen readers, usability), with its results recorded in the protocol ([index](../manual-tests/README.md)) |
| Block reference | `docs/reference/blocks/` | Users | **Generated** from the catalog; CI fails if stale |
| Diagnostics reference | `docs/reference/diagnostics/` | Users | **Generated** from the message catalog plus explanations; CI fails if a code is undocumented |
| API docs | rustdoc / TSDoc | Contributors | Built in CI; `missing_docs` warnings |
| CLI reference | `docs/reference/cli.md` | Users, CI authors | Generated from `clap` definitions |
| Desktop app README | `apps/desktop/README.md` | Contributors | The dependency table (version, release date, licence, why) and the *Adding an IPC command* steps; updated in the PR that adds a dependency or a command |
| `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `CHANGELOG.md` | Repo root | Everyone | Keep a Changelog format |

The app ships the user guide and references **offline**. *Help* opens bundled
pages, and every diagnostic links to its reference entry.

* User-visible changes get a line in the `[Unreleased]` section of
  `CHANGELOG.md`. The README's status line and the status note in
  [10 §10.1](10-roadmap.md#101-milestones) are updated when a milestone ends.
* All Markdown is linted with markdownlint, and external links are checked
  with lychee (§9.3). rustdoc is built with warnings as errors.
* **In M2** there is no *Help* menu: *Learn more* in Problems opens the
  published diagnostics reference, and the setup page opens the MSYS2 and
  WinLibs sites, in the system browser through fixed links
  ([08 §8.8](08-security.md#88-webview-and-ipc-hardening)). *Help*, the
  bundled offline pages and per-code links come in M5.

## 9.5 Versioning

| Thing | Scheme |
| ------- | -------- |
| Application | SemVer. `0.x` until 1.0. |
| Project file | Integer `formatVersion` with migrations ([05 §5.7](05-project-format.md#57-versioning-and-migration)) |
| Catalog / packs | SemVer. Per-block integer `v` with migrations. |
| CLI JSON output, IPC | Versioned schemas. Breaking changes only in major releases. |
| IPC | The integer `IPC_VERSION` in `b2c-ipc`, generated into `packages/ipc-types` and compared at startup through `app_info`; a mismatch shows a blocking error ([02 §2.5.7](02-architecture.md#257-versioning)). The `ipc-types` package version equals the app version. |
| Machine-local files, build manifests, recovery metadata | A `format` tag and an integer `formatVersion` each ([05 §5.9](05-project-format.md#59-machine-local-data)) |

## 9.6 Release process

1. Release branch cut, with feature freeze. The changelog is curated.
2. Release gate ([08 §8.10](08-security.md#810-releases-and-updates)): all
   checks green, no high/critical advisories, threat-model review done, manual
   test matrix done.
3. A signed tag triggers `release.yml`. Artifacts are signed, SBOMs and
   attestations are attached, and the draft release is reviewed and
   published.
4. The update manifest is published (signed). The documentation site is
   deployed.

**Channels:** `nightly` (automated, for testers), `beta`, `stable`.
