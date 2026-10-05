# 4. User Interface

> Status: **Draft v0.1**

## 4.1 Main window layout

```text
┌──────────────────────────────────────────────────────────────────────────────────────────────┐
│ ≡  Guessing Game ▼  │ main × │ player × │ + │   [Debug ▼]  ► Run  ■ Stop  Build  Settings    │  ← toolbar
├──────────┬───────────────────────────────────────────────────────────┬───────────────────────┤
│ ● Program│  Find   tidy   collapse all          zoom - + reset       │  C++   Outline   Help │
│ ● Vars   │                                                           │ ───────────────────── │
│ ● Math   │                                                           │ 1 #include <iostream> │
│ ● Logic  │            Blockly workspace (Zelos renderer)             │ 2                     │
│ ● Text   │                                                           │ 3 int main() {        │
│ ● Control│                                                           │ 4     int secret = …  │
│ ● Loops  │                                                           │   ▲ hover ↔ highlight │
│ ● I/O    │                                                           │                       │
│ ● Funcs  │                                                           │                       │
│ ● Collect│                                                [minimap]  │                       │
│ ● Types  │                                                           │                       │
│ …        │                                                           │                       │
├──────────┴───────────────────────────────────────────────────────────┴───────────────────────┤
│  Console  │  Problems (2)  │  Build output  │                                   □ ↕ ×        │  ← dock
│  Guess a number from 1 to 100!                                                               │
│  Your guess: 50_                                                                             │
├──────────────────────────────────────────────────────────────────────────────────────────────┤
│ √ g++ 15.2.0 (MSYS2 UCRT64)  │  C++20  │  Debug  │  ! Contains Raw C++ (1)  │  Saved 10:42   │  ← status bar
└──────────────────────────────────────────────────────────────────────────────────────────────┘
```

* All docks are resizable, collapsible and remembered per user. The right dock
  and bottom dock can be swapped or undocked into a split.
* **Focus mode** (`F11`) hides everything except the canvas and toolbox.
* **Presentation mode** enlarges blocks and fonts for teaching on a projector.

**In M2** (see §4.13 for everything that comes later):

* The top bar shows the project name, followed by `•` while there are unsaved
  changes. The window title does not change: the webview has no window
  permission ([08 §8.8](08-security.md#88-webview-and-ipc-hardening)).
* The **Debug / Release** dropdown in the toolbar lasts for the session,
  starts at *Debug* and is never saved in the project. The project settings
  dialog comes in M3.
* A project with several modules gets a simple module switcher, one canvas per
  module, without adding, renaming or deleting modules (M3). Frames and notes
  are not drawn yet, and are kept unchanged on save.
* Docks are resizable and collapsible. Swapping, splitting and remembering
  them, Focus mode and Presentation mode come in M5.

## 4.2 Toolbox

* **Scratch-style continuous toolbox.** Category bubbles on the left; the
  flyout scrolls continuously through all categories.
* **Dynamic categories.** *Variables* lists the symbols in scope at the
  current selection. *My Blocks* lists call blocks for every user-defined
  function, grouped by module. *Libraries* lists installed packs.
* **Beginner / Advanced.** Advanced categories and rarely used blocks are
  hidden until *Show advanced blocks* is enabled (per project, defaulting to
  the user preference).
* Toolbox search filters the flyout live (the same fuzzy index as Quick
  Insert).
* The categories, their order, icons, colours and entries come from
  `catalog/toolbox.toml` ([03 §3.11.1](03-block-language.md#3111-catalog-format)).
  Entries can carry **presets**, so one block appears in useful forms:
  *repeat until*, *if … else*, a new `int` variable starting at `0`, or *ask*
  with the prompt `"Your answer: "`. A new variable's start value follows its
  type (`0`, `0.0`, `true`, `'a'`, `""`), and a call from *My Blocks* comes
  with a default argument of each parameter's type.

**In M2:** the toolbox shows Program, Variables, Math, Logic, Text, Control,
Loops, Input / Output and Functions (*My Blocks*). *Variables* lists the
symbols in scope at the selected block (with nothing selected, those visible
at the end of `main`) and offers *Make a variable* (default names in
[03 §3.6](03-block-language.md#36-symbols-and-scoping)). The continuous flyout
uses `@blockly/continuous-toolbox` only if it passes the dependency policy
([08 §8.9](08-security.md#89-supply-chain)) and supports the dynamic
categories with Blockly 12.5; otherwise M2 uses Blockly's category toolbox and
records the difference. *Libraries* comes with library packs (M4), *Show
advanced blocks* and toolbox search in M3.

## 4.3 Code panel (live C++)

* A read-only CodeMirror 6 view of the generated C++ for the active module,
  with a file switcher (`main.cpp`, `main.hpp`, `b2c_support.hpp`).
* **Two-way highlighting.** Hovering or selecting a block highlights its C++
  range. Clicking a line in the code selects and scrolls to the block that
  produced it (via the source map, [06 §6.9](06-compiler-pipeline.md#69-source-maps)).
* Diagnostics are shown as gutter markers in the code panel as well as on
  blocks.
* *Copy all*, *Copy selection* and *Export project…*.
* The code panel follows the user's *code style* settings (indent width,
  brace style), which are generator options, not a post-formatter.
* **In M2** the code style is the indent width only, 2 or 4 spaces (default
  4); brace styles and the other options come in M5. The live preview and the
  build use the same value, and it is part of the build's configuration hash
  ([07 §7.5.1](07-toolchain-build-run.md#751-build-directory)), so the code
  that is shown is the code that is compiled. *Export project…* comes in M3.
* Invisible and format characters (Unicode category Cf, such as U+200B) in
  block text are shown as visible placeholders (`⟨U+200B⟩`) in block fields,
  tooltips and the code panel. The stored text is unchanged.

## 4.4 Diagnostics UX

| Where | What |
| ------- | ------ |
| On the block | A badge (✖ error, ⚠ warning, ℹ info) at the block's top-right corner. The block outline is tinted. Hovering shows the friendly message. |
| On an expression slot | Squiggly underline under the offending token |
| Problems panel | A sortable list: severity, message, module, block path ("main › repeat until › if"). Clicking selects the block. |
| Code panel | Gutter marker and underline |
| Toolbar | ▶ Run is disabled with a tooltip ("2 errors – click to see the first") when analyser errors exist (configurable) |

Each diagnostic has:

* a **friendly message** (catalogued, localisable)
* an optional **quick fix** (e.g. *Declare `total`*, *Convert explicitly*,
  *Add `return`*)
* **Show C++ compiler message** to reveal the raw g++ text
* a **Learn more** link to the diagnostics reference (bundled offline docs)
* for an analyser warning or info code (`W05xx` or `I05xx`), at whatever
  level it is shown, **Change level…**: off, info, warning, error or *not set*,
  either *for this project* (a change to the project: one undo step, saved
  with it) or *on this computer* (saved at once in the machine settings, for
  every project; Undo does not reverse it). *Not set* removes the entry, so
  the project's level or the default applies again. When this computer
  already sets the code, choosing a level *for this project* says that this
  computer's level still applies here. g++ and toolchain messages have no
  level.

Project settings and the Settings page each list every lint with its default
level, and each entry can be set back to *not set*. When this computer
overrides a project's level, Project settings shows both (*"Project: error ·
This computer: off"*), because the machine value wins
([06 §6.6](06-compiler-pipeline.md#66-stage--types-flow-checks-and-lints)).

**Run on errors** (a machine setting, [05 §5.9](05-project-format.md#59-machine-local-data))
changes only what the toolbar does:

* *Disable Run* (the default): ▶ Run is disabled while the analyser reports
  errors, with the tooltip *"N errors – click to see the first"*.
* *Show problems*: ▶ Run stays enabled; pressing it opens the Problems panel
  at the first error instead of building.

Either way the backend refuses to build or run a project with analyser errors
([07 §7.6.1](07-toolchain-build-run.md#761-preconditions)).

**Compiler messages from the last build.** g++ and linker diagnostics stay on
their blocks and in Problems after the project changes, dimmed and marked
*from the last build*, for as long as the content hash differs from the
build's `projectHash`. A diagnostic is dropped when the block it points to is
deleted, and all of them are replaced by the next build.

**Block paths** in Problems name each level by a short label: `main` for the
program block, the function's name for a function definition, and for other
blocks the friendly label up to its first placeholder, with the current
dropdown text filled in (*main › repeat until › if*).

**In M2:** *Learn more* opens the published diagnostics reference through
`open_help_link`; links to each code's entry and the bundled offline pages
come in M5. Quick fixes and squiggles under slot tokens come with expression
slots in M3, and *Change level…* with lint levels in M5.

## 4.5 Console (integrated terminal)

* xterm.js connected to the program's pseudo-terminal: colours, cursor
  movement, line editing and `Ctrl+C` all behave like a real console.
* The console is never a keyboard trap (§4.8). With no program running,
  `Tab` and `Shift+Tab` move the focus as usual. While a program runs they go
  to the program, and `Ctrl+Tab` moves the focus to the header's first
  enabled button; the terminal's accessible description names that key.
* The header shows state (`Running`, then the exit text of
  [07 §7.6.4](07-toolchain-build-run.md#764-exit-decoding): *Finished (exit
  code 0)*, *Finished with exit code 3*, *Stopped*, *Crashed: the program
  tried to use memory it doesn't own (segmentation fault / access
  violation)*, …), elapsed time,
  **■ Stop**, **⟲ Run again** and **Clear**. It also says *Running with IDE
  helpers* for IDE runs, and *process group only* when Linux cannot contain
  the program in a cgroup ([08 §8.14](08-security.md#814-residual-risks-accepted-documented-to-users)).
* **▶ Run while a program is running** stops it (the header shows
  *Stopped*), builds if needed and runs again, exactly like ⟲ Run again.
* **Run options** (persisted per project): command-line arguments (an argv
  list editor, never a shell string), working directory (project folder or
  sandbox folder), stdin from file, and *Run in external terminal*. **In M2**
  there is no editor for them (M3): a program gets the arguments and the
  working directory stored in the project, and a project that has never been
  saved runs in its sandbox folder.
* Output is batched to at most 60 updates/s. Scrollback is capped (default
  10,000 lines; 1,000–100,000 in Settings). When the program writes faster
  than the console can show, older output is dropped and replaced by a
  *"… N lines skipped"* marker ([07 §7.6.5](07-toolchain-build-run.md#765-run-limits)),
  or *"… output skipped"* when only part of one very long line was dropped.
  Clipboard writes via escape sequences (OSC 52) are disabled.
  Hyperlinks (OSC 8) are shown but open only after confirmation and only for
  `http`/`https`. **In M2** activating a link shows the full URL with a *Copy
  link* button and opens nothing.

## 4.6 Toolchain setup experience

* **First launch.** Toolchain discovery runs in the background. If a
  compatible g++ is found, the status bar shows it and nothing else happens.
* **No g++ found.** A friendly setup page explains what a compiler is and
  shows step-by-step instructions for the platform:
  * **Windows:** MSYS2 (recommended): install it, then run `pacman -S
    mingw-w64-ucrt-x86_64-gcc` in the *MSYS2 UCRT64* shell. Alternatively
    WinLibs via `winget install BrechtSanders.WinLibs.POSIX.UCRT`. *I installed
    it → Rescan* is available.
  * **Linux:** distro-specific commands (`sudo apt install g++`, `sudo dnf
    install gcc-c++`, `sudo pacman -S gcc`), detected from `/etc/os-release`.
  * *Choose g++ manually…* opens a native file dialog.
* **Toolchain settings page.** Lists all discovered toolchains with version,
  target, location, capabilities (C++ standards, `std::format`, sanitizers,
  SARIF diagnostics) and health checks. The user can select the default.

**In M2** the setup page and the toolchain list are one page:

* The page asks the backend for `toolchain_setup_info`
  ([02 §2.5](02-architecture.md#25-ipc-surface)): the platform, whether any
  usable toolchain exists, and the Linux distribution's `ID` and `ID_LIKE`
  (read from `/etc/os-release`, or `/usr/lib/os-release`, at most 64 KiB,
  with a strict parser). `debian` and `ubuntu` show the `apt` command;
  `fedora`, `rhel` and `centos` show `dnf`; `arch` shows `pacman`; anything
  else shows all three.
* The MSYS2 and WinLibs links open the fixed help links `msys2Install` and
  `winlibs` in the system browser ([08 §8.8](08-security.md#88-webview-and-ipc-hardening)).
* The list shows each toolchain's version, target, flavour (such as *MSYS2
  UCRT64*), location, capabilities and problems, marks the selected one, and
  offers *Select as default*, *Rescan* and *Choose g++ manually…*. While
  background discovery runs, the list says so and updates when it finishes.
* When the selected toolchain is missing or unusable, a build falls back to
  the first usable one in discovery order and says so with the warning
  `B2C-T1022` ([07 §7.2](07-toolchain-build-run.md#72-discovery)), never
  silently.

## 4.7 Command palette and keyboard

* `Ctrl+Shift+P` opens the command palette (all commands, fuzzy searchable).
* `Ctrl+Space` opens Quick Insert ([03 §3.14](03-block-language.md#314-quick-insert-type-to-block)).
* **Full keyboard navigation** of the workspace, built on Blockly's keyboard
  navigation work: move between blocks and connections with the arrow keys,
  `Enter` to edit a field, `Ctrl+C`/`Ctrl+V`/`Delete`/`Ctrl+Z`/`Ctrl+Y` as
  usual.
* Default shortcuts: `F5` Run, `Shift+F5` Stop, `Ctrl+B` Build, `Ctrl+S` Save,
  `Ctrl+F` Find, `F12` Go to definition, `Shift+F12` Find references, `F2`
  Rename. All are remappable.

**In M2:** Blockly 12's keyboard navigation is on (core, or the maintained
plugin if it passes the dependency policy) for moving between blocks and
connections, connecting, editing fields, copy, paste and delete. The
shortcuts `F5`, `Shift+F5`, `Ctrl+B` and `Ctrl+S` work; the command palette,
remapping, Find, Go to definition, Find references and Rename come in M5, and
Quick Insert in M3. Full screen-reader announcements for the workspace and
the accessibility audit are M5 work.

## 4.8 Accessibility

* WCAG 2.2 AA for all app chrome: contrast, focus visibility, target sizes and
  reflow.
* Screen-reader support: ARIA roles and labels for panels. Blocks are
  announced with their full textual form ("if guess less than secret, then, 1
  statement").
* Colour is never the only signal. Categories have icons, diagnostics have
  icons and text, and Raw C++ has a pattern and a badge.
* Themes: Light, Dark, High Contrast (light and dark), and a colour-blind-safe
  category palette. UI scale is adjustable from 75% to 200% independently of
  block zoom.
* Reduced motion is respected (no animated glow when the OS setting is on).
* Blockly's built-in prompt, confirm and alert boxes (for example when
  renaming) are replaced by the app's accessible dialogs, which use Radix UI
  primitives.
* **In M2** every panel and dialog is checked with axe-core in the component
  tests and reachable by keyboard. The High Contrast themes, the checked
  colour-blind-safe palette and UI scaling come in M5.

## 4.9 Localisation

* UI strings live in ICU MessageFormat catalogs (`src/i18n/<locale>.json`).
  Block labels live in catalog TOML under per-locale label tables.
* The generated C++ is always the same regardless of UI language. User
  identifiers are the user's own, and support helper names are English.
* Friendly diagnostics are localised. The raw g++ text is shown as produced
  (g++ runs with `LC_ALL=C.UTF-8` for parsing stability; see [07 §7.5](07-toolchain-build-run.md#75-compiling)).

## 4.10 Project lifecycle UX

* **Start page.** New project (templates: *Empty*, *Hello World*, *Guessing
  Game*, *Text Adventure*, *Class Example*, …), *Open…*, recent projects,
  example gallery.
* **Autosave.** A recovery snapshot is written every 30 s and on blur. After a
  crash, the start page offers to restore it.
* **Save.** Explicit `Ctrl+S`; the top bar shows `•` after the project name
  when there are unsaved changes (§4.1); closing with unsaved changes prompts.
* **Untrusted project.** The open flow shows the Restricted Mode banner
  ([08 §8.3](08-security.md#83-workspace-trust-and-restricted-mode)).
* **Export.** *File → Export as C++ project…* writes `src/`,
  `CMakeLists.txt`, `Makefile` and `README.md` to a chosen folder
  ([06 §6.11](06-compiler-pipeline.md#611-export)).

**In M2:**

* **Templates** are a closed set bundled in the app: *Empty* (one module
  `main` with an empty *when program starts* block) and *Hello World*. The
  other templates and the example gallery come with the full set in M5; the
  guessing game is built from *Empty*. A new project gets a fresh project ID
  and the default C++ standard from the settings, and it is trusted, so it can
  be built and run before its first save
  ([08 §8.3](08-security.md#83-workspace-trust-and-restricted-mode)).
* **Recent projects** lists up to 10 entries, newest first, with the project
  name and where it is. An entry whose file is gone stays in the list; opening
  it says so and offers to remove it.
* **One project per window.** Opening or creating another project first asks
  about unsaved changes and then closes the current one.
* **Unsaved changes.** Moving blocks counts as a change, because block
  positions are saved; scrolling and zooming do not, but the current viewport
  is written into the file whenever it is saved. Closing the window, opening
  another project and quitting all ask *Save*, *Don't save* or *Cancel*
  ([02 §2.6](02-architecture.md#26-process-model-and-concurrency)).
* **Recovery.** While the project has unsaved changes, a snapshot is written
  every 30 s and when the window loses focus ([05 §5.10](05-project-format.md#510-saving-and-recovery)).
  After a crash, the start page lists the snapshots of app instances that are
  no longer running and offers *Restore* or *Discard*. Several instances of
  the app can run at once without offering each other's snapshots.
* **Changed on disk.** When the open file changes outside the app, a dialog
  offers *Reload* or *Keep mine (save as…)*. When the file was deleted or
  renamed, only *Keep mine* is offered. Saving never overwrites a file that
  changed on disk.
* **Untrusted project.** The Restricted Mode banner explains why Build and Run
  are disabled and offers *Trust…*, which opens the backend's native trust
  dialog.
* *Export* comes in M3.

## 4.11 Debugger (later phase)

* Click a statement block's left edge to toggle a **breakpoint** (red dot).
* **Debug ▶** runs under GDB (MI mode). The executing block glows, and *Step
  over / Step into / Step out / Continue* work at **block granularity**
  (stepping until the source map points to a different block).
* The **Variables panel** shows in-scope symbols with their block-level names
  and values. The **Call stack** shows function blocks.
* **Trace run** (no debugger needed) instruments the build to report the
  current block through the event side channel, giving Scratch-like "glow"
  while the program runs, throttled to 60 Hz ([07 §7.7](07-toolchain-build-run.md#77-runtime-event-side-channel)).

## 4.12 Settings page

The Settings page edits the machine settings
([05 §5.9](05-project-format.md#59-machine-local-data)). A change is saved at
once and applies to every project on this computer; nothing on the page is
stored in a project.

* **Code style:** the indent width of the generated C++, 2 or 4 spaces (§4.3).
* **Run on errors:** *Disable Run* or *Show problems* (§4.4).
* **Console:** the scrollback, 1,000 to 100,000 lines.
* **Toolchain:** a link to the toolchain page (§4.6), where the default
  compiler is selected.
* **Build cache:** *Clear build cache* deletes every cached build that is not
  in use and reports the space freed
  ([07 §7.5.1](07-toolchain-build-run.md#751-build-directory)). It asks for
  confirmation inside the app; it is not a security decision, so there is no
  native dialog.
* When `settings.json` held an invalid value, was corrupt or came from a newer
  version, the page says which settings were reset or left as they were.

Lint levels, the build and run settings (M5) and the machine-local extra
flags and environment pass-through list (M4) join the page later.

## 4.13 What the M2 editor includes

M2, the desktop editor MVP ([10 §10.1](10-roadmap.md#101-milestones)),
implements the parts of this chapter marked *In M2* above. Everything below
arrives in a later milestone, and **M2 has no UI entry point for it**: no
greyed-out menu items, buttons or shortcuts that lead nowhere.

| Milestone | Not in M2 |
| --- | --- |
| M3 | Typing in expression slots, *Expand* / *Collapse* and Quick Insert (`Ctrl+Space`); the type picker popover; collection, string, struct, enum, file, error, random and time blocks; multi-module tabs and headers (adding, renaming and deleting modules); *Export* (`project_export_dialog`); the friendly g++ message catalog; globals, constants, math functions, conversions and organisation blocks; Textbook style; the Friendly / C++ label switch and *Show advanced blocks*; toolbox search; the project settings dialog (standard, options, configurations, defines); run options (arguments, working directory, stdin file, external terminal); quick fixes and *Copy bug report*; feature gating in the UI |
| M4 | Classes, lambdas, memory, templates and concurrency blocks; library packs and the *Libraries* category; Raw C++ blocks and the *Contains Raw C++ (n)* indicator; the extra-flags and environment pass-through settings |
| M5 | Outline, frames, search, rename (`F2`), go to definition (`F12`), find references (`Shift+F12`), snippets, minimap, tidy-up, collapse to signatures, back/forward and bookmarks; the debugger, breakpoints, trace glow and runtime errors on blocks (§4.11); localisation; the command palette and remappable shortcuts; Focus and Presentation modes; swapping, splitting and remembering docks; High Contrast themes and the checked colour-blind-safe palette; lint levels (*Change level…*, lint lists); build and run settings (timeouts, cache size, link mode, caps); the example gallery and the full template set; *Export diagnostics bundle* |
| M6 | The updater and the first-run update question, sandboxed runs, installers |
