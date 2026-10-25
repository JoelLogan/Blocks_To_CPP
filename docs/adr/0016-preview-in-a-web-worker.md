# ADR-0016: The live preview runs in a Web Worker

* Status: Proposed (owner decision D15 of 2026-10-10 moved the preview to a
  Web Worker in M3; the points marked *owner to confirm* are still open)
* Date: 2026-10-24
* Amends [ADR-0010](0010-wasm-delivery-under-the-csp.md): the compiler core
  runs as two instances, one of them in a dedicated worker, and the worker's
  script is served with a policy of its own. The page's CSP is unchanged.

## Context

### What M3 needs

M3's exit is a set of intro-CS assignments, each an example project with a
golden test (M3-D1 in [next-steps §3.3](../plans/next-steps.md#33-decisions-before-m3-starts),
[10 §10.1](../spec/10-roadmap.md#m3-language-breadth-i)). The blocks they need
(collections, strings, structs, files, errors) and multi-module projects with
headers (L18) make each `update()` do more work per edit: more statements to
lower, more modules to analyse, larger generated files. Two goals bound that
work ([01 §1.4](../spec/01-overview.md#non-functional-goals)):

* **N4:** the core's `update()` of a 1,000-block module under 50 ms (p95),
  measured in the webview.
* **N12:** interactions within 100 ms at 1,000 blocks, and no main-thread task
  over 50 ms while a program of up to 200 blocks is edited. M3 gates these
  small-program budgets once measured (M3-9).

Owner decision D15 ([M2 polish plan](../plans/m2-polish-plan.md#d15-when-should-the-performance-architecture-and-library-upgrades-happen-and-when-do-budgets-block-merges))
moved the preview to a Web Worker in M3, after the single `update()` export
(P-1), with the worker's CSP verified on both webviews first. The roadmap
records it as Q11. The plan sketched this ADR
(`docs/plans/m2-polish-plan.md:1556-1565`). The specification still says
that only modules over 2,000 blocks use a worker, in M5
(`docs/spec/06-compiler-pipeline.md:590`, `:602-605`;
`docs/spec/02-architecture.md:491-495`).

### What the code does today

One WebAssembly instance runs on the main thread. The editor compiles the
embedded module and instantiates it with fresh glue state
(`packages/b2c-core-wasm/src/loader.ts:32-60`), starting it when the editor
mounts (`apps/desktop/src/editor/EditorWorkspace.tsx:177-180`). The instance
keeps the analysis of its last `update()` in a thread-local `Session`
(`crates/b2c-core-wasm/src/lib.rs:91-95`, `src/session.rs:104-123`), and the
scope query and the clipboard answer from it. Every method of `CoreWasm` is
synchronous (`packages/b2c-core-wasm/src/api.ts:187-193`). Only the preview
goes through an asynchronous interface, written so that a worker can take
over (`apps/desktop/src/editor/preview/service.ts:1-5`, `:21-28`).

Per edit at 1,000 blocks the preview's task takes 56–58 ms on the main
thread: `update()` 42.5–44 ms, reading the canvas 8 ms, the rest storing the
result (P-1's status, `docs/plans/m2-polish-plan.md:1258`). In the guessing
game (40 blocks) it takes 6–8 ms.

Every caller of the core, and whether it needs the answer in the same task:

| Caller | Core call | Answer needed at once? |
| --- | --- | --- |
| Preview pipeline, after each debounced edit (`apps/desktop/src/editor/preview/pipeline.ts:215`) | `update` | No: awaited behind `PreviewService` |
| Field menus and call labels (`packages/blockly-ext/src/fields/symbol-ref.ts:148-155`, `src/mutators/variadic.ts:322-326`), through `analysisScope` (`apps/desktop/src/editor/services/symbols.ts:72-89`) | `symbolsInScope` | Yes: Blockly builds a menu synchronously |
| A block from the toolbox settled where it lands, in the drop's event group (`apps/desktop/src/editor/toolbox/landing.ts:151`); *Make a variable*'s taken names (`toolbox/scope.ts:489-503`) | `symbolsInScope` | Yes: inside Blockly's drag end, one undo step |
| Connection checker (`services/symbols.ts:167-189`, `packages/blockly-ext/src/checker/checker.ts:93`) | none: `preview.blockTypes` and the TypeScript conversion table | Yes, but it reads the store only |
| Copy, cut, duplicate (`apps/desktop/src/editor/clipboard/controller.ts:174`) | `clipboardMake` | Yes: a copy fills the DOM `copy` event in the same task (`controller.ts:25-26`, `clipboard/bridge.ts:1-21`) |
| Paste, duplicate (`controller.ts:271-278`, `:350`) | `pastePrepare`, `canonical` | Yes: the actions run synchronously |
| Opening a project (`apps/desktop/src/editor/load.ts:84-96`) | `load`, `canonical` | No, and rare |
| Save, build, autosave every 30 s (`features/project/lifecycle.ts:782-784`, `:867`; `features/build-run/document.ts:60`, `:87`; `features/recovery/autosave.ts:31`, `:207`) | `canonical` | Called synchronously, once per action |
| The project name check (`app/layout/ProjectTitle.tsx:103`) and the module-switch check (`editor/sync/session.ts:541-551`) | `canonical` | Yes: synchronous validation |
| An update that failed twice (`preview/pipeline.ts:283`) | `canonical` | No |

So only `update()` is on the per-edit path, and the scope query and the
clipboard need synchronous answers that today come from `update()`'s kept
analysis.

### Constraints found in the code

* **The page's CSP** (`apps/desktop/src-tauri/tauri.conf.json:28`,
  [08 §8.8](../spec/08-security.md#88-webview-and-ipc-hardening)) allows
  `worker-src 'self'` and `script-src 'self' 'wasm-unsafe-eval'`, and limits
  `connect-src` to the IPC endpoints. ADR-0010 never fetches the module and
  never uses a `data:` URL.
* **A worker gets no CSP from Tauri.** Tauri 2.12.4 sends the configured
  policy only with `.html` assets (`tauri` crate, `src/manager/mod.rs:422-439`
  and `src/protocol/tauri.rs:182-191`). Under CSP Level 3 a dedicated worker
  loaded from a URL takes the policy of its own script's response, not the
  page's. The plan's hypothesis that the worker's response carries the CSP
  therefore does not hold: on an engine that follows CSP Level 3, the worker
  would run with no policy at all. Which rule WebKitGTK and WebView2 follow
  is not verified.
* **The frozen prototype** (`tauri.conf.json:30`) is Tauri's
  `Object.freeze(Object.prototype)` (`tauri` crate,
  `scripts/freeze_prototype.js`), run in the page's realm only. A worker has a
  realm of its own.
* **Trusted Types.** The backend adds the report-only policy
  `require-trusted-types-for 'script'` to the app's HTML
  (`apps/desktop/src-tauri/src/window.rs:268-284`). The script URL given to
  `new Worker()` is a Trusted Types sink, so a plain string is reported on
  WebView2. The app has no Trusted Types policy yet.
* **IPC.** Every IPC request carries the `Tauri-Invoke-Key` header (`tauri`
  crate, `src/ipc/protocol.rs:479-482`), which only the page's init script
  holds, and the isolation pattern (`tauri.conf.json:31-36`) encrypts each
  message in the isolation frame. A worker has neither.
* **The clipboard's indent width.** The C++ that a copy puts on `text/plain`
  uses the indent width of the last `update()` (`session.rs:50-55`,
  `:104-105`), which is 4 before any update (`src/options.rs:110`).
* **The scope query's index** stores one trail node per declaration and one
  position per block, so it grows with blocks plus declarations, never with
  their product (`crates/b2c-lang/src/query.rs:1-10`, `:71-85`;
  `src/scope.rs:28-41`). A query walks the trail from the block's node, then
  adds the module's functions (`query.rs:206-242`, `:295-298`).

## Options considered

**Where the work runs:**

1. **Keep the core on the main thread and make it cheaper.** The per-block
   cache of [06 §6.13](../spec/06-compiler-pipeline.md#613-performance-in-the-editor)
   (re-lower only the changed top-level block) is XL and planned for M5. The
   main thread still pays for every edit, and the cost grows with M3's
   blocks and modules. It can come later on top of the worker.
2. **Everything in the worker, every call asynchronous.** Blockly's menus, the
   landing in the drop's event group and the name check need an answer in the
   same task, and a copy must fill the DOM's `copy` event before it returns.
   Answering them from a cache means the scope index of option 3, and the
   clipboard cannot wait at all. Rejected.
3. **`update()` in a worker, a scope index for the synchronous queries, a
   main-thread instance for the rest (chosen).** The per-edit work leaves the
   main thread; the rare calls stay synchronous. Costs: two instances, an
   index format, and copy and paste analyse on demand.
4. **A second main-thread instance that also runs `update()`** to answer
   scope queries. The main thread pays for the analysis again. Rejected.
5. **Synchronous calls into the worker** with `SharedArrayBuffer` and
   `Atomics.wait`. `Atomics.wait` is not allowed on a page's main thread, and
   shared memory needs cross-origin isolation headers on every response.
   Rejected.

**How the synchronous scope query is answered (with option 3):**

1. **Each block's answer listed in full.** Grows with blocks times visible
   symbols: up to 3 million entries at 5,000 blocks with 600 variables.
   Rejected.
2. **The analyser's own index, sent as it is (chosen).** Linear, and the
   lookup mirrors `query.rs`, so the answers are the same by construction and
   by test.

**How the worker is loaded:**

1. **A `blob:` worker** (Vite's `?worker&inline`). Needs `worker-src blob:`,
   which would let any script in the page start a worker from text it made.
   Rejected.
2. **A packaged script from `'self'` (chosen),** which `worker-src 'self'`
   already allows.

**How the worker gets the WebAssembly:**

1. **The worker imports the embedded-bytes chunk itself.** A second 1.43 MB
   chunk in the bundle, decoded and compiled again.
2. **The page posts its compiled `WebAssembly.Module` (chosen).** Compiled
   once; a `Module` can be cloned to a worker of the same page. Kept as the
   fallback if a webview refuses to clone it.

## Decision

### 1. What runs where

* **The preview worker.** One dedicated module worker per editor window
  runs `update()` and `scope_index()` and nothing else. Its instance keeps
  the analysis, as the M2 instance did. It starts right after the main
  instance (`EditorWorkspace.tsx:177-180`), not on the path to the readiness
  marker.
* **Every module** previews in the worker, whatever its size
  (*owner to confirm*).
* **The main instance** (`initCore()`) stays for `load`, `canonical`,
  `clipboardMake`, `pastePrepare`, `conversionTable` and `version`, all
  synchronous as today. It never runs `update()` while the worker works.
  `canonical` stays here: since P-1 it runs per save, build and autosave,
  not per edit, and four of its callers need it synchronously. The plan's
  sketch moved it to the worker (*owner to confirm*).
* **Scope queries** are answered on the main thread from the **scope index**
  that comes with each preview (§4): a TypeScript lookup, no WebAssembly call.
  The `symbols_in_scope` export stays, as the tests' oracle; the editor stops
  calling it.
* **The connection checker** keeps reading `preview.blockTypes` from the
  store.
* **Clipboard.** The main instance has no kept preview, so `clipboard_make`
  and `paste_prepare` analyse the document they are given
  (`session.rs:151-162`): about one preview's work per copy or paste,
  40 ms at 1,000 blocks (*owner to confirm*). Two changes come with it:
  * `Session` keeps the analysis it made, one entry keyed by content hash and
    indent width, so a duplicate, or several pastes into an unchanged
    document, analyse once;
  * `clipboard_make` takes the preview options as a third argument, so the
    C++ on `text/plain` has the machine's indent width.
* **Fallback.** When the worker cannot start, or 3 worker starts in a row
  produce no answer, the preview runs on the main instance as in M2 for the
  rest of the session, and calls `scope_index()` after each `update()`, so
  the rest of the editor reads the same data either way. This is logged once
  as a warning, with no banner (*owner to confirm*).

### 2. Loading the worker under the CSP

* **The script.** The entry `apps/desktop/src/editor/preview/worker/core.worker.ts`
  is imported with Vite's `?worker&url`, built as an ES module worker
  (`worker.format: 'es'`) and emitted under `workers/`. The page creates it
  with `new Worker(url, { type: 'module', name: 'b2c-preview' })`, which
  `worker-src 'self'` allows. No `blob:` or `data:` worker, and the page's
  CSP does not change.
* **The WebAssembly.** The page compiles the embedded module once, as now;
  `loader.ts` exports that compile as `coreModule(): Promise<WebAssembly.Module>`.
  The page posts the `Module` in the `start` message, and the worker
  instantiates it with its own glue (`createGlue()`). The worker's bundle
  never contains `#pkg-bytes`: `instantiate()` moves out of `loader.ts` into
  `src/instance.ts`, which does not reference it. If a webview refuses to
  clone a `Module`, the worker imports `#pkg-bytes` and compiles it itself;
  the worker's policy below already allows that.
* **The worker's own policy.** The backend adds
  `Content-Security-Policy: default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'none'`
  to every app response under `/workers/` with a JavaScript content type, in
  the `on_web_resource_request` handler that already adds the Trusted Types
  header (`window.rs:77-79`), as a constant `WORKER_CSP` beside
  `TRUSTED_TYPES_REPORT_ONLY` (`window.rs:40`). An existing CSP header is
  kept. With `default-src 'none'`, `connect-src` is `'none'`: the worker can
  make no request at all, not even to the IPC endpoints the page may reach.
  `worker-src 'none'` is explicit because it would otherwise fall back to
  `script-src`. If a webview gives the worker the page's policy instead, it
  still runs under a strict one.
* **Verify first.** Before the W5 packages, a spike on both webviews checks:
  (a) a module worker starts from `tauri://localhost` (WebKitGTK) and
  `http://tauri.localhost` (WebView2); (b) a `Module` can be posted to it;
  (c) which policy the worker runs under (the probe of §6); (d) what the
  Trusted Types trial reports for `new Worker` with and without the policy
  of §6. Its results are written into this ADR before it is accepted. If (a)
  fails, the worker is built as a classic worker (`worker.format: 'iife'`);
  if (b) fails, the fallback above applies.

### 3. The message protocol

`packages/b2c-core-wasm/src/worker/protocol.ts`:

```ts
export const WORKER_PROTOCOL = 1;

/** Page → worker. */
export type WorkerRequest =
  | { p: 1; kind: 'start'; module: WebAssembly.Module }
  | { p: 1; kind: 'update'; id: number; documentJson: string; optionsJson: string }
  | { p: 1; kind: 'probe'; id: number }; // end-to-end builds only

/** Worker → page. */
export type WorkerReply =
  | { p: 1; kind: 'started'; versionJson: string }
  | { p: 1; kind: 'startFailed'; message: string }
  | {
      p: 1;
      kind: 'updated';
      id: number;
      /** What `update()` returned, unparsed: a result or an error envelope. */
      updateJson: string;
      /** What `scope_index()` returned, or null when the document did not load. */
      scopeJson: string | null;
      /** When `update()` started: `performance.timeOrigin + performance.now()` in the worker. */
      startedAt: number;
      /** How long `update()` and `scope_index()` took, in milliseconds. */
      ms: number;
    }
  | { p: 1; kind: 'trapped'; id: number; message: string }
  | { p: 1; kind: 'probed'; id: number; probe: WorkerProbe };
```

* **Versions.** Every message carries `p: 1`. A message with another `p`, an
  unknown `kind` or a field of the wrong type is a protocol error: the worker
  logs and ignores it; the client restarts the worker. Both sides ship in one
  build, so a mismatch is a broken build, not a combination to support. The
  `started` reply's version must equal the main instance's `version()`.
* **Request IDs** are positive integers that increase per client. A reply
  must carry the ID of the request in flight; any other reply is a protocol
  error.
* **One request at a time, the newest waiting.** The client posts an
  `update` only while none runs. A newer `update` replaces the waiting one,
  whose promise resolves to `null` (superseded). There is no cancel message:
  the worker never holds more than one request, and a running `update()`
  cannot be interrupted; only the watchdog stops it.
* **Sizes.** The document text is cut as `core.ts` does before it crosses
  (one code unit over 32 MiB, so the loader still reports `B2C-E0101`;
  `packages/b2c-core-wasm/src/core.ts:114-116`), and only `indentWidth`
  crosses in the options (`core.ts:157`).
* **Structured clone, no transfer.** Strings are copied. The core's JSON is
  posted unparsed and parsed once on the main thread with `core.ts`'s checks
  (`core.ts:53-108`), so error envelopes and protocol errors behave as in
  M2. Transferring would mean encoding to bytes and decoding again on the
  main thread, which costs what it saves.
* **Timeouts.** 20 s for `start`, 30 s for an `update` (a watchdog: the
  largest document the loader accepts is expected to preview in a few
  seconds). Past either, the client terminates the worker and starts a new
  one, and the request fails like a trap.
* **Errors.**
  * A trap in the worker (the glue throws): the worker replies `trapped`,
    drops its instance and instantiates a fresh one from the `Module` it
    holds, as `resetCore()` does. The client rejects with `CoreTrap`; the
    pipeline retries once and shows `trapRecovered`
    (`pipeline.ts:214-224`), as in M2.
  * The worker's `error` or `messageerror` event, a protocol error or the
    watchdog: the client terminates the worker, starts a new one and rejects
    the request with `CoreTrap`.
  * 3 worker starts in a row without an answer: the client's state becomes
    `failed`, and the app falls back to the main thread (§1). An answer
    resets the count.
  * An error envelope (invalid options): `CoreError`, as today.
  * Messages and logs name no project text.

The client (`src/worker/client.ts`) and the host (`src/worker/host.ts`):

```ts
export interface CoreWorkerClient {
  readonly state: 'starting' | 'ready' | 'failed';
  /** @throws CoreError (`init`) when the worker cannot be started. */
  ready(): Promise<CoreVersion>;
  /** Resolves `null` when a newer request replaced this one before it was sent. */
  update(documentJson: string, options: PreviewOptions): Promise<WorkerUpdate | null>;
  /** Terminates the worker and starts a new one. */
  restart(): Promise<void>;
  /** End-to-end builds only. */
  probe(): Promise<WorkerProbe>;
  dispose(): void;
}

export interface WorkerUpdate {
  readonly result: UpdateResult;
  readonly scope: ScopeIndex | null;
  /** When `update()` started, on the page's clock, and how long it took. */
  readonly startedAt: number;
  readonly ms: number;
}

export function createCoreWorkerClient(options: {
  /** Creates the worker; tests pass one end of a `MessageChannel`. */
  readonly spawn: () => WorkerLike;
  readonly module: () => Promise<WebAssembly.Module>;
  readonly startTimeoutMs?: number; // 20,000
  readonly updateTimeoutMs?: number; // 30,000
  readonly maxFailedStarts?: number; // 3
}): CoreWorkerClient;

/** The worker's side: answers `start`, `update` and, when given, `probe`. */
export function serveCoreWorker(
  scope: WorkerScopeLike,
  options?: { readonly probe?: () => Promise<WorkerProbe> },
): void;
```

`WorkerLike` and `WorkerScopeLike` are the few members of `Worker` and
`DedicatedWorkerGlobalScope` that the two sides use (`postMessage`,
`onmessage`, `onerror`, `onmessageerror`, `terminate`).

### 4. The scope index

* **Rust.** `b2c_lang::Analysis::scope_index_parts(&self) -> ScopeIndexParts<'_>`
  is a read-only view of the private `ScopeIndex`: the trail (parent, C++
  name, symbol), each recorded block with its position at the block and its
  per-input positions, and each module's function list. `b2c-core-wasm`
  encodes it (`wire.rs`) for the kept analysis in
  `Session::scope_index(&self) -> Option<String>`, exported as
  `scope_index() -> String` (the JSON below, or `null` before an update has
  loaded a document). `EXPECTED_EXPORTS` and `EXPORTED_FUNCTIONS`
  (`packages/b2c-core-wasm/scripts/build.mjs:55-74`), `src/glue.d.ts` and
  `src/core.ts` change together, as for every export.
* **Format, version 1** (declared in `src/api.ts`):

  ```ts
  export interface ScopeIndexData {
    readonly v: 1;
    /** The analysed document's content hash: the update's `contentHash`. */
    readonly contentHash: string;
    /** Every name the trail and the function lists use (the C++ spellings), once each. */
    readonly names: readonly string[];
    /**
     * The trail, three numbers per node: its parent (-1 for none, always smaller than the node),
     * its name (index into `names`) and its symbol (index into the update's `symbols`; -1 for a
     * hidden name, or a symbol the program does not list).
     */
    readonly trail: readonly number[];
    /** Per module, in document order: the functions calls can use, as (name, symbol) index pairs. */
    readonly functions: readonly (readonly number[])[];
    /**
     * One entry per recorded block: its ID, the node and module at the block (module -1 when
     * nothing was recorded there), then a name, node and module for each statement list or
     * value input with a position of its own. Node -1: no local symbol.
     */
    readonly blocks: readonly (readonly (string | number)[])[];
  }
  ```

  For example, a `main` (`b001`) creating `score` (`b002`) and `total`
  (`b003`) before a print (`b004`), in a module with a function `greet`
  (the update's `symbols` being `greet`, `score`, `total`; the positions
  inside the two starting values are left out):

  ```json
  {"v":1,"contentHash":"9f…","names":["score","total","greet"],
   "trail":[-1,0,1, 0,1,2],"functions":[[2,0]],
   "blocks":[["b001",-1,0,"BODY",-1,0],["b002",-1,0],["b003",0,0],["b004",1,0]]}
  ```

  Blocks are a list, not an object keyed by ID, so no block ID can name a
  prototype property.
* **Lookup.** `packages/b2c-core-wasm/src/scopeIndex.ts` exports
  `class ScopeIndex` with `static parse(json: string, preview: PreviewResult): ScopeIndex`
  and `symbolsAt(blockId: string, input: string | null): SymbolInfo[] | null`.
  The lookup follows `query.rs:206-242`: the input's position when one is
  recorded under that name, else the block's, else `null`; walk the trail,
  each name counting once and adding its symbol if it has one; then the
  module's functions whose names were not seen; each symbol once, in the
  order of the update's `symbols` (which Rust sorted by name, then ID). The
  block IDs go into a `Map`. `parse` checks the version, the content hash
  against the preview's, that every parent is smaller than its node and
  every index is in range, and throws `CoreError` (`protocol`) otherwise;
  a lookup never throws.
* **Store.** `AnalysisState` (`apps/desktop/src/app/store/state.ts:61-73`)
  gains `scope: ScopeIndex | null`, set in the same `setAnalysis` call as
  `preview`. `analysisScope` takes `{ scope, preview }` instead of
  `{ core, preview }`; a block the index lacks still resolves from its place
  (`toolbox/scope.ts:239-272`).
* **Changes to the index.** M3-4 (lowering parked stacks) and L18 (headers)
  add trail nodes and positions in the same model, which the format carries
  as it is. A change to how `visible()` walks raises `v`, and the lookup
  changes with it.

### 5. The preview pipeline: order, drags and consistency

* **An answer is applied when no newer one has been.** A run's answer is
  applied when its sequence number is greater than that of the last applied
  answer, the same project is open, and no `invalidate()` came after the run
  started (an epoch that `invalidate()` raises). This replaces "abandon
  unless it is the latest run" at the answer (`pipeline.ts:187-195`), so
  that while a newer run is in flight the C++ still follows the edits. An
  older answer after a newer one is dropped. The checks before the canvas is
  read stay as they are.
* **Nothing from the worker reaches the canvas during a drag.** An answer
  that arrives while a block is dragged is held, the newest only, and
  applied when the drag has ended, where the session already does the module
  switch it put off (`sync/session.ts:559`). The pipeline gets
  `isBusy: () => boolean` and `release()` for this. Reading the canvas is
  already put off during a drag (`session.ts:687-700`).
* **`isIdle()`** keeps its meaning (`pipeline.ts:135-137`): no run waits and
  the latest run's answer is applied. The drag-start snapshot reads the
  canvas when it is not idle (`session.ts:515-534`), which happens more
  often now that the main thread is free while `update()` runs.
* **Undo and redo** act on the canvas; answers never write it. An undo
  during an update is read by the next run.
* **The dirty flag** follows the applied answers, as in M2; answers are
  never applied out of order, so it never goes back to an older document.
* **Autosave** writes the project's document, which only applied answers
  change, so it never writes an older document than one it wrote before.
* **Save and build** read the canvas themselves (`currentDocument()`,
  `session.ts:291-312`) and make the text with the main instance; they never
  wait for the worker.
* **Diagnostics** name blocks by ID. Badges for blocks deleted since are
  skipped (`apps/desktop/src/editor/diagnostics/badges.ts:113-121`), as during
  the debounce today.
* **Anything that acts on an answer checks the canvas first.** A quick fix
  (M3, one undo step) applies only when the block the diagnostic names is
  still on the canvas with what the fix expects. The landing of a toolbox
  block uses the latest index, synchronously, and resolves a block the index
  has not seen from its place, as in M2.
* **Spans.** `b2c:preview` runs from the run's start until the answer is
  applied (its latency). Its phases are `b2c:preview:read` (reading and
  serialising the canvas), `b2c:preview:update` (recorded on the page's
  timeline from the worker's `startedAt` and `ms`), `b2c:preview:receive`
  (parsing the answer and the index), `b2c:preview:commit` and
  `b2c:preview:publish`.
* **The service.** `PreviewService` (`preview/service.ts:11-18`) becomes:

  ```ts
  export interface PreviewAnswer {
    readonly result: UpdateResult;
    readonly scope: ScopeIndex | null;
    /** The worker's timing of `update()`, or null on the main thread. */
    readonly timing: { readonly startedAt: number; readonly ms: number } | null;
  }
  export interface PreviewService {
    readonly thread: () => 'worker' | 'main';
    /** Resolves null when a newer request replaced this one. */
    update(documentJson: string, options: PreviewOptions): Promise<PreviewAnswer | null>;
    /** After a `CoreTrap`: a fresh instance (a new worker, or the main instance's restart). */
    restart(): Promise<void>;
  }
  ```

  `workerPreviewService` (new, `preview/workerService.ts`) wraps the client
  and switches to `mainThreadPreviewService` when the client fails. The
  pipeline's `recover()` calls `service.restart()` instead of
  `host.restart()` (`pipeline.ts:350-359`).

### 6. Trusted Types, the frozen prototype and the probe

* **Trusted Types.** The page creates the worker through a policy named
  `b2c-core-worker`, whose `createScriptURL` accepts exactly the URL of the
  `?worker&url` import (made absolute against `document.baseURI`) and throws
  for anything else. The policy is created once, and only when
  `globalThis.trustedTypes` exists; otherwise the URL is passed as it is. It
  lives in `apps/desktop/src/lib/trustedTypes.ts` beside the trial's
  collector. When Trusted Types are enforced, the enforced CSP must list it
  (`trusted-types b2c-core-worker`). The worker uses no Trusted Types sink
  (module workers have no `importScripts`, and the ESLint bans of 08 §8.8
  apply to its files), so the report-only header is not added to its
  response.
* **The frozen prototype.** The worker's entry imports `./freeze` first, a
  module whose only statement is `Object.freeze(Object.prototype)`. Imports
  are evaluated in order, so the glue and the host run frozen, as the page
  does.
* **The probe.** End-to-end builds pass a probe to `serveCoreWorker`
  (`import.meta.env.MODE === 'e2e'` in the entry, ADR-0009's seam; other
  builds do not contain it). It tries `eval('1')` and a request to
  `http://ipc.localhost/`, listens for the worker's
  `securitypolicyviolation` events, and reports
  `{ eval: 'blocked' | 'allowed'; request: 'blocked' | 'allowed' | 'failed'; directives: string[] }`.
  The page's policy allows that request and the worker's does not, so the
  result shows which policy the worker runs under.

### 7. Files

| Path | What |
| --- | --- |
| `crates/b2c-lang/src/query.rs` | `ScopeIndexParts`, `Analysis::scope_index_parts` |
| `crates/b2c-core-wasm/src/{lib,session,wire}.rs` | `scope_index` export; `clipboard_make`'s options argument; the clipboard's analysis cache |
| `packages/b2c-core-wasm/src/instance.ts` | `instantiate(module)`, moved out of `loader.ts` |
| `packages/b2c-core-wasm/src/loader.ts` | `coreModule()` |
| `packages/b2c-core-wasm/src/scopeIndex.ts` | `ScopeIndex` |
| `packages/b2c-core-wasm/src/worker/{protocol,host,client}.ts` | §3 |
| `apps/desktop/src/editor/preview/worker/{core.worker,freeze,probe}.ts` | The worker's entry |
| `apps/desktop/src/editor/preview/{service,workerService,pipeline}.ts` | §5 |
| `apps/desktop/src/editor/services/symbols.ts`, `toolbox/plugin.ts` | `analysisScope` reads the index |
| `apps/desktop/src/app/store/state.ts` | `AnalysisState.scope` |
| `apps/desktop/src/lib/trustedTypes.ts` | The `b2c-core-worker` policy |
| `apps/desktop/vite.config.ts` | ES module worker, entry files under `workers/` |
| `apps/desktop/src-tauri/src/window.rs` | `WORKER_CSP` on `/workers/*.js` |
| `apps/desktop/src/e2e/index.ts` | `previewThread()`, `stopPreviewWorker()`, `probePreviewWorker()`, `coreMemory()` |

### 8. Formats and versions

No change to the project format (`formatVersion`), the clipboard format or
the IPC contract (`IPC_VERSION`), and no new diagnostic code: a failing
worker is shown with the existing `trapRecovered` notice. The core's exports
change (`scope_index`, `clipboard_make`'s third argument) in one build with
their callers, and the build guard already refuses an out-of-date core
(`apps/desktop/vite/wasmCore.ts`). The worker protocol (1) and the index
format (1) never leave one running app, so they need no upgrade path.

## Consequences

**What changes where** is in the table of files above. No Blockly seam changes
(ADR-0014), and no dependency is added: Vite builds workers itself.

**What gets harder.**

* Two instances on two threads: a bug can now sit in the hand-off (order,
  staleness, a lost reply). The shim tests and the E2E flow cover it.
* Two policies, the page's and the worker's, both stated in 08 §8.8 and
  checked by the consistency test.
* The app's first Trusted Types policy must be listed when Trusted Types are
  enforced.
* Memory: each instance starts with 1,245,184 bytes of linear memory
  (measured on the current `b2c_core_wasm_bg.wasm`) and grows to the largest
  document it handled. The E2E hook reports both.
* Copy, cut, paste and duplicate cost one analysis on the main thread at
  large sizes (*owner to confirm*).
* The preview benchmark measures something else (Tests), and its history
  restarts.

**Performance (expected; measured in the spike and the W5 A/B run).** Per
edit at 1,000 blocks, the main thread does the read (8 ms), the receipt
(parsing the answer and the index, estimated at 2–5 ms) and the store
updates: about 15 ms instead of 56–58 ms. The preview's latency stays within
a few milliseconds of today's. `webview.edit-1000.p95-all` should fall by
about the `update()` time, around 3 %, within the gate's noise: Blockly's
drawing dominates there, and the render patch (M3-2) is the larger lever. The
guessing game's edit task should fall from about 36 to about 30 ms, which
helps M3-9's 50 ms budget as M3's blocks grow. Selection, dragging and cold
start should not change.

**Security ([08](../spec/08-security.md)).**

* The code that loads untrusted project JSON on every edit runs in a context
  with no DOM, no IPC key, no isolation key and, under its own policy, no
  network and no `eval`. The loader and every limit of
  [05 §5.6](../spec/05-project-format.md#56-validation-limits) apply
  unchanged, in the worker as on the main thread.
* The page's CSP is not relaxed. The worker's policy is stricter than the
  page's, and a webview that applies the page's policy to the worker still
  runs it strictly.
* The page validates every reply's shape and parses the core's JSON with the
  same checks as before; the index is checked before use. A worker that
  misbehaves is restarted, never trusted further.
* The worker's bundle contains no `@tauri-apps` code and no embedded bytes
  (a build test).
* The M3 threat-model review records the spike's results and the probe's per
  webview ([open items](../security/threat-model-reviews.md#open-items-for-the-m3-review)).
* No new IPC command, file write, `unsafe` code or dependency.

**Specification sections to change** (with the W5 work, not before):

* [01 §1.4](../spec/01-overview.md#non-functional-goals), N4: measured as the
  preview worker's `update()` in the webview.
* [02 §2.2](../spec/02-architecture.md#22-high-level-component-diagram): the
  diagram shows the preview worker and the main-thread instance.
* [02 §2.3](../spec/02-architecture.md#23-repository-layout): the
  `packages/b2c-core-wasm/` line names the worker's client and host and the
  scope index.
* [02 §2.4.1](../spec/02-architecture.md#241-editing-in-the-webview-no-ipc):
  the core runs in the preview worker; dropdowns read the scope index.
* [02 §2.6](../spec/02-architecture.md#26-process-model-and-concurrency): the
  UI-thread bullet (every module previews in the worker, with the fallback).
* [06 §6.5](../spec/06-compiler-pipeline.md#65-stage--names-and-scopes): its
  last sentence ("The WASM core answers scope queries from the analysis
  cached by its last preview") becomes the scope index on the main thread.
* [06 §6.13](../spec/06-compiler-pipeline.md#613-performance-in-the-editor):
  the *Delivery under the CSP* bullet (the posted `Module`, the worker's
  policy), the bullet "Modules over 2,000 blocks run analysis in a Web
  Worker", and the last bullet (the main thread in M2; the per-block cache
  stays in M5).
* [08 §8.8](../spec/08-security.md#88-webview-and-ipc-hardening): the
  worker's policy after the CSP block, and the Trusted Types bullet (the
  `b2c-core-worker` policy).
* [08 §8.13](../spec/08-security.md#813-continuous-security-process): a
  nightly security case for the worker.
* [09 §9.2](../spec/09-quality-and-delivery.md#92-testing-strategy): the
  *Benchmarks* bullet (the preview metric's new definition, the main-thread
  metric) and the spans bullet (`b2c:preview:receive`, the worker's
  `update` span).

The developer guide (`desktop-architecture.md`, `desktop-security.md`,
`testing.md`), the benchmarks' README and the package README describe the
worker with the same work.

## Tests

**Rust.**

* `b2c-lang`: `scope_index_parts` lists every block that `scope_at` answers,
  with the same position, and every parent is smaller than its node; the
  cases of ADR-0012 (parked stacks, a disabled definition) are covered.
* `crates/b2c-core-wasm/tests/scope.rs`: `scope_index()` is `null` before an
  update and unchanged by one whose document does not load. For every
  example, the M3-D1 assignment projects and every file of the security
  corpus, it is version 1 JSON with indexes in range and the update's
  content hash, and a reference lookup in the test over that JSON agrees
  with `symbols_in_scope` for every block and input.
* `tests/clipboard.rs`: `clipboard_make` with indent widths 2 and 4 gives
  that width on `text/plain`; on an instance that never previewed, payload
  and text equal those of one that did; two calls on one document analyse
  once.
* `tests/robustness.rs`: `scope_index()` never panics on its corpus.
* The criterion benchmark reports `native.pipeline.scope-index` (not gated).

**TypeScript (Vitest).**

* `packages/b2c-core-wasm/test/scopeIndex.test.ts`, with the real core
  (skipped without a build unless `B2C_REQUIRE_WASM` is set): for every
  example, assignment project and security file, the generated 1,000-block
  document, and documents changed by a seeded list of edits (move, delete,
  disable, park), every block with `input` null, each of its input names
  and a name it lacks: `ScopeIndex.symbolsAt` equals `core.symbolsInScope`.
  Malformed indexes (another version or content hash, a parent not smaller
  than its node, an index out of range) are refused by `parse`.
* `test/worker.test.ts`: client and host joined by Node's `MessageChannel`
  (the worker shim; real structured cloning). The handshake and versions;
  answers equal to the main-thread `update()` for every example; three
  updates while one runs (the second resolves `null`, the third runs); a
  reply with another ID; a trap (a glue that throws: `CoreTrap`, then a
  fresh instance); a hang (fake timers: terminated and restarted); garbage
  messages; three failed starts (`failed`); the host with `Object.prototype`
  frozen.
* `apps/desktop/src/editor/preview/pipeline.test.ts`: answers out of order
  (the older is dropped), `invalidate()` between a run and its answer, an
  answer held during a drag and applied at its end (the newest only),
  `isIdle()` while an answer is outstanding, `syncFailed` only for applied
  runs, the spans, the switch to the main thread.
* `pipeline.core.test.ts`: the same pipeline over the worker shim with the
  real core, including the moved-block dirty case.
* `services/services.test.ts`: menus are built from the index, with no call
  to `symbolsInScope` (a spy).
* Clipboard tests: `text/plain` has the machine's indent width; a duplicate
  analyses once.
* `apps/desktop/src-tauri`: `window.rs` unit tests (the policy is added to
  `/workers/*.js` app responses only, not to HTML, `/assets/` scripts or
  other hosts, and an existing CSP header is kept); `tests/consistency.rs`
  checks `WORKER_CSP` against 08 §8.8.
* A build test of the app (beside `vite/wasmCore.test.ts`): exactly one
  entry under `workers/`; nothing it loads contains `WASM_BASE64` or
  `@tauri-apps`; the page creates it through the `b2c-core-worker` policy.

**End to end.**

* Every flow and the exit test run with the worker.
* `e2e/specs/flows/preview-worker.e2e.ts`: `previewThread()` is `worker` on
  both systems; an edit previews; after `stopPreviewWorker()` the next edit
  previews on a new worker; each M3-D1 assignment project opens and its
  preview equals the golden C++.
* `e2e/specs/security/worker.e2e.ts` (nightly): injected script cannot start
  a worker from a `blob:` or `data:` URL, while the app's worker URL starts
  (the positive control); through the E2E hook, an exception that the
  security README records, the probe reports `eval` and the IPC request
  blocked on both webviews.
* The Trusted Types table stays at no violation on WebView2.

**Benchmarks.**

* `webview.preview-1000.p95` is redefined as the `b2c:preview` span, from
  the run's start until the answer is applied, instead of the long task the
  heartbeat finds (that task no longer holds the run). `accepted.json` gets a
  `redefined` entry for Linux and Windows.
* A new `webview.preview-1000.main-p95`: per edit, the sum of the
  main-thread phases (`read`, `receive`, `commit`, `publish`). Reported in
  M3, gated with M3-9's budgets (*owner to confirm*).
* The W5 pull request runs the A/B job, and no gated metric may regress. The
  pull request's job leaves out cold start, so a manual A/B run
  (`bench-ab.yml`) compares `webview.cold-start` on both systems.

## Owner to confirm

1. Every module previews in the worker, not only modules over 2,000 blocks
   as 06 §6.13 says now (*owner to confirm*).
2. Copy, cut, paste and duplicate stay synchronous on the main-thread
   instance and analyse the document there: about 40 ms per action at
   1,000 blocks, once per unchanged document. The alternative is pasting
   through the worker, which makes paste asynchronous; copy cannot be
   (*owner to confirm*).
3. `canonical` stays on the main thread, unlike the plan's sketch
   (*owner to confirm*).
4. When the worker fails for good, the preview falls back to the main thread
   with a log entry and no banner (*owner to confirm*).
5. `webview.preview-1000.p95` becomes the preview's latency, and the new
   main-thread metric is only reported until M3-9 gates the small-program
   budgets (*owner to confirm*).
