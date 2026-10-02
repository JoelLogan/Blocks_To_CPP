# Blocks to C++

**Build real C++ programs by snapping blocks together.**

Blocks to C++ is a Scratch-style, drag-and-drop programming environment for
**Windows and Linux**. Every block arrangement becomes clean, readable,
standard C++. That C++ is compiled with the **g++ already on your system** and
run in a built-in terminal.

* 🧩 **Scratch-like blocks:** colourful, snap-together, no syntax errors.
* ⚡ **Few blocks, much power:** type expressions straight into slots
  (`price * qty + tax`), use `⊕` to add more parts, and pick types from a
  single field.
* 🔍 **See the C++:** a live code view highlights the C++ for every block,
  and you can export a normal C++/CMake project at any time.
* 🛠 **Real C++:** functions, classes, templates, containers, pointers,
  exceptions, files, threads, third-party libraries, plus a clearly marked Raw
  C++ escape hatch for anything else.
* 🧭 **Errors on the right block:** our analyser and g++ messages are mapped
  back to the exact block and explained in plain language.
* 🔒 **Secure by default:** projects from elsewhere open in Restricted Mode,
  compiler flags never come from project files, and block text can't inject
  code.

> **Project status:** design phase. The full technical specification is in
> [`docs/spec/`](docs/spec/README.md). Implementation follows the roadmap in
> [`docs/spec/10-roadmap.md`](docs/spec/10-roadmap.md).

## Documentation

| Document | Description |
|----------|-------------|
| [Specification](docs/spec/README.md) | Architecture, block language, project format, translation pipeline, toolchain/build/run, security model, quality process, roadmap |
| [Architecture Decision Records](docs/adr/README.md) | Why key technical choices were made |
| [Security policy](SECURITY.md) | How to report vulnerabilities |

## Planned technology

Tauri 2 (Rust backend + system webview) · Blockly (Zelos renderer) · React +
TypeScript · a Rust compiler core compiled natively and to WebAssembly ·
CodeMirror 6 · xterm.js · system g++ 11+ (13+ recommended).

## Licence

To be decided; see [open question Q1](docs/spec/10-roadmap.md#103-open-questions-owner-decisions-needed).
