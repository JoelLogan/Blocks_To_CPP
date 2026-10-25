# M3 contracts: the preview worker (ADR-0016)

The exact interfaces behind [ADR-0016](../../adr/0016-preview-in-a-web-worker.md),
for the W5 package brief: the callers of the core, how the worker is built
and loaded, the messages, the scope index, the clipboard arguments, the
pipeline's rules, the files, the tests and the benchmarks. The ADR records
the decision and its reasons; this file records what the code must match.
If the two disagree, the ADR wins and this file is corrected. Status: draft,
with the ADR (Proposed).

## 1. Callers of the core

Every caller in M2, and what it uses after the change.

| Caller | Core call | Answer needed at once? | After the change |
| --- | --- | --- | --- |
| Preview pipeline, after each debounced edit (`apps/desktop/src/editor/preview/pipeline.ts:215`) | `update` | No: awaited behind `PreviewService` | The worker (the main instance before the worker's first `started`, and in the fallback) |
| Field menus and call labels (`packages/blockly-ext/src/fields/symbol-ref.ts:148-155`, `src/mutators/variadic.ts:322-326`), through `analysisScope` (`apps/desktop/src/editor/services/symbols.ts:72-89`, wrapped at `toolbox/plugin.ts:79-81`) | `symbolsInScope` | Yes: Blockly builds a menu synchronously | The scope index |
| A block from the toolbox settled where it lands, in the drop's event group (`apps/desktop/src/editor/toolbox/landing.ts:151`); *Make a variable*'s taken names (`toolbox/scope.ts:489-503`) | `symbolsInScope` | Yes: inside Blockly's drag end, one undo step | The scope index |
| Connection checker (`services/symbols.ts:167-189`, `packages/blockly-ext/src/checker/checker.ts:93`) | none: `preview.blockTypes` and the TypeScript conversion table | Yes, but it reads the store only | Unchanged |
| Copy, cut, duplicate (`apps/desktop/src/editor/clipboard/controller.ts:174`) | `clipboardMake` | Yes: a copy fills the DOM `copy` event in the same task (`controller.ts:25-26`, `clipboard/bridge.ts:1-21`) | Main instance, with the applied preview passed in (§5) |
| Paste, duplicate (`controller.ts:271-278`, `:350`) | `pastePrepare`, `canonical` | Yes: the actions run synchronously | Main instance, with the index's answer passed in (§5) |
| Opening a project (`apps/desktop/src/editor/load.ts:84-96`) | `load`, `canonical` | No, and rare | Main instance |
| Save, build, autosave every 30 s (`features/project/lifecycle.ts:782-784`, `:867`; `features/build-run/document.ts:60`, `:87`; `features/recovery/autosave.ts:31`, `:207`) | `canonical` | Called synchronously, once per action | Main instance |
| The project name check (`app/layout/ProjectTitle.tsx:103`) and the module-switch check (`editor/sync/session.ts:541-551`) | `canonical` | Yes: synchronous validation | Main instance |
| An update that failed twice (`preview/pipeline.ts:283`) | `canonical` | No | Main instance |

`conversionTable` and `version` stay on the main instance.

## 2. Building and loading the worker

* **Entry.** `apps/desktop/src/editor/preview/worker/core.worker.ts`, imported
  with Vite's `?worker&url`. `apps/desktop/vite.config.ts` sets
  `worker.format: 'es'` and the worker's entry file name
  `workers/[name]-[hash].js`. A scratch build with the repository's Vite
  8.3.4 emits it there.
* **Creation.** `new Worker(url, { type: 'module', name: 'b2c-preview' })`,
  through the Trusted Types policy below when `globalThis.trustedTypes`
  exists.
* **The WebAssembly.** `packages/b2c-core-wasm/src/loader.ts` exports
  `coreModule(): Promise<WebAssembly.Module>` (the compile it already keeps).
  `instantiate(module)` moves to `src/instance.ts`, which never references
  `#pkg-bytes`; the worker imports only `instance.ts`, the glue and the host.
  Nothing the worker loads contains `WASM_BASE64` (the build test of §8).
* **The worker's policy.** In `apps/desktop/src-tauri/src/window.rs`, beside
  `TRUSTED_TYPES_REPORT_ONLY` (`window.rs:40`):

  ```rust
  pub(crate) const WORKER_CSP: &str =
      "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'none'";
  ```

  The `on_web_resource_request` handler (`window.rs:77-79`) adds it as
  `Content-Security-Policy` to every app response whose path starts with
  `/workers/` and whose content type is JavaScript, and keeps an existing
  CSP header. Tauri 2.12.4 calls the handler for every response of its
  protocol (`tauri` crate, `src/protocol/tauri.rs:188-190`) and adds its own
  CSP only to `.html` assets (`src/manager/mod.rs:422-439`,
  `src/protocol/tauri.rs:182-184`). The report-only Trusted Types header is
  not added to worker responses.
* **Trusted Types.** `apps/desktop/src/lib/trustedTypes.ts` creates the policy
  `b2c-core-worker` once, only when `globalThis.trustedTypes` exists. Its
  `createScriptURL` accepts exactly the `?worker&url` URL made absolute
  against `document.baseURI`, and throws for anything else. When Trusted
  Types are enforced, the enforced CSP lists it
  (`trusted-types b2c-core-worker`).
* **The frozen prototype.** The entry's first import is `./freeze`, whose only
  statement is `Object.freeze(Object.prototype)`.
* **The probe.** End-to-end builds only (`import.meta.env.MODE === 'e2e'` in
  the entry, ADR-0009's seam):

  ```ts
  export interface WorkerProbe {
    readonly eval: 'blocked' | 'allowed';
    readonly request: 'blocked' | 'allowed' | 'failed';
    /** The violated directives of the worker's `securitypolicyviolation` events. */
    readonly directives: readonly string[];
  }
  ```

  It tries `eval('1')` and a request to `http://ipc.localhost/`. The page's
  policy allows that request and `WORKER_CSP` does not.

## 3. The message protocol

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
      /** How long `update()` alone took, in milliseconds (N4's metric). */
      updateMs: number;
      /** How long `scope_index()` took, in milliseconds. */
      indexMs: number;
    }
  | { p: 1; kind: 'trapped'; id: number; message: string }
  | { p: 1; kind: 'probed'; id: number; probe: WorkerProbe };
```

Rules:

* **Versions.** Every message carries `p: 1`. Another `p`, an unknown `kind`
  or a field of the wrong type is a protocol error: the worker logs and
  ignores it, the client restarts the worker. The `started` reply's version
  must equal the main instance's `version()`.
* **Request IDs** are positive integers that increase per client. A reply
  must carry the ID of the request in flight; any other is a protocol error.
* **One request at a time, the newest waiting.** The client posts an `update`
  only while none runs. A newer `update` replaces the waiting one, whose
  promise resolves to `null`. There is no cancel message: the worker never
  holds more than one request, and a running `update()` cannot be
  interrupted; only the watchdog stops it.
* **Sizes.** The document text is cut as `core.ts` does before it crosses (one
  code unit over 32 MiB, so the loader still reports `B2C-E0101`;
  `packages/b2c-core-wasm/src/core.ts:114-116`); only `indentWidth` crosses
  in the options (`core.ts:157`).
* **Structured clone, no transfer.** The core's JSON is posted unparsed and
  parsed once on the main thread with `core.ts`'s checks (`core.ts:53-108`),
  so error envelopes and protocol errors behave as in M2. Transferring would
  mean encoding to bytes and decoding again on the main thread, which costs
  what it saves.
* **Timeouts.** 20 s from `start` to `started`; 30 s from posting an `update`
  to its reply (the watchdog).

Restarts have one owner for each event:

| Event | The worker | The client | The request in flight | A failed start? |
| --- | --- | --- | --- | --- |
| `trapped` | Has dropped its instance and instantiated a fresh one from the `Module` it holds, as `resetCore()` does | Nothing more; posts the waiting request | Rejects with `CoreTrap` | No |
| `startFailed`, or no `started` within 20 s | — | Terminates it and starts a new worker | None yet | Yes |
| `error`, `messageerror` or a protocol error | — | Terminates it and starts a new worker; posts the waiting request once that has answered `started` | Rejects with `CoreTrap` | Yes, when the worker had given no `updated` reply |
| No reply to an `update` within 30 s | — | The same | Rejects with `CoreTrap` | No; logged |

* **The fallback.** Three failed starts in a row make the client's state
  `failed`; its waiting request then rejects with `CoreError` (`init`), and
  the service runs it, and every later update, on the main instance. An
  `updated` reply resets the count.
* **Before the first `started`.** Until a worker of this session has answered
  `started`, the service runs updates on the main instance. After that,
  requests wait for the worker, also during a restart.
* **Error envelopes** (invalid options) reject with `CoreError`, as today.
  Messages and logs name no project text.

The client (`src/worker/client.ts`) and the host (`src/worker/host.ts`):

```ts
export interface CoreWorkerClient {
  readonly state: 'starting' | 'ready' | 'failed';
  /** Whether a worker of this client has ever answered `started`. */
  readonly everStarted: boolean;
  /** @throws CoreError (`init`) when the client has failed. */
  ready(): Promise<CoreVersion>;
  /** Resolves `null` when a newer request replaced this one before it was sent. */
  update(documentJson: string, options: PreviewOptions): Promise<WorkerUpdate | null>;
  /** End-to-end hook only: terminates the worker and handles it as an `error` event. */
  kill(): void;
  /** End-to-end builds only. */
  probe(): Promise<WorkerProbe>;
  dispose(): void;
}

export interface WorkerUpdate {
  readonly result: UpdateResult;
  readonly scope: ScopeIndex | null;
  /** When `update()` started, on the page's clock. */
  readonly startedAt: number;
  readonly updateMs: number;
  readonly indexMs: number;
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

`WorkerLike` and `WorkerScopeLike` are the members of `Worker` and
`DedicatedWorkerGlobalScope` the two sides use (`postMessage`, `onmessage`,
`onerror`, `onmessageerror`, `terminate`).

## 4. The scope index

* **Rust.** `b2c_lang::Analysis::scope_index_parts(&self) -> ScopeIndexParts<'_>`
  is a read-only view of the private `ScopeIndex`
  (`crates/b2c-lang/src/query.rs:71-85`): the trail (parent, C++ name,
  symbol), each recorded block with its position at the block and its
  per-input positions, each module's function list, and each module's
  parked point (`main_ends`, `parked_point`, `query.rs:97-111`).
  `b2c-core-wasm` encodes it (`wire.rs`) for the kept analysis in
  `Session::scope_index(&self) -> Option<String>`, exported as
  `scope_index() -> String` (the JSON below, or `null` before an update has
  loaded a document). `EXPECTED_EXPORTS` and `EXPORTED_FUNCTIONS`
  (`packages/b2c-core-wasm/scripts/build.mjs:55-74`), `src/glue.d.ts` and
  `src/core.ts` change together.
* **Format, version 1** (declared in `src/api.ts`):

  ```ts
  export interface ScopeIndexData {
    readonly v: 1;
    /** The analysed document's content hash: the update's `contentHash`. */
    readonly contentHash: string;
    /** Every name the trail and the function lists use (the C++ spellings), once each, in first use. */
    readonly names: readonly string[];
    /**
     * The trail, three numbers per node: its parent (-1 for none, always smaller than the node),
     * its name (index into `names`) and its symbol (index into the update's `symbols`; -1 for a
     * hidden name, or a symbol the program does not list).
     */
    readonly trail: readonly number[];
    /** Per module, in document order: the functions calls can use, as (name, symbol) index pairs. */
    readonly functions: readonly (readonly number[])[];
    /** Per module, in document order: the node at the end of its first lowered `main` body, or -1. */
    readonly parked: readonly number[];
    /**
     * One entry per recorded block, sorted by block ID: its ID, the node and module at the block
     * (module -1 when nothing was recorded there), then a name, node and module for each
     * statement list or value input with a position of its own. Node -1: no local symbol.
     */
    readonly blocks: readonly (readonly (string | number)[])[];
  }
  ```

* **Worked example.** A module whose `main` (`b001`) creates `score`
  (`b002`) and then `total` (`b003`), both without a starting value, before a
  print (`b004`), and which defines a function `greet` (`b005`) with no
  parameters and an empty body. The update's `symbols` are `greet`, `score`,
  `total` (sorted by name). Each `create` first appends a hidden node for
  its name and records its `VALUE` position there
  (`crates/b2c-lang/src/lower/stmt.rs:152-155`; `lower/mod.rs:702-717`;
  `scope.rs:118-137`, `:170-183`), then declares the name on the node it
  started from:

  ```json
  {"v":1,"contentHash":"9f…","names":["score","total","greet"],
   "trail":[-1,0,-1, -1,0,1, 1,1,-1, 1,1,2],
   "functions":[[2,0]],"parked":[3],
   "blocks":[["b001",-1,0,"BODY",-1,0],["b002",-1,0,"VALUE",0,0],
             ["b003",1,0,"VALUE",2,0],["b004",3,0],["b005",-1,0,"BODY",-1,0]]}
  ```

  So `b003` sees `greet` and `score`; inside its `VALUE` (node 2) `total` is
  hidden; `b004` sees all three. The example was derived from the lowering
  by hand. W5 turns it into a fixture produced by the real `scope_index()`;
  if they differ, the analyser is right and this example is corrected.
* **Lookup.** `packages/b2c-core-wasm/src/scopeIndex.ts`:

  ```ts
  export class ScopeIndex {
    /** @throws CoreError (`protocol`) for a malformed index. */
    static parse(json: string, preview: PreviewResult): ScopeIndex;
    /** `Analysis::scope_at`: null when the index has no answer for the block. */
    symbolsAt(blockId: string, input: string | null): SymbolInfo[] | null;
    /** `Analysis::scope_after`. */
    symbolsAfter(blockId: string): SymbolInfo[] | null;
    /** `Analysis::parked_scope`: empty for a module the document does not have. */
    parkedScope(moduleId: string): SymbolInfo[];
    /** `visible_at` of `crates/b2c-core-wasm/src/scope.rs:89-103`: what a paste binds against. */
    visibleAt(moduleId: string, blockId: string | null, input: string | null): SymbolInfo[];
  }
  ```

  `symbolsAt` follows `query.rs:204-242` and `:295-298`: the input's
  position when one is recorded under that name, else the block's, else
  `null`; walk the trail
  from that node, each name counting once and adding its symbol if it has
  one, following a parent only when it is smaller than the node; then the
  module's functions whose names were not seen; each symbol once, in the
  order of the update's `symbols` (which Rust sorted by name, then ID).
  `symbolsAfter` adds the variable the block declares, as `scope_after` does
  (`query.rs:308-326`; the editor's `toolbox/scope.ts:153-173` already does
  the same). `parkedScope` walks from the module's `parked` node, or lists
  the module's functions alone for -1. Block IDs go into a `Map`. `parse`
  checks the version, the content hash against the preview's, that every
  parent is smaller than its node and every index is in range; a lookup
  never throws.
* **Store.** `AnalysisState` (`apps/desktop/src/app/store/state.ts:61-73`)
  gains `scope: ScopeIndex | null`, set in the same `setAnalysis` call as
  `preview`. `analysisScope` takes `{ scope, preview }` instead of
  `{ core, preview }`.

## 5. The clipboard arguments

```text
clipboard_make(document_json, block_ids_json, options_json, reuse_json | null) -> string
paste_prepare(clipboard_text, document_json, target_json, seed_hex, visible_json | null) -> string
```

* `options_json` is the preview options (`{"indentWidth": 2 | 4}`); the C++
  on `text/plain` uses it when the core generates the code itself.
* `reuse_json` is `null` or the applied preview's
  `{ contentHash, indentWidth, files, sourceMap }`. When the copied
  document's content hash and the indent width equal these, the core cuts
  the text from these files and source map (`crates/b2c-core-wasm/src/cut.rs:29-33`,
  which skips any range it cannot use and never panics) and analyses
  nothing. Otherwise it ignores them.
* `visible_json` is `null` or `{ contentHash, symbols }`: the index's
  `visibleAt` for the paste target. When the document's content hash equals
  `contentHash`, the core binds references against these symbols
  (`session.rs:277-283`) and analyses nothing; otherwise it ignores them.
* Without a match, `Session` analyses the document and keeps that one
  analysis, keyed by content hash and indent width, so a duplicate, or
  several pastes into an unchanged document, analyse once.
* A malformed `reuse_json` or `visible_json` fails with
  `FacadeError::InvalidArguments`. The payload and the binding rules of
  05 §5.12 do not change.
* The clipboard controller passes `reuse_json` and `visible_json` from the
  store's applied preview and index only; it never builds them otherwise.
* M3-5's compact source map (W5) changes what `sourceMap` holds; `cut.rs`
  reads the form M3-5 defines.

## 6. The service and the pipeline

`apps/desktop/src/editor/preview/service.ts`:

```ts
export interface PreviewAnswer {
  readonly result: UpdateResult;
  readonly scope: ScopeIndex | null;
  /** When `update()` started (the page's clock) and how long it and `scope_index()` took. */
  readonly timing: { readonly startedAt: number; readonly updateMs: number; readonly indexMs: number };
}

export interface PreviewService {
  readonly thread: () => 'worker' | 'main';
  /** Resolves null when a newer request replaced this one. */
  update(documentJson: string, options: PreviewOptions): Promise<PreviewAnswer | null>;
  /**
   * After `update` rejected with `CoreTrap`: resolves when a fresh instance can take the retry.
   * Main thread: `host.restart()`. Worker: waits until the worker that replaced its own instance,
   * or the one the client started, is ready. Never restarts a worker.
   */
  recover(): Promise<void>;
}
```

`workerPreviewService` (new, `preview/workerService.ts`) wraps the client: it
uses the main instance until the client's first `started` and after the
client has failed; otherwise the worker. In main-thread mode it calls
`scope_index()` after each `update()` and times both.

The pipeline (`preview/pipeline.ts`):

* **At the read** a run records its sequence number, the invalidation epoch,
  the project handle and `base`, the project's `document` it reads against.
* **The apply rule**, for a run's answer and for `commitWithoutPreview`'s
  commit: apply when the pipeline is not disposed, the handle is the same,
  the epoch is the same (no `invalidate()`), the sequence number is greater
  than the last applied one, and `project.document` is `base` or the
  document the pipeline's own last commit wrote. Otherwise drop the answer;
  when the drop was for the document and the run is the latest, schedule a
  run.
* **The hold.** While `isBusy()` (a pointer drag), an answer that passes the
  checks so far is kept instead of applied, replacing an older kept one.
  `release()`, called where the session ends the drag
  (`sync/session.ts:559`), applies it by the rule above.
* **A trap.** On `CoreTrap` from `update`: if the run is still the latest,
  `await service.recover()` and retry once; otherwise stop. A second trap, or
  any other failure, goes to `commitWithoutPreview`, which runs `canonical()`
  on the main instance and calls `host.restart()` on a trap there, as now
  (`pipeline.ts:273-302`, `:349-359`).
* **`isIdle()`**: no run waits, nothing is held, and the latest run's answer
  is applied. The drag-start snapshot reads the canvas when the pipeline is
  not idle (`sync/session.ts:515-534`), which happens more often now that the
  main thread is free while `update()` runs.
* **Undo and redo** act on the canvas; answers never write it. An undo during
  an update is read by the next run.
* **Spans.** `b2c:preview` from the run's start until its answer is applied
  (any hold included). Phases: `b2c:preview:read`, `b2c:preview:update` and
  `b2c:preview:index` (placed on the page's timeline from `startedAt`,
  `updateMs` and `indexMs`), `b2c:preview:receive` (parsing the answer and the
  index), `b2c:preview:commit` and `b2c:preview:publish`.

## 7. Files

| Path | What |
| --- | --- |
| `crates/b2c-lang/src/query.rs` | `ScopeIndexParts`, `Analysis::scope_index_parts` |
| `crates/b2c-core-wasm/src/{lib,session,wire,scope}.rs` | The `scope_index` export; the clipboard arguments of §5; the one-entry analysis cache |
| `packages/b2c-core-wasm/src/instance.ts` | `instantiate(module)`, moved out of `loader.ts` |
| `packages/b2c-core-wasm/src/loader.ts` | `coreModule()` |
| `packages/b2c-core-wasm/src/scopeIndex.ts` | `ScopeIndex` |
| `packages/b2c-core-wasm/src/worker/{protocol,host,client}.ts` | §3 |
| `apps/desktop/src/editor/preview/worker/{core.worker,freeze,probe}.ts` | The worker's entry |
| `apps/desktop/src/editor/preview/{service,workerService,pipeline}.ts` | §6 |
| `apps/desktop/src/editor/services/symbols.ts`, `toolbox/plugin.ts` | `analysisScope` reads the index |
| `apps/desktop/src/editor/clipboard/controller.ts` | Passes the applied preview and the index's answer (§5) |
| `apps/desktop/src/app/store/state.ts` | `AnalysisState.scope` |
| `apps/desktop/src/lib/trustedTypes.ts` | The `b2c-core-worker` policy |
| `apps/desktop/vite.config.ts` | ES module worker, entry files under `workers/` |
| `apps/desktop/src-tauri/src/window.rs` | `WORKER_CSP` on `/workers/*.js` |
| `apps/desktop/src/e2e/index.ts` | `previewThread()`, `stopPreviewWorker()` (the client's `kill()`), `probePreviewWorker()`, `coreMemory()` |

## 8. Tests

**Rust.**

* `b2c-lang`: `scope_index_parts` lists every block that `scope_at` answers,
  with the same position; every parent is smaller than its node; each
  module's parked point gives `parked_scope`; the cases of ADR-0012 (parked
  stacks, a disabled definition) are covered.
* `crates/b2c-core-wasm/tests/scope.rs`: `scope_index()` is `null` before an
  update and unchanged by one whose document does not load. For every
  example, the M3-D1 assignment projects and every file of the security
  corpus it is version 1 JSON with indexes in range and the update's content
  hash, and a reference lookup in the test over that JSON agrees with
  `scope_at` for every block and input, `scope_after` for every block,
  `parked_scope` for every module and `visible_at` for each kind of paste
  target. The worked example of §4 is a fixture.
* `tests/clipboard.rs`: indent widths 2 and 4 on `text/plain`; an instance
  that never previewed gives the payload and text of one that did; two calls
  on one document analyse once; with `reuse_json` from that document's
  preview the result is the same and nothing is analysed (a counter); a
  `reuse_json` with another content hash or indent width is ignored;
  `paste_prepare` with `visible_json` from the reference lookup equals the
  analysing path; malformed arguments are `InvalidArguments`.
* `tests/robustness.rs`: `scope_index()` never panics on its corpus, and
  neither do the clipboard calls with garbage files or source maps in
  `reuse_json`.
* The criterion benchmark reports `native.pipeline.scope-index` (not gated).

**TypeScript (Vitest).**

* `packages/b2c-core-wasm/test/scopeIndex.test.ts`, with the real core
  (skipped without a build unless `B2C_REQUIRE_WASM` is set): for every
  example, assignment project and security file, the generated 1,000-block
  document, and documents changed by a seeded list of edits (move, delete,
  disable, park), every block with `input` null, each of its input names and
  a name it lacks: `symbolsAt` equals `core.symbolsInScope`. `symbolsAfter`,
  `parkedScope` and `visibleAt` match fixtures produced by the Rust reference
  lookup. Malformed indexes (another version or content hash, a parent not
  smaller than its node, an index out of range) are refused by `parse`.
* `test/worker.test.ts`: client and host joined by Node's `MessageChannel`
  (real structured cloning, including the `Module`). The handshake and
  versions; answers equal to the main-thread `update()` for every example;
  three updates while one runs (the second resolves `null`, the third runs);
  a reply with another ID; a trap with a request waiting (`CoreTrap`, a fresh
  instance in the same worker, the waiting request answered, no new worker);
  an `error` with a request waiting (a new worker, the waiting request
  posted to it); a hang (fake timers: replaced, not counted as a failed
  start); garbage messages; three failed starts (`failed`); an `error` after
  `updated` replies (not counted); the host with `Object.prototype` frozen.
* `preview/workerService.test.ts`: the main instance before the first
  `started` and after `failed`; `recover()` never restarts a worker.
* `apps/desktop/src/editor/preview/pipeline.test.ts`: answers out of order
  (the older is dropped); `invalidate()` between a run and its answer; a
  rename while an update is outstanding (the answer is dropped, the next run
  commits the new name, `dirty` is right); a save adopted while an update is
  outstanding (the answer is dropped, the saved `generator` and viewports
  stay, `dirty` stays false); `commitWithoutPreview` under the same rule; an
  answer held during a drag and applied at its end, the newest only, in the
  worker and the main-thread modes, with the rule checked again at release;
  `isIdle()` while an answer is outstanding or held; a trap retried only
  while its run is the latest; a `canonical` trap after a worker trap (one
  `host.restart()`, no worker restart); `syncFailed` only for applied runs;
  the spans; the switch to the main thread.
* `pipeline.core.test.ts`: the same pipeline over the worker shim with the
  real core, including the moved-block dirty case.
* `services/services.test.ts`: menus are built from the index, with no call to
  `symbolsInScope` (a spy).
* Clipboard controller tests: `text/plain` has the machine's indent width;
  the reuse arguments come only from the applied preview; a duplicate
  analyses at most once.
* `apps/desktop/src-tauri`: `window.rs` unit tests (the policy is added to
  `/workers/*.js` app responses only, not to HTML, `/assets/` scripts or
  other hosts, and an existing CSP header is kept); `tests/consistency.rs`
  checks `WORKER_CSP` against 08 §8.8.
* A build test of the app (beside `vite/wasmCore.test.ts`): exactly one entry
  under `workers/`; nothing it loads contains `WASM_BASE64` or `@tauri-apps`;
  the page creates it through the `b2c-core-worker` policy.

**End to end.**

* Every flow and the exit test run with the worker.
* `e2e/specs/flows/preview-worker.e2e.ts`: `previewThread()` is `worker` on
  both systems; an edit previews; after `stopPreviewWorker()` the next edit
  previews on a new worker; each M3-D1 assignment project opens and its
  preview equals the golden C++.
* `e2e/specs/security/worker.e2e.ts` (nightly): injected script cannot start
  a worker from a `blob:` or `data:` URL, while the app's worker URL starts
  (the positive control); through the E2E hook, an exception the security
  README records, the probe reports `eval` and the IPC request blocked on
  both webviews.
* The Trusted Types table stays at no violation on WebView2.

## 9. Benchmarks

| Metric | Definition | Gated | Target |
| --- | --- | --- | --- |
| `webview.preview-1000.update-p95` (new) | `update()` alone at 1,000 blocks, timed where it runs (the worker; the main thread in the fallback), from the `b2c:preview:update` span; p95 of 10 edits per sample, 10 samples | yes (regressions) | N4: < 50 ms |
| `webview.preview-1000.p95` (redefined) | The `b2c:preview` span: from the run's start until its answer is applied. `accepted.json` gets a `redefined` entry for Linux and Windows | yes (regressions) | none |
| `webview.preview-1000.main-p95` (new) | Per edit, the sum of the main-thread phases (`read`, `receive`, `commit`, `publish`) | no | none |
| `webview.copy-1000.max-task`, `webview.paste-1000.max-task`, `webview.duplicate-1000.max-task` (new) | The longest main-thread task of one copy, paste or duplicate of a 20-block stack at 1,000 blocks, with and without a matching applied preview | no | N12: within 100 ms |

M3-9 gates the small-program budgets on the guessing game's metrics
(`webview.guessing-game.edit.max-task`, `webview.guessing-game.select.max-task`).
The W5 pull request runs the A/B job, and no gated metric may regress. Its
job leaves out cold start, so a manual run of `bench-ab.yml` compares
`webview.cold-start` on both systems.

## 10. Shared M3 allocation

The cross-ADR review of 2026-10-25 set one table of diagnostic codes and one
version plan for ADR-0016 to ADR-0019; they are kept in one place,
[types.md §6](types.md#6-diagnostic-codes) and
[§7](types.md#7-versions), so that they cannot drift. ADR-0016 allocates no
code. Its own rows of the version plan:

* the worker protocol and the scope index are version 1 each, internal to
  one running app, and change no format;
* the source map passes through the worker unchanged (M3-5's compact form
  must keep the optional `module` key ADR-0019 adds);
* `IPC_VERSION` does not change.

## 11. Specification sections to change

With the W5 work, not before. Where several of ADR-0016 to ADR-0019 change
one section, the owner wave is named; the others add their own sentences.

* [01 §1.4](../../spec/01-overview.md#non-functional-goals), N4: measured as
  the preview worker's `update()` in the webview.
* [02 §2.2](../../spec/02-architecture.md#22-high-level-component-diagram):
  the diagram shows the preview worker and the main-thread instance.
* [02 §2.3](../../spec/02-architecture.md#23-repository-layout): the
  `packages/b2c-core-wasm/` line names the worker's client and host and the
  scope index.
* [02 §2.4.1](../../spec/02-architecture.md#241-editing-in-the-webview-no-ipc):
  the core runs in the preview worker; dropdowns read the scope index.
* [02 §2.6](../../spec/02-architecture.md#26-process-model-and-concurrency):
  the UI-thread bullet (every module previews in the worker, with the
  fallback; `docs/spec/02-architecture.md:491-495`).
* [06 §6.5](../../spec/06-compiler-pipeline.md#65-stage--names-and-scopes):
  only its last sentence ("The WASM core answers scope queries from the
  analysis cached by its last preview") becomes the scope index on the main
  thread. The rest of §6.5, and 06 §6.6 and 03 §3.5.3, are W1's (ADR-0017
  and ADR-0018).
* [06 §6.13](../../spec/06-compiler-pipeline.md#613-performance-in-the-editor),
  owned by W5 with ADR-0016 and carrying ADR-0018's cache-key sentence: the
  *Delivery under the CSP* bullet (the posted `Module`, the worker's policy),
  the bullet "Modules over 2,000 blocks run analysis in a Web Worker"
  (`docs/spec/06-compiler-pipeline.md:590`, `:602-605`), and the last bullet
  (the main thread in M2; the per-block cache stays in M5).
* [08 §8.8](../../spec/08-security.md#88-webview-and-ipc-hardening): the
  worker's policy after the CSP block, and the Trusted Types bullet (the
  `b2c-core-worker` policy).
* [08 §8.13](../../spec/08-security.md#813-continuous-security-process): a
  nightly security case for the worker.
* [09 §9.2](../../spec/09-quality-and-delivery.md#92-testing-strategy),
  which all four ADRs change, each in its own bullets: the *Benchmarks*
  bullet (§9's metrics) and the spans bullet (`b2c:preview:index`,
  `b2c:preview:receive`, the worker's `update` span).

The developer guide (`desktop-architecture.md`, `desktop-security.md`,
`testing.md`), the benchmarks' README (N4's target moves to
`webview.preview-1000.update-p95`; the redefined `webview.preview-1000.p95`
loses it) and the package README describe the worker with the same work.
