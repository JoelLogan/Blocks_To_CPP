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
