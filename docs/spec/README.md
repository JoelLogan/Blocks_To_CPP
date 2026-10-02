# Blocks2Cpp: Technical Specification

> **Status:** Draft v0.1 (2026-10-02). This is a living document. Every
> behaviour-changing PR updates the relevant chapter, and significant decisions
> are recorded as [ADRs](../adr/README.md).

Blocks2Cpp is a Scratch-style drag-and-drop editor for Windows and Linux
that turns blocks into real, readable C++. It compiles that C++ with the
user's own g++ and runs it in a built-in terminal.

## Chapters

| # | Chapter | What it covers |
|---|---------|----------------|
| 1 | [Overview, goals and requirements](01-overview.md) | Product summary, principles, personas, goals and non-goals, non-functional targets, glossary |
| 2 | [System architecture](02-architecture.md) | Technology stack, components, trust boundaries, repository layout, data flow, IPC surface, process model, platform notes |
| 3 | [The block language](03-block-language.md) | Block shapes, expression slots, types, scoping, the full block catalog by category, organisation features, Raw C++, library packs, coverage matrix, worked examples, Quick Insert |
| 4 | [User interface](04-user-interface.md) | Layout, toolbox, live code view, diagnostics UX, console, toolchain setup, keyboard, accessibility, localisation, debugger UI |
| 5 | [Project format and persistence](05-project-format.md) | `.b2c` JSON format, block nodes, symbols, validation limits, migrations, machine-local data, atomic saves, hashing, clipboard |
| 6 | [Translation pipeline](06-compiler-pipeline.md) | Load → resolve → lower → names → types → order → emit; typed text leaves; source maps; includes; export; diagnostics model |
| 7 | [Toolchain, compilation and execution](07-toolchain-build-run.md) | g++ discovery and probing, flags, libraries, build directory and caching, diagnostics mapping, running in a PTY, event side channel, debugger, CLI |
| 8 | [Security design and threat model](08-security.md) | Scope, trust boundaries, workspace trust, injection-proof codegen, compiler/filesystem/process/webview hardening, supply chain, releases, threat table, continuous security process |
| 9 | [Quality, testing, documentation and delivery](09-quality-and-delivery.md) | Engineering standards, test strategy, CI, documentation policy, versioning, releases |
| 10 | [Roadmap, risks and product decisions](10-roadmap.md) | Milestones M0–M6, risks, product decisions (decided and proposed) |

## Reading paths

* **"What will it be like to use?"** Chapters 1, 3 and 4.
* **"How does a block become a running program?"** Chapters 3.4, 6 and 7.
* **"Is it safe?"** Chapter 8, then 5.6, 7.4–7.7 and 2.5.
* **"How do we build it?"** Chapters 2, 9 and 10.

## Conventions

* Section references look like `03 §3.4` (chapter file, section number).
* **MUST/SHOULD/MAY** are used in their RFC 2119 sense only where noted. Most
  of the spec is descriptive design.
* Diagnostic codes (`B2C-E0201`) and block IDs (`io.print`) used here are the
  real identifiers the implementation will use.
