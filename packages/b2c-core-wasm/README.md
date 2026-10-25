# @blocks2cpp/b2c-core-wasm

The Rust compiler core, [`crates/b2c-core-wasm`](../../crates/b2c-core-wasm/src/lib.rs),
compiled to WebAssembly for the editor, with its TypeScript loader and types. The editor runs the
same loading, canonical saving and code generation as the backend and the CLI, so the live C++
preview cannot drift from what is built
([ADR-0003](../../docs/adr/0003-rust-core-native-and-wasm.md),
[spec 06 §6.13](../../docs/spec/06-compiler-pipeline.md#613-performance-in-the-editor)).

## API

```ts
import { initCore, randomSeedHex } from '@blocks2cpp/b2c-core-wasm';

const core = await initCore();
const loaded = core.load(bytes); // untrusted bytes: every limit of 05 §5.6
if (loaded.ok) {
  const json = JSON.stringify(loaded.document);
  const update = core.update(json, { indentWidth: 4 }); // the preview, with textHash
  const visible = core.symbolsInScope('b005', null); // from that update's analysis
  const saved = core.canonical(json); // { text, hash, textHash }: only to save or build
}
```

| Function                                            | Result                                                                                                                                                                                              |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `version()`                                         | `{app, catalog, formatVersion, sourceMapVersion}`                                                                                                                                                   |
| `load(bytes)`                                       | `{ok: true, document, diagnostics}` (migrated, blocks upgraded (05 §5.7), not resolved, canonical key order; a `B2C-E0603` for each block that could not be upgraded) or `{ok: false, diagnostics}` |
| `canonical(documentJson)`                           | `{ok: true, text, hash, textHash, diagnostics: []}`: what a save writes (05 §5.2), the content hash (05 §5.11) and the SHA-256 of the text                                                          |
| `update(documentJson, { indentWidth })`             | `{stage, diagnostics, files, sourceMap, buildable, placeholders, contentHash, blockTypes, symbols, slotTypes, textHash}`: the preview and the hash of the canonical text, from one load             |
| `symbolsInScope(blockId, input)`                    | `SymbolInfo[]` or `null`: what a block may refer to (06 §6.5), from the **last update's** analysis                                                                                                  |
| `conversionTable()`                                 | `[{from, to, conversion}]` for every pair of static types: the analyser's rule (06 §6.6)                                                                                                            |
| `clipboardMake(documentJson, blockIds)`             | `{ok: true, payload, text?, diagnostics: []}`: the clipboard payload (05 §5.12) and the blocks' C++                                                                                                 |
| `pastePrepare(text, documentJson, target, seedHex)` | `{ok: true, blocks, unresolved, diagnostics}`: a payload's blocks ready to insert at `target`                                                                                                       |

- **Every document that enters the editor goes through `load()` first.** Never build editor state
  from a plain `JSON.parse` of untrusted text, and never merge loaded objects into others with
  `Object.assign` (a `__proto__` key in `x-ext` is kept as data).
- **The preview carries on through errors.** A build stops at the first stage with an error; the
  preview, once the document has loaded, always generates best-effort C++ (with `/* error */`
  placeholders). `stage` is where a build would stop (`load`, `resolve`, `analyze`, or `generate`
  when nothing failed), `buildable` whether it would build, and only a load failure gives no
  files. For an error-free project the files are byte for byte what the backend builds.
- **Diagnostics, files, source maps, static types and symbols** have the `b2c-ir` serde shapes,
  the same as `b2c check --format json` ([docs/reference/cli.md](../../docs/reference/cli.md#json-output)).
  Their TypeScript types come from `@blocks2cpp/ipc-types` (generated from the Rust types) and are
  re-exported here. Messages may quote project text: render them as text, never as HTML.
- `blockTypes` gives the static type of each value block of the program (what the connection
  checker and the output types of `var.get` and `func.call` use), `slotTypes` that of each
  expression slot, by block ID, then input name (ADR-0018), and `symbols` every symbol of the
  program, sorted by name. Types are `TypeJson`: type terms (`"int"`, `{"std::vector": "int"}`,
  `{"ref": "sym_x"}`) and `"error"`; until the switch to the term form
  (docs/plans/m3-contracts/types.md §8.2), M2's seven types keep their M2 names (`"string"`).

### One load per edit

The editor calls `update()` alone after each edit (06 §6.13): it loads and hashes the document
once and returns the preview with `textHash`, the SHA-256 of the document's canonical text. Unlike
the content hash (`contentHash`, which leaves out block positions and the other layout-only keys
of 05 §5.11), `textHash` changes exactly when the file a save would write changes, so the editor
decides whether there are unsaved changes by comparing it with the saved text's `textHash` (from
`canonical()`). It asks `canonical()` for the text itself only when it needs the text: to save,
to build, and to write a recovery snapshot. A document that does not load gives `stage: 'load'`
with the loader's problems, and `null` for both hashes.

### Scope query

`update()` keeps the analysis of the document it previewed, and `symbolsInScope()` answers from it
without running the pipeline again. **Call it only after your own update has finished**, so the
answer is about the document on screen. With `input` null (or a value input) it lists what is
visible at the block; with one of the block's statement inputs (`BODY`, `DO0`, `ELSE`, …), what is
visible at the start of that list, including a loop's counter and a function's parameters. Every
block of the previewed document answers: a disabled block in a list or input the analyser reaches
answers for its position, a block nested in it like it, and a parked block (loose, in a loose
stack, or nested in one) with the parked scope, what is visible at the end of its module's `main`
body. It answers `null` (no answer) before an update has loaded a document, for a block that
document does not have, and for one nested too deeply; `[]` means that nothing is visible there.
An update whose document does not load keeps the analysis of the last one that did, which is
what the editor still shows.
`conversionTable()` lets the connection checker refuse exactly the connections the analyser would
report (`invalid`). The core's table covers every pair of the sample types of the shared type
fixture (`packages/blockly-ext/src/types/fixture.json`); until the switch to the term form, the
export returns the 49 pairs of M2's seven types. The scope query also lists the fields and
enumerators of each struct and enum it lists (milestone M3), each with its `owner`.

### Clipboard

The editor uses the DOM `copy`, `cut` and `paste` events with `application/x-blocks2cpp+json`
(the payload) and `text/plain` (the C++), and an in-memory copy inside the app (05 §5.12).

- `clipboardMake(documentJson, blockIds)` copies blocks, in the given order. A listed block inside
  another listed block is copied once, as part of it; a top-level block keeps its loose `stack`.
  Copies have no canvas position. `refs` names each symbol the blocks use but do not declare:
  variables, parameters and loop counters by their plain name (`score`), functions with the
  global qualifier (`::area`), because milestone M2 has no namespaces. The payload is checked
  with the same loader as a paste before it is returned. `text` is the blocks' C++, cut from the
  last preview when it shows this document (otherwise generated at the last preview's indent
  width), as whole blocks: whole lines without their indentation for statements and
  definitions, the exact expression for value blocks. It is absent when none of the blocks
  produce code (loose or disabled blocks).
- `pastePrepare(text, documentJson, target, seedHex)` loads the payload with the **same parser,
  limits and `B2C-E01xx` codes as a project file** (a payload that is not clipboard data is
  `B2C-E0138`), gives every block and every symbol the blocks declare a fresh ID that the document
  does not use, and binds the other references again by qualified name and kind among the symbols
  visible at `target`. When no visible symbol has the recorded name, a reference whose original
  symbol is visible there with the same kind keeps it, even when it was renamed since the copy
  (so projects that share symbol IDs, such as copies of one example, bind such references
  silently, whatever the symbol is called there). One that finds no match (or several) stays a
  reference to its original symbol, is listed in `unresolved`, and gets a `B2C-E0201`
  diagnostic naming the original.
  - The `blocks` are ready to insert as they are. On the canvas a copied loose stack stays one
    block with `stack`; in or after a block, its stacked blocks follow the head in `blocks` and
    no block has `stack` (a block inside another one cannot have one, `B2C-E0139`).
  - `target` is `{module, block, input}`: `block: null` is the module's canvas, where pasted
    blocks are parked and see the parked scope (the end of `main`'s body); a `block` with an
    `input` is the start of that statement list (or that value input); a `block` with
    `input: null` is the place **directly after** the block in its list, where a variable the
    block itself creates is visible. A disabled statement in a list the analyser reaches has the
    scope of its position, a block inside a disabled statement has that statement's scope, and a
    paste into or after a parked block has the parked scope (05 §5.12).
  - `seedHex` is 64 hex digits of fresh randomness; use `randomSeedHex()` (from
    `crypto.getRandomValues`) for every paste or duplicate. The core itself uses no randomness, so
    the same seed always gives the same IDs.

### Errors

A problem in the project is never an exception; it is a diagnostic. The wrapper throws:

- `CoreError` (with `kind`) when the core refuses the arguments (`invalidOptions` for preview
  options; `invalidArguments` for a malformed block ID list, paste target or seed, or one naming a
  module or block the document does not have), fails in a way that is a bug (`encode`,
  `internal`), returns something unexpected (`protocol`), or cannot be started (`init`). The
  instance keeps working.
- `CoreTrap` when the WebAssembly instance stops: a trap (a Rust panic aborts with
  `RuntimeError: unreachable`), running out of memory, or any other exception from inside a call.
  The instance is never called again. `initCore()` notices and starts a fresh instance from the
  already compiled module; `resetCore()` forces that. A trap is always a bug in Blocks2Cpp.

## How the module is loaded

The CSP stays `script-src 'self' 'wasm-unsafe-eval'` and `connect-src` forbids fetching the
module, so the build embeds the optimised module as base64 in its own chunk (`#pkg-bytes`), which
the bundler emits as a separate file served from `'self'` and imports lazily. `initCore()` decodes
it (strict base64, no `atob` string), compiles it once with `WebAssembly.compile` and instantiates
it. It is never a `data:` URL and never fetched. The wasm-bindgen glue is wrapped in
`createGlue()` by the build, so every instance gets its own glue state: the generated glue keeps
one instance in module-level variables and could not be re-initialised after a trap.

`#glue` and `#pkg-bytes` are
[subpath imports](https://nodejs.org/docs/latest-v22.x/api/packages.html#subpath-imports) of this
package. Their `types` condition points at the committed `src/glue.d.ts` and `src/pkg-bytes.d.ts`,
so type checking works without a build.

## Build

The build needs the pinned Rust toolchain (with `wasm32-unknown-unknown`), the wasm-bindgen CLI
of the version pinned in `Cargo.lock` and, optionally, binaryen's `wasm-opt`.
[`BUILDING.md`](../../BUILDING.md) names their versions and sets them up on each system
([troubleshooting](../../BUILDING.md#the-webassembly-core-does-not-build)). From the repository
root:

```sh
pnpm wasm:build     # pnpm --filter @blocks2cpp/b2c-core-wasm build; B2C_SKIP_WASM_OPT=1 without wasm-opt
pnpm app:build      # the core, then the release build of the app
```

[`scripts/build.mjs`](scripts/build.mjs) first checks both tools and names every one that is
missing or unsuitable, with how to get it. It then runs `cargo build --profile wasm-release
--target wasm32-unknown-unknown -p b2c-core-wasm`, `wasm-bindgen --target web`, `wasm-opt -Oz` and
writes `pkg/` (not committed): `glue.js`, `pkg-bytes.js`, `b2c_core_wasm_bg.wasm`, `sizes.json`
and, last, `build-info.json`. It stops when the CLI version differs from `Cargo.lock`, when the
crate's exports differ from `src/glue.d.ts` (change `EXPECTED_EXPORTS` in the script with them),
or when the module is over the **size budget of 2,000,000 bytes after `wasm-opt -Oz`** (the gzip
size is reported only).

| Variable            | Effect                                                                                 |
| ------------------- | -------------------------------------------------------------------------------------- |
| `WASM_BINDGEN`      | The wasm-bindgen CLI to run (default: `wasm-bindgen` on `PATH`)                        |
| `WASM_OPT`          | binaryen's `wasm-opt` (default: `wasm-opt` on `PATH`)                                  |
| `B2C_SKIP_WASM_OPT` | `1` skips `wasm-opt` for local builds; the budget is checked on the unoptimised module |
| `CARGO`             | The cargo to run                                                                       |

### An out-of-date core stops the app's build

`pkg/` is not rebuilt by itself after a `git pull`, and an app built with an old core shows C++
from the old compiler. So `pkg/build-info.json` records a SHA-256 of the core's inputs, taken
before cargo reads them ([`scripts/build-info.mjs`](scripts/build-info.mjs) lists them): the root
`Cargo.toml`, the `Cargo.lock` entries of `b2c-core-wasm` and everything it depends on,
`rust-toolchain.toml`, `catalog/`, the `Cargo.toml` and `src/` of each crate compiled into the
core (`b2c-core-wasm`, `b2c-ir`, `b2c-model`, `b2c-catalog`, `b2c-lang`, `b2c-codegen`) and the two
build scripts. The desktop app's Vite plugin
([`apps/desktop/vite/wasmCore.ts`](../../apps/desktop/vite/wasmCore.ts)) computes it again: a
production build of the editor (`tauri build`, `pnpm --filter @blocks2cpp/desktop build`, also the
E2E build) stops when the core is missing, older than its inputs (naming the ones that changed) or
has no `build-info.json`; the dev server and Vitest only warn. A development build of the app also
compares the core's catalog version with the backend's when it starts, and shows a blocking error
when they differ ([spec 02 §2.5.7](../../docs/spec/02-architecture.md#257-versioning)).

## Checks

```sh
pnpm --filter @blocks2cpp/b2c-core-wasm typecheck
pnpm --filter @blocks2cpp/b2c-core-wasm lint
pnpm --filter @blocks2cpp/b2c-core-wasm format:check
pnpm --filter @blocks2cpp/b2c-core-wasm test            # or test:coverage
cargo test --locked -p b2c-core-wasm                     # the same functions, natively
```

`test/build-info.test.ts` checks the fingerprint: stable, changed by every input group and by
nothing else, and an input list that names every crate and `include_str!` file the core compiles.
The Vitest suite under Node runs every example (load, canonical save byte for byte, preview equal
to `tests/golden/<name>/main.cpp`, `update()`'s hashes equal to `canonical()`'s), the
malicious-project suite
(`tests/security/projects/README.md`, Loader column), and the scope query, block types,
conversion table and clipboard (including the malicious-clipboard suite,
`tests/security/clipboard/README.md`) through the built module. Without a build
those tests are skipped with a warning; with `B2C_REQUIRE_WASM=1` (for CI jobs that build first) a
missing build fails them instead. The loader and wrapper tests use fakes and run without a build.
