# Benchmarks

The performance benchmarks of [09 §9.2](../../../../docs/spec/09-quality-and-delivery.md#92-testing-strategy)
and their regression gate. M2 measures and gates regressions; meeting the absolute targets N2–N4
and N12 of [01 §1.4](../../../../docs/spec/01-overview.md) is M5's exit, and absolute budgets on
small programs are only reported until M3. The nightly `bench` job of
[`nightly.yml`](../../../../.github/workflows/nightly.yml) runs them on Ubuntu (under `xvfb`) and
Windows and compares them with the baseline; the A/B job compares a pull request's build with its
base's on one runner ([A/B runs](#ab-runs)).

| Metric                                            | What is measured                                                                                                                                                                                                                     | Gated | Target        |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----- | ------------- |
| `native.pipeline.preview`                         | criterion: load, resolve, analyse and generate the 1,000-block document natively, as the editor's WebAssembly core runs them                                                                                                         | yes   | N4            |
| `native.pipeline.{load,resolve,analyze,generate}` | criterion: each stage on its own                                                                                                                                                                                                     | no    |               |
| `webview.cold-start`                              | From the WebDriver session request (which starts the app) to the readiness marker: the start page shown, Blockly's workspace injected and the toolbox rendered; 10 starts with fresh profiles, after one warm-up start               | yes   | N2: < 2 s     |
| `webview.preview-1000.p95`                        | The preview pipeline's task in the app at 1,000 blocks, C++ panel folded: reading the canvas, `update()` in WebAssembly (one load of the document), storing the result; p95 of 10 edits per sample, 10 samples                       | yes   | N4: < 50 ms   |
| `webview.edit-1000.p95`                           | From an edit until the page has finished what it started (Blockly's re-rendering of the edited stack, the preview, the C++ panel's update), with the C++ panel shown, less the pipeline's 50 ms debounce; p95 of 10 edits per sample | yes   | N12           |
| `webview.select-1000.max-task`                    | The longest main-thread task after a click that selects a block at 1,000 blocks, in 8 scopes in turn; one click per sample, 10 samples after a warm-up click                                                                         | yes   | N12           |
| `webview.guessing-game.select.max-task`           | The guessing game (40 blocks, C++ panel shown): the longest task after a click that selects a block in another scope; one click per sample                                                                                           | no    | N12: ≤ 50 ms  |
| `webview.guessing-game.edit.max-task`             | The guessing game: the longest task after a print is added to `main`, until the page has finished what it started; one edit per sample                                                                                               | no    | N12: ≤ 50 ms  |
| `webview.console-200000.run`                      | From a click on ► Run until the console says _Finished_, for a built program that prints 200,000 lines (the console cleared before); the median of 3 runs per sample                                                                 | yes   |               |
| `webview.console-200000.max-task`                 | The longest main-thread task of such a run, until the page has finished what the run started; the median of 3 runs per sample                                                                                                        | no    | N12           |
| `webview.drag-5000.frame-p95`                     | Frame times while a block is dragged away and back with real pointer input in a 5,000-block workspace; p95 of one pair of drags per sample, 10 samples after a warm-up pair                                                          | yes   | N3: ≤ 33.3 ms |

Every timing of the webview benchmarks is taken inside the page with its own clock
([`page.ts`](page.ts)), so WebDriver's round trips are not part of any measurement. A heartbeat (a
message the page posts to itself again and again) finds the page's long tasks: between two beats
the page ran something else, a task or its rendering, and a gap of 4 ms or more counts. Each
benchmark takes 10 samples, or `B2C_BENCH_SAMPLES` (the A/B runs take fewer per run and add them
up). The selection benchmarks go round their list of blocks from target `B2C_BENCH_OFFSET` (0 by
default; the A/B runs start each round where the round before stopped).

While an edit or a click is timed, the page also observes the app's User Timing spans
([`src/lib/timing.ts`](../../src/lib/timing.ts): the preview pipeline `b2c:preview` and its phases,
`b2c:toolbox-refresh`, `b2c:select` and `b2c:code-panel`). The preview, selection and
guessing-game benchmarks print the median time of each span per interaction under their summary
line, which says where the time went (`spans per click (median): b2c:toolbox-refresh … ms, …`).
The spans are not metrics; an app built before they existed shows none. The preview's phases are
`b2c:preview:read`, `b2c:preview:update` (the core's single `update()`), `b2c:preview:commit` and
`b2c:preview:publish`; an app built before the single `update()` export shows
`b2c:preview:canonical` and `b2c:preview:analyse` instead of `b2c:preview:update`, so in an A/B run
against such a base, compare their sum with it. The preview benchmark also prints, for the edits
with the C++ panel shown, the medians of when the edit's C++ was in the editor's state, when the
page settled and the longest task (`per edit, C++ panel shown (median): C++ in the state after …
ms, settled after … ms, longest task … ms`), so a slower edit shows whether its time went before
the C++ appeared or after it, which the spans alone do not.

- **Cold start** ([`launch.ts`](launch.ts)): the time starts just before the session request and
  ends at the page clock's time (its time origin plus `performance.now()`) when the readiness marker
  first holds, checked every 4 ms. Right after the session starts the window may not show the app's
  page yet, and may replace its document while the page is asked (msedgedriver then reports a
  script timeout long before the script's own). So the window is first looked at with short
  synchronous scripts, every 10 ms, until it shows the app's page (`tauri://localhost` on Linux,
  `http://tauri.localhost` on Windows), and the wait for the marker runs there; a wait cut short by
  a new document is started again in that document (at most 5 waits, and only until 60 s after the
  session request). The time is always taken from the session request, so a restart can only make
  a start slower. When the marker already holds at the first look at a document, or when its wait
  begins, the time is an upper bound. The benchmark's summary line counts those starts, and the
  restarted ones.
- **Preview and edits** ([`preview.bench.e2e.ts`](preview.bench.e2e.ts)): a print with a unique
  text is added at the end of `main` through the test hook, and the page checks the hook's
  `code()` until the text is there ([`interaction.ts`](interaction.ts) `timedEdit`). With the C++
  panel folded, the first long task after which the text is in the code is the pipeline's run,
  which happens in one task because its WebAssembly calls are synchronous. Then, with the panel
  shown, the page also waits until it has run no long task for 300 ms, and the edit lasts until
  the end of the last one. The print is then deleted with the keyboard (selected, then _Delete_),
  so every edit is made to the same 1,000 blocks; in the second phase the page is also left to
  settle afterwards, outside the timing.
- **Selecting** ([`select.bench.e2e.ts`](select.bench.e2e.ts), the guessing game's in
  [`guessing.bench.e2e.ts`](guessing.bench.e2e.ts)): the blocks of [`targets.ts`](targets.ts) are
  clicked in turn with real pointer input, the button held for 50 ms as a person's click holds it,
  each in another scope than the one before, so every click changes the selection (the toolbox
  does not follow it: it lists every variable of the module). Before each click, outside the
  timing, the canvas is scrolled through the test hook (`scrollCanvasBy`, which keeps the
  selection, unlike a drag of the background) until the block is well inside the visible canvas,
  and the page is left to settle (10 frames in a row within 50 ms). The page times its tasks from
  the press until it has run no long task for 300 ms ([`interaction.ts`](interaction.ts)
  `timedClick`); the longest task that ended after the press is the sample. The block must not be
  selected before and must be selected after. At 1,000 blocks the C++ panel is folded, as in the
  drag benchmark, to leave the canvas room on a small window.
- **Guessing game edits** ([`guessing.bench.e2e.ts`](guessing.bench.e2e.ts)): as the second phase
  of the preview benchmark, in the example `examples/guessing_game.b2c`; the sample is the longest
  task from the insertion (which is part of it) until the page has settled.
- **Console output** ([`console.bench.e2e.ts`](console.bench.e2e.ts)): a new project from the
  _Empty_ template gets, through the test hook, a program that prints a first line, 200,000 lines in
  a _repeat_ loop and a last line, and is run once to build it. Each run then clears the console,
  outside the timing, and clicks ► Run with real pointer input; the page times from the press until
  the console's state line says _Finished_ (`TASKS_SETTLE_SCRIPT` waits for it to leave _Finished_
  and come back), and records its tasks until it has run no long task for 300 ms. The run is the
  program writing its lines and the console showing them (the backend's output events, the
  terminal's parsing and rendering, and, once it is on, its accessibility tree), which keeps the
  page busy most of the time; it varies by about 10% from run to run, so a sample is the median of 3
  runs. The time stops at _Finished_ rather than when the page settles: a few short tasks of the
  window's chrome come after the run on timers, which made that end point vary twice as much.
  After each run the console must end with the last line and hold no "… N lines skipped" marker:
  the output, about 3 MB, stays below the 4 MiB the backend lets go unacknowledged, so nothing is
  dropped. The program runs on the machine's g++, as in the end-to-end tests. On Windows the
  pseudoconsole throttles the program to the speed at which it renders its own screen (its pipe
  fills and the program waits), so a run takes tens of seconds there rather than about one: a run
  may take 2 minutes on Windows and 25 s elsewhere.
- **Drag** ([`drag.bench.e2e.ts`](drag.bench.e2e.ts)): a separate `func.define` beside `main` is
  dragged away and back across empty canvas in 60 moves of 16 ms each, and the page records the
  timestamps of the animation frames that run while the pointer drags. Before each drag, outside
  the timed part, the canvas is panned until the handle is well inside the part of it that is
  visible then, and the page is left to settle (10 frames in a row within 50 ms). The drag ends at
  a point of that part ([`handle.ts`](handle.ts) `aimDelta`): up and to the right for the drag
  away, down and to the left for the drag back. The toolbox's flyout stays open over the canvas's
  left part and its width changes as the toolbox follows the program, and a drop on it would
  delete the handle; on the Windows runner the window is small (about 1,030 by 750 pixels), so
  little canvas is left. After each drop the page is left to finish what the drop started (the
  same 10 frames), and the handle must have moved with the pointer: on the canvas (scrolling
  aside) it must have landed within half the drag's length of where the pointer left it, or the
  run fails rather than measure a drag that missed the handle. A handle that cannot be grabbed
  fails the run with where it is on screen, and a drop that loses it with what the page looked
  like (the drag, the window, the elements under the drop point, where the flyout, the zoom
  controls and the trash can are, and the last press and release).

The benchmarks turn off the end-to-end harness's long-task recorder (`launchApp`'s
`recordLongTasks: false`, see [`../README.md`](../README.md)): they time the page's tasks
themselves.

Right after the launch, each benchmark that uses the test hook checks that the app's hook has the
methods it calls ([`capabilities.ts`](capabilities.ts) lists them per benchmark; cold start calls
none). A missing method fails the benchmark at once, naming the method, unless
`B2C_BENCH_SKIP_NOTE` (an absolute path) is set: then the benchmark is skipped and writes why into
that file. The A/B job sets it for the base's runs, whose build may be older than the benchmarks
([A/B runs](#ab-runs)). A benchmark that starts to call another hook method adds it to its list.

## The generated document

[`document.ts`](document.ts) here and
[`benches/pipeline/document.rs`](../../../../crates/b2c-core-wasm/benches/pipeline/document.rs) in
`b2c-core-wasm` generate the same documents: one module whose `program.main` holds units of 8 blocks
(a variable, a counted loop holding an `if … else` that changes it, and a print of an arithmetic
expression with a `var.get` in it), then single prints up to the requested count; the drag benchmark's
document adds the separate `func.define dragMe`. Both generators check a SHA-256 of the document (as
compact JSON with sorted keys) against the same constants, so changing one generator without the
other fails a test (`document.test.ts` here, `cargo test -p b2c-core-wasm --test bench_document`
there). The Rust test also checks that the documents load and preview with no error.

## Running them locally (Linux)

The native benchmark (a short run with `--quick`; the nightly job runs 50 samples per benchmark):

```sh
cargo bench -p b2c-core-wasm --bench pipeline -- --quick
```

The webview benchmarks need the app under test, `tauri-driver`, `WebKitWebDriver` and `xvfb`, as the
end-to-end tests do (see [`../README.md`](../README.md)); a file name after `--project bench` runs
only that benchmark (`select.bench`, for example):

```sh
B2C_BENCH_OUT=/tmp/b2c-bench WEBKIT_DISABLE_DMABUF_RENDERER=1 xvfb-run -a \
  pnpm --filter @blocks2cpp/desktop exec vitest run --config e2e/bench/vitest.bench.config.ts --project bench
```

Each benchmark writes one results file per metric (`<metric>.json`) into `B2C_BENCH_OUT` (an
absolute path; by default `bench/` in the harness's artifacts folder). The benchmarks' own logic is
tested without the app:

```sh
pnpm --filter @blocks2cpp/desktop exec vitest run --config e2e/bench/vitest.bench.config.ts --project bench-unit
```

## The comparison

[`tools/bench-compare.py`](../../../../tools/bench-compare.py) (Python 3, no dependencies) converts
criterion's samples into a results file and compares a run with the history of its system, or a
head build's runs with a base build's:

```sh
python3 tools/bench-compare.py criterion --dir target/criterion --group pipeline --os linux \
  --gate preview --out /tmp/b2c-bench/native.pipeline.json
python3 tools/bench-compare.py compare --os linux --results-dir /tmp/b2c-bench \
  --history history.json --accepted apps/desktop/e2e/bench/accepted.json \
  [--update-history history.json --run-id ID --commit SHA]
python3 tools/bench-compare.py ab --os linux --base /tmp/ab/base --head /tmp/ab/head \
  --accepted apps/desktop/e2e/bench/accepted.json --accepted-base base-accepted.json \
  --runs /tmp/ab/ab-runs.tsv
python3 tools/bench-compare.py check-accepted apps/desktop/e2e/bench/accepted.json
python3 tools/bench-compare.py --self-test
```

- Each metric needs at least 10 samples; its median is compared with the baseline, the median of
  the medians of the last 5 runs in the history that have the metric in the same unit.
- A gated metric more than 10% worse fails the run (exit status 1). Until a metric has 5 baselines,
  or for an ungated metric, the comparison is only reported. Unusable input exits with status 2.
- The table goes to standard output (as UTF-8 on every system) and, with `--summary`, to the job
  summary.
- With `--update-history`, a run without an unaccepted regression is added to the history, which
  keeps the last 5 runs. The nightly job keeps the history of each system in the Actions cache
  (`bench-history-<system>-<run>`), and only a scheduled run (always on the default branch) in which
  every benchmark ran and none regressed unaccepted is kept. A manual run compares without changing
  it.

### Redefinitions and intended slowdowns: `accepted.json`

A change that makes a gated metric slower on purpose records it in [`accepted.json`](accepted.json)
(format version 2), one entry per metric and system, of one of two kinds:

- `redefined`: the benchmark now measures something else (a definition that measures more), so the
  old baselines no longer compare;
- `accepted`: an intended slowdown (a feature that costs time).

```json
{
  "metric": "webview.edit-1000.p95",
  "system": "linux",
  "commit": "<the commit whose performance is accepted: 7 to 40 lower-case hex digits>",
  "kind": "redefined | accepted",
  "reason": "One line: why the slowdown is intended"
}
```

- The first nightly run that sees an entry of either kind, and has its metric, does not fail on that
  metric: the metric's history restarts with that run (its older medians are dropped) and the
  history notes the entry as applied, so it applies once. The metric is then reported until it has
  5 new baselines.
- An A/B run does not fail on a metric that the head's file accepts (`accepted`) and the base's file
  does not: the change under test brings the entry. For an entry added together with the change,
  `commit` is the pull request's head commit (the A/B run measures it merged into the base).
- A redefinition never waives an A/B difference: the head's benchmarks measure both builds, so both
  medians follow the new definition, and a difference between them is the change's.
- Every entry needs its kind (`check-accepted` refuses a file without, and so do the nightly and
  A/B comparisons; only an A/B run's base file may still be format version 1, without kinds).
- Entries stay as a record; one taken out of the file is also forgotten by the history. The history
  file (format version 2) lists the applied entries; version 1 files are still read.

### A/B runs

The A/B job gates a pull request at once, also for metrics that have no history yet. Its jobs are
those of [`bench-ab-run.yml`](../../../../.github/workflows/bench-ab-run.yml), and two workflows
start them:

- [`bench-ab-pr.yml`](../../../../.github/workflows/bench-ab-pr.yml), when the `bench-ab` label is
  added to a pull request (and on each push while it has the label). The head is the pull request
  merged into its base branch (the merge commit GitHub makes for it) and the base is that branch,
  the merge commit's first parent: the two builds differ by the pull request's changes only, so
  commits that landed on the base branch after the pull request branched off count for neither
  side, and the head's `accepted.json` is the merged file. It runs every benchmark but
  `cold-start`, on both systems.
- [`bench-ab.yml`](../../../../.github/workflows/bench-ab.yml), by hand on the branch or tag to
  measure (_Use workflow from_). The head is the commit the run is for; no input changes it. The
  base must be one of the head's ancestors: by default where the head branched off the default
  branch (their merge base; on the default branch itself that is the head), or the branch, tag or
  commit that the `base` input names. Any other base fails the run before anything is built. It
  takes the benchmarks (`preview`, `select`, `guessing`, `console`, `cold-start`, `drag`; by
  default all but `cold-start`) and the systems (both, Linux or Windows).

The split is the trust boundary (CodeQL's `actions/cache-poisoning`): the jobs build and run the
code of both commits. A pull request's code runs only in a `pull_request` run, which has a
read-only token and the pull request's own cache scope. A manual run can write to the cache scope
of the branch it runs on, the default branch's included, from which other runs restore, so it
builds only commits of the repository's own history. No A/B job saves to the Actions cache.

The jobs build the app under test from each commit, and then on one runner per system
[`ab-rounds.sh`](ab-rounds.sh) runs the head's benchmarks against both builds in alternating runs
(base, head, head, base: 2 rounds of 5 samples, 10 per side), and `bench-compare.py ab` compares
the head's medians with the base's at 10%. Each round starts the selection benchmarks where the
round before stopped (`B2C_BENCH_OFFSET`, the same for both builds), so each side clicks the same
10 targets as a nightly run, all 8 scopes of `select-1000` among them.

Each benchmark runs in a Vitest call of its own, and one that cannot be compared does not stop the
others:

- The head's harness measures both builds, so a benchmark that calls a test-hook method the base's
  build lacks cannot run on the base (`scrollCanvasBy` came with the W2-1 core, so the selection
  benchmarks need a base from after it). The base's runs set `B2C_BENCH_SKIP_NOTE`, so such a
  benchmark is skipped there, with the reason, and not run on the head either.
- The head's harness also generates the documents both builds open, so they use only blocks that
  every block catalog since 1.0.0 reads the same way (the down-by step of each unit is a plain
  _change_ by `-1`, not the _down by_ option catalog 1.2.0 added). A build that still shows a block
  of the opened document as a placeholder (it cannot read it, so it would work on another program)
  fails the benchmark when it opens the document (`openProjectFile` in [`project.ts`](project.ts)),
  so the run ends "Not conclusive" for it instead of comparing two different programs.
- A benchmark that fails is not started again; the head still runs it in the round where the base
  failed, so that a failure of the head's build is found too. Each run's artifacts go into
  `$B2C_E2E_ARTIFACTS/<side>/round-<n>/<benchmark>/`, and the job shows the logs of a failed run.

Only the benchmarks that ran in every round on both sides are compared (the others' results files
move to `left-out/`). `ab-rounds.sh` records what happened to each benchmark in `ab-runs.tsv`, and
`bench-compare.py ab --runs` lists the skipped and failed ones under the table. The job fails on a
gated regression or on a benchmark that failed on the head; a skip, or a failure on the base, does
not fail it, but the summary then ends "Not conclusive" for those benchmarks (and the job shows a
warning): the run says nothing about their metrics.

A change expected to alter a gated metric runs the A/B job on its pull request. W4-2, W5-1 and W3-11
of the M2 polish wave merge only with a green A/B run attached whose table compares their metric,
or once their metrics have at least 5 nightly baselines: the toolbox that no longer follows the
selection (W4-2) is measured by `select-1000.max-task`, the snap radius (W5-1) by `drag-5000.frame-p95`, and the console's
screen-reader output (W3-11) by `console-200000.run`, all part of a pull request's run. `drag-5000` drags a definition
across empty canvas, where nothing can connect: it shows that the radius does not slow a drag down, but it never
previews a place, so what W5-1's previews cost is not gated (the plan's W5-1 status keeps that open). A run that
is not conclusive for the benchmark of one of them (the base build cannot run it, or it failed on
the base) does not count. Locally, with two builds of the app:

```sh
AB_XVFB=1 apps/desktop/e2e/bench/ab-rounds.sh /tmp/base/blocks2cpp-desktop \
  /tmp/head/blocks2cpp-desktop /tmp/ab preview.bench select.bench guessing.bench console.bench \
  drag.bench
python3 tools/bench-compare.py ab --os linux --base /tmp/ab/base --head /tmp/ab/head \
  --runs /tmp/ab/ab-runs.tsv
```

Results files (`blocks2cpp/bench-results`, one per producer), the history
(`blocks2cpp/bench-history`) and the acceptance file (`blocks2cpp/bench-accepted`) are JSON with a
format tag, checked strictly when read (sizes, names, finite non-negative samples, no unknown or
duplicate keys); the script's docstring describes them.
