# M3 contracts: W5's first item, the worker spike and M3-1's first part

What wave W5 starts with ([next steps §3.4](../next-steps.md#34-waves)):
the spike that [ADR-0016](../../adr/0016-preview-in-a-web-worker.md) §2
asks for before anything else ("Verify first"), and then the parts of
M3-1 (the preview in a Web Worker) that do not depend on W2's blocks. It
refines [worker.md](worker.md) for what the spike found; where the two
differ, §9 proposes the correction to worker.md, and worker.md itself is
not rewritten here. If this file and the ADR disagree, the ADR wins.

Written on 2026-10-27 from `df4b4f5` of `ccr-01f82d9b-v70u20`, on the
branch `m3/w5-worker-spike`, while W2 builds the language blocks on the
coordinating branch.

## 1. Scope

**In:**

1. **The spike** (ADR-0016 §2, items a to d): a dedicated module worker
   starts from the app's origin on WebKitGTK (`tauri://localhost`) and on
   WebView2 (`http://tauri.localhost`); the page's compiled
   `WebAssembly.Module` can be posted to it; which policy it runs under
   (the probe of ADR-0016 §6); what the Trusted Types trial reports for
   `new Worker` with and without the `b2c-core-worker` policy. Measured on
   Linux at 1,000 blocks: the worker's start-up, a preview's round trip
   through `postMessage` against the main-thread call, and the
   structured-clone cost of the largest messages. Windows is proved by the
   E2E job of `desktop.yml` on this branch, which runs the same check.
2. **M3-1, the parts that need nothing from W2:** building and loading the
   worker (worker.md §2), the message protocol and its validation (§3), the
   client and the host (§3), the service wrapper with its fallback to the
   main thread (§6), the worker's policy in the backend (§2), the Trusted
   Types policy (§2), the frozen prototype and the probe (§2), and their
   tests (§8). Behind a build flag that stays off (§4).

**Out, and why:**

* **The scope index** (worker.md §4): it needs
  `Analysis::scope_index_parts()` in `crates/b2c-lang`, which W2 owns, and
  a `scope_index()` export of `crates/b2c-core-wasm` whose content W2's
  blocks change (new symbol kinds, members). The protocol carries
  `scopeJson` already; the worker sends `null` until the export exists.
  So `AnalysisState.scope` and `analysisScope` reading the index
  (`services/symbols.ts`, W2's) wait too.
* **The clipboard arguments** (worker.md §5): new arguments of
  `clipboard_make` and `paste_prepare`, which depend on the index, and the
  spike's copy, paste and duplicate measurements, which only mean something
  with W2's blocks in the documents. The Rust API of `b2c-core-wasm` stays
  unchanged here.
* **The pipeline's full apply rule and the drag hold** (worker.md §6): this
  branch adds the half of the apply rule that keeps an answer from undoing
  a rename or an adopted save (§5.4); the rest (an older answer applied
  while a newer run waits, the hold during a pointer drag, `release()`)
  belongs with switching the flag on, because only then can an answer
  arrive in another task than its read.
* **The benchmarks' metrics** (worker.md §9) and **the specification**
  (worker.md §11, `docs/spec/` is W2's while W2 runs): §10.

## 2. The spike

### 2.1 What runs

The check is an E2E-build-only part of the test hook
(`apps/desktop/src/e2e/workerCheck.ts`), started with
`startWorkerCheck(rounds)` and polled with `workerCheck()` (a hook call
cannot wait for a promise on every driver). On the open project it:

1. creates a worker from the plain string URL, without the Trusted Types
   policy, terminates it, and counts the page's new violations;
2. creates the app's worker through the `b2c-core-worker` policy with the
   client of §5.2 (the page's compiled `Module` posted in `start`; the
   worker's core version must equal the main instance's) and times it
   until `started`; counts the page's new violations;
3. runs the project's `update()` in the worker and on the main thread and
   compares the two answers (JSON of the parsed results);
4. runs `rounds` more updates through the worker, timing each round trip
   (from posting to the parsed answer, on the page's clock) and `update()`
   alone in the worker, and the same `update()` on the main thread;
5. times `structuredClone` of the update request (the document) and of its
   reply (the core's answer), the median of five;
6. asks the worker's probe whether `eval('1')` ran and whether a request to
   `http://ipc.localhost/` was blocked by its policy.

The E2E spec `e2e/specs/flows/preview-worker.e2e.ts` runs it on the
guessing game (3 rounds; every check must hold) and on the benchmarks'
1,000-block document (20 rounds; the numbers go to standard output). Both
run in the PR job on Linux and Windows, so Windows is checked on every push.

### 2.2 Verdict and numbers

**WebKitGTK 2.52.6 (Ubuntu 24.04, the E2E app under xvfb): clean.**

| Item (ADR-0016 §2) | Found |
| --- | --- |
| (a) a module worker from `tauri://localhost` | Starts from `workers/core.worker-<hash>.js`; Tauri's protocol serves it and its glue chunk, and `on_web_resource_request` is called for both (the debug log shows `WORKER_CSP` added to each) |
| (b) posting the `Module` | Works; the worker instantiates it with its own glue, and its answers equal the main thread's for the guessing game and the 1,000-block document (`sameAsMain`) |
| (c) the worker's policy | Its own. `eval` is refused. The page reaches the IPC endpoint (`ipc://localhost/`) and the worker does not. A control build whose `WORKER_CSP` also allowed `connect-src ipc: http://ipc.localhost` let the worker's request through, so it is the worker's policy that refuses it, not the page's and not the platform. WebKitGTK fires no `securitypolicyviolation` event in the worker, so the probe reports `failed`, not `blocked`, there |
| (d) Trusted Types | WebKitGTK 2.52 has Trusted Types: the trial reports one violation for `new Worker` from a plain string URL, and none through the `b2c-core-worker` policy |

Measured on this container (a slower CPU than CI's runners: the main
thread's `update()` at 1,000 blocks takes 65–107 ms here, against 42.5–44
ms in the M2 measurements), three runs of 20 rounds at 1,000 blocks
(152,004 characters of document, 466,399 of answer). WebKit's clock counts
whole milliseconds here.

| Measure | Result |
| --- | --- |
| Worker start-up (creating it until `started`, the `Module` posted) | 15, 15 and 37 ms (the first run's disk cache was cold); 16–19 ms for the guessing game |
| First update in a fresh worker | 116–151 ms (the instance grows its memory); 18–22 ms for the guessing game |
| Round trip (posting the request until the answer is parsed on the page) | median 81 ms, p95 108 ms |
| `update()` alone in the worker | median 73.5 ms, p95 101 ms |
| `update()` on the main thread, with its parse | median 75 ms, p95 95 ms |
| Round trip less the worker's `update()` | median 7 ms, p95 10 ms, 5 to 12 ms (posting, the reply's clone and the main thread's `JSON.parse` of the answer) |
| `structuredClone` of the update request | 0.15 ms (mean of 20) |
| `structuredClone` of the reply | 0.65–0.8 ms (mean of 20) |

So per edit at 1,000 blocks the main thread keeps about 7 ms of the
preview's work instead of all of it, and the latency grows by about the
same 7 ms; the largest message costs under a millisecond to clone. This is
in line with ADR-0016's estimate (a receipt of 2–5 ms). The parse of the
answer is now the main thread's largest part of the receipt; the compact
source map (M3-5) shrinks it.

**WebView2 (Windows, `windows-2025`, `http://tauri.localhost`): clean.**
From the E2E job of `desktop.yml` on this branch (run 40572876535, job
122431463376, commit `c1df203`).

| Item (ADR-0016 §2) | Found |
| --- | --- |
| (a) a module worker from `http://tauri.localhost` | Starts; WebView2's resource requests for the worker's scripts reach the same handler, so it gets `WORKER_CSP` |
| (b) posting the `Module` | Works; answers equal the main thread's for both documents |
| (c) the worker's policy | Its own: `eval` refused, and the IPC request refused with `connect-src` reported in the worker (WebView2 fires the event there, so the probe says `blocked`; directives seen: `connect-src`, `script-src`), while the page reaches the endpoint |
| (d) Trusted Types | One violation for a plain string URL, none through the policy |

| Measure, 1,000 blocks, 20 rounds | Result |
| --- | --- |
| Worker start-up | 14.7 ms (17.5 ms for the guessing game) |
| First update in a fresh worker | 73.9 ms (9.4 ms for the guessing game) |
| Round trip | median 55.3 ms, p95 59.3 ms |
| `update()` alone in the worker | median 52.0 ms, p95 56 ms |
| `update()` on the main thread, with its parse | median 54.1 ms, p95 58.9 ms |
| Round trip less the worker's `update()` | median 3.2 ms, p95 3.6 ms (3.0 to 3.8) |
| `structuredClone` of the request / the reply | 0.16 ms / 0.42 ms |

**Verdict: clean on both webviews.** Under the app's real policy, with no
`unsafe-eval` and the WebAssembly delivered as ADR-0010 has it (the page
compiles the embedded bytes, the worker only instantiates the posted
`Module`), the core loads and runs in a dedicated module worker on
WebKitGTK and on WebView2, under the worker's own stricter policy. The
main thread's share of a 1,000-block preview falls to the receipt (3 ms on
the Windows runner, about 7 ms on this slower Linux container), and the
latency grows by the same amount. So step 2 of the brief went ahead (§3).

## 3. Design

### 3.1 Where the code lives

As worker.md §7 has it, with two refinements:

* **A package entry of its own.** `@blocks2cpp/b2c-core-wasm/worker`
  (`src/worker/index.ts`) exports the protocol, the client and the host.
  The worker imports only it; nothing it reaches imports `#pkg-bytes`
  (`src/instance.ts` holds `instantiate`, moved out of `loader.ts`), so
  the worker's bundle never holds the embedded bytes. *Alternative:* the
  worker imports the package's main entry and relies on tree-shaking to
  drop the loader. Rejected: `loader.ts`'s dynamic import of `#pkg-bytes`
  would still be a chunk of the worker's build.
* **Raw calls.** The worker posts the core's JSON unparsed (worker.md §3).
  `createCore` now returns, beside the `CoreWasm`, a `RawCore` with
  `version()` and `update(documentJson, optionsJson)` that return the
  core's text with the same bounds and trap handling; `parseUpdate` and
  `parseVersion` are the parses `CoreWasm` uses, exported for the client.
  `CoreWasm` itself is unchanged.

### 3.2 Building and loading

* `apps/desktop/vite.config.ts`: `worker.format: 'es'` and the worker
  build's entry, chunk and asset names under `workers/`. Vite 8.3.4 emits
  `workers/core.worker-<hash>.js` and `workers/glue-<hash>.js` (the glue is
  a dynamic import of `instance.ts`); neither holds `WASM_BASE64` or
  `@tauri-apps` code.
* `src/editor/preview/worker/core.worker.ts` imports `./freeze` first, then
  serves the protocol; the probe (`./probe.ts`) is passed only when
  `import.meta.env.MODE === 'e2e'`, and the release build leaves it out.
* `src/editor/preview/worker/spawn.ts` creates the worker from
  `core.worker.ts?worker&url` made absolute against `document.baseURI`,
  through `coreWorkerScriptUrl` of `src/lib/trustedTypes.ts`: the
  `b2c-core-worker` policy, created once, only when `globalThis.trustedTypes`
  exists, accepting exactly that URL.
* `apps/desktop/src-tauri/src/window.rs`: `WORKER_CSP` and
  `add_worker_csp`, called from the same `on_web_resource_request` handler
  as the Trusted Types header: added to app responses under `/workers/`
  whose type is `text/javascript` or `application/javascript`, never twice,
  keeping a policy already there.

### 3.3 The protocol

As worker.md §3. The checks (`parseRequest`, `parseReply`): a
structured-cloned plain object (its prototype `Object.prototype` or none)
with `p: 1`, a known `kind` and exactly that kind's keys, each of the right
type: IDs positive safe integers, times finite and not negative, messages
at most 512 characters, a probe's directives at most 32 names of the
collector's form, `module` a `WebAssembly.Module`. An extra key is a
protocol error too, so nothing unexpected crosses unnoticed.

The host chains requests, so a `probe` or an `update` that arrives while it
replaces a trapped instance waits for it. After a trap it instantiates a
fresh instance from its `Module` first and then replies `trapped`; when that
fails it replies `startFailed`, which the client treats as a failed start.
Its log lines name no project text, only the error's name and message from
the runtime.

### 3.4 The client

`createCoreWorkerClient` as worker.md §3, with an `acceptVersion` option
(the page passes "equal to the main instance's `version()`"; a mismatch is
a protocol error, so a stale worker script fails three starts and the
preview falls back) and a `log` option. Two cases worker.md leaves open:

* **A webview that cannot post a `Module`.** `postMessage` throws a
  `DataCloneError` at once; that cannot change by retrying, so the client
  fails at once (no three attempts) and the service falls back.
* **The page's own compile fails.** The main instance fails the same way
  and reports it; the client fails at once.

`update` resolves `null` for a request a newer one replaced and after
`dispose()`; it rejects `CoreTrap` for a trap or a replacement while the
request ran, `CoreError` for an error envelope, and `CoreError` (`init`)
once the client has failed. `WorkerUpdate` carries `scopeJson` unparsed
until `ScopeIndex` exists (§1).

### 3.5 The service and the pipeline

`PreviewService` becomes worker.md §6's interface: `update` resolves a
`PreviewAnswer` (the result, the index, the timing) or `null`, `thread()`
says where the next update runs, and `recover()` gets a fresh instance
after a `CoreTrap`. `mainThreadPreviewService` keeps M2's behaviour (with
`recover()` = `host.restart()`). `workerPreviewService`
(`preview/workerService.ts`):

* runs updates on the main instance until its client's first `started`,
  and starts the client after the first main-thread update, so a project's
  first preview and the cold start do not wait for it;
* runs them in the worker once it has started, also while it is replaced;
* falls back to the main instance for good when the client fails, logging
  once, with no banner (ADR-0016 owner point 4);
* `recover()`: after a trap in the worker, waits for the worker (it has
  replaced its own instance, or the client is starting a new worker), and
  never restarts one; after a trap on the main instance, `host.restart()`.

The pipeline (`preview/pipeline.ts`) takes `null` as an overtaken run,
calls `service.recover()` instead of `host.restart()` before the retry, and
applies an answer only when the project's document is still the one the
run read (§5.4).

## 4. The flag

**A build flag, `B2C_PREVIEW_WORKER`, off.** `vite.config.ts` turns the
environment variable `B2C_PREVIEW_WORKER=1` at build time into
`import.meta.env.B2C_PREVIEW_WORKER`, which the editor reads to pick
`workerPreviewService` instead of the main-thread service. Every build in
CI, and every shipped build, leaves it off; the worker's script is still
built and served (it is what the spike's E2E check loads).

Why a build flag and not a setting:

* A setting is user-visible and lives in `settings.json` (the IPC
  contract, `crates/b2c-ipc`, regenerated types, a format to migrate);
  this switch exists only until the worker is integrated, and nobody but a
  developer should turn it on before then.
* Off is the M2 code path, unchanged, so W2's blocks are integrated on a
  main thread whose behaviour is known.
* Until the rest of the apply rule and the drag hold land (§1), an answer
  from the worker could arrive while a block is being dragged.
* **Until the scope index lands, menus go stale with the flag on.** The
  field menus, the landing and *Make a variable* ask the main instance's
  `symbolsInScope`, which answers from that instance's last `update()`;
  once the worker previews, the main instance stops updating. Measured: with
  `B2C_PREVIEW_WORKER=1`, 15 of 16 local Linux E2E tests pass (editor,
  rename, save and reload, clipboard, errors, typed slots, selection, crash
  recovery, the worker flow), and the M2 exit flow fails because the menu
  of a *set* block lacks the variable `guess` made after the worker started.
  worker.md §4's index is the fix.

So the flag must stay off in every build until W5's integration step after
W2, which adds the index and the rest of the apply rule and then removes the
flag, making the worker the default.

## 5. Changes

### 5.1 Files this branch owns

| Path | What |
| --- | --- |
| `packages/b2c-core-wasm/src/instance.ts` (new) | `instantiate(module)`, `startFailed`, moved out of `loader.ts` |
| `packages/b2c-core-wasm/src/loader.ts` | `coreModule()` |
| `packages/b2c-core-wasm/src/core.ts` | `RawCore`, `parseUpdate`, `parseVersion`, `previewOptionsJson`, `boundText` exported |
| `packages/b2c-core-wasm/src/worker/{index,protocol,host,client}.ts` (new) | §3.3, §3.4 |
| `packages/b2c-core-wasm/package.json` | the `./worker` export |
| `packages/b2c-core-wasm/test/worker*.test.ts` (new) | §7 |
| `apps/desktop/src/editor/preview/worker/{core.worker,freeze,probe,spawn}.ts` (new) | §3.2 |
| `apps/desktop/src/editor/preview/{service,workerService,pipeline,index}.ts` | §3.5 |
| `apps/desktop/src/editor/EditorWorkspace.tsx` | picks the service by the flag |
| `apps/desktop/src/lib/trustedTypes.ts` | the `b2c-core-worker` policy |
| `apps/desktop/src/e2e/{contract,index,workerCheck}.ts`, `src/main.tsx` | the spike's check and the worker hooks |
| `apps/desktop/vite.config.ts` | the worker build, the flag |
| `apps/desktop/src-tauri/src/window.rs` | `WORKER_CSP`, `add_worker_csp` |
| `apps/desktop/e2e/specs/flows/preview-worker.e2e.ts` (new) | §2.1, §7 |

### 5.2 IPC, formats, versions

No IPC command, no change to `crates/b2c-ipc`, the project format, the
clipboard format or the settings. The Rust API of `b2c-core-wasm` is
unchanged (no export added or changed; `EXPECTED_EXPORTS` is the same).
The worker protocol is version 1 and never leaves one running app.

### 5.3 Dependencies

None.

### 5.4 The apply rule's first half

At the read a run notes the project's `document`; its answer (and the
commit of an update that failed) applies only when the project's document
is still that one, and is dropped otherwise, with a new run scheduled when
the dropped run was the latest. worker.md §6 also allows "the document the
pipeline's own last commit wrote"; with only the latest run committing, a
run reads its base after every older run has been overtaken, so that case
cannot arise until older answers may apply (the second half), and this
branch leaves it out. On the main thread this never drops anything (the read, `update()`
and the commit are one task, as in M2); with the worker it keeps an answer
from undoing a rename or a save adopted while the update ran.

## 6. Security and accessibility

* **Security.** The worker that loads untrusted project JSON on every edit
  has no DOM, no IPC (no `Tauri-Invoke-Key`, no isolation frame), no
  network and no `eval` under `WORKER_CSP`, and runs with
  `Object.prototype` frozen. Its files hold no `@tauri-apps` code and no
  embedded bytes. Every reply is checked before use and a misbehaving
  worker is replaced, never trusted. The page's CSP is unchanged: no
  `blob:` or `data:` worker. The worker's URL goes through a Trusted Types
  policy that accepts exactly that URL. Logs and errors carry error names
  and runtime messages, never project text. The probe exists only in E2E
  builds.
* **Accessibility.** Nothing visible changes: no new control, message or
  banner; a worker that fails falls back silently (logged).

## 7. Tests

* **Package** (`packages/b2c-core-wasm/test/`, Vitest under Node):
  * `worker-protocol.test.ts`: every request and reply kind accepted;
    another version, an unknown kind, a missing, extra or mistyped field,
    a class instance, an array, a probe with a bad directive refused.
  * `worker.test.ts`: client and host joined by Node's `MessageChannel`
    (real structured cloning, the `Module` included), with the real core
    when it is built (skipped without it unless `B2C_REQUIRE_WASM` is set,
    as `wasm.test.ts`) and with a fake glue otherwise: the handshake and
    the version check; answers equal to the main-thread `update()` for every
    example; three updates while one runs (the second resolves `null`, the
    third runs); a reply with another ID; a trap with a request waiting
    (`CoreTrap`, a fresh instance in the same worker, the waiting request
    answered, no new worker); an `error` with a request waiting (a new
    worker, the request posted to it); a hang (fake timers: replaced, not
    counted as a failed start); garbage replies; three failed starts
    (`failed`); a `DataCloneError` (failed at once); an `error` after
    answers (not counted); the host with `Object.prototype` frozen.
* **App** (Vitest, happy-dom): `workerService.test.ts` (the main instance
  before the first `started` and after `failed`; `recover()` never restarts
  a worker; the timing), `pipeline.test.ts` (an answer dropped after a
  rename or an adopted save, and the next run commits; `null` answers;
  `recover()` used for the retry), `trustedTypes.test.ts` (the policy
  accepts only the worker's URL, is created once), `e2e/workerCheck.test.ts`.
* **Build:** `vite/worker.test.ts` beside `vite/wasmCore.test.ts`: a
  production build has exactly one worker entry under `workers/`, and
  nothing under `workers/` contains `WASM_BASE64`, `@tauri-apps` or the
  probe.
* **Backend:** `window.rs` unit tests: the policy on `/workers/*.js` app
  responses only (not HTML, `/assets/` scripts, other paths, hosts or
  types), once, keeping an existing policy.
* **End to end:** `preview-worker.e2e.ts` on both webviews (§2.1).

## 8. Decisions for the owner

1. **The flag is a build flag, off** (§4). *Recommendation:* keep it off
   until W5's integration step after W2, which removes it.
2. **A webview that cannot post a `Module` falls back at once** (§3.4)
   instead of after three failed starts. *Recommendation:* yes; retrying
   cannot change it.
3. **An extra key in a message is a protocol error** (§3.3).
   *Recommendation:* yes; both sides ship in one build.

The decisions the spike informs are in §2.2.

## 9. Proposed corrections to worker.md

For the owner of worker.md to apply (this branch does not edit it):

1. **§2, the probe's request.** The IPC endpoint is `ipc://localhost/` on
   WebKitGTK and `http://ipc.localhost/` on WebView2; the probe tries both.
   WebKitGTK refuses a worker's request under its policy without a
   `securitypolicyviolation` event in the worker, so the probe cannot say
   `blocked` there. Proposed: the probe also makes the request from the page
   as a control, and the check is "the page reaches the endpoint, the worker
   does not" (`WorkerProbe.request` is then `blocked` or `failed`).
2. **§2 and §8, Trusted Types.** WebKitGTK 2.52 has Trusted Types as well:
   the trial reports `new Worker` from a plain string there too, and nothing
   through the policy. "The Trusted Types table stays at no violation"
   applies to both webviews, not only WebView2.
3. **§2, the build.** Vite 8.3.4 names the worker build's options
   `worker.rolldownOptions` (`rollupOptions` is a deprecated alias), and the
   worker is two files: `workers/core.worker-<hash>.js` and the glue's
   chunk (`instance.ts` imports `#glue` dynamically). The build test of §8
   is "exactly one *entry* under `workers/`".
4. **§2, the probe's `eval`.** A direct `eval` makes the bundler warn
   (and deoptimise the module); the probe uses an indirect one, which a
   policy without `'unsafe-eval'` refuses the same way.
5. **§3, the client.** Add `acceptVersion` (the page passes "equal to the
   main instance's `version()`") and `log`; a `DataCloneError` when posting
   the `Module`, a worker that cannot be created and a failed compile on the
   page make the client fail at once rather than after three starts.
   `WorkerUpdate.scope` is `scopeJson: string | null` until `ScopeIndex`
   exists, and `PreviewAnswer.scope` likewise.
6. **§3, extra keys.** A message with a key its kind does not have is a
   protocol error, like a missing one.
7. **§6, the pipeline.** The apply rule can land in two halves: the
   document check (this branch) first, and the rest (an older answer applied
   while a newer run waits, the drag hold, `release()`) with the flag's
   removal, since only then can an answer arrive in another task than its
   read.
8. **§9, measurements.** On WebKitGTK `performance.now()` counts whole
   milliseconds here, so a single clone cannot be timed; the spike times a
   clone as the mean of 20. The copy, paste and duplicate measurements of
   ADR-0016 §1 wait for W2's blocks and the clipboard arguments (§1).

## 10. Needs from W2, and later

* **Needs from W2** (paths W2 owns): `Analysis::scope_index_parts()` in
  `crates/b2c-lang/src/query.rs` (worker.md §4) once W2's symbol kinds and
  members are in; then W5 adds the `scope_index()` export and the
  `ScopeIndex` lookup. `analysisScope` in
  `apps/desktop/src/editor/services/symbols.ts` reads the index (worker.md
  §4, "Store").
* **The specification** (W2 owns `docs/spec/` while it runs): the sections
  of worker.md §11, plus 08 §8.8's text for `WORKER_CSP` (and then
  `tests/consistency.rs` checks it against the spec), with the integration.
* **The integration step after W2:** the scope index and the clipboard
  arguments; the rest of the apply rule and the drag hold; the spans and
  metrics of worker.md §6 and §9; `previewThread()`,
  `stopPreviewWorker()`, `probePreviewWorker()` and `coreMemory()` hooks
  and the E2E flows of worker.md §8 with the worker on; removing the flag.

## 11. Status

As of 2026-10-27, branch `m3/w5-worker-spike`.

**Done:**

* **The spike: clean on both webviews** (§2.2). WebKitGTK 2.52.6 locally
  under xvfb (three runs, a control build for the policy) and WebView2 on
  `windows-2025` in CI (Desktop run 40572876535, E2E job 122431463376 at
  `c1df203`; again at `ca84c2e`, run 40574017390). The worker loads from the
  packaged script, takes the posted `Module`, previews exactly as the main
  thread does, and runs under `WORKER_CSP`.
* **M3-1's first part** (§3), behind the build flag `B2C_PREVIEW_WORKER`,
  off in every build (§4): the package's `./worker` entry (protocol and
  checks, client, host), `instance.ts` and `coreModule()`, the worker's
  entry with the frozen prototype and the E2E-only probe, the Vite worker
  build under `workers/`, the `b2c-core-worker` Trusted Types policy,
  `WORKER_CSP` in the backend, `PreviewService` as worker.md §6 has it,
  `workerPreviewService` with its fallback, the pipeline's `null`
  answers, `recover()` and the first half of the apply rule, and the E2E
  hooks `startWorkerCheck`, `workerCheck`, `previewThread` and
  `stopPreviewWorker`.
* **Tests** (§7): package 316 tests, 96 % of lines (the worker's client and
  host at 95 %, every example through the worker with the real core, the
  host with `Object.prototype` frozen); desktop 2,127 tests, 94.9 % of
  lines; the backend's `window.rs` tests; the worker build test; the
  `preview-worker` E2E flow (3 tests) on Linux and Windows, and locally also
  in a build with the flag on.
* Checks run before the last code push: `cargo fmt`, the desktop crate's
  clippy (both feature sets), `cargo doc` with `-D warnings`,
  `B2C_REQUIRE_GXX=1 cargo test --workspace` (117 test binaries pass),
  every package's typecheck, lint and format check, the four
  `tools/check-*.py`, markdownlint. The Windows-target clippy of `ci.yml`
  was not run locally: no crate of the workspace changed, and `ci.yml`
  passed on the branch.

**Not done, and why:**

* The scope index, the clipboard arguments, `AnalysisState.scope` and
  `analysisScope` reading the index (§1, §10): they need W2's
  `crates/b2c-lang` and `services/symbols.ts`. Until they land, the flag
  must stay off: with it on, menus go stale (§4, measured).
* The second half of the apply rule and the drag hold (§1): with the
  flag's removal.
* The spans and benchmark metrics of worker.md §6 and §9, the
  specification changes of worker.md §11 and 08 §8.8's text for
  `WORKER_CSP` (W2 owns `docs/spec/` while it runs), `coreMemory()`, the
  nightly security case: with the integration step.
* The copy, paste and duplicate measurements of ADR-0016 §1: with the
  clipboard arguments, on W2's blocks.

**CI on the branch's head** (`ca84c2e`; the commit adding this section
changes only this file): `ci.yml` green. `desktop.yml`: frontend, frontend
tests, the app on both systems, and the E2E tests on both systems green;
the visual-diff steps fail on Linux and Windows (the canvas scene and the
toolbox scenes math, text, io and functions), exactly as on the
coordinating branch's `df4b4f5` (run 40257310666) and its later W2 runs:
the baselines predate W2's blocks. This branch changes no drawing, so no
scene is its own and no baseline was touched. `osv-scanner.yml` fails on
the branch and on the coordinating branch's latest run alike; this branch
changes no lockfile and adds no dependency.

**The coordinator's review of 2026-10-27** (five findings, each fixed with
a test that fails without the fix; none refuted):

1. *`acceptVersion` asked the live main core* (`7fbab0a`). Confirmed: a
   main instance that trapped while the worker previews threw out of the
   client's message handler and left a healthy worker `starting` until the
   start timeout. The client now treats a check that throws as a refusal
   (`worker.test.ts`, "treats an acceptVersion that throws as a refusal":
   three refusals at once, no timer, then `failed`), and
   `appWorkerPreviewService` reads the main version once, when it creates
   the client after a successful main-thread update (`spawn.test.ts`: a
   worker of that version is accepted after the main instance trapped, and
   while the host has no instance). `appWorkerPreviewService` takes the
   worker and module as optional dependencies for that test.
2. *The base check of the commit without a preview was untested; the
   "own last commit" clause never decides* (`fdcd05a`). Confirmed, both.
   New test "does not keep an edit without a preview when a save or rename
   wrote the document meanwhile"; the clause and its `written` field are
   gone (a run reads its base after every older run has been overtaken,
   and only the latest commits), the old test is renamed "applies an
   answer when nothing else wrote the document", and §3.5 and §5.4 say so.
3. *The E2E check asserted nothing about Trusted Types* (`ebbc5fc`).
   Confirmed. The collector counts `require-trusted-types-for` events on
   their own (`trustedTypesCount()`), the check reads that count, and the
   guessing-game flow asserts at least one report for a worker from a plain
   string and none through the policy (local Linux run: 1 and 0).
   `workerCheck.test.ts` fires other directives' reports in the same
   windows and expects them not counted.
4. *The flag test read the developer's environment* (`ebbc5fc`).
   Confirmed (it failed with `B2C_PREVIEW_WORKER=1` exported); it now
   stubs the variable and checks off, a wrong value and on.
5. *The failure text's comment* (`ebbc5fc`). The comment now says the name
   and the first 200 characters of the message are reported, and where
   they come from; the message stays, since it is what makes a failed E2E
   check diagnosable.

Checks before the push of these fixes: every package's typecheck, lint
and format check; the package's tests (317, 96 % of lines); the desktop
suite with coverage (2,133 of 2,134 pass; `navigation.test.ts`'s "include
Make a variable, which Enter presses", in the toolbox's keyboard code this
branch does not touch, timed out at its 20 s under coverage on this
container, as it did once before, and passes run on its own, twice);
the isolation tests; the four `tools/check-*.py`; `cargo fmt`; the
`preview-worker` E2E flow on Linux (3 of 3). No Rust changed in this
round.

The OSV-Scanner failure is GHSA-qwgh-2vcv-g2f7 (block-buffer 0.10.4 via
Tauri's sha2), excepted on the coordinating branch in `c22ae1c`; this
branch does not merge that branch, so the failure stays here until the
coordinator merges.

**Merge notes for the coordinator:**

* No file of W2's list is touched. Files outside this document's own
  list (§5.1) that changed: `apps/desktop/src/editor/sync/drag.test.ts`
  (its preview service adapted to the new `PreviewService` interface),
  `apps/desktop/src/editor/preview/coreHost.test.ts`,
  `apps/desktop/src/e2e/index.test.ts` (new hook dependencies) and
  `apps/desktop/src/lib/trustedTypes.test.ts`.
* Expected conflicts: none textual unless W2 also changed
  `src/e2e/contract.ts`, `src/e2e/index.ts` or its test (both add hook
  methods at the end; keep both), `EditorWorkspace.tsx`, `main.tsx`,
  `vite.config.ts`, `pipeline.ts` or its tests. A W2 test that builds its
  own `PreviewService` needs `thread`, `recover` and answers of the
  `PreviewAnswer` shape (see `drag.test.ts`).
* Nothing changes at run time in any build of CI or of a release: the
  editor's preview stays on the main thread unless the build sets
  `B2C_PREVIEW_WORKER=1`. What does run everywhere is serving the worker's
  two scripts (only the E2E hook starts the worker) and the backend's
  `WORKER_CSP` on them.
