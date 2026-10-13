# 9. Quality, Testing, Documentation and Delivery

> Status: **Draft v0.1** · Related ADRs: [0007](../adr/0007-backend-crates-and-ipc-contract.md), [0009](../adr/0009-e2e-tooling-and-test-seams.md), [0010](../adr/0010-wasm-delivery-under-the-csp.md)

## 9.1 Engineering standards

### Rust

* Edition 2024. The toolchain is pinned in `rust-toolchain.toml`, and the MSRV
  is declared in the workspace `Cargo.toml` and checked in CI.
* `rustfmt` (checked). Clippy runs with `-D warnings`, `pedantic` (except
  `module_name_repetitions` and `must_use_candidate`) and these lints at
  `deny`: `unwrap_used`, `expect_used` (outside tests), `panic`, `todo`,
  `unimplemented`, `dbg_macro`, `print_stdout`, `print_stderr`, and
  `indexing_slicing` in the WebAssembly facade (`b2c-core-wasm`), plus
  `disallowed_methods` (`std::process::Command::new` outside
  `b2c-process`). Raw output text is confined by module privacy: the only
  function that appends raw text is private to the printer module
  ([06 §6.8.2](06-compiler-pipeline.md#682-typed-text-parse-dont-validate)).
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
* Known defects, open diagnostics and technical debt are tracked in the
  plans under `docs/plans/` and in the roadmap's status notes. GitHub issues
  with milestone labels replace them once the owner sets up the issue tracker
  (deferred on 2026-10-12); the *Known issue* template, which asks for the
  evidence (commit, run, logs and hypothesis), is ready for that.
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
| **Performance** | `criterion` + scripted workspace benchmarks | Codegen p95 at 1k blocks, edits and selections at 1k blocks, drag frame time at 5k blocks, cold start; the longest main-thread task of every E2E flow (reported). More than 10% regression fails CI on the nightly benchmark job and on a pull request's A/B job. |
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
  1,000 blocks (reading the canvas, the compiler core's update in
  WebAssembly, `canonical()` and `preview()` until a single `update()`
  export replaces them, and storing the result, found by an in-page
  heartbeat) and the p95 frame time while dragging in a 5,000-block
  workspace (one sample is a drag away and back). The edit benchmark
  includes Blockly's rendering with the C++ panel open (from an edit until
  the page has finished what it started); selecting blocks in 8 scopes at
  1,000 blocks measures the longest task after a click; and, on the guessing
  game, the longest task after a selection and after an edit is reported.
  Every timing is taken inside the page, never across WebDriver. A run takes
  the median of at least 10 samples and compares it with the baseline, the
  median of the last 5 successful nightly runs on the default branch for the
  same system, kept in the Actions cache per system
  (`tools/bench-compare.py`). More than 10% worse fails; until 5 baselines
  exist the comparison is only reported.
  * Every E2E flow records its longest main-thread task (a heartbeat in the
    page, from the window's readiness until the test ends); the report never
    fails a test.
  * The app measures its preview pipeline and each of its phases, each
    rebuild of the toolbox's flyout, the selection handler and the code
    panel's update as User Timing spans (`b2c:…`); nothing in the app shows
    them. The long-task recorder names the spans that overlap each task it
    keeps, and the benchmarks report each span's time per edit or click
    beside their metrics; the E2E hook gives each span's latest duration.
  * A change expected to alter a gated metric runs the A/B benchmark job on
    its pull request (`bench-ab.yml`): the head (the pull request merged
    into its base branch) and the base (that branch) are built and measured
    in alternating runs on one runner, at least 10 samples each, and the
    head's medians are compared with the base's at the same 10%. An intended
    slowdown is recorded in `apps/desktop/e2e/bench/accepted.json` with its
    reason, which restarts that metric's baseline: the first nightly run
    that sees the entry does not fail on it and starts the metric's history
    again, and an A/B run whose head adds the entry passes.
  * The M2 polish wave's Variables listing, zoom-aware snap radius and
    screen-reader console output (W4-2, W5-1 and W3-11 of the
    [plan](../plans/m2-polish-plan.md)) merge only with a green A/B run
    attached, or once their metrics have at least 5 nightly baselines.
  * M2 gates regressions. Absolute interaction budgets on small programs are
    reported in M2 and gated from M3 once the toolbox refreshes per section.
    Meeting N2–N4 and N12 ([01 §1.4](01-overview.md#14-goals)) at
    1,000–5,000 blocks is part of M5.

## 9.3 Continuous integration

GitHub Actions workflows. All actions are SHA-pinned and least-privilege
([08 §8.9](08-security.md#89-supply-chain)).

| Workflow | Trigger | Jobs |
| ---------- | --------- | ------ |
| `ci.yml` | PR, push to any branch, weekly, manual | `lint` (rustfmt, clippy, also for the Windows target, markdownlint) · `test` (ubuntu, windows) · `test-cgroup` (the Linux cgroup path) · `gcc` (GCC 11 and 15 containers) · `wasm` (build + size budget) · `coverage` (the gates of §9.2) · `docs` (rustdoc, lychee link check, links and heading anchors of every Markdown file, diagnostic codes documented, generated IPC types and catalog outputs up to date) · `deny` · `secrets` (gitleaks) · `security` (pnpm audit, security suite) · `fuzz` (60 s per target) |
| `desktop.yml` | PR and push touching the app, the crates, the packages or the Rust and Node configuration; manual (optionally making new visual-diff candidates) | `frontend` (`tsc --noEmit`, eslint, prettier, package layering) · `test-web` (Vitest with coverage, after building the WASM core; the unit tests of the E2E harness, the benchmarks and the visual diff; the benchmark comparison's self-test and the acceptance file's check) · `app` (build, tests, release checks and start check on ubuntu and windows) · `e2e` (ubuntu, windows: the exit test and the breadth flows, then the visual diff; the Trusted Types summary, which is meaningful on WebView2; the longest main-thread task of each flow, reported) |
| `codeql.yml` | PR, push, weekly | CodeQL: JavaScript/TypeScript, Rust, GitHub Actions (code scanning, see below) |
| `osv-scanner.yml` | PR, push, daily | OSV-Scanner over every lockfile; fails on known vulnerabilities and uploads results to code scanning when available |
| `scorecard.yml` | Push to `main`, weekly, branch-protection changes | OpenSSF Scorecard (code scanning, see below) |
| `workflow-audit.yml` | PR and push touching `.github/`; weekly and manual (adding online audits) | zizmor (auditor persona) |
| `pages.yml` | PR and push touching `docs/`, `site/`, `LICENSE`, `SECURITY.md`, the Node/pnpm config or the workflow itself; manual | Build the specification website (fails on broken links or anchors, unpublished docs and raw HTML); deploy to GitHub Pages from the default branch |
| `nightly.yml` | Daily, manual | `fuzz` (30 minutes per target, one job per target, with a kept corpus) · `links` (lychee over all Markdown) · `e2e-app` (the app under test, built once per OS for the jobs below) · `e2e` (both OSes, every flow and the visual diff) · `e2e-security` (both OSes, [08 §8.13](08-security.md#813-continuous-security-process)) · `bench` (both OSes, with the 10% gate and the acceptance file) |
| `bench-ab.yml` | The `bench-ab` label on a PR (and each push while it has the label); manual (base and head refs, benchmarks, systems) | `plan` (checks the inputs) · `app` (the app under test from the base and from the head, per OS) · `bench` (ubuntu, windows: the head's benchmarks against both builds in alternating runs on one runner, 10 samples each, compared at 10% with the head's acceptance file; §9.2) |
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
* The `docs` job's anchor check (`node site/build.mjs --check-repo-markdown`)
  reads every Markdown file git tracks, except installed dependencies, fuzz
  corpora, local tool state and the released entries of `CHANGELOG.md`, and
  the Markdown references (`file.md#anchor`) in the messages of the build
  scripts. Each relative link must point to a file in the repository, and
  each anchor to a heading GitHub gives that anchor; lychee checks no
  anchors.
* (M3) A weekly `build-docs` job runs `BUILDING.md`'s Debian steps word for
  word in Debian 12 and 13 containers, up to a release build that starts
  under xvfb.
* The weekly dependency health report lists `cargo deny check advisories`
  (including unmaintained crates), `pnpm outdated -r`, the number of
  duplicate crate versions, and `osv-scanner.toml` exceptions that expire
  within 14 days.

**Implemented so far:** every workflow and job in the table except
`release.yml`, which arrives before the first signed release (M6). The
breadth flows and the visual diff in `desktop.yml`'s `e2e` job, and
`nightly.yml`'s `e2e-app`, `e2e`, `e2e-security` and `bench` jobs, came last
in M2. The Windows runs of the breadth flows and the nightly suites pass,
like the Linux ones (scheduled runs since 2026-10-07,
[10 §10.1](10-roadmap.md#101-milestones)). The nightly benchmark gate
only reports a metric until five scheduled runs per system have kept it;
the A/B job (`bench-ab.yml`) and the long-task recorder came with the M2
polish wave, and the A/B job gates a labelled pull request at once. The
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
| `BUILDING.md` | Repo root | Everyone who builds from source (all users until M6), contributors | The one place with prerequisites, install commands and build and run steps per system, kept apart from design notes; its Debian and Windows packages and build commands are the ones CI uses |
| Desktop app README | `apps/desktop/README.md` | Contributors | The dependency table (version, release date, licence, why) and the *Adding an IPC command* steps; updated in the PR that adds a dependency or a command |
| `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `CHANGELOG.md` | Repo root | Everyone | Keep a Changelog format |

The app ships the user guide and references **offline**. *Help* opens bundled
pages, and every diagnostic links to its reference entry.

* User-visible changes get a line in the `[Unreleased]` section of
  `CHANGELOG.md`. The README's status line and the status note in
  [10 §10.1](10-roadmap.md#101-milestones) are updated when a milestone ends.
* All Markdown is linted with markdownlint, and external links are checked
  with lychee (§9.3). Links and heading anchors are checked for every
  tracked Markdown file. rustdoc is built with warnings as errors.
* Build steps are written once: other documents link to `BUILDING.md`.
  Pinned tool versions appear only there and in their source of truth (such
  as `Cargo.lock`). Until the desktop app's README is cut to a code map, it
  and the WebAssembly core's README still repeat a summary of them.
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
