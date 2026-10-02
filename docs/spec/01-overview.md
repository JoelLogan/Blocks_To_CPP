# 1. Overview, Goals and Requirements

> Status: **Draft v0.1** · Owner: project maintainers · Last reviewed: 2026-10-02

## 1.1 Product summary

**Blocks to C++** (working name, short form `b2c`) is a desktop application for
Windows and Linux. You build programs by dragging and snapping together blocks,
much like [Scratch](https://scratch.mit.edu). Under the hood every block
arrangement is translated into **real, readable, standard C++**. That C++ is
compiled with the **g++ already installed on the user's machine** and run
inside the application.

It is meant to be:

* **As approachable as Scratch.** Colourful snap-together blocks, instant
  feedback, and no syntax errors from typos.
* **As capable as C++.** Functions, classes, templates, containers, pointers,
  exceptions, files, threads, the standard library and third-party libraries.
  The goal is that almost anything you can write in C++ you can build here.
* **Honest about the C++ it produces.** The generated code is always visible
  and idiomatic, and a user can export it as an ordinary C++ project and leave
  the tool behind at any time.

## 1.2 Guiding principles

| # | Principle | What it means in practice |
|---|-----------|---------------------------|
| P1 | **Few blocks, much power** | One block should express one *idea*, not one *token*. We use inline fields, dropdowns, `+`/`−` variadic slots, typed expression slots and type pickers instead of making users drag extra blocks. |
| P2 | **Organisation is a feature** | Projects are split into modules (files), definition blocks are top-level and order-independent, and users get grouping frames, collapse, search, a minimap and "go to definition". |
| P3 | **What you see is what compiles** | Each block maps to well-defined C++. There is no hidden runtime, no interpreter and no proprietary VM. The live C++ view is the source of truth for semantics. |
| P4 | **Fail early, explain kindly** | Our own analyser catches most mistakes before g++ runs. When g++ does complain, the error is mapped back to the exact block and explained in plain language. |
| P5 | **Secure by default** | Untrusted project files cannot execute anything until the user explicitly trusts them, compiler flags are never taken from project files, and generated code cannot be injected through block text. See [08-security.md](08-security.md). |
| P6 | **Production quality, always** | Strict linting, high test coverage, reproducible builds, continuous vulnerability scanning and documentation updated in the same PR as the code. See [09-quality-and-delivery.md](09-quality-and-delivery.md). |
| P7 | **Escape hatches, clearly marked** | Anything the block language cannot express can be written as a *Raw C++* block. These blocks are visually distinct and count toward the project's trust level. |

## 1.3 Target users (personas)

| Persona | Background | Needs |
|---------|------------|-------|
| **Learner** (age 12+) | Has used Scratch and wants "real" programming. | Friendly block labels ("list of numbers"), help with errors, no setup pain, can see the C++ it is learning. |
| **Student** | Taking an intro C++ / CS course. | Can complete course assignments (I/O, loops, functions, structs, classes, vectors, files) and hand in readable `.cpp` files. |
| **Teacher** | Teaches programming. | Example projects, a CLI for batch-checking submissions, predictable output, exported code students can diff. |
| **Hobbyist / maker** | Knows some programming. | Fast prototyping with real libraries (e.g. SFML, raylib) through *library packs*, raw C++ when needed. |
| **Experienced developer** | Fluent in C++. | Should never feel boxed in: templates, lambdas, smart pointers, threads, custom flags (machine-local), keyboard-driven block entry. |

## 1.4 Goals

### Functional goals

* **G1 Visual editor.** A Scratch-style drag-and-drop canvas built on Blockly
  with a Scratch-like ("Zelos") renderer, a categorised toolbox, multiple
  module tabs and undo/redo.
* **G2 Broad C++ coverage.** Native blocks for the core language and the
  commonly used standard library, data-driven *library blocks* for everything
  else, typed *expression slots* for concise maths and logic, and *Raw C++*
  blocks as a last resort. See the coverage matrix in
  [03-block-language.md §3.12](03-block-language.md#312-c-coverage-matrix).
* **G3 Live C++ preview.** Generated C++ updates within 50 ms of an edit, with
  two-way highlighting between blocks and code.
* **G4 Compile with system g++.** Automatic discovery of g++ installations
  (MSYS2/MinGW-w64, WinLibs, TDM, Scoop, Chocolatey, distro packages), version
  and capability probing, and a guided setup when none is found.
* **G5 Run inside the app.** An integrated terminal (pseudo-terminal) with
  interactive stdin, ANSI colours, a Stop button, exit-code display and
  friendly crash explanations.
* **G6 Diagnostics on blocks.** Our analyser's errors and g++'s errors both
  appear as badges on the offending block, in a Problems list, and in plain
  language.
* **G7 Export.** Export a project as plain C++ (`.hpp`/`.cpp` files plus
  `CMakeLists.txt` and a `Makefile`) that builds without this tool.
* **G8 Headless CLI.** A `b2c` command-line tool that can check, generate,
  build and run projects. It is used for CI, grading, and our own end-to-end
  tests.
* **G9 Extensibility.** *Library packs* (declarative TOML) add blocks for any
  C++ library. The standard-library blocks themselves are shipped as a library
  pack, so the mechanism is exercised by our own code.
* **G10 Debugging (later phase).** Breakpoints on blocks, step through blocks,
  and inspect variables via GDB/MI, with the currently executing block
  highlighted.

### Non-functional goals

| ID | Requirement | Target |
|----|-------------|--------|
| N1 | Platforms | Windows 10 (1809+) and Windows 11 x64; Linux x64 (glibc 2.35+, e.g. Ubuntu 22.04+, Fedora 39+, Debian 12+). arm64 Linux is best-effort. macOS is out of scope for 1.0 but is not precluded by the architecture. |
| N2 | Cold start | < 2 s to an interactive editor on a mid-range laptop |
| N3 | Editor responsiveness | ≥ 30 fps dragging in a 5,000-block workspace |
| N4 | Live preview latency | < 50 ms (p95) codegen + analysis for a 1,000-block module |
| N5 | Memory | < 400 MB resident for a typical (≤ 2,000 blocks) project |
| N6 | Installer size | < 30 MB (no bundled browser engine, no bundled compiler) |
| N7 | Accessibility | Full keyboard operation, screen-reader labels, high-contrast theme, colour-blind-safe category palette (WCAG 2.2 AA for app chrome) |
| N8 | Localisation | All UI and block labels externalised; English at 1.0, structure ready for more |
| N9 | Privacy | No telemetry. No network access except opt-in update checks and user-clicked links. |
| N10 | Reliability | Autosave and crash recovery. Atomic saves. A project file is never left half-written. |
| N11 | Determinism | The same project, catalog version and settings always generate byte-identical C++. |

## 1.5 Non-goals (for 1.0)

* **Not an interpreter or VM.** Programs always run as native executables
  built by g++.
* **No round-trip import of arbitrary C++ into blocks.** Parsing real-world
  C++ into blocks is a research problem. We support importing *declarations*
  (function signatures, simple structs) so that external code becomes callable
  blocks, and Raw C++ blocks hold anything else.
* **Not a sandbox for hostile code.** A program the user runs has the user's
  privileges, just as in any IDE. We provide trust prompts and optional
  best-effort sandboxing, not a security guarantee. See
  [08-security.md](08-security.md).
* **No bundled compiler in 1.0.** We use the system g++. A verified
  toolchain-installer helper is a post-1.0 roadmap item.
* **No cloud features, accounts or online sharing in 1.0.**

## 1.6 Key terms

| Term | Meaning |
|------|---------|
| **Block** | A visual element on the canvas: a statement, expression, definition or container. |
| **BDM** | *Block Document Model*: our versioned, editor-independent JSON representation of blocks, and the canonical storage format. |
| **Catalog** | The declarative definitions of all block types (TOML), shared by the editor and the compiler. |
| **Library pack** | A catalog bundle that adds blocks, types and headers for a C++ library. |
| **Module** | One source unit of a project. It becomes a `.cpp` file, plus a `.hpp` file if it shares anything. Each module is shown as an editor tab. |
| **Expression slot** | A value input that accepts typed text such as `a * b + 3`, parsed and checked by our own expression parser. |
| **SAST / CAST** | *Semantic AST* (language-level, typed) and *C++ AST* (syntax-level); the compiler's internal representations. |
| **Source map** | A mapping from generated C++ ranges to block IDs, used for highlighting and diagnostics. |
| **Restricted Mode** | The state of an untrusted project: it can be viewed and edited, but cannot be built or run. |
| **Toolchain** | A discovered g++ installation and its probed capabilities. |
