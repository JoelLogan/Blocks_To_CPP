# 10. Roadmap, Risks and Product Decisions

> Status: **Draft v0.1**

## 10.1 Milestones

> **Implementation status (2026-10-11).** **M2** is in progress. Its
> automated exit criteria are met, but the owner's first trial (Debian,
> 2026-10) found 18 issues, so an [M2 polish wave](#m2-polish-part-of-m2)
> runs inside M2 before the usability sessions: their participants must be
> first-time users, so they are not spent on problems already known. The
> desktop editor ([apps/desktop](../../apps/desktop/README.md),
> [getting started](../user-guide/getting-started.md)) builds and runs block
> programs with the user's g++ on Linux and Windows: blocks and a toolbox
> generated from the catalog, a live C++ preview with two-way highlighting,
> problems on blocks and in a list, the setup page, a console with Stop and
> exit decoding, projects with autosave and crash recovery, workspace trust
> and Restricted Mode, the validated clipboard and the reload of files
> changed outside the app, behind the hardened IPC contract of 36 commands.
> The guessing-game exit test, assembled from the *Empty* template, and the
> breadth E2E flows pass in CI on both systems (commit `8c531d3`); the visual
> diff of the canvas runs there too, but compares nothing until its baselines
> are committed. The first scheduled nightly run, with the E2E security
> tests, the benchmarks and the fuzzing, passed on both systems (2026-10-07,
> commit `0709960`). The first measurements on CI's runners, which have no
> GPU, are a cold start of 1.2 s (Linux) and 2.8 s (Windows), and a p95 frame
> time while dragging in a 5,000-block workspace of 40 ms (Linux) and 914 ms
> (Windows): meeting the targets N2–N4 and N12 is M5's exit, M2 gates
> regressions only. Readings in the app (debug backend, production frontend,
> Linux under xvfb without a GPU, 2026-10-10): selecting 1.2 s, an edit at
> the end of `main` 0.85 s, dragging a 1,000-block stack 14 s in all; a
> toolbox rebuild 0.13–0.17 s at 40 blocks. They are to be measured again in
> a release build. The polish wave's benchmarks (2026-10-13, debug backend,
> the end-to-end frontend build, after the preview churn was reduced) give
> the longest task after a click at 1,000 blocks about 1.2 s (the toolbox's
> rebuild about 0.55 s of it), an edit until the page settles with the C++
> panel shown about 1.4 s at p95, and the preview task about 100 ms at p95;
> on the guessing game a selection about 140 ms (the toolbox's rebuild about
> 120 ms) and an edit about 33 ms; a program's 200,000 lines take about
> 0.8 s to show in the console. M2 is done once the polish wave is finished
> and these are recorded on a polish build: the threat-model review, the
> first green weekly run, the toolchain matrix (at least the Debian cell, or a
> recorded deferral), committed visual baselines, the usability sessions with
> at least three first-time users ([protocol](../manual-tests/m2-usability.md)),
> the NVDA and Orca pass ([checklist](../manual-tests/m2-accessibility.md)),
> and the review at the milestone demo that nothing of M3–M6 has an entry
> point in the app (first run on the code, recorded in the usability
> protocol). The benchmarks' history builds up from the first CI runs
> ([09 §9.3](09-quality-and-delivery.md#93-continuous-integration)).
>
> **M1** is done: loading and
> validating projects, the block catalog, analysis, C++ generation with source
> maps, toolchain discovery and probing, building with g++ and running
> programs, all behind the `b2c` command-line tool
> ([reference](../reference/cli.md)). Fifteen example projects pass the golden
> tests on Linux with GCC 11, 13 and 15 and on Windows with MSYS2 UCRT64,
> together with the malicious-project suite and fuzz smoke tests. The
> diagnostics model's quick-fix field arrives with quick fixes in M3. **M0** is
> done: the desktop shell ([apps/desktop](../../apps/desktop/README.md)) is a
> Tauri 2 window with an empty Blockly canvas, the specified Content Security
> Policy, the isolation pattern and minimal capabilities, built and
> start-checked in CI on Linux and Windows, and CI checks the crate layering
> rules. Three M0 items moved: branch protection waits until the `main` branch
> is created at the end of 1.0 (owner's decision), and the `packages/`
> skeletons and markdownlint came with M2, where they were first needed.

Each milestone ends with a demo, a threat-model review
([08 §8.13](08-security.md#813-continuous-security-process)), a manual test
pass ([09 §9.2](09-quality-and-delivery.md#92-testing-strategy): screen
readers, high contrast, the toolchain matrix) and updated documentation.

### M0: Foundations

* Monorepo scaffold: Cargo and pnpm workspaces, crate/package skeletons with
  layering checks, a Tauri app with an empty Blockly canvas.
* CI: lint, test, CodeQL, cargo-deny, OSV-Scanner, gitleaks, zizmor,
  Scorecard, Dependabot. Branch protection. PR template with security
  checklist. CODEOWNERS.
* Docs: this spec, ADRs, README, SECURITY.md, CONTRIBUTING.md,
  CHANGELOG.md.
* **Exit:** CI green on Windows and Linux; an empty app launches on both.

### M1: Compiler vertical slice (headless)

* BDM v1 with validator and limits; catalog loader; lowering, names, types
  and codegen for **`main`, variables (`int`/`double`/`bool`/`string`),
  arithmetic, comparison and logic, if/else, while/repeat/for-range,
  print/ask, functions with parameters and return, block comments**.
* Typed encoders (§8.4) with full unit, property and fuzz tests. Source maps.
  Diagnostics model.
* Toolchain discovery and probing. Argv construction. SARIF/JSON/text
  diagnostics. Build cache. `b2c check | generate | build | run | toolchains`.
* Golden test harness with 10+ example programs.
* **Exit:** every example compiles and runs identically on Linux (GCC 11, 13
  and 15) and Windows (MSYS2), and the security regression suite for
  encoders passes.

### M2: Desktop editor MVP

* Blockly with Zelos and a theme, a toolbox generated from the catalog, BDM ⇄
  workspace sync, WASM live preview with two-way highlighting, diagnostics on
  blocks and in the Problems panel.
* Toolchain setup page. Build and run in a PTY console with Stop and exit
  decoding.
* Project new/open/save/autosave/recovery. Workspace trust and Restricted
  Mode. IPC hardening (capabilities, isolation, CSP).
* Dynamic Program, Variables and Functions categories (the standard variable
  set with scope- and type-filtered menus), and symbol dropdowns filtered by
  scope. Parked (loose) blocks: kept, warned, never run
  ([ADR-0012](../adr/0012-parked-blocks.md)). A type-aware connection
  checker, Scratch-style snapping and wrapping (wrapping moves to M3 if the
  polish wave cannot fit it). Copy and paste through the validated clipboard
  format. External change detection for open projects (Reload or Keep mine).
* Machine settings: a versioned, schema-checked `settings.json`
  (`settings_get`/`settings_update`) and a Settings page (code style, Run on
  errors, console). Build cache eviction (size cap, 30-day pruning) and
  *Clear build cache*.
* The IDE init unit for IDE runs (UTF-8 console on Windows), never shown in
  the code view or exported. Linux cgroup v2 containment for builds and runs,
  with an RSS watchdog fallback and a "process group only" notice when it is
  unavailable. Windows app hardening (DLL search order, long-path manifest).
  A Trusted Types trial (report-only) and a review checklist for custom
  fields and tooltips. Structured logging with rotating local logs.
* Packages: `ipc-types` (TypeScript IPC types generated from the Rust
  commands, versioned, with a CI staleness check), `blockly-ext`,
  `catalog-gen` (the toolbox and the block reference, with a CI staleness
  check) and `b2c-core-wasm`.
* CI: nightly (30-minute fuzzing with a kept corpus, E2E security tests and
  benchmarks on both OSes) and weekly (mutation testing, dependency health)
  workflows; coverage gates; benchmarks with a regression gate (codegen at
  1,000 blocks, large-workspace drag, cold start); a visual-diff test of the
  canvas on both OSes; markdownlint, external link check, rustdoc build, WASM
  size budget and cargo-geiger. Fuzz targets for the diagnostics parsers.
* Docs: `BUILDING.md` (building and running from source) and the
  getting-started page.
* **Exit:** a first-time user builds and runs the guessing game
  ([03 §3.13.1](03-block-language.md#3131-guessing-game-input-random-loops-branching))
  without reading docs. E2E tests cover the flow on both OSes.

### M2 polish (part of M2)

The owner's first trial (Debian, 2026-10) found 18 issues (U1 to U18). They
are tracked in the [plan](../plans/m2-polish-plan.md), not as GitHub issues:
the owner deferred the issue tracker (W0-7) and the upstream reports on
2026-10-12. The wave also fixes
two data-loss and correctness defects, adds DOM-level chrome checks and
small-program benchmarks, and writes `BUILDING.md`. Its items are split into
a must-have set and an if-time set, worked on in two parallel streams within
a budget of 8 weeks (owner's decision of 2026-10-10). The
[plan](../plans/m2-polish-plan.md) gives each item's cause, fix and tests.

* Decisions and groundwork: parked blocks
  ([ADR-0012](../adr/0012-parked-blocks.md)), additive format keys
  ([ADR-0013](../adr/0013-additive-format-keys.md)), the rules for extending
  Blockly ([ADR-0014](../adr/0014-extending-blockly.md)), spec corrections,
  Dependabot groups and an anchor check over all Markdown (the issue
  tracker is deferred).
* Small fixes, the two P0 defects (a drop on the toolbox deleting the blocks
  below a previewed place; a negative number typed into a slot being
  rejected) and `BUILDING.md`.
* A performance guard: the longest main-thread task of every E2E flow, A/B
  benchmark runs, and selection and edit metrics.
* Layout (toolbox width, dock sizes, window fit), named canvas controls, the
  console between runs, and renaming the project.
* Parked blocks, the standard variable set, a Variables listing that does
  not depend on the selection, snapping, and the remaining documentation.
* If time: remembered toolbox and dock sizes, block upgrades on load and one
  *change* block for every update operator, Scratch-style wrapping, the
  developer guide, and the other items the plan marks *M2 if time*.

**Exit:** both P0 defects are fixed; each of U1–U18 is fixed or moved to M3
with the owner's agreement; items marked *M2 if time* move to M3 without
further sign-off when the wave's budget is spent; the usability protocol and
the getting-started guide describe the polished editor.

### M3: Language breadth I

* Expression slots and Quick Insert. Type picker. Collections, strings,
  structs, enums, files, errors, random and time.
* Multi-module projects with headers. Export to CMake/Make. Friendly
  message catalog for g++ errors. Support helpers.
* Global variables and constants (with a const/constexpr suggestion), math
  functions and text/number conversions. Organisation blocks (namespace, use
  header, use namespace, static_assert) and the textbook-style
  `using namespace std` project setting (Q4).
* The expert-mode switch (Q8): Friendly/C++ block labels and *Show advanced
  blocks*. Toolbox search. A project settings dialog (standard, options,
  build configurations, defines) and run options (arguments, working
  directory, stdin from a file, run in an external terminal).
* Ending a program's input: in pipe mode, where Ctrl+D is an ordinary byte, a
  program that reads until the end of its input can then finish on its own,
  and stdin from a file builds on it. This adds a close-input operation to
  `b2c-process` sessions (pipe mode closes the input pipe, PTY mode sends the
  terminal's end-of-file character) and an `eof` flag or a close-input
  command to the IPC contract.
* Quick fixes on diagnostics, applied as one undo step. *Copy bug report* for
  suspected generator bugs. Feature gating by C++ standard and toolchain
  capability, with fallbacks. A snapshot test of every catalog block's
  default C++, compiled with `-fsyntax-only`.
* `switch` (with fall-through), `do … while`, `wait`, *with arguments* on
  *when program starts* (a checkbox, not a second block), overloads, default
  arguments and the *moved* parameter mode. Type-specific variable
  operations, and with *Show advanced blocks* the bitwise compound
  assignments. The project name in the project settings dialog.
* The type and slot foundations: an open type representation, typed slots,
  catalog schema extensions (including the catalog's `headers`),
  `text + "x"` where C++ allows it, and constant folding for overflow and
  division by zero.
* From the M2 polish wave (owner's decisions of 2026-10-10):
  * the live preview in a Web Worker (ADR-0016, after a single `update()`
    export), with a compact source map and a C++ panel that pauses while
    hidden;
  * the small-program benchmarks gated, once the toolbox refreshes per
    section and a baseline has been measured;
  * Blockly 13, continuous-toolbox 13 and xterm.js 6.1 with its add-ons,
    after a spike that also checks which workarounds the
    [upstream reports](../plans/upstream/README.md) let go; the
    patched Blockly render management and a linear `getDescendants`, if
    [ADR-0014](../adr/0014-extending-blockly.md) and the measurement allow
    it;
  * lowering parked stacks for the scope query and block types
    ([ADR-0012](../adr/0012-parked-blocks.md));
  * the Appearance setting (System, Light, Dark) after a `[data-theme]`
    token refactor, with the console's colours following it, and, if still
    wanted, *Clear the console when a run starts*;
  * `pnpm doctor`, and a weekly job that follows `BUILDING.md`'s Debian
    steps in Debian 12 and 13 containers;
  * Run reusing the build's analysis, removing unused sandbox folders,
    newer `g++-N` names, and the skipped-lines marker on Windows;
  * every polish item moved past the cut line.
* **Exit:** a typical intro-CS course's assignments (I/O, loops, functions,
  vectors, structs, files) can be completed. Each has a golden test.

### M4: Language breadth II

* Classes (inheritance, virtual, operators, special members), lambdas,
  memory and smart pointers, templates and constraints, concurrency.
* Library-pack system (the `std` pack complete), library profiles, generic
  member block, Raw C++ blocks. First decided: whether building a project
  created in the app asks for trust once it holds Raw C++ or libraries
  ([08 §8.3.1](08-security.md#831-how-trust-is-decided)).
* Machine-local extra compiler and linker flags (one flag per row, checked
  against the denylist, each change confirmed in a native dialog the backend
  raises) and the environment pass-through list
  ([07 §7.4.5](07-toolchain-build-run.md#745-machine-local-extra-flags-advanced),
  [ADR-0005](../adr/0005-no-compiler-flags-in-projects.md)).
* **Exit:** the C++ coverage matrix
  ([03 §3.12](03-block-language.md#312-c-coverage-matrix)) is fully
  green for 1.0 rows, and a sample SFML/raylib project builds through a
  library profile.

### M5: Organisation, debugging and polish

* Outline, frames, search, rename, go-to-definition, snippets, minimap and
  tidy-up.
* GDB debugger with block stepping. Trace (glow) mode. Runtime error mapping.
* Accessibility audit and fixes, i18n infrastructure, and performance targets
  (N2–N5 and N12) met.
* User guide and tutorials (beyond M2's getting-started page), and the
  offline help system.
* Find references, back/forward history and bookmarks. Collapse to
  signatures. A command palette and remappable shortcuts. Focus and
  Presentation modes. Dock layout (swap, split). High Contrast themes (light
  and dark) and a checked colour-blind-safe category palette. The window's
  size and position remembered. Non-drag ways to place blocks, including a
  keyboard equivalent of wrapping statements in a C-block.
* A build is up to date when its generated files are identical
  ([07 §7.5.1](07-toolchain-build-run.md#751-build-directory)), so editing a
  parked block no longer recompiles.
* Code style options in the generator (braces, indentation, pointer
  alignment, line width) and lint levels (in the project file, overridden
  per machine; Q9), with a malicious-project test that a project file cannot
  lower `W0520`. Build and run settings
  (compile timeout, cache size, Windows link mode, memory and process caps
  for runs). With a process cap, a program in a terminal that hits the
  scope's `TasksMax` and ends at once with all its processes (the terminal
  hangs them up) must still be reported as over the limit: today the scope
  can be gone before its `pids.events` is read (watch it continuously, or
  keep the scope alive until it has been read). The start-page example
  gallery and full template set.
  *Help → Export diagnostics bundle*.

### M6: 1.0 hardening and release

* External security review. All residual items triaged.
* Signing, updater, installers and packaging for Windows and Linux.
  Experimental sandboxed run (Linux bubblewrap).
* Linux packages built against glibc 2.35, and install smoke tests on the
  oldest supported systems (N1). Installer size under 30 MB (N6).
  Reproducible release builds (`SOURCE_DATE_EPOCH`, `--remap-path-prefix`).
  Release channels: nightly, beta and stable.
* Trusted Types enforced on WebView2 when the trial reports no violations
  for a milestone, or an ADR records why not.
* Beta programme with learners and teachers. Release gate passed. **1.0**.

### Post-1.0 candidates

Clang support · macOS · verified toolchain installer (pinned URLs + SHA-256 +
signatures) · more library packs (SFML, raylib, Dear ImGui) · Windows
AppContainer sandbox · import of C++ declarations from headers · classroom
features (assignment templates, CLI grading reports) · Flatpak · C++ modules
once GCC support matures · a toolbox block-size setting (small, medium,
large) · a deprecation period for block types after 1.0 (badge and automatic
replacement; before 1.0 an exact replacement retires a type at once, owner's
decision of 2026-10-10) · evaluate and adopt Tauri 3 once it is stable and
has had a point release with no open security advisories. Tauri is confined
to the desktop adapter (`apps/desktop/src-tauri`) and the frontend's IPC
transport (`apps/desktop/src/lib/ipc.ts`), so the work is mainly
re-verifying the security configuration of
[08 §8.8](08-security.md#88-webview-and-ipc-hardening)
(CSP, capabilities and permissions, the isolation hook, navigation locking),
updating the adapter, the Linux webview packages and the E2E driver, and a
full manual test pass on both OSes. A newer Linux webview stack would also
drop glib 0.18 and its tracked advisory (`osv-scanner.toml`).

## 10.2 Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Blockly performance in WebKitGTK on large workspaces | Medium | Medium | The first readings, taken locally on Linux under a virtual display without a GPU, are above N3 and N4: the preview task at 1,000 blocks about 120 ms at p95 (the native pipeline alone about 19 ms), and dragging at 5,000 blocks about 50 ms per frame at p95. CI's readings for dragging at 5,000 blocks (p95) are 40 ms on Linux and 914 ms on Windows. In the app (2026-10-10, debug backend, Linux without a GPU): selecting 1.2 s, an edit at the end of `main` 0.85 s, dragging a 1,000-block stack 14 s in all; a toolbox rebuild 0.13–0.17 s at 40 blocks. Benchmarked on 2026-10-12/13 (debug backend, end-to-end frontend build, after the preview churn was reduced): the longest task after a click at 1,000 blocks about 1.2 s (the toolbox's rebuild about 0.55 s of it), an edit until the page settles with the C++ panel shown about 1.4 s at p95, the preview task about 100 ms at p95 (120 ms before P-2). Mitigation: gated benchmarks with A/B runs ([09 §9.2](09-quality-and-delivery.md#92-testing-strategy)); a patched and upstreamed render management and a linear `getDescendants` (M3, if the measurement justifies the patch under [ADR-0014](../adr/0014-extending-blockly.md)); the preview in a Web Worker ([02 §2.6](02-architecture.md#26-process-model-and-concurrency), M3); per-function views (M5). Also per-module canvases and collapse/virtualisation. Fallback: recommend splitting modules. |
| Rendering differences between WebView2 and WebKitGTK | Medium | Low | E2E on both, a visual-diff test of the core canvas, avoid cutting-edge CSS |
| Type-checker false positives frustrate users | Medium | High | Gradual typing; only report certain errors; defer to g++; a differential test oracle |
| Windows toolchain variety (broken PATHs, mixed runtimes) | High | Medium | Strong probing, health checks, clear guided setup, a wide manual test matrix |
| "Anything C++ can do" causes scope creep | High | Medium | The coverage matrix is the contract. Raw C++ and packs cover the long tail. New native blocks need a catalog proposal. |
| Tauri Isolation pattern / CSP friction with Blockly | Medium | Low | Prototype in M0. Documented exceptions (`style-src 'unsafe-inline'`). |
| Blockly, its plugins and xterm.js move to new majors faster than the app | High | Medium | Separate Dependabot pull requests, one scheduled migration per milestone, small subclasses listed under [ADR-0014](../adr/0014-extending-blockly.md) and covered by E2E and visual tests |
| Dependency compromise (npm ecosystem) | Medium | High | §8.9 controls: minimum release age, build-script allowlist, minimal dependencies, scanners |
| Users trust malicious shared projects | Medium | High | Clear trust UX, Raw C++ visibility, Mark-of-the-Web warnings, sandbox option post-1.0 |

## 10.3 Product decisions

Decisions marked **Decided** were made by the project owner. **Proposed**
items are the defaults we build toward until the owner says otherwise.

| # | Question | Decision / proposed default | Status |
|---|----------|-----------------------------|--------|
| Q1 | **License** for the project | **Apache-2.0** (includes a patent grant; matches Blockly). See [`LICENSE`](../../LICENSE). | Decided 2026-10-02 |
| Q2 | **Product name** | **Blocks2Cpp**, with the short form `b2c` for the CLI, crates and C++ namespace | Decided 2026-10-02 |
| Q3 | Default C++ standard for new projects | **C++20** (supported by every toolchain we accept, GCC 11+) | Decided 2026-10-04 |
| Q4 | Offer *"use namespace std"* beginner mode by default? | **No.** Explicit `std::` teaches real-world style. There is a per-project toggle (*Textbook style*) and a `use namespace` block for one module or function, with exact name rules ([ADR-0006](../adr/0006-using-namespace.md)). | Decided 2026-10-04 |
| Q5 | Bundle or download a toolchain on Windows in 1.0? | **No.** Guided setup only; a verified installer helper is a post-1.0 candidate. | Decided 2026-10-02 |
| Q6 | Minimum Windows version | **Windows 10 1809** (ConPTY, WebView2). To be revisited as support for Windows 10 ends (WebView2, MSYS2 and GitHub's runners). | Decided 2026-10-04 |
| Q7 | Update checks | **Opt-in**, asked on first run | Decided 2026-10-04 |
| Q8 | Primary audience emphasis (learners vs. power users) for default UI | **Learner-first** defaults (friendly labels, advanced hidden), one switch to expert mode | Decided 2026-10-04 |
| Q9 | Where are lint levels stored? | **In the project file** (`project.lints`), **overridable in each user's machine settings**; the machine value wins when both are set. Errors cannot be changed. See [06 §6.6](06-compiler-pipeline.md#66-stage--types-flow-checks-and-lints). | Decided 2026-10-04 |
| Q10 | Are loose blocks errors? | **No:** they are parked, warned (`B2C-W0505`) and never generated ([ADR-0012](../adr/0012-parked-blocks.md)). | Decided 2026-10-10 |
| Q11 | Preview off the main thread before M5? | **Yes, in M3:** the compiler core's update runs in a Web Worker (ADR-0016, written before that work starts). | Decided 2026-10-10 |
| Q12 | Does the Variables listing depend on the selected block? | **No:** it lists every variable of the module, grouped by the function that declares it, and is rebuilt only when variables or functions change. | Decided 2026-10-10 |
| Q13 | Permissions of new project files | **The user's umask** (usually readable by others); an existing file keeps its permissions, and machine-local files and recovery snapshots stay owner-only (`0600`). | Decided 2026-10-10 |
