# 8. Security Design and Threat Model

> Status: **Draft v0.1** · Reviewed at every milestone and whenever a trust boundary changes. The reporting policy is in [/SECURITY.md](../../SECURITY.md) · Related ADRs: [0007](../adr/0007-backend-crates-and-ipc-contract.md), [0008](../adr/0008-pty-and-containment-in-b2c-process.md), [0010](../adr/0010-wasm-delivery-under-the-csp.md)

## 8.1 Scope and assumptions

Blocks2Cpp is a **development tool**. Its purpose is to compile and run
native code on the user's machine with the user's privileges. Security work
therefore focuses on **making sure nothing runs or changes that the user did
not knowingly ask for**, and on not adding new weaknesses to the user's
system.

**In scope:**

* Malicious or malformed **project files**, clipboard content and library
  packs from untrusted sources (downloads, email, shared drives, classmates)
* **Code injection** through block content into generated C++
* Attacks on the **webview** (content injection) and the **IPC** boundary
* **Compiler-invocation abuse** (flags, environment, binary planting)
* **Filesystem abuse** (path traversal, symlink races, overwriting files)
* **Resource exhaustion** of the app, the compiler or the user's machine
* **Supply-chain** compromise of our dependencies, CI, or update channel

**Out of scope, documented as residual risk:**

* A program that the user **deliberately trusts and runs** can do anything
  the user can. We warn, but do not sandbox, by default.
* Malware already running as the same user. It can modify our settings,
  toolchain or binaries directly.
* Vulnerabilities inside g++ itself. GCC's own security policy states that
  compiling untrusted sources can result in arbitrary code execution in the
  compiler and should be sandboxed. We respond to this by **not compiling
  untrusted projects** (§8.3).
* Physical access, OS compromise, and kernel or webview-engine zero-days
  (mitigated only by keeping the engines updated).

## 8.2 Assets, actors and trust boundaries

| Asset | Why it matters |
| ------- | ---------------- |
| User's files and accounts (everything the user can access) | Running code has full user privileges |
| User's projects | Integrity (no silent corruption or injection), availability |
| Settings, trust store, toolchain selection | Control which code runs and how it is compiled |
| Our release artifacts and update channel | A compromise would reach every user |

| Actor | Capabilities |
| ------- | -------------- |
| **Project author (untrusted)** | Fully controls the contents of a `.b2c` file, clipboard payloads and library packs the user installs |
| **Remote web content** | None: the app loads no remote content (§8.8) |
| **Dependency / CI attacker** | Could tamper with npm/crates packages, GitHub Actions or build infrastructure |
| **Local user** | Trusted; owns the machine |

```text
 untrusted ─────────────────────────────────────────────────────────────────────────────────► trusted
 .b2c file / clipboard / pack ─[B1: validator]─► webview ─[B2: IPC + capabilities]─► Rust backend
                                                                                      │
                                                     [B3: trust gate] ─► g++ ─► executable
                                                     [B4: trust gate] ─► run executable (user privileges)
```

## 8.3 Workspace trust and Restricted Mode

**Principle: opening a file is always safe. Building or running it is a
decision the user makes explicitly, once, per project.**

* **Trusted:** projects created in this app on this machine (recorded at first
  save), and projects or folders the user explicitly trusts.
* **Restricted Mode (everything else):** the user can open, view, edit and
  inspect the generated C++ (our own hardened generator only). **Build, Run,
  Debug and Export-and-build are disabled.**
* **Why gate building too:** g++ is not hardened against hostile source (see
  §8.1). Raw C++ could also exhaust resources or read local files into the
  binary at compile time (`#include`, `#embed`, `.incbin`).
* **The trust dialog is native,** raised by the backend (the webview cannot
  fake or bypass it). It explains in plain language that *"Running this
  project lets it do anything a program on your computer can do"*, lists the
  project's Raw C++ blocks, library requirements and file-system blocks, and
  offers **Trust this project**, **Trust everything in this folder**, or
  **Stay in Restricted Mode**.
* **Mark-of-the-Web:** on Windows, files whose `Zone.Identifier` stream says
  they came from the Internet (zone 3 or higher) get an additional, stronger
  warning.
* **Re-check on outside change:** the trust record stores a hash of
  security-relevant content: all Raw C++ text, library requirements, pack
  references and defines. If those change **outside the app** (detected at
  load), the project returns to Restricted Mode. Edits inside the app update
  the record.
* **Library packs installed by the user are code.** Installing one shows its
  manifest, block templates and requested libraries, and requires explicit
  confirmation. Packs bundled with the app are part of the signed release.

### 8.3.1 How trust is decided

* **The trust store** is `trust.json` in the machine folder
  ([05 §5.9](05-project-format.md#59-machine-local-data)). Only the backend
  writes it, and only in three cases: after the native trust dialog, at the
  first save of a project created in the app, and when a trusted project is
  saved in the app (to record its new security hash). A missing or corrupt
  file means nothing is trusted.
* **The security hash** (`b2c_model::security_hash`) is SHA-256 over the
  ASCII bytes `b2c-trust-v1`, a line feed, and then the compact JSON object
  `{"rawCpp": […], "libraries": […], "packs": […], "defines": […]}`:
  * `rawCpp`: `[blockId, fieldName, text]` for every text field of every block
    whose type starts with `raw.`, in all modules, nested or not, disabled or
    not, sorted by block ID and then field name;
  * `libraries`: the project's library names, sorted;
  * `packs`: the pack references `{id, version}`, sorted by ID and then
    version;
  * `defines`: the project's defines in file order, in their file shape.

  It is stored as lower-case hex in `rawCodeHashAtGrant`. Moving blocks,
  comments and ordinary block edits never change it.
* **Matching.** A project record applies only when both the project ID and
  the canonical path are equal (paths compared case-insensitively on
  Windows). The project is trusted when the hash is equal too. When the ID and
  path match but the hash differs, its security-relevant content changed
  outside the app, and it opens restricted (*changed outside*). A copy of a
  trusted file at another path has no record (*no record*).
* **Folder trust** covers a canonical folder and everything below it,
  compared by path components (case-insensitively on Windows). It stores no
  hash, so a project in a trusted folder is not re-flagged after an outside
  change; the user trusted everything there.
* **Revoking** (`trust_revoke`) removes only the project's own record. A
  project inside a trusted folder stays trusted, and the response says that
  the trust comes from the folder, so the UI can explain why.
* **New projects** (`project_new`) are trusted in memory (*created here*), so
  they can be built and run before the first save; until then their build
  folder is keyed by the project ID and they run in their sandbox folder. The
  first save records trust for the chosen path. *Save as* of a trusted
  project records trust for the new path; *Save as* of a restricted project
  leaves it restricted.
* **Restored snapshots** ([05 §5.10](05-project-format.md#510-saving-and-recovery)):
  a snapshot of a project that was never saved restores as trusted (*created
  here*). A snapshot with a file path restores as trusted only when that
  path's trust record still exists and either its hash equals the snapshot's
  security hash or the snapshot was written while the project was trusted.
  Every other snapshot restores in Restricted Mode.
* **What the dialog lists.** The dialog lists the Raw C++ blocks, the library
  requirements and the file-system blocks (`b2c_model::security_summary`) of
  the latest document the backend received for the project (when it was
  opened, saved, built or snapshotted), and granting trust records that
  document's security hash.
* **The dialog itself** is raised from Rust with `tauri-plugin-dialog`'s Rust
  API; the webview has no dialog permission. It offers the three choices as
  buttons if the pinned version supports custom labels, and otherwise asks two
  native questions in turn (*Trust this project?*, then *This project only,
  or everything in this folder?*). On Linux it uses the GTK3 backend without
  D-Bus. The plugin is a new dependency justified under §8.9; using its
  dialog library (`rfd`) directly is the fallback. Cancelling is the same as
  staying in Restricted Mode, and `trust_grant` then returns the unchanged
  trust. There is at most one `trust_grant` per project every 2 s, and one
  native dialog at a time.
* **Mark-of-the-Web.** The backend reads the file's `Zone.Identifier` stream
  (at most 64 KiB) and parses `ZoneId` in its `[ZoneTransfer]` section
  strictly. A zone of 3 or higher (Internet, Restricted sites) marks the
  project, and the dialog shows the stronger warning. It never changes the
  trust state by itself, and a missing stream or a parse failure means no
  mark.
* **What the UI is told:** `{ state, source, restrictedReason, markOfTheWeb }`,
  with `state` `trusted` or `restricted`, `source` `createdHere`, `project`,
  `folder` or `null`, and `restrictedReason` `noRecord`, `changedOutside` or
  `null`. Trust is evaluated again on every open, reload and restore.

## 8.4 Code injection through block content

**Threat:** a project shows harmless-looking blocks (`print "Hello"`), but
crafted field text breaks out of its C++ context and injects code (e.g. a
string `"); std::system("…"); //`), or hides code from a reviewer.

**Mitigation architecture** (see [06 §6.1](06-compiler-pipeline.md#61-overview)):
all user text reaches the C++ output **only** through typed CAST leaves with
dedicated encoders; expressions are re-printed from parsed ASTs; and the only
unescaped paths are Raw C++ blocks (visible, trust-gated) and catalog templates
(validated at pack load; packs are trust-gated). Each encoder below has
exhaustive unit tests, property tests (proptest) and a fuzz target.

### 8.4.1 Identifiers

An identifier is accepted only if **all** of these hold:

1. It matches `^[A-Za-z][A-Za-z0-9_]{0,63}$`. ASCII only, no leading
   underscore. This removes reserved `_Upper` and global `_lower` names and
   Unicode homoglyph attacks.
2. It contains no `__` (reserved anywhere in C++).
3. It is not a C++ keyword (all standards up to C++26), an alternative token
   (`and`, `or`, `not`, `xor`, `bitand`, `bitor`, `compl`, `and_eq`, `or_eq`,
   `xor_eq`, `not_eq`), or a contextual keyword (`final`, `override`,
   `import`, `module`).
4. It is not a standard or predefined macro name or a common problem name:
   `assert`, `errno`, `NULL`, `EOF`, `stdin`, `stdout`, `stderr`, `offsetof`,
   `EXIT_SUCCESS`, `RAND_MAX`, `INT_MAX`, … and **`linux` / `unix`, which
   GCC predefines as macros in `gnu++` modes**. The list lives in the catalog
   and is tested by compiling a declaration of every allowed sample name
   under each supported standard and mode.
5. It is not reserved by the generator: `main` (except for `main` itself),
   `std`, `b2c`, the `B2C_` prefix, or generator temporaries.
6. Module names are additionally unique **case-insensitively** and are not
   Windows device names (`CON`, `PRN`, `AUX`, `NUL`, `COM0`–`COM9`,
   `LPT0`–`LPT9`), because they become file names.

### 8.4.2 String and character literals

Encoder for `StrLit` (UTF-8 input; **NUL is rejected** at validation):

| Input | Output | Reason |
| ------- | -------- | -------- |
| `\` | `\\` | |
| `"` | `\"` | |
| `?` directly after another `?` | `\?` | The output never contains `??`, so it cannot form a trigraph (`??/`) even if trigraphs were ever enabled. A lone `?` stays as it is, so prompts read naturally (`"Age? "`) |
| newline, tab, carriage return | `\n`, `\t`, `\r` | |
| Other C0 controls U+0001–U+001F and U+007F | **3-digit octal** `\001` | `\x` escapes are *greedy*: `"\x41BC"` is a single, out-of-range escape (verified with GCC 13). Octal escapes stop at 3 digits. |
| Bidi controls (U+061C, U+200E–U+200F, U+202A–U+202E, U+2066–U+2069), all format characters (Unicode category Cf: zero-width spaces/joiners, U+FEFF, …), C1 controls U+0080–U+009F, U+0085, U+2028, U+2029 | `\uXXXX` universal character names | Defeats **Trojan Source** (CVE-2021-42574) visual reordering and invisible text, while preserving the exact runtime string |
| Everything else | Literal UTF-8 | Readability (`"héllo ✓"`) |

`CharLit` uses the same table, with `'` → `\'`. Non-ASCII `char` values are
rejected by the analyser, which suggests a string or `char32_t`.

**Property test:** for random Unicode strings (no NUL), the generated program
`std::cout << <encoded>` must output exactly the input's UTF-8 bytes. This
runs in CI under every supported GCC version.

### 8.4.3 Comments

Block comments become `//` lines. The encoder:

1. Splits on **every** line terminator: `\r\n`, `\n`, `\r`, U+0085, U+2028,
   U+2029, vertical tab and form feed. This prevents a "single-line" comment
   from ending early and turning the rest of the text into code.
2. Replaces controls, bidi characters and Cf characters with visible
   placeholders (`<U+202E>`).
3. Trims trailing whitespace. **If a line then ends with `\`, it appends a
   space and the sentinel `//`.** A backslash at the end of a line, **even when followed by
   spaces**, splices the next physical line into the comment in GCC. Verified
   with GCC 13: `// note \␠` followed by `x = 2;` silently swallowed the
   assignment. Without this rule, a comment could hide the next statement
   from the compiler while it stays visible in the block view. A line ending
   in `??/` gets the same sentinel: it would splice the next line if
   trigraphs were enabled, and GCC's `-Wtrigraphs` (part of `-Wall`) warns
   about it.
4. Never emits `/*` block comments, so there is no `*/` termination to
   attack.

Trigraphs are never enabled: the minimum standard is C++17, where they no
longer exist, and `-trigraphs` is on the extra-flags denylist.

### 8.4.4 Numbers

`NumLit` accepts only the C++ literal grammar (decimal, `0x`, `0b`, octal with
an explicit dropdown; digit separators `'`; suffixes `u`, `l`, `ll`, `z`, `f`).
It is range-checked against the target type; overflow is error `E0517`.
`inf`/`nan` are emitted as `std::numeric_limits<T>::infinity()` /
`quiet_NaN()`. The printed form is normalised; it is not the user's raw text.

### 8.4.5 Expression slots and templates

* Slot text is tokenised by our lexer and parsed by our grammar
  ([03 §3.4](03-block-language.md#34-expression-slots)). Any token outside the
  grammar is a parse error. Identifiers must resolve to known symbols. The
  output is re-printed from the AST.
* Library-pack templates are lexed at pack load: no preprocessor, no
  comments, balanced brackets, no `;` in expression templates. Holes receive
  only emitted sub-expressions, parenthesised by precedence
  ([03 §3.11.2](03-block-language.md#3112-template-lowering-library-blocks)).

### 8.4.6 Raw C++ and hidden-text defences

* Raw C++ is opaque by design, and is visible, badged and trust-gated (§8.3).
* The Raw editor and code panel render bidi and invisible characters as
  visible placeholders, and so do block fields and tooltips
  ([04 §4.3](04-user-interface.md#43-code-panel-live-c)). The analyser warns (`W0520`) and g++ runs with
  `-Wbidi-chars=any`. A project file cannot lower `W0520`
  ([05 §5.3](05-project-format.md#53-top-level-structure)); only the user's
  own machine settings can.

## 8.5 Compiler invocation safety

| Threat | Mitigation |
| -------- | ------------ |
| Code execution through flags (`-fplugin=`, `-B`, `-wrapper`, `-specs=`, `@file`, linker plugins) | **Project files cannot contain flags** ([ADR-0005](../adr/0005-no-compiler-flags-in-projects.md)). The backend builds argv from closed enums. Machine-local extra flags go through a denylist **and** a native confirmation ([07 §7.4.5](07-toolchain-build-run.md#745-machine-local-extra-flags-advanced)). |
| File writes through flags (`-o`, `-MF`, `-save-temps`, `-fdump-*`) | Same as above. Output paths are always backend-generated inside the build directory. |
| Shell injection | No shell anywhere. `Vec<OsString>` argv. Clippy `disallowed_methods` bans `std::process::Command` outside `b2c-process`. |
| `.bat`/`.cmd` argument-injection (BatBadBut, CVE-2024-24576) | Only `g++.exe` is accepted. MSRV is well above the Rust fix (1.77.2). |
| Binary planting (`g++.exe` in the project folder or CWD) | Discovery never searches those, skips relative `PATH` entries, and uses canonical absolute paths ([07 §7.2](07-toolchain-build-run.md#72-discovery)). The selected path is shown in the status bar. |
| Toolchain swapped after selection | Fingerprint re-checked before each build. A change triggers a re-probe and a notice. |
| Environment influence (`CPATH`, `GCC_EXEC_PREFIX`, `LD_PRELOAD`, …) | Allowlisted environment ([07 §7.5.2](07-toolchain-build-run.md#752-invocation)) |
| `pkg-config` output injecting flags | Tokenised and allowlisted to `-I`, `-isystem`, `-L`, `-l`, `-D`, `-pthread` |
| Compiler resource exhaustion | Time, memory, process and output limits. Process-tree kill. |

## 8.6 Filesystem safety

* **Opaque handles:** the webview never sends paths. Every path comes from a
  native dialog in the backend or from backend-owned locations
  ([02 §2.5](02-architecture.md#25-ipc-surface)).
* **Generated file names** derive only from validated module names. They are
  never built from free text.
* **Containment:** every write target is canonicalised (its parent directory)
  and verified to lie inside the intended root (build dir, export dir, recovery
  dir) before writing.
* **No link following** when creating directories or files in the cache:
  `symlink_metadata` checks, Windows reparse-point attribute checks,
  `O_NOFOLLOW`/`O_EXCL` for new files, and per-user `0700` roots.
* **Windows specifics:** reserved device names, trailing dots/spaces,
  alternate data streams (`:`), case-insensitive collisions, and long paths
  (the app manifest declares `longPathAware`).
* **Atomic writes** for projects and settings ([05 §5.10](05-project-format.md#510-saving-and-recovery)).
  Exports never overwrite files they did not create (manifest-based).
* **Size-bounded reads:** files are read up to *limit + 1* bytes, so oversized
  files are rejected without being fully loaded.
* **Temporary files:** the `tempfile` crate (random names, exclusive
  creation, `0600`), always in private directories, never in shared `/tmp`
  paths with predictable names.
* **Machine-local data** ([02 §2.7](02-architecture.md#27-persistence-locations)):
  every folder is created one level at a time, without following links, with
  mode `0700` on Linux (Windows: the user-profile ACL), and every file is
  `0600`. Every machine-local file (settings, trust, recent files,
  toolchains, recovery snapshots, build manifests) is written atomically, and
  the `.bak` copy of a project through its own temporary file, so a link
  planted at the `.bak` path is replaced, never followed. Every read is
  size-bounded (1 MiB for settings and recent files, 4 MiB for trust and
  toolchains, the project limit for documents) and reads only regular files.
* **No overwriting changed files:** `project_save` re-hashes the file before
  writing and refuses when it changed outside the app since it was opened or
  last saved ([05 §5.10](05-project-format.md#510-saving-and-recovery)).

## 8.7 Process execution safety

* Programs run only after the trust gate. The backend re-checks trust and the
  build hash in `run_start` and does not rely on the UI.
* Programs run in a Job Object (Windows) or process group/cgroup (Linux) so the
  whole tree is killed on Stop or app exit.
* Children inherit only the PTY/pipe handles they need (explicit handle lists
  on Windows).
* At startup, the app calls `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32
  | LOAD_LIBRARY_SEARCH_APPLICATION_DIR)` on Windows to prevent DLL
  planting against the app itself.
* GDB runs with `-nx` and `set auto-load off`, so `.gdbinit` and Python
  auto-load scripts in a project folder cannot execute
  ([07 §7.8](07-toolchain-build-run.md#78-debugger-later-phase)).
* Event side-channel records are untrusted, schema-validated and
  rate-limited ([07 §7.7](07-toolchain-build-run.md#77-runtime-event-side-channel)).
* **Optional sandboxed run (post-1.0, best-effort):** Linux via
  `bubblewrap` (read-only system binds, private `/tmp`, no network, project
  folder bind) and/or Landlock; Windows via an AppContainer. This is offered
  as *"Run in sandbox"* for untrusted projects and is explicitly labelled as
  reducing risk, not eliminating it.

**How M2 implements these** ([ADR-0008](../adr/0008-pty-and-containment-in-b2c-process.md)):

* On Windows a program is created suspended, assigned to its Job Object
  (`KILL_ON_JOB_CLOSE`) and only then resumed, so it cannot start anything
  outside the job. A program in a pseudo console inherits no handles; in pipe
  mode it gets exactly its three pipe ends through
  `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`. Captured compiler runs move to the same
  path (handle list, suspended start, job); if that does not fit into M2, the
  gap is recorded in the milestone's threat-model review.
* The platform calls sit in `b2c_process::os` as safe wrappers with
  `// SAFETY:` comments: `atomic_replace`, `harden_dll_search` and
  `open_https_url`. The app and the CLI call `harden_dll_search` as the first
  statement of `main`; if it fails, a warning is logged and startup continues.
* `open_https_url` accepts only a URL that starts with `https://` and is
  printable ASCII of at most 2,048 characters without spaces or quotes. On
  Linux it runs `/usr/bin/xdg-open` or `/bin/xdg-open` (an absolute path, never
  `PATH`) with the URL as the only argument, in its own process group; on
  Windows it calls `ShellExecuteW`. It is reached only through
  `open_help_link` (§8.8).
* Every build and run tree is killed when the app quits or exits; see
  §8.14 for what happens after a crash of the app itself.

## 8.8 Webview and IPC hardening

**Content Security Policy** (release builds):

```text
default-src 'none';
script-src 'self' 'wasm-unsafe-eval';
style-src 'self' 'unsafe-inline';
img-src 'self' data:;
font-src 'self';
connect-src ipc: http://ipc.localhost;
worker-src 'self';
base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'
```

* `'wasm-unsafe-eval'` permits compiling our bundled WASM core only. It does
  **not** permit JavaScript `eval`. The core's bytes are embedded in a
  lazily imported script from `'self'`, because `connect-src` forbids
  fetching them ([ADR-0010](../adr/0010-wasm-delivery-under-the-csp.md)).
* `style-src 'unsafe-inline'` is a **tracked exception**: Blockly injects its
  stylesheet at runtime. Inline *styles* do not execute script. We will remove
  the exception if Blockly supports nonce or constructable-stylesheet
  injection.
* **No remote content:** all assets are bundled, nothing comes from a CDN, and
  there are no remote fonts. Navigation away from the app origin is blocked,
  and new windows are denied. Help links open in the OS browser through
  `open_help_link` with fixed IDs only.

**Help links.** `open_help_link` maps a closed set of IDs to fixed URLs and
opens them from Rust (§8.7); the webview has no opener permission and cannot
pass a URL:

| `linkId` | URL |
| --- | --- |
| `msys2Install` | `https://www.msys2.org/` |
| `winlibs` | `https://winlibs.com/` |
| `diagnosticsReference` | The published diagnostics reference of the project's documentation (the exact URL is in `b2c_ipc::LinkId`) |

Links to individual diagnostic codes and bundled offline help come in M5.

**Tauri configuration:**

* The capabilities file grants only our custom commands, and only to the
  `main` window. The `fs`, `shell`, `http` and `process` plugins are **not
  included**, and `dialog` is used from Rust only.
* `withGlobalTauri: false`. DevTools are disabled in release builds.
* The **Isolation pattern** is enabled. An isolated iframe validates every IPC
  message against an allowlist of command names and payload shapes before the
  backend sees it.
* Every command re-validates its input (`deny_unknown_fields`, size limits,
  enum checks) and is rate-limited where relevant (`run_input`).

In detail ([02 §2.5](02-architecture.md#25-ipc-surface)):

* **Capabilities.** `capabilities/main-window.json` applies to the window
  `main` only (local content) and lists exactly one `allow-<command>`
  permission per registered command. It grants no `core:*` permission, so the
  webview can neither `listen()` to events nor use the window API (the dirty
  marker is therefore shown inside the app, not in the title), and no `fs`,
  `shell`, `http`, `process`, `dialog` or opener permission.
* **The isolation hook** loads three scripts: the allowlist generated from
  `b2c-ipc` (`allowlist.generated.js`), the validator (`validate.js`) and the
  hook. For every message it checks the command name and then the exact set
  of top-level keys (Tauri itself ignores extra ones), accepts only plain
  objects, rejects `__proto__`, `constructor` and `prototype` keys at any
  depth, and checks types, string lengths (a document at most 33,554,432
  UTF-16 units, program input at most 87,384 characters), ID formats, integer
  ranges and enum values. Channel arguments must look like
  `__CHANNEL__:<1–10 digits>`. Tauri's internal `plugin:__TAURI_CHANNEL__|fetch`
  with a `null` payload is allowed, because large channel messages are
  delivered through it. Anything else throws, so the message is never sent.
  The hook has its own tests with a valid sample of every command and every
  kind of malformed message.
* **The backend re-validates everything** (`b2c_ipc::decode`) and enforces the
  resource bounds: 32 open projects, 8 running programs, one native dialog at
  a time, one `trust_grant` per project every 2 s, and 200 calls and 1 MiB
  per second of program input.
* `freezePrototype` is on, and `dangerousDisableAssetCspModification` is limited
  to `style-src`, as in M0.

**Frontend coding rules** (enforced by ESLint and CI):

* Banned: `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `document.write`,
  `eval`, `new Function`, string-argument `setTimeout`/`setInterval`, and
  React `dangerouslySetInnerHTML` (`eslint-plugin-no-unsanitized`,
  `no-restricted-syntax` and `no-restricted-properties` rules;
  `eslint-plugin-react` is not used, see [ADR-0009](../adr/0009-e2e-tooling-and-test-seams.md)).
* User content (block text, comments, compiler output, program output) is
  always rendered as **text**: React text nodes, SVG text nodes in Blockly
  fields, and xterm.js cells. Custom Blockly fields and tooltips go through a
  review checklist.
* Help pages are rendered from our Markdown **at build time** with a
  sanitising renderer. No Markdown or HTML is rendered from project content.
* xterm.js: OSC 52 (clipboard) is disabled. OSC 8 links require
  confirmation and only open `http`/`https`. In M2 the confirmation only
  shows the URL with a *Copy link* button; nothing is opened.
* Trusted Types: evaluated in report-only mode on WebView2, and enforced once
  Blockly runs cleanly under it.

**The Trusted Types trial (M2).** The backend adds the response header
`Content-Security-Policy-Report-Only: require-trusted-types-for 'script'` to
the app's own HTML responses, in every build; the enforced CSP is unchanged.
The frontend counts `securitypolicyviolation` events, recording only the
directive, never sample text. E2E builds expose the counts, and the Windows
E2E job writes them to its summary. WebKitGTK may ignore the header, so the
trial is meaningful on WebView2.

## 8.9 Supply chain

| Control | Detail |
| --------- | -------- |
| **Minimal dependencies** | Each new dependency needs a PR justification covering purpose, maintenance health, licence, size and transitive count. Prefer the standard library and platform APIs. |
| **Lockfiles** | `Cargo.lock` and `pnpm-lock.yaml` are committed. CI uses `cargo --locked` and `pnpm install --frozen-lockfile`. |
| **pnpm 11 hardening** (`pnpm-workspace.yaml`) | `minimumReleaseAge: 10080` (7 days) to avoid freshly published compromised versions, with `minimumReleaseAgeStrict` (a too-new version fails the install instead of being exempted); `trustPolicy: no-downgrade` (fail if a version's publishing provenance is weaker than earlier versions'); `allowBuilds` allowlist with `strictDepBuilds` (an unlisted install script fails the install); `blockExoticSubdeps: true` (no git/tarball transitive deps) |
| **cargo-deny** | `advisories` (deny), `licenses` (allowlist), `bans` (duplicate/forbidden crates), `sources` (crates.io only) |
| **Vulnerability scanning** | RustSec via `cargo-deny`/`cargo-audit`, `pnpm audit`, **OSV-Scanner** across both ecosystems, **CodeQL** (JS/TS, Rust, Actions), GitHub Dependabot alerts. An advisory that does not affect Blocks2Cpp (for example, in code nothing calls) may be recorded as a **tracked exception** in `osv-scanner.toml` with the reason and an expiry date, after which the scan fails again until it is re-checked; an advisory that `pnpm audit` reports is also listed in `auditConfig.ignoreGhsas` of `pnpm-workspace.yaml`, with the same reason and review date. High or critical advisories in shipped dependencies are never excepted. |
| **Updates** | Dependabot for cargo, npm and github-actions (weekly, grouped). Security updates are raised immediately and are merged with priority after review. |
| **CI hardening** | Actions pinned by full commit SHA; `permissions: {}` by default with per-job least privilege; `persist-credentials: false`; no `pull_request_target` with PR checkout; **zizmor** workflow audits; **OpenSSF Scorecard** |
| **Secrets** | gitleaks in CI, GitHub secret scanning + push protection. Signing keys live only in a protected `release` environment with required reviewers. |
| **Reproducibility** | Pinned Rust toolchain (`rust-toolchain.toml`), exact pnpm version (`packageManager`) and Node.js major version (`.node-version`, so CI picks up Node security patches); `SOURCE_DATE_EPOCH`; `--remap-path-prefix` |
| **Provenance** | CycloneDX SBOMs (Rust + npm) and GitHub artifact attestations (SLSA build provenance) attached to every release |
| **Unsafe Rust** | `#![forbid(unsafe_code)]` everywhere except platform modules in `b2c-process`. `cargo-geiger` report in CI. Every `unsafe` needs `// SAFETY:` and CODEOWNERS review. |

## 8.10 Releases and updates

* **Windows:** installers and executables are Authenticode-signed (key in an
  HSM-backed signing service).
* **Linux:** AppImage/`.deb`/`.rpm` with published SHA-256 sums signed by the
  release key (minisign). Signed package repositories follow later.
* **Updater:** the Tauri updater plugin over HTTPS, with **Ed25519
  signature verification** against a public key embedded in the app. Update
  checks are opt-in (asked on first run). Downgrades are refused.
* **Release gate:** no open high/critical advisories in shipped dependencies,
  all security workflows green, threat model reviewed for the milestone, and
  the malicious-project regression suite passing (§8.12).

## 8.11 Privacy and logging

* No telemetry and no analytics. There is no network access except opt-in
  update checks and user-clicked links.
* Local logs rotate (5 × 5 MiB) and contain no project contents. Paths appear
  only at debug log level.
* **Log format (M2):** JSON lines with an RFC 3339 UTC timestamp, the level,
  the target, the span fields and the message. The files are `blocks2cpp.log`
  and, after rotation at 5 MiB, `blocks2cpp.1.log` to `blocks2cpp.4.log`
  (five files at most), each `0600` in the `0700` logs folder
  ([02 §2.7](02-architecture.md#27-persistence-locations)). The default level
  is `info`; `B2C_LOG=debug` raises it. No level ever records project
  content: no block text, generated C++, compiler output, or program input or
  output. A panic hook logs the panic's location, not its message.
* *Help → Export diagnostics bundle* collects logs, toolchain info and
  settings (no project content unless ticked) into a file the user can review
  before sharing.

## 8.12 Threat summary

| ID | Threat | Vector | Mitigations | Residual |
| ---- | -------- | -------- | ------------- | ---------- |
| T1 | Code runs on project open | Malicious `.b2c` | No build/run on open; Restricted Mode; parser limits | — |
| T2 | Code runs at compile time | Hostile Raw C++, flags | Trust gate before build; no flags in projects; argv from enums | User trusts a malicious project |
| T3 | Injection via block text | Crafted strings/comments/identifiers | Typed encoders (§8.4), re-printing from AST, property tests and fuzzing | Generator bug (mitigated by tests) |
| T4 | Hidden code (Trojan Source, line splices) | Bidi/invisible chars, trailing `\` in comments | §8.4.2–8.4.3 encoders; visible placeholders; `-Wbidi-chars=any` | — |
| T5 | Parser DoS / memory exhaustion | Huge or deep JSON, duplicate keys | Size/depth/count limits before allocation; flat statement arrays | — |
| T6 | Prototype pollution (frontend) | `__proto__` keys | Keys rejected; data validated in WASM into fresh typed objects | — |
| T7 | XSS in webview | Project text, compiler/program output | Text-only rendering, strict CSP, ESLint bans, isolation pattern | Webview engine bugs |
| T8 | IPC abuse after XSS | Calls to backend commands | Opaque handles; no path parameters; native dialogs for trust and flag changes; isolation hook with a generated allowlist; backend re-validation; rate limits and resource bounds | Oversized bodies parsed by Tauri before the handler (accepted, see below) |
| T9 | Path traversal / overwrite | Module names, export, symlinks | Name validation; containment checks; no-follow; manifest-based export | — |
| T10 | Binary planting | `g++.exe` in project folder or CWD | Never searched; absolute canonical paths; fingerprinting | Same-user malware |
| T11 | Environment tampering | `CPATH`, `LD_PRELOAD`, … | Allowlisted environment for the compiler | — |
| T12 | Runaway compiler/program | Template bombs, infinite loops, fork bombs | Limits; Job Objects/cgroups; Stop kills the tree; flood protection | Process-group escape without cgroups (Linux) |
| T13 | Malicious library pack | Templates, link requests | Pack-load validation; trust confirmation; no raw flags | User trusts a malicious pack |
| T14 | Debugger script execution | `.gdbinit`, auto-load scripts | `-nx`, `auto-load off`, debuginfod off | — |
| T15 | Dependency compromise | npm/crates | Lockfiles, min release age, build-script allowlist, scanners, review | Undetected zero-day compromise |
| T16 | CI / release compromise | Workflow injection, stolen keys | Pinned actions, least privilege, zizmor, protected environments, attestations | — |
| T17 | Malicious update | Update channel | Signature verification, HTTPS, no downgrade, opt-in | Signing-key compromise |

**Accepted IPC exposure.** Tauri parses an IPC message body before our
handler runs, so the backend's own size check (§8.8) comes after that parse.
The isolation hook rejects oversized documents and program input before they
are sent, which leaves this exposure to a compromised webview on the same
machine that bypasses the hook. That is accepted for M2. Reading raw request
bodies in the handlers is adopted only if profiling shows a need.

**Malicious-project regression suite:** `tests/security/projects/` holds a
crafted `.b2c` file for **every** threat above that a file can express:
injection strings, comment splices, bidi text, deep nesting, duplicate keys,
oversized numbers, Windows device names, prototype-pollution keys, and
template abuse in packs. Each has an asserted outcome (rejected, or emitted
safely), and the suite runs on every PR.

## 8.13 Continuous security process

| Cadence | Activity |
| --------- | ---------- |
| **Every PR** | CodeQL · cargo-deny · `pnpm audit` · OSV-Scanner · Clippy (`-D warnings`, security-relevant lints) · ESLint security rules · gitleaks · zizmor (when workflows change) · fuzz smoke run (60 s per target) · escaping property tests · malicious-project suite · security checklist in the PR template (new IPC? new `unsafe`? new dependency? new file write?) |
| **Nightly** | Extended fuzzing (30 min per target, with a persisted corpus) · full OSV scan against the default branch (`osv-scanner.yml`, daily) · E2E security tests on Windows and Linux |
| **Weekly** | Dependabot update PRs · OpenSSF Scorecard · review of open security alerts (triaged within the week) · mutation testing of the encoders and validators · dependency health report · `cargo-geiger` report of `unsafe` use |
| **Per milestone** | Threat-model review (this document) · manual review of all new IPC commands, `unsafe` blocks and file/process code paths · dependency licence and health review |
| **Per release** | Release gate (§8.10) · SBOM + provenance · signed artifacts |

**Nightly E2E security tests** run in the real app on both systems
([09 §9.2](09-quality-and-delivery.md#92-testing-strategy)) and check that:

1. every file in `tests/security/projects/` is rejected or opens in
   Restricted Mode, and no compiler process is ever started;
2. `build_start` and `run_start` called directly through Tauri's internal
   invoke on a restricted project are refused with `restricted`;
3. an unknown command is dropped by the isolation hook;
4. unknown fields, an oversized document, program input over 64 KiB, an
   out-of-range terminal size and forged handles are refused;
5. HTML and script text in a project's strings and comments is shown
   literally and runs nothing (a canary global stays unset);
6. navigation to an external URL and `window.open` are blocked;
7. a trust-relevant change made outside the app (editing `trust.json`, or a
   define in the project file) returns the project to Restricted Mode.

**Vulnerability handling.** Reports come in through GitHub private
vulnerability reporting ([/SECURITY.md](../../SECURITY.md)). Fixes are
developed in a private fork, released with a GitHub Security Advisory (and a
CVE where applicable), and noted in the changelog.

## 8.14 Residual risks (accepted, documented to users)

1. **Trusted code is fully privileged.** Once a user trusts a project, its
   program can do anything they can. Mitigations are clear warnings, Raw C++
   visibility and the optional sandbox (post-1.0).
2. **Webview engine vulnerabilities.** WebView2 is evergreen (auto-updating).
   WebKitGTK depends on the distribution, so we document keeping it updated.
3. **Linux process escape without cgroups.** A program that daemonises can
   outlive *Stop* when cgroup v2 user scopes are unavailable. This is
   documented, and the run status shows when containment is "process group
   only". The same applies when the app itself crashes: the app kills every
   build and run when it quits normally, but it does not use
   `PR_SET_PDEATHSIG`, so after a crash a program can keep running. With
   cgroups, the next start of the app kills the `b2c-*` scopes whose owner has
   died; without them, a process group can outlive the crash.
4. **Same-user malware** can tamper with anything we store. This is outside
   any user-space application's control.
