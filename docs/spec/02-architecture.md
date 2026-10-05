# 2. System Architecture

> Status: **Draft v0.1** · Related ADRs: [0001](../adr/0001-desktop-shell-tauri.md), [0002](../adr/0002-block-editor-blockly.md), [0003](../adr/0003-rust-core-native-and-wasm.md), [0007](../adr/0007-backend-crates-and-ipc-contract.md), [0008](../adr/0008-pty-and-containment-in-b2c-process.md), [0010](../adr/0010-wasm-delivery-under-the-csp.md)

## 2.1 Technology stack

| Layer | Choice | Why |
| ------- | -------- | ----- |
| Desktop shell | **Tauri 2** | Small installers, a Rust backend (memory-safe process and file handling), and a capability-based IPC permission model. It uses the OS webview: WebView2 on Windows, WebKitGTK on Linux. |
| Block editor | **Blockly 12.x** (Apache-2.0, maintained by the Raspberry Pi Foundation since Nov 2025) with the **Zelos** renderer | The de-facto standard visual-programming library. Zelos gives the Scratch look, and Blockly is actively maintained with a growing accessibility effort. |
| Frontend | **TypeScript 5 (strict)**, **React 19**, **Vite**, **Zustand** (state), **Radix UI primitives** (accessible widgets) | Mainstream, well-typed and accessible, with a small dependency footprint. |
| Code view | **CodeMirror 6** (C++ mode, read-only plus the Raw C++ editor) | Lighter than Monaco and CSP-friendly (no worker/blob requirements). |
| Terminal | **xterm.js** | A real terminal emulator, so interactive programs behave exactly as they would in a console. |
| Compiler core | **Rust (edition 2024)** crates, compiled **natively** for the backend and CLI and to **WebAssembly** for the in-editor live preview | One implementation of parsing, analysis and codegen, with no drift between preview and build. Memory-safe, fuzzable and fast. |
| Process / PTY | Our own PTY layer in `b2c-process`: ConPTY (`CreatePseudoConsole`) on Windows, `openpty` through `rustix` on Linux, and a pipe-mode fallback; Job Objects (Windows); process groups and cgroup v2 scopes through `systemd-run` (Linux). See [ADR-0008](../adr/0008-pty-and-containment-in-b2c-process.md). | Correct interactive I/O, and whole-tree containment that is in place before the child runs any code. |
| IPC contract | Rust types in `b2c-ipc`; TypeScript types, a typed client and the isolation allowlist **generated** with `ts-rs` ([ADR-0007](../adr/0007-backend-crates-and-ipc-contract.md)) | One definition of every command, so the backend, the frontend and the isolation hook cannot drift. |
| Package managers | **pnpm 11** (workspace, lockfile, minimum release age, build-script allowlist) and **Cargo** | Supply-chain hardening by default. See [08-security.md §8.9](08-security.md#89-supply-chain). |

## 2.2 High-level component diagram

```text
┌───────────────────────────────── Desktop application (one OS process tree) ──────────────────────────────────┐
│                                                                                                              │
│  ┌──────────────── Webview (untrusted-content zone, strict CSP) ────────────────┐                            │
│  │                                                                              │                            │
│  │  React UI shell ── panels, menus, dialogs, settings, problems, console       │                            │
│  │        │                                                                     │                            │
│  │  Blockly workspace + b2c extensions (custom fields, connection checker,      │                            │
│  │        │          renderer theme, quick-insert, mutators)                    │                            │
│  │        ▼                                                                     │                            │
│  │  Workspace ↔ BDM sync layer ─────────► b2c-core (WASM): validate, analyse,   │                            │
│  │                                        generate preview C++ + source map,    │                            │
│  │                                        parse expression slots, scope queries │                            │
│  └───────────────────────────────┬──────────────────────────────────────────────┘                            │
│                                  │  Tauri IPC (typed commands + channels; capability-restricted;             │
│                                  │  isolation pattern; no fs/shell plugins exposed)                          │
│  ┌───────────────────────────────▼──────────────────────────────────────────────┐                            │
│  │  Rust backend (trusted zone)                                                 │                            │
│  │   src-tauri      thin adapters: decode requests, channels, native dialogs    │                            │
│  │   b2c-ipc        IPC contract: requests, responses, events, errors, limits   │                            │
│  │   b2c-app        services: projects, trust, settings, recent, recovery       │                            │
│  │   b2c-build      build and run sessions, cache, toolchain registry           │──► spawns ──► g++ (system) │
│  │   b2c-toolchain  discovery, probing, argv construction, diagnostics parsing  │                            │
│  │   b2c-process    spawn, PTY, Job Objects / process groups / cgroups, limits  │──► spawns ──► user program │
│  │   b2c-store      settings, trust store, recent files, recovery, logs         │               (in PTY)     │
│  │   b2c-core (native) authoritative validate → analyse → generate              │                            │
│  └──────────────────────────────────────────────────────────────────────────────┘                            │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

**Trust boundaries:**

1. Project file contents → backend parser. Untrusted, fully validated.
2. Webview → backend IPC. Semi-trusted; the backend re-validates everything.
3. Backend → g++. g++ is not hardened against hostile input, so untrusted
   projects are never compiled without trust.
4. Backend → user program. Arbitrary native code, by design; gated by trust.

The full threat model is in [08-security.md](08-security.md).

## 2.3 Repository layout

```text
Blocks_To_CPP/
├── apps/
│   └── desktop/
│       ├── src/                    # React + TypeScript frontend
│       │   ├── app/                #   shell, layout, toolbar, status bar, commands, keybindings
│       │   ├── editor/             #   Blockly integration, BDM sync, live preview, quick-insert
│       │   ├── panels/             #   code view, problems, console, build output
│       │   ├── features/           #   project, recovery, toolchain, settings, trust, export
│       │   ├── i18n/               #   message catalogs
│       │   └── lib/                #   IPC transport for the generated client, utilities
│       ├── src-tauri/              # Tauri shell (thin adapters to b2c-app): commands, channels,
│       │   │                       #   native dialogs, window events, logging, capabilities/
│       │   ├── isolation/          #   isolation hook: generated allowlist + validator
│       │   └── isolation-tests/    #   node --test tests of the hook
│       └── e2e/                    # WebDriver end-to-end tests (selenium-webdriver, tauri-driver)
├── crates/
│   ├── b2c-ir/                     # shared contracts: IDs, diagnostics, typed text [wasm + native, forbid(unsafe)]
│   │                               #   leaves, types, SAST, symbol info, source maps
│   ├── b2c-model/                  # BDM types, schema, limits, migrations,         [wasm + native, forbid(unsafe)]
│   │                               #   security hash, clipboard format
│   ├── b2c-catalog/                # catalog + library-pack loading, type metadata, [wasm + native, forbid(unsafe)]
│   │                               #   toolbox metadata
│   ├── b2c-lang/                   # lowering to the SAST, symbols, analysis,       [wasm + native, forbid(unsafe)]
│   │                               #   scope queries, expression parser, lints
│   ├── b2c-codegen/                # CAST, emitter, pretty-printer, includes,       [wasm + native, forbid(unsafe)]
│   │                               #   source maps, export (CMake/Make)
│   ├── b2c-core-wasm/              # wasm-bindgen facade for the frontend           [wasm + native, deny(unsafe)]
│   ├── b2c-toolchain/              # g++ discovery, probing, argv, SARIF/JSON/text  [native, forbid(unsafe)]
│   │                               #   diagnostics parsing, sanitizer reports
│   ├── b2c-process/                # spawning, PTY, Job Objects, process groups,    [native; the ONLY crate that
│   │                               #   cgroups, limits, OS helpers (os module)        may contain `unsafe`]
│   ├── b2c-ipc/                    # IPC contract: requests, responses, events,     [native, forbid(unsafe)]
│   │                               #   errors, IDs, limits, command table, TS generation (feature `ts`)
│   ├── b2c-store/                  # machine-local data: directories, atomic        [native, forbid(unsafe)]
│   │                               #   writes, settings, recent, trust, recovery, logs
│   ├── b2c-build/                  # build and run sessions, cache and eviction,    [native, forbid(unsafe)]
│   │                               #   toolchain registry
│   ├── b2c-app/                    # Tauri-free backend: one method per IPC command [native, forbid(unsafe)]
│   └── b2c-cli/                    # `b2c` command-line tool                        [native, forbid(unsafe)]
├── packages/
│   ├── ipc-types/                  # TS types and client generated from b2c-ipc (ts-rs)
│   ├── b2c-core-wasm/              # builds the WASM core; loader for the embedded bytes
│   ├── blockly-ext/                # block registration, custom fields, theme, connection checker, mutators
│   └── catalog-gen/                # build time only: catalog.json → TS block definitions + block reference
├── catalog/
│   ├── core/                       # core language blocks (*.toml)
│   ├── std/                        # standard-library pack (*.toml)
│   └── toolbox.toml                # toolbox categories, entries and presets
├── examples/                       # example projects (also used as golden tests)
├── tests/
│   ├── golden/                     # expected generated C++ + expected program output
│   └── security/                   # malicious-project and clipboard suites
├── fuzz/                           # cargo-fuzz targets
├── docs/
│   ├── spec/                       # this specification
│   ├── adr/                        # architecture decision records
│   ├── user-guide/                 # end-user documentation (built into a static site)
│   └── reference/                  # block reference (generated), diagnostics reference, CLI
├── site/                           # specification website generator (GitHub Pages), see site/README.md
├── tools/                          # repository checks (layering, diagnostics docs, …)
├── .github/                        # workflows, dependabot, CODEOWNERS, templates
├── Cargo.toml                      # Cargo workspace
├── pnpm-workspace.yaml             # pnpm workspace
├── rust-toolchain.toml             # pinned Rust toolchain
├── deny.toml                       # cargo-deny policy
├── SECURITY.md
├── CONTRIBUTING.md
└── README.md
```

**Layering rules (enforced in CI by `tools/check-layering.py`):**

| Crate | May depend on (workspace crates) |
| --- | --- |
| `b2c-ir` | nothing |
| `b2c-model` | `b2c-ir` |
| `b2c-catalog`, `b2c-lang` | `b2c-ir`, `b2c-model` |
| `b2c-codegen` | `b2c-ir` |
| `b2c-core-wasm` | `b2c-ir`, `b2c-model`, `b2c-catalog`, `b2c-lang`, `b2c-codegen` |
| `b2c-process` | nothing |
| `b2c-toolchain` | `b2c-ir`, `b2c-model`, `b2c-process` |
| `b2c-ipc` | `b2c-ir`, `b2c-model` |
| `b2c-store` | `b2c-ir`, `b2c-model`, `b2c-process` |
| `b2c-build` | `b2c-ir`, `b2c-model`, `b2c-catalog`, `b2c-lang`, `b2c-codegen`, `b2c-toolchain`, `b2c-process`, `b2c-store`, `b2c-ipc` |
| `b2c-app` | `b2c-ir`, `b2c-model`, `b2c-build`, `b2c-toolchain`, `b2c-store`, `b2c-ipc` |
| `b2c-cli` | `b2c-ir`, `b2c-model`, `b2c-build`, `b2c-toolchain`, `b2c-store` |
| `blocks2cpp-desktop` (`src-tauri`) | `b2c-ir`, `b2c-model`, `b2c-build`, `b2c-toolchain`, `b2c-ipc`, `b2c-store`, `b2c-app` |

Test-only exceptions: `b2c-lang` may use `b2c-codegen` (its end-to-end tests
compile what the analyser accepts) and `b2c-catalog` (its scope and robustness
tests analyse documents as the resolve stage completes or rejects them), and
`b2c-app` may use `b2c-core-wasm`
(the test that the live preview and the build generate identical files).

* `b2c-ir` holds the types that cross a stage boundary: validated IDs,
  diagnostics, the typed text leaves of [§6.8.2](06-compiler-pipeline.md#682-typed-text-parse-dont-validate),
  value types, the Semantic AST, symbol information and source maps. Because
  the SAST lives there, `b2c-lang` and `b2c-codegen` do not depend on each
  other and can change independently.
* The compiler crates (`ir`, `model`, `catalog`, `lang`, `codegen`) and
  `b2c-core-wasm` do **no I/O**: no filesystem, no processes, no clock and no
  randomness. They are pure functions of their inputs, which makes them
  deterministic, WASM-compatible and easy to fuzz. Their external
  dependencies are on an allowlist in the same script.
* Only `b2c-process` may use `unsafe`, and only in modules that call platform
  APIs (Job Objects, PTY, the `os` helpers). Every `unsafe` block carries a
  `// SAFETY:` justification and needs a second reviewer (CODEOWNERS).
  `b2c-core-wasm` uses `deny(unsafe_code)` instead of `forbid`, because the
  code that `#[wasm_bindgen]` generates contains `unsafe` glue; hand-written
  `unsafe` stays forbidden there by review.
* The platform helpers in `b2c_process::os` (atomic replace, DLL search
  hardening, opening a fixed `https` URL) are re-exported as `b2c_build::os`,
  so the app and the CLI do not depend on `b2c-process` directly.
* `b2c-ipc`, `b2c-store` and `b2c-app` do not depend on Tauri, so the whole
  backend is built and tested, and the TypeScript types are generated,
  without the webview libraries ([ADR-0007](../adr/0007-backend-crates-and-ipc-contract.md)).
* `apps/desktop/src-tauri` contains no business logic. It adapts IPC, native
  dialogs and window events to `b2c-app`.

**Package layering rules (enforced in CI by `tools/check-package-layering.py`):**

| Package | May depend on (workspace packages) |
| --- | --- |
| `@blocks2cpp/ipc-types` | nothing (it has no dependencies at all) |
| `@blocks2cpp/b2c-core-wasm` | `ipc-types` (types only) |
| `@blocks2cpp/blockly-ext` | `ipc-types`, `b2c-core-wasm` (and `blockly`) |
| `@blocks2cpp/catalog-gen` | nothing; it runs at build time only and is never a dependency |
| `@blocks2cpp/desktop` (`apps/desktop`) | `ipc-types`, `b2c-core-wasm`, `blockly-ext` |

## 2.4 Data flow

### 2.4.1 Editing (in the webview, no IPC)

```text
user drags block ─► Blockly change event ─► (debounce 50 ms) ─► sync layer serialises workspace → BDM
                                                                       │
                                     b2c-core (WASM): validate → lower → analyse → generate
                                                                       │
              ┌────────────────────────────────┬───────────────────────┴───────────────┐
              ▼                                ▼                                       ▼
    diagnostics → block badges      C++ text + source map → code panel      scope/type info → dropdowns,
                  + Problems list                                            connection checks, quick-insert
```

### 2.4.2 Building (IPC to the backend)

```text
Run ▶ ─► frontend sends build_start {handle, document (BDM JSON text), config} ─► backend
   backend: size check + strict load (native b2c-core) ─► trust check ─► generate C++ (authoritative)
          ─► write changed files to the private build dir ─► g++ compile (parallel TUs)
          ─► link ─► parse SARIF/JSON/text diagnostics ─► map to blocks via source map
          ─► stream progress and diagnostics over the build channel ─► finished {outcome, projectHash}
   frontend: outcome built or upToDate ─► run_start {buildId, runOptions}
```

The backend **never** compiles C++ text received from the webview. It always
regenerates the C++ itself from the BDM. Therefore a compromised webview cannot
widen what gets compiled beyond what the blocks (including Raw C++ blocks,
which are trust-gated) already express.

### 2.4.3 Running

```text
backend spawns the program in a PTY (ConPTY / openpty) inside a Job Object / process group (+ cgroup scope)
   PTY output ─► output channel (raw bytes, batched ≤ 16 ms, numbered) ─► xterm.js ─► run_ack {seq}
   keystrokes ─► run_input (base64, ≤ 64 KiB) ─► PTY
   event side-channel (named pipe / socket, M5) ─► runtime-error and trace events ─► block highlighting
   exit ─► exit code / signal / NTSTATUS decoded ─► exit event with a friendly message
```

## 2.5 IPC surface

The webview reaches the backend only through the commands below. One Rust
crate, `b2c-ipc`, defines the whole contract: every request, response, event
and error type, the opaque IDs, the limits and the command table. The
TypeScript types and typed client (`packages/ipc-types`) and the isolation
hook's allowlist ([08 §8.8](08-security.md#88-webview-and-ipc-hardening)) are
**generated** from it with `ts-rs`, never hand-written, and CI fails when they
are stale ([ADR-0007](../adr/0007-backend-crates-and-ipc-contract.md)). Each
command is a thin Tauri adapter around one method of `b2c_app::Backend`, which
uses no Tauri types. A test checks that the five places a command appears
(Tauri's handler list, the build script's command manifest, the capability
file, the generated allowlist and the generated client) list the same
commands.

### 2.5.1 Requests and responses

* **Envelope.** A command with arguments takes exactly one top-level key,
  `request` (a JSON object), plus its named channel keys (`onEvent`,
  `onOutput`). A command without arguments takes `{}`, or only its channel
  key (`app_subscribe`). Tauri silently ignores extra top-level keys, so the
  isolation hook rejects every key that is not in the command's list.
* **Decoding.** Handlers receive the `request` value as untyped JSON and decode
  it with `b2c_ipc::decode`: `deny_unknown_fields` at every level, camelCase
  keys, closed enums, then range, encoding and ID checks. Any failure is a
  typed `invalidRequest` error, and a rejected request changes no state. The
  backend never relies on the UI or on the isolation hook having checked
  anything.
* **Documents.** A project document crosses IPC as a **string** holding the
  BDM JSON text, in both directions. The backend checks its length (at most
  33,554,432 bytes, the file limit of [05 §5.6](05-project-format.md#56-validation-limits))
  **before** parsing, and then loads it with `b2c_model::load`, the same strict
  loader as for files. The frontend passes every document it receives through
  the WASM loader before building editor state.
* **Types and casing.** IPC types are separate from the storage and cache
  types, which never cross IPC. Fields and enum values are camelCase.
  `Diagnostic` and `SourceMap` keep the JSON shapes the CLI already prints
  ([07 §7.9](07-toolchain-build-run.md#79-command-line-interface)); their
  field names are single words, so they fit the convention. `SymbolInfo` has
  one shape in WASM and IPC ([06 §6.5](06-compiler-pipeline.md#65-stage--names-and-scopes)),
  and its parameter mode keeps the value `read_only`.
* **Dialogs.** A command that shows a native dialog returns
  `{ status: "cancelled" }` or `{ status: "ok", … }`. A cancel is not an error.
* **No paths in requests.** No request carries a filesystem path. Paths come
  only from native dialogs that the backend raises, from the recent list, from
  recovery metadata or from backend-owned locations. Responses may contain
  paths for display only (`displayPath`), and paths are never accepted back.

### 2.5.2 Commands

M2 registers these 36 commands. The *Request* column shows the `request`
object, or `{}` for a command without arguments; channel arguments are named
separately.

| Command | Request | Response | Notes |
| --- | --- | --- | --- |
| `app_info` | `{}` | `{ appVersion, ipcVersion, platform, catalogVersion }` | Called first (§2.5.7). Replaces M0's `app_version`. |
| `app_subscribe` | none; channel `onEvent` | `{}` | The channel for app events (§2.5.3). Called once at startup; a later call replaces the channel. |
| `app_quit` | `{}` | `{}` | Shuts down (§2.6) and exits. |
| `project_new` | `{ template }` | `{ handle, document, trust }` | `template` is `empty` or `helloWorld`, bundled in the binary. The handle has no path and is trusted (`createdHere`). |
| `project_open_dialog` | `{}` | cancelled, or `{ handle, document, trust, fileName, migratedFrom }` | Native open dialog **in the backend**, filtered to `*.b2c`. `migratedFrom` is the file's older `formatVersion`, or `null`. |
| `project_open_recent` | `{ recentId }` | `{ handle, document, trust, fileName, migratedFrom }` | A file that no longer exists gives `notFound` and stays in the list. |
| `project_reload` | `{ handle }` | `{ document, trust, migratedFrom }` | Reads the bound file again after an outside change; trust is evaluated again. |
| `project_save` | `{ handle, document }` | `{ savedAt, hash }` | Atomic write, only to the path bound to `handle` ([05 §5.10](05-project-format.md#510-saving-and-recovery)). |
| `project_save_as_dialog` | `{ handle, document }` | cancelled, or `{ handle, savedAt, hash, fileName }` | Rebinds the handle to the path chosen in a native dialog. |
| `project_close` | `{ handle }` | `{}` | Cancels the project's build, stops its program and forgets the handle. |
| `project_set_dirty` | `{ handle, dirty }` | `{}` | Lets the backend ask before the window closes (§2.6). |
| `recent_list` | `{}` | `{ entries: [{ recentId, projectName, displayPath, lastOpenedAt }] }` | Newest first, at most 10. |
| `recent_remove` | `{ recentId }` | `{}` | |
| `recovery_save` | `{ handle, document }` | `{}` | Writes the handle's recovery snapshot. |
| `recovery_list` | `{}` | `{ snapshots: [{ snapshotId, projectName, savedAt, hasPath }] }` | Only snapshots of app instances that are no longer running. |
| `recovery_restore` | `{ snapshotId }` | `{ handle, document, trust, fileName }` | `fileName` is `null` for a project that was never saved. Trust follows [08 §8.3](08-security.md#83-workspace-trust-and-restricted-mode). |
| `recovery_discard` | `{ snapshotId }` | `{}` | |
| `trust_get` | `{ handle }` | `{ trust }` | |
| `trust_grant` | `{ handle }` | `{ trust }` | Shows the **native** trust dialog from the backend, so the webview cannot silently grant trust. Returns the unchanged trust when the user stays in Restricted Mode or cancels. |
| `trust_revoke` | `{ handle }` | `{ trust }` | Removes the project's own record; trust that comes from a folder remains and is reported. |
| `toolchain_list` | `{}` | `{ toolchains, discovering }` | Cached results; `discovering` is `true` while background discovery runs. |
| `toolchain_rescan` | `{}` | `{ toolchains, discovering }` | Discovers and probes again. |
| `toolchain_add_dialog` | `{}` | cancelled, or `{ toolchain }` | The user picks a `g++` executable in a native dialog; one that cannot be used gives `toolchainRejected`. |
| `toolchain_select` | `{ toolchainId }` | `{}` | The only way to change the selected toolchain in the settings. |
| `toolchain_setup_info` | `{}` | `{ platform, noUsableToolchain, distro }` | `platform` is `windows` or `linux`; `distro` is `{ id, idLike }` from `os-release`, or `null` ([04 §4.6](04-user-interface.md#46-toolchain-setup-experience)). |
| `build_start` | `{ handle, document, config }` + channel `onEvent` | `{ buildId }` | `config` is `debug` or `release`. One active build per project: a new one cancels the old one, and the project's running program is stopped first. |
| `build_cancel` | `{ buildId }` | `{}` | Kills the compiler process tree. A no-op for a finished build. |
| `build_cache_clear` | `{}` | `{ freedBytes, skippedInUse }` | Deletes the build cache except entries in use ([07 §7.5.1](07-toolchain-build-run.md#751-build-directory)). |
| `run_start` | `{ buildId, runOptions: { cols, rows } }` + channels `onOutput`, `onEvent` | `{ runId }` | Requires a successful build of the current content ([07 §7.6.1](07-toolchain-build-run.md#761-preconditions)). Arguments and the working directory come from the built document. |
| `run_input` | `{ runId, data }` | `{}` | `data` is base64 of at most 65,536 bytes. |
| `run_resize` | `{ runId, cols, rows }` | `{}` | Bounds-checked (§2.5.6). |
| `run_stop` | `{ runId }` | `{}` | Kills the whole process tree. |
| `run_ack` | `{ runId, seq }` | `{}` | Flow control for the output channel (§2.5.3). |
| `settings_get` | `{}` | `{ settings, notices }` | The full settings with defaults filled in ([05 §5.9](05-project-format.md#59-machine-local-data)). |
| `settings_update` | `{ codeStyle?, run?, console? }` | `{ settings }` | A strict partial update. Any other key, including `toolchain`, `newProject` and `buildCache`, is rejected. |
| `open_help_link` | `{ linkId }` | `{}` | `msys2Install`, `winlibs` or `diagnosticsReference`, each mapped to a fixed `https` URL. No arbitrary URLs ([08 §8.8](08-security.md#88-webview-and-ipc-hardening)). |

**Not in M2:** `project_export_dialog { handle, document, options }` (export
plain C++ with CMake and Make files to a folder chosen in a native dialog,
with a progress channel) arrives with export in M3. There are deliberately no
commands to set the window title, write to the log, inspect the cache, use
the clipboard or watch a file: the dirty marker is shown inside the app, the
clipboard uses DOM events in the webview
([05 §5.12](05-project-format.md#512-clipboard-format)), and opening a
project starts watching it.

### 2.5.3 Push channels

Every push from the backend uses a Tauri `Channel` passed as a command
argument, never global events: the capability grants no `core:event`
permission, so the webview cannot call `listen()`.

* **App events** (`app_subscribe`), tagged by `kind`:
  `projectChangedOnDisk { handle, deleted }`,
  `toolchainsUpdated { toolchains, discovering }`,
  `settingsNotice { notices }` and `closeRequested`. `toolchain_list` also
  returns `discovering`, so an event sent before the subscription is not
  missed.
* **Build events** (`build_start`, `onEvent`), tagged by `kind`:
  * `progress { stage, done, total }`, with `stage` one of `generate`,
    `compile`, `link`;
  * `diagnostics { items }`;
  * `finished { outcome, projectHash, elapsedMs }`, with `outcome` one of
    `built`, `upToDate`, `projectErrors`, `toolchainProblem`, `cancelled`,
    `failed`, and `projectHash` 64 hex digits or `null`. Exactly one
    `finished` event is sent, and it is always the last.
* **Run output** (`run_start`, `onOutput`): raw bytes, batched at most every
  16 ms, and numbered from 1 in send order.
* **Run events** (`run_start`, `onEvent`), JSON tagged by `kind`:
  * `started { containment, mode, ideHelpers }` first, with `containment` one
    of `jobObject`, `cgroup`, `processGroupOnly` and `mode` one of `pty`,
    `pipes`;
  * `skipped { lines, afterSeq }` when output was dropped
    ([07 §7.6.5](07-toolchain-build-run.md#765-run-limits));
  * `exit { afterSeq, elapsedMs, status, crash, sanitizer, message }` exactly
    once, last. `status` is `{ type: "exited", code }`,
    `{ type: "signaled", signal }`, `{ type: "exception", ntstatus }` or
    `{ type: "stopped" }`. `crash` is one of `memoryAccess`, `stackOverflow`,
    `divisionByZero`, `aborted`, `trap`, `interrupted`, `terminated`,
    `killed`, `outOfMemory`, `heapCorruption`, `missingDll`, `brokenPipe`,
    `resourceLimit`, `other`, or `null`. `sanitizer` is
    `{ tool: "address" | "undefined", kind }` or `null`. `message` is the
    exit text of [07 §7.6.4](07-toolchain-build-run.md#764-exit-decoding).
  * `afterSeq` is the number of output batches sent before the event. The
    frontend applies an event only after it has written that many batches.
* **Flow control.** The terminal acknowledges with `run_ack { runId, seq }`,
  where `seq` is the highest batch it has written, at most every 100 ms. When
  more than 4 MiB is unacknowledged, the backend keeps only the tail
  ([07 §7.6.5](07-toolchain-build-run.md#765-run-limits)).
* Tauri delivers JSON channel messages of 8,192 bytes or more, and raw ones of
  1,024 bytes or more, through its internal command
  `plugin:__TAURI_CHANNEL__|fetch`, which the isolation hook therefore allows.

### 2.5.4 Opaque IDs

| ID | Format | Refers to |
| --- | --- | --- |
| `handle` | `ph_` + 32 lower-case hex digits | An open project, bound to the canonical path the user chose, or to no path for a new project |
| `buildId` | `bd_` + 32 hex digits | A build session and its record |
| `runId` | `rn_` + 32 hex digits | A run session |
| `recentId` | `rc_` + 32 hex digits | An entry of the recent list |
| `snapshotId` | `sn_` + 32 hex digits | A recovery snapshot |
| `toolchainId` | `tc_` + 16 hex digits | A toolchain: the first 16 hex digits of the SHA-256 of its canonical driver path (UTF-8, or UTF-16LE on Windows), so it is stable across restarts |
| `linkId` | closed enum | One of the fixed help URLs |

The random IDs take 128 bits from the operating system's CSPRNG. An unknown
or stale ID gives a typed error (`unknownHandle`, `unknownBuild`, …), and
build records are dropped when their project closes. Because a handle is only
a key into the backend's own table, the webview cannot name a filesystem path
at all, and path traversal through IPC is structurally impossible.

### 2.5.5 Errors

Every command returns either its response or an `IpcError`, tagged by
`code`:

| Code | Arguments | Meaning |
| --- | --- | --- |
| `invalidRequest` | `reason`: `malformed`, `unknownField`, `missingField`, `badEnum`, `badId`, `outOfRange` or `badEncoding`; `field` or `null` | The request does not decode or validate |
| `payloadTooLarge` | `limit` | A document or input is over its limit; it was not parsed |
| `unknownHandle`, `unknownBuild`, `unknownRun`, `unknownRecent`, `unknownSnapshot`, `unknownToolchain` | | The ID does not exist (any more) |
| `invalidDocument` | `diagnostics` | The document does not load (`B2C-E01xx`) |
| `newerFormat` | `needs` | The document was made by a newer version (`B2C-E0108`) |
| `restricted` | | Build or run of a project in Restricted Mode |
| `changedOnDisk` | | Save refused because the file changed outside the app |
| `noPath` | | Save of a project that was never saved |
| `notFound` | | The file of a recent entry or a snapshot no longer exists |
| `staleBuild` | | The build does not match the current content, or its executable changed |
| `buildNotSuccessful` | | `run_start` for a build that produced no program |
| `projectErrors` | `count` | The analyser reports errors |
| `notRunning` | | Input for a program that has ended |
| `rateLimited` | | Too many calls (program input, trust dialog) |
| `busy` | | Another native dialog is open |
| `tooManyHandles`, `tooManySessions` | | A resource bound was reached (§2.6) |
| `toolchainRejected` | `diagnostics` | A compiler picked by the user cannot be used |
| `io` | `kind`: `notFound`, `permissionDenied`, `alreadyExists` or `other` | A file operation failed |
| `internal` | | A bug; details are only in the log |

No error carries a path or project content; internal details, such as I/O
errors with paths, go to the log at debug level. User-facing text comes from
the frontend's message catalog, never from the `Display` text of an error. No
panic crosses the IPC boundary: the adapters catch panics and return
`internal`.

### 2.5.6 Limits

| Limit | Value |
| --- | --- |
| Document | 33,554,432 bytes (33,554,432 UTF-16 units in the isolation hook) |
| `run_input` data | 65,536 bytes (87,384 base64 characters) |
| `run_input` rate | 200 calls and 1 MiB per second per run; more gives `rateLimited` |
| Terminal size | `cols` 2–1000, `rows` 1–1000 |
| Open projects | 32 handles |
| Running programs | 8 in the app; one build and one run per project |
| Native dialogs | one at a time |
| `trust_grant` | one call per handle every 2 s |
| Recent entries | 10 |
| Output | batches at most every 16 ms; at most 4 MiB unacknowledged |
| Scrollback | 1,000–100,000 lines (setting, default 10,000) |

### 2.5.7 Versioning

`b2c_ipc::IPC_VERSION` is an integer (1 in M2), and the generator writes the
same constant into `packages/ipc-types`. `app_info` returns it together with
the app version, the platform and the catalog version, and the frontend shows
a blocking error screen when the two differ. The frontend and the backend
ship together, so a mismatch means a broken build, not a combination to
support. The package version equals the app version. A breaking change to a
request, response, event or error bumps `IPC_VERSION`, and breaking changes
happen only in major releases, which means any `0.x` release until 1.0
([09 §9.5](09-quality-and-delivery.md#95-versioning)).

## 2.6 Process model and concurrency

* **UI thread (webview):** Blockly and React. In M2 the WASM preview runs on
  the main thread behind an asynchronous API. A dedicated Web Worker for large
  modules (over 2,000 blocks; the WASM chunk is loaded from `'self'`, so the
  CSP stays intact) arrives with incremental analysis in M5, or earlier if the
  1,000-block webview benchmark misses its target.
* **Backend:** each command handler runs on Tauri's blocking thread pool. Each
  build and each run is a *session* on its own dedicated thread with a
  cancellation token; the command returns the session's ID at once, and the
  session reports through its channels. Compiler invocations run in parallel
  up to `min(available_parallelism, 8)` translation units.
* **State:** `b2c_app::Backend` holds the open projects (the handle map), the
  build and run sessions, the toolchain registry, the settings, the trust
  store, the recent list and the recovery store, each behind its own lock. No
  lock is held while waiting for a process or a dialog.
* **Resource bounds:** at most 32 open handles (`tooManyHandles`), at most 8
  running programs in the app (`tooManySessions`), one active build and one
  active run per project, and one native dialog at a time (`busy`).
  `build_start` stops the project's running program first, because Windows
  locks a running executable.
* **Closing the window:** the backend handles the window's close request. When
  any project is dirty (as reported by `project_set_dirty`), it keeps the
  window open and sends `closeRequested`; the frontend asks *Save*, *Don't
  save* or *Cancel* and then calls `app_quit`.
* **Shutdown:** `app_quit` and the app's exit event run the same idempotent
  shutdown: cancel every build, stop every program and kill its process tree,
  stop the file watchers, and delete the recovery snapshots of clean projects.

## 2.7 Persistence locations

| Data | Windows | Linux |
| ------ | --------- | ------- |
| Settings (`settings.json`), recent files (`recent.json`) | `%APPDATA%\Blocks2Cpp\` | `$XDG_CONFIG_HOME/blocks2cpp/` |
| Trust store (`trust.json`), toolchains (`toolchains.json`) | `%LOCALAPPDATA%\Blocks2Cpp\` | `$XDG_CONFIG_HOME/blocks2cpp/` |
| Cache root: build cache `builds`, run sandbox `sandbox` | `%LOCALAPPDATA%\Blocks2Cpp\` | `$XDG_CACHE_HOME/blocks2cpp/` |
| Autosave / crash recovery | `%LOCALAPPDATA%\Blocks2Cpp\recovery\` | `$XDG_STATE_HOME/blocks2cpp/recovery/` |
| Logs (rotating, local only) | `%LOCALAPPDATA%\Blocks2Cpp\logs\` | `$XDG_STATE_HOME/blocks2cpp/logs/` |

* `trust.json` and `toolchains.json` describe this machine (canonical paths,
  compiler fingerprints), so on Windows they live in the local, non-roaming
  folder ([ADR-0007](../adr/0007-backend-crates-and-ipc-contract.md), owner
  to confirm). On Linux the configuration folder holds both kinds of file.
* The cache root holds `builds/` ([07 §7.5.1](07-toolchain-build-run.md#751-build-directory))
  and `sandbox/`, the working folders of programs that do not run in their
  project folder ([07 §7.6.2](07-toolchain-build-run.md#762-spawning)). The
  CLI uses the same cache root. Earlier versions used
  `%LOCALAPPDATA%\Blocks2Cpp\cache\` on Windows; that folder is abandoned, not
  migrated, and can be deleted.
* When an XDG variable is unset, the XDG Base Directory defaults apply:
  `~/.config`, `~/.cache` and `~/.local/state`. Relative values are ignored,
  as that specification requires. Values with a `..` part are ignored the
  same way (on Windows an `APPDATA` or `LOCALAPPDATA` with one is an error),
  because the folders are created level by level and a `..` after a link
  leads somewhere other than its text says.
* All of these directories are created with owner-only permissions, one level
  at a time and without following links (`symlink_metadata` checks, and
  reparse-point checks on Windows): `0700` on Linux; on Windows they inherit
  the user-profile ACL, which already excludes other standard users. Every
  file in them is `0600` on Linux. Temporary files are created by `tempfile`
  in the same private directory as their target.
* The paths are computed by `b2c_store::Dirs`, not by Tauri's path resolver,
  whose folder names follow the bundle identifier. E2E builds redirect every
  folder under one test root ([ADR-0009](../adr/0009-e2e-tooling-and-test-seams.md)).

## 2.8 Cross-platform considerations

| Concern | Windows | Linux |
| --------- | --------- | ------- |
| Webview | WebView2 (Evergreen, preinstalled on Win 10/11; the installer bootstraps it if missing) | WebKitGTK 4.1. Known GPU/driver issues are documented with workarounds (e.g. `WEBKIT_DISABLE_DMABUF_RENDERER=1`). |
| PTY | ConPTY (Windows 10 1809+) through `CreatePseudoConsole`, the child started suspended and assigned to the Job Object before it runs ([ADR-0008](../adr/0008-pty-and-containment-in-b2c-process.md)); pipe mode as a fallback | `openpty`; the child is a session leader with the PTY as its controlling terminal; pipe mode as a fallback |
| Process-tree kill | Job Object with `KILL_ON_JOB_CLOSE` | Process group + `killpg`, and a cgroup v2 scope through `systemd-run` when available ([07 §7.5.2](07-toolchain-build-run.md#752-invocation)) |
| Program runtime DLLs | Link with `-static` by default so `.exe` files run standalone | Dynamic linking (standard) |
| Console encoding | The IDE init unit sets the console code pages to UTF-8 ([07 §7.6.3](07-toolchain-build-run.md#763-ide-init-unit)) | UTF-8 locale |
| Executable suffix | `.exe` | none; mode `0700` |
| Path rules | Reserved device names, trailing dots/spaces, ADS `:` and `MAX_PATH` all handled ([08 §8.6](08-security.md#86-filesystem-safety)) | Symlink-safe creation |
| Packaging | MSI + NSIS (Tauri bundler), Authenticode-signed | AppImage, `.deb`, `.rpm`. Flatpak is post-1.0 (it needs `flatpak-spawn --host` to reach the system g++). |
