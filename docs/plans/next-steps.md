# Next steps: closing M2, then M3

Status: plan, 2026-10-15. Part 1 is what remains of M2, almost all of it the
owner's. Part 2 is a draft plan for M3, for review before any M3 work
starts. Nothing in Part 2 begins until M2 is closed (or the owner says
otherwise).

## 1. Where M2 stands

Every item of the [M2 polish plan](m2-polish-plan.md) that can be done
without the owner is done, reviewed and on the branch
`ccr-01f82d9b-v70u20`, with CI, the Desktop workflow (E2E on Linux and
Windows) and the weekly run green. The last additions:

- the window's chrome in the visual diff (W7-1): 15 scenes per theme, light
  on both systems and dark on Linux, all taken on both CI systems;
- Release builds no longer warn with Ubuntu's GCC 14 (found running the
  toolchain matrix in a container, W7-5b);
- the threat-model review's TM-M2-4 (Trusted Types: no violation in 48 E2E
  tests on WebView2) and TM-M2-5 (the Windows security specs, 31 of 31)
  closed;
- `BUILDING.md` and `CONTRIBUTING.md` corrected after a run on a clean clone
  (W6-3).

## 2. Closing M2: the owner's part

In this order: the decisions first (they change what the sessions test),
then the baselines (they expire), then the release-build work.

| #   | Item                                    | What to do                                                                                                                                                                                                                                                           | Where                                                                                                              | Time   |
| --- | --------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ | ------ |
| 1   | D13 follow-up: the wrapping rule        | Approve the _top handle_ (a C-block dropped with its top at a statement wraps it and what follows), or reject it, which moves wrapping to M3                                                                                                                         | [plan, D13](m2-polish-plan.md#d13-how-should-c-blocks-wrap-existing-statements-u8)                                 | 10 min |
| 2   | D19: the edit benchmark's gate          | Choose the per-edit median, a p95 over all of a run's edits, or keep the p95 of 10                                                                                                                                                                                   | [plan, W4-2](m2-polish-plan.md#w4--block-set-variables-and-parked-blocks)                                          | 5 min  |
| 3   | W7-2: visual baselines                  | Download `visual-candidates-ubuntu-24.04` and `visual-candidates-windows-2025` from Desktop run 38807356226 (or any later green run; artifacts expire after 14 days), look at the 47 images, commit them under `apps/desktop/e2e/visual/baselines/<linux\|windows>/` | [`e2e/visual/README.md`](../../apps/desktop/e2e/visual/README.md)                                                  | 1 h    |
| 4   | Release build, U1–U18 re-check          | `pnpm app:build` on Debian; check each of your 18 observations against the build                                                                                                                                                                                     | [`BUILDING.md`](../../BUILDING.md), [plan §3](m2-polish-plan.md)                                                   | 2 h    |
| 5   | W7-5a: toolchain cell L1                | Debian's g++, seven steps, in the record table; mark the other cells run or deferred to M3                                                                                                                                                                           | [`m2-toolchains.md`](../manual-tests/m2-toolchains.md)                                                             | 1 h    |
| 6   | W2-2: profile                           | The profile protocol on your machine                                                                                                                                                                                                                                 | [`m2-performance-profile.md`](../manual-tests/m2-performance-profile.md)                                           | 1 h    |
| 7   | W7-6: usability sessions                | The sessions, then the NVDA/Orca pass and the demo exclusion review                                                                                                                                                                                                  | [`m2-usability.md`](../manual-tests/m2-usability.md), [`m2-accessibility.md`](../manual-tests/m2-accessibility.md) | days   |
| 8   | Dependabot #5 and #6                    | criterion 0.8 and rfd 0.17: say "apply" and I update the branch, adapt the code and run the dialog E2E on both systems; or keep holding                                                                                                                              | GitHub                                                                                                             | 5 min  |
| 9   | F-8: issue tracker and upstream reports | Say when to start the labels and issues (W0-7) and to file the Blockly drafts                                                                                                                                                                                        | [`upstream/`](upstream/README.md)                                                                                  | 5 min  |
| 10  | The default branch                      | When M2 is signed off: ask for a pull request from `ccr-01f82d9b-v70u20`; I open it, you merge                                                                                                                                                                       | GitHub                                                                                                             | 10 min |

What I do with each result:

- **Decisions (1, 2, 8, 9):** apply them on the branch the same day, with the
  spec, plan and changelog updated and CI green.
- **Baselines (3):** check that both systems' visual steps now compare and
  pass, and that a throwaway colour change fails them (the W7-2 check), then
  correct 09 §9.3 and 10 §10.1.
- **Findings (4–7):** each becomes a row in the plan. The M2 exit rule
  decides where it goes: a P0 or a regression is fixed before M2 closes
  (a short M2.1 wave, same process as the polish wave); anything else is
  fixed then or moved to M3 with your agreement.
- **Sign-off:** the roadmap's M2 exit checklist with the evidence for each
  line, then the pull request (10).

## 3. M3 plan (draft)

### 3.1 Goal and exit

M3 is _Language breadth I_ ([10 §10.1](../spec/10-roadmap.md#m3-language-breadth-i)).
Exit: a typical intro-CS course's assignments (I/O, loops, functions,
vectors, structs, files) can be completed, each with a golden test.

The exit is only checkable once the assignments are chosen, so choosing them
is the first piece of M3 work (M3-D1), and they become the acceptance tests
that drive the waves.

### 3.2 Scope

From the roadmap, grouped by where the work lands:

- **Compiler foundations** (Rust, `b2c-lang`, `b2c-catalog`, `b2c-codegen`):
  an open type representation (L14, XL), typed slots (L15, XL), catalog
  schema extensions with the catalog's `headers` (L17, L8), `text + "x"`
  where C++ allows it (L16), constant folding for overflow and division by
  zero (L10).
- **Language blocks:** collections (vector, deque, set, map, queue, stack,
  priority queue; [03 §3.7.8](../spec/03-block-language.md#378-collections)),
  strings, structs and enums (§3.7.9; classes stay in M4), files (§3.7.10),
  errors (§3.7.11), random and time, global variables and constants, math
  functions, text and number conversions, organisation blocks (namespace,
  use header, use namespace, `static_assert`) and the `using namespace std`
  setting (Q4); `switch`, `do … while`, `wait`, _with arguments_ on _when
  program starts_, overloads, default arguments, the _moved_ parameter mode,
  type-specific variable operations and the bitwise compound forms (M3-8).
- **Projects, build and run:** multi-module projects with headers (L18),
  export to CMake and Make, a project settings dialog, run options
  (arguments, working directory, stdin from a file, an external terminal),
  ending a program's input (a close-input operation in `b2c-process` and the
  IPC contract), `run_start` reusing the build's analysis (S7-5), pruning
  sandbox folders (S7-7), open-ended `g++-N` names (S7-8).
- **Editor:** Quick Insert ([03 §3.14](../spec/03-block-language.md#314-quick-insert-type-to-block)),
  the type picker, toolbox search, the expert-mode switch (Friendly and C++
  labels, _Show advanced blocks_; Q8), quick fixes as one undo step,
  _Copy bug report_, feature gating by standard and toolchain, the friendly
  message catalog for g++ errors, the Appearance setting (F-1, with the
  console's colours, U14d).
- **Performance and platform (from M2):** the preview in a Web Worker
  (M3-1, a new ADR-0016), the compact source map and a C++ panel that pauses
  while hidden (M3-5), lowering parked stacks for the scope query (M3-4),
  the small-program budgets gated (M3-9), Blockly 13, continuous-toolbox 13
  and xterm.js 6.1 after a spike (M3-3), the render patch only if ADR-0014
  and the measurement allow it (M3-2), `pnpm doctor` (M3-6), a weekly job
  that follows `BUILDING.md` in Debian 12 and 13 containers (M3-7), the
  skipped-lines marker on Windows (RD-15).
- **Quality and security:** a snapshot test of every catalog block's default
  C++ compiled with `-fsyntax-only`; the M3 threat-model review (the
  worker's CSP and scope index, files and paths a program may touch, the
  Trusted Types table of the security specs); an npm licence gate
  (TM-M2-6); the toolchain matrix cells deferred from M2.

### 3.3 Decisions before M3 starts

Each with a recommendation; none is made until the owner answers.

| ID    | Question                             | Recommendation                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| ----- | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| M3-D1 | Which assignments define "intro-CS"? | About 10 classic ones, each an example project with a golden test: temperature table (loops, formatting), grade average and maximum (vector), word and line count of a file (files, strings), student records sorted by mark (struct, sort), menu with `switch` and an enum, palindrome and string reversal, dice statistics (random, map), number guessing with retries (exists), matrix sum (vector of vectors), input validation with exceptions (errors) |
| M3-D2 | Which collections in M3?             | All of §3.7.8's table, since the polymorphic phrase blocks cost little once the type representation exists; the generic member block (`.`) last, if time                                                                                                                                                                                                                                                                                                     |
| M3-D3 | Files: what may a program touch?     | Any path the program names, as now (the program runs as the user); the editor offers relative paths under the project folder, and the threat-model review records it                                                                                                                                                                                                                                                                                         |
| M3-D4 | Expert mode by default?              | Off (Friendly labels); the switch is a machine setting, not a project setting                                                                                                                                                                                                                                                                                                                                                                                |
| M3-D5 | External terminal on Linux           | Use `x-terminal-emulator` when present, else a list of known terminals; Windows uses `conhost`; Run in the app stays the default                                                                                                                                                                                                                                                                                                                             |
| M3-D6 | Budget and cut line                  | As in M2: a must-have set (the foundations, the blocks the assignments need, multi-module, run options) and an if-time set that moves to M4 without sign-off when the budget is spent                                                                                                                                                                                                                                                                        |
| M3-D7 | Blockly 13 timing                    | A 2-day spike first (W0); upgrade before the editor wave only if the spike is clean, otherwise after M3                                                                                                                                                                                                                                                                                                                                                      |

### 3.4 Waves

Each wave works as M2's did: contracts first, packages in parallel
worktrees, one integrator, review lenses with adversarial verification, a
fixer, then Linux E2E locally and Windows through CI before anything is
pushed.

| Wave | Content                                                                                                                                                                                                                                                  | Depends on                     | Size |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------ | ---- |
| W0   | Decisions above; ADR-0016 (Worker) and ADRs for the type representation (L14), slot typing (L15) and the header split (L18); the catalog schema extension (L17); the M3-D1 assignments written as failing golden tests; the Blockly 13 / xterm 6.1 spike | —                              | M    |
| W1   | Foundations: L14, L15, L17 with `headers` (L8), L16, L10; the catalog snapshot test                                                                                                                                                                      | W0 ADRs                        | XL   |
| W2   | Language blocks needed by the assignments: collections, strings, structs, enums, files, errors, random, time, globals, constants, math, conversions, `switch`, `do … while`, overloads, default arguments, M3-8                                          | W1                             | XL   |
| W3   | Projects, build, run: multi-module (L18), CMake/Make export, project settings dialog, run options, close-input, S7-5, S7-7, S7-8                                                                                                                         | W1 (L18 ADR)                   | L    |
| W4   | Editor: type picker, Quick Insert, toolbox search, expert mode, quick fixes, _Copy bug report_, feature gating, friendly g++ messages, Appearance (F-1, U14d)                                                                                            | W1, W2 for the blocks it shows | L    |
| W5   | Performance and platform: Worker preview (M3-1), M3-5, M3-4, M3-9, the Blockly 13 upgrade if W0 cleared it, M3-6, M3-7, RD-15                                                                                                                            | W2 (stable `update()`)         | L    |
| W6   | Close-out: every M3-D1 assignment green, threat-model review, toolchain cells, docs and the getting-started guide, usability protocol for M3, a green nightly and weekly                                                                                 | all                            | M    |

W2 and W3 can run in parallel once W1's contracts land; W4 starts with the
pieces that need no new blocks (expert mode, toolbox search, Appearance).

### 3.5 Critical path and risks

- **The type and slot work (L14, L15) is the critical path.** Every new
  block depends on it. Mitigation: W0 writes the ADRs and the contracts
  (types in `b2c-ir`, the catalog schema) before code, and W1's packages
  are split by crate so they run in parallel.
- **Blockly 13** may break the subclasses ADR-0014 lists. Mitigation: the
  spike in W0, the upgrade only if it is clean, and the visual diff (with
  baselines from W7-2) catches what the E2E flows do not.
- **Preview speed with more blocks:** the edit and preview benchmarks are
  gated; the Worker (M3-1) moves the work off the main thread, and the
  small-program budgets (M3-9) are gated once measured.
- **Windows behaviour** of close-input (ConPTY), the external terminal and
  file paths: each gets a Windows E2E flow from the start, not at the end.
- **Generated C++ correctness** across GCC 11 to 15: the `-fsyntax-only`
  snapshot test runs in CI on GCC 11, 13 and 15, and the every-g++ build
  test added in M2 covers the local side-by-side compilers.

### 3.6 First steps once M3 is approved

1. Ask the M3-D questions in one batch, with the recommendations above.
2. Write the M3-D1 assignments as example projects whose golden tests fail
   until W2, so progress is visible as tests turning green.
3. Draft the four ADRs (Worker, types, slots, headers) for review.
4. Run the Blockly 13 / xterm 6.1 spike and record it.
5. Then W1, with the plan's must-have and if-time sets agreed.
