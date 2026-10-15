# Contributing to Blocks2Cpp

Thanks for helping. This guide covers how to set up, the rules every change
follows, the checks a change must pass, and where things are documented. The
detailed engineering standards are in
[spec chapter 9](docs/spec/09-quality-and-delivery.md); the security rules are
in [spec chapter 8](docs/spec/08-security.md).

## Set up

Set up your machine with [`BUILDING.md`](BUILDING.md), the one place with the
install and build steps for Debian, Ubuntu and Windows: Rust, Node.js and pnpm,
the WebAssembly tools, the webview libraries and g++ 11 or newer. Its
[Develop](BUILDING.md#develop) section starts the app from the development
server.

The checks below also need:

- Python 3 for the small tools in `tools/`;
- for the end-to-end tests, the drivers and the virtual display listed in
  [`apps/desktop/e2e/README.md`](apps/desktop/e2e/README.md#running-them-locally-linux);
- optionally, to run locally what CI runs on dependencies and workflows:
  [cargo-deny](https://github.com/EmbarkStudios/cargo-deny),
  [lychee](https://github.com/lycheeverse/lychee),
  [zizmor](https://github.com/zizmorcore/zizmor) and
  [actionlint](https://github.com/rhysd/actionlint).

## Repository layout

See [spec §2.3](docs/spec/02-architecture.md#23-repository-layout). In short:

- The Cargo workspace has 14 crates. In `crates/`: the pure compiler crates
  `b2c-ir`, `b2c-model`, `b2c-catalog`, `b2c-lang` and `b2c-codegen`, and
  `b2c-core-wasm`, their WebAssembly facade for the editor; the native
  `b2c-toolchain`, `b2c-process`, `b2c-ipc`, `b2c-store`, `b2c-build` and
  `b2c-app`; and `b2c-cli`, the `b2c` command-line tool. The desktop app's
  crate, `blocks2cpp-desktop`, is in `apps/desktop/src-tauri/`.
- `packages/` holds the frontend packages, `apps/desktop/` the Tauri app,
  `catalog/` the block definitions, `examples/` the example projects, `tests/`
  the golden files and the security suites, and `fuzz/` the fuzz targets.
- `docs/` holds the specification, the decision records, the guides, the
  references and the manual test protocols; `site/` builds the specification
  website, and `tools/` holds the repository's checks.

## Rules for every change

- **Small, focused pull requests** with
  [Conventional Commit](https://www.conventionalcommits.org) titles (`feat:`,
  `fix:`, `docs:`, `sec:`, …).
- **Tests and docs in the same PR.** Behaviour changes update the spec
  chapter, the user guide or the reference docs they affect. New diagnostic
  codes get an entry in
  [docs/reference/diagnostics/](docs/reference/diagnostics/README.md) (CI
  checks this). Significant decisions get an [ADR](docs/adr/README.md).
- **User-visible changes** get a line in [CHANGELOG.md](CHANGELOG.md).
- **No new dependency without a justification** in the PR: purpose,
  maintenance health, licence, size and transitive count (spec §8.9), and a
  row in the
  [dependency table](docs/developer-guide/dependencies.md). The dependency
  policy is enforced by `cargo deny` and pnpm's workspace settings.
- **Security-relevant code needs extra care:** new IPC commands, `unsafe`
  (allowed only in `crates/b2c-process`), process or file operations, and
  anything that turns user text into C++ (only through `b2c_ir::text`). These
  paths have CODEOWNERS review.
- Generated C++ must stay **deterministic** and **readable**; snapshot tests
  show every change to it.

## Checks

CI runs these on every pull request
([spec §9.3](docs/spec/09-quality-and-delivery.md#93-continuous-integration)),
except the few marked _not run by CI_; run the ones for the parts you changed
before you push. They run from the repository root, after the set-up above,
with the WebAssembly core built. The fuzz smoke test, CodeQL, OSV-Scanner and
the secret scan run in CI only, and the nightly and weekly workflows add the
security end-to-end tests, the benchmarks, longer fuzzing and mutation
testing.

**Rust** (`ci.yml` and the app job of `desktop.yml`). Both workflows build
with `RUSTFLAGS='-D warnings'`, so a compiler warning fails CI even where no
Clippy line looks: in the fuzz targets, and in code that only the wasm32
check compiles. The first line sets it for your shell; changing `RUSTFLAGS`
rebuilds everything, so you may prefer a `CARGO_TARGET_DIR` of its own for
these checks.

```sh
export RUSTFLAGS='-D warnings'               # as in CI
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
rustup target add x86_64-pc-windows-gnu      # once, for the next line
cargo clippy --locked --all-targets --target x86_64-pc-windows-gnu -- -D warnings
cargo clippy --locked -p blocks2cpp-desktop --all-targets -- -D warnings
cargo clippy --locked -p blocks2cpp-desktop --all-targets --features e2e-hooks -- -D warnings
cargo test --locked                          # unit, property and g++-backed tests
cargo test --locked -p blocks2cpp-desktop    # B2C_REQUIRE_GXX=1: fail without g++
cargo test --locked -p blocks2cpp-desktop --features e2e-hooks
cargo check --locked --manifest-path fuzz/Cargo.toml
cargo check --locked --target wasm32-unknown-unknown -p b2c-ir -p b2c-model -p b2c-catalog -p b2c-lang -p b2c-codegen -p b2c-core-wasm
python3 tools/check-layering.py              # which crate may use which
cargo deny --all-features --locked check     # the dependency policy (cargo-deny)
cargo llvm-cov --locked --no-report          # the tests again, measured (cargo-llvm-cov), then the gates:
cargo llvm-cov report --summary-only --fail-under-lines 80
cargo llvm-cov report -p b2c-ir -p b2c-model -p b2c-catalog -p b2c-lang -p b2c-codegen --summary-only --fail-under-lines 90
```

**Generated files and docs** (the docs job of `ci.yml`, and `pages.yml`):

```sh
python3 tools/check-diagnostics-docs.py      # every diagnostic code is documented
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps
cargo test --locked -p b2c-ipc --features ts --test generate   # IPC types and isolation allowlist
cargo test --locked -p b2c-catalog --test export               # the catalog, then from it
pnpm --filter @blocks2cpp/catalog-gen run generate             # the Blockly definitions and block reference
git status --short                           # the three lines above must change nothing
pnpm exec markdownlint-cli2                  # every Markdown file
node site/build.mjs --check-repo-markdown    # links and heading anchors in all Markdown
pnpm --filter @blocks2cpp/site test          # the anchor check's own tests
python3 tools/check-build-docs.py            # BUILDING.md's pinned versions, written nowhere else
python3 tools/check-build-docs.py --self-test
pnpm run site:build                          # the specification website
lychee --config lychee.toml './**/*.md'      # external links
```

**Frontend** (`desktop.yml`):

```sh
pnpm desktop:check                           # TypeScript, ESLint and Prettier, in every frontend package
python3 tools/check-package-layering.py      # which package may use which
python3 tools/check-package-layering.py --self-test
pnpm -r --if-present run test:coverage       # every package's tests and coverage gate, the isolation tests included
pnpm --filter @blocks2cpp/desktop run build  # frontend build
pnpm --filter @blocks2cpp/desktop exec vitest run --config e2e/vitest.e2e.config.ts --project support
pnpm --filter @blocks2cpp/desktop exec vitest run --config e2e/bench/vitest.bench.config.ts --project bench-unit
pnpm --filter @blocks2cpp/desktop exec vitest run --config e2e/visual/vitest.visual.config.ts --project visual-unit
python3 tools/bench-compare.py --self-test
python3 tools/bench-compare.py check-accepted apps/desktop/e2e/bench/accepted.json
pnpm audit                                   # advisories in the npm dependencies
```

In one package: `pnpm --filter @blocks2cpp/desktop run test` runs Vitest
(happy-dom), headless, then `test:isolation`, the isolation hook's validator
(`node --test`); `test:coverage` is the same with the 75% line gate.
[Testing](docs/developer-guide/testing.md) describes the desktop app's tests.

**End-to-end tests** (`desktop.yml`, on Linux and Windows): build the app
under test and run them as
[`apps/desktop/e2e/README.md`](apps/desktop/e2e/README.md) describes; CI runs
`pnpm --filter @blocks2cpp/desktop run e2e --exclude 'specs/security/**'` and
then the visual diff. The security tests, benchmarks and fuzzing run nightly.

**Workflows and repository tools** (`ci.yml`, `workflow-audit.yml`):

```sh
tools/dependency-health.sh --self-test       # the weekly dependency report's script
python3 tools/memory-capped-runner.py --self-test   # the weekly mutation job's test runner
zizmor --persona auditor --offline .github/workflows   # workflow security audit
actionlint                                   # workflow syntax and expressions (not run by CI)
```

**Formatting of the root Markdown** (not run by CI): `README.md`,
`BUILDING.md`, `CHANGELOG.md` and this file are Prettier-formatted, with the
repository's Prettier (`apps/desktop/README.md` is checked by the frontend's
`format:check`):

```sh
pnpm --dir apps/desktop exec prettier --check ../../README.md ../../BUILDING.md ../../CHANGELOG.md ../../CONTRIBUTING.md
```

The specification and the decision records (`docs/spec/`, `docs/adr/`) are
wrapped by hand at about 78 columns and are not Prettier-formatted.

## Definition of done

The pull request template has the checklist (tests, docs, changelog, security
questions, accessibility). CI must be green: the [checks](#checks) above on
Linux and Windows, OSV-Scanner, secret scanning and the website build, plus
CodeQL where code scanning is available
([spec §9.3](docs/spec/09-quality-and-delivery.md#93-continuous-integration)).

## Where things are documented

| Topic                                                                     | Where                                                                                                                        |
| ------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| Installing the tools, building and starting the app and the CLI           | [`BUILDING.md`](BUILDING.md)                                                                                                 |
| Using the app                                                             | [User guide](docs/user-guide/README.md)                                                                                      |
| What Blocks2Cpp is meant to be: architecture, language, formats, security | [Specification](docs/spec/README.md), also as a [website](https://joellogan.github.io/Blocks_To_CPP/)                        |
| Why a technical choice was made                                           | [Decision records](docs/adr/README.md)                                                                                       |
| How the desktop app is built inside, its security settings, dependencies  | [Developer guide](docs/developer-guide/README.md)                                                                            |
| Where the desktop app's code is                                           | [`apps/desktop/README.md`](apps/desktop/README.md)                                                                           |
| The frontend packages and their template                                  | [`packages/README.md`](packages/README.md)                                                                                   |
| End-to-end tests, benchmarks and the visual diff                          | [`apps/desktop/e2e/README.md`](apps/desktop/e2e/README.md)                                                                   |
| Blocks, diagnostics and the command-line tool                             | [Blocks](docs/reference/blocks/README.md), [diagnostics](docs/reference/diagnostics/README.md), [CLI](docs/reference/cli.md) |
| Checks done by hand at a milestone                                        | [Manual tests](docs/manual-tests/README.md)                                                                                  |
| Example projects, fuzzing, the security suites                            | [`examples/`](examples/README.md), [`fuzz/`](fuzz/README.md), [`tests/security/`](tests/security/projects/README.md)         |
| Plans in progress and reports to upstream projects                        | [`docs/plans/`](docs/plans/m2-polish-plan.md), [upstream reports](docs/plans/upstream/README.md)                             |
| The specification website's build                                         | [`site/README.md`](site/README.md)                                                                                           |
| What changed                                                              | [CHANGELOG.md](CHANGELOG.md)                                                                                                 |

## Reporting security issues

Do not open public issues for vulnerabilities; follow [SECURITY.md](SECURITY.md).

## Licence

By contributing you agree that your contributions are licensed under the
[Apache License 2.0](LICENSE).
