# 7. Toolchain, Compilation and Execution

> Status: **Draft v0.1** · Crates: `b2c-toolchain`, `b2c-process`, `b2c-build`, `b2c-store`, `b2c-app`, `b2c-cli` · Related ADR: [0008](../adr/0008-pty-and-containment-in-b2c-process.md)

## 7.1 Supported toolchains

| Toolchain | Minimum | Recommended | Notes |
| ----------- | --------- | ------------- | ------- |
| GCC (g++) | **11** | **13+** | 11: C++20 core, `-fdiagnostics-plain-output`. 13: `std::format`, SARIF diagnostics. 14: `<print>`, `-fhardened`. 15: `-fdiagnostics-add-output`. |
| Windows flavours | MinGW-w64 (UCRT or MSVCRT runtime): MSYS2, WinLibs, TDM-GCC 10+, Scoop/Chocolatey `mingw`, Strawberry Perl's bundled GCC | MSYS2 UCRT64 | Cygwin GCC is detected and **warned against** (binaries need `cygwin1.dll`). Legacy mingw.org (`C:\MinGW`) is flagged as outdated. |
| Linux | Distro GCC, `g++-NN` side-by-side versions, RHEL `gcc-toolset-N`, Homebrew-on-Linux | Distro GCC 13+ | |
| Clang | — | — | Detected and labelled *not supported yet*. The driver abstraction (`trait Toolchain`) leaves room for it post-1.0. |

## 7.2 Discovery

Discovery runs at startup (in the background, with cached results shown
immediately), on *Rescan*, and when a cached toolchain's fingerprint changes.
Starting the app never probes: it loads `toolchains.json`, and the background
discovery sends the `toolchainsUpdated` app event when it finishes
([02 §2.5](02-architecture.md#25-ipc-surface)). A rescan probes at most four
compilers at a time.

**Search order:**

1. The user-selected toolchain from settings (validated again before use).
   It is changed only by `toolchain_select`. Before each build its
   fingerprint is checked again, and a changed compiler is probed again
   (`B2C-T1009`). When the selected toolchain is missing or unusable, the
   build falls back to the first usable toolchain in discovery order and
   reports the warning `B2C-T1022`, never silently
   ([toolchain diagnostics](../reference/diagnostics/toolchain.md)).
2. **Windows**
   * `PATH` entries, **absolute paths only**: empty, relative and `.`
     entries are skipped, because on Windows they would resolve against the
     current directory.
   * Well-known locations: `C:\msys64\{ucrt64,mingw64,mingw32}\bin`, MSYS2
     installs found via the uninstall registry key's `InstallLocation`,
     `%USERPROFILE%\scoop\apps\{gcc,mingw,mingw-winlibs}\current\bin`,
     `%ProgramData%\chocolatey\lib\mingw\tools\install\mingw64\bin`,
     `%LOCALAPPDATA%\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.*\mingw64\bin`,
     `C:\TDM-GCC-64\bin`, `C:\Strawberry\c\bin`, `C:\MinGW\bin` (legacy).
   * Only files named exactly `g++.exe` are accepted. **`.bat`/`.cmd` files
     are never executed** (they go through `cmd.exe` parsing; see
     CVE-2024-24576 "BatBadBut").
3. **Linux**
   * `PATH` entries (absolute only): `g++`, then `g++-16` … `g++-11`.
   * `/usr/bin`, `/usr/local/bin`, `/opt/rh/gcc-toolset-*/root/usr/bin`,
     `/home/linuxbrew/.linuxbrew/bin`.

**Never searched:** the project's folder, the process's current directory,
the build cache, removable or network (UNC) paths (a UNC toolchain can be
added manually, with a warning). This prevents *binary planting*, where a
project folder ships its own `g++.exe`. A rescan excludes the folders of all
open projects, the cache root and the current directory. At build time, a
selected or discovered toolchain whose canonical path lies inside the open
project's canonical folder is refused with `B2C-T1002`.

**Adding a compiler manually** (*Choose g++ manually…*,
`toolchain_add_dialog`): the user picks the file in a native dialog. On
Windows only a file named exactly `g++.exe` is accepted (`.bat`, `.cmd` and
anything else give `B2C-T1002`); a network path is accepted with the warning
`B2C-T1020`. The file is canonicalised, probed and stored with the source
*manual*, even when it fails health checks (it is then listed as not usable,
with its problems). Only a file that is refused, or that cannot be probed at
all, is not added: the command returns `toolchainRejected` with the problems
([02 §2.5.5](02-architecture.md#255-errors)).

Each candidate is **canonicalised** (symlinks resolved; on Windows the path is
normalised and checked to be a regular file, not a reparse point into an
unexpected location), deduplicated by canonical path, and **fingerprinted**:
`(canonical path, size, mtime, SHA-256 of the driver binary, -dumpfullversion,
-dumpmachine)`.

The UI names a toolchain by its **toolchain ID**: `tc_` followed by the first
16 hex digits of the SHA-256 of its canonical driver path (UTF-8, or UTF-16LE
on Windows). The ID is stable across restarts, so the selection in the
settings survives them.

## 7.3 Capability probing

Probes run once per fingerprint, in parallel. Each probe has a 10 s timeout and
runs in a private temporary directory (inside the owner-only
`<cache>/probe-tmp/`) with the sanitised environment from §7.5.2. Results are stored in `toolchains.json` in the machine folder
([02 §2.7](02-architecture.md#27-persistence-locations); format in
[05 §5.9](05-project-format.md#59-machine-local-data)), which the app and the
CLI share.

| Probe | Method | Used for |
| ------- | -------- | ---------- |
| Version | `g++ -dumpfullversion` (fallback `-dumpversion`) | Minimum-version check, feature defaults |
| Target | `g++ -dumpmachine` (`x86_64-w64-mingw32`, `x86_64-linux-gnu`, `*-cygwin`, …) | Platform flags, Cygwin warning |
| Integrity | `g++ -print-prog-name=cc1plus` exists and runs; trivial compile+link+run of a hello-world probe | Detect broken or partial installs (common on Windows) |
| Clang masquerade | `g++ --version` mentions `clang` | Classify as Clang |
| Standards | `-std=c++17/20/23/26` (`c++2c` before GCC 14) with `-fsyntax-only` on an empty TU | Standard dropdown |
| Library features | `-fsyntax-only` snippets: `<format>`, `<print>`, `<stacktrace>`, `std::ranges`, `std::jthread`, `contains`, designated initialisers | Feature gating and fallbacks ([06 §6.10](06-compiler-pipeline.md#610-includes-and-support-helpers)) |
| Diagnostics format | Accepts `-fdiagnostics-add-output=sarif:…`? else `-fdiagnostics-format=sarif-file`? else `json`? else plain text | §7.5.3 ladder |
| Sanitizers | Link (and run) probe with `-fsanitize=address,undefined`; and with `-fsanitize=undefined -fsanitize-undefined-trap-on-error` | Debug configuration ([§7.4.3](#743-sanitizers-and-hardening)) |
| Hardening | `-fhardened`, `-D_FORTIFY_SOURCE=3`, `-fstack-protector-strong`, `-fstack-clash-protection`, `-fcf-protection` accepted and link OK | Release hardening |
| Static link | `-static` link of the probe (Windows) | Standalone `.exe` default |
| Debugger | `gdb` alongside g++ (same `bin`), `gdb --version` ≥ 10 | Debugger availability |

Over IPC the app shows each toolchain as `{ id, version, target, flavor,
displayPath, source, usable, selected, capabilities, problems }`.
`capabilities` lists the supported standards and whether `std::format`,
sanitizers and SARIF diagnostics are available; `problems` holds the probe's
diagnostics. `flavor` is a display name derived from the location (*MSYS2
UCRT64*, *WinLibs*, *System*, …) or `null`, and `source` is `path`,
`wellKnown` or `manual`.

## 7.4 From build options to `argv`

The backend constructs every compiler command line itself from **closed
enums** in the project's build configuration
([05 §5.3](05-project-format.md#53-top-level-structure)), plus machine-local
settings. Commands are built as `Vec<OsString>` and passed to the OS directly,
**never through a shell**.

### 7.4.1 Base flags

| Purpose | Flags |
| --------- | ------- |
| Standard | `-std=c++20` (or `gnu++20` with *GNU extensions*) |
| Encoding | `-finput-charset=UTF-8 -fexec-charset=UTF-8` |
| Stable output | `-fdiagnostics-color=never -fdiagnostics-urls=never -fmessage-length=0` + the diagnostics-format flags from §7.5.3 |
| Safety net | `-Wbidi-chars=any` (GCC 12+; [08 §8.4](08-security.md#84-code-injection-through-block-content)) |
| Project headers | `-iquote <build>/gen` |
| Misc | `-pipe` |
| Defines | `-D<Ident>=<value>` from validated `defines` (value rendered by the emitter's literal printer) |
| IDE-only | `-include <build>/ide/b2c_ide.hpp` (trace macros) only for *trace* builds (M5); the IDE init TU `<build>/ide/b2c_ide_init.cpp` compiled into IDE builds only (§7.6.3) |

### 7.4.2 Warning levels

| Level | Flags |
| ------- | ------- |
| `minimal` | `-Wall` |
| `helpful` (default) | `-Wall -Wextra -Wpedantic` |
| `strict` | `helpful` + `-Wshadow -Wconversion -Wsign-conversion -Wold-style-cast -Wnon-virtual-dtor -Woverloaded-virtual -Wnull-dereference -Wdouble-promotion -Wformat=2 -Wimplicit-fallthrough` |
| *warnings as errors* (toggle) | `-Werror` |

### 7.4.3 Sanitizers and hardening

| Configuration | Linux | Windows (MinGW-w64) |
| --------------- | ------- | --------------------- |
| **Debug** | `-O0 -g -fno-omit-frame-pointer -D_GLIBCXX_ASSERTIONS`, plus `-fsanitize=address,undefined -fno-sanitize-recover=undefined` when probed OK | `-O0 -g -fno-omit-frame-pointer -D_GLIBCXX_ASSERTIONS`, plus `-fsanitize=undefined -fsanitize-undefined-trap-on-error` when probed OK (ASan is not available for MinGW GCC; the UI says so) |
| **Release** | `-O2 -DNDEBUG` | `-O2 -DNDEBUG` |
| **Hardening** (default on) | `-fhardened` on GCC 14+ for optimised builds without AddressSanitizer (it includes `_FORTIFY_SOURCE` and `_GLIBCXX_ASSERTIONS`, which debug builds handle themselves); otherwise `-U_FORTIFY_SOURCE -D_FORTIFY_SOURCE=3` (2 on GCC < 12) `-D_GLIBCXX_ASSERTIONS -fstack-protector-strong -fstack-clash-protection -fcf-protection -fPIE -pie -Wl,-z,relro,-z,now -Wl,-z,noexecstack` (each probe-gated) | `-fstack-protector-strong` (probe-gated) and `-Wl,--dynamicbase,--nxcompat,--high-entropy-va` (defaults in modern binutils, stated explicitly) |
| **Link** | dynamic (standard); `-pthread` when concurrency is used | `-static` by default so `.exe` files run anywhere (setting: *dynamic*); `-pthread` when concurrency is used |

Sanitizer and hardening choices that a toolchain cannot satisfy are dropped,
with an info diagnostic. They never fail the build.

### 7.4.4 Libraries and library profiles

A project (or library pack) **requests libraries by name** (`"libraries":
["sfml-graphics"]`). Names are resolved against machine-local **library
profiles**:

```json
{ "sfml-graphics": {
    "includeDirs": ["C:\\libs\\SFML-3.0\\include"],
    "libDirs":     ["C:\\libs\\SFML-3.0\\lib"],
    "link":        ["sfml-graphics", "sfml-window", "sfml-system"],
    "runtimeDirs": ["C:\\libs\\SFML-3.0\\bin"],
    "subsystem":   "console" } }
```

* These become `-isystem <dir>`, `-L <dir>` and `-l<name>` arguments. Every
  directory is chosen via a native dialog, canonicalised and verified to
  exist. Link names are validated against `[A-Za-z0-9_+.-]{1,64}`.
* **Linux pkg-config integration.** A profile may say `"pkgConfig": "sfml-all"`.
  The output of `pkg-config --cflags --libs` is tokenised and **allowlisted**
  to `-I`, `-isystem`, `-L`, `-l`, `-D<ident>[=<value>]` and `-pthread`.
  Anything else is dropped with a warning.
* An unresolved library name stops the build with a guided fix: *"This
  project needs the library `sfml-graphics`. Set it up…"*.
* `subsystem: "windows"` (GUI apps without a console) maps to `-mwindows` on
  Windows. It is a closed enum.

### 7.4.5 Machine-local extra flags (advanced)

Experienced users may add extra compiler or linker flags in **machine
settings**, never in projects:

* They are entered as an argv list (one flag per row), not a shell string.
* **Changing them requires a native confirmation dialog raised by the
  backend**, so script running in a compromised webview cannot set them
  silently.
* A denylist rejects flags that execute code, write files or break the
  pipeline: `-fplugin*`, `-B*`, `-wrapper`, `-specs*`/`--specs*`, `@<file>`
  (response files), `-o`, `-x`, `-save-temps*`, `-fdump-*`, `-M*`/`-MF`,
  `-fprofile-*=<path>`, `-fdiagnostics-*`, `-Wl,-plugin*`/`-Xlinker -plugin`,
  `-fuse-ld=<path>`, `--sysroot`, `-iplugindir*`, `-print-*`, `-v`/`-###`.
  The denylist is unit-tested and documented.

## 7.5 Compiling

### 7.5.1 Build directory

```text
<cache>/builds/<projectFolder>/<config>-<optionsHash8>/
├── lock                     held by the build using this folder; its mtime is the entry's last use
├── gen/                     generated sources (rewritten only when content changes)
│   ├── main.cpp  main.hpp  player.cpp  player.hpp  b2c_support.hpp
├── ide/                     IDE-only init unit and trace header (never exported)
├── obj/<tu>-<keyHash12>.o   content-addressed objects (M3)
├── diag/<tu>.sarif          per-TU diagnostics
├── out/<slug>[.exe]         final executable (mode 0700 on Linux)
├── sourcemap.json
└── build-manifest.json      projectHash, toolchain fingerprint, IDE flag, argv per step, executable hash, result
```

* `<cache>` is the cache root of [02 §2.7](02-architecture.md#27-persistence-locations)
  (`%LOCALAPPDATA%\Blocks2Cpp\` on Windows, `$XDG_CACHE_HOME/blocks2cpp/` on
  Linux), shared by the app and the CLI.
* Directories are created with `create_dir` (not `create_dir_all` past the
  cache root), refusing to follow symlinks or junctions inside the cache. They
  are owner-only on Linux.
* File names derive only from validated module names
  (`[a-z0-9_-]{1,64}`, lower-cased). They never come from user text, and a
  name that is a Windows device name is refused.
* `<projectFolder>` (also used for the sandbox folder, §7.6.2) is the project
  ID in lower case plus the first 8 hex digits of the SHA-256 of its exact
  spelling, for example `prj_hello-1a2b3c4d`. IDs that differ only in case
  never share a folder on a case-insensitive file system, and no ID can name
  a Windows device (`CON`, `NUL`, `COM1`, …).
* `<config>-<optionsHash8>` is the configuration (`debug` or `release`) and
  the first 8 hex digits of a hash over everything that changes the output
  besides the project content: the toolchain's SHA-256 and canonical path,
  the build configuration, the language settings, the defines, the indent
  width and whether this is an IDE build. IDE and CLI builds of the same
  project therefore never share an executable (only IDE builds contain the
  init unit, §7.6.3).
* A build holds an exclusive lock on `lock` from checking whether the program
  is up to date until it records the result, so two builds of the same project
  at once (two copies of a project file keep its ID) never leave an
  executable that does not match its recorded inputs.
* **`build-manifest.json`** records a successful build:
  `{ format: "blocks2cpp/build-manifest", formatVersion: 1, projectHash,
  toolchain: { path, size, modifiedNs, sha256, version, target }, ide,
  steps: [{ kind, argv }], executable: { name, size, sha256 },
  result: "success" }`, where a step's `kind` is `compileAndLink`, `compile`
  or `link`. It is written atomically only after success and deleted when a
  build is cancelled or fails, so a manifest always describes the executable
  next to it. Before a program starts, `run_start` reads it again and checks
  the executable's size and SHA-256 (`staleBuild` on a mismatch). It replaces
  M1's build stamp; `sourcemap.json` stays.
* A build is **up to date** when the manifest matches the project hash, the
  toolchain and the IDE flag, the executable matches its recorded size and
  hash, and no generated or IDE file changed on disk. A build that has to
  rewrite any of those files deletes the manifest first, so a new generator
  version that produces different code for the same project hash always
  recompiles.
* **Object cache key** = SHA-256(toolchain fingerprint ‖ normalised argv
  without output paths ‖ TU contents ‖ contents of all generated project
  headers ‖ support header). System headers are covered by the toolchain
  fingerprint. This is simpler and more reliable than modification times.
  The object cache matters only for multi-module projects and arrives with
  them in M3; until then a project's translation units are compiled straight
  into `out/`.
* LRU eviction when the cache exceeds 2 GiB (configurable). Entries untouched
  for 30 days are pruned at startup. *Clear build cache* is in the menu.

**Eviction and clearing:**

* The unit is one `builds/<projectFolder>/<config>-<optionsHash8>/` folder.
  Its last use is the modification time of its `lock` file, which every build
  and run touches.
* After each finished build and at startup, the app removes the least recently
  used entries while the cache is larger than `buildCache.maxBytes` (default
  2 GiB, [05 §5.9](05-project-format.md#59-machine-local-data)). At startup it
  also removes entries unused for 30 days.
* The most recently used entry is never evicted for size, and a run holds a
  shared lock on its entry's `lock` file for as long as it runs. An entry
  whose lock is held (a build or run is using it) is skipped, and so is an
  entry whose `lock` is a link or not a regular file. Links
  and junctions are never followed, every folder is checked to lie inside the
  canonical cache root before it is deleted, and project folders left empty
  are removed. Only `builds/` is touched: `sandbox/` and `toolchains.json`
  are never evicted.
* Only the app evicts. The CLI never deletes cache entries.
* *Clear build cache* (on the Settings page, [04 §4.12](04-user-interface.md#412-settings-page);
  `build_cache_clear`) deletes everything in `builds/` except entries in use
  and reports the bytes freed and the number of entries skipped.

### 7.5.2 Invocation

* **Executable:** the toolchain's canonical absolute path.
* **Working directory:** `<build>/diag` (so SARIF files land there).
* **Environment (allowlist, not denylist):**
  * Windows: `SystemRoot`, `windir`, `SystemDrive`, `TEMP`/`TMP` → a private
    temp dir, `PATH` → `<toolchain bin>;%SystemRoot%\System32;%SystemRoot%`,
    `LANG=C`.
  * Linux: `PATH` → `<toolchain dir>:/usr/local/bin:/usr/bin:/bin`, `HOME`,
    `TMPDIR` → a private temp dir, `LC_ALL=C.UTF-8` (fallback `C`).
  * Plus a user-configured pass-through list (machine settings) for unusual
    setups.
  * Removed by construction: `CPATH`, `CPLUS_INCLUDE_PATH`, `LIBRARY_PATH`,
    `COMPILER_PATH`, `GCC_EXEC_PREFIX`, `DEPENDENCIES_OUTPUT`,
    `SUNPRO_DEPENDENCIES`, `GCC_COLORS`, `LD_PRELOAD`, … This gives
    determinism and defence in depth against environment-based tampering.
* **Parallelism:** TUs compile concurrently, up to `min(cores, 8)`. Linking
  starts when all TUs have succeeded. A single-TU project is compiled and
  linked in one invocation.
* **Limits:**
  * Timeout per TU: 120 s (configurable, 10–600 s).
  * Memory: 4 GiB per invocation. On Windows, a Job Object
    `JobMemoryLimit`. On Linux, a cgroup-v2 transient scope (`MemoryMax`)
    when a user systemd instance is available; otherwise `prlimit(2)
    RLIMIT_AS` applied right after spawn, plus an RSS watchdog.
  * Process count: Job Object `ActiveProcessLimit` 32 / cgroup `pids.max`.
  * Captured stderr is capped at 4 MiB and the rest is truncated with a
    notice.
  * Template bombs or `#include "/dev/zero"` in Raw C++ hit these limits and
    produce a clear *"The compiler ran out of time/memory"* message.

**Linux containment in detail** ([ADR-0008](../adr/0008-pty-and-containment-in-b2c-process.md)):

* **cgroup v2 scopes.** When the app starts, it checks once that cgroup v2
  controllers exist, that `systemd-run` exists at `/usr/bin/systemd-run` or
  `/bin/systemd-run` (an absolute path, never `PATH`), that
  `XDG_RUNTIME_DIR` is set, and that a trial scope
  (`systemd-run … -- cat /proc/self/cgroup`, which also shows where the
  manager creates scopes) succeeds within 5 s. If so, each compiler (and each
  program, §7.6.2) runs as
  `systemd-run --user --scope --quiet --collect --expand-environment=no --unit=b2c-build-<appPid>-<16 hex> -p MemoryMax=4G -p MemorySwapMax=0 -p TasksMax=32 -- <g++> <args>`.
  `--expand-environment=no` (systemd 254 and later; left out for older
  versions, which reject it) stops `systemd-run` from expanding `$VARS` in the
  arguments. The scope also catches processes that leave the process group.
  `TasksMax` counts threads as well as processes. Without the `memory` or
  `pids` controller in the scope, the same limits fall back to the watchdogs
  below, counting the scope's processes.
* `systemd-run` needs `XDG_RUNTIME_DIR` (and `DBUS_SESSION_BUS_ADDRESS`, when
  set) to reach the user's service manager. These two are the only variables
  added to the compiler's allowlisted environment, and only when a scope is
  used; `systemd-run` itself also sets `INVOCATION_ID`.
* Stopping writes `1` to the scope's `cgroup.kill`, falling back to `SIGKILL`
  for every process in `cgroup.procs` until it is empty. An `oom_kill` in
  `memory.events` is reported as running out of memory (a `C:limit`
  diagnostic for compilers). After `--collect` has removed an empty scope, a
  run that failed counts as out of memory when the parent folder's
  hierarchical `oom_kill` count grew while it ran; an out-of-memory kill in a
  sibling group at the same moment can therefore be misreported, which is
  harmless.
* **Fallback.** Without cgroups, the compiler runs in its own process group
  with `RLIMIT_AS`, and an **RSS watchdog** adds up the resident memory
  (`VmRSS` in `/proc`) of the group's processes every 100 ms and kills the
  group above 4 GiB, reported the same way.
* At startup, `b2c-*` scopes left behind by an app instance that no longer
  runs are killed ([08 §8.14](08-security.md#814-residual-risks-accepted-documented-to-users)).

### 7.5.3 Diagnostics capture and mapping

**Format ladder** (chosen from the probe results):

| GCC | Mechanism | Human-readable text |
| ----- | ----------- | --------------------- |
| 15+ | `-fdiagnostics-add-output=sarif:file=<tu>.sarif`; when one invocation compiles several sources, `sarif:version=2.1` without `file=`, so GCC names one file per source | Kept on stderr as normal |
| 13–14 | `-fdiagnostics-format=sarif-file` (writes `<source>.sarif` into the working directory) | Reconstructed from SARIF |
| 11–12 | `-fdiagnostics-format=json` (stderr) | Reconstructed from JSON |
| other / unknown | `-fdiagnostics-plain-output`, parsing `file:line:col: severity: message [-Woption]` + `note:` continuation lines | Raw text |

**Mapping:**

1. Parse the results with strict, size-limited parsers (SARIF/JSON via
   `serde` with minimal structs; the text parser is hand-written and
   fuzz-tested).
2. Map file + line + column through the source map to the innermost
   block/part ([06 §6.9](06-compiler-pipeline.md#69-source-maps)). Include
   chains (`In file included from…`) and template-instantiation notes become
   `related` locations.
3. Apply the **friendly message catalog**: rules keyed by GCC option and/or
   anchored message patterns, with tests against recorded fixtures from GCC
   11, 13 and 15. For example:
   * `no match for 'operator<<'` → *"Can't print a value of type `Point`.
     Tick **printable** on the struct, or print its fields."*
   * `undefined reference to 'foo(int)'` (linker) → *"`foo` is declared but
     has no body. Check its **define** block or library setup."*
   * Errors inside Raw C++ → shown on the raw block, with the line underlined
     in its editor.
4. **Generator bug detection.** Code generated from non-raw blocks should
   never fail to compile when the analyser reported no errors. If it does,
   the diagnostic is labelled *"This looks like a bug in Blocks2Cpp"* and
   offers *Copy bug report* (a sanitised report with the minimal block
   subtree, generated C++ and g++ version, copied to the clipboard; nothing is
   sent anywhere).

### 7.5.4 Cancellation

A new build for the same project, *Stop*, or closing the project cancels the
session. This means killing the compiler process tree (Job Object termination
on Windows; `SIGTERM` to the process group on Linux, then `SIGKILL` after 2 s)
and discarding partial outputs. Cached objects are only ever written by
atomic rename after a successful compile, so the cache cannot be corrupted.

* The 2 s grace applies to the compiler runs of builds, both on cancel and on
  timeout. Capability probes are killed at once.
* A cancelled build deletes its partial outputs and its build manifest and
  finishes with the outcome `cancelled`. Cancelling a build that has already
  finished does nothing.
* A build runs as a *session* on its own thread with a cancellation token;
  `build_start` returns its `buildId` at once, and the session reports
  progress, diagnostics and exactly one `finished` event through its channel
  ([02 §2.5](02-architecture.md#25-ipc-surface)). The record of a build
  (outcome, project hash, executable, the document it built) is kept until
  its project closes, at most 8 per project: older build IDs become unknown
  (`unknownBuild`).
* Progress counts translation units. A single-unit build, compiled and linked
  in one invocation, reports `compile` 0/1, then `compile` 1/1 and `link`
  1/1.

## 7.6 Running programs

### 7.6.1 Preconditions

* The project is **trusted** ([08 §8.3](08-security.md#83-workspace-trust-and-restricted-mode)).
* A successful build exists whose `projectHash` and options match. Otherwise
  Run builds first.
* The analyser reports no errors with the current lint levels
  ([06 §6.6](06-compiler-pipeline.md#66-stage--types-flow-checks-and-lints)).
  Lint levels are not part of the project hash, so this is checked even when
  a matching build exists. It applies to ⟲ Run again too.

The backend checks these itself and never relies on the UI:

* `build_start` checks trust first. A restricted project gives `restricted`,
  and no build folder is created and no process is started.
* `run_start` refuses, in this order: a build of a project that is no longer
  open (`unknownBuild`); a project that is not trusted now (`restricted`); a
  build whose outcome is not `built` or `upToDate` (`buildNotSuccessful`); a
  build whose `projectHash` differs from the hash of the latest document the
  backend received for the project, or whose executable no longer matches its
  build manifest (`staleBuild`); and a document with analyser errors
  (`projectErrors`, with the count).

### 7.6.2 Spawning

* **PTY:** our own PTY layer in `b2c-process` (ConPTY on Windows 10 1809+,
  `openpty` on Linux; [ADR-0008](../adr/0008-pty-and-containment-in-b2c-process.md))
  with the size of the xterm.js viewport. *Pipe mode* is a fallback
  (and the CLI default when not attached to a terminal). On Linux the program
  is a session leader with the PTY as its controlling terminal; on Windows it
  is created suspended, assigned to the Job Object, and only then resumed.
* **argv:** `[<out>/<slug>, ...runArgs]` from the run options list, passed
  without a shell.
* **Working directory:** the project folder (default) or a per-project
  sandbox folder in the cache. Users choose; the choice is stored per project
  as an enum.
* **In M2** the arguments and the working directory come from the document
  that was built (`run.args`, `run.workingDirectory`); `run_start` takes only
  the terminal size. A project that has never been saved has no folder, so it
  always runs in its sandbox folder, `<cache>/sandbox/<projectFolder>/`.
* **Environment:** the user's own environment (programs legitimately need it)
  minus IDE-internal variables, plus:
  * `TERM=xterm-256color` (Linux)
  * Library-profile `runtimeDirs` (and the toolchain `bin` when linked
    dynamically on Windows) prepended to `PATH` / `LD_LIBRARY_PATH`
  * `ASAN_OPTIONS=halt_on_error=1:detect_leaks=1` and
    `UBSAN_OPTIONS=print_stacktrace=1:halt_on_error=1` for sanitizer builds
  * `B2C_EVENTS=<channel>` (IDE runs only, §7.7)
* The **IDE-internal variables** removed are `B2C_*`, `TAURI_*`, `WEBVIEW2_*`,
  `WEBKIT_*`, `APPDIR`, `APPIMAGE`, `ARGV0` and `OWD`. `detect_leaks=0`
  replaces `detect_leaks=1` where leak detection is not available
  (`B2C-T1021`). In M2 `B2C_EVENTS` is never set, because the event channel
  arrives in M5. How the AppImage's library path is handled is settled with
  the AppImage bundle (M6).
* **Containment:** Windows Job Object with `KILL_ON_JOB_CLOSE`, so closing the
  app always kills the program tree. Linux: a new process group, and a
  cgroup-v2 scope when available (which also catches double-forked or
  `setsid` children).
* On Linux the scope is created as for compilers (§7.5.2), named
  `b2c-run-<appPid>-<16 hex>`, without memory or task limits. `RLIMIT_AS` is
  never applied to programs, because it breaks AddressSanitizer; the optional
  memory cap of the settings (M5) is enforced by the RSS watchdog instead. The
  `started` run event reports the containment that was actually used
  (`jobObject`, `cgroup` or `processGroupOnly`) and whether the program runs
  in a PTY or with pipes.

### 7.6.3 IDE init unit

For IDE runs only, a small extra translation unit (`ide/b2c_ide_init.cpp`) is
linked in. It **never appears in the generated code view or in exports**, and
the console header says *"Running with IDE helpers"*. Its static initialiser:

* **Windows:** sets the console input/output code pages to UTF-8, so
  `std::cout << "héllo ✓"` displays correctly.
* Installs a `std::set_terminate` handler that reports an uncaught exception's
  type and `what()` (and a `<stacktrace>` when the toolchain supports it) to
  the event channel, then calls `std::abort()`.
* Opens the event channel if `B2C_EVENTS` is set and is otherwise a no-op.

Keeping this in a separate TU keeps `<windows.h>` and IDE plumbing out of the
user's code.

**In M2** the unit does only the first item. It is written to
`<build>/ide/b2c_ide_init.cpp` for IDE builds, compiled in the same g++
invocation as `main.cpp` and linked on both systems; on Windows its static
initialiser sets the console code pages to UTF-8, and elsewhere it does
nothing. It never reads `B2C_EVENTS`. The terminate handler and the event
channel arrive in M5. Because the IDE flag is part of the build folder's
options hash (§7.5.1), builds with and without the unit never share an
executable, and the CLI's builds never contain it. A compiler diagnostic
located in `ide/` is reported as *"This looks like a bug in Blocks2Cpp"*
(§7.5.3).

### 7.6.4 Exit decoding

| Outcome | Shown as |
| --------- | ---------- |
| Exit code 0 | *Finished (exit code 0)* |
| Exit code n | *Finished with exit code n*; a non-zero code is highlighted |
| Linux `SIGSEGV` / Windows `0xC0000005` | *Crashed: the program tried to use memory it doesn't own (segmentation fault / access violation).* Common causes are listed: an index out of range with *fast unchecked access*, a null pointer, or a dangling reference. |
| Windows `0xC00000FD` / Linux `SIGSEGV` with a stack-overflow pattern | *Crashed: stack overflow, probably infinite recursion*; from M5 also *in `<function>`* |
| `SIGFPE` / `0xC0000094` | *Crashed: integer division by zero* |
| `SIGABRT` / exit code 3 / `0xC0000409` | *Stopped itself: an uncaught error or failed check*, plus the details from the event channel if any |
| Sanitizer report | Parsed (`ERROR: AddressSanitizer: heap-buffer-overflow …`), summarised, and mapped to blocks via the stack frames |
| Stopped by the user | *Stopped* |

Runtime diagnostics (`R:*`) appear in the Problems panel and on blocks when a
location could be mapped.

The texts are those of `b2c_process::ExitStatus::describe`, which also covers
illegal instructions from safety checks, `Ctrl+C`, termination, kills,
running out of memory, heap corruption, a missing DLL, a broken pipe and
resource limits. A crash message ends with its technical name in brackets,
for example *(SIGFPE)* or *(exception 0xC00000FD)*. The `exit` run event
carries the status, a closed `crash` kind and this message
([02 §2.5](02-architecture.md#25-ipc-surface)).

**Sanitizer reports in M2.** The backend scans the program's output for
`==<pid>==ERROR: AddressSanitizer: <kind>` and UndefinedBehaviorSanitizer's
`runtime error:` at the start of a line, with at most 4 KiB of state. The exit
message then summarises the report, for example *Crashed:
heap-buffer-overflow (AddressSanitizer)*, and the `exit` event carries
`{ tool, kind }`. Mapping the report to blocks through its stack frames, and
naming the function, arrive in M5. The scanner is fuzzed. LeakSanitizer
reports (`ERROR: LeakSanitizer`) are not matched in M2, so a Debug program
that only leaks shows *Finished with exit code 23*; whether to summarise them
is decided with the M5 runtime diagnostics.

### 7.6.5 Run limits

* No wall-clock limit by default (interactive programs), with a prominent
  **Stop** button. The CLI has `--timeout`.
* Optional memory cap and process cap (Job Object / cgroup) in settings.
* **Output flood protection:** the backend coalesces output into batches every
  ≤ 16 ms. If the frontend falls behind, intermediate output beyond the
  scrollback cap is dropped with a *"… 1,204,331 lines skipped"* marker,
  so the UI never freezes.
* **Memory and process limits** stop the program at once (no grace period).
  Its `exit` event then reports `crash: outOfMemory` with *Crashed: the
  program ran out of memory*, or `crash: resourceLimit` with *Stopped: the
  program started too many processes*, rather than the signal that ended it.
  A program the system's out-of-memory killer ends inside its cgroup scope
  is reported the same way. In M2 programs have no caps, so only the system
  can end one this way.

How the flood protection works ([02 §2.5](02-architecture.md#25-ipc-surface)):

* Output batches are numbered from 1. The console acknowledges what it has
  written with `run_ack`, at most every 100 ms.
* While more than 4 MiB is unacknowledged, the backend keeps only the last
  `console.scrollbackLines` lines (at most 8 MiB) and counts the lines it
  drops. When the acknowledgements catch up, it sends a `skipped` event with
  the exact count, then the kept tail. Without acknowledgements the backend
  still keeps only the tail, so its memory stays bounded. The console also
  counts as behind when more than 1,024 batches are unacknowledged, so a
  console that never acknowledges costs bounded memory even when output only
  trickles.
* The `skipped` count is the number of line breaks dropped. A single line
  longer than 8 MiB can be cut without its break, so `lines` can be 0.
* An acknowledgement after the program ended succeeds (late acknowledgements
  are normal); `run_input`, `run_resize` and `run_stop` then give
  `notRunning`. An acknowledgement above the last batch sent gives
  `invalidRequest`. The last 64 ended runs are remembered, so calls for them
  give `notRunning` rather than `unknownRun`.
* The `exit` event always follows the last output batch: its `afterSeq` names
  the number of batches before it.
* **Input** (`run_input`) is at most 64 KiB per call and is rate-limited to
  200 calls and 1 MiB per second per program. At most 64 calls wait for a
  program that is not reading its input; more give `rateLimited`.
* At most 8 programs run at once in the app, and one per project.
* Program input and output are never written to the log.

## 7.7 Runtime event side channel

A dedicated channel, separate from stdout/stderr, carries structured events
from the IDE init unit and trace instrumentation. Escape sequences in the
terminal stream are not used, because ConPTY may rewrite or drop unknown
sequences, and mixing with user output is fragile.

* **Windows:** a named pipe `\\.\pipe\b2c-<128-bit random>` created with
  `FILE_FLAG_FIRST_PIPE_INSTANCE`, `PIPE_REJECT_REMOTE_CLIENTS`, and a
  security descriptor granting only the current user.
* **Linux:** a Unix domain socket in a `0700` per-run directory.
* **Protocol:** newline-delimited JSON records of at most 4 KiB, at most 1,000
  records/s (excess dropped). Record types are `uncaught_exception`, `trace`
  and `stacktrace`.
* **Validation:** records are untrusted (the program can write anything).
  They are parsed with strict schemas, and block IDs must exist in the current
  build's source map. Everything else is ignored.
* **Trace mode:** codegen inserts `B2C_TRACE(<index>)` at each statement. The
  macro is defined only via the IDE `-include` header and records the latest
  block index. The channel sends it at most every 16 ms, which yields
  Scratch-like glow with modest overhead (documented; off by default).

## 7.8 Debugger (later phase)

* GDB from the same toolchain directory, run as `gdb --interpreter=mi3 -nx
  -iex "set auto-load off" -iex "set debuginfod enabled off" -iex "set confirm
  off"`.
  * `-nx` and `auto-load off` prevent **`.gdbinit` and auto-loaded Python
    scripts** (which could ship in an untrusted folder) from executing.
  * Disabling debuginfod avoids unexpected network access.
* GDB runs inside the same Job Object / process group as the program.
* Breakpoints on blocks map to the first line of the block's source-map range.
  Block-level stepping repeats `-exec-next`/`-exec-step` until the current
  line maps to a different block.
* Variable names in generated code equal user identifiers, so the Variables
  panel needs no name demangling beyond hiding generator temporaries.
* MI output is parsed with a strict, fuzz-tested parser.

## 7.9 Command-line interface

```text
b2c check     <project.b2c> [--format text|json] [--no-machine-lints]
b2c generate  <project.b2c> --out <dir> [--export] [--no-machine-lints]
b2c build     <project.b2c> [--config debug|release] [--toolchain <g++ path>] [--out <file>] [--no-machine-lints]
b2c run       <project.b2c> [--config …] [--stdin <file>] [--timeout <dur>] [--no-machine-lints] [-- <program args>…]
b2c toolchains [--format text|json]
b2c migrate   <project.b2c> [--in-place]
b2c fmt       <project.b2c> [--check]
```

* Same pipeline, same limits and same argv construction as the app.
* The CLI does not consult the GUI trust store. Running `b2c build` on a named
  file is an explicit user decision, like running `make`. This is documented
  prominently.
* The CLI shares the cache root and `toolchains.json` with the app
  ([02 §2.7](02-architecture.md#27-persistence-locations)), but its builds
  never include the IDE init unit, so they use their own build folders
  (§7.5.1). Only the app evicts cache entries; the CLI never deletes them.
* The CLI does not read the code style from the machine settings, so it
  always generates C++ with an indent width of 4. On Windows its first action
  is the same DLL search hardening as the app's
  ([08 §8.7](08-security.md#87-process-execution-safety)).
* Lint levels come from the project file and the machine's `settings.json`,
  exactly as in the app ([06 §6.6](06-compiler-pipeline.md#66-stage--types-flow-checks-and-lints)).
  The CLI reads only the lint levels from that file and never writes it. A
  file or entry that is not valid gets a warning on standard error and is not
  used. A machine without a settings file, such as a CI runner, uses the
  project's levels, and so does `--no-machine-lints`, so a teacher can check
  exactly as CI does.
* **Exit codes:** `0` success; `1` project errors; `2` usage (including an
  input file that cannot be read); `3` toolchain problem. `b2c run` returns the
  program's own exit code, `124` if `--timeout` stopped it, `125` if it could
  not build or start the program, and on Linux `128 + N` if signal `N` killed
  it (the shell convention). The reference is
  [docs/reference/cli.md](../reference/cli.md).
* `--format json` output is versioned and documented for teachers' and CI
  tooling.
