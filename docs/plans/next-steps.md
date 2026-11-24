# Next steps: closing M2, then M3

Status: plan, 2026-10-15; updated 2026-10-24 with the owner's decisions,
on 2026-10-25: M2 is merged into `main`, M3 has begun at the owner's word
(Part 2, §3.6), and its wave W1 is done (§3.7); and on 2026-10-28: wave W2
is done, the ten assignments are green, and three slices of W3, W4 and W5
are merged (§3.8). Part 1 is what remains of M2, almost all of it the
owner's. Part 2 is the plan for M3.

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
- on 2026-10-24: the owner's decisions recorded (D13, D19, M3-D1 to D7), D19
  implemented, every open Dependabot update applied under the release-age
  policy, the advisories fixed (one high, `happy-dom`, dev only, is a dated
  exception until its fix is old enough), a format-string bug in the Linux
  native dialogs fixed (TM-M2-9), and an intermittent E2E failure of the
  `selection` flow root-caused (the test read the toolbox in three calls a
  rebuild could fall between) and fixed.
- on 2026-10-24: pull request #30 merged the branch into `main`
  (`22c2532`), with every check green, and the owner made `main` the default
  branch and let the `github-pages` environment deploy from it. Dependabot,
  the scheduled Nightly, Weekly and Scorecard runs and the Pages deploy now
  follow `main`. The Actions cache is kept per branch, so Nightly's
  benchmark history and the kept fuzz corpora start again on `main`: the
  first five complete scheduled runs only report, the gates apply from the
  sixth, and the fuzzers grow their corpora again from the seeds.

## 2. Closing M2: the owner's part

In this order: the decisions first (they change what the sessions test),
then the baselines (they expire), then the release-build work.

| #   | Item                                    | What to do                                                                                                                                                                                                                                                                                                              | Where                                                                                                              | Time   |
| --- | --------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ | ------ |
| 1   | D13 follow-up: the wrapping rule        | Approve the _top handle_ (a C-block dropped with its top at a statement wraps it and what follows), or reject it, which moves wrapping to M3                                                                                                                                                                            | [plan, D13](m2-polish-plan.md#d13-how-should-c-blocks-wrap-existing-statements-u8)                                 | 10 min |
| 2   | D19: the edit benchmark's gate          | Choose the per-edit median, a p95 over all of a run's edits, or keep the p95 of 10                                                                                                                                                                                                                                      | [plan, W4-2](m2-polish-plan.md#w4--block-set-variables-and-parked-blocks)                                          | 5 min  |
| 3   | W7-2: visual baselines                  | Download `visual-candidates-ubuntu-24.04` and `visual-candidates-windows-2025` from Desktop run 38807356226 (or any later green run; artifacts expire after 14 days), look at the 47 images, commit them under `apps/desktop/e2e/visual/baselines/<linux\|windows>/`                                                    | [`e2e/visual/README.md`](../../apps/desktop/e2e/visual/README.md)                                                  | 1 h    |
| 4   | Release build, U1–U18 re-check          | `pnpm app:build` on Debian; check each of your 18 observations against the build                                                                                                                                                                                                                                        | [`BUILDING.md`](../../BUILDING.md), [plan §3](m2-polish-plan.md)                                                   | 2 h    |
| 5   | W7-5a: toolchain cell L1                | Debian's g++, seven steps, in the record table; mark the other cells run or deferred to M3                                                                                                                                                                                                                              | [`m2-toolchains.md`](../manual-tests/m2-toolchains.md)                                                             | 1 h    |
| 6   | W2-2: profile                           | The profile protocol on your machine                                                                                                                                                                                                                                                                                    | [`m2-performance-profile.md`](../manual-tests/m2-performance-profile.md)                                           | 1 h    |
| 7   | W7-6: usability sessions                | The sessions, then the NVDA/Orca pass and the demo exclusion review                                                                                                                                                                                                                                                     | [`m2-usability.md`](../manual-tests/m2-usability.md), [`m2-accessibility.md`](../manual-tests/m2-accessibility.md) | days   |
| 8   | Dependabot #5 and #6                    | criterion 0.8 and rfd 0.17: say "apply" and I update the branch, adapt the code and run the dialog E2E on both systems; or keep holding                                                                                                                                                                                 | GitHub                                                                                                             | 5 min  |
| 9   | F-8: issue tracker and upstream reports | Say when to start the labels and issues (W0-7) and to file the Blockly drafts                                                                                                                                                                                                                                           | [`upstream/`](upstream/README.md)                                                                                  | 5 min  |
| 10  | The default branch                      | When M2 is signed off: ask for a pull request from `ccr-01f82d9b-v70u20`; I open it, you merge                                                                                                                                                                                                                          | GitHub                                                                                                             | 10 min |
| 11  | D20: the nightly benchmark gate         | Nightly was red from 2026-10-16 to 2026-10-24 at one commit, mostly because its benchmark history only takes runs that pass: choose (A) record every run and fail on 2 of 3 nights, (B) nightly report-only with A/B runs as the gate, or (C) compare only runs on the same processor; and (D), dragging gated like D19 | [plan, D20](m2-polish-plan.md#d20-how-should-the-nightly-benchmark-gate-deal-with-the-runners-variance)            | 10 min |

**Status (2026-10-24):** 1, D13: approved (the top handle stays). 2, D19:
the p95 over all the edits of a run, as long as it does not slow the testing
down; done (merged in `095e62b`): `webview.edit-1000.p95-all` is gated, from
the same edits, so the benchmark takes no longer. 3: the owner committed the
47 baselines (`81100ec`); both systems' visual steps compare and pass. 8:
approved, with every other open Dependabot update and the security findings;
done (`44fe162`..`3cf40a2`), with a new finding fixed (TM-M2-9, below). The
pull request (10): at the owner's word, `main` was made at `43a1389` (the
last commit of M1), and the pull request goes from `ccr-01f82d9b-v70u20` into
it; merged on 2026-10-24 (#30). 11, D20: Nightly
was red from 2026-10-16 to 2026-10-24 (the `selection` race, fixed; one
Windows driver race; and the benchmark gate's history); decided (A) with (D)
and done (`f4c152c`, `9fea7cc`, `30e3549`): every complete scheduled run
enters the benchmark history, a gated metric fails only when it is over the
gate in 2 of the last 3 scheduled runs (the first warns), and dragging is
gated on `webview.drag-5000.frame-p95-all`, the p95 over every frame of a
run's timed drags, whose history starts with its first nightly run. 4–7 and
9 remain.

**Dated follow-ups** (held by the 7-day release age or by an upstream
release; each is a small commit with its checks, no decision needed):

| From       | Update                                                                                                                 | Then                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 2026-11-11 | `tauri-plugin-dialog` 2.8.4 (moves to `rfd` ^0.17; old enough since 2026-11-04)                                        | Not yet: `rfd` 0.17.2 (2026-01-12) is still the newest 0.17 release (checked 2026-11-04), and its GTK message dialog still hands the text to `gtk_message_dialog_format_secondary_text` as the format (`src/backend/gtk3/message_dialog.rs`:112; only the title goes after `"%s"`), so TM-M2-9 returns and the `rfd` tripwire in `dialogs.rs` fails. Take it with an `rfd` 0.17 release that passes the text after `"%s"`, or with the `%` doubling restored; re-check weekly |
| 2026-11-11 | ESLint, `@eslint/config-array`, typescript-eslint and `eslint-plugin-jsx-a11y` on `minimatch` 11 (GHSA-g3hv-75wg-m249) | Take each release that asks for `minimatch` ^11.0.1 once it is 7 days old. When the lockfile holds no `minimatch` below 11.0.1, remove the exceptions in `osv-scanner.toml` and `auditConfig.ignoreGhsas`. Otherwise re-check weekly; the OSV entry expires on 2027-02-04, the review date of both                                                                                                                                                                            |

**Nightly follow-up** (no decision needed): done in `fe2a57b`. The flow
launcher (`e2e/specs/flows/lib/launch.ts`) retries a launch once when
msedgedriver cannot connect to WebView2's renderer ("session not created …
unable to connect to renderer"), with a new web view folder and driver log
and the retry printed; it happened once in about 1,400 Windows launches
(Nightly of 2026-10-19), before the test began. Unit tests check which
driver messages count (msedgedriver's and WebKitWebDriver's, also ones that
must not), the retry itself, and the launcher with the fake driver.
`launchApp` still fails at once: the benchmarks must not retry, and the
security tests, the exit criterion and the canvas screenshot make about a
dozen Windows launches a night, against some 150 in the flows.

What I do with each result:

- **Decisions (1, 2, 8, 9, 11):** apply them on the branch the same day, with the
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
  C++ compiled with `g++ -c` (done in W1); the M3 threat-model review
  (the worker's CSP and scope index, files and paths a program may touch,
  the Trusted Types table of the security specs); an npm licence gate
  (TM-M2-6); the toolchain matrix cells deferred from M2.

### 3.3 Decisions before M3 starts

Each with a recommendation. **Decided 2026-10-24:** the owner accepted every
recommendation; for M3-D1, the assignments listed. M3-D3 in the owner's
words: like any good program, a program only touches what it is given
permission to touch or told to touch. So the generated program reads and
writes only the files its blocks name (no support helper touches a file on
its own), and the app touches only the open project's folder, its own
machine folders and what the user picks in a dialog.

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
| W0   | Decisions above; ADR-0016 (Worker) and ADRs for the type representation (L14), slot typing (L15) and the header split (L18); the catalog schema extension (L17); the M3-D1 assignments as acceptance tests checked against reference solutions; the Blockly 13 / xterm 6.1 spike. Done 2026-10-25 (§3.6) | — | M    |
| W1   | Foundations from [the contracts](m3-contracts/types.md): L14, L15, L17 with `headers` (L8), L16, L10, and ADR-0019's IR types (no output change); the catalog snapshot test. Done 2026-10-25 (§3.7) | W0 ADRs | XL   |
| W2   | Language blocks needed by the assignments: collections, strings, structs, enums, files, errors, random, time, globals, constants, math, conversions, `switch`, `do … while`, overloads, default arguments, M3-8. Contract and packages: [w2.md](m3-contracts/w2.md). Done 2026-10-28 (§3.8) | W1                             | XL   |
| W3   | Projects, build, run: multi-module (L18) in two parts (functions across modules after W1; structs, enums, globals and constants after W2's definition blocks; the per-unit object cache if-time), _namespace_ and _use namespace_ (from W2, [w2.md §1.10](m3-contracts/w2.md#110-not-in-w2)), CMake/Make export, project settings dialog, run options, close-input, S7-5, S7-7, S7-8. Export, run options, close-input, the project settings page and S7-5, S7-7, S7-8 merged 2026-10-28 ([w3-export-run.md](m3-contracts/w3-export-run.md)); the rest done 2026-11-20 ([w3-modules.md](m3-contracts/w3-modules.md), §3.9), but for the if-time sweep (package S), moved to M4 | W1; W2's integration for the first part, as it shares files with W2 ([w2.md §3.4](m3-contracts/w2.md#34-order)); W2 for the second part | L    |
| W4   | Editor: type picker, Quick Insert, toolbox search, expert mode, quick fixes, _Copy bug report_, feature gating, friendly g++ messages, Appearance (F-1, U14d); from W2, the ⚙ popover (`constexpr`, `static`), _format … with_ and the unchecked `[]` index from the block's context menu. Appearance, expert mode's settings and the gating service merged 2026-10-28 ([w4-appearance-expert.md](m3-contracts/w4-appearance-expert.md)) | W1, W2 for the blocks it shows | L    |
| W5   | Performance and platform: starting with the worker CSP spike on both webviews (ADR-0016), the Worker preview (M3-1), M3-5, M3-4, M3-9, M3-6, M3-7, RD-15 (the Blockly 13 upgrade moved after M3: W0's spike was not clean); the toolbox flyout's cost per block (§3.8). The spike and M3-1's first part, behind a flag, merged 2026-10-28 ([w5-worker.md](m3-contracts/w5-worker.md)) | W2 (stable `update()`) | L    |
| W6   | Close-out: every M3-D1 assignment green, threat-model review, toolchain cells, docs and the getting-started guide, usability protocol for M3, a green nightly and weekly                                                                                 | all                            | M    |

W3's first part (functions across modules) starts after W2's integration
step, not alongside W2 as first planned: it edits the files W2's P0, C1 and
C2 edited (`SHARED` on _define_, the function pre-pass, the print order),
and both of W2's agents were busy ([w2.md §3.4](m3-contracts/w2.md#34-order)).
W3's second part follows it; W4 starts with the pieces that need no new
blocks (expert mode, toolbox search, Appearance).

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
- **Generated C++ correctness** across GCC 11 to 15: the catalog
  snapshot test (in place since W1) runs in CI on GCC 11, 13 and 15 and on
  MSYS2, and the every-g++ build test added in M2 covers the local
  side-by-side compilers, as W1's test of codegen's new forms does.

### 3.6 First steps once M3 is approved

1. Ask the M3-D questions in one batch, with the recommendations above.
2. Write the M3-D1 assignments as acceptance tests, checked against
   reference solutions until their block projects exist, so progress is
   visible as assignments turning green. Done:
   [`tests/assignments/`](../../tests/assignments/README.md) holds the ten,
   each with a reference solution that checks its run data until its block
   project passes (1 of 10 green: the guessing game).
3. Draft the four ADRs (Worker, types, slots, headers) for review. Drafted
   on 2026-10-25 as Proposed: [ADR-0016](../adr/0016-preview-in-a-web-worker.md)
   (the preview in a Web Worker), [ADR-0017](../adr/0017-open-type-representation.md)
   (the open type representation, with L17 and L8),
   [ADR-0018](../adr/0018-slot-typing.md) (slot typing) and
   [ADR-0019](../adr/0019-multi-module-header-split.md) (multi-module
   projects and the header split); each lists the points for the owner to
   confirm. Reviewed the same day (one reviewer per ADR, one for the
   assignments, one across all four; every finding checked again before it
   was applied), and split: each ADR keeps the decision, and
   [`m3-contracts/`](m3-contracts/types.md) holds the exact contracts W1
   turns into code (`types.md`, `slots.md`, `worker.md`, `modules.md`),
   with one table of new diagnostic codes and one version plan.
4. Run the Blockly 13 / xterm 6.1 spike and record it. Done:
   [the record](m3-spike-blockly13.md). Not clean (4 of 105 Linux E2E
   tests and 79 unit tests fail, the slots render differently, dragging is
   16.7 % slower), so under M3-D7 the upgrade waits until after M3; xterm
   6.1 has no stable release yet.
5. Then W1, with the plan's must-have and if-time sets agreed.

### 3.7 W1: done, and what the owner decides

**Done** on 2026-10-25, on `ccr-01f82d9b-v70u20` up to `209b75e`: the
contracts (`de24f49`); seven packages from parallel worktrees, merged in
turn: type terms with format version 2 and clipboard version 2
(`426bf7b`), the catalog's signatures, header table and catalog 1.3.0
(`d6ecfac`), library and user types in codegen (`92af516`), M3's types and
typed slots in the analyser (`75c9d19`, with `3fdec18` and `d11598f`), the
IPC types, `slotTypes` and the scope query's members (`c2c9521`, `ed93c3e`,
`010de7f`), the editor's copy of the rules with the wire switch (`8af978a`,
`01ea173`, `f613fee`, `163ffaa`) and the catalog snapshot test (`92ddb7a`);
then the integration step of types.md §8.3, with start values on landing,
the typed-slots E2E flow and benchmarks (`8da4f27`), and L16 and L10
(`209b75e`). The golden C++ is byte-identical across the wave. The
specification, the changelog and the contracts were brought up to date
after it; the contracts now describe the code W2 and W3 build on. Left for
the next waves: Windows E2E and GCC 15 run in CI on the next push;
`webview.drag-typed-1000.frame-p95-all` is gated once it has 5 baselines;
_Textbook style_ brings its own format-version check; and W2 gives each new
block a case in the catalog snapshot test.

**For the owner to confirm** (each ADR's last section has the points in
full):

- **ADR-0016** (W5): 1, every module previews in the worker; 2, copy and
  paste reuse the applied preview, decided after the spike's measurement;
  3, `canonical` stays on the main thread; 4, a worker that fails for good
  falls back with a log entry and no banner; 5, N4 moves to `update-p95`.
- **ADR-0017** (in W1's code): 1, format version 2 holds _Textbook style_
  and type terms, and versions without a change of meaning load without a
  migration; 2, `std::string` on the wire too; 3, no unsigned or sized
  integers, with codegen's casts; 4, streams and errors only as types of
  declared symbols; 5, catalog `headers` emitted, four core blocks losing
  some; 6, at most 8 levels and 32 names; 7, the changed collection rows of
  03 §3.7.8; 8, ticked structs need qualifying fields, and C++17 gets six
  comparison operators.
- **ADR-0018** (in W1's code): 1, a TypeScript copy held to Rust by a
  fixture; 2, `list of` and `{…}` typed only from their place (`E0315`
  without one); 3, **the editor now refuses drops M2 allowed** (text into
  `create [int]`, an `int` argument, `return` in `main`); declining it is
  one change, `typeAllows` in `checker.ts`; 4, brace items cast rather than
  refused; 5, start values change only on a field change or a landing; 6,
  parked blocks' slots typed from their own parts.
- **ADR-0019** (W3): 1, new definitions start unshared; 2, includes follow
  use, `use header of module` if-time; 3, `#pragma once`; 4, the object
  cache if-time; 5, a module name starting with `b2c` is an analyser error;
  6, a module's own name hides another module's shared name.
- **L16 and L10** (in W1's code): two quoted texts are joined
  (`std::string("a") + "b"`) rather than refused; `math.arithmetic` keeps
  `check = "number"`, so text blocks still do not snap into `+`;
  `B2C-E0517` is reused for constant calculations, and `B2C-W0506` is the
  first free `W05xx` code. Also the integration step's choice: a table
  without the four blocks' signatures checks nothing of their values'
  types. Codegen's unspecified formats (a struct printed as
  `Point{x: 1, y: 2}`, an enum's `switch`) are reviewed with W2's blocks.

**Review of W1.** Six reviewers (types, security, compatibility, editor,
codegen, tests and docs) found one major and fourteen minor points, all
fixed on the branch after `cf164be` except what follows. The major one:
typed checks made a scope query per parent they typed, so keyboard listings
and drags on programs with `set` blocks and prints of variables were up to
tens of times slower than in M2; they now type only what a fit needs and ask
about scopes only to confirm a refusal (slots.md §4.3, _Speed_).

- **For the owner to confirm:** a decimal division by a constant zero whose
  result goes straight into an `int` or `char` is now the error `B2C-E0517`
  (C++ leaves converting infinity or NaN undefined) instead of the warning
  `B2C-W0506`; a division nested in a larger calculation still warns. The
  typed benchmark document now also sets and prints its variables, so
  `webview.keyboard-list-1000.ms` and `webview.drag-typed-1000.frame-p95-all`
  start a new series (both are reported, not gated).
- **Left for W2:** a `members` field's menu (W1 lists the plain scope, as M2
  did) and the checker tests that need a place-typed block or a `members`
  field (slots.md §9); a user function named `to_string` that takes an enum
  clashes with the `to_string` generated for a joined enum (the call is
  qualified, `::to_string(c)`, so a variable of that name no longer hides
  it): reserve the name or move the generated function into namespace
  `b2c` with W2's enum blocks.
- **Left for W3:** W2 moved the joined enum's text function into the
  support namespace (`b2c::to_text(Color)`, printed right after the enum in
  the global `::b2c`, so a user function `to_string` no longer clashes,
  [w2.md §2.11](m3-contracts/w2.md#211-the-to_string-clash)). The overload
  must stay in the global `b2c` namespace: once an enum lives in a module's
  namespace, the overload is printed after it with the user namespace
  closed and reopened, and names the enum by its qualified name
  (`to_text(geo::Color)`).

### 3.8 W2: done, and what the owner decides

**Done** on 2026-10-28, on `ccr-01f82d9b-v70u20`, from `f508fca`: the
contract ([w2.md](m3-contracts/w2.md), `cfef598`, after three reviews);
round R0, the Rust and the editor groundwork (P0-R `04a6db1`, P0-T
`7865f02`, the four categories `95135af`); round R1, the collection blocks
(package A, `17731f3`, matrix_sum green) and the text, input/output, math
and error blocks (package B, `42041a5`; temperature_table and
input_validation, then grade_average `836d12c`, dice_statistics `12101d3`
and palindrome `aaf367f`); package T, which keeps the toolbox flyout's
blocks between showings (`b410c45`); round R2, structs and enums (package
C1, `565025b`, student_records), switch, _do … then repeat_ and the file
loop (package D, `a1bd1d7`, word_count) and the switch on an enum
(`82acd48`, bank_menu `1635bc5`); round R3, default values, overloads,
globals and constants, _change_'s new operators, _flip_, _with arguments_
and struct field defaults (package C2, `e52d953`..`c4700b6`) and the
if-time sweep (package S: _use header_, _check at compile time_ and the
file blocks _open file_, _read line_, _write_ and _file exists_, `950e0ee`,
`b4e1347`), with the review of C2 and S (`d8487f0`..`f48de01`); the
integration step of w2.md §5.3 (`5337b05`..`9a4739d`, with its review
fixes `33deca8`, `8ca23ca` and `603ce8a`); the E2E tests made to fit an
800 × 560 window (`e500b1c`..`ae7e761`); and this docs step. The catalog
has 86 blocks in 13 categories (55 of them W2's) and stays 1.3.0, and
**all ten assignments are green** (`M3 assignments: 10 of 10 green`), so
M3's language exit is met for them. The golden C++ of the earlier examples
is byte-identical. The integration step ran every check of w2.md §5.1, the
every-g++ forms test on a project with every catalog block (g++ 11 to 14,
C++17 and C++20, strict, `-Werror`), and the whole Linux E2E suite at the
default size, 1024 × 768 and 800 × 560 after the small-window fixes (63
files, 246 passed, 1 skipped each), and at 1028 × 749 before them (243
passed, 1 skipped); the native benchmark's new
`preview-language` shape takes 15.4 ms locally, and a local native A/B of
`pipeline/preview` against `f508fca` gave +1.4 % (gate 10 %). The A/B job
on `f508fca..603ce8a` (bench-ab run 40733079892, all metric groups, both
systems) found no gated regression: on Linux `webview.edit-1000.p95-all`
−29.7 %, `preview-1000.p95` −4.8 %, `select-1000.max-task` −1.8 % and
`drag-5000.frame-p95-all` −4.3 %; on Windows +5.7 %, −0.3 %, +6.8 % and
+7.9 %, and `console-200000.run` −1.4 %. The only metric more than 10 %
worse is Windows' ungated `guessing-game.edit.max-task`, 8.15 → 9.15 ms.
`webview.drag-typed-1000.frame-p95-all` is still reported only (no A/B
metric group runs it), so W3's integration gates it. The
specification describes what W2 built, and w2.md §7.1 records each place
where the code differs from the contract, with its reason. After the docs
step the visual baselines were rebaselined from the CI candidates of
Desktop run 40733070176 on `603ce8a` (`ade20b2`, merged in `32ea263`): 47
Linux and Windows images, the toolbox scenes with every category icon
drawn as a glyph on both systems, the Program, Variables and Errors
flyouts, `canvas-guessing-game`, `chrome-window-smallest` and the menu
scenes, with Linux `chrome-toolbox-files` and `chrome-toolbox-files-dark`
new. W3 and W4 were merged by then, and the console scenes did not change;
locally they still differ, because a machine without cgroup containment
adds the console's _Process group only_ note.

**Moved, each with its reason in [w2.md §1.10](m3-contracts/w2.md#110-not-in-w2):**
_namespace_ and _use namespace_ to W3 (§3.4's row); the ⚙ popover, _format
… with_ and the unchecked `[]` index to W4 (§3.4's row); _moved_
parameters, the generic member block, _delete file_, _create folder_, _for
each file in folder_, a random seed, the clock, the stopwatch and _read
word_ to M4 ([10 §10.1](../spec/10-roadmap.md#m4-language-breadth-ii));
the structured binding after M3. _catch anything_ is not planned.

**For the owner to confirm** ([w2.md §6](m3-contracts/w2.md#6-for-the-owner-to-confirm)):
falling through is the block _go on into the next case_, not a checkbox on
each case; there is no _catch anything_; the items moved to M4 and after
M3 above; _namespace_ and _use namespace_ in W3 and the three editor items
in W4; until W4's type picker the menus offer lists of int, double and
text, the module's structs and enums and lists of them, while maps and
lists of lists come from the project file and paste; _join_ and _as text_
refuse a struct, printable or not (a generated `b2c::to_text` per printable
struct could come later as a compatible change); and W3's first part
waited for W2's integration step.

**Still to do for W2:**

- Windows E2E and GCC 15 on the next push, then the review of the wave
  with its lenses and the fixer, as in every M3 wave;
- the comment of `catalog/toolbox.toml` still says the four categories are
  empty until their packages fill them; it is corrected with the next
  catalog change.

**Toolbox cost, for W5** (a first-class item). Showing the continuous
flyout draws every catalog block: with package B's 47 blocks the app's
toolbox refresh took 204–216 ms against 145–149 ms with 26 (WebKitGTK,
debug build), and the happy-dom flyout 1.09 s against 0.40 s; the catalog
now has 86. Package T keeps unchanged blocks between showings, which took
the refresh after a new variable from 225 ms to 45 ms on the guessing game
and from 300 ms to 52 ms at 1,000 blocks, but the first showing still draws
every block. W5 takes the cost per block on (lazy or per-category
rendering, or cached block drawings) with a gated toolbox-refresh
benchmark (`webview.declare-variable.toolbox-refresh` is reported now), and
checks whether a gated webview metric moved with W2. The desktop tests that
show the flyout keep `FLYOUT_TEST_TIMEOUT_MS` until then
([09 §9.2](../spec/09-quality-and-delivery.md#92-testing-strategy)); a test
helper that injects only some categories would let most of them go back
(a follow-up from package T).

**W3, W4 and W5 so far.** Merged on 2026-10-28 (`93febef`, `b841a9d`,
`012c948`), each green in CI but for the visual diff:

- **W3**, export, run options and the runtime items
  ([w3-export-run.md](m3-contracts/w3-export-run.md)): close-input (the
  console's _End input_, `Ctrl+D` with pipes), arguments and the working
  folder in the project, the input file for the app session, the external
  terminal, export to CMake and Make (`b2c generate --export`, _Export as
  C++ project…_), the project settings page, and S7-5, S7-7 and S7-8. The
  IPC contract has 39 commands. **Remaining:** done on 2026-11-20 but for
  the if-time sweep (§3.9); the spec sections of w3-export-run.md §10 that
  W3's docs step did not cover remain (§3.9).
- **W4**, Appearance and expert mode
  ([w4-appearance-expert.md](m3-contracts/w4-appearance-expert.md)): the
  Appearance setting with the `[data-theme]` refactor and a console that
  follows the theme, the _Block labels_ and _Show advanced blocks_
  settings with one _Expert mode_ switch, and the gating service. W2's
  package C2 wired the gating service into one place: _change_'s operator
  menu lists the bitwise forms only with _Show advanced blocks_.
  **Remaining:** the rest of its "Needs from W2"
  (w4-appearance-expert.md §6): the C++ label mode in the blocks (§6.1;
  `CPP_OPTION_LABELS` covers every operator and function menu since the
  integration step), the toolbox hiding gated entries and categories
  (§6.2), the other field menus (§6.3) and the catalog's `advanced` and
  `requires` keys (§6.4), which W2 did not apply; then the type picker,
  Quick Insert, toolbox search, quick fixes, _Copy bug report_, the friendly
  g++ messages, the ⚙ popover, _format … with_ and the `[]` index, and the
  spec sections of its §8.
- **W5**, the preview worker's spike and M3-1's first part, behind the build
  flag `B2C_PREVIEW_WORKER` ([w5-worker.md](m3-contracts/w5-worker.md)):
  clean on both webviews. **Remaining:** its "Needs from W2" (§10): the
  scope index (`Analysis::scope_index_parts()` in `crates/b2c-lang`, now
  that W2's symbol kinds and members are in) read by `analysisScope`; then
  its integration step (the clipboard arguments, the rest of the apply rule
  and the drag hold, the spans and metrics, the hooks and E2E flows with the
  worker on, and removing the flag), M3-5, M3-4, M3-9, M3-6, M3-7, RD-15 and
  the toolbox cost above.

### 3.9 W3: done, and what remains

**Done** on 2026-11-20, on `ccr-01f82d9b-v70u20`, from `62f5b9f`: W3's
second part, after its contract ([w3-modules.md](m3-contracts/w3-modules.md),
reviewed three times on 2026-10-28). Round R0: P0, the
contracts in code (the IR's namespace symbols and spellings, Textbook style
readable and written, `MAX_NAMESPACE_DEPTH`, _share with other modules_ on
_define_ written only when ticked, the layout and its codes, files by the
layout and `b2c_support.hpp` from two modules on; `43239a7`..`2a73ca2`),
and package B, building and exporting several modules (`gen/` pruning,
relative object paths, messages across units, export manifest format 3 and
_Replace_ keeping edited files; merged in `c3f3cd9`, with its fixes
`b6213db`, `58b860f` and `256d12c`). Round R1: package F, functions across
modules (`c5d8536`..`59435b1`, the `unit_converter` example), and package
E, the editor for modules (the module strip, module edits with _Undo
delete_, the C++ tab by module, the project name on the settings page;
merged in `7906a48`), with the reviews of F and E (`34d7871`..`b5c810b`)
and `B2C-E0213` made program-wide (`03bf24a`). Round R2: package T,
structs, enums, globals and constants across modules (`a80b630`..`2c922c0`,
the `gradebook` and `inventory` examples); package N1, the _namespace_
block and the standard-name table (`3d8778d`..`f1bbce6`); and package C,
the per-unit object cache (`38b7e4e`, `db34a5d`, `0b7133d`). Round R3:
package N2, the lookup model's M3 subset, _use namespace … in this file_
and Textbook style (`9dcd0e0`..`be726fa`, the `shapes` and
`textbook_grades` examples; N1 and N2 merged in `55a1c7e`). The
integration step of w3-modules.md §5.3 (`f6bbcaf`..`07f907d`: the
standard-name table with GCC 15's names, the every-g++ forms test on a
three-module program, paste binding namespace-qualified names and keeping
free symbol IDs (W3M-D12 (b)), `helper_placement` dropped, the scope
index's test, the native benchmark's 8-module case `pipeline/preview-modules`
(reported, not gated), `pnpm test:tools` in CI, the security suite and
fuzz seeds at the limits, and the nightly 256-module build and export),
then the module strip's last fix and its visual scenes (`a89f994`,
`3fbad30`), and this docs step. The catalog has 88 blocks. The owner's
decisions W3M-D10 to W3M-D13 were taken as recommended: `B2C-E0213` is
program-wide (ADR-0019 point 6 amended), `SHARED` is written only when
ticked, a paste keeps free symbol IDs, and Textbook style has no preview in
M3. The specification describes what W3 built, and w3-modules.md §7.1
records each place where the code differs from the contract, with its
reason.

**After the docs step** (2026-11-21): the wave review's three lenses and
its challenger confirmed seven findings and refuted one (B1: keeping
`obj/` through a failed build is the contract's choice). The fixes, each
with a test that failed first, merged in `ef151aa`: a shared item's header
parts spelled as the header sees them and a shared global's starting value
as its `.cpp` does (C1, C3; a header default argument had bound to another
module's variable), `::geo::area` when the first qualifier is contested
(C2), an undeclared namespace's members left out (C4), a definition dragged
out of a namespace's mouth leaving those below it (E1), _Undo delete_
focusing the restored module (E2) and _use namespace_ following the
namespace rather than one of its blocks (E3). The golden C++ of the
examples did not change. CI also needed `resolve_failed`'s damaged-block
test spread over every core (it had pushed the Windows job past its
40-minute limit; `e19b2a0`) and the module strip's visual baselines
(`e69e36a`). The wave's bench A/B (`62f5b9f` against `3fbad30`) found no
gated regression: on Linux the preview's p95 fell from 53.5 to 7.5 ms. On
Windows the preview benchmark could not run, since with the worker the
main thread's preview task is often shorter than the 4 ms it looked for;
`webview.preview-1000.p95` is now the `b2c:preview` span, with the new
`update-p95` (gated, N4) and `main-p95` (reported), as worker.md §9 planned
(`a5f1af8`, `0e328eb`).

**Moved to M4:**

- **Package S**, the if-time sweep: _use namespace … here_, the standard
  sub-namespaces in _use namespace_'s menu and _use header of module_, cut
  for the budget ([10 §10.1](../spec/10-roadmap.md#m4-language-breadth-ii)).
  `B2C-E0703` (a C++20 sub-namespace in an older standard) is allocated and
  documented, and waits for it.
- Textbook style's preview, _Rename all and turn on_ and the hovers that
  explain `::`; the lookup model's rules for classes, Raw C++ and library
  packs; the background probe of a toolchain newer than the standard-name
  table (w3-modules.md §1.8).

**Still to do for W3:**

- **W5 §10.1**, lowering parked stacks (M3-4): not done by the integration
  step, since it is well defined but not small (w3-modules.md §7.1, _From
  the W4 and W5 merges_); a change of its own. _Done on 2026-11-23_
  (§3.10, w5-performance.md §10.1).
- **Lookup (owner decision):** the scope query hides another module's
  shared definition behind the module's own definition of the same plain
  name, whatever their namespaces (an own `::count` hides a shared
  `geo::count`), and the paste binds only what the query lists. C++ would
  see both. Fixing it changes `ScopeIndex::visible` and both lookups (Rust
  and TypeScript), so it was left as it is and is put to the owner
  (w3-modules.md §7.1).
- **WebAssembly size:** the core is 1,994,569 bytes after `wasm-opt`
  (after M3-4). The owner decided on 2026-11-24 to raise the budget to
  2,100,000 bytes (ADR-0003 and ADR-0010 amended) and to follow with a
  saving pass on the core as its own package (§3.10).
- **Analysis time:** it grows faster than linearly with the module count
  when every module has globals: 128 modules take 0.56 s and 256 take
  1.5 s (the layout of 64 modules sharing structs, functions and globals is
  about linear, `c4ecb92`).
- **Bench A/B:** `pipeline/preview-modules` and the two new preview
  metrics are gated once they have five baselines each.
- The spec sections of w3-export-run.md §10 (W3's export and run slice)
  that this docs step did not cover: 04 §4.5 (_End input_, run options),
  §4.10 and §4.12, 05 §5.9 (`run.externalTerminal`), and 07 §7.5.1's
  sandboxes, §7.6.1, §7.6.2, §7.6.5 and §7.9; 02 §2.5.2 and
  `docs/reference/cli.md` already name the new commands and `--replace`.
- `ProjectExportDialogResponse` could gain an additive list of the kept
  files, so that the app names them (it logs their count now).

### 3.10 What remains of M3

Surveyed on 2026-11-23 from the waves' contracts (each one's _Status_
section). W0 to W3 are done; W4 and W5 are done but for the pieces left to
the coordinator; W6 has not started.

**Next, in this order** (each a package of its own, small enough for one
agent and a review):

1. **M3-4, parked stacks in the scope query** (W5 §5, §10.1): the exact
   proposal is written; it lowers a parked stack so the menus see its
   declarations. _Done on 2026-11-23:_ a parked stack is lowered in a
   sandbox (`lower/parked.rs`), its declarations are visible below them in
   its stack and its reporters typed, and the generated C++ is unchanged;
   the corrections to the proposal are in w5-performance.md §10.1.
2. **W4's coordinator pieces** (w4-editor-tools.md §7, §12): the ⚙ popover
   (`constexpr`, `static`), _format … with_ and the unchecked `[]` index
   from a block's context menu, and the catalog's `advanced` and `requires`
   keys. `block:text.format` stays AWAITED until then. _Done on
   2026-11-24:_ the four pieces, each a commit, with the corrections in
   w4-editor-tools.md §7.1; `block:text.format` is the catalog's now, and
   the WebAssembly core is 2,011,983 bytes.
3. **Back to the contracts** (owner decision of 2026-11-24: nothing in a
   contract disappears without the owner hearing of it). Each correction
   row a package recorded instead of building what its contract says is put
   to the owner with its reason; what the owner does not accept is reworked
   to the contract, with tests that fail first. From w4-editor-tools.md §7.1:
   - `requires` as the contract's table, not a list of words (about 20 KB
     of the WebAssembly core, which the saving pass must make room for);
   - an option's `advanced` and `requires` in the contract's shape;
   - _format … with_'s pattern as a value input, not a text field;
   - _format … with_'s `std::ostringstream` form under C++17 (03 §3.7.4);
   - the gate's reason in the tooltip of a block the gate refuses;
   - the ⚙ popover on _global_ as well as _create_, holding the _constant_
     tick (03 §3.7.2);
   - a `constexpr` decimal number readable in another `constexpr` value
     (L10 telling `constexpr` doubles apart);
   - the C++ label mode showing `numbers[i]` for unchecked access;
   - a map's unchecked _item of_ as the contract writes it; `m[k]` on a
     read-only map does not compile, so if the contract cannot be followed
     there, the owner decides, not the package.

   Then the same sweep for the correction rows of w3-modules.md §7.1,
   w5-performance.md §10.1 (M3-4's nine) and the earlier waves' contracts:
   each one put to the owner and then kept or reworked. **From now on**, a
   package that finds its contract cannot be built as written stops and
   asks the owner, with its reason, instead of recording a correction on
   its own; a deviation stands only once the owner has accepted it.
4. **A saving pass on the WebAssembly core** (owner decision of
   2026-11-24, with the budget raised to 2,100,000 bytes): measure what
   the module spends its bytes on and trim it, so the budget can come back
   down.
5. **W5's leftovers** (w5-worker.md §11, w5-performance.md §14): the
   clipboard arguments with their copy, paste and duplicate measurements
   (ADR-0016 §1), `coreMemory()` and the nightly security case of
   worker.md, RD-15, and the first run of `building.yml` (M3-7), which runs
   weekly and has not run yet: GitHub dispatches only a workflow that is on
   the default branch, so it first runs after the merge into `main`.
6. **The spec sections** still owed: worker.md §11 and 08 §8.8's
   `WORKER_CSP`, w4-appearance-expert.md §8, w4-editor-tools.md §9, and
   w3-export-run.md §10 (listed in §3.9).
7. **W6, the close-out** (§3.4).

**For the owner:** W4A-4 (the title bar) and W4A-5 (the _View_ menu) from
w4-appearance-expert.md, and the scope query's shadowing of another
module's shared definition (§3.9). The friendly g++ rules were recorded
against GCC 13 only; GCC 11 and 14 are installed here, GCC 15 is not.
