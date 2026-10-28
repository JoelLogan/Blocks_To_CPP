# M3 contracts: W5's second part, performance and platform

What wave W5 still owes after its first part
([w5-worker.md](w5-worker.md), merged on 2026-10-28), from
[next steps §3.2](../next-steps.md#32-scope) (_Performance and platform_),
[§3.4](../next-steps.md#34-waves) (W5's row) and
[§3.8](../next-steps.md#38-w2-done-and-what-the-owner-decides) (_Toolbox
cost, for W5_, and _W3, W4 and W5 so far_): M3-1's second part (the preview
worker on by default), the toolbox flyout's cost per block, M3-5, M3-4,
M3-9, M3-6 and M3-7. It refines [worker.md](worker.md) and w5-worker.md;
where they differ, §11 proposes the correction, and neither is rewritten
here. If this file and [ADR-0016](../../adr/0016-preview-in-a-web-worker.md)
disagree, the ADR wins.

Written on 2026-10-28 from `8a3d8ec` of `ccr-01f82d9b-v70u20`, on the
branch `m3/w5-performance`, while the coordinator builds W3's second part
and another session builds W4's.

## 1. Scope

**In:**

1. **M3-1's second part: the preview worker on by default** (§2). The scope
   index (worker.md §4): `Analysis::scope_index_parts()` in
   `crates/b2c-lang/src/query.rs` (the one change this branch makes in
   `crates/b2c-lang`, w5-worker.md §10), the `scope_index()` export of
   `crates/b2c-core-wasm`, `ScopeIndex` in `packages/b2c-core-wasm`, and the
   editor's scope query answered from it while the worker previews; the rest
   of the pipeline's apply rule (worker.md §6); the spans and metrics
   (worker.md §6, §9); then the worker made the default, measured first on
   both webviews with W2's 86 blocks, with the main thread as the fallback
   w5-worker.md §3.5 describes.
2. **The toolbox flyout's cost per block** (§3): the first showing and the
   refresh profiled at 86 blocks and brought down in
   `apps/desktop/src/editor/toolbox/inflater.ts` and `continuous.ts`,
   keeping package T's reuse rules; the refresh benchmark gated (§6).
3. **M3-5** (§4): a compact source map for the preview, and a C++ panel that
   pauses while hidden.
4. **M3-4** (§5): lowering parked stacks for the scope query and block
   types. The lowering is in `crates/b2c-lang`, which the coordinator owns,
   so this branch writes it as an exact proposal (§10.1), with the tests that
   decide it, and does not implement it.
5. **M3-9** (§6): the small-program budgets measured and gated.
6. **M3-6** (§7): `pnpm doctor`.
7. **M3-7** (§8): a weekly job that follows `BUILDING.md` in Debian 12 and 13
   containers.

**Out, and why:**

* **The clipboard arguments** (worker.md §5). With the worker on, the main
  instance no longer previews, so `clipboard_make` and `paste_prepare` find
  no kept analysis for the document they are given and analyse it themselves
  (`session.rs`, `analysis_for`): the answers stay right (they always were
  for the document given), and a copy, paste or duplicate costs one analysis
  on the main thread. The arguments that pass the applied preview and the
  index's answer instead are an optimisation of that; they change
  `apps/desktop/src/editor/clipboard/` (W4's) and are proposed in §10.2 with
  the measurement that decides whether they are needed (`webview.copy-1000`,
  `paste-1000`, `duplicate-1000`, worker.md §9).
* **RD-15** (the skipped-lines marker on Windows) is W5's row in §3.4 but not
  in this brief; it stays for W5's close-out or W6.
* **The specification** (`docs/spec/`) is the coordinator's: the sections of
  worker.md §11, and those of §4 and §6 here, are listed in §12 for the
  coordinator's docs step.
* **M3-2 and M3-3** (the render patch and Blockly 13): moved after M3 by W0's
  spike.

## 2. M3-1's second part: the worker on by default

### 2.1 The scope index

**Rust, `b2c-lang`** (`query.rs`, additive; nothing else in the crate, not
even `lib.rs`: the new types are reached through the method, not named):

```rust
/// A position: the trail node there (`None`: no local symbol) and the module index.
pub struct ScopePoint { pub node: Option<usize>, pub module: usize }
/// The positions recorded for one block.
pub struct ScopeBlock<'a> {
    pub id: &'a BlockId,
    pub at: Option<ScopePoint>,
    pub inputs: Vec<(&'a str, ScopePoint)>,
}
/// A read-only view of the scope index (worker.md §4), for encoders.
pub struct ScopeIndexParts<'a> { /* private: &'a Analysis */ }

impl<'a> ScopeIndexParts<'a> {
    /// The trail: (parent, C++ name, symbol), parents always before their nodes.
    pub fn trail(self) -> impl Iterator<Item = (Option<usize>, &'a str, Option<&'a SymbolId>)>;
    /// Every recorded block, sorted by ID.
    pub fn blocks(self) -> impl Iterator<Item = ScopeBlock<'a>>;
    /// The program's modules, by module index.
    pub fn modules(self) -> impl Iterator<Item = &'a ModuleId>;
    /// Each module's functions, `(C++ name, symbol)` in symbol order.
    pub fn functions(self) -> impl Iterator<Item = &'a [(String, SymbolId)]>;
    /// Each module's parked point (`None`: the functions alone).
    pub fn parked(self) -> impl Iterator<Item = Option<usize>>;
    /// Each struct and enum with the members `with_members` lists after it.
    pub fn members(self) -> impl Iterator<Item = (&'a SymbolId, Vec<&'a SymbolId>)>;
    /// The variable each block creates that `scope_after` adds after it.
    pub fn created(self) -> Vec<(&'a BlockId, &'a SymbolId)>;
}

impl Analysis {
    pub fn scope_index_parts(&self) -> ScopeIndexParts<'_>;
}
```

Test in `query.rs`: a reference lookup over the parts agrees with `scope_at`
for every recorded block and input name, with `parked_scope`, and the
members and the created variable are those `with_members` and `scope_after`
use; every parent is smaller than its node. The exhaustive check on real
documents is in `b2c-core-wasm` (below), whose tests can load them. Three
refinements of worker.md §4, each because the lookup must equal the
analyser's answer exactly and not by inference:

* **Members are listed**, not inferred from `owner`: `with_members` adds a
  struct's `fields` (an enum's `values`) whose owner is the struct, in that
  order; a lookup that guessed them from `SymbolInfo.owner` would differ for
  a damaged project whose table has a member the struct does not list.
* **The variable a block creates is listed** (`created`), for
  `symbolsAfter`: `scope_after` finds it by the symbol's origin block, which
  `SymbolInfo` does not carry, and leaves out a catch part's error and a
  _for each_ item.
* **The modules are listed**, so `parkedScope(moduleId)` finds the module's
  index as `parked_scope` does (`program.modules`).

**Rust, `b2c-core-wasm`:** `Session::scope_index(&self) -> Option<String>`
encodes the kept analysis (`src/scope_index.rs`); the export
`scope_index() -> String` returns it or `null`. `EXPECTED_EXPORTS`,
`EXPORTED_FUNCTIONS` (`scripts/build.mjs`), `src/glue.d.ts` and `RawCore`
(`src/core.ts`, `scopeIndex()`) change together; `CoreWasm` does not (its
many stand-ins in tests stay valid), since only the worker's host asks.
Format version 1, as worker.md §4 with the three keys above:

```ts
export interface ScopeIndexData {
  readonly v: 1;
  readonly contentHash: string;
  readonly modules: readonly string[];
  readonly names: readonly string[];
  readonly trail: readonly number[];          // parent, name, symbol per node
  readonly functions: readonly (readonly number[])[];
  readonly parked: readonly number[];
  readonly blocks: readonly (readonly (string | number)[])[];
  /** Per struct or enum with members: [owner, member, …], symbol indexes. */
  readonly members: readonly (readonly number[])[];
  /** Per block that creates a variable `symbolsAfter` adds: [block ID, symbol]. */
  readonly created: readonly (readonly [string, number])[];
}
```

Symbol indexes are into the update's `symbols` (`Analysis::symbol_infos`,
sorted by name, then ID), so a lookup's answer is sorted by sorting the
indexes; a trail symbol the table does not list is -1, as `infos` skips it.
The user's name of a symbol (`ScopeIndex.names`, used by `infos`) is already
the `name` of the update's `SymbolInfo`, so it does not cross again.

Tests: `crates/b2c-core-wasm/tests/scope_index.rs`: `null` before an update,
and an update whose document does not load keeps the old index; for every
example (the ten assignments among them), every security project that loads
and the three generated 1,000-block documents (plain, collections, W2's
language blocks): version 1, the update's content hash, every parent smaller
than its node and every index in range, and a reference lookup written in the
test over the JSON alone equals `scope_at` (every block, with no input, each
of its inputs and a name it lacks), `scope_after` (every block) and
`parked_scope` (every module). Dropping the members from the reference lookup
makes all three fail. `tests/robustness.rs`: the export is JSON (`null` or an
object) after every generated input. The criterion benchmark
`native.pipeline.scope-index` (reported).

**TypeScript, `packages/b2c-core-wasm/src/scopeIndex.ts`:** `ScopeIndex` as
worker.md §4 (`parse`, `symbolsAt`, `symbolsAfter`, `parkedScope`,
`visibleAt`). `parse` checks the version, the content hash against the
preview's, that every parent is smaller than its node and every index is in
range; a lookup never throws. Tests: `test/scopeIndex.test.ts` with the real
core (skipped without a build unless `B2C_REQUIRE_WASM` is set): for every
example, every security project that loads, and the examples changed by three
seeded sets of edits (a statement disabled, parked on the canvas or deleted),
every block with `input` null, each of its input names and a name it lacks:
`symbolsAt` equals `core.symbolsInScope`; `visibleAt` after each block of the
guessing game binds a copied reference exactly when the core's `pastePrepare`
does; malformed indexes (17 kinds) are refused. `symbolsAfter` and
`parkedScope` are checked against the analyser by the Rust test above, whose
lookup is the same walk.

`visibleAt` is kept (worker.md §4) for §10.2; until then nothing calls it
but its test.

**The worker** posts `scope_index()`'s text with every `updated` reply
(`scopeJson`, already in the protocol), timed as `indexMs`. The main-thread
service does not call it (see §2.2).

### 2.2 Where the editor's scope query is answered

worker.md §4 puts the index in `AnalysisState.scope` and has
`analysisScope` (`services/symbols.ts`) read it. `symbols.ts` is the
coordinator's while W3 runs, and `app/store` is shared, so this branch
answers the same question one layer down, in files it owns:

* **`CoreHost.current()`** (`preview/coreHost.ts`) returns, instead of the
  raw instance, a view of it (one object per instance, so identity holds
  while the instance lives) whose `symbolsInScope` answers from the host's
  **applied index** when there is one, and from the instance otherwise.
  Every other method is the instance's. `setCore` still publishes the raw
  instance, which features use for `canonical` and the clipboard.
* **The applied index** is set by the pipeline in the same task as the
  preview it belongs to (`publish`): the answer's parsed index when the
  update ran in the worker, `null` when it ran on the main thread, whose
  instance then holds that very analysis (as in M2). So a menu always
  answers about the applied preview, from wherever that preview was made.
  `invalidate()` (a new document shown) clears it.

So every caller of worker.md §1 that asks `symbolsInScope` (the field
menus, the call labels, the landing, _Make a variable_) gets the index's
answer while the worker previews, with no change to their files.
_Alternative:_ the store field and `analysisScope` reading it, as worker.md
§4 has it. Not rejected: it is the cleaner end state, and §10.3 proposes it
to the coordinator; the host view then becomes the store's reader and the
behaviour does not change.

### 2.3 The rest of the apply rule, and the hold

In `preview/pipeline.ts`, as worker.md §6, refined by what the first half
taught (w5-worker.md §5.4):

* **An older answer is applied while a newer run waits**, when its sequence
  number is greater than the last applied one, the epoch (bumped by
  `invalidate()`) and the project handle are the same, and the project's
  document is the run's `base` or the document the pipeline's own last
  commit wrote. Without it, edits less than one `update()` apart would drop
  every answer until the typing stops (each answer is overtaken by the next
  read). On the main thread no two runs overlap, so nothing changes there.
* **The hold.** While `isBusy()` (a pointer drag), an answer that passes is
  kept instead of applied, replacing an older kept one, and applied by the
  rule above once `isBusy()` is false, checked every animation frame (no
  `release()` call is needed from the session). `isBusy` is a new optional
  pipeline option, `() => false` by default; the session passes
  `isDragging(workspace)` (§10.4, one line in `sync/session.ts`, W4's).
  Until it does, an answer may be applied during a drag: correct (the
  pre-drag snapshot and the base check do not depend on it), but the
  dragged blocks' diagnostics can change under the pointer.
* **`isIdle()`** stays "no run waits, and the latest run committed", so the
  drag-start snapshot reads the canvas while an answer is outstanding or held.
* **Spans** (worker.md §6): `b2c:preview` from the run's start until its
  answer is applied (a hold included); `b2c:preview:update` now the
  `update()` alone, placed on the page's timeline from the answer's
  `startedAt` and `updateMs` (where it ran); new `b2c:preview:index` likewise,
  `b2c:preview:receive` (parsing the answer and the index on the page), and
  `b2c:preview:wait` (posting until the answer, less nothing: the time the
  main thread was free). `read`, `commit` and `publish` as now.

### 2.4 Measure first, then the default

Measured before the switch, on WebKitGTK locally and on WebView2 through the
E2E job of `desktop.yml` (which runs the worker flow on both systems), with
W2's 86 blocks in the catalog: the preview benchmark at 1,000 blocks with
the worker and on the main thread (`B2C_PREVIEW_WORKER=0`, §2.5), the
guessing game's edit, the worker flow's round trip, and the whole E2E suite
with the worker on. The numbers are in §13.

**The switch** (decided by those numbers, §13): `previewWorkerEnabled()`
returns true unless the build sets `B2C_PREVIEW_WORKER=0`. The variable is
kept as an **off switch** for one reason: the A/B job and a developer need
to build the main-thread path from the same tree to compare (§2.4's
measurements, and any later regression hunt). It is not a setting, no
shipped build sets it, and the main-thread path it selects is the same code
as the fallback, which the worker's failure takes at run time (three failed
starts, a webview that cannot post a `Module`, the page's compile failing:
w5-worker.md §3.4). _Alternative:_ remove the variable altogether. Rejected
for now: the fallback stays tested by unit tests either way, but measuring
it in the app would then need a code change; §9 asks the owner.

### 2.5 E2E

* `preview-worker.e2e.ts` gains: `previewThread()` is `worker` after the
  first update on both systems; an edit previews; after
  `stopPreviewWorker()` the next edit previews on a new worker; a variable
  made after the worker started is in a _set_ block's menu (the M2 exit
  flow's failure of w5-worker.md §4); each M3-D1 assignment project opens
  and its preview equals its golden C++.
* Every other flow runs with the worker on, since it is the default: the
  whole Linux suite at the three sizes locally, both systems in CI.
* The nightly security case (`e2e/specs/security/worker.e2e.ts`, worker.md
  §8) is the coordinator's with the spec's 08 §8.13 (§12).

## 3. The toolbox flyout's cost per block

Next steps §3.8: showing the continuous flyout draws every catalog block;
with 47 blocks the refresh took 204–216 ms against 145–149 ms with 26
(WebKitGTK, debug build), and package T made the refresh after a new
variable 45–52 ms by keeping unchanged blocks, but the first showing draws
all 86.

**Method.** Profile first, then change: the first showing and the refresh
after a declaration, in the app under WebKitGTK (the `declare-variable`
benchmarks and `b2c:toolbox-refresh`, plus a per-phase breakdown taken with
User Timing marks in an E2E build: building the dynamic categories, each
inflater's `load`, `layout_`, `reflowInternal_`, the disposal and sweep) and
in happy-dom (`continuous.refresh.test.ts`). The candidates, each kept only
if the profile shows its cost and the benchmark moves:

1. **The fingerprint** (`inflater.ts`): every kept block is serialised with
   `Blockly.serialization.blocks.save` when it is built and again when it is
   disposed of, on every showing, to tell whether the user changed it in the
   flyout. A flag set by the flyout workspace's own change events (a field
   edited, a block moved or attached inside the flyout) would replace the
   second serialisation; the first is not needed when the flag starts clear.
   Reuse rule kept: a changed block is never kept.
2. **Bounds read twice per item** (`layout_` and `reflowInternal_`
   both call `getBoundingRectangle()` on every item, which reads the block's
   transform up the SVG tree): measured once per item per showing.
3. **The item keys** (`JSON.stringify(info)` per block item, and the label
   keys): kept unless the profile shows them.
4. **Lazy drawing of the first showing**: drawing only the categories in or
   near the view, the others as placeholders of their measured height.
   Considered last: it changes how the continuous flyout scrolls and selects
   categories (04 §4.2), so only if 1–3 do not bring the first showing near
   the refresh's cost.

Every change keeps package T's rules (next steps §3.8; the module comment of
`inflater.ts`): everything a toolbox block shows comes from its toolbox item,
the project's dependencies are in `dynamicContentsKey`, and a block that
declares something or that the user changed in the flyout is built again.
`reuse.test.ts` and `continuous.refresh.test.ts` keep passing unchanged; new
tests cover each change (a block edited in the flyout is not kept after the
change event path replaces the fingerprint, a block moved inside the flyout
is not kept, the bounds are read once).

## 4. M3-5: the compact source map and a C++ panel that pauses

### 4.1 The compact source map

At 1,000 blocks the preview's answer is about 466,000 characters, most of it
the source map (one object per range: two positions, the module, the block,
the part), and parsing it is the largest part of the main thread's receipt
with the worker (w5-worker.md §2.2). The compact form is the preview's
alone: `b2c_ir::SourceMap` (the IR's, which the build and `cut.rs` read) is
unchanged, and so is every file the build writes.

```ts
/** PreviewResult.sourceMap, version 2 (packages/b2c-core-wasm/src/api.ts). */
export interface CompactSourceMap {
  readonly version: 2;
  /** Module IDs, block IDs and parts used by the ranges, each once, in first use. */
  readonly modules: readonly string[];
  readonly blocks: readonly string[];
  readonly parts: readonly Part[];
  /** Per generated file, in the files' order: 7 numbers per range, in the IR's order:
   *  start line, start column, end line, end column, module, block, part (indexes). */
  readonly files: readonly { readonly path: string; readonly ranges: readonly number[] }[];
}
```

* Encoded in `crates/b2c-core-wasm/src/wire.rs` from the IR's map;
  `CoreVersion.sourceMapVersion` becomes 2 for the preview (the core's
  `version()`; the page and the worker must agree, which the version check
  already covers).
* Decoded nowhere into objects: `buildSourceMapIndex`
  (`apps/desktop/src/panels/code/sourcemap.ts`) reads the arrays directly.
  `expandSourceMap(compact): SourceMap` exists for tests and any reader that
  wants the old shape.
* `cut.rs` keeps reading the IR's map. worker.md §5's `reuse_json` passes the
  applied preview's map back to Rust; it then carries the compact form, which
  `cut.rs` would read through a decoder in `b2c-core-wasm` (§10.2).
* Tests: Rust, for every example: the compact map expands to exactly the
  IR's map (`wire.rs` test with a decoder in the test); TypeScript:
  `sourcemap.test.ts` gives the same answers for both forms (every block's
  ranges, every part, `blockAt` at every offset of the guessing game's
  file); malformed compact maps (a range count not a multiple of 7, an index
  out of range) give an empty index, never a throw. The golden C++ and the
  diagnostics cannot change (the generator is untouched); `examples.rs` and
  the golden tests run unchanged.

The project format, the IPC contract and the clipboard format do not change:
the source map crosses only from the core to the page.

### 4.2 A C++ panel that pauses while hidden

The C++ panel stays mounted when its dock is folded or another tab is shown
(`hidden`, `EditorLayout.tsx`). Today it still builds the source map index
and replaces the CodeMirror document on every preview. `CodePanel` watches
its own element with an `IntersectionObserver` (present in both webviews;
absent in happy-dom, where the panel counts as shown): while it is not
shown, a new preview, diagnostics or highlight only mark it stale; when it is
shown again, it applies the latest once (one `b2c:code-panel` span). The
index is built lazily, on the first use after a new preview. Nothing visible
changes: the panel shows the latest code whenever it can be seen. Tests: the
panel hidden through three previews and shown again shows the third, with
one CodeMirror transaction; a highlight set while hidden is scrolled to when
shown.

## 5. M3-4: lowering parked stacks (proposal)

ADR-0012 parks loose stacks: they are never generated, their declarations
declare nothing, and every block in them answers the scope query with its
module's parked scope. The M2 polish plan's M3-4 lowers them **in a
sandbox** so that a parked `create x` is visible below it in its stack and
the stack's reporters are typed, without changing the program: the scopes
checkpoint and roll back around each parked stack, the lowered statements go
to a side table that codegen never reads, and the diagnostics of the sandbox
are dropped (`ItemCtx::Parked`).

This is `crates/b2c-lang`'s lowering (`lower/mod.rs`, `lower/stmt.rs`,
`scope.rs`, `query.rs`'s `record_unlowered`), which the coordinator owns and
is changing for W3's second part, so §10.1 gives the exact proposal and its
tests. The scope index needs nothing more for it: the sandbox records
positions on the same trail, so `scope_index_parts()` carries them as it
carries every other position.

## 6. M3-9: the small-program budgets measured and gated

* **Gated (regressions, 10 %, as every gated metric):**
  `webview.guessing-game.edit.max-task`,
  `webview.guessing-game.select.max-task`,
  `webview.declare-variable.toolbox-refresh` and
  `webview.declare-variable-1000.toolbox-refresh` (`gate: true` in the
  benchmarks that write them). `bench-compare.py` compares a newly gated
  metric only once it has five baselines, so gating them now fails nothing
  until the nightly has run five times.
* **The budget (N12: no task over 50 ms while a program of up to 200 blocks
  is edited).** The guessing-game benchmark fails when the median of a
  metric's samples is over 50 ms (`webview.guessing-game.edit.max-task`
  and `.select.max-task`), as an absolute check beside the regression gate.
  Measured on the runners (next steps §3.8: Windows 8.15–9.15 ms for the
  edit) and locally (§13), so the budget has a margin of about five.
* The A/B job (`bench-ab.yml`) is the coordinator's to dispatch; §13 says
  what to run.

## 7. M3-6: `pnpm doctor`

`pnpm doctor` (a root script running `node tools/doctor.mjs`, no
dependency) checks the machine against `BUILDING.md`'s pins and says, for
each line, OK, a warning or a problem with the command that fixes it:

| Check | Source of truth | Problem when |
| --- | --- | --- |
| Node.js | `package.json` `engines.node` | older |
| pnpm | `packageManager` | another version runs the script |
| Rust | `rust-toolchain.toml` | `rustc --version` differs (rustup missing: problem) |
| wasm32 target | `rust-toolchain.toml` `targets` | `rustup target list --installed` lacks it |
| wasm-bindgen CLI | `Cargo.lock` | missing or another version |
| wasm-opt | `BUILDING.md` minimum | missing: a warning (optional) |
| WebKitGTK 4.1 (Linux) | `BUILDING.md` step 1 | `pkg-config --exists webkit2gtk-4.1` fails |
| g++ | `MIN_GCC_MAJOR` of `b2c-toolchain` | missing or older: a warning (only to run programs) |
| tauri-driver | `TAURI_DRIVER_VERSION` in `desktop.yml` | missing: a note; another version: a warning (contributors only) |
| The WebAssembly core | `packages/b2c-core-wasm/pkg` | missing, or older than a source of `crates/` it is built from: a warning naming `pnpm wasm:build` |
| `node_modules` | `pnpm-lock.yaml` | missing, or older than the lockfile: names `pnpm install --frozen-lockfile` |

It exits 1 when there is a problem, 0 otherwise, and runs on Linux and
Windows (paths through `node:path`, commands through `spawnSync` without a
shell, `.exe` on Windows). It reads files and runs version commands only;
it changes nothing and sends nothing anywhere. Tests:
`tools/doctor.test.mjs` with `node --test` (each check's parsing on sample
outputs, the exit code), run from the root `test:tools` script and CI's
frontend job is not changed (§10.5 asks the coordinator to add it there, or
it runs in `pnpm -r test` through a workspace package; see §9).
`BUILDING.md`'s _Troubleshooting_ gains one line pointing to it (an additive
line in a file the coordinator does not own; `check-build-docs.py` is run).

## 8. M3-7: a weekly job that follows `BUILDING.md`

`.github/workflows/building.yml` (new; no existing job changes): weekly
(Mondays, a minute unlike `weekly.yml`'s) and on `workflow_dispatch`, a
matrix of `debian:12` and `debian:13` containers on `ubuntu-24.04`. Each
step extracts the shell blocks of `BUILDING.md`'s _Linux: Debian and
Ubuntu_ section with `tools/building-steps.py` (new) and runs them in order,
as written, with two substitutions the script makes and prints: `sudo` is
dropped (the container runs as root and has no `sudo`), and the
`git clone … && cd` of step 4 is replaced by the checkout already there. It
then starts `./target/release/blocks2cpp-desktop` under `xvfb-run` for 20 s
and fails if the app exits before that (as `desktop.yml`'s start check
does). Permissions `contents: read`; actions pinned by commit as in the
other workflows; no secrets, no cache (a cache would hide a missing step).
`tools/building-steps.py` has a test (`tools/test_building_steps.py`): the
blocks it finds in the current `BUILDING.md`, the substitutions, and a
section that is missing fails.

## 9. Decisions for the owner

1. **The worker is the default, with `B2C_PREVIEW_WORKER=0` kept as an
   off switch for measurements** (§2.4). _Recommendation:_ yes; remove the
   switch with M5's per-block cache, when the main-thread path is only the
   fallback.
2. **The editor's scope query answers from the host's applied index**
   (§2.2), until the coordinator moves it into the store (§10.3).
   _Recommendation:_ yes.
3. **The clipboard arguments wait for their measurement** (§1, §10.2).
   _Recommendation:_ yes; a copy at 1,000 blocks costs one analysis on the
   main thread (about the preview's `update()` less the generation), within
   N12's 100 ms on the runners.
4. **The compact source map is the preview's only** (§4.1): the IR's and the
   build's maps keep version 1. _Recommendation:_ yes.
5. **M3-4 is a proposal for the coordinator** (§5, §10.1).
   _Recommendation:_ yes; it is in `crates/b2c-lang`, which W3's second part
   is changing.
6. **The guessing-game budget is checked in the benchmark itself** (§6),
   not by `bench-compare.py`. _Recommendation:_ yes; it is one number per
   metric, and the benchmark already has it.
7. **The weekly `BUILDING.md` job is a new workflow** (§8), not a job of
   `weekly.yml`. _Recommendation:_ yes; it fails for other reasons (the
   distributions' mirrors) and is easier to read and re-run on its own.

## 10. Needs from the coordinator, and from W4

### 10.1 M3-4, lowering parked stacks (`crates/b2c-lang`)

Proposed for the coordinator's next change of the lowering (after W3's
second part, which edits the same functions). References are to `8a3d8ec`.

* **Where.** `Lowerer::program` (`lower/mod.rs:430`) meets a parked head in
  the catch-all arm of its item loop (`:483-486`, `report_parked`) and never
  visits `head.stack`; `run` (`:64`) then calls
  `index.record_unlowered(document)` (`:87`), which records every parked
  block with the module's parked point (`query.rs:176-190`). The sandbox runs
  in `run`, after `program()` and before `record_unlowered`, so its positions
  win (`record_block` keeps the first, `query.rs:111`).
* **`scope.rs`: seeding and a checkpoint.** `Scopes` has no way to make the
  names of a trail node visible again (`by_name`, `by_sym` are built only by
  `declare`). Add `Scopes::seed(&mut self, node: Option<NodeId>, key:
  ListKey)`: `reset()`, push one frame whose `entry` is `node`, set `head` to
  `node`, and fill `by_name`/`by_sym` by walking the trail from `node`
  (the first name seen wins, as `ScopeIndex::visible` hides). The sandbox's
  own frames are then pushed and popped as usual; `reset()` afterwards.
  Trail nodes made in the sandbox stay in the trail (recorded positions
  point at them).
* **`ItemCtx::Parked`** (`mod.rs:180-190`), set through `begin_item`
  (`:638`). The exhaustive matches decide: `program.exit` (`stmt.rs:136`)
  counts as not in `main`; a `return` (`stmt.rs:616-655`) and the expected
  return type (`typed.rs:235-237`) are as in `main` (no value); its
  diagnostics are dropped anyway (below).
* **The sandbox**, per module in document order, per parked head (the arm
  above) and each block stacked under a head or a definition, as one list:
  `begin_item(ItemCtx::Parked)`; `scopes.seed(index.parked_point(module).node,
  ListKey::new(head, "#parked"))`; `let saved = std::mem::take(&mut
  self.diags)` (and `too_deep_reported`); lower the blocks as statements
  (the same `lower_list` a body uses), recording positions as it does; drop
  the lowered statements; `self.diags = saved`; `scopes.reset()`.
* **Symbols.** Parked declarations go through `declare_local` →
  `insert_symbol` (`mod.rs:1217`) into a side table
  `Lowerer.parked_symbols: SymbolTable` instead of `self.symbols`
  (`insert_symbol` takes the table from `ItemCtx`), kept in
  `Analysis.parked_symbols`. `program.symbols` is unchanged, so codegen, the
  checks and the build see no difference. `query.rs`'s `infos`,
  `with_members`, `scope_after` and `ScopeIndexParts::{members, created}`
  look a symbol up in `program.symbols`, then in `parked_symbols`;
  `symbol_infos` (the update's `symbols`) lists both, so the scope index's
  symbol indexes cover them. `slot_types` and `constants` written in the
  sandbox are dropped with its statements, or kept for `block_types` (next).
  Parked declarations are `DeclStatus::Detached` today (`collect.rs:176`)
  and references to them report unavailable (`resolve.rs:253-275`); in the
  sandbox those reports land in the dropped `Diags`.
* **`block_types`** (`query.rs:576`): also walks the sandbox's lowered
  statements, kept in `Analysis.parked_stmts` for that only (codegen never
  reads `Analysis`, only `program`), through `record_types` (`:614`).
* **`record_unlowered`** then records only what the sandbox did not reach (a
  stack too damaged to lower, a block nested too deeply).
* **Tests** (the M2 polish plan's four, `tests/parked.rs` beside the
  existing nine): a parked `create x` is visible below it in its stack and
  not to `main` or another stack; `block_types` has the parked reporters;
  parked code adds no diagnostic but B2C-W0505 and the C++ of every example,
  golden test and `parked_stacks_never_change_the_generated_cpp`
  (`b2c-build/src/frontend.rs:574`) is unchanged; a text value no longer
  snaps into a parked `int` _set_ (an E2E check, the editor's). This
  branch's `tests/scope_index.rs` passes unchanged: the sandbox's positions
  are trail nodes like any other.

### 10.2 The clipboard arguments (W4's `editor/clipboard/`, this branch's core)

worker.md §5 as written, with `reuse_json`'s `sourceMap` in the compact form
of §4.1. To be done if `webview.copy-1000.max-task`, `paste-1000` or
`duplicate-1000` (to be added to `apps/desktop/e2e/bench/`, this branch's,
in the integration that adds the arguments) is over N12's 100 ms with the
worker on.

### 10.3 The index in the store (`app/store`, `services/symbols.ts`)

`AnalysisState.scope: ScopeIndex | null`, set by the pipeline with the
preview; `analysisScope` reads `{ scope, preview }` and falls back to the
core when `scope` is `null`. The host's view (§2.2) is then removed.

### 10.4 The hold's one line (`sync/session.ts`, W4's)

In `EditorSession`'s constructor, the pipeline's options gain
`isBusy: () => isDragging(this.workspace)`.

### 10.5 `pnpm doctor`'s test in CI

`ci.yml`'s or `desktop.yml`'s frontend job runs `node --test tools/` (or this
branch's `pnpm test:tools`), if the coordinator wants it in CI; this branch
does not change existing jobs.

### 10.6 Coordination with W3

The coordinator's rows of `w3-modules.md` §3.3 (on the coordinating branch) ("Shared files
and the W4/W5 boundary"), each answered:

1. **`packages/b2c-core-wasm/src/bdm.ts`, `usingNamespaceStd` in the
   analysis options: agree.** This branch does not change `bdm.ts` or the
   preview options (`PreviewOptions` stays `{ indentWidth }`; the worker
   protocol posts the options as the core's JSON, `previewOptionsJson`, so a
   new optional key crosses once `previewOptionsJson` passes it).
2. **`crates/b2c-core-wasm/src/clipboard.rs`: agree.** This branch does not
   change `clipboard.rs` (the clipboard arguments are out, §1, §10.2). When
   they come, `reuse_json` and `visible_json` are read in `session.rs`
   (`analysis_for`, `paste_prepare`) before `clipboard::rebind`, so W3's
   precedence, qualified names and kept IDs stay in `clipboard.rs` where they
   are.
3. **`facade.rs`, `benches/pipeline/`, `tests/common/mod.rs`: agree.** The
   only change to `facade.rs` is the source map's serialisation
   (`#[serde(serialize_with)]` on `Preview::source_map`, §4.1) and the
   version `version()` reports; the options pass through untouched.
   `tests/common/mod.rs` is unchanged. The new criterion case
   `scope-index` runs on the same prepared document as `preview`, whatever
   its shape, so an 8-module case can be added beside it.
4. **M3-5's compact source map: agree.** Each compact file keeps the IR's
   optional `module` (§4.1, tested by the round trip). `shared` is a key of
   the symbols (`SymbolInfo`), not of the source map; the update's `symbols`
   are unchanged by this branch.
5. **The scope index and other modules' shared symbols: agree.** The index
   names a symbol by its index into the update's `symbols`, so a symbol's
   module, namespace and `shared` come with its `SymbolInfo` and need no new
   key. If W3's package F makes other modules' shared symbols visible through
   `ScopeIndex::visible`'s function lists or a new list per module,
   `ScopeIndexParts` gains one method for it (callers of the existing
   methods do not change), `scope_index.rs` one key, and the lookup one step;
   the Rust and TypeScript lookup tests compare with `scope_at` for every
   block, so a missing step fails them at once.

## 11. Proposed corrections to worker.md and w5-worker.md

1. worker.md §4: the format gains `members` and `partVariables` (§2.1); the
   index's `trail` symbol is -1 also for a symbol the table does not have.
2. worker.md §4 "Store" and §7: the index is applied by the host until the
   coordinator moves it into the store (§2.2, §10.3).
3. worker.md §6 "The hold": no `release()`; the pipeline re-checks
   `isBusy()` every animation frame while it holds an answer (§2.3).
4. worker.md §6: `invalidate()` bumps an epoch of its own; the sequence
   number then only orders runs.
5. w5-worker.md §4: the flag is inverted rather than removed (§2.4).

## 12. Specification sections to change (the coordinator's docs step)

Those of worker.md §11, plus: 06 §6.9 (the preview's compact source map,
version 2, beside the IR's version 1); 04 §4.3 (the C++ panel pauses while
hidden); 09 §9.2 (the gated guessing-game and toolbox metrics and the 50 ms
budget; the spans `b2c:preview:index`, `:receive`, `:wait`); 09 §9.3
(`building.yml`); `BUILDING.md` and CONTRIBUTING (`pnpm doctor`); the
benchmarks' README (this branch's, updated here).

## 13. Measurements

Filled in as the work lands; every performance change gives before and after
numbers from the repository's benchmarks. The A/B job to dispatch on this
branch against its base is listed in the Status.
