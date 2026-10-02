# 9. Quality, Testing, Documentation and Delivery

> Status: **Draft v0.1**

## 9.1 Engineering standards

### Rust

* Edition 2024. The toolchain is pinned in `rust-toolchain.toml`, and the MSRV
  is declared in the workspace `Cargo.toml` and checked in CI.
* `rustfmt` (checked). Clippy runs with `-D warnings` plus a curated
  `pedantic` subset and these lints at `deny`: `unwrap_used`, `expect_used`
  (outside tests), `panic`, `indexing_slicing` (in parsers/encoders), `todo`,
  `dbg_macro`, `print_stdout`, plus `disallowed_methods` (e.g.
  `std::process::Command::new` outside `b2c-process`, raw writes outside the
  emitter's token module).
* `#![forbid(unsafe_code)]` in every crate except `b2c-process`
  ([02 §2.3](02-architecture.md#23-repository-layout)).
* Errors use `thiserror` enums per crate, with user-facing text produced by
  the i18n layer, never by `Display` of internal errors. No panics cross the
  IPC boundary: command handlers return typed errors.
* `#![warn(missing_docs)]` on all library crates.
* Logging uses `tracing` with structured fields and spans per build/run
  session.

### TypeScript / frontend

* `strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`,
  `noImplicitOverride`, `noFallthroughCasesInSwitch`.
* ESLint (flat config): `typescript-eslint` *strict-type-checked*,
  `eslint-plugin-react`, `react-hooks`, `jsx-a11y`, `no-unsanitized`, and
  custom `no-restricted-*` rules from [08 §8.8](08-security.md#88-webview-and-ipc-hardening).
  No `any` and no non-null assertions without a justified disable comment.
* Prettier (checked). IPC types are **generated** from Rust (`specta`/`ts-rs`);
  a CI check fails if they are stale.

### General

* **Conventional Commits** (`feat:`, `fix:`, `docs:`, `sec:`, …), small
  focused PRs, squash merges.
* Branch protection on `main`: required status checks, at least one review,
  CODEOWNERS review for `crates/b2c-process/`, `apps/desktop/src-tauri/`,
  `.github/`, `catalog/`, and security-relevant encoders.
* **Definition of Done** for every PR:
  - [ ] Tests added or updated (unit + golden/E2E where behaviour is visible)
  - [ ] Docs updated (spec/user guide/reference); a new ADR for significant decisions
  - [ ] `CHANGELOG.md` entry for user-visible changes
  - [ ] Security checklist answered (new IPC command? new `unsafe`? new dependency? new process/file operation? new untrusted input?)
  - [ ] Accessibility checked for UI changes (keyboard path, labels, contrast)
  - [ ] No new warnings; all CI checks green

## 9.2 Testing strategy

| Level | Tooling | Scope / oracle |
|-------|---------|----------------|
| **Unit** | `cargo nextest`, Vitest | Every module. Encoders, parsers and validators aim for 100% branch coverage. |
| **Snapshot** | `insta` | Generated C++ for every catalog block in its default configuration, plus curated variants. Each snapshot is also compiled with `-fsyntax-only`. |
| **Golden end-to-end** | `b2c` CLI | `examples/*.b2c` → generate → compile → run with an stdin fixture → compare stdout, stderr and exit code with `tests/golden/`. Matrix: GCC 11, 13 and 15 on Linux (containers), MSYS2 UCRT64 GCC on Windows. |
| **Property-based** | `proptest` | (a) String-literal round trip: arbitrary text → program prints exactly those bytes. (b) **Differential "no generator bugs" oracle**: random well-typed block programs from a BDM generator; if the analyser reports no errors, g++ must accept the output. (c) Determinism: generate twice, and with shuffled top-level order → identical bytes. (d) BDM → Blockly → BDM round trip is the identity. |
| **Fuzzing** | `cargo-fuzz` (libFuzzer) | BDM parser, expression parser, type parser, catalog/pack loader, SARIF/JSON/text diagnostics parsers, GDB/MI parser, event-channel parser, comment/string encoders |
| **Mutation** | `cargo-mutants` (weekly) | Security-critical encoders and validators must kill all mutants |
| **Frontend components** | Vitest + Testing Library + axe-core | Panels, dialogs, custom Blockly fields. Automated accessibility checks. |
| **Desktop E2E** | WebdriverIO + `tauri-driver` (Windows + Linux) | Create project, Quick Insert, build, run with input, stop; open an untrusted project → Restricted Mode; trust flow; save/reload; crash recovery |
| **Security regression** | Custom harness | `tests/security/projects/*.b2c` with asserted outcomes ([08 §8.12](08-security.md#812-threat-summary)) |
| **Performance** | `criterion` + scripted workspace benchmarks | Codegen p95 at 1k blocks, drag frame time at 5k blocks, cold start. More than 10% regression fails CI on the benchmark job. |
| **Manual** | Per milestone | Screen readers (NVDA on Windows, Orca on Linux), high-contrast themes, real-world toolchain matrix (WinLibs, TDM, Scoop, Strawberry, distro GCCs) |

**Coverage gates:** core compiler crates ≥ 90% lines, all Rust ≥ 80%, and
frontend ≥ 75%. Measured with `cargo-llvm-cov` and Vitest coverage, and
reported on PRs.

## 9.3 Continuous integration

GitHub Actions workflows. All actions are SHA-pinned and least-privilege
([08 §8.9](08-security.md#89-supply-chain)).

| Workflow | Trigger | Jobs |
|----------|---------|------|
| `ci.yml` | PR, push to `main` | `lint` (rustfmt, clippy, eslint, prettier, markdownlint, `tsc --noEmit`) · `test-rust` (ubuntu, windows) · `test-web` · `wasm` (build + size budget) · `golden` (GCC matrix) · `e2e` (ubuntu, windows) · `docs` (lychee link check, generated reference up to date) · `security` (cargo-deny, pnpm audit, osv-scanner, gitleaks, security suite) · `fuzz-smoke` |
| `codeql.yml` | PR, push, weekly | CodeQL: JavaScript/TypeScript, Rust, GitHub Actions |
| `workflow-audit.yml` | PR touching `.github/` | zizmor |
| `pages.yml` | PR, push touching `docs/` or `site/` | Build the specification website (fails on broken links, raw HTML or missing anchors); deploy to GitHub Pages from the default branch |
| `nightly.yml` | Daily | Extended fuzzing, full OSV scan, E2E on both OSes, benchmarks |
| `weekly.yml` | Weekly | `cargo-mutants`, OpenSSF Scorecard, dependency health report |
| `release.yml` | Signed tag `v*` | Build and bundle (Windows MSI/NSIS, Linux AppImage/deb/rpm), sign, SBOMs, provenance attestations, draft GitHub Release. Runs in the protected `release` environment. |

## 9.4 Documentation

Documentation is part of the product and is maintained **in the same PR** as
the code it describes.

| Doc | Location | Audience | Maintenance |
|-----|----------|----------|-------------|
| Specification | `docs/spec/` | Contributors | Living document with status and last-reviewed date per chapter |
| Specification website | `site/` → GitHub Pages | Everyone | **Generated** from `docs/spec/` and `docs/adr/` on every change; the build fails on broken links |
| ADRs | `docs/adr/` | Contributors | One per significant decision (template in `docs/adr/README.md`); superseded, never deleted |
| User guide + tutorials | `docs/user-guide/` (mdBook) | Users, teachers | Screenshots regenerated by E2E tests |
| Block reference | `docs/reference/blocks/` | Users | **Generated** from the catalog; CI fails if stale |
| Diagnostics reference | `docs/reference/diagnostics/` | Users | **Generated** from the message catalog plus explanations; CI fails if a code is undocumented |
| API docs | rustdoc / TSDoc | Contributors | Built in CI; `missing_docs` warnings |
| CLI reference | `docs/reference/cli.md` | Users, CI authors | Generated from `clap` definitions |
| `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `CHANGELOG.md` | Repo root | Everyone | Keep a Changelog format |

The app ships the user guide and references **offline**. *Help* opens bundled
pages, and every diagnostic links to its reference entry.

## 9.5 Versioning

| Thing | Scheme |
|-------|--------|
| Application | SemVer. `0.x` until 1.0. |
| Project file | Integer `formatVersion` with migrations ([05 §5.7](05-project-format.md#57-versioning-and-migration)) |
| Catalog / packs | SemVer. Per-block integer `v` with migrations. |
| CLI JSON output, IPC | Versioned schemas. Breaking changes only in major releases. |

## 9.6 Release process

1. Release branch cut, with feature freeze. The changelog is curated.
2. Release gate ([08 §8.10](08-security.md#810-releases-and-updates)): all
   checks green, no high/critical advisories, threat-model review done, manual
   test matrix done.
3. A signed tag triggers `release.yml`. Artifacts are signed, SBOMs and
   attestations are attached, and the draft release is reviewed and
   published.
4. The update manifest is published (signed). The documentation site is
   deployed.

**Channels:** `nightly` (automated, for testers), `beta`, `stable`.
