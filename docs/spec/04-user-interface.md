# 4. User Interface

> Status: **Draft v0.1**

## 4.1 Main window layout

```
┌──────────────────────────────────────────────────────────────────────────────────────────────┐
│ ☰  Guessing Game ▾   │ main ✕ │ player ✕ │ ＋ │        [Debug ▾]  ▶ Run   ■ Stop   🔨   ⚙  │  ← title/tool bar
├──────────┬───────────────────────────────────────────────────────────┬───────────────────────┤
│ ● Program│  🔍 search   ⤢ tidy   ◱ collapse all          zoom − ＋ ⟲  │  C++   Outline   Help │
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
│  Console  │  Problems (2)  │  Build output  │                                   ⬚ ↕ ✕         │  ← bottom dock
│  Guess a number from 1 to 100!                                                               │
│  Your guess: 50▌                                                                              │
├──────────────────────────────────────────────────────────────────────────────────────────────┤
│ ✔ g++ 15.2.0 (MSYS2 UCRT64)  │  C++20  │  Debug  │  ⚠ Contains Raw C++ (1)  │  Saved 10:42   │  ← status bar
└──────────────────────────────────────────────────────────────────────────────────────────────┘
```

* All docks are resizable, collapsible and remembered per user. The right dock
  and bottom dock can be swapped or undocked into a split.
* **Focus mode** (`F11`) hides everything except the canvas and toolbox.
* **Presentation mode** enlarges blocks and fonts for teaching on a projector.

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

## 4.4 Diagnostics UX

| Where | What |
|-------|------|
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

## 4.5 Console (integrated terminal)

* xterm.js connected to the program's pseudo-terminal: colours, cursor
  movement, line editing and `Ctrl+C` all behave like a real console.
* The header shows state (`Running` / `Exited with code 0` / `Crashed: tried to
  access memory it shouldn't (segmentation fault)`), elapsed time,
  **■ Stop**, **⟲ Run again** and **Clear**.
* **Run options** (persisted per project): command-line arguments (an argv
  list editor, never a shell string), working directory (project folder or
  sandbox folder), stdin from file, and *Run in external terminal*.
* Output is batched to at most 60 updates/s. Scrollback is capped (default
  10,000 lines). Clipboard writes via escape sequences (OSC 52) are disabled.
  Hyperlinks (OSC 8) are shown but open only after confirmation and only for
  `http`/`https`.

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
* **Save.** Explicit `Ctrl+S`; the title shows `•` when there are unsaved
  changes; closing with unsaved changes prompts.
* **Untrusted project.** The open flow shows the Restricted Mode banner
  ([08 §8.3](08-security.md#83-workspace-trust-and-restricted-mode)).
* **Export.** *File → Export as C++ project…* writes `src/`,
  `CMakeLists.txt`, `Makefile` and `README.md` to a chosen folder
  ([06 §6.11](06-compiler-pipeline.md#611-export)).

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
