# ADR-0010: Delivering the WebAssembly core under the unchanged CSP

* Status: Accepted
* Date: 2026-10-05

## Context

The editor runs the compiler core as WebAssembly for the live preview
([ADR-0003](0003-rust-core-native-and-wasm.md)). The app's Content Security
Policy ([08 §8.8](../spec/08-security.md#88-webview-and-ipc-hardening))
allows scripts only from `'self'`, allows WebAssembly compilation through
`'wasm-unsafe-eval'`, and limits `connect-src` to the IPC endpoints
(`ipc: http://ipc.localhost`).

The usual way to load a module, `fetch()` of the `.wasm` file followed by
`WebAssembly.instantiateStreaming`, is a connection to `'self'`, which
`connect-src` forbids. The module is about 1–2 MB, so how it is loaded also
affects cold start ([N2](../spec/01-overview.md#non-functional-goals)).

## Options considered

1. **Add `'self'` to `connect-src`.** Simple, but it widens the policy for
   every script in the page, and the app origin differs between WebView2 and
   WebKitGTK, so the exception would be platform-specific.
2. **A `data:` URL for the module.** Needs `data:` in `connect-src`, which
   would let any injected script read arbitrary data URLs. Rejected.
3. **Fetch the bytes over IPC** from the backend. Adds a command that copies
   the module on every start, and the bytes then depend on the backend.
4. **Embed the bytes in a JavaScript module (chosen).** The build writes the
   optimised `.wasm` as base64 into a separate JS chunk that the app imports
   lazily from `'self'` (allowed by `script-src`), decodes, and instantiates
   asynchronously with `WebAssembly.instantiate`, through wasm-bindgen's
   init function. Compilation is allowed by `'wasm-unsafe-eval'`.
5. **Synchronous `new WebAssembly.Module`.** Blocks the UI thread while a
   module of this size compiles. Rejected.

## Decision

Option 4. The CSP stays exactly as specified. The WebAssembly bytes are never
fetched and never put in a `data:` URL.

* The package `@blocks2cpp/b2c-core-wasm` builds the crate with
  `--profile wasm-release` for `wasm32-unknown-unknown`, runs `wasm-bindgen
  --target web` (the CLI version pinned to the lockfile's `wasm-bindgen`,
  0.2.129) and `wasm-opt -Oz` (binaryen pinned by release and SHA-256 in CI),
  then writes the base64 chunk.
* `initCore()` imports the chunk on first use and is idempotent. After a
  WebAssembly trap, `resetCore()` instantiates a fresh module.
* **Size budget:** at most 2,000,000 bytes for the `wasm-opt -Oz` output (the
  stricter reading of ADR-0003's 2 MB), checked in CI; the gzip size is
  reported only.

## Consequences

* The chunk is about 4/3 of the module size because of base64. It is loaded
  lazily, so it is not on the path to the first paint, and it is cached with
  the other bundled assets.
* The same chunk works in a Web Worker later (`worker-src 'self'` already
  allows one), when analysis moves off the main thread
  ([06 §6.13](../spec/06-compiler-pipeline.md#613-performance-in-the-editor)).
* The build needs `wasm-bindgen-cli` at exactly the pinned version and
  binaryen in CI. Local builds can skip `wasm-opt` with
  `B2C_SKIP_WASM_OPT=1`; then only the unoptimised size is checked.
* The build output (`packages/b2c-core-wasm/pkg/`) is not committed. A
  committed declaration of the generated bindings lets the TypeScript
  typecheck run without a build, and the jobs that run frontend tests or E2E
  tests build the package first.
