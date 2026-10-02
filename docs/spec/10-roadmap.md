# 10. Roadmap, Risks and Open Questions

> Status: **Draft v0.1**

## 10.1 Milestones

Each milestone ends with a demo, a threat-model review
([08 §8.13](08-security.md#813-continuous-security-process)) and updated
documentation.

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
* **Exit:** a first-time user builds and runs the guessing game
  ([03 §3.13.1](03-block-language.md#3131-guessing-game-input-random-loops-branching))
  without reading docs. E2E tests cover the flow on both OSes.

### M3: Language breadth I

* Expression slots and Quick Insert. Type picker. Collections, strings,
  structs, enums, files, errors, random and time.
* Multi-module projects with headers. Export to CMake/Make. Friendly
  message catalog for g++ errors. Support helpers.
* **Exit:** a typical intro-CS course's assignments (I/O, loops, functions,
  vectors, structs, files) can be completed. Each has a golden test.

### M4: Language breadth II

* Classes (inheritance, virtual, operators, special members), lambdas,
  memory and smart pointers, templates and constraints, concurrency.
* Library-pack system (the `std` pack complete), library profiles, generic
  member block, Raw C++ blocks.
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

### M6: 1.0 hardening and release

* External security review. All residual items triaged.
* Signing, updater, installers and packaging for Windows and Linux.
  Experimental sandboxed run (Linux bubblewrap).
* Beta programme with learners and teachers. Release gate passed. **1.0**.

### Post-1.0 candidates

Clang support · macOS · verified toolchain installer (pinned URLs + SHA-256 +
signatures) · more library packs (SFML, raylib, Dear ImGui) · Windows
AppContainer sandbox · import of C++ declarations from headers · classroom
features (assignment templates, CLI grading reports) · Flatpak · C++ modules
once GCC support matures.

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

## 10.3 Open questions (owner decisions needed)

| # | Question | Proposed default |
|---|----------|------------------|
| Q1 | **License** for the project | Apache-2.0 (patent grant; matches Blockly). MIT is the simpler alternative. |
| Q2 | **Product name** ("Blocks to C++" is a working name) | Keep it until branding work |
| Q3 | Default C++ standard for new projects | C++20 (supported by every toolchain we accept, GCC 11+) |
| Q4 | Offer *"use namespace std"* beginner mode by default? | No. Explicit `std::` teaches real-world style; there is a per-project toggle. |
| Q5 | Bundle or download a toolchain on Windows in 1.0? | No. Guided setup only; a verified installer comes post-1.0. |
| Q6 | Minimum Windows version | Windows 10 1809 (ConPTY, WebView2) |
| Q7 | Update checks | Opt-in, asked on first run |
| Q8 | Primary audience emphasis (learners vs. power users) for default UI | Learner-first defaults (friendly labels, advanced hidden), one switch to expert mode |
