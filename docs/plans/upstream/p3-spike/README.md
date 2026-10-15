# P-3 render-management spike: tests and tools

The files the P-3 spike measured with (plan, P-3's status and D6's follow-up),
kept so that M3-2 can land the patch with them and reproduce its evidence. They
are not part of the app's suites: each is written for the place it ran from,
and is copied there to run. The patch itself is
[`../blockly-render-management.bundle.patch`](../blockly-render-management.bundle.patch)
(the npm bundle) and
[`../blockly-render-management.patch`](../blockly-render-management.patch)
(Blockly's sources); the report is
[`../blockly-render-management.md`](../blockly-render-management.md).

| File | What it is | Runs from |
| ---- | ---------- | --------- |
| [`renderSpike.test.ts`](renderSpike.test.ts) | Vitest: the render count when a print is appended to a 500-statement chain (fails without the patch with `B2C_SPIKE_PATCHED` set: 503 drawn against at most 4), the placement property test (40 random edits each with Zelos and Geras against `workspace.render()`), and `getDescendants`' order and timing | `apps/desktop/src/editor/sync/` |
| [`patch_bundle.py`](patch_bundle.py) | Applies the patch to Blockly 12.5.1's `blockly_compressed.js` (checks the file's SHA-256 first) | anywhere |
| [`instrument_bundle.py`](instrument_bundle.py) | Adds timing probes (`globalThis.__p3`) to a stock or patched bundle, for a diagnostic build only | anywhere |
| [`p3probe.bench.e2e.ts`](p3probe.bench.e2e.ts) | The E2E probe that reads those timings during the edit and selection benchmarks' interactions at 1,000 blocks | `apps/desktop/e2e/bench/` |

## Running the render count and the property test

From the repository root, with the WebAssembly core built:

```sh
cp docs/plans/upstream/p3-spike/renderSpike.test.ts apps/desktop/src/editor/sync/
B2C_SPIKE_OUT=/tmp/p3-stock.txt \
  pnpm --filter @blocks2cpp/desktop exec vitest run src/editor/sync/renderSpike.test.ts \
  --hookTimeout=600000
```

The counts and timings go to the file `B2C_SPIKE_OUT` names. Loading the
500-statement chain takes about 8 minutes in happy-dom on either side, and
without the patch removing it again takes longer than Vitest's default 10 s
hook timeout, hence `--hookTimeout`. Without the patch the render count draws
every block of the chain (503 for the append, 501 for the deletion), and the
property test passes (a full render is its own reference). Then patch the
installed bundle (pnpm's store is shared, so work on a copy of
`node_modules` or restore the file afterwards), and run again with
`B2C_SPIKE_PATCHED=1`: the render count expects at most 4 blocks drawn, and the
property test checks that the patched render places everything as a full one.

```sh
BUNDLE="$(realpath apps/desktop/node_modules/blockly)/blockly_compressed.js"
cp "$BUNDLE" /tmp/blockly_compressed.stock.js
python3 -I docs/plans/upstream/p3-spike/patch_bundle.py /tmp/blockly_compressed.stock.js "$BUNDLE"
B2C_SPIKE_PATCHED=1 B2C_SPIKE_OUT=/tmp/p3-patched.txt \
  pnpm --filter @blocks2cpp/desktop exec vitest run src/editor/sync/renderSpike.test.ts \
  --hookTimeout=600000
cp /tmp/blockly_compressed.stock.js "$BUNDLE"
rm apps/desktop/src/editor/sync/renderSpike.test.ts
```

`patch_bundle.py` gives the same file as applying the bundle patch with
`patch -p1` (SHA-256 `e7e0db89…`, as the patch's header says); the
measurements in the plan were made with it.

## Running the probe

Build a diagnostic E2E app whose bundle went through
`instrument_bundle.py` (after `patch_bundle.py` for the patched side), copy
`p3probe.bench.e2e.ts` to `apps/desktop/e2e/bench/`, and run it as the
benchmarks run (`apps/desktop/e2e/bench/README.md`) with `B2C_P3_OUT` set to
a file and `B2C_P3_LABEL` to `stock` or `patched`. Each line it writes is one
edit, deletion or click with its timings.
