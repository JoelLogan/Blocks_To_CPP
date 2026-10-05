# Changelog

All notable changes to Blocks2Cpp are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) (0.x until 1.0).

## [Unreleased]

### Added

- Technical specification, architecture decision records and security policy.
- Specification website published with GitHub Pages.
- Rust workspace with the shared contracts of the compiler pipeline: validated
  IDs, the diagnostics model, injection-safe C++ text encoders, the Semantic AST,
  source maps, the project file model and the milestone-M1 block catalog.
- Project loading with every validation rule of the format, canonical saving
  and content hashing; the block catalog and its resolve stage.
- The analyser: lowering of the 32 milestone-M1 blocks, expression slots,
  scoping, types, flow checks and lints, with 46 documented diagnostics.
- C++ generation: readable, deterministic code with source maps and support
  helpers for random numbers and safe input.
- Toolchain discovery and probing, safe compiler commands and environment,
  and parsing of g++ diagnostics in SARIF, JSON and text.
- Process control that stops whole process trees (process groups on Linux,
  Job Objects on Windows).
- The `b2c` command-line tool: `check`, `generate`, `build`, `run`,
  `toolchains`, `migrate` and `fmt`.
- Fifteen example projects with golden tests, a malicious-project regression
  suite, fuzz targets, and CI on Linux (GCC 11, 13 and 15) and Windows (MSYS2)
  with security scanning.
- The desktop app shell (milestone M0): a Tauri 2 window with an empty Blockly
  canvas (Zelos renderer) and placeholder panels, hardened with the specified
  Content Security Policy, the isolation pattern and minimal capabilities, and
  built and checked in CI on Linux and Windows.
- More CI quality gates: API docs built with warnings denied, Markdown lint,
  an external link check, line-coverage gates (80% for all Rust code, 90% for
  the compiler crates), a size budget for the WebAssembly core, checks that
  generated files are up to date, and fuzz targets for the g++ diagnostics
  parsers, seeded with recorded GCC 11 and GCC 13 output.
- Milestone M2 foundations: the IPC contract between the editor and the
  backend (crate `b2c-ipc`, with generated TypeScript types and an isolation
  allowlist), machine-local settings and recent-project stores, pseudo-terminal
  sessions for running programs (openpty on Linux, ConPTY on Windows), the
  editor's scope query, the WebAssembly core for the live C++ preview, the
  catalog export that generates the toolbox and the block reference, and
  frontend test tooling with coverage and accessibility checks.
- Pasting blocks is validated like opening a file (new codes `B2C-E0138` and
  `B2C-E0139`), and loose block stacks are saved intact (`stack`).
- Nightly fuzzing with a kept corpus, and weekly mutation testing and
  dependency-health reports.
- Desktop editor building blocks (milestone M2): Blockly blocks generated from
  the catalog with Zelos shapes, light and dark themes and custom fields that
  show text literally; a type-aware connection checker and the variadic
  mutators; the app shell (start-up, state store, layout, toolbar, Run gating,
  accessible dialogs); the C++ code panel with source-map highlighting, the
  Problems list, the console and the build output; and build-cache eviction
  (2 GiB limit by default, 30-day pruning, Clear build cache).
- Builds run as cancellable sessions with progress, record a build manifest
  instead of a build stamp, compile modules in parallel, and link a small IDE
  unit (UTF-8 console on Windows) into app builds only; the Windows cache root
  is `%LOCALAPPDATA%\Blocks2Cpp`.
- Run sessions for the editor's console: terminal or pipe mode, batched output
  with flood protection, rate-limited input, friendly exit messages and
  sanitizer report summaries.
- Stronger containment: compilers and programs run in cgroup v2 scopes on
  Linux when a user service manager is available (otherwise a process group
  with an RSS watchdog), stale scopes are cleaned up at start-up, and captured
  runs on Windows inherit only an explicit list of handles. Compilers stopped
  for memory now report a limit, not a compiler crash.
- The app's toolchain list: background discovery that never delays start-up,
  compilers added by hand, a remembered choice that is checked again before
  each build (new warning `B2C-T1022` when it falls back), and the Linux
  distribution for setup instructions.
- Machine-local trust store (`trust.json`) with project and folder trust,
  safe across several app instances; the Windows Mark-of-the-Web check; and
  size-rotating local logs (5 × 5 MiB).
- The WebAssembly core answers the editor's scope, type and conversion
  questions and makes and checks clipboard payloads, with fresh IDs and
  references bound again where blocks are pasted.
- More tests from mutation testing: the mutants it found missed in the
  project-file parser, the decoder and the toolchain code are now caught, and
  the few equivalent ones are excluded with a reason each.
- The desktop app's backend (crate `b2c-app`): every IPC command without
  Tauri, with project open, save and close, workspace trust, builds and runs,
  toolchains, settings and the recent list, and start-up work that never
  delays the first window.
- Recovery snapshots for unsaved work, kept per app instance with locks so
  several instances never take each other's snapshots.
- The block editor: the Blockly canvas reads and writes the project format
  faithfully (unknown or unrepresentable blocks become placeholders that keep
  their data, so an unchanged project saves byte for byte), with a live C++
  preview 50 ms after each change, fresh symbol IDs for duplicated and pasted
  blocks, and a module switcher.
- The editor's toolbox: the catalog's categories in a continuous, Scratch-style
  toolbox with presets, and Variables, Loops and My Blocks categories that follow
  the scope at the selected block.
- Problems on blocks: a badge with a shape per severity, an outline and a mark
  on the exact part, for live and last-build diagnostics (dimmed once the
  project changed), and two-way highlighting between blocks, the C++ view and
  the Problems list.
- Review fixes: the scope query answers at disabled blocks; pasting into a
  statement list unstacks loose stacks; numbers JavaScript cannot hold exactly
  are refused on load, and free-form floats are saved in JavaScript's form; the
  console no longer traps the keyboard (Ctrl+Tab leaves it while a program
  runs) and shows dropped output; dialogs and docks keep keyboard focus; long
  expression tooltips are cut; and the editor reads the WebAssembly core anew
  after a crash recovery.
- Review fixes: on Windows, trust compares only ASCII letter case in paths,
  so a lookalike folder name is never trusted; a build returns its ID at once
  and can be cancelled while it waits for the compiler search or runs the
  front end; the program that was just built survives cache eviction; ended
  runs release their terminals; and dynamically linked Windows programs find
  the compiler's DLLs.
