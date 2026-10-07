# Blocks2Cpp

**Build real C++ programs by snapping blocks together.**

Blocks2Cpp is a Scratch-style, drag-and-drop programming environment for
**Windows and Linux**. Every block arrangement becomes clean, readable,
standard C++. That C++ is compiled with the **g++ already on your system** and
run in a built-in terminal.

- 🧩 **Scratch-like blocks:** colourful, snap-together, no syntax errors.
- ⚡ **Few blocks, much power:** type expressions straight into slots
  (`price * qty + tax`), use `⊕` to add more parts, and pick types from a
  single field.
- 🔍 **See the C++:** a live code view highlights the C++ for every block,
  and you can export a normal C++/CMake project at any time.
- 🛠 **Real C++:** functions, classes, templates, containers, pointers,
  exceptions, files, threads, third-party libraries, plus a clearly marked Raw
  C++ escape hatch for anything else.
- 🧭 **Errors on the right block:** our analyser and g++ messages are mapped
  back to the exact block and explained in plain language.
- 🔒 **Secure by default:** projects from elsewhere open in Restricted Mode,
  compiler flags never come from project files, and block text can't inject
  code.

> **Project status:** the desktop block editor works on Windows and Linux:
> build a program from blocks, watch its C++ appear, and build and run it
> with your g++ in the built-in console (milestone M2 of the
> [roadmap](docs/spec/10-roadmap.md#101-milestones), in progress: its
> automated checks pass, and the manual usability and screen-reader checks
> are still to be done). The compiler also works from the command line (M1).
> There are no installers yet (M6), so the app is built from source. The list
> above is the goal for 1.0;
> [what the M2 editor leaves to later milestones](docs/spec/04-user-interface.md#413-what-the-m2-editor-includes)
> includes typing expressions into slots and exporting a project. The full
> technical specification is in [`docs/spec/`](docs/spec/README.md).

## Try it

You need [Rust](https://rustup.rs) and g++ 11 or newer (on Windows,
[MSYS2](https://www.msys2.org)'s UCRT64 g++). The pinned Rust toolchain is
installed automatically.

### The desktop editor

The desktop app also needs [Node.js](https://nodejs.org) 22.13 or newer with
[pnpm](https://pnpm.io), and on Linux the WebKitGTK libraries
([details](apps/desktop/README.md#requirements)):

```sh
pnpm install --frozen-lockfile
pnpm --filter @blocks2cpp/b2c-core-wasm build   # the compiler core the editor runs (WebAssembly)
pnpm desktop:dev                                 # open the app, reloading as you edit
```

Building the WebAssembly core needs the wasm-bindgen CLI
(`cargo install wasm-bindgen-cli --version 0.2.129 --locked`) and binaryen's
`wasm-opt` (`sudo apt install binaryen` on Debian and Ubuntu); without
`wasm-opt`, set `B2C_SKIP_WASM_OPT=1`
([details](packages/b2c-core-wasm/README.md#build)).
Then follow [Getting started](docs/user-guide/getting-started.md): it walks
you from the first start to a number-guessing game built from blocks.

### The command-line tool

```sh
cargo build --release -p b2c-cli
./target/release/b2c run examples/hello_world.b2c        # build and run
./target/release/b2c run examples/guessing_game.b2c      # an interactive program
./target/release/b2c generate examples/fizzbuzz.b2c --out generated   # see the C++
./target/release/b2c check examples/primes.b2c           # check without building
./target/release/b2c toolchains                          # the compilers found
```

[`examples/`](examples/README.md) has 15 projects to try, and
[`docs/reference/cli.md`](docs/reference/cli.md) describes every command.

## Documentation

| Document                                                            | Description                                                                                                                       |
| ------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| [User guide](docs/user-guide/README.md)                             | Getting started with the desktop editor: setting up g++, building and running a first program, Restricted Mode                    |
| [Specification website](https://joellogan.github.io/Blocks_To_CPP/) | The whole specification and decision records on one page, built from `docs/` by [`site/`](site/README.md)                         |
| [Specification](docs/spec/README.md)                                | Architecture, block language, project format, translation pipeline, toolchain/build/run, security model, quality process, roadmap |
| [Architecture Decision Records](docs/adr/README.md)                 | Why key technical choices were made                                                                                               |
| [Command-line tool](docs/reference/cli.md)                          | `b2c check`, `generate`, `build`, `run`, `toolchains`, `migrate` and `fmt`                                                        |
| [Block reference](docs/reference/blocks/README.md)                  | Every block, the C++ it becomes and its options                                                                                   |
| [Diagnostics reference](docs/reference/diagnostics/README.md)       | Every message Blocks2Cpp can show, with examples and fixes                                                                        |
| [Desktop app](apps/desktop/README.md)                               | How the editor is built, its security settings and its dependencies                                                               |
| [Manual tests](docs/manual-tests/README.md)                         | The milestone checks done by hand: screen readers and usability                                                                   |
| [Security policy](SECURITY.md)                                      | How to report vulnerabilities                                                                                                     |

## Technology

Tauri 2 (Rust backend + system webview) · Blockly (Zelos renderer) · React +
TypeScript · a Rust compiler core compiled natively and to WebAssembly ·
CodeMirror 6 · xterm.js · system g++ 11+ (13+ recommended).

## Licence

Blocks2Cpp is licensed under the [Apache License, Version 2.0](LICENSE).
Unless you explicitly state otherwise, any contribution you intentionally
submit for inclusion in this project is licensed under the same terms,
without any additional terms or conditions.

The font files in [`site/src/fonts/`](site/src/fonts/) are not covered by the
Apache License. They are © The Atkinson Hyperlegible Next Project Authors and
licensed under the [SIL Open Font License 1.1](site/src/fonts/OFL.txt).
