# ADR-0003: One Rust compiler core, built natively and as WebAssembly

* Status: Accepted
* Date: 2026-10-02

## Context

Translation from blocks to C++ (parsing, name resolution, type checking,
code generation and source maps) is needed in three places:

1. **The editor:** live C++ preview, diagnostics, scope-aware dropdowns and
   expression-slot parsing on every edit (< 50 ms).
2. **The backend:** the authoritative generation before compiling. It must not
   trust C++ text from the webview.
3. **The CLI:** headless check/build/run for CI, teachers and our own golden
   tests.

Two implementations (e.g. TypeScript for the editor and Rust for the backend)
would drift. Security-critical escaping would then exist twice, and tests
would only cover one copy.

## Options considered

1. **TypeScript core** used by the editor, with the backend and CLI running
   it in Node or an embedded JS engine
   * Cons: puts a JS runtime in the privileged backend; weaker typing for the
     security-critical encoders.
2. **Rust core in the backend only**, with the editor calling it over IPC
   * Cons: an IPC round trip on every keystroke in expression slots; the
     editor stalls if the backend is busy building.
3. **Rust core compiled both natively and to WASM**
   * Pros: one implementation; memory-safe; fuzzable; the same code and the
     same results everywhere.
   * Cons: a WASM build step, the CSP needs `'wasm-unsafe-eval'`, and some
     JS↔WASM marshalling cost.

## Decision

**Option 3.** The crates `b2c-model`, `b2c-catalog`, `b2c-lang` and
`b2c-codegen` are pure (no I/O) and compile for both native targets and
`wasm32-unknown-unknown`. `b2c-core-wasm` is a thin `wasm-bindgen` facade.

## Consequences

* The compiler crates must stay free of I/O, threads, clocks and randomness.
  This is enforced by a CI check that builds them for WASM.
* The CSP includes `'wasm-unsafe-eval'` (WASM compilation only, not JS `eval`).
* WASM size is budgeted (≤ 2 MB after `wasm-opt -Oz`) and checked in CI.
* The backend always regenerates C++ from the BDM natively, so compromising the
  webview cannot change what is compiled beyond what the blocks express.
