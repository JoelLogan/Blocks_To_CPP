# Contributing to Blocks2Cpp

Thanks for helping. This guide covers how to set up, the rules every change
follows, and how changes are reviewed. The detailed engineering standards are in
[spec chapter 9](docs/spec/09-quality-and-delivery.md); the security rules are in
[spec chapter 8](docs/spec/08-security.md).

## Set up

You need:

* **Rust**: install [rustup](https://rustup.rs). The repository pins the exact
  toolchain in `rust-toolchain.toml`; `rustup toolchain install` in the
  repository fetches it.
* **g++ 11 or later** (13+ recommended). Linux: your distribution's `g++`.
  Windows: [MSYS2](https://www.msys2.org) with `pacman -S mingw-w64-ucrt-x86_64-gcc`,
  then add `C:\msys64\ucrt64\bin` to `PATH`.
* **Node.js 22.13+ and pnpm 11** for the website and the desktop app
  (see [site/README.md](site/README.md)).
* Python 3 for the small tools in `tools/`.

Then:

```sh
cargo build                     # all crates (the desktop app is built separately)
cargo test                      # unit, property and g++-backed tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all
```

## Repository layout

See [spec §2.3](docs/spec/02-architecture.md#23-repository-layout). In short:
`crates/` holds the Rust crates (the pure compiler crates `b2c-ir`, `b2c-model`,
`b2c-catalog`, `b2c-lang`, `b2c-codegen`; the native `b2c-toolchain`,
`b2c-process`, `b2c-build`; and the `b2c` CLI), `catalog/` the block
definitions, `apps/desktop/` the Tauri app, `docs/` the specification and
references, `site/` the specification website.

## Rules for every change

* **Small, focused pull requests** with [Conventional Commit](https://www.conventionalcommits.org)
  titles (`feat:`, `fix:`, `docs:`, `sec:`, …).
* **Tests and docs in the same PR.** Behaviour changes update the spec chapter,
  the user guide or the reference docs they affect. New diagnostic codes get an
  entry in [docs/reference/diagnostics/](docs/reference/diagnostics/README.md)
  (CI checks this). Significant decisions get an [ADR](docs/adr/README.md).
* **User-visible changes** get a line in [CHANGELOG.md](CHANGELOG.md).
* **No new dependency without a justification** in the PR: purpose,
  maintenance health, licence, size and transitive count (spec §8.9). The
  dependency policy is enforced by `cargo deny` and pnpm's workspace settings.
* **Security-relevant code needs extra care:** new IPC commands, `unsafe`
  (allowed only in `crates/b2c-process`), process or file operations, and
  anything that turns user text into C++ (only through `b2c_ir::text`). These
  paths have CODEOWNERS review.
* Generated C++ must stay **deterministic** and **readable**; snapshot tests
  show every change to it.

## Definition of done

The pull request template has the checklist (tests, docs, changelog, security
questions, accessibility). CI must be green: format, Clippy, tests on Linux and
Windows, the WebAssembly build of the compiler crates, `cargo deny`,
CodeQL, OSV-Scanner, secret scanning and the website build.

## Reporting security issues

Do not open public issues for vulnerabilities; follow [SECURITY.md](SECURITY.md).

## Licence

By contributing you agree that your contributions are licensed under the
[Apache License 2.0](LICENSE).
