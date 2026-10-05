# @blocks2cpp/b2c-core-wasm

The Rust compiler core, [`crates/b2c-core-wasm`](../../crates/b2c-core-wasm/src/lib.rs),
compiled to WebAssembly for the editor, with its TypeScript loader and types. The editor runs the
same loading, canonical saving and code generation as the backend and the CLI, so the live C++
preview cannot drift from what is built
([ADR-0003](../../docs/adr/0003-rust-core-native-and-wasm.md),
[spec 06 §6.13](../../docs/spec/06-compiler-pipeline.md#613-performance-in-the-editor)).

## API

```ts
import { initCore } from '@blocks2cpp/b2c-core-wasm';

const core = await initCore();
const loaded = core.load(bytes); // untrusted bytes: every limit of 05 §5.6
if (loaded.ok) {
  const preview = core.preview(JSON.stringify(loaded.document), { indentWidth: 4 });
  const saved = core.canonical(JSON.stringify(loaded.document)); // { text, hash }
}
```

| Function                                 | Result                                                                                                              |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `version()`                              | `{app, catalog, formatVersion, sourceMapVersion}`                                                                   |
| `load(bytes)`                            | `{ok: true, document, diagnostics: []}` (migrated, not resolved, canonical key order) or `{ok: false, diagnostics}` |
| `canonical(documentJson)`                | `{ok: true, text, hash, diagnostics: []}`: what a save writes (05 §5.2) and the content hash (05 §5.11)             |
| `preview(documentJson, { indentWidth })` | `{stage, diagnostics, files, sourceMap, buildable, placeholders, contentHash, blockTypes, symbols}`                 |

- **Every document that enters the editor goes through `load()` first.** Never build editor state
  from a plain `JSON.parse` of untrusted text, and never merge loaded objects into others with
  `Object.assign` (a `__proto__` key in `x-ext` is kept as data).
- **The preview carries on through errors.** A build stops at the first stage with an error; the
  preview, once the document has loaded, always generates best-effort C++ (with `/* error */`
  placeholders). `stage` is where a build would stop (`load`, `resolve`, `analyze`, or `generate`
  when nothing failed), `buildable` whether it would build, and only a load failure gives no
  files. For an error-free project the files are byte for byte what the backend builds.
- **Diagnostics, files and source maps** have the `b2c-ir` serde shapes, the same as
  `b2c check --format json` ([docs/reference/cli.md](../../docs/reference/cli.md#json-output)).
  Messages may quote project text: render them as text, never as HTML.
- `blockTypes` and `symbols` are empty until the scope query arrives (milestone M2, wave 2).

### Errors

A problem in the project is never an exception; it is a diagnostic. The wrapper throws:

- `CoreError` (with `kind`) when the core refuses the arguments (`invalidOptions`), returns
  something unexpected (`encode`, `protocol`), or cannot be started (`init`). The instance keeps
  working.
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

`#glue` and `#pkg-bytes` are [subpath imports](https://nodejs.org/api/packages.html#subpath-imports)
of this package. Their `types` condition points at the committed `src/glue.d.ts` and
`src/pkg-bytes.d.ts`, so type checking works without a build.

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
to `tests/golden/<name>/main.cpp`) and the malicious-project suite
(`tests/security/projects/README.md`, Loader column) through the built module. Without a build
those tests are skipped with a warning; with `B2C_REQUIRE_WASM=1` (for CI jobs that build first) a
missing build fails them instead. The loader and wrapper tests use fakes and run without a build.
