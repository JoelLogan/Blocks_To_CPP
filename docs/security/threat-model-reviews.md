# Threat-model reviews

Every milestone ends with a review of the threat model
([spec §8.13](../spec/08-security.md#813-continuous-security-process)): the
code of the milestone is checked against the threats of
[§8.12](../spec/08-security.md#812-threat-summary) and against every trust
boundary that changed. This file records one entry per milestone, newest
first. An entry is not edited after the milestone closes, except to link the
fix of one of its findings.

Each entry gives:

- **Date and commit:** when the review was made, and the commit it read.
- **Scope:** the parts of the code and the processes it looked at, with the
  checks that were run and their results.
- **Threats:** each item of §8.12, and whether the milestone meets it.
- **Findings:** each problem, with its severity (high, medium, low or
  informational), what goes wrong and when, and what was done about it.
- **Accepted residuals:** the risks that stay, and why they are accepted.
- **Open items:** what the next review must look at.

## M2: desktop editor MVP

- **Date:** 2026-10-15.
- **Commit:** `d4dfafb` on branch `ccr-01f82d9b-v70u20`, after wave W5 of
  the M2 polish wave; the review is the plan's item W7-3.
- **Previous review:** none; this is the first recorded review. M0 and M1
  were reviewed in the adversarial reviews of their waves, which left no
  record of this form.

### Scope

New in M2, and therefore reviewed in full:

- the IPC surface: the 36 commands of `crates/b2c-ipc`, their adapters in
  `apps/desktop/src-tauri/src/commands`, the isolation hook and the allowlist
  generated from the command table, the capability and the generated
  permissions, and the Content Security Policy;
- `settings_update` with its `ui.layout` part (polish item W3-4);
- the app's current directory, `<cache>/cwd`, and where the native dialogs
  start (W3-10 and its review fix);
- block upgrades when a document is loaded (W4-1);
- copy and paste through the validated clipboard format;
- `unsafe` code in `crates/b2c-process`;
- the stores of `crates/b2c-store`: file modes, atomic writes, `trust.json`
  and the recovery snapshots;
- the Trusted Types trial;
- the licences of the dependencies added since M1 (`43a1389`, the last
  commit of M1).

Checks run for this review, at `d4dfafb` unless the fix below is named:

| Check | Result |
| --- | --- |
| `cargo deny --locked check licenses bans sources` (cargo-deny 0.20.2) | `bans ok, licenses ok, sources ok`; duplicate versions only warn |
| CI at `d4dfafb`: cargo-deny with advisories, `pnpm audit`, gitleaks, CodeQL, OSV-Scanner | all green |
| `pnpm licenses list --json -r` (all 484 npm packages, development ones included) | no copyleft-only licence; see below |
| `cargo test -p b2c-ipc --features ts` (the decoder, the schemas, and the generated allowlist, samples and TypeScript types against the command table) | all pass |
| `pnpm --filter @blocks2cpp/desktop run test:isolation` (the isolation hook) | 31 tests pass |
| `cargo test -p b2c-store`, with the fix of TM-M2-1 | all pass |
| Desktop workflow at `d4dfafb`: end-to-end tests on Linux and Windows | green |
| Nightly at `bee5c0a` (2026-10-14): E2E security tests | Linux green; Windows did not start (see TM-M2-5) |
| Weekly `cargo-geiger` report (runs at `205ab1d` and `9e9bd35`) | no report: the job counted every crate as failed (TM-M2-3) |
| `cargo geiger` 0.13.0, run locally as the weekly job runs it, for `b2c-process` and `b2c-app` | reports written, exit status 1 (TM-M2-3); numbers below |

The desktop crate (`blocks2cpp-desktop`) was read, not built, for this
review.

### Threats

Status of each threat of §8.12 in M2. *Met* means every mitigation the table
names for it is implemented and tested; *not in M2* means the feature the
threat needs does not exist yet.

| ID | Threat | M2 | Evidence and notes |
| --- | --- | --- | --- |
| T1 | Code runs on project open | Met | Opening never builds or runs. Trust is evaluated on open, reload and restore, after the §5.6 limits and after block upgrades, which keep the security hash. E2E security case 1 opens every file of `tests/security/projects` and sees no compiler start. |
| T2 | Code runs at compile time | Met | `build_start` and `run_start` refuse a restricted project in the backend (`b2c-app/src/build_run.rs`; E2E case 2). No flags in projects. M2 has no Raw C++ block, so a document with one has an error and is never built. The *created here* handle is open for M4 (§8.3.1). |
| T3 | Injection through block text | Met | The typed encoders of M1 are unchanged. Parked blocks are never generated (ADR-0012); a property test shows they never change the C++. |
| T4 | Hidden code | Met | Encoders unchanged since M1; the code panel and the block fields show invisible and bidi characters as placeholders. |
| T5 | Parser denial of service | Met, with a residual | The strict loader's limits apply to files, IPC documents (size checked before parsing) and clipboard payloads. An upgraded document is checked against the limits again. Residual: Tauri parses an IPC body before the handler (§8.12, accepted). |
| T6 | Prototype pollution | Met | The isolation hook refuses `__proto__`, `constructor` and `prototype` keys and anything but plain objects; the backend schema refuses the same keys; `freezePrototype` is on; the WASM core builds fresh typed objects. |
| T7 | XSS in the webview | Met, with a residual | Text-only rendering, the CSP of §8.8 (checked by `src-tauri/tests/consistency.rs`), ESLint bans; E2E case 5. Residual: webview engine bugs. Trusted Types are only in trial (see below). |
| T8 | IPC abuse after XSS | Met, with residuals | See *IPC commands*. Residuals: bodies parsed by Tauri before the handler, argument-less commands reachable around the isolation frame (§8.12), and `build_start` on a *created here* handle (§8.3.1, M4). |
| T9 | Path traversal and overwriting | Met | No command takes a path (opaque `ph_`, `rc_`, `sn_`, `bd_`, `rn_`, `tc_` IDs). Files come from native dialogs or backend locations; writes refuse links at the target and replace a link at the `.bak`. |
| T10 | Binary planting | Met, with the residual of §8.12 | W3-10: the app works in an empty owner-only `<cache>/cwd`; discovery excludes the cache, the current directory and the open projects' folders; the build-time check and *Choose g++ manually…* use the same rule. Residual as §8.12 states (a home-folder or root project's subfolders). |
| T11 | Environment tampering | Met | The compiler's environment is allowlisted (M1, in the mutation scope). |
| T12 | Runaway compiler or program | Met, with the residual of §8.14 | Time, memory, process and output limits; Job Objects on Windows, cgroup v2 scopes or the RSS watchdog on Linux; Stop kills the tree. Residual: without cgroups a daemonising program can outlive Stop. |
| T13 | Malicious library pack | Not in M2 | Packs arrive in M4. |
| T14 | Debugger script execution | Not in M2 | The debugger arrives in M5. |
| T15 | Dependency compromise | Met, with a residual | Lockfiles, pnpm's 7-day minimum release age and build-script allowlist, cargo-deny, OSV-Scanner, `pnpm audit`, Dependabot, the per-dependency justification in `apps/desktop/README.md`. Residual: Cargo has no tool-enforced minimum release age (it is checked by hand). |
| T16 | CI or release compromise | Met for CI; releases not in M2 | Actions pinned by SHA, `permissions: {}` by default, zizmor, Scorecard, gitleaks. Signing, protected release environments and attestations come with releases (M6). |
| T17 | Malicious update | Not in M2 | The updater arrives in M6. |

### IPC commands

The command table (`crates/b2c-ipc/src/commands.rs`) is the single source of
the TypeScript client, the isolation allowlist and its samples, and is
compared by `src-tauri/tests/consistency.rs` with the handler list, the
capability and the generated permission files. Of the 36 commands, 11 take
no arguments (`app_info`, `app_quit`, `project_open_dialog`, `recent_list`,
`recovery_list`, `toolchain_list`, `toolchain_rescan`,
`toolchain_add_dialog`, `toolchain_setup_info`, `build_cache_clear`,
`settings_get`), `app_subscribe` takes only a channel, and 24 take a
`request`.

Each request passes three checks that can only refuse:

1. the isolation hook (`src-tauri/isolation/validate.js`): the command name,
   the exact message and argument keys, plain objects only, no reserved key
   at any depth, string lengths (UTF-16 units), base64 alphabet and decoded
   size, ID formats, integer ranges and enum values;
2. `b2c_ipc::decode` in the backend: the same schema (exact keys, `null` is
   not an absent key, UTF-8 sizes), then serde with `deny_unknown_fields`,
   then the request's own `validate` (base64 contents, ranges again);
3. the backend services: 32 open projects, 8 programs, one dialog at a time,
   one `trust_grant` per project every 2 s, 200 `run_input` calls and 1 MiB
   per second, documents parsed by the strict loader after a size check.

What the requests carry: opaque IDs (`ph_`, `rc_`, `sn_`, `bd_`, `rn_` with
32 hex digits, `tc_` with 16), project documents (at most 33,554,432 units in
the hook and 32 MiB in the backend), program input (base64 of at most 64 KiB),
terminal sizes (2–1000 × 1–1000), `run_ack`'s sequence number (up to
2⁵³ − 1), closed enums (`template`, `config`, `linkId`, `onErrors`) and the
settings patch below. No request carries a path, a URL, a flag or a command.

The capability (`capabilities/main-window.json`) applies to the `main`
window only and lists one `allow-<command>` per command and nothing else: no
`core:*`, dialog, fs, shell, http, process or opener permission. The dialog
plugin is registered from Rust; it depends on `tauri-plugin-fs`, which is
compiled in but never registered, so the dialog plugin's scope extension has
nothing to extend and the webview can call neither plugin. The CSP in
`tauri.conf.json` is the one of §8.8; `withGlobalTauri` is off,
`freezePrototype` on, and `dangerousDisableAssetCspModification` covers
`style-src` only. Navigation is limited to this platform's app origin and new
windows are denied (`src-tauri/src/window.rs`; E2E case 6).

The generated files are current: `cargo test -p b2c-ipc --features ts`
compares the allowlist, the samples and `packages/ipc-types` with what the
command table generates, and it passed.

### `settings_update` and `ui.layout`

`settings_update` takes a partial patch of `codeStyle`, `run`, `console` and
`ui.layout`; `toolchain`, `newProject` and `buildCache` cannot be changed
over IPC. Each layout value is an integer in its range (toolbox 160–1200,
C++ dock 200–960, bottom dock 96–720) or a boolean, checked in the hook, in
the backend schema, again in `SettingsPatch::validate` and again in the
store's `update`; unknown keys are refused at every level. When the file is
read, each value is checked on its own and an invalid one is reset with a
notice. The values only size the layout, as numbers in CSS. The nightly
IPC-abuse test builds its generic cases from the generated samples, so
`settings_update` with its `ui` part gets them end to end (extra and reserved
keys, a missing or malformed request, each top-level field of another type).
Out-of-range sizes, extra keys and wrong types inside `ui.layout` are tested
in the hook's unit tests (`isolation-tests/validate.test.js`) and the
backend's (`b2c-ipc`, `b2c-store`), not end to end (TM-M2-7).

Every update writes the whole `settings.json` (at most 1 MiB, `0600`,
atomic). There is no rate limit: the 500 ms batching is the frontend's.
Script that took over the webview could make the app rewrite the file in a
loop; that costs disk writes, which it can cause in larger amounts with
`project_save` or `recovery_save`, and is accepted (R-5).

### The current directory and the dialogs' start folders

At start the desktop app makes `<cache>/cwd` its current directory
(`enter_working_folder` in `src-tauri/src/lib.rs`): created owner-only one
level at a time without following links (`b2c_store::ensure_private_dir`),
refused if it is a link or a file, and never written to. The cache root comes
from absolute `XDG_CACHE_HOME`/`HOME` or `LOCALAPPDATA` values without `..`
parts. If entering it fails, the app keeps its start folder and discovery
still excludes that folder. On Windows the DLL search order is restricted
before anything else runs, so the current directory never supplies a DLL.

The project dialogs start in the project's own folder, else the documents or
home folder (`project_start_folder`), never in `<cache>/cwd`; on Windows only
the project's folder is passed. *Choose g++* starts in `/usr/bin` or the
system drive's root with no file chosen, and the backend refuses a compiler
in an open project's folder or the cache before running it. The suggested
file name of *Save as* is made from the project name with only letters,
digits, space, `-`, `_` and `.`, trimmed, never a device name.

### Block upgrades on load

`b2c_catalog::upgrade_document` runs after the strict loader has applied the
§5.6 limits and before trust is evaluated: in `load_file` (open, open recent,
reload), in `recovery_restore`, in the WASM `load` and `paste_prepare`, and in
`resolve`. Each upgraded block is checked against the limits again; the whole
document is then checked once more, and if its security hash or security
summary changed, or it no longer loads, every upgrade of the document is
undone and each upgraded block reports `B2C-E0603`. Trust is therefore
evaluated on a document whose security hash equals the file's
(§8.3.1). `upgrade.rs` is in the resolve fuzz target and in the
mutation-testing scope.

### Copy and paste

The clipboard carries the canonical payload as
`application/x-blocks2cpp+json` and the C++ as `text/plain`, through the
webview's own `copy`, `cut` and `paste` events; there is no clipboard
plugin and no IPC command. Only the custom type is read back, at most
32 MiB + 1 units, and it is never parsed in TypeScript: the WASM core's
`paste_prepare` loads it with the strict clipboard loader (the §5.6 limits,
no duplicate keys, `B2C-E0138` for anything that is not clipboard data),
gives every block a fresh ID, upgrades it, and
binds its references by name in the target's scope. The result becomes
Blockly blocks, whose text is rendered as text. Whatever the editor then
saves or builds is parsed again by the backend. The WASM core is not a trust
boundary for the backend; a payload that breaks it only harms the page that
pasted it. Pasted blocks keep a reference whose symbol ID the target declares
with the same kind (projects made from one template share IDs); that is a
correctness matter, documented in `crates/b2c-core-wasm/src/clipboard.rs`.

### `unsafe` code

`unsafe_code` is `forbid` in every crate through the workspace lints, except
two crates at `deny`: `b2c-process`, where only the modules `os`, `platform`
and `pty` and three Windows test files allow it, and `b2c-core-wasm`, where
only the glue that `#[wasm_bindgen]` generates contains it (no hand-written
`unsafe`; `forbid` could not be lifted for the generated code). This review
corrected §8.9, which said `forbid` everywhere else. `b2c-process` denies
Clippy's `undocumented_unsafe_blocks`, so each of its 46 `unsafe` sites
carries a `SAFETY` comment. All were read:

| File | Sites | What they call | Justification (from the `SAFETY` comments, checked) |
| --- | --- | --- | --- |
| `os/windows.rs` | 3 | `MoveFileExW` (`atomic_replace`), `SetDefaultDllDirectories` (`harden_dll_search`), `ShellExecuteW` (`open_https_url`) | NUL-terminated UTF-16 strings owned by the function and alive for the call; valid flag combinations; no caller memory written. `open_https_url` validates the URL before the call (§8.7). |
| `platform/create.rs` | 16 | `CreateProcessW` with an extended startup list, `InitializeProcThreadAttributeList` (twice), `UpdateProcThreadAttribute` (in the `unsafe fn AttributeList::set`, whose contract the callers state), `DeleteProcThreadAttributeList`, `CreatePipe`, `SetHandleInformation`, `DuplicateHandle`, `ResumeThread`, `TerminateProcess`, `WaitForSingleObject`, `GetExitCodeProcess`, and `OwnedHandle::from_raw_handle` for each new handle | Buffers sized by the first `InitializeProcThreadAttributeList` call and pointer-aligned; the attribute list and the values it points to outlive the process creation; each handle is valid, owned once and closed once by `OwnedHandle`. |
| `platform/windows.rs` | 18 | Job objects (`CreateJobObjectW`, `SetInformationJobObject` twice, `AssignProcessToJobObject`, `TerminateJobObject`), the job's completion port (`CreateIoCompletionPort`, `GetQueuedCompletionStatus`), resuming a suspended process's thread (`CreateToolhelp32Snapshot`, `Thread32First`, `Thread32Next`, `OpenThread`, `ResumeThread`), the handle list of a captured run, `GetStdHandle`, and `from_raw_handle` for the job, port, snapshot and thread | Results checked against null or `INVALID_HANDLE_VALUE` before use; information structures initialised and sized exactly; the completion port's "overlapped" value is a process ID and never dereferenced; the standard input handle is duplicated, never closed. |
| `platform/unix.rs` | 1 | `Command::pre_exec` with `prlimit64(RLIMIT_AS)` | Between `fork` and `exec` only one system call through rustix's raw backend, no allocation or locks, a `Copy` limit computed before the fork; lowering a limit needs no privilege. |
| `pty/unix.rs` | 3 | `Command::pre_exec` (`setsid`, `TIOCSCTTY`), `libc::signal` and `sigemptyset` with `pthread_sigmask` | Only async-signal-safe functions in the child; the child has one thread; errors are built without allocating. Resets ignored signals so Ctrl+C and Stop work. |
| `pty/windows.rs` | 5 | `CreatePseudoConsole`, `ResizePseudoConsole`, `ClosePseudoConsole`, and the pseudo-console and handle-list attributes | The console handle is passed as the attribute's value, as documented; the console stays open until the process is created and is closed once, on drop. |

No site was found whose comment does not hold.

`cargo geiger` 0.13.0, run locally on Linux (the host target only, so the
Windows code counts as present but unused): `b2c-process` uses 8 of the 168
`unsafe` expressions in its source and has 1 `unsafe` method (the
attribute-list setter); its dependencies `libc`, `rustix`, `errno` and
`linux-raw-sys` hold the rest of its tree's `unsafe`. The whole backend tree
below `b2c-app` uses 132 of 635 `unsafe` functions, 5,836 of 33,456
expressions, 29 of 851 impls, 2 of 32 traits and 231 of 2,757 methods, all
in third-party crates except `b2c-process`. The other workspace crates of
that tree have no `unsafe`; the tool marks them "missing
`#![forbid(unsafe_code)]`" only because it looks for the attribute in the
source, while the workspace sets the lint in `Cargo.toml`.

### The stores

- **Folders** are created one level at a time, `0700`, never through a link;
  the store's own folder must be a real folder, and is made `0700`, which
  fails unless the user owns it.
- **Atomic writes** (`b2c_store::atomic`): a temporary file in the same
  folder, created exclusively with a random name, written, synced and renamed
  over the target (`MoveFileExW` with write-through on Windows, `rename` and a
  directory sync on Linux). A link or folder at the target is refused; a link
  at the `.bak` is replaced, never followed; the previous version is opened
  without blocking and compared with what was inspected, so a FIFO swapped in
  cannot hang a save. Machine-local files (settings, recent list, trust,
  toolchains, build manifests) and recovery snapshots are `0600`, also when an
  older file was wider.
- **Project file modes** (decision Q13, 10 §10.3; D16(6) of the polish plan):
  a new project file must get the user's default mode, `0666` less the umask.
  The code gave it `0600` (TM-M2-1, fixed by this review); a saved project
  keeps its mode, and so does its `.bak`.
- **`trust.json`** is read with a 4 MiB bound and only as a regular file; a
  missing, oversized or invalid file trusts nothing. Changes are serialised
  across app instances by an exclusive lock on a `0600` lock file. Only the
  backend writes it, in the three cases of §8.3.1.
- **Recovery snapshots** live in the owner-only recovery folder, one folder
  per running instance under an exclusive lock; documents and metadata are
  written as pairs that a crash cannot split, read with bounds and validated
  strictly; a snapshot restores as trusted only under the rules of §8.3.1,
  and in Restricted Mode when its recorded path now resolves elsewhere.
- **Logs** are `0600` in a `0700` folder, rotated at 5 × 5 MiB, and hold no
  project content; paths appear at debug level only.

### Trusted Types trial

The backend adds `Content-Security-Policy-Report-Only: require-trusted-types-for
'script'` to the app's own HTML in every build
(`add_trusted_types_report_only` in `src-tauri/src/window.rs`), and the E2E
jobs of `desktop.yml` and `nightly.yml` write each test's violation count, its
directives, and whether the policy was active, to their job summaries.

No numbers are recorded here (TM-M2-4). The summaries exist only as GitHub
Actions job summaries, which this review could not read, and they expire with
the run. The report left by the last local Linux run on the review machine
(WebKitGTK, which may ignore the header) has no test rows. The Windows E2E
job at `d4dfafb`, the one that measures WebView2, passed, and the trial never
fails a test. The trial stays report-only; the enforcement criterion of the
roadmap (M6: no violations on WebView2 for a milestone) needs a recorded
result.

### Licences of dependencies added since M1

Every Cargo dependency passes cargo-deny's allowlist
(`deny.toml`: Apache-2.0, BSD, BSL-1.0, CC0-1.0, CDLA-Permissive-2.0, ISC,
MIT, MIT-0, MPL-2.0 for unmodified dependencies, Unicode-3.0, Zlib). The
lockfile grew from 478 to 522 crates. The direct dependencies added since
M1:

| Crate | Version | Licence | Where |
| --- | --- | --- | --- |
| `getrandom` | 0.4.3 | MIT OR Apache-2.0 | `b2c-ipc`, `b2c-store` |
| `tracing` | 0.1.44 | MIT | `b2c-build`, `b2c-app`, the desktop app |
| `tracing-subscriber` | 0.3.23 | MIT | the desktop app |
| `tauri-plugin-dialog` | 2.8.1 | Apache-2.0 OR MIT | the desktop app |
| `rfd` | 0.16.0 | MIT | the desktop app, Windows only |
| `notify` | 8.2.0 | CC0-1.0 | `b2c-app` |
| `libc` | 0.2.190 | MIT OR Apache-2.0 | `b2c-process`, Unix only |
| `wasm-bindgen` | 0.2.129 | MIT OR Apache-2.0 | `b2c-core-wasm` |
| `ts-rs` | 12.0.1 | MIT | `b2c-ipc`, feature `ts` only (never in an app build) |
| `criterion` | 0.7.0 | Apache-2.0 OR MIT | `b2c-core-wasm`, development only |

The npm direct dependencies added since M1 are listed with their licences in
`apps/desktop/README.md`: the shipped ones (`zustand`, three `@radix-ui`
packages, five CodeMirror and Lezer packages, `@xterm/xterm`,
`@xterm/addon-fit`, `@blockly/continuous-toolbox`) are MIT or Apache-2.0.
Across all 484 npm packages, development ones included, `pnpm licenses list`
found MIT (419), Apache-2.0 (21), ISC (13), BSD-2-Clause (9), BSD-3-Clause
(7), MPL-2.0 (4: `axe-core` and `eslint-plugin-no-unsanitized` for tests and
lint, `lightningcss` and its Linux binary in the build), `Apache-2.0 OR MIT`
(3), and one each of
0BSD, BlueOak-1.0.0, CC0-1.0, MIT-0, `MIT AND Zlib`, PSF-2.0/Python-2.0
(`argparse`) and `MIT OR GPL-3.0-or-later` (`jszip`, a development
dependency of `selenium-webdriver`, used under MIT). The production set
(`pnpm licenses list --prod`) is MIT, Apache-2.0, `Apache-2.0 OR MIT`, BSD,
ISC, MIT-0 and 0BSD. Unlike Cargo, npm has no licence gate in CI (TM-M2-6).

### Findings

**TM-M2-1 (low): new project files were owner-only, against decision Q13.**
Fixed in this review.

- *What:* the owner decided on 2026-10-10 (Q13 in 10 §10.3; D16(6) of the
  polish plan) that a new project file gets the permissions any new file of
  the user gets. `write_atomic_keeping_mode` created it `0600`, and neither
  05 §5.10 nor 08 §8.6 said what a new project's mode is (polish plan §5
  items 41 and 55 had not been applied).
- *Scenario:* under the usual umask `022`, a student saves a new project into
  a folder shared with a class group; the file is `0600`, so the teacher's
  account cannot open it, while a file the student copies there is `0644`.
  Nothing becomes more exposed than the user's own umask allows.
- *Fix:* `crates/b2c-store/src/atomic.rs` creates the temporary file of a
  new project with mode `0666`, which the system narrows by the umask, and no
  longer resets it; a replaced project keeps its mode; machine-local files
  and recovery snapshots stay `0600`. 05 §5.10 and 08 §8.6 now say so.
  Tests: `atomic::tests::projects_keep_their_mode_and_new_ones_get_the_users_default`
  and `tests/store.rs`'s
  `machine_local_files_become_owner_only_and_projects_keep_their_mode`
  compare a new project's mode with that of a file created the ordinary way;
  both failed before the fix (umask `022`: `0600` against `0644`).

**TM-M2-2 (informational): spec text that no longer matched the code.**
Fixed in this review.

- §8.9 said `unsafe_code` is forbidden everywhere except `b2c-process`'s
  platform modules; `b2c-core-wasm` is at `deny` for `#[wasm_bindgen]`'s
  generated glue. The row now says so, and that the weekly geiger report is
  informational.
- §8.3.1 named the dialog plugin as 2.8.0; the lockfile pins 2.8.1.
- §8.13 named this file without linking it; it now links it, and 09 §9.4 lists
  it with the other documents.
- The adapter of `settings_update`
  (`apps/desktop/src-tauri/src/commands/settings.rs`) still says in its module
  documentation that the webview may change only the code style, the
  Run-on-errors choice and the scrollback; it may also change `ui.layout`.
  Not changed here (the desktop crate was not built in this review); a
  one-line documentation fix.

**TM-M2-3 (low): the weekly `cargo-geiger` report has never produced a
report.** Open.

- *What:* §8.13 lists a weekly `cargo-geiger` report of `unsafe` use. In all
  three weekly runs so far (`205ab1d` on 2026-10-05, `9e9bd35` twice on
  2026-10-12) the `geiger` job of `weekly.yml` reported every crate of the
  workspace as failed, and passed by design (a tool failure is only a
  warning), so its summary holds no report.
- *Cause (reproduced locally with the job's command):* cargo-geiger writes
  the report to standard output and then exits with status 1 whenever it
  warns, and it warns about every dependency file it did not scan: the
  `out/private.rs` that the build scripts of `serde`, `serde_core` and
  `thiserror` generate, and the `README.md` that several crates include as
  documentation. Every workspace crate depends on one of them. The job treats
  any non-zero status as a failure and drops the report.
- *Effect:* no count of `unsafe` in the dependency tree has been recorded
  before this review. The `unsafe_code` lints and review still gate the
  project's own code, so this is a gap in monitoring, not in the gate.
- *Proposed fix* (in `weekly.yml`, for the package that owns it): count a
  run as reported when the report file is not empty, whatever the exit status,
  and add the number of `WARNING:` lines to the summary; fail only on an empty
  report.

**TM-M2-4 (low): the Trusted Types trial's results are not recorded
anywhere durable.** Open.

- *What:* the per-test counts go only to GitHub Actions job summaries, which
  expire with the run, and no milestone record holds them. The roadmap's M6
  criterion ("no violations on WebView2 for a milestone") needs one.
- *Next step:* before M2 closes, copy the Trusted Types table of a green
  Windows E2E job of the polish build into this entry; and have the E2E jobs
  upload `trusted-types.jsonl` as an artifact (or append it to the nightly's
  kept history) so later milestones can cite it.

**TM-M2-5 (informational): the nightly Windows E2E security job did not
run its checks at `bee5c0a`.** Open, tracked with the nightly failures.

- The app did not become ready within 30 s (`window.__B2C_E2E__.ready()`), so
  none of the Windows security cases ran that night; Linux passed. The
  Windows E2E of the Desktop workflow at `d4dfafb` passed. The M2 close needs
  a green Windows run of the security specs on the polish build.

**TM-M2-6 (informational): npm licences have no automated gate.** Open,
proposed for M3.

- cargo-deny enforces an allowlist for crates; for npm packages the licence
  is checked by hand, in the pull request that adds the package and its row in
  `apps/desktop/README.md`. Today's set is acceptable (see above), but a new
  transitive package with a copyleft-only licence would not fail CI. Proposed:
  a check of `pnpm licenses list --prod --json` against the `deny.toml`
  allowlist next to `ci.yml`'s `pnpm audit` job.

**TM-M2-7 (informational): the end-to-end IPC abuse cases stop at the
request's top level.** Open.

- `hookMutations` in `apps/desktop/e2e/specs/security/lib/abuse.ts` replaces
  each top-level field of a request with a value of another type, but does
  not descend into nested objects (`settings_update`'s `codeStyle`, `run`,
  `console` and `ui.layout`). The nested rules run in the same validator and
  decoder that the unit tests cover, so the gap is in end-to-end evidence
  only; polish item W3-4 asked to confirm that the nightly test covers
  `settings_update{ui}`, and it does so only at the top level.
- *Proposed:* let `hookMutations` recurse into nested request objects (an
  unknown key and a value of another type at each level), with a case in
  `helpers.e2e.ts`.

No finding of this review is a vulnerability that a project file, a
clipboard payload or a web page can exploit.

### Accepted residuals

- **R-1** (§8.12, T5 and T8): Tauri parses an IPC message body before the
  handler checks its size; only a compromised webview that bypasses the
  isolation hook can send one. Accepted for M2.
- **R-2** (§8.12, T8): script in the editor's frame can call the 11
  argument-less commands around the isolation frame; each is on the
  allowlist anyway. Accepted for M2; checked nightly.
- **R-3** (§8.3.1, T2 and T8): `build_start` builds the document the editor
  sends, so a *created here* handle could build a document copied from a
  restricted project. M2 builds no Raw C++ and encodes defines and library
  names, so it gains nothing. Must be decided before M4.
- **R-4** (§8.12, T10): a compiler in a subfolder of a home-folder or root
  project folder is found when the user picks it or has it on `PATH`.
- **R-5**: `settings_update`, like `project_save` and `recovery_save`, has
  no rate limit; a compromised webview can make the app write files
  repeatedly within the size limits. Accepted: it can do no more than write
  the user's own files, which it may anyway.
- **R-6** (§8.1): the log file and the trust store's lock are opened after a
  link check, without `O_NOFOLLOW`; in their owner-only folders only the
  same user (out of scope) can race the check.
- **R-7** (§8.14): without cgroups a daemonising program can outlive Stop or
  a crash of the app; trusted programs run with the user's privileges.
- **R-8** (§8.8): WebKitGTK's remote inspector, when whoever sets the app's
  environment turns it on, exposes the page and its IPC.
- **R-9** (T15): no tool enforces a minimum release age for crates; it is
  checked by hand when a version is added.

### Open items for the M3 review

- The Web Worker preview (ADR-0016): the worker's CSP on both webviews, no
  IPC in the worker, and the scope index.
- The results of TM-M2-3 and TM-M2-4, and the Windows security run of
  TM-M2-5.
- Before M4: R-3, and pasting Raw C++ blocks into a trusted project, which
  the in-app edit rule of §8.3 would then trust on the next save.
