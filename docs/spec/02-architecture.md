# 2. System Architecture

> Status: **Draft v0.1** · Related ADRs: [0001](../adr/0001-desktop-shell-tauri.md), [0002](../adr/0002-block-editor-blockly.md), [0003](../adr/0003-rust-core-native-and-wasm.md)

## 2.1 Technology stack

| Layer | Choice | Why |
|-------|--------|-----|
| Desktop shell | **Tauri 2** | Small installers, a Rust backend (memory-safe process and file handling), and a capability-based IPC permission model. It uses the OS webview: WebView2 on Windows, WebKitGTK on Linux. |
| Block editor | **Blockly 12.x** (Apache-2.0, maintained by the Raspberry Pi Foundation since Nov 2025) with the **Zelos** renderer | The de-facto standard visual-programming library. Zelos gives the Scratch look, and Blockly is actively maintained with a growing accessibility effort. |
| Frontend | **TypeScript 5 (strict)**, **React 19**, **Vite**, **Zustand** (state), **Radix UI primitives** (accessible widgets) | Mainstream, well-typed and accessible, with a small dependency footprint. |
| Code view | **CodeMirror 6** (C++ mode, read-only plus the Raw C++ editor) | Lighter than Monaco and CSP-friendly (no worker/blob requirements). |
| Terminal | **xterm.js** | A real terminal emulator, so interactive programs behave exactly as they would in a console. |
| Compiler core | **Rust (edition 2024)** crates, compiled **natively** for the backend and CLI and to **WebAssembly** for the in-editor live preview | One implementation of parsing, analysis and codegen, with no drift between preview and build. Memory-safe, fuzzable and fast. |
| Process / PTY | `portable-pty` (ConPTY on Windows, openpty on Linux); Job Objects (Windows) and process groups (Linux) | Correct interactive I/O and reliable whole-tree termination. |
| Package managers | **pnpm 11** (workspace, lockfile, minimum release age, build-script allowlist) and **Cargo** | Supply-chain hardening by default. See [08-security.md §8.9](08-security.md#89-supply-chain). |

## 2.2 High-level component diagram

```
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
│  │   commands/   thin IPC handlers: input validation, opaque handles            │                            │
│  │   b2c-build   build orchestration, cache, incremental, sessions              │──► spawns ──► g++ (system) │
│  │   b2c-toolchain  discovery, probing, argv construction, diagnostics parsing  │                            │
│  │   b2c-process    spawn, PTY, Job Objects / process groups, limits            │──► spawns ──► user program │
│  │   b2c-core (native) authoritative validate → analyse → generate              │               (in PTY)     │
│  │   settings, trust store, recent files, autosave, logs                        │                            │
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

```
Blocks_To_CPP/
├── apps/
│   └── desktop/
│       ├── src/                    # React + TypeScript frontend
│       │   ├── app/                #   shell, routing, layout, commands, keybindings
│       │   ├── editor/             #   Blockly integration, BDM sync, quick-insert
│       │   ├── panels/             #   code view, problems, console, build output
│       │   ├── features/           #   project, toolchain, settings, trust, export
│       │   ├── i18n/               #   message catalogs
│       │   └── lib/                #   IPC client (typed), utilities
│       ├── src-tauri/              # Tauri shell (thin): commands, state, capabilities/
│       └── e2e/                    # WebDriver end-to-end tests (tauri-driver)
├── crates/
│   ├── b2c-model/                  # BDM types, schema, limits, migrations          [wasm + native, forbid(unsafe)]
│   ├── b2c-catalog/                # catalog + library-pack loading, type metadata  [wasm + native, forbid(unsafe)]
│   ├── b2c-lang/                   # SAST, types, symbols, lowering, analysis,      [wasm + native, forbid(unsafe)]
│   │                               #   expression parser, lints
│   ├── b2c-codegen/                # CAST, emitter, pretty-printer, includes,       [wasm + native, forbid(unsafe)]
│   │                               #   source maps, export (CMake/Make)
│   ├── b2c-core-wasm/              # wasm-bindgen facade for the frontend           [wasm, forbid(unsafe)]
│   ├── b2c-toolchain/              # g++ discovery, probing, argv, SARIF/JSON/text  [native, forbid(unsafe)]
│   │                               #   diagnostics parsing
│   ├── b2c-process/                # spawning, PTY, Job Objects, process groups,    [native; the ONLY crate that
│   │                               #   limits, event side-channel                     may contain `unsafe`]
│   ├── b2c-build/                  # build/run orchestration, cache, sessions       [native, forbid(unsafe)]
│   └── b2c-cli/                    # `b2c` command-line tool                        [native, forbid(unsafe)]
├── packages/
│   ├── blockly-ext/                # custom fields, renderer theme, connection checker, mutators
│   ├── catalog-gen/                # build-time: catalog TOML → typed TS block definitions + docs
│   └── ipc-types/                  # TS types generated from Rust IPC types (ts-rs / specta)
├── catalog/
│   ├── core/                       # core language blocks (*.toml)
│   └── std/                        # standard-library pack (*.toml)
├── examples/                       # example projects (also used as golden tests)
├── tests/
│   ├── golden/                     # expected generated C++ + expected program output
│   └── fuzz/                       # cargo-fuzz targets
├── docs/
│   ├── spec/                       # this specification
│   ├── adr/                        # architecture decision records
│   ├── user-guide/                 # end-user documentation (built into a static site)
│   └── reference/                  # generated: block reference, diagnostics reference
├── site/                           # specification website generator (GitHub Pages), see site/README.md
├── .github/                        # workflows, dependabot, CODEOWNERS, templates
├── Cargo.toml                      # Cargo workspace
├── pnpm-workspace.yaml             # pnpm workspace
├── rust-toolchain.toml             # pinned Rust toolchain
├── deny.toml                       # cargo-deny policy
├── SECURITY.md
├── CONTRIBUTING.md
└── README.md
```

**Layering rules (enforced in CI by dependency checks):**

* `b2c-model` ← `b2c-catalog` ← `b2c-lang` ← `b2c-codegen` ← (`b2c-core-wasm`, `b2c-build`)
* The compiler crates (`model`, `catalog`, `lang`, `codegen`) do **no I/O**:
  no filesystem, no processes, no clock and no randomness. They are pure
  functions of their inputs, which makes them deterministic,
  WASM-compatible and easy to fuzz.
* Only `b2c-process` may use `unsafe`, and only in modules that call platform
  APIs (Job Objects, PTY). Every `unsafe` block carries a `// SAFETY:`
  justification and needs a second reviewer (CODEOWNERS).
* `apps/desktop/src-tauri` contains no business logic. It adapts IPC to
  `b2c-build` and `b2c-toolchain`.

## 2.4 Data flow

### 2.4.1 Editing (in the webview, no IPC)

```
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

```
Run ▶ ─► frontend sends {projectHandle, BDM snapshot, configName} ─► backend
   backend: re-validate BDM (native b2c-core) ─► trust check ─► generate C++ (authoritative)
          ─► write changed files to private build dir ─► g++ compile (parallel TUs, cached objects)
          ─► link ─► parse SARIF/JSON/text diagnostics ─► map to blocks via source map
          ─► stream progress/diagnostics over IPC channel ─► on success: start run session
```

The backend **never** compiles C++ text received from the webview. It always
regenerates the C++ itself from the BDM. Therefore a compromised webview cannot
widen what gets compiled beyond what the blocks (including Raw C++ blocks,
which are trust-gated) already express.

### 2.4.3 Running

```
backend spawns program in PTY (ConPTY / openpty) inside a Job Object / process group
   PTY output ─► IPC channel (batched ≤ 16 ms) ─► xterm.js
   keystrokes ─► IPC run_input ─► PTY
   optional event side-channel (named pipe / FIFO) ─► runtime-error and trace events ─► block highlighting
   exit ─► exit code / signal / NTSTATUS decoded ─► friendly message
```

## 2.5 IPC surface

All commands are Rust functions with `serde` types. Every request struct uses
`#[serde(deny_unknown_fields)]` and has explicit size limits. TypeScript
bindings are generated, never hand-written. Streaming uses Tauri `Channel`s.

| Command | Request | Response / stream | Notes |
|---------|---------|-------------------|-------|
| `project_new` | `{ template }` | `{ handle, document }` | Templates are bundled resources. |
| `project_open_dialog` | `{}` | `{ handle, document, trust }` | Native dialog **in the backend**. The webview never supplies a path. |
| `project_open_recent` | `{ recentId }` | same | `recentId` refers to the backend's recent-files list. |
| `project_save` | `{ handle, document }` | `{ savedAt, hash }` | Atomic write. Only to the path bound to `handle`. |
| `project_save_as_dialog` | `{ handle, document }` | `{ handle }` | Rebinds the handle to the new path. |
| `project_export_dialog` | `{ handle, document, options }` | progress channel | Export plain C++ + CMake/Make to a dialog-chosen folder. |
| `trust_get` / `trust_grant` / `trust_revoke` | `{ handle }` | `{ trust }` | `trust_grant` shows a **native** confirmation dialog from the backend, so the webview cannot silently grant trust. |
| `toolchain_list` | `{}` | `[Toolchain]` | Cached discovery results. |
| `toolchain_rescan` | `{}` | `[Toolchain]` | |
| `toolchain_add_dialog` | `{}` | `Toolchain` | The user picks a `g++` executable via a native dialog. |
| `toolchain_select` | `{ toolchainId }` | `{}` | |
| `build_start` | `{ handle, document, config }` | `{ buildId }` + channel: progress, diagnostics, finished | One active build per project. A new build cancels the old one. |
| `build_cancel` | `{ buildId }` | `{}` | Kills the compiler process tree. |
| `run_start` | `{ buildId, runOptions }` | `{ runId }` + channel: output bytes, events, exit | Requires a successful build whose content hash matches. |
| `run_input` | `{ runId, data: bytes ≤ 64 KiB }` | `{}` | |
| `run_resize` | `{ runId, cols, rows }` | `{}` | Bounds-checked. |
| `run_stop` | `{ runId }` | `{}` | Kills the whole process tree. |
| `settings_get` / `settings_update` | partial settings | settings | Validated against a schema. Machine-local only. |
| `open_help_link` | `{ linkId }` | `{}` | Opens one of a fixed set of documentation URLs. No arbitrary URLs. |

**Opaque handles.** `handle` is a random 128-bit ID that maps, inside the
backend, to a canonical path the user chose through a native dialog. The
webview cannot name filesystem paths at all, so path traversal through IPC is
structurally impossible.

## 2.6 Process model and concurrency

* **UI thread (webview):** Blockly and React. WASM analysis runs on the main
  thread for small modules. For modules over 2,000 blocks it runs in a
  dedicated Web Worker (the WASM module is loaded from `'self'`, so CSP stays
  intact).
* **Backend:** a Tokio runtime (provided by Tauri). Each build and run is an
  async *session* with a cancellation token. Compiler invocations run in
  parallel up to `min(available_parallelism, 8)` translation units.
* **State:** the backend holds `AppState { projects: HandleMap, builds:
  SessionMap, runs: SessionMap, toolchains, settings, trust }` behind
  fine-grained locks. No lock is held across `.await` on process I/O.

## 2.7 Persistence locations

| Data | Windows | Linux |
|------|---------|-------|
| Settings, trust store, recent files | `%APPDATA%\Blocks2Cpp\` | `$XDG_CONFIG_HOME/blocks2cpp/` |
| Build cache | `%LOCALAPPDATA%\Blocks2Cpp\builds\` | `$XDG_CACHE_HOME/blocks2cpp/builds/` |
| Autosave / crash recovery | `%LOCALAPPDATA%\Blocks2Cpp\recovery\` | `$XDG_STATE_HOME/blocks2cpp/recovery/` |
| Logs (rotating, local only) | `%LOCALAPPDATA%\Blocks2Cpp\logs\` | `$XDG_STATE_HOME/blocks2cpp/logs/` |

All of these directories are created with owner-only permissions (`0700` on
Linux; on Windows they inherit the user-profile ACL, which already excludes
other standard users).

## 2.8 Cross-platform considerations

| Concern | Windows | Linux |
|---------|---------|-------|
| Webview | WebView2 (Evergreen, preinstalled on Win 10/11; the installer bootstraps it if missing) | WebKitGTK 4.1. Known GPU/driver issues are documented with workarounds (e.g. `WEBKIT_DISABLE_DMABUF_RENDERER=1`). |
| PTY | ConPTY (Windows 10 1809+) | `openpty` |
| Process-tree kill | Job Object with `KILL_ON_JOB_CLOSE` | Process group (`CommandExt::process_group(0)`) + `killpg`. cgroup v2 scope when available. |
| Program runtime DLLs | Link with `-static` by default so `.exe` files run standalone | Dynamic linking (standard) |
| Console encoding | IDE init sets the console CP to UTF-8 ([07 §7.6](07-toolchain-build-run.md#76-running-programs)) | UTF-8 locale |
| Executable suffix | `.exe` | none; mode `0700` |
| Path rules | Reserved device names, trailing dots/spaces, ADS `:` and `MAX_PATH` all handled ([08 §8.6](08-security.md#86-filesystem-safety)) | Symlink-safe creation |
| Packaging | MSI + NSIS (Tauri bundler), Authenticode-signed | AppImage, `.deb`, `.rpm`. Flatpak is post-1.0 (it needs `flatpak-spawn --host` to reach the system g++). |
