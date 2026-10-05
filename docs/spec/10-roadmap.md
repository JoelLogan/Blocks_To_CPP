# 10. Roadmap, Risks and Product Decisions

> Status: **Draft v0.1**

## 10.1 Milestones

> **Implementation status (2026-10-04).** **M1** is done: loading and
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
> skeletons and markdownlint come with M2, where they are first needed.

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
* Dynamic Variables and My Blocks categories, and symbol dropdowns filtered
  by scope. A type-aware connection checker. Copy and paste through the
  validated clipboard format. External change detection for open projects
  (Reload or Keep mine).
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
* **Exit:** a first-time user builds and runs the guessing game
  ([03 §3.13.1](03-block-language.md#3131-guessing-game-input-random-loops-branching))
  without reading docs. E2E tests cover the flow on both OSes.

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
  directory, stdin file, run in an external terminal).
* Quick fixes on diagnostics, applied as one undo step. *Copy bug report* for
  suspected generator bugs. Feature gating by C++ standard and toolchain
  capability, with fallbacks. A snapshot test of every catalog block's
  default C++, compiled with `-fsyntax-only`.
* **Exit:** a typical intro-CS course's assignments (I/O, loops, functions,
  vectors, structs, files) can be completed. Each has a golden test.

### M4: Language breadth II

* Classes (inheritance, virtual, operators, special members), lambdas,
  memory and smart pointers, templates and constraints, concurrency.
* Library-pack system (the `std` pack complete), library profiles, generic
  member block, Raw C++ blocks.
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
  (N2–N5) met.
* User guide and tutorials, and the offline help system.
* Find references, back/forward history and bookmarks. Collapse to
  signatures. A command palette and remappable shortcuts. Focus and
  Presentation modes. Dock layout (swap, split, remembered). Light, Dark and
  High Contrast themes with a checked colour-blind-safe category palette.
* Code style options in the generator (braces, indentation, pointer
  alignment, line width) and lint levels (in the project file, overridden
  per machine; Q9), with a malicious-project test that a project file cannot
  lower `W0520`. Build and run settings
  (compile timeout, cache size, Windows link mode, memory and process caps
  for runs). The start-page example gallery and full template set.
  *Help → Export diagnostics bundle*.

### M6: 1.0 hardening and release

* External security review. All residual items triaged.
* Signing, updater, installers and packaging for Windows and Linux.
  Experimental sandboxed run (Linux bubblewrap).
* Linux packages built against glibc 2.35, and install smoke tests on the
  oldest supported systems (N1). Installer size under 30 MB (N6).
  Reproducible release builds (`SOURCE_DATE_EPOCH`, `--remap-path-prefix`).
  Release channels: nightly, beta and stable.
* Beta programme with learners and teachers. Release gate passed. **1.0**.

### Post-1.0 candidates

Clang support · macOS · verified toolchain installer (pinned URLs + SHA-256 +
signatures) · more library packs (SFML, raylib, Dear ImGui) · Windows
AppContainer sandbox · import of C++ declarations from headers · classroom
features (assignment templates, CLI grading reports) · Flatpak · C++ modules
once GCC support matures · a deprecation path for block types (badge and
automatic replacement; until a block type is removed, the migration chain is
enough) · evaluate and adopt Tauri 3 once it is stable and has had a point
release with no open security advisories. Tauri is confined to the desktop
adapter (`apps/desktop/src-tauri`) and the frontend's IPC transport
(`apps/desktop/src/lib/ipc.ts`), so the work is mainly re-verifying the
security configuration of [08 §8.8](08-security.md#88-webview-and-ipc-hardening)
(CSP, capabilities and permissions, the isolation hook, navigation locking),
updating the adapter, the Linux webview packages and the E2E driver, and a
full manual test pass on both OSes. A newer Linux webview stack would also
drop glib 0.18 and its tracked advisory (`osv-scanner.toml`).

## 10.2 Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Blockly performance in WebKitGTK on large workspaces | Medium | Medium | Early benchmarks in M2, per-module canvases, collapse/virtualisation, Web Worker analysis. Fallback: recommend splitting modules. |
| Rendering differences between WebView2 and WebKitGTK | Medium | Low | E2E on both, a visual-diff test of the core canvas, avoid cutting-edge CSS |
| Type-checker false positives frustrate users | Medium | High | Gradual typing; only report certain errors; defer to g++; a differential test oracle |
| Windows toolchain variety (broken PATHs, mixed runtimes) | High | Medium | Strong probing, health checks, clear guided setup, a wide manual test matrix |
| "Anything C++ can do" causes scope creep | High | Medium | The coverage matrix is the contract. Raw C++ and packs cover the long tail. New native blocks need a catalog proposal. |
| Tauri Isolation pattern / CSP friction with Blockly | Medium | Low | Prototype in M0. Documented exceptions (`style-src 'unsafe-inline'`). |
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
