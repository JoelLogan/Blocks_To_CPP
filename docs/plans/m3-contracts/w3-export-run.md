# M3 contracts: W3, export, run options and the runtime items

What this part of wave W3 builds ([next steps §3.2 and §3.4](../next-steps.md#34-waves)):
everything under _Projects, build and run_ that does not depend on
multi-module projects (L18, a separate part of W3). In the order it is built:

1. ending a program's input (_close-input_) in `b2c-process`, the build
   crate, the IPC contract and the console;
2. three items of the M2 runtime audit: open-ended `g++-N` names (S7-8),
   pruning sandbox folders (S7-7) and `run_start` reusing the build's
   analysis (S7-5);
3. run options: arguments, working directory, standard input from a file
   and an external terminal, with the UI;
4. export to CMake and Make (`b2c-build`, `b2c generate --export`, the
   desktop's `project_export_dialog`);
5. the project settings dialog.

It follows [ADR-0004](../../adr/0004-project-format.md),
[ADR-0005](../../adr/0005-no-compiler-flags-in-projects.md) (no compiler
flags, paths or commands in projects),
[ADR-0007](../../adr/0007-backend-crates-and-ipc-contract.md) (the generated
IPC contract), [ADR-0008](../../adr/0008-pty-and-containment-in-b2c-process.md)
(our own PTY and containment) and
[ADR-0013](../../adr/0013-additive-format-keys.md) (additive format keys),
and the owner's M3-D3 (a program touches only what it is given; the app
touches only the open project's folder, its own machine folders and what
the user picks in a dialog) and M3-D5 (the external terminal).

## 1. Close-input

### 1.1 What it means

_End input_ tells the program that nothing more will be typed: its next read
of standard input returns end-of-file (C++: `std::cin` sets `eofbit`,
`std::getline` fails), as if the input came from a file that has ended.
Without it, a program that reads "until the input ends" (word counts, sums
of a list) can only be finished with Stop, which loses its result.

| Connection | What _End input_ does | Afterwards |
| --- | --- | --- |
| Pipes (Linux and Windows) | closes this process's end of the program's input pipe | every read gives end-of-file |
| Linux PTY | writes the terminal's end-of-file character (`VEOF`, normally `Ctrl+D`) once, or twice when a partly typed line may be waiting (the first then hands over that line without a line break, as in any terminal) | the next read gives end-of-file; a program that clears the error and reads again waits, as in a terminal |
| Windows `ConPTY` | writes `Ctrl+Z` and Enter, after an Enter of its own when a partly typed line may be waiting (that line then reaches the program with a line break) | the next read gives end-of-file |

"A partly typed line may be waiting" is decided from the last byte written:
anything but `\r` or `\n` (or nothing written yet: no). A wrong guess is
harmless on Linux (a second `VEOF` gives one more empty read) and costs one
empty line on Windows. A program that switched its terminal out of
canonical mode gets the bytes as keystrokes, which is what pressing the keys
would do.

**Alternatives considered.** Closing the PTY's master side ends input on
Linux but hangs the terminal up (`SIGHUP`, output lost); `ConPTY` has no
end-of-input operation, and closing its input pipe ends the pseudo console.
A Windows input of only `Ctrl+Z` is not end-of-file in the console's
cooked mode until Enter.

### 1.2 Layers

* `b2c-process`: `PtyWriter::close_input()` (for every clone; idempotent).
  Afterwards writes fail with `BrokenPipe`. Pipes: the end is closed once
  writes in flight have finished. Tests: `tests/pty_unix.rs` (PTY and
  pipes: `cat` ends; a partly typed line is delivered; writes fail
  afterwards; idempotent) and `tests/pty_windows.rs` (`sort.exe` ends in
  `ConPTY` and with pipes), Windows through CI.
* `b2c-build`: `RunSessions::close_input(run_id)` queues the close behind the
  input already queued (the writer thread does both, in order). Later
  `run_input` calls give the new error `inputClosed` (from the start for a
  program that reads an input file or runs in a window of its own); a
  second close succeeds. Tests in `tests/run_sessions.rs` with a compiled C++ program that
  counts lines until end-of-file, in PTY and pipe mode, on both systems
  (Windows through CI, with `B2C_REQUIRE_GXX`).
* IPC: `run_close_input { runId }` → `{}`; errors `unknownRun`,
  `notRunning`. New error code `inputClosed` for `run_input` after it.
* Console: an **End input** button in the header while a program runs and
  its input is open (it then says _Input ended_). In pipe mode, where the
  terminal cannot turn `Ctrl+D` into end-of-file, `Ctrl+D` typed in the
  console does the same; in PTY mode `Ctrl+D` (Linux) and `Ctrl+Z` Enter
  (Windows) keep working as in a real terminal.

## 2. The runtime audit items

### 2.1 S7-8: open-ended `g++-N`

Linux discovery tries, in each folder, `g++`, then every file named `g++-N`
(N a whole number from 11, written without leading zeros, at most 3 digits)
newest first, instead of the fixed `g++-16` … `g++-11`. The folder is
listed once; nothing else changes (canonical dedup, exclusions, execute bit).
Tests: `g++-17` and `g++-21` are found in the right order, `g++-10`,
`g++-011`, `g++-13.2` and `g++-1x` are not.

### 2.2 S7-7: pruning sandbox folders

A run in a sandbox (`<cache>/sandbox/<projectFolder>/`) marks it used: a
sibling lock file `<cache>/sandbox/<projectFolder>.lock` (owner-only,
outside the folder the program sees) gets its modification time set and a
shared lock held for as long as the program runs (`cache::hold_sandbox`).
Startup maintenance removes sandboxes whose lock is older than 30 days and
not held, with the build cache's safety rules: no links followed, every
level checked to be a real folder inside the canonical `sandbox/` folder,
the lock taken exclusively during the deletion, the lock file removed last.
A sandbox without a lock file (made by M2) counts from its folder's own
modification time. Only the app prunes; the CLI never does. Tests in
`b2c-build/tests/cache.rs`: old and unlocked is removed, old but held is
kept, recent is kept, a link named like a sandbox is removed as a link and
its target kept, an M2 sandbox is judged by its folder's time.

### 2.3 S7-5: `run_start` reuses the build's analysis

A build records what its analysis depended on (today the generator options;
from M5 also the lint levels) and how many analyser errors it found. A
successful build has none. `run_start` re-runs the front end only when the
latest document's hash or those inputs differ from the build's; since it
already refuses a build whose hash is not the latest document's
(`staleBuild`), in practice it re-runs only after a settings change. The
check order of 07 §7.6.1 is unchanged. Test: `b2c-app` counts front-end
runs (a test seam on the backend) and sees none for a fresh build, one
after the indent width changed.

## 3. Run options

### 3.1 Which belong where

| Option | Stored | Why |
| --- | --- | --- |
| Arguments | project, `run.args` (format 1, exists) | part of how the program is meant to run; an argv list, never a shell string |
| Working directory | project, `run.workingDirectory` (format 1, exists) | `project` or `sandbox`, a closed enum |
| Input from a file | this computer, per project, for the app session | a path (ADR-0005), and the app opens only what the user picks in a dialog (M3-D3) |
| Run in an external terminal | this computer: `settings.json` `run.externalTerminal` | about the user's desktop, not the program; a project should not open windows |

So **no project-format change**: the two project keys exist since format 1
and stay in it.

### 3.2 Input from a file

* `run_input_file { handle, action }` with `action` one of `get`, `choose`
  (a native open dialog raised by the backend, starting in the project's
  folder, or the documents folder for a project without a file) and
  `clear` → `{ fileName, cancelled }`, where `fileName` is the chosen file's
  name only (never its path) or `null`. The path stays in the backend, keyed
  by the project's ID, until the app ends.
* `run_start` opens the file read-only. It must be a regular file (not a
  folder, FIFO or device; on Linux opened with `O_NONBLOCK` checked by
  `fstat` before use), else `io` (`notFound` or `other`). The program then
  runs with pipes: standard input is the file itself (it can seek), output
  goes to the console as usual, and the console's input is closed from the
  start (the header says _Input from `name`_).
* `b2c-process`: `spawn_piped` accepts `Stdin::File` (on Windows the file
  handle joins the explicit handle list).
* An input file and the external terminal together: the input file wins and
  the run uses the console (the dialog says so).

### 3.3 External terminal (M3-D5)

* Setting `run.externalTerminal` (default `false`), on the Settings page and
  in the project settings dialog's run section (marked _this computer_).
* **Linux:** the first of `x-terminal-emulator -e` (only when it leads to
  `xterm`, `uxterm`, `lxterm`, `kitty`, `alacritty`, `foot` or `urxvt`,
  terminals that keep the window in the process started), then a fixed list
  (`konsole --nofork -e`, `xfce4-terminal --disable-server -x`, `kitty`,
  `alacritty -e`, `foot`, `xterm -e`, and last `gnome-terminal --wait --`),
  each found only in `/usr/bin` and `/bin` (never `PATH` or the project). The
  terminal runs `/bin/sh -c <fixed script> b2c-run <status file> <program>
  <args…>`: the program and its arguments are positional parameters, never
  part of the script, so no argument is ever parsed by a shell. The script
  runs the program, writes its exit status into the status file (in a new
  owner-only folder in the cache's `run-tmp/`) and waits for Enter. The run
  is contained like any other (the terminal is inside the cgroup scope or
  process group, so Stop closes its window). The exit event reports the
  program's status from the file, or _Finished in the external terminal_
  without a code when the file is missing (a terminal that hands the window
  to a server process; `gnome-terminal` needs `--wait` for this reason).
* **Windows:** the program is created with `CREATE_NEW_CONSOLE` (a `conhost`
  window, or the user's default terminal), suspended, put in its Job Object
  and resumed, so Stop and closing the app end it. The IDE init unit, which
  only IDE builds contain, waits for a key (`_getch`) before the window
  closes when the app set `B2C_EXTERNAL_TERMINAL=1` (it removes the variable
  first, so the program's own children never see it); a program that
  crashes closes its window at once, and the console says how it ended. The
  exit status is the program's.
* No terminal found: `run_start` gives the new error `noExternalTerminal`,
  and the console offers to run in the app instead.

## 4. Export to CMake and Make

### 4.1 What is written

```text
<chosen folder>/<project-slug>/
├── src/                 the generated .cpp/.hpp, without the banner
├── CMakeLists.txt       CMake ≥ 3.20
├── Makefile             g++ and GNU make
├── README.md            how to build, what the flags are
└── .b2c-export.json     { format, formatVersion: 1, files: [...] }
```

`<project-slug>` is the build's executable slug. The generated code is what
the app compiles minus the IDE init unit (06 §6.11). Flags come from the
same lists `b2c-toolchain` builds the app's command lines from (one
function per group: standard, encoding, warnings, optimisation and
assertions, sanitizers, hardening), so they cannot drift:

* both files: the standard (`-std=c++20`, `gnu++` with GNU extensions), UTF-8
  input and execution character sets, the warning level and _warnings as
  errors_, `-O*`, `-g`, `-fno-omit-frame-pointer`,
  `-D_GLIBCXX_ASSERTIONS` or `-DNDEBUG`, the project's defines (each
  validated as for a build, `B2C-T1019`; a text define holding `#`, `;` or
  `$` is refused with `B2C-T1019` too, as CMake and make cannot pass those
  unchanged), `-pthread` when the program uses threads;
* CMake: `Debug` and `Release` from the project's two configurations
  (default `Debug`, as the app); sanitizers and hardening flags only where
  `check_cxx_compiler_flag` / `check_linker_flag` accept them (the app's
  probes, done by CMake), Windows rules as the app's (no AddressSanitizer,
  the UBSan trap, `-static`); MSVC gets `/permissive- /Zc:__cplusplus
  /utf-8` and `/W4`;
* Make: `make` (debug) or `make CONFIG=release`; sanitizers on Linux debug
  builds unless `SANITIZE=0`, hardening unless `HARDEN=0`, with the
  portable subset (README says how to turn either off when g++ lacks them);
  `-static` on Windows.
* Machine-only flags (extra flags, library profile paths, the diagnostics
  format, `-iquote <build>/gen`, `-pipe`) are never written. Library
  profiles arrive in M4; until then a project names no libraries.

### 4.2 Where and how

* `b2c-build::export`: `export_project(document, generated, target, mode)`
  writes the layout into `target` atomically per file, with the build
  folder's file rules (no link followed, names only from validated module
  names). `mode` is `New` (the folder must be missing or empty) or `Replace`
  (the folder must hold a `.b2c-export.json`; only the files it lists are
  replaced or removed, anything else is kept, and a file the new export
  would write that the earlier one did not list refuses the export).
* CLI: `b2c generate <project> --out <dir> --export` writes the layout into
  `<dir>` (`--replace` to replace an earlier export). Without `--export` it
  writes the generated files as before, with the banner; `--export` without
  the layout (M2's meaning) is gone.
* App: `project_export_dialog { handle, document }` → `{ cancelled: true }`
  or `{ cancelled: false, folderName, files }`. The backend raises a native
  folder dialog, creates `<slug>/` in the chosen folder; when that already
  holds an earlier export it asks in a native dialog whether to replace it
  (default _Cancel_); a non-empty folder that is not an export gives `io`
  (`alreadyExists`). Allowed in Restricted Mode (it only writes code, as
  the code view shows it; 08 §8.3 disables Export-and-build, which does not
  exist yet). Menu: _Export as C++ project…_ in `≡`.
* Tests: unit tests of both files' text for each configuration, defines with
  quotes and backslashes, refused with `#`, `;` and `$`; replace keeps
  foreign files and never writes over one; refuses a link as target; `tests/export.rs` builds an exported example with `make`
  and, when present, `cmake --build` and runs it, comparing its output with
  the app's build (run when the tools exist, required with
  `B2C_REQUIRE_GXX`, `make` being in every CI image; `cmake` checked when
  present).

## 5. Project settings page

A full-window page like Settings (`≡` → _Project settings…_; the
`projectSettings` screen), rather than a modal dialog: it reuses the pages'
frame (a heading that takes the focus, _Back_), and the editor stays mounted
under it. Three sections, each a native form control with a label:

* **Language:** C++ standard (`c++17`…`c++26`), _GNU extensions_.
* **Build:** for _Debug_ and _Release_: optimisation, debug information,
  sanitizers (address, undefined), warning level, warnings as errors,
  hardening. Defines: a list of name, type (`int`, `bool`, text) and value
  rows, with _Add_ and _Remove_; a name is checked as a C++ identifier as the
  user types (the backend's `B2C-T1019` remains the rule).
* **Run:** arguments (one row per argument, never a shell string; _Add_,
  _Remove_, _Move up_, _Move down_), working directory (project folder or
  sandbox), input file (_Choose…_, _Clear_; this computer), _Run in a
  separate terminal window_ (this computer).
* Options that M3's generator reads (`checkedIndexing`, `formattingStyle`)
  appear when the generator uses them; the page shows none it ignores yet.

_Save changes_ applies the project changes as one edit (an unsaved change,
`•`), like renaming, and closes the page; _Cancel_ and _Back_ discard them.
The machine settings apply at once (as on the Settings page). Every problem
is shown under its field (`aria-invalid`, `aria-describedby`, `role="alert"`)
once the field is changed, or for all fields after a refused _Save changes_,
which also moves the focus to the first one.

## 6. IPC changes (all additive; mirrored in `b2c-ipc`, `packages/ipc-types` regenerated)

| Command / type | Change |
| --- | --- |
| `run_close_input` | new: `{ runId }` → `{}` |
| `run_input_file` | new: `{ handle, action }` → `{ fileName, cancelled }` |
| `project_export_dialog` response | `{ cancelled, folderName, files }` (`folderName` `null` when cancelled) |
| `project_export_dialog` | new: `{ handle, document }` → `{ cancelled, folderName?, files? }` |
| `RunEvent.started` | adds `inputFile: boolean` and `externalTerminal: boolean` |
| `IpcError` | adds `inputClosed` and `noExternalTerminal` |
| settings `run` | adds `externalTerminal: boolean` (get and patch) |

Each new command goes into the command table, the desktop's handler list,
permissions and capability, and the isolation allowlist, as the
consistency tests require.

## 7. Files

**Owned:** `crates/b2c-process/src/pty/`, `crates/b2c-process/tests/pty_*.rs`;
`crates/b2c-build/src/run/`, `src/cache.rs`, `src/build_dir.rs`,
`src/session/record.rs`, `src/ide.rs`, new `src/export.rs`, their tests;
`crates/b2c-toolchain/src/discovery.rs`, `src/command.rs`;
`crates/b2c-ipc/src/` (commands, dto, error); `crates/b2c-app/src/build_run.rs`,
`backend.rs`, `settings.rs`, `dialogs.rs`, new `export.rs`;
`crates/b2c-store/src/settings.rs`; `crates/b2c-cli/src/`;
`apps/desktop/src-tauri/` (commands, capability, isolation allowlist);
`apps/desktop/src/features/build-run/`, `features/project/` and a new
`features/project-settings/`; `packages/ipc-types` (generated).

**Not touched** (W2's): `catalog/`, `crates/b2c-lang`, `crates/b2c-codegen`,
`crates/b2c-catalog`, `packages/blockly-ext/src`, `packages/catalog-gen`,
`apps/desktop/src/editor/toolbox/`, `editor/services/symbols.ts`,
`docs/spec/`, `CHANGELOG.md`, `docs/plans/next-steps.md`, `w2.md`,
`tests/assignments/`, `examples/`, `tests/golden/`.

## 8. Security notes

* No path crosses IPC: the input file and the export folder come from native
  dialogs raised by the backend; responses carry file names only.
* The input file is opened read-only, must be a regular file, and is never
  logged (its name only at debug level).
* The external terminal's script is a constant; the program and its
  arguments are positional parameters of `/bin/sh`. Terminals are found
  only in `/usr/bin` and `/bin`. On Windows no shell is involved.
* Export writes only under the folder the user chose, never follows links,
  and replaces only files its own manifest lists.
* Sandbox pruning keeps the build cache's deletion rules (§2.2).
* `B2C_EXTERNAL_TERMINAL` is an IDE-internal variable: the user's own
  `B2C_*` variables are still removed, and the init unit removes it.

## 9. Accessibility notes

The End input button and the dialog's controls are native buttons, inputs
and selects with visible labels; state changes (_Input ended_, _Exported to
`folder`_) are announced through the existing status region; the dialog
traps focus only while open and returns it.

## 10. Specification sections to change later (`docs/spec/` is read-only here)

* 02 §2.5.2: the three new commands, the `started` fields, the error codes;
  drop "Not in M2: project_export_dialog".
* 04 §4.1 (project settings dialog), §4.5 (End input, run options without
  the "In M2" paragraph), §4.10 (Export), §4.12 (external terminal setting).
* 05 §5.9: `run.externalTerminal` in `settings.json`.
* 06 §6.11: the layout and the flags as built; "in M1/M2" paragraph removed.
* 07 §7.2 (open-ended `g++-N`, done), §7.5.1 (sandboxes pruned, the lock
  file), §7.6.1 (S7-5), §7.6.2 (input file, external terminal, the init
  unit's pause), §7.6.5 (close-input), §7.9 (`--export --replace`).
* `docs/reference/cli.md`: `generate --export --replace`.

## 11. Needs from W2

None so far. The dialog's _Textbook style_ switch (`options.usingNamespaceStd`,
format 2) is W2's and W4's; the dialog leaves room for it.

## 12. Decisions for the owner (each with a recommendation)

| ID | Question | Recommendation |
| --- | --- | --- |
| W3-D1 | Where the input file is kept | In memory per project for the app session (this contract); persisting it per project in a machine file can follow in M4 if users ask |
| W3-D2 | External terminal per project or per computer | Per computer (`settings.json`), as here; the spec's "persisted per project" changes |
| W3-D3 | Linux terminal list | As §3.3; `x-terminal-emulator` first (Debian's choice) |
| W3-D4 | Windows window after the program ends | The init unit waits for a key (`_getch`, only in the external window) |
| W3-D5 | Export in Restricted Mode | Allowed (it runs nothing) |
| W3-D6 | Makefile defaults | Sanitizers in debug on Linux and hardening on, both switchable (`SANITIZE=0`, `HARDEN=0`) |
| W3-D7 | Close-input on Windows with a partly typed line | Accept that the line arrives with a line break (Linux: without) |
| W3-D8 | Project settings as a page or a modal dialog | A page, as built (the spec says dialog); a modal can follow if usability tests ask for one |
| W3-D9 | Linux terminals with a server process (`gnome-terminal`) | Used last, with `--wait`; the program then runs outside the app's containment, which the contract states |
