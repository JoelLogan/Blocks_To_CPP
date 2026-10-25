# ADR-0016: The live preview runs in a Web Worker

* Status: Proposed (owner decision D15 of 2026-10-10 moved the preview to a
  Web Worker in M3; the points marked *owner to confirm* are still open)
* Date: 2026-10-24; revised 2026-10-25 after the W0 review
* Amends [ADR-0010](0010-wasm-delivery-under-the-csp.md): the compiler core
  runs as two instances, one of them in a dedicated worker whose script has a
  policy of its own. The page's CSP is unchanged.
* Contracts: [the preview worker](../plans/m3-contracts/worker.md) holds the
  callers, messages, signatures, index format, files, tests, benchmarks and
  specification changes for W5. Where it and this ADR differ, this ADR wins.

## Context

M3's exit is a set of intro-CS assignments (M3-D1 in
[next-steps §3.3](../plans/next-steps.md#33-decisions-before-m3-starts)).
Their blocks and multi-module projects make each `update()` do more work.
Two goals bound it ([01 §1.4](../spec/01-overview.md#non-functional-goals)):
**N4**, the core's `update()` of a 1,000-block module under 50 ms (p95) in
the webview; and **N12**, interactions within 100 ms at 1,000 blocks and no
main-thread task over 50 ms while a program of up to 200 blocks is edited
(M3-9 gates these small-program budgets). Owner decision D15
([M2 polish plan](../plans/m2-polish-plan.md#d15-when-should-the-performance-architecture-and-library-upgrades-happen-and-when-do-budgets-block-merges))
moved the preview to a Web Worker in M3, after P-1's single `update()`, with
the worker's CSP verified on both webviews first (roadmap Q11). The
specification still has a worker only for modules over 2,000 blocks, in M5
(`docs/spec/06-compiler-pipeline.md:590`, `:602-605`).

**Today** one WebAssembly instance runs on the main thread
(`packages/b2c-core-wasm/src/loader.ts:32-60`), started when the editor
mounts (`apps/desktop/src/editor/EditorWorkspace.tsx:177-180`). It keeps the
analysis of its last `update()` (`crates/b2c-core-wasm/src/session.rs:104-123`),
from which the scope query and the clipboard answer. Every `CoreWasm` method
is synchronous (`packages/b2c-core-wasm/src/api.ts:187-193`); only the
preview is behind an asynchronous interface
(`apps/desktop/src/editor/preview/service.ts:1-5`). Per edit at 1,000 blocks
the preview's task takes 56–58 ms: `update()` 42.5–44 ms, reading the canvas
8 ms (`docs/plans/m2-polish-plan.md:1258`). Only `update()` is on the
per-edit path. Menus, the landing of a toolbox block and *Make a variable*
need a scope answer in the same task; a copy must fill the DOM `copy` event
before it returns; paste, duplicate and the name checks are synchronous;
save, build and autosave call `canonical` once per action (contracts §1).

The read, `update()` and the commit happen in one task today
(`preview/pipeline.ts:198-247` awaits only microtasks). So nothing writes the
project's document between a run's read and its commit, although a rename
(`editor/sync/session.ts:328-339`) and an adopted save
(`features/project/lifecycle.ts:840-880`) write it without `invalidate()`.
That stops holding once `update()` runs elsewhere.

**Constraints found in the code:**

* The page's CSP (`apps/desktop/src-tauri/tauri.conf.json:28`,
  [08 §8.8](../spec/08-security.md#88-webview-and-ipc-hardening)) allows
  `worker-src 'self'` and `script-src 'self' 'wasm-unsafe-eval'`.
* Tauri 2.12.4 adds the policy only to `.html` assets (`tauri` crate,
  `src/manager/mod.rs:422-439`, `src/protocol/tauri.rs:182-184`) and calls
  `on_web_resource_request` for every response (`:188-190`). Under CSP
  Level 3 a dedicated worker takes its own script's policy, so it would run
  with none. What WebKitGTK and WebView2 do is not verified.
* The frozen prototype (`tauri.conf.json:30`) covers the page's realm only.
  The report-only Trusted Types header (`apps/desktop/src-tauri/src/window.rs:268-284`)
  reports `new Worker()`'s URL as a sink; the app has no policy yet. A
  worker has no `Tauri-Invoke-Key` (`tauri` crate, `src/ipc/protocol.rs:479-482`)
  and no isolation frame (`tauri.conf.json:31-36`).
* A copy uses the analysis only to cut the copied blocks' C++ from
  regenerated files and source map (`session.rs:186-197`, `src/cut.rs:29-33`);
  a paste, only for the symbols visible at its target (`session.rs:277-283`).
  The page holds the preview's files and source map (`api.ts:67-90`).
* The scope query's index has one trail node per declaration and per hidden
  name, and one position per block and per input that has its own: linear in
  blocks plus declarations (`crates/b2c-lang/src/query.rs:1-10`, `:71-85`;
  `src/scope.rs:118-137`).

## Options considered

**Where the work runs:**

1. **The main thread, made cheaper.** The per-block cache of
   [06 §6.13](../spec/06-compiler-pipeline.md#613-performance-in-the-editor)
   is XL and planned for M5, and the main thread still pays per edit. It can
   come later, on top of the worker.
2. **Everything in the worker, every call asynchronous.** Menus, the landing
   and the name checks need an answer in the same task, and a copy cannot
   wait. Rejected.
3. **`update()` in a worker, a scope index for the synchronous queries, a
   main-thread instance for the rest (chosen).** Costs two instances, an
   index format, and another source for copy and paste.
4. **A second main-thread instance running `update()`,** or **synchronous
   calls into the worker** (`Atomics.wait` is not allowed on a page's main
   thread, and shared memory needs cross-origin isolation). Rejected.

**The synchronous scope query:** each block's answer in full grows with
blocks times symbols (3 million entries at 5,000 blocks and 600 variables),
rejected; **the analyser's own index**, linear, with a lookup that mirrors
`query.rs` (chosen).

**Loading the worker:** a `blob:` worker needs `worker-src blob:`, which lets
any script start a worker from text it made, rejected; **a packaged script
from `'self'`** (chosen).

**The worker's WebAssembly:** importing the embedded-bytes chunk in the
worker adds a second 1.43 MB chunk to its files (Vite builds the worker
separately, and inlines it into a classic worker), decoded and compiled
again, rejected; **the page posts its compiled `WebAssembly.Module`**
(chosen). A webview that refuses the clone previews on the main thread.

**What copy and paste use instead of the kept analysis:**

1. **An analysis on the main instance per action.** Simple, but about one
   preview's analysis per action at large sizes, estimated at 40 ms at 1,000
   blocks and not measured; duplicate pays it on top of `canonical()`.
2. **What the page holds, when it matches (chosen).** When the document is
   the one the applied preview analysed, copy cuts its text from the
   preview's files and source map, and paste binds against the scope index's
   answer at the target. Otherwise option 1.
3. **Paste through the worker.** Paste turns asynchronous; copy cannot.
   Rejected.

## Decision

### 1. What runs where

* **The preview worker,** one dedicated module worker per editor window, runs
  `update()` and `scope_index()` only and keeps the analysis. It starts right
  after the main instance, off the path to the readiness marker.
* **Every module** previews in the worker, whatever its size
  (*owner to confirm*).
* **The main instance** keeps `load`, `canonical`, `clipboardMake`,
  `pastePrepare`, `conversionTable` and `version`, synchronous. `canonical`
  stays (since P-1 it runs per save, build and autosave, and four callers
  need it synchronously), unlike the plan's sketch (*owner to confirm*).
  Until a worker has answered `started`, the main instance also runs the
  preview, so a project's first preview never waits and cold start is
  unchanged.
* **Scope queries** are answered on the main thread from the **scope index**
  each preview carries (§4). `symbols_in_scope` stays as the tests' oracle.
  The connection checker keeps reading `preview.blockTypes`.
* **Clipboard.** `clipboard_make` gets the applied preview's content hash,
  indent width, files and source map; `paste_prepare` gets the index's
  answer at the target. Each uses them only when the document's content hash
  (for a copy, also the indent width) matches; otherwise it analyses and
  keeps that one analysis, so a duplicate analyses once. `clipboard_make`
  takes the preview options, so `text/plain` has the machine's indent width.
  The spike of §2 measures copy, paste and duplicate at 1,000 blocks on both
  webviews, with and without the reuse, and the result is written here
  before the owner decides (*owner to confirm*).
* **Fallback.** When the worker fails (§3) or a webview refuses to clone a
  `Module`, the preview runs on the main instance as in M2 for the rest of
  the session and calls `scope_index()` after each `update()`, so the editor
  reads the same data. It is logged once, with no banner (*owner to confirm*).

### 2. Loading the worker under the CSP

* **The script** is imported with Vite's `?worker&url`, built as an ES module
  worker under `workers/` and created with
  `new Worker(url, { type: 'module', name: 'b2c-preview' })`. No `blob:` or
  `data:` worker; the page's CSP does not change.
* **The WebAssembly.** The page compiles the embedded module once and posts
  the `Module` in the `start` message; the worker instantiates it with its
  own glue. The worker never imports `#pkg-bytes`, not even as a fallback,
  so nothing it loads holds the embedded bytes (a build test).
* **The worker's policy.** The backend adds
  `Content-Security-Policy: default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'none'`
  (`WORKER_CSP`) to every app response under `/workers/` with a JavaScript
  type, in the handler that adds the Trusted Types header
  (`window.rs:77-79`), keeping an existing CSP header. The worker can make no
  request at all; `worker-src 'none'` is explicit because it would otherwise
  fall back to `script-src`. A webview that gives the worker the page's
  policy still runs it under a strict one.
* **Verify first.** W5 starts with a spike on both webviews: (a) a module
  worker starts from `tauri://localhost` and `http://tauri.localhost`; (b) a
  `Module` can be posted to it; (c) which policy it runs under (the probe of
  §6); (d) what the Trusted Types trial reports for `new Worker` with and
  without §6's policy; and §1's clipboard measurements. The results are
  written here before acceptance. If (a) fails, the worker is a classic
  worker, still without the bytes. If (b) fails on a webview, that webview
  previews on the main thread; if the owner would rather the worker compiled
  the bytes there, this ADR is amended first, and the build test then allows
  exactly one lazily imported bytes chunk for the ES worker.

### 3. The message protocol

The messages and APIs are in contracts §3. The rules:

* Every message carries protocol version 1; another version, an unknown kind
  or a mistyped field is a protocol error. Both sides ship in one build, and
  the worker's core version must equal the main instance's.
* One request at a time, the newest waiting: a newer `update` replaces the
  waiting one, which resolves as superseded. No cancel message. A reply for
  another request than the one in flight is a protocol error.
* The document is cut as `core.ts` does, so `B2C-E0101` still comes from the
  loader; only the indent width crosses in the options. Structured clone, no
  transfer: the core's JSON crosses unparsed and is parsed once on the main
  thread with `core.ts`'s checks. The answer also carries when `update()`
  started and how long `update()` and `scope_index()` each took (§7).
* Timeouts: 20 s to start; a 30 s watchdog on each `update`.
* **Restarts have one owner.** On a trap the worker replies `trapped` and
  makes a fresh instance from its `Module`, as `resetCore()` does; the client
  rejects that request with `CoreTrap`, posts the waiting one and restarts
  nothing. The client replaces the worker only on `startFailed` or the start
  timeout, an `error` or `messageerror` event, a protocol error or the
  watchdog, rejecting the request in flight with `CoreTrap` and posting the
  waiting one to the new worker. The pipeline never restarts the worker
  (§5).
* **Failure.** A failed start is a worker replaced before its first answer
  to an `update`, for any reason but the watchdog. Three in a row make the
  client fail, and §1's fallback applies; an answer resets the count. A trap
  never counts. The watchdog does not count, because the same document would
  hang the main thread; it is logged.
* Messages and logs name no project text.

### 4. The scope index

* `b2c_lang::Analysis::scope_index_parts()` is a read-only view of the
  analyser's private index: the trail, each recorded block's positions, and
  each module's functions and parked point. `b2c-core-wasm` exports it for
  the kept analysis as `scope_index()`, `null` before a document has loaded.
* **Format, version 1:** each name once; the trail as (parent, name, symbol),
  every parent smaller than its node; per module its functions and parked
  point; per recorded block its positions; the content hash. Symbols are
  indexes into the update's `symbols`. Blocks are a list, not an object
  keyed by ID, so no ID can name a prototype property. The type and a worked
  example are in contracts §4.
* **Lookup.** A TypeScript `ScopeIndex` mirrors `scope_at`, `scope_after` and
  `parked_scope` of `query.rs`, and the core's `visible_at` for a paste.
  `parse` checks the version, the content hash against the preview's, the
  parent order and every index, and throws `CoreError` (`protocol`); a
  lookup never throws.
* `AnalysisState` gains `scope`, set with `preview` in one `setAnalysis`; a
  block the index lacks resolves from its place (`toolbox/scope.ts:239-272`).
  M3-4 and L18 add trail nodes and positions in the same model; a change to
  how the trail is walked raises the version.

### 5. The preview pipeline: order, drags and consistency

* **An answer is applied only onto the document it was read against.** A run
  notes the project's document when it reads the canvas. Its answer, and the
  commit of an update that failed (`pipeline.ts:273-302`), applies only when
  the same project is open, no `invalidate()` came since the run started, its
  sequence number is above the last applied one, and the project's document
  is still the noted one or the one the pipeline's own last commit wrote.
  Otherwise it is dropped, and if the run was the latest the pipeline reads
  again. So a run read before a rename or a save never commits after it.
  This replaces "abandon unless latest" at the answer
  (`pipeline.ts:187-195`), so the C++ follows the edits while a newer run
  waits; an older answer after a newer one is dropped.
* **No answer is applied during a pointer drag, in every mode.** The hold is
  in the pipeline, so it applies in the worker, on the main thread before the
  worker has started, and in the fallback. An answer arriving during a drag
  is held, the newest only, and applied by the rule above when the drag ends
  (`sync/session.ts:559`). The canvas is not read during a drag
  (`session.ts:687-700`). ADR-0018's slot cache relies on this.
* **A trap.** After `CoreTrap` the run is retried once, only while it is
  still the latest, once the service is ready: on the main thread after
  `host.restart()`, as in M2; with the worker by waiting, never by a
  restart. The commit of an update that failed keeps using the main instance
  and restarting it on a trap there (`pipeline.ts:349-359`).
* **Guarantees.** The dirty flag never goes back to an older document; an
  answer never undoes a rename or a saved layout; autosave never writes a
  document older than a snapshot it wrote. Pipeline tests pin each.
* `isIdle()` (`pipeline.ts:135-137`) also requires that nothing is held.
  Answers never write the canvas; save and build read it themselves
  (`session.ts:291-312`) and never wait. Whatever acts on an answer checks
  the canvas first: deleted blocks get no badge
  (`editor/diagnostics/badges.ts:113-121`), and a quick fix (M3) applies
  only to a block still as it expects.

### 6. Trusted Types, the frozen prototype and the probe

* The page creates the worker through a Trusted Types policy
  `b2c-core-worker` that accepts exactly the worker's URL, created only when
  `globalThis.trustedTypes` exists; an enforced CSP must list it. The worker
  uses no sink (module workers have no `importScripts`; 08 §8.8's ESLint bans
  apply), so its response gets no report-only header.
* The worker's entry first imports a module that runs
  `Object.freeze(Object.prototype)`, so the glue and host run frozen.
* End-to-end builds (ADR-0009's seam) give the worker a probe that tries
  `eval` and a request to the IPC endpoint, which the page's policy allows
  and the worker's does not; so it shows which policy runs.

### 7. Measurement

* `b2c:preview` spans a run from its start until its answer is applied (its
  latency, any hold included), with phases for the read, `update()`,
  `scope_index()`, the receipt, the commit and the publish.
* **N4** moves to a new `webview.preview-1000.update-p95`: `update()` alone,
  timed where it runs, gated for regressions with N4's < 50 ms target.
* `webview.preview-1000.p95` becomes the latency span, gated for regressions
  only, without a target; its history restarts.
* A new `webview.preview-1000.main-p95`, the main thread's work per edit, is
  reported; M3-9 gates the small-program budgets on the guessing game's
  metrics, not a 1,000-block one (*owner to confirm*). Copy, paste and
  duplicate at 1,000 blocks get reported metrics (N12). See contracts §9.

### 8. Formats and versions

No change to the project format, the clipboard format or the IPC contract,
and no diagnostic code: a failing worker shows the `trapRecovered` notice.
The core's exports change (`scope_index`; new arguments of `clipboard_make`
and `paste_prepare`) in one build with their callers, which the build guard
enforces (`apps/desktop/vite/wasmCore.ts`). The worker protocol (1) and the
index format (1) never leave one running app. M3-5's compact source map
(also W5) crosses inside the answer unchanged, and the copy's cut reads its
form. ADR-0016 to ADR-0019 share one version plan and code table (contracts
§10).

## Consequences

No Blockly seam changes (ADR-0014) and no dependency. What gets harder:

* Two instances on two threads: a bug can sit in the hand-off. The apply
  rule, the shim tests and the E2E flow cover it.
* Two policies, both in 08 §8.8 and checked by the consistency test, and the
  app's first Trusted Types policy, to list once Trusted Types are enforced.
* Each instance starts with 1,245,184 bytes of linear memory and grows to the
  largest document it handled; an E2E hook reports both.
* Copy and paste have two ways to one answer, held equal by tests; without a
  matching preview they cost an analysis on the main thread.

**Performance (expected; measured in the spike and the W5 A/B run).** Per
edit at 1,000 blocks the main thread does the read (8 ms), the receipt (2–5
ms, estimated) and the store updates: about 15 ms instead of 56–58 ms, with
the latency within a few milliseconds of today's. `webview.edit-1000.p95-all`
should fall about 3 %, within noise (Blockly's drawing dominates; the render
patch, M3-2, is the larger lever), and the guessing game's edit task from
about 36 to about 30 ms. Selection, dragging and cold start should not
change.

**Security ([08](../spec/08-security.md)).** The code that loads untrusted
project JSON on every edit runs with no DOM, no IPC or isolation key, no
network and no `eval`; the limits of
[05 §5.6](../spec/05-project-format.md#56-validation-limits) apply
unchanged. The page's CSP is not relaxed. Replies and the index are checked
before use, and a worker that misbehaves is restarted, never trusted. The
worker's files hold no `@tauri-apps` code and no embedded bytes. The M3
threat-model review records the spike and the probe per webview
([open items](../security/threat-model-reviews.md#open-items-for-the-m3-review)).
No new IPC command, file write, `unsafe` code or dependency.

**Specification.** W5 changes 01 §1.4 (N4's measurement), 02 §2.2, §2.3,
§2.4.1 and §2.6, the last sentence of 06 §6.5 (the rest is W1's), 06 §6.13
(owned by W5 with this ADR), 08 §8.8 and §8.13, and 09 §9.2's benchmark and
span bullets; the developer guide and the benchmarks' README follow. Each
change and its owner wave is listed in contracts §11.

## Tests

The list is contracts §8: the index against `scope_at`, `scope_after`,
`parked_scope` and `visible_at` on the examples, assignments and security
corpus; the clipboard with and without the reuse; client and host over
Node's `MessageChannel` (a trap with a request waiting, the watchdog, failed
starts); the pipeline with a rename or a save while an update is
outstanding, the drag hold in both modes and a `canonical` trap after a
worker trap; `WORKER_CSP`; the worker's files; and E2E flows and the probe on
both webviews.

## Owner to confirm

1. Every module previews in the worker ([§1](#1-what-runs-where)).
2. Copy and paste reuse the applied preview and analyse on the main instance
   otherwise, decided after the spike's measurement ([§1](#1-what-runs-where)).
3. `canonical` stays on the main thread ([§1](#1-what-runs-where)).
4. A worker that fails for good falls back to the main thread with a log
   entry and no banner ([§1](#1-what-runs-where)).
5. N4 moves to `update-p95`; the latency metric is gated for regressions
   only; the 1,000-block main-thread metric is reported
   ([§7](#7-measurement)).
