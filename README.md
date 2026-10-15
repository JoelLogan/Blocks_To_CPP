# Blocks2Cpp

**Build real C++ programs by snapping blocks together.**

Blocks2Cpp is a Scratch-style, drag-and-drop programming environment for
**Windows and Linux**. Every block arrangement becomes clean, readable,
standard C++. That C++ is compiled with the **g++ already on your system** and
run in a built-in terminal.

> **Status:** the desktop editor, milestone M2 of the [roadmap](docs/spec/10-roadmap.md#101-milestones),
> works on Windows and Linux and is being polished before its usability tests.
> There are no installers yet (M6), so you [build it from source](BUILDING.md).

## What works today (M2)

- 🧩 **Blocks from a toolbox:** colourful, snap-together blocks for variables,
  maths and logic, text, control and loops, input and output, and your own
  functions. Blocks left loose on the canvas are kept, and do not run.
- 🔍 **See the C++:** a live view shows the C++ of your blocks as you build,
  and highlights the code of the block you select, and the block of the code
  you click.
- 🧭 **Problems on the right block:** the analyser's messages, in plain
  language, and g++'s own messages are shown on the blocks they are about and
  listed under Problems.
- ▶️ **Build and run** with your g++ (11 or newer) in the built-in console,
  with Stop and the program's exit explained. The app helps you install g++
  when it finds none.
- 💾 **Projects** in a readable file format, with autosave and recovery after
  a crash, and a warning when the file changes outside the app.
- 🔒 **Secure by default:** projects from elsewhere open in Restricted Mode
  until you trust them, compiler flags never come from project files, and
  block text can't inject code.
- ⌨️ **Keyboard:** the canvas and the toolbox can be used without a mouse,
  and announce what the keyboard reaches to screen readers.
- 🛠 **A command-line tool** (`b2c`) that checks, translates, builds and runs
  projects without the app.

The [M2 editor's scope](docs/spec/04-user-interface.md#413-what-the-m2-editor-includes)
lists what later milestones add, such as typing expressions into slots and
exporting a project.

## Goals for 1.0

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
- 📦 **Signed installers** for Windows and Linux, and updates only when you
  agree to them.

The [roadmap](docs/spec/10-roadmap.md#101-milestones) says which milestone
brings what, and the full technical specification is in
[`docs/spec/`](docs/spec/README.md).

## Get Blocks2Cpp

There are no installers yet, so you build Blocks2Cpp from its source code.
[`BUILDING.md`](BUILDING.md) has every step for Debian, Ubuntu and Windows:
what to install, how to build the desktop app and the command-line tool, and
how to start them. To build and run the programs you make, the app needs
g++ 11 or newer; it shows you how to install it if you have none.

Then follow [Getting started](docs/user-guide/getting-started.md): it walks
you from the first start to a number-guessing game built from blocks.
[`examples/`](examples/README.md) has 15 projects to try, and
[`docs/reference/cli.md`](docs/reference/cli.md) describes every command of
the command-line tool.

## Documentation

| Document                                                            | Description                                                                                                                       |
| ------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| [Building Blocks2Cpp](BUILDING.md)                                  | What to install, and how to build, start and update the app and the command-line tool on Linux and Windows                        |
| [User guide](docs/user-guide/README.md)                             | Getting started with the desktop editor: setting up g++, building and running a first program, Restricted Mode                    |
| [Specification website](https://joellogan.github.io/Blocks_To_CPP/) | The whole specification and decision records on one page, built from `docs/` by [`site/`](site/README.md)                         |
| [Specification](docs/spec/README.md)                                | Architecture, block language, project format, translation pipeline, toolchain/build/run, security model, quality process, roadmap |
| [Architecture Decision Records](docs/adr/README.md)                 | Why key technical choices were made                                                                                               |
| [Command-line tool](docs/reference/cli.md)                          | `b2c check`, `generate`, `build`, `run`, `toolchains`, `migrate` and `fmt`                                                        |
| [Block reference](docs/reference/blocks/README.md)                  | Every block, the C++ it becomes and its options                                                                                   |
| [Diagnostics reference](docs/reference/diagnostics/README.md)       | Every message Blocks2Cpp can show, with examples and fixes                                                                        |
| [Contributing](CONTRIBUTING.md)                                     | How to set up, the rules for a change, the checks CI runs, and where things are documented                                        |
| [Developer guide](docs/developer-guide/README.md)                   | How the desktop app is built inside: architecture, security settings, dependencies, testing and recipes                           |
| [Desktop app](apps/desktop/README.md)                               | What the app is and where its code is                                                                                             |
| [Manual tests](docs/manual-tests/README.md)                         | The milestone checks done by hand: screen readers, usability, performance and the toolchain matrix                                |
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
