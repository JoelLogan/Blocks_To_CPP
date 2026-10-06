# Benchmarks

The performance benchmarks of [09 §9.2](../../../../docs/spec/09-quality-and-delivery.md#92-testing-strategy)
and their regression gate. M2 measures and gates regressions; meeting the absolute targets N2–N4 of
[01 §1.4](../../../../docs/spec/01-overview.md) is M5's exit. The nightly `bench` job of
[`nightly.yml`](../../../../.github/workflows/nightly.yml) runs them on Ubuntu (under `xvfb`) and
Windows and compares them with the baseline.

| Metric                                            | What is measured                                                                                                                                                                                                       | Gated | Target        |
| ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----- | ------------- |
| `native.pipeline.preview`                         | criterion: load, resolve, analyse and generate the 1,000-block document natively, as the editor's WebAssembly core runs them                                                                                           | yes   | N4            |
| `native.pipeline.{load,resolve,analyze,generate}` | criterion: each stage on its own                                                                                                                                                                                       | no    |               |
| `webview.cold-start`                              | From the WebDriver session request (which starts the app) to the readiness marker: the start page shown, Blockly's workspace injected and the toolbox rendered; 10 starts with fresh profiles, after one warm-up start | yes   | N2: < 2 s     |
| `webview.preview-1000.p95`                        | The preview pipeline's task in the app at 1,000 blocks: reading the canvas, `canonical()` and `preview()` in WebAssembly, storing the result; p95 of 10 edits per sample, 10 samples                                   | yes   | N4: < 50 ms   |
| `webview.edit-1000.p95`                           | From an edit to its C++ in the editor's state, less the pipeline's 50 ms debounce (adds Blockly's re-rendering of the edited stack)                                                                                    | no    |               |
| `webview.drag-5000.frame-p95`                     | Frame times while a block is dragged away and back with real pointer input in a 5,000-block workspace; p95 of one pair of drags per sample, 10 samples after a warm-up pair                                            | yes   | N3: ≤ 33.3 ms |

Every timing of the webview benchmarks is taken inside the page with its own clock
([`page.ts`](page.ts)), so WebDriver's round trips are not part of any measurement:

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
- **Preview** ([`preview.bench.e2e.ts`](preview.bench.e2e.ts)): a print with a unique text is added
  at the end of `main` through the test hook, and the page checks the hook's `code()` until the
  text is there. A heartbeat (a message the page posts to itself again and again) finds the long
  tasks meanwhile; the first one after which the text is in the code is the pipeline's run, which
  happens in one task because its WebAssembly calls are synchronous. The print is then deleted with
  the keyboard (selected, then _Delete_), so every edit is made to the same 1,000 blocks.
- **Drag** ([`drag.bench.e2e.ts`](drag.bench.e2e.ts)): a separate `func.define` beside `main` is
  dragged away and back across empty canvas in 60 moves of 16 ms each, and the page records the
  timestamps of the animation frames that run while the pointer drags. Before each drag, outside
  the timed part, the canvas is centred on the handle again and the page left to settle (10 frames
  in a row within 50 ms): a drop does not always leave the handle, or the canvas's view, exactly
  where the pointer moves suggest, and over many drags it could drift off screen. After each drop
  the page is left to finish what the drop started (the same 10 frames), and the handle must have
  moved with the pointer: on the canvas (scrolling aside) it must have landed within half the
  drag's length of where the pointer left it ([`handle.ts`](handle.ts)), or the run fails rather
  than measure a drag that missed the handle. A handle that cannot be grabbed fails the run with
  where it is on screen.

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
end-to-end tests do (see [`../README.md`](../README.md)):

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
criterion's samples into a results file and compares a run with the history of its system:

```sh
python3 tools/bench-compare.py criterion --dir target/criterion --group pipeline --os linux \
  --gate preview --out /tmp/b2c-bench/native.pipeline.json
python3 tools/bench-compare.py compare --os linux --results-dir /tmp/b2c-bench \
  --history history.json [--update-history history.json --run-id ID --commit SHA]
python3 tools/bench-compare.py --self-test
```

- Each metric needs at least 10 samples; its median is compared with the baseline, the median of
  the medians of the last 5 runs in the history that have the metric in the same unit.
- A gated metric more than 10% worse fails the run (exit status 1). Until a metric has 5 baselines,
  or for an ungated metric, the comparison is only reported. Unusable input exits with status 2.
- The table goes to standard output and, with `--summary`, to the job summary.
- With `--update-history`, a run without a regression is added to the history, which keeps the last
  5 runs. The nightly job keeps the history of each system in the Actions cache
  (`bench-history-<system>-<run>`), and only a scheduled run (always on the default branch) in which
  every benchmark ran and none regressed is kept. A manual run compares without changing it.
- After a deliberate change in performance, delete that system's `bench-history-*` cache entries
  (_Actions → Caches_); the gate is then informational again until 5 new runs are kept.

Results files (`blocks2cpp/bench-results`, one per producer) and the history
(`blocks2cpp/bench-history`) are JSON with a format tag, checked strictly when read (sizes, names,
finite non-negative samples, no unknown or duplicate keys); the script's docstring describes both.
