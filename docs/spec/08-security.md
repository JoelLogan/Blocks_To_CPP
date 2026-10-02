# 8. Security Design and Threat Model

> Status: **Draft v0.1** · Reviewed at every milestone and whenever a trust boundary changes. The reporting policy is in [/SECURITY.md](../../SECURITY.md).

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
|-------|----------------|
| User's files and accounts (everything the user can access) | Running code has full user privileges |
| User's projects | Integrity (no silent corruption or injection), availability |
| Settings, trust store, toolchain selection | Control which code runs and how it is compiled |
| Our release artifacts and update channel | A compromise would reach every user |

| Actor | Capabilities |
|-------|--------------|
| **Project author (untrusted)** | Fully controls the contents of a `.b2c` file, clipboard payloads and library packs the user installs |
| **Remote web content** | None: the app loads no remote content (§8.8) |
| **Dependency / CI attacker** | Could tamper with npm/crates packages, GitHub Actions or build infrastructure |
| **Local user** | Trusted; owns the machine |

```
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
  they came from the Internet (zone 3) get an additional, stronger warning.
* **Re-check on outside change:** the trust record stores a hash of
  security-relevant content: all Raw C++ text, library requirements, pack
  references and defines. If those change **outside the app** (detected at
  load), the project returns to Restricted Mode. Edits inside the app update
  the record.
* **Library packs installed by the user are code.** Installing one shows its
  manifest, block templates and requested libraries, and requires explicit
  confirmation. Packs bundled with the app are part of the signed release.

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
|-------|--------|--------|
| `\` | `\\` | |
| `"` | `\"` | |
| `?` | `\?` | Prevents trigraph sequences (`??/`) if trigraphs were ever enabled |
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
3. Trims trailing whitespace. **If a line then ends with `\`, it appends the
   sentinel ` //`.** A backslash at the end of a line, **even when followed by
   spaces**, splices the next physical line into the comment in GCC. Verified
   with GCC 13: `// note \␠` followed by `x = 2;` silently swallowed the
   assignment. Without this rule, a comment could hide the next statement
   from the compiler while it stays visible in the block view.
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
  visible placeholders. The analyser warns (`W0520`) and g++ runs with
  `-Wbidi-chars=any`.

## 8.5 Compiler invocation safety

| Threat | Mitigation |
|--------|------------|
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

## 8.8 Webview and IPC hardening

**Content Security Policy** (release builds):

```
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
  **not** permit JavaScript `eval`.
* `style-src 'unsafe-inline'` is a **tracked exception**: Blockly injects its
  stylesheet at runtime. Inline *styles* do not execute script. We will remove
  the exception if Blockly supports nonce or constructable-stylesheet
  injection.
* **No remote content:** all assets are bundled, nothing comes from a CDN, and
  there are no remote fonts. Navigation away from the app origin is blocked,
  and new windows are denied. Help links open in the OS browser through
  `open_help_link` with fixed IDs only.

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

**Frontend coding rules** (enforced by ESLint and CI):

* Banned: `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `document.write`,
  `eval`, `new Function`, string-argument `setTimeout`/`setInterval`, and
  React `dangerouslySetInnerHTML` (`eslint-plugin-no-unsanitized`,
  `react/no-danger`, `no-restricted-properties`).
* User content (block text, comments, compiler output, program output) is
  always rendered as **text**: React text nodes, SVG text nodes in Blockly
  fields, and xterm.js cells. Custom Blockly fields and tooltips go through a
  review checklist.
* Help pages are rendered from our Markdown **at build time** with a
  sanitising renderer. No Markdown or HTML is rendered from project content.
* xterm.js: OSC 52 (clipboard) is disabled. OSC 8 links require
  confirmation and only open `http`/`https`.
* Trusted Types: evaluated in report-only mode on WebView2, and enforced once
  Blockly runs cleanly under it.

## 8.9 Supply chain

| Control | Detail |
|---------|--------|
| **Minimal dependencies** | Each new dependency needs a PR justification covering purpose, maintenance health, licence, size and transitive count. Prefer the standard library and platform APIs. |
| **Lockfiles** | `Cargo.lock` and `pnpm-lock.yaml` are committed. CI uses `cargo --locked` and `pnpm install --frozen-lockfile`. |
| **pnpm 11 hardening** | `minimumReleaseAge: 10080` (7 days) to avoid freshly published compromised versions; `allowBuilds` allowlist (dependency install scripts are otherwise blocked); `blockExoticSubdeps: true` (no git/tarball transitive deps) |
| **cargo-deny** | `advisories` (deny), `licenses` (allowlist), `bans` (duplicate/forbidden crates), `sources` (crates.io only) |
| **Vulnerability scanning** | RustSec via `cargo-deny`/`cargo-audit`, `pnpm audit`, **OSV-Scanner** across both ecosystems, **CodeQL** (JS/TS, Rust, Actions), GitHub Dependabot alerts |
| **Updates** | Dependabot for cargo, npm and github-actions (weekly, grouped). Security updates are raised immediately and are merged with priority after review. |
| **CI hardening** | Actions pinned by full commit SHA; `permissions: {}` by default with per-job least privilege; `persist-credentials: false`; no `pull_request_target` with PR checkout; **zizmor** workflow audits; **OpenSSF Scorecard** |
| **Secrets** | gitleaks in CI, GitHub secret scanning + push protection. Signing keys live only in a protected `release` environment with required reviewers. |
| **Reproducibility** | Pinned Rust (`rust-toolchain.toml`) and Node/pnpm (`packageManager`, `.node-version`) versions; `SOURCE_DATE_EPOCH`; `--remap-path-prefix` |
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
* Local logs rotate (5 × 5 MB) and contain no project contents. Paths appear
  only at debug log level.
* *Help → Export diagnostics bundle* collects logs, toolchain info and
  settings (no project content unless ticked) into a file the user can review
  before sharing.

## 8.12 Threat summary

| ID | Threat | Vector | Mitigations | Residual |
|----|--------|--------|-------------|----------|
| T1 | Code runs on project open | Malicious `.b2c` | No build/run on open; Restricted Mode; parser limits | — |
| T2 | Code runs at compile time | Hostile Raw C++, flags | Trust gate before build; no flags in projects; argv from enums | User trusts a malicious project |
| T3 | Injection via block text | Crafted strings/comments/identifiers | Typed encoders (§8.4), re-printing from AST, property tests and fuzzing | Generator bug (mitigated by tests) |
| T4 | Hidden code (Trojan Source, line splices) | Bidi/invisible chars, trailing `\` in comments | §8.4.2–8.4.3 encoders; visible placeholders; `-Wbidi-chars=any` | — |
| T5 | Parser DoS / memory exhaustion | Huge or deep JSON, duplicate keys | Size/depth/count limits before allocation; flat statement arrays | — |
| T6 | Prototype pollution (frontend) | `__proto__` keys | Keys rejected; data validated in WASM into fresh typed objects | — |
| T7 | XSS in webview | Project text, compiler/program output | Text-only rendering, strict CSP, ESLint bans, isolation pattern | Webview engine bugs |
| T8 | IPC abuse after XSS | Calls to backend commands | Opaque handles; no path parameters; native dialogs for trust and flag changes; backend re-validation | — |
| T9 | Path traversal / overwrite | Module names, export, symlinks | Name validation; containment checks; no-follow; manifest-based export | — |
| T10 | Binary planting | `g++.exe` in project folder or CWD | Never searched; absolute canonical paths; fingerprinting | Same-user malware |
| T11 | Environment tampering | `CPATH`, `LD_PRELOAD`, … | Allowlisted environment for the compiler | — |
| T12 | Runaway compiler/program | Template bombs, infinite loops, fork bombs | Limits; Job Objects/cgroups; Stop kills the tree; flood protection | Process-group escape without cgroups (Linux) |
| T13 | Malicious library pack | Templates, link requests | Pack-load validation; trust confirmation; no raw flags | User trusts a malicious pack |
| T14 | Debugger script execution | `.gdbinit`, auto-load scripts | `-nx`, `auto-load off`, debuginfod off | — |
| T15 | Dependency compromise | npm/crates | Lockfiles, min release age, build-script allowlist, scanners, review | Undetected zero-day compromise |
| T16 | CI / release compromise | Workflow injection, stolen keys | Pinned actions, least privilege, zizmor, protected environments, attestations | — |
| T17 | Malicious update | Update channel | Signature verification, HTTPS, no downgrade, opt-in | Signing-key compromise |

**Malicious-project regression suite:** `tests/security/projects/` holds a
crafted `.b2c` file for **every** threat above that a file can express:
injection strings, comment splices, bidi text, deep nesting, duplicate keys,
oversized numbers, Windows device names, prototype-pollution keys, and
template abuse in packs. Each has an asserted outcome (rejected, or emitted
safely), and the suite runs on every PR.

## 8.13 Continuous security process

| Cadence | Activity |
|---------|----------|
| **Every PR** | CodeQL · cargo-deny · `pnpm audit` · OSV-Scanner · Clippy (`-D warnings`, security-relevant lints) · ESLint security rules · gitleaks · zizmor (when workflows change) · fuzz smoke run (60 s per target) · escaping property tests · malicious-project suite · security checklist in the PR template (new IPC? new `unsafe`? new dependency? new file write?) |
| **Nightly** | Extended fuzzing (30 min per target, with a persisted corpus) · full OSV scan against the default branch · E2E security tests on Windows and Linux |
| **Weekly** | Dependabot update PRs · OpenSSF Scorecard · review of open security alerts (triaged within the week) |
| **Per milestone** | Threat-model review (this document) · manual review of all new IPC commands, `unsafe` blocks and file/process code paths · dependency licence and health review |
| **Per release** | Release gate (§8.10) · SBOM + provenance · signed artifacts |

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
   only".
4. **Same-user malware** can tamper with anything we store. This is outside
   any user-space application's control.
