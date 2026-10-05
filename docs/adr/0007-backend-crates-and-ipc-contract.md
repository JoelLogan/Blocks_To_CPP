# ADR-0007: Backend crates and a generated IPC contract

* Status: Accepted (confirmed by the owner on 2026-10-05)
* Date: 2026-10-05

## Context

M2 turns the M0 shell into an editor that creates, opens, saves and recovers
projects, decides trust, discovers toolchains, builds and runs programs, and
keeps machine-local settings. That is 36 IPC commands
([02 §2.5](../spec/02-architecture.md#25-ipc-surface)), five machine-local files
and a recovery store ([05 §5.9](../spec/05-project-format.md#59-machine-local-data)),
and long-running build and run sessions that stream to the webview.

Constraints:

* `apps/desktop/src-tauri` holds no business logic
  ([02 §2.3](../spec/02-architecture.md#23-repository-layout)). Anything that
  depends on Tauri needs the WebKitGTK development packages to build and test,
  so logic placed there is hard to test headless and in the GCC containers.
* The CLI shares the build cache and the toolchain list with the app, so the
  code that knows where machine-local data lives cannot sit in the app alone.
* The webview is semi-trusted. Three things must agree on every command:
  the Rust handler that decodes the request, the TypeScript client that sends
  it, and the isolation hook that checks it before the backend sees it
  ([08 §8.8](../spec/08-security.md#88-webview-and-ipc-hardening)). Written
  by hand, they drift, and a drifted hook either blocks valid calls or lets
  malformed ones through.

## Options considered

1. **Everything in `src-tauri`.** No new crates. But the logic needs WebKitGTK
   to compile and test, the thin-adapter rule is broken, and the CLI cannot
   reuse the stores.
2. **One backend crate** with the IPC types, the stores and the services.
   `b2c-build` emits IPC events and the CLI needs the stores, while the services
   need `b2c-build`, so one crate either creates a dependency cycle or forces
   the CLI and `b2c-build` to depend on the whole backend.
3. **Three crates (chosen).** `b2c-ipc` is the contract (request, response,
   event and error types, IDs, limits and the command table; it depends on
   `b2c-ir` and `b2c-model`). `b2c-store` is the machine-local storage
   (directories, atomic writes, bounded reads, settings, recent files, trust,
   recovery snapshots, logs; it depends on `b2c-ir`, `b2c-model` and
   `b2c-process`). `b2c-app` holds the Tauri-free services, one method per
   command (it depends on `b2c-ir`, `b2c-model`, `b2c-build`,
   `b2c-toolchain`, `b2c-store` and `b2c-ipc`).

For the TypeScript types:

1. **Hand-written TypeScript.** No dependency, but it drifts from Rust.
2. **specta with tauri-specta.** Generates from the command functions, but ties
   generation to the Tauri crate and its version, so it needs WebKitGTK and
   cannot run from a Tauri-free contract crate.
3. **ts-rs (chosen)** behind the Cargo feature `ts` of `b2c-ipc` only. A test
   calls ts-rs for each type (no `#[ts(export)]`, so nothing is written
   implicitly) and writes the files deterministically. The pure compiler crates
   gain no dependency: `b2c-ipc` mirrors their shared types (`Diagnostic` and
   the source map) with `From` conversions that keep their existing JSON
   shapes.

## Decision

* The three crates of option 3, with the layering table in
  [02 §2.3](../spec/02-architecture.md#23-repository-layout). `src-tauri` is
  a set of adapters that decode, call `b2c_app::Backend` on Tauri's blocking
  pool, and wrap channels and native dialogs.
* **One command table generates everything else.** `b2c_ipc::COMMANDS` lists
  each command's name, request schema (`ObjectSchema`), channels and response
  type. A generator test (feature `ts`) writes:
  * `packages/ipc-types/src/generated/types.ts`: every DTO, sorted;
  * `packages/ipc-types/src/generated/commands.ts`: `IPC_VERSION`,
    `COMMAND_NAMES` and a typed client over an injected transport;
  * `apps/desktop/src-tauri/isolation/allowlist.generated.js`: the command
    names and argument schemas the isolation hook enforces;
  * `apps/desktop/src-tauri/isolation-tests/samples.generated.json`: one valid
    payload per command, for the hook's tests.

  With `B2C_UPDATE_IPC=1` the test rewrites them; otherwise it compares them
  byte for byte, and CI fails when they are stale. A test also checks that
  each request schema has exactly the keys its serialised sample has, so the
  schema and serde cannot drift.
* **The five places a command appears stay in step**: Tauri's handler list,
  the build script's command manifest (from `COMMAND_NAMES`), the capability
  file, the generated allowlist and the generated client. A desktop test
  asserts they are the same set.
* **IPC types are their own types.** Storage and cache types never cross IPC.
  Fields and enum values are camelCase; `Diagnostic` and `SourceMap` keep the
  JSON the CLI already prints, and `SymbolInfo` has the same JSON in WASM and
  IPC. `IPC_VERSION` is an integer emitted into the generated client and
  checked at startup.
* **Machine-local file locations** ([02 §2.7](../spec/02-architecture.md#27-persistence-locations)):
  `settings.json` and `recent.json` stay in `%APPDATA%\Blocks2Cpp\` on
  Windows. `trust.json` and `toolchains.json` go to
  `%LOCALAPPDATA%\Blocks2Cpp\` instead of `%APPDATA%`, because both are bound
  to this machine ([05 §5.8](../spec/05-project-format.md#58-what-is-deliberately-not-stored-in-a-project)):
  trust records name canonical paths, and toolchain records name compilers and
  their fingerprints. `%APPDATA%` roams with domain profiles, so a trust
  decision made on one machine would follow the user to another. On Linux
  both stay in `$XDG_CONFIG_HOME/blocks2cpp/`, which does not roam. The paths
  are computed by `b2c_store::Dirs`, not by Tauri's path resolver, whose
  folder name is the bundle identifier.

## Consequences

* The whole backend, every command included, is tested headless with
  `cargo test` on Linux and Windows, without WebKitGTK; only the adapters and
  the mock-runtime channel tests need it.
* Adding a command means: a request type and schema in `b2c-ipc`, a method on
  `Backend`, an adapter, and a regeneration. The consistency test and the
  staleness check catch anything forgotten.
* New dependency: ts-rs, optional and never enabled in app builds. It needs a
  justification row in the desktop dependency table and must pass `cargo deny`.
* `tools/check-layering.py`, CODEOWNERS and the layering table gain the three
  crates. `b2c-build` may depend on `b2c-store` and `b2c-ipc`, and the CLI on
  `b2c-store`.
* Only `b2c_store::Dirs` and 02 §2.7 know which folder holds `trust.json`;
  nothing else depends on it.
