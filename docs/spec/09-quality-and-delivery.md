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
| **Snapshot** | `insta` | Generated C++ for every catalog block in its default configuration, plus curated variants (the forms whose headers the generator chooses, and library-typed variables and parameters from a project file). Each snapshot must include the header of every standard name it uses, and is compiled with `g++ -c` under C++17 and C++20 with the *strict* warnings as errors. |
| **Golden end-to-end** | `b2c` CLI | `examples/*.b2c` → generate → compile → run with an stdin fixture → compare stdout, stderr and exit code with `tests/golden/`. Matrix: GCC 11, 13 and 15 on Linux (containers), MSYS2 UCRT64 GCC on Windows. |
| **Property-based** | `proptest` | (a) String-literal round trip: arbitrary text → program prints exactly those bytes. (b) **Differential "no generator bugs" oracle**: random well-typed block programs from a BDM generator; if the analyser reports no errors, g++ must accept the output. (c) Determinism: generate twice, and with shuffled top-level order → identical bytes. (d) BDM → Blockly → BDM round trip is the identity. |
| **Fuzzing** | `cargo-fuzz` (libFuzzer) | BDM parser, expression parser, type parser, catalog/pack loader, SARIF/JSON/text diagnostics parsers, GDB/MI parser, event-channel parser, comment/string encoders |
| **Mutation** | `cargo-mutants` (weekly) | Security-critical encoders and validators must kill all mutants |
| **Frontend components** | Vitest + Testing Library + axe-core | Panels, dialogs, custom Blockly fields. Automated accessibility checks. |
| **Desktop E2E** | selenium-webdriver + Vitest + `tauri-driver` (Windows + Linux) | Create project, Quick Insert, build, run with input, stop; open an untrusted project → Restricted Mode; trust flow; save/reload; crash recovery |
| **Visual diff** | WebDriver screenshots + pixelmatch | The canvas with a known project on both systems |
| **Chrome visual diff** | WebDriver screenshots + pixelmatch | In the smallest window (800×560): the whole window, the open main menu over the editor, the toolbar's Debug/Release drop-down closed and hovered, the toolbox after each category, the console after two runs (scrolled up, and at the bottom panel's tallest); light on both systems, dark on Linux (Windows dark with the Appearance setting, M3) |
| **Security regression** | Custom harness | `tests/security/projects/*.b2c` with asserted outcomes ([08 §8.12](08-security.md#812-threat-summary)) |
| **Performance** | `criterion` + scripted workspace benchmarks | Codegen p95 at 1k blocks, edits and selections at 1k blocks, drag frame time at 5k blocks, cold start; typed drags and the keyboard mover's listing at 1k blocks and the longest main-thread task of every E2E flow (reported). More than 10% regression fails CI on the nightly benchmark job and on a pull request's A/B job. |
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
* **The catalog snapshot test** (`crates/b2c-build/tests/catalog_syntax.rs`)
  generates C++ for every core catalog block in its default configuration,
  as the toolbox gives it (the entry's preset, the catalog's defaults and
  shadows; the editor's own choices only for names, the chosen variable or
  function, *set*'s start value and a new *define*'s empty parameter list),
  each in the smallest program that passes the analyser and g++, and for
  curated variants: *stop program* in a function, a simple *ask*, an *ask*
  for text, a literal used as a `std::string`, and variables and parameters
  of library types read from a format-version-2 project. It runs the whole
  front end (`run_frontend`), as the build does, and snapshots each case
  with `insta` (the app version masked; a `<case>@c++17` snapshot only where
  the C++17 code differs). g++ cannot show a missing header, since
  libstdc++'s headers include one another, so each `std::` name in a case's
  code must have its header (from a table in the test) among the file's
  `#include` lines. All cases are compiled with `g++ -c` (not
  `-fsyntax-only`, which skips the middle end's warnings such as
  `-Wreturn-type`), one invocation per standard, under C++17 and C++20 with
  a debug build's flags at the *strict* warning level and `-Werror`. A
  catalog block without a
  case fails the test. It is skipped without g++ unless `B2C_REQUIRE_GXX` is
  set, and runs in the Linux (GCC 13), GCC 11, GCC 15 and Windows (MSYS2)
  test jobs. Each catalog file's cases live in
  `crates/b2c-build/tests/catalog_syntax_cases/<file>.rs`, a module of the
  test, and a missing case's message names that folder; the catalog tests
  count the blocks of `catalog/core/*.toml` rather than a fixed number.
* **Tests that run g++ themselves** go through the probed toolchain,
  `b2c-process` and `compiler_env`, as a build does (`catalog_syntax.rs`,
  `run_data.rs` and W2's run tests), and canonicalise paths with
  `b2c_toolchain::paths::canonical`: `std::fs::canonicalize` gives a
  verbatim `\\?\` path on Windows, from which g++ cannot find `cc1plus`. A
  test that also tries every `/usr/bin/g++-N` starts at the oldest
  supported GCC (`MIN_GCC_MAJOR`, 11). M3's wave W2 adds such tests for its
  blocks: `crates/b2c-build/tests/text_errors.rs`, `structs.rs`,
  `control_files.rs` and `functions_globals.rs` build and run their
  programs with every g++ found, under C++17 and C++20 at the *strict*
  level with `-Werror`; `control_files.rs` runs each program in a fresh
  folder holding its input files, checks that the folder is unchanged
  after a program that only reads, and runs one that writes, adds to and
  reads back a file in its run folder. The analyser's rules for W2's blocks
  are in `crates/b2c-lang/tests/` (`text_errors.rs`, `control_files.rs` and
  others), and the editor's typing and mutators in `blockly-ext`'s tests
  (`text-errors.test.ts`, `checker/structs.test.ts`, …).
* **Types and typed slots.** The editor's TypeScript copies of the type rules
  and of typed slots are held to the analyser by two committed fixtures,
  `packages/blockly-ext/src/types/fixture.json` (the sample types, their
  keys, labels, C++ spellings, conversions, classes, type functions and
  pattern bindings) and `slot-fixture.json` (cases for every rule of the
  one-pass typing, a seeded sample of each catalog block with a pattern,
  and the analyser's value types of the blocks whose output is `any` or
  `number`). `crates/b2c-lang/tests/fixtures.rs` writes them and fails when
  they are stale, as for `catalog.json`
  (`B2C_UPDATE_TYPE_FIXTURES=1 cargo test -p b2c-lang --test fixtures`);
  `blockly-ext`'s Vitest suites check every case, and the desktop's
  integration test compares the core's conversion table with the
  TypeScript rule on every row. The analyser's tests run every document
  with the catalog's signatures and with M2 blocks' patterns written out by
  hand, and compare the results. A recorded snapshot
  (`m2_documents_analyse_with_the_catalog_table_as_with_m2_rules` in
  `crates/b2c-lang/tests/props.rs`) holds the catalog's table to what M2's
  own rules gave on 400 generated programs, every example and every
  malicious project that loads, apart from rules changed on purpose since
  (`+` on text); a document that uses a block or block family added after
  M2 (`common::AFTER_M2` in `crates/b2c-lang/tests/common/mod.rs`) is left
  out of it, and the comparison takes the catalog headers off both
  programs. The slot fixture samples the parts of repeated fields too (one
  count for all of a block's repeated fields, a part per index), so the
  analyser and the editor are compared on each caught error's type. The
  property test that well-typed programs have no errors divides by `b + i`,
  never by a constant, since a constant zero divisor is a real
  `B2C-E0316`. The no-false-refusal test
  (`apps/desktop/src/editor/sync/noFalseRefusal.test.ts`, with the real
  core) samples value blocks and inputs in every example, and each drop the
  editor refuses must give an analyser error at the moved block or its
  parent once made; a refused drop into a parked parent must give one once
  the parent is attached. Codegen's forms test compiles a program with every
  sample value type, a comparable struct in a set and a map, a printable
  struct, an enum printed and joined, an index and a size compared with an
  `int`, with the *strict* warnings under C++17 and C++20, with the `g++` on
  `PATH` and every `/usr/bin/g++-N` from `MIN_GCC_MAJOR` (11) on, then runs
  it. Since M3's wave W2 it also builds
  `crates/b2c-codegen/tests/fixtures/every_block.b2c`, a project that uses
  every block of the catalog (a second test fails when a catalog block is
  missing from it): through load, resolve, analysis and generation with no
  diagnostic at all, under C++17 and C++20 at the *strict* level with
  `-Werror`, run in a folder of its own with an argument and two lines of
  input, and its whole output checked. Constant folding is
  compared with a reference evaluator on random expressions, and for 400
  random constant expressions the analyser's value must equal the
  reference's, and g++ compiles each as a `constexpr` value with a
  `static_assert` of the analyser's value.
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
  `decode/`, `upgrade_document` in `b2c-catalog/src/upgrade.rs` (block
  upgrades on load, 05 §5.7), `b2c-build/src/build_dir.rs` and
  `check_program` in `b2c-process/src/command.rs`, excluding the generated
  `reserved_names.rs`. Any missed mutant fails the job; timeouts and unviable
  mutants are reported only. Each test process runs under a 2 GiB cap on its
  resident memory (`tools/memory-capped-runner.py`, the job's cargo target
  runner), so a mutant that makes a loop endless and allocating fails its
  tests within seconds and counts as caught, instead of exhausting the
  runner. The cap does not reach the compilers and programs the tests start,
  and it is not an address-space limit: one would be inherited by them, and
  AddressSanitizer programs cannot start under it. A mutant's tests time out
  after 1.5 times the unmutated tests' time, which bounds what a loop that
  does not allocate costs.
* **Desktop unit tests that show the toolbox.** Showing the continuous
  toolbox's flyout draws every catalog block, which M3's wave W2 nearly tripled
  (86 blocks), and happy-dom draws them slowly. A desktop test file that
  mounts the toolbox therefore sets its own test limit,
  `FLYOUT_TEST_TIMEOUT_MS` (20 s, `apps/desktop/src/editor/toolbox/testing.ts`),
  with the measurement that justifies it in the file, instead of a
  suite-wide limit; a file whose slowest test is under 2.5 s leaves it
  unset. A few slower tests (the flyout's own showing times, the keyboard
  navigation file) set a measured limit of their own. Every category that
  gains blocks is measured again. The flyout keeps its blocks between
  showings since W2, so what the limit bounds is the first showing.
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
  it), a flood of 10,000,000 output lines, the setup page, an external
  change (also to a deleted file), the clipboard, settings after a restart,
  the variable menus, the connection checker, selecting a block from its
  code, a drag from every toolbox category, placing blocks with real pointer
  drags (into a C-block off its connection at two zoom levels, a drop on the
  toolbox while a place is previewed, also without a last move, a drop on
  the trash can undone with `Ctrl+Z`, *Show all blocks* keeping the zoom,
  and an empty *if* wrapping statements of `main`, undone and redone, at
  three zoom levels), typed slots (a text value refused by `create [int]`
  and accepted by `create [string]`, a start value swapped by a type change
  and restored in one undo step, a wrong argument refused by a drag and not
  offered by the keyboard mover), and a build and a run with input. Both run
  in the pull request E2E job on both systems; the security tests of
  [08 §8.13](08-security.md#813-continuous-security-process) run nightly.
  Typing in expression slots and Quick Insert are M3 features and are not
  tested in M2. M3's wave W2 adds the flows `collections`, `errors`,
  `structs-enums`, `control` (a switch on a whole number and one on an enum,
  with `B2C-W0508`), `files-run` (a project reading the fixture `lines.txt`
  from its folder, and its *if it can't be opened* part), and
  `functions-globals` (a global listed under *Everywhere*, a default value
  typed into a parameter row), each green at the default size, 1024 × 768
  and 800 × 560. A flow brings a canvas element into the visible canvas
  before it clicks it (`fieldOf`, or `centerOn` and `bringIntoView`): the
  toolbox's flyout stays open over the canvas's left part, which WebDriver's
  `isDisplayed` does not see. It waits for Blockly's delivery
  (`waitForDelivery`) after inserting blocks before it measures them, waits
  for a condition and never for a fixed time, and matches console text
  without relying on its spaces. The selection flow tells one showing of the
  flyout from the next by the ID of its *create* block, since the flyout
  keeps its other blocks. *Without reading docs* is checked by a manual
  usability session with first-time users on both systems before M2 is
  declared done ([the protocol](../manual-tests/m2-usability.md)).
* **The M3 assignments.** The intro-CS assignments that make up M3's exit
  (decision M3-D1 of the
  [next steps](../plans/next-steps.md#33-decisions-before-m3-starts)) are
  acceptance tests in
  [`tests/assignments/`](../../tests/assignments/README.md), with run data
  in the golden tests' form. Until an assignment's block project passes, a
  hand-written reference solution, built with the debug configuration's
  flags and `-Werror`, checks that data, and the project stays in the
  assignment's folder, since every file in `examples/` must pass the tests
  that read all examples. Then the project moves to `examples/`, the run
  data to `tests/golden/`, and the example is a golden test like the
  others. The folder's `status.toml` says which are green, and the test
  (`crates/b2c-build/tests/assignments.rs`) prints the count and fails when
  a pending assignment's project starts to pass, until it is marked green.
  It shares the golden test's run check, which runs each program in a copy
  of its run folder and fails if the program changes anything there.
* **Visual diff.** The guessing game in a 1000×700 window (set through the
  app's window-size seam) at zoom 1.0, light theme, caret hidden, with the
  C++ and bottom panels folded and the *Program* category chosen, compared
  pixel by pixel with pixelmatch (threshold 0.1) and tile by tile: a 16×16
  tile in which pixelmatch counted no pixel has changed when the mean colour
  of its pixels that are flat in the baseline (3 or more of their 8
  neighbours alike, pixelmatch's test for a pixel that is not anti-aliased)
  moves more than 0.015, pixelmatch's OKLab HyAB distance; each tile's sum is
  divided by 256, also for the smaller tiles at the edges. The tile rule
  finds a recolouring too slight for 0.1 on any one pixel, such as the W2
  review's new Types and Files colours (0.06 to 0.08 on every pixel, which
  pixelmatch alone did not count), and leaves strong changes, however small,
  to the pixel rule. More than 0.5% of differing pixels, or of changed tiles,
  fails (on a scene of fewer than 200 tiles, such as the toolbar's 150, one
  changed tile fails), and so does a screenshot of another size than its
  baseline. On every committed baseline both rules let each channel ±3 and
  the whole image 2 lighter through, and 5 lighter fails; the W2
  recolouring fails its 9 scenes at twice the tile limit
  (`apps/desktop/e2e/visual/compare.ts` gives the margins). The chrome's
  scenes (the table's row) are taken in one launch per theme, light and, on
  Linux, dark (`GTK_THEME=Adwaita:dark`), each cut out of a screenshot of
  the window and compared the same way. The whole-window scenes have the
  *Program* category chosen. Before each one the pointer is parked (or rests
  on the drop-down it hovers), nothing has the focus, the caret and
  WebKitGTK's overlay scroll bars are hidden, the fonts have loaded, no
  animation runs, and two screenshots in a row must be the same; the status
  bar's toolchain, the console's elapsed time and the app version in the C++
  panel's first line are masked. Each system has its own baselines in
  `apps/desktop/e2e/visual/baselines/{linux,windows}/`. The E2E job uploads
  its screenshots as candidates on every run (a manual run with the
  `visual-baselines` input only writes candidates); a person reviews them
  and commits the baselines. Until a screenshot's baseline is committed, it
  passes with a warning. It runs in the pull request E2E job and nightly.
  The toolbox's category column always keeps the room of its scroll bar
  (`scrollbar-gutter: stable`), so the canvas sits at the same place
  whether or not the categories fit.
* **Benchmarks.** `criterion` measures the native pipeline (load, resolve,
  analyse, generate, and the whole preview) on a generated 1,000-block
  module. Through the E2E harness, the webview benchmarks measure cold start
  to the editor's readiness marker, the p95 of the preview pipeline's task at
  1,000 blocks (reading the canvas, `update()` in WebAssembly and storing
  the result, found by an in-page heartbeat) and the frame times while
  dragging in a 5,000-block workspace: the gated metric is the p95 over
  every frame of a run's timed drags away and back (about 1,200 in a
  nightly run), and the p95 of each drag away and back is reported. The
  pooled metric (decision D20) has a name of its own,
  `webview.drag-5000.frame-p95-all`, so its nightly history starts empty
  with its first run and needs no `accepted.json` entry; it is reported
  until it has 5 baselines, and the per-pair metric's history goes on as a
  report. The edit benchmark
  includes Blockly's rendering with the C++ panel open (from an edit until
  the page has finished what it started); its gated metric is the p95 over
  every edit of a run (100 in a nightly run), so that a few slow edits do
  not decide it, and the p95 of each sample's 10 edits is reported;
  selecting blocks in 8 scopes at
  1,000 blocks measures the longest task after a click; a program's output
  in the console is timed from a click on ► Run until the console says
  *Finished*, for 200,000 lines (the median of 3 runs per sample; on
  Windows the pseudoconsole throttles the program, so a run takes tens of
  seconds there and may take 2 minutes); and, on
  the guessing game, the longest task after a selection and after an edit
  is reported. Every timing is taken inside the page, never across
  WebDriver. A run takes the median of at least 10 samples (for the edits
  and the drag's frames, the p95 of all of them) and compares it with the
  baseline, the median of
  the last 5 scheduled nightly runs on the default branch for the same
  system, kept in the Actions cache per system (`tools/bench-compare.py`).
  A gated metric more than 10% worse is over the gate; it fails when it was
  also over the gate in one of the 2 runs before (2 of the last 3 runs),
  and the first time it only warns. Every complete scheduled run enters the
  history, one that failed too, with the metrics it found over the gate: the
  hosted runners' speed varies from night to night by more than the gate,
  and a history of only the runs that passed stayed at the fast runners
  (decision
  [D20](../plans/m2-polish-plan.md#d20-how-should-the-nightly-benchmark-gate-deal-with-the-runners-variance)
  of the M2 polish plan). A regression
  that lasts thus fails 2 or 3 nights and then becomes the baseline, so a
  red night is acted on when it comes (a fix, or an entry in
  `accepted.json`, below). Until 5 baselines exist the comparison is only
  reported.
  * Every E2E flow records its longest main-thread task (a heartbeat in the
    page, from the window's readiness until the test ends); the report never
    fails a test.
  * The app measures its preview pipeline and each of its phases, each
    rebuild of the toolbox's flyout, the selection handler, the code
    panel's update and the keyboard mover's listing of places
    (`b2c:keyboard-targets`) as User Timing spans (`b2c:…`); nothing in the
    app shows them. The long-task recorder names the spans that overlap
    each task it keeps, and the benchmarks report each span's time per edit
    or click beside their metrics; the E2E hook gives each span's latest
    duration.
  * A change expected to alter a gated metric runs the A/B benchmark job on
    its pull request (`bench-ab-pr.yml`): the head (the pull request merged
    into its base branch) and the base (that branch) are built and measured
    in alternating runs on one runner, at least 10 samples each, and the
    head's medians are compared with the base's at the same 10% (for the
    edits and the drag's frames, the p95 over all of each side's edits or
    frames, its runs pooled). By hand
    (`bench-ab.yml`), the job measures the commit it runs on against one of
    that commit's ancestors, and nothing else (§9.3). Each benchmark runs
    on its own: one that the base's build cannot run (its E2E hook lacks a
    method the benchmark calls) is skipped, one that fails is left out of
    the comparison, and the summary lists both. The job fails on a gated
    regression or on a benchmark that failed on the head.
  * A change of a gated metric that is meant is recorded in
    `apps/desktop/e2e/bench/accepted.json` with its reason and its kind:
    `redefined` (the benchmark now measures something else) or `accepted`
    (an intended slowdown). Either kind restarts that metric's baseline:
    the first nightly run that sees the entry does not fail on it, does not
    count it as over the gate, and starts the metric's history again. An
    A/B run whose head adds an `accepted` entry passes; a redefinition never
    waives an A/B difference, because both builds are measured with the
    head's benchmarks.
  * The M2 polish wave's Variables listing, zoom-aware snap radius and
    screen-reader console output (W4-2, W5-1 and W3-11 of the
    [plan](../plans/m2-polish-plan.md)) merge only with a green A/B run
    attached whose table compares their metric, or once their metrics have
    at least 5 nightly baselines. Their metrics are the selection at 1,000
    blocks, dragging at 5,000 blocks and the console's output; a pull
    request's A/B run takes every benchmark but cold start. Dragging at
    5,000 blocks moves a definition where nothing can connect, so it never
    previews a place: what the snap radius's previews cost is not gated in
    M2 (the plan's W5-1 status keeps it open). A benchmark the
    base build cannot run, or that fails on the base, is left out without
    failing the job, and the summary then ends "Not conclusive" for it: such
    a run does not count for the rule.
  * Typed slots have two benchmarks at 1,000 blocks: the frame times of a
    number block dragged slowly down a column of 200 `create` blocks of
    alternating types, in a module whose other blocks set and print those
    variables, so that every move asks about typed slots
    (`webview.drag-typed-1000.frame-p95-all`, the p95 over every frame,
    reported; it is to be gated once it has 5 baselines, as the pooled drag
    metric was), and the time to list the keyboard mover's places for a
    number block (`webview.keyboard-list-1000.ms`, reported, against N12's
    100 ms). A base build without the `b2c:keyboard-targets` span skips the
    listing in an A/B run, with a note.
  * M3's wave W2 adds three reported benchmarks: the native pipeline on a
    1,000-block module of the collection blocks
    (`native.pipeline.preview-collections`) and of W2's blocks of every kind
    with definitions (`native.pipeline.preview-language`), each a shape of
    its own so that `native.pipeline.preview` keeps its history (the A/B
    job reads the webview workspaces with the base build's catalog, which
    refuses W2's blocks, so those stay as they were); and the toolbox's
    refresh after a new variable
    (`webview.declare-variable.toolbox-refresh`, on the guessing game and at
    1,000 blocks, with `webview.declare-variable.max-task`), to be gated
    once each has 5 baselines.
  * M2 gates regressions. Absolute interaction budgets on small programs are
    reported in M2 and gated from M3 once the toolbox refreshes per section.
    Meeting N2–N4 and N12 ([01 §1.4](01-overview.md#14-goals)) at
    1,000–5,000 blocks is part of M5.

## 9.3 Continuous integration

GitHub Actions workflows. All actions are SHA-pinned and least-privilege
([08 §8.9](08-security.md#89-supply-chain)).

| Workflow | Trigger | Jobs |
| ---------- | --------- | ------ |
| `ci.yml` | PR, push to any branch, weekly, manual | `lint` (rustfmt, clippy, also for the Windows target, markdownlint, the self-tests of the weekly jobs' scripts) · `test` (ubuntu, windows) · `test-cgroup` (the Linux cgroup path) · `gcc` (GCC 11 and 15 containers) · `wasm` (build + size budget) · `coverage` (the gates of §9.2) · `docs` (rustdoc, lychee link check, links and heading anchors of every Markdown file, pinned versions in `BUILDING.md` and nowhere else, diagnostic codes documented, generated IPC types and catalog outputs up to date) · `deny` · `secrets` (gitleaks) · `security` (pnpm audit, security suite) · `fuzz` (60 s per target) |
| `desktop.yml` | PR and push touching the app, the crates, the packages or the Rust and Node configuration; manual (optionally making new visual-diff candidates) | `frontend` (`tsc --noEmit`, eslint, prettier, package layering) · `test-web` (Vitest with coverage, after building the WASM core; the unit tests of the E2E harness, the benchmarks and the visual diff; the benchmark comparison's self-test and the acceptance file's check) · `app` (build, tests, release checks and start check on ubuntu and windows) · `e2e` (ubuntu, windows: the exit test and the breadth flows, then the visual diff; the Trusted Types summary, which is meaningful on WebView2; the longest main-thread task of each flow, reported) |
| `codeql.yml` | PR, push, weekly | CodeQL: JavaScript/TypeScript, Rust, GitHub Actions (code scanning, see below) |
| `osv-scanner.yml` | PR, push, daily | OSV-Scanner over every lockfile; fails on known vulnerabilities and uploads results to code scanning when available |
| `scorecard.yml` | Push to `main`, weekly, branch-protection changes | OpenSSF Scorecard (code scanning, see below) |
| `workflow-audit.yml` | PR and push touching `.github/`; weekly and manual (adding online audits) | zizmor (auditor persona) |
| `pages.yml` | PR and push touching `docs/`, `site/`, `LICENSE`, `SECURITY.md`, the Node/pnpm config or the workflow itself; manual | Build the specification website (fails on broken links or anchors, unpublished docs and raw HTML); deploy to GitHub Pages from the default branch |
| `nightly.yml` | Daily, manual | `fuzz` (30 minutes per target, one job per target, with a kept corpus) · `links` (lychee over all Markdown) · `e2e-app` (the app under test, built once per OS for the jobs below) · `e2e` (both OSes, every flow and the visual diff) · `e2e-security` (both OSes, [08 §8.13](08-security.md#813-continuous-security-process)) · `bench` (both OSes, with the 10% gate over 2 of the last 3 scheduled runs and the acceptance file) |
| `bench-ab-pr.yml` | The `bench-ab` label on a PR (and each push while it has the label) | `base` (the PR's merge commit is the head, its first parent the base) · `ab` (`bench-ab-run.yml`, every benchmark but cold start, both OSes) |
| `bench-ab.yml` | Manual (base, benchmarks, systems) | `base` (the dispatched commit is the head; checks that the base is one of its ancestors, by default its merge base with the default branch) · `ab` (`bench-ab-run.yml`) |
| `bench-ab-run.yml` | Called by the two above | `plan` (checks the inputs) · `app` (the app under test from the base and from the head, per OS) · `bench` (ubuntu, windows: the head's benchmarks against both builds in alternating runs on one runner, 10 samples each, each benchmark on its own, those the base cannot run skipped; compared at 10% with the head's acceptance file; §9.2) |
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
* A pull request's code runs only in `pull_request` runs, which GitHub gives
  a read-only token and the pull request's own cache scope. A run that can
  write to the Actions cache of the default branch, from which other runs
  restore (push, schedule, manual), never checks out a pull request: code
  running there could take the run's cache token and plant entries (CodeQL
  `actions/cache-poisoning`). The A/B job is therefore two workflows that
  call `bench-ab-run.yml`: `bench-ab-pr.yml` for a pull request, and
  `bench-ab.yml` by hand, which builds only the dispatched commit and one
  of its ancestors. Its head is always the calling run's own commit, and
  none of its jobs saves to the cache.
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
* The `docs` job's `tools/check-build-docs.py` checks the versions in
  `BUILDING.md` against their sources (`Cargo.lock`, `rust-toolchain.toml`,
  `package.json`'s `engines` and `packageManager`, `.node-version` and the
  workflows' pinned tool versions) and its Debian `apt` packages against the
  `app` job of `desktop.yml`, and fails when any other Markdown file states
  one of these versions or installs those packages. A statement is a tool's
  name followed by a version in the same sentence (Node.js in any letter
  case, `engines.node` included, but not `@types/node`; binaryen's release
  right after its name or after the word version or release), or the exact
  wasm-bindgen, Rust or pnpm pin anywhere. Dated records (`CHANGELOG.md`,
  `docs/plans/`, `docs/adr/`) are exempt, and a dependency table's row of a
  pinned tool is checked against its source instead. Its `--self-test`
  breaks each rule, and bumps wasm-bindgen in a scratch copy of the
  repository.
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
only reports a metric until five scheduled runs per system have kept it,
and since 2026-10-24 keeps every complete scheduled run and fails a metric
over the gate in 2 of the last 3 (§9.2); the A/B job (`bench-ab-pr.yml`,
`bench-ab.yml` and `bench-ab-run.yml`) and the long-task recorder came
with the M2 polish wave, and the A/B job gates a
labelled pull request at once. The visual diff compares every screenshot
with its baseline since the baselines were committed on 2026-10-24; a new
scene only warns until its baseline is committed.

## 9.4 Documentation

Documentation is part of the product and is maintained **in the same PR** as
the code it describes.

| Doc | Location | Audience | Maintenance |
| ----- | ---------- | ---------- | ------------- |
| Specification | `docs/spec/` | Contributors | Living document with status and last-reviewed date per chapter |
| Specification website | `site/` → GitHub Pages | Everyone | **Generated** from `docs/spec/` and `docs/adr/` on every change; the build fails on broken links |
| ADRs | `docs/adr/` | Contributors | One per significant decision (template in `docs/adr/README.md`); superseded, never deleted |
| User guide + tutorials | `docs/user-guide/` (Markdown; the mdBook build and the bundled copy come with the offline help in M5) | Users, teachers | Updated with the features it describes; screenshots, once added, are regenerated by E2E tests. M2 has the [getting-started page](../user-guide/getting-started.md) |
| Threat-model reviews | [`docs/security/threat-model-reviews.md`](../security/threat-model-reviews.md) | Maintainers | One entry per milestone ([08 §8.13](08-security.md#813-continuous-security-process)): date, commit, scope, findings, accepted residuals |
| Manual test protocols | `docs/manual-tests/` | Testers, maintainers | One protocol per milestone check that cannot be automated (screen readers, usability, the toolchain matrix), with its results recorded in the protocol ([index](../manual-tests/README.md)) |
| Block reference | `docs/reference/blocks/` | Users | **Generated** from the catalog; CI fails if stale |
| Diagnostics reference | `docs/reference/diagnostics/` | Users | **Generated** from the message catalog plus explanations; CI fails if a code is undocumented |
| API docs | rustdoc / TSDoc | Contributors | Built in CI; `missing_docs` warnings |
| CLI reference | `docs/reference/cli.md` | Users, CI authors | Generated from `clap` definitions |
| `BUILDING.md` | Repo root | Everyone who builds from source (all users until M6), contributors | The one place with prerequisites, install commands and build and run steps per system, kept apart from design notes; its Debian and Windows packages and build commands are the ones CI uses |
| Developer guide | `docs/developer-guide/` | Contributors | Architecture, security settings, dependency table, testing, recipes. The dependency table (version, release date, licence, why), the *Adding an IPC command* recipe and the list of Blockly seams ([ADR-0014](../adr/0014-extending-blockly.md)) are updated in the PR that adds a dependency, a command or a seam |
| Desktop app README | `apps/desktop/README.md` | Contributors | What the app is, its code map and links; no build steps or design notes |
| `CONTRIBUTING.md` | Repo root | Contributors | Set-up (a link to `BUILDING.md`), the rules for a change, the one list of the checks CI runs, and where things are documented; updated with the CI workflows |
| `README.md`, `SECURITY.md`, `CHANGELOG.md` | Repo root | Everyone | The README separates what works today from the goals for 1.0 and carries no commands; `CHANGELOG.md` follows the Keep a Changelog format |

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
  as `Cargo.lock`); `tools/check-build-docs.py` checks that for the pinned
  versions and the Debian packages (§9.3).
* The checks CI runs are listed together in `CONTRIBUTING.md`; other
  documents link to that list rather than repeat it.
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
