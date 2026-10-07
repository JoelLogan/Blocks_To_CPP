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
  const preview = core.preview(json, { indentWidth: 4 });
  const saved = core.canonical(json); // { text, hash }
  const visible = core.symbolsInScope('b005', null); // from that preview's analysis
}
```

| Function                                            | Result                                                                                                              |
| --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `version()`                                         | `{app, catalog, formatVersion, sourceMapVersion}`                                                                   |
| `load(bytes)`                                       | `{ok: true, document, diagnostics: []}` (migrated, not resolved, canonical key order) or `{ok: false, diagnostics}` |
| `canonical(documentJson)`                           | `{ok: true, text, hash, diagnostics: []}`: what a save writes (05 §5.2) and the content hash (05 §5.11)             |
| `preview(documentJson, { indentWidth })`            | `{stage, diagnostics, files, sourceMap, buildable, placeholders, contentHash, blockTypes, symbols}`                 |
| `symbolsInScope(blockId, input)`                    | `SymbolInfo[]`: what a block may refer to (06 §6.5), from the **last preview's** analysis                           |
| `conversionTable()`                                 | `[{from, to, conversion}]` for all 49 pairs of static types: the analyser's rule (06 §6.6)                          |
| `clipboardMake(documentJson, blockIds)`             | `{ok: true, payload, text?, diagnostics: []}`: the clipboard payload (05 §5.12) and the blocks' C++                 |
| `pastePrepare(text, documentJson, target, seedHex)` | `{ok: true, blocks, unresolved, diagnostics}`: a payload's blocks ready to insert at `target`                       |

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
  checker and the output types of `var.get` and `func.call` use), and `symbols` every symbol of
  the program, sorted by name.

### Scope query

`preview()` keeps the analysis of the document it previewed, and `symbolsInScope()` answers from
it without running the pipeline again. **Call it only after your own preview has finished**, so the
answer is about the document on screen. With `input` null (or a value input) it lists what is
visible at the block; with one of the block's statement inputs (`BODY`, `DO0`, `ELSE`, …), what is
visible at the start of that list, including a loop's counter and a function's parameters. It
answers `[]` before a successful preview (a preview whose document does not load forgets the
analysis) and for blocks the analyser does not reach (inside a disabled block, loose, or nested
too deeply); a disabled statement in a list the analyser reaches answers for its position.
`conversionTable()` lets the connection checker refuse exactly the connections the analyser would
report (`invalid`).

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
  - `target` is `{module, block, input}`: `block: null` is the module's canvas (only functions are
    visible); a `block` with an `input` is the start of that statement list (or that value
    input); a `block` with `input: null` is the place **directly after** the block in its list,
    where a variable the block itself creates is visible. A disabled statement in a list the
    analyser reaches has the scope of its position; a block the analyser does not reach (inside a
    disabled block, or loose) sees what the canvas sees.
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

Needs the pinned Rust toolchain (with `wasm32-unknown-unknown`), the wasm-bindgen CLI of the
version pinned in `Cargo.lock` (0.2.129) and binaryen's `wasm-opt`:

```sh
cargo install wasm-bindgen-cli --version 0.2.129 --locked
pnpm --filter @blocks2cpp/b2c-core-wasm build
```

[`scripts/build.mjs`](scripts/build.mjs) runs `cargo build --profile wasm-release --target
wasm32-unknown-unknown -p b2c-core-wasm`, `wasm-bindgen --target web`, `wasm-opt -Oz` and writes
`pkg/` (not committed): `glue.js`, `pkg-bytes.js`, `b2c_core_wasm_bg.wasm` and `sizes.json`. It
stops when the CLI version differs from `Cargo.lock`, when the crate's exports differ from
`src/glue.d.ts` (change `EXPECTED_EXPORTS` in the script with them), or when the module is over
the **size budget of 2,000,000 bytes after `wasm-opt -Oz`** (the gzip size is reported only).

| Variable            | Effect                                                                                 |
| ------------------- | -------------------------------------------------------------------------------------- |
| `WASM_BINDGEN`      | The wasm-bindgen CLI to run (default: `wasm-bindgen` on `PATH`)                        |
| `WASM_OPT`          | binaryen's `wasm-opt` (default: `wasm-opt` on `PATH`)                                  |
| `B2C_SKIP_WASM_OPT` | `1` skips `wasm-opt` for local builds; the budget is checked on the unoptimised module |
| `CARGO`             | The cargo to run                                                                       |

## Checks

```sh
pnpm --filter @blocks2cpp/b2c-core-wasm typecheck
pnpm --filter @blocks2cpp/b2c-core-wasm lint
pnpm --filter @blocks2cpp/b2c-core-wasm format:check
pnpm --filter @blocks2cpp/b2c-core-wasm test            # or test:coverage
cargo test --locked -p b2c-core-wasm                     # the same functions, natively
```

The Vitest suite under Node runs every example (load, canonical save byte for byte, preview equal
to `tests/golden/<name>/main.cpp`), the malicious-project suite
(`tests/security/projects/README.md`, Loader column), and the scope query, block types,
conversion table and clipboard (including the malicious-clipboard suite,
`tests/security/clipboard/README.md`) through the built module. Without a build
those tests are skipped with a warning; with `B2C_REQUIRE_WASM=1` (for CI jobs that build first) a
missing build fails them instead. The loader and wrapper tests use fakes and run without a build.
