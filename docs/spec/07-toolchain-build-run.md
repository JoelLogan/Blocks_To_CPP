# 7. Toolchain, Compilation and Execution

> Status: **Draft v0.1** · Crates: `b2c-toolchain`, `b2c-process`, `b2c-build`, `b2c-cli`

## 7.1 Supported toolchains

| | Minimum | Recommended | Notes |
|---|---------|-------------|-------|
| GCC (g++) | **11** | **13+** | 11: C++20 core, `-fdiagnostics-plain-output`. 13: `std::format`, SARIF diagnostics. 14: `<print>`, `-fhardened`. 15: `-fdiagnostics-add-output`. |
| Windows flavours | MinGW-w64 (UCRT or MSVCRT runtime): MSYS2, WinLibs, TDM-GCC 10+, Scoop/Chocolatey `mingw`, Strawberry Perl's bundled GCC | MSYS2 UCRT64 | Cygwin GCC is detected and **warned against** (binaries need `cygwin1.dll`). Legacy mingw.org (`C:\MinGW`) is flagged as outdated. |
| Linux | Distro GCC, `g++-NN` side-by-side versions, RHEL `gcc-toolset-N`, Homebrew-on-Linux | Distro GCC 13+ | |
| Clang | — | — | Detected and labelled *not supported yet*. The driver abstraction (`trait Toolchain`) leaves room for it post-1.0. |

## 7.2 Discovery

Discovery runs at startup (in the background, with cached results shown
immediately), on *Rescan*, and when a cached toolchain's fingerprint changes.

**Search order:**

1. The user-selected toolchain from settings (validated again before use).
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
project folder ships its own `g++.exe`.

Each candidate is **canonicalised** (symlinks resolved; on Windows the path is
normalised and checked to be a regular file, not a reparse point into an
unexpected location), deduplicated by canonical path, and **fingerprinted**:
`(canonical path, size, mtime, SHA-256 of the driver binary, -dumpfullversion,
-dumpmachine)`.

## 7.3 Capability probing

Probes run once per fingerprint, in parallel. Each probe has a 10 s timeout and
runs in a private temporary directory with the sanitised environment from
§7.5.2. Results are stored in `toolchains.json`.

| Probe | Method | Used for |
|-------|--------|----------|
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

## 7.4 From build options to `argv`

The backend constructs every compiler command line itself from **closed
enums** in the project's build configuration
([05 §5.3](05-project-format.md#53-top-level-structure)), plus machine-local
settings. Commands are built as `Vec<OsString>` and passed to the OS directly,
**never through a shell**.

### 7.4.1 Base flags

| Purpose | Flags |
|---------|-------|
| Standard | `-std=c++20` (or `gnu++20` with *GNU extensions*) |
| Encoding | `-finput-charset=UTF-8 -fexec-charset=UTF-8` |
| Stable output | `-fdiagnostics-color=never -fdiagnostics-urls=never -fmessage-length=0` + the diagnostics-format flags from §7.5.3 |
| Safety net | `-Wbidi-chars=any` (GCC 12+; [08 §8.4](08-security.md#84-code-injection-through-block-content)) |
| Project headers | `-iquote <build>/gen` |
| Misc | `-pipe` |
| Defines | `-D<Ident>=<value>` from validated `defines` (value rendered by the emitter's literal printer) |
| IDE-only | `-include <build>/ide/b2c_ide.hpp` (trace macros) only for *trace* builds; IDE init TU linked only for IDE runs (§7.6.3) |

### 7.4.2 Warning levels

| Level | Flags |
|-------|-------|
| `minimal` | `-Wall` |
| `helpful` (default) | `-Wall -Wextra -Wpedantic` |
| `strict` | `helpful` + `-Wshadow -Wconversion -Wsign-conversion -Wold-style-cast -Wnon-virtual-dtor -Woverloaded-virtual -Wnull-dereference -Wdouble-promotion -Wformat=2 -Wimplicit-fallthrough` |
| *warnings as errors* (toggle) | `-Werror` |

### 7.4.3 Sanitizers and hardening

| Configuration | Linux | Windows (MinGW-w64) |
|---------------|-------|---------------------|
| **Debug** | `-O0 -g -fno-omit-frame-pointer -D_GLIBCXX_ASSERTIONS`, plus `-fsanitize=address,undefined -fno-sanitize-recover=undefined` when probed OK | `-O0 -g -fno-omit-frame-pointer -D_GLIBCXX_ASSERTIONS`, plus `-fsanitize=undefined -fsanitize-undefined-trap-on-error` when probed OK (ASan is not available for MinGW GCC; the UI says so) |
| **Release** | `-O2 -DNDEBUG` | `-O2 -DNDEBUG` |
| **Hardening** (default on) | `-fhardened` on GCC 14+; otherwise `-U_FORTIFY_SOURCE -D_FORTIFY_SOURCE=3` (2 on GCC < 12) `-D_GLIBCXX_ASSERTIONS -fstack-protector-strong -fstack-clash-protection -fcf-protection -fPIE -pie -Wl,-z,relro,-z,now -Wl,-z,noexecstack` (each probe-gated) | `-fstack-protector-strong` (probe-gated) and `-Wl,--dynamicbase,--nxcompat,--high-entropy-va` (defaults in modern binutils, stated explicitly) |
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

```
<cache>/builds/<projectId>/<config>-<optionsHash8>/
├── gen/                     generated sources (rewritten only when content changes)
│   ├── main.cpp  main.hpp  player.cpp  player.hpp  b2c_support.hpp
├── ide/                     IDE-only init unit and trace header (never exported)
├── obj/<tu>-<keyHash12>.o   content-addressed objects
├── diag/<tu>.sarif          per-TU diagnostics
├── out/<slug>[.exe]         final executable (mode 0700 on Linux)
├── sourcemap.json
└── build-manifest.json      projectHash, toolchain fingerprint, argv per step, object keys, result
```

* Directories are created with `create_dir` (not `create_dir_all` past the
  cache root), refusing to follow symlinks or junctions inside the cache. They
  are owner-only on Linux.
* File names derive only from validated module names
  (`[a-z0-9_-]{1,64}`, lower-cased). They never come from user text.
* **Object cache key** = SHA-256(toolchain fingerprint ‖ normalised argv
  without output paths ‖ TU contents ‖ contents of all generated project
  headers ‖ support header). System headers are covered by the toolchain
  fingerprint. This is simpler and more reliable than modification times.
* LRU eviction when the cache exceeds 2 GiB (configurable). Entries untouched
  for 30 days are pruned at startup. *Clear build cache* is in the menu.

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

### 7.5.3 Diagnostics capture and mapping

**Format ladder** (chosen from the probe results):

| GCC | Mechanism | Human-readable text |
|-----|-----------|---------------------|
| 15+ | `-fdiagnostics-add-output=sarif:file=<tu>.sarif` | Kept on stderr as normal |
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

## 7.6 Running programs

### 7.6.1 Preconditions

* The project is **trusted** ([08 §8.3](08-security.md#83-workspace-trust-and-restricted-mode)).
* A successful build exists whose `projectHash` and options match. Otherwise
  Run builds first.

### 7.6.2 Spawning

* **PTY:** `portable-pty` (ConPTY on Windows 10 1809+, `openpty` on Linux)
  with the size of the xterm.js viewport. *Pipe mode* is a fallback
  (and the CLI default when not attached to a terminal).
* **argv:** `[<out>/<slug>, ...runArgs]` from the run options list, passed
  without a shell.
* **Working directory:** the project folder (default) or a per-project
  sandbox folder in the cache. Users choose; the choice is stored per project
  as an enum.
* **Environment:** the user's own environment (programs legitimately need it)
  minus IDE-internal variables, plus:
  * `TERM=xterm-256color` (Linux)
  * Library-profile `runtimeDirs` (and the toolchain `bin` when linked
    dynamically on Windows) prepended to `PATH` / `LD_LIBRARY_PATH`
  * `ASAN_OPTIONS=halt_on_error=1:detect_leaks=1` and
    `UBSAN_OPTIONS=print_stacktrace=1:halt_on_error=1` for sanitizer builds
  * `B2C_EVENTS=<channel>` (IDE runs only, §7.7)
* **Containment:** Windows Job Object with `KILL_ON_JOB_CLOSE`, so closing the
  app always kills the program tree. Linux: a new process group, and a
  cgroup-v2 scope when available (which also catches double-forked or
  `setsid` children).

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

### 7.6.4 Exit decoding

| Outcome | Shown as |
|---------|----------|
| Exit code 0 | *Finished (exit code 0)* |
| Exit code n | *Finished with exit code n*; a non-zero code is highlighted |
| Linux `SIGSEGV` / Windows `0xC0000005` | *Crashed: the program tried to use memory it doesn't own (segmentation fault / access violation).* Common causes are listed: an index out of range with *fast unchecked access*, a null pointer, or a dangling reference. |
| Windows `0xC00000FD` / Linux `SIGSEGV` with a stack-overflow pattern | *Crashed: stack overflow – probably infinite recursion in `<function>`* |
| `SIGFPE` / `0xC0000094` | *Crashed: integer division by zero* |
| `SIGABRT` / exit code 3 / `0xC0000409` | *Stopped itself: an uncaught error or failed check*, plus the details from the event channel if any |
| Sanitizer report | Parsed (`ERROR: AddressSanitizer: heap-buffer-overflow …`), summarised, and mapped to blocks via the stack frames |
| Stopped by the user | *Stopped* |

Runtime diagnostics (`R:*`) appear in the Problems panel and on blocks when a
location could be mapped.

### 7.6.5 Run limits

* No wall-clock limit by default (interactive programs), with a prominent
  **Stop** button. The CLI has `--timeout`.
* Optional memory cap and process cap (Job Object / cgroup) in settings.
* **Output flood protection:** the backend coalesces output into batches every
  ≤ 16 ms. If the frontend falls behind, intermediate output beyond the
  scrollback cap is dropped with a *"… 1,204,331 lines skipped"* marker,
  so the UI never freezes.

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

```
b2c check     <project.b2c> [--format text|json]
b2c generate  <project.b2c> --out <dir> [--export]
b2c build     <project.b2c> [--config debug|release] [--toolchain <g++ path>] [--out <file>]
b2c run       <project.b2c> [--config …] [--stdin <file>] [--timeout <dur>] [-- <program args>…]
b2c toolchains [--format text|json]
b2c migrate   <project.b2c> [--in-place]
b2c fmt       <project.b2c> [--check]
```

* Same pipeline, same limits and same argv construction as the app.
* The CLI does not consult the GUI trust store. Running `b2c build` on a named
  file is an explicit user decision, like running `make`. This is documented
  prominently.
* **Exit codes:** `0` success; `1` project errors; `2` usage; `3` toolchain
  problem. `b2c run` returns the program's own exit code, or `125` if it could
  not build or start the program.
* `--format json` output is versioned and documented for teachers' and CI
  tooling.
