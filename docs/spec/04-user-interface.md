# 4. User Interface

> Status: **Draft v0.1**

## 4.1 Main window layout

```text
┌──────────────────────────────────────────────────────────────────────────────────────────────┐
│ ≡  Guessing Game ▼  │ main × │ player × │ + │   [Debug ▼]  ► Run  ■ Stop  Build  Settings    │  ← toolbar
├──────────┬───────────────────────────────────────────────────────────┬───────────────────────┤
│ ● Program│  Find   tidy   collapse all     zoom − + 100% show all    │  C++   Outline   Help │
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

* The `≡` main menu at the left of the top bar has *New project…*, *Open…*,
  *Save* (`Ctrl+S`), *Save as…*, *Rename project…*, *Undo* (`Ctrl+Z`),
  *Redo* (`Ctrl+Y`) and *Close project*; the items about the open project
  appear only while one is open. *Undo* and *Redo* are for the block editor
  while it is shown, and work wherever the focus is except in text fields,
  dialogs, the open menu and a running program's console (`Ctrl+Shift+Z`
  also redoes); in the menu they are greyed out while there is nothing to
  undo or redo. *Open…* starts in the documents folder (or the home folder),
  and *Save as…* in the project's own folder (for a project without a file,
  as *Open…* does); neither ever starts in the app's working folder
  ([07 §7.2](07-toolchain-build-run.md#72-discovery)).
* The top bar shows the project name, followed by `•` while there are unsaved
  changes; a name too long for the bar is cut off with `…` before the `•`,
  which stays in view. Clicking the name (or *Rename project…*) edits it in
  place: Enter or moving the focus away keeps it, Escape cancels, and
  `Ctrl+S`, `Ctrl+B` and `F5` keep it before they save, build or run. A name
  has 1–100 characters on one line, without control or bidirectional-override
  characters, and spaces at either end are removed. Enter (or one of those
  keys) on a name that breaks these rules says why and keeps the field open,
  and the key does nothing else; moving the focus away then keeps the old
  name. Renaming is an unsaved change, is saved as
  `project.name` and never renames the file; *Save as…* suggests the name as
  the file name while the project has no file yet. Undo does not undo a
  rename: rename the project again instead. The window title does not change:
  the webview has no window permission
  ([08 §8.8](08-security.md#88-webview-and-ipc-hardening)).
* The **Debug / Release** dropdown in the toolbar lasts for the session,
  starts at *Debug* and is never saved in the project. The project settings
  dialog comes in M3.
* A project with several modules gets a simple module switcher, one canvas per
  module, without adding, renaming or deleting modules (M3). A switch asked
  for while a block is dragged (a problem in another module chosen, or another
  module) is made once the block is dropped. Frames and notes are not drawn
  yet, and are kept unchanged on save.
* A block's menu offers Cut, Copy, Paste, Duplicate, Add comment,
  Collapse/Expand, Disable/Enable and Delete; the canvas menu offers Undo,
  Redo, Paste, *Clean up blocks* (lines the top-level blocks up in a column),
  Collapse/Expand blocks and *Delete N blocks*, which counts the blocks it
  deletes (not their expression slots) and asks with *Cancel* as the default.
  One Undo brings all of them back.
* Dropping a block on the trash can deletes it; the trash can keeps nothing,
  and Undo brings deleted blocks back.
* The canvas has *Zoom out*, *Zoom in*, *Reset zoom* (back to 100 % around
  the centre; it shows the zoom now) and *Show all blocks*, which scrolls to
  the blocks without changing the zoom: it centres them where they fit, and
  otherwise shows their top-left corner. `Ctrl` + wheel or a pinch zooms
  around the pointer; `Ctrl+=`, `Ctrl+-` and `Ctrl+0` zoom from the keyboard,
  on the canvas and on the controls. The controls are named buttons at the
  top right of the canvas, never over the toolbox, and reachable with Tab
  after the canvas.
* On first start the window opens at 1280 × 800, made smaller when the
  screen's work area is smaller (where the system names no primary monitor
  and knows no work area, as under Wayland, the app fits it to the monitor it
  opens on, less 64 × 100 px for panels and the title bar). The C++ dock
  starts at 30 % of the window's width (280–480 px) and the bottom dock at
  28 % of its height (120–280 px); each follows the window's size until it
  is resized. Each dock takes at most 65 % of the editor's area (the window
  less the toolbar, any banner and the status bar), and the C++ dock never
  leaves the canvas less than 200 px beside the toolbox's flyout at its
  narrowest; resizing the window keeps these limits. The toolbox's flyout and
  the docks are resizable by pointer and keyboard, and the docks are
  collapsible. When less than 200 px of canvas would remain, the flyout
  narrows first, then the C++ dock, which is hidden when even its narrowest
  width does not fit and shown again when it fits. The flyout's width, the
  docks' sizes and whether each dock is hidden are remembered on this
  computer ([05 §5.9](05-project-format.md#59-machine-local-data)); a dock
  hidden only because it did not fit is remembered as shown. Swapping and
  splitting docks, Focus mode and Presentation mode come in M5.
* A canvas opens with the view it was saved with; one saved without a view
  (a template) shows its blocks in the middle of the canvas, or their
  top-left corner when they do not fit, clear of the toolbox.
* The status bar says *Saved* (with the time after a save in this session)
  or *Unsaved* while there are unsaved changes or the project has no file
  yet.
* Menus, hints and dialogs are always drawn above the canvas, the toolbox and
  dragged blocks; opening a dialog (the console's link dialog included) or a
  full-window page such as Settings first closes the block editor's
  drop-downs and field editors.

## 4.2 Toolbox

* **Scratch-style continuous toolbox.** Category bubbles on the left; the
  flyout scrolls continuously through all categories, and its blocks always
  show current names. Choosing a category scrolls the flyout to it, and that
  category stays selected at every zoom level; scrolling by hand selects the
  category at the top. The flyout shows its blocks at one size whatever the
  canvas zoom (three quarters of their size at 100 %), is as wide as the user
  drags its edge with the *Resize the toolbox* splitter (from 160 px; by
  default as wide as its widest block, up to 320 px), is opaque, and cuts off
  a block wider than itself (which can still be dragged out). When less than
  200 px of canvas would remain beside it, the flyout narrows, down to
  160 px. Rebuilding a dynamic category never moves what the flyout shows:
  the category in view stays in view.
* **Dynamic categories.** *Program* offers *when program starts* only while
  the project has none; otherwise it says where the program starts.
  *Variables* offers *Make a variable* (name and type), the `create` block,
  one `set`, and, when a number or character variable exists, one `change`,
  then a getter for each variable of the module, grouped by the function
  that declares it (at most 50, then a note of how many more there are).
  It does not depend on the selection. A *set* or *change* block dragged
  from the toolbox comes with a variable already chosen; if that variable is
  not visible where the block is dropped, the block takes the first fitting
  variable visible there, as part of the same drop (one Undo removes it). A getter keeps its variable: if the variable is
  not visible where it is dropped, the block shows `B2C-E0203` and its menu
  offers the variables that are. A block still in the toolbox lists every
  variable of the module in its menu, grouped like the category.
  *Functions* lists *define* and *return*, then, under *Your functions*, a
  call block for each function of the module shown, or a short hint when
  there is none; module headings appear only when the project has more than
  one module. A call block of a known function has as many arguments as the
  function has parameters: it shows no ⊖/⊕ while they match, and only the
  one that brings it back in line when the definition changes. *Libraries*
  lists installed packs.
* **Rebuilds.** The flyout is rebuilt only when what the dynamic categories
  show changes (the module's variables and functions, whether the project
  has *when program starts*, the modules), never while a block is dragged, a
  field is edited, a menu is open or a category scroll runs; the selected
  category stays selected.
* **Beginner / Advanced.** Advanced categories and rarely used blocks are
  hidden until *Show advanced blocks* is enabled (per project, defaulting to
  the user preference).
* Toolbox search filters the flyout live (the same fuzzy index as Quick
  Insert).
* The categories, their order, icons, colours and entries come from
  `catalog/toolbox.toml` ([03 §3.11.1](03-block-language.md#3111-catalog-format)).
  Entries can carry **presets** that fill in values (a new `int` starting at
  `0`; *ask* with `"Your answer: "`). A new variable's start value follows
  its type (`0`, `0.0`, `true`, `'a'`, `""`), and so does the value of *set*
  when another variable is chosen in it while the value is still a start
  value, in the toolbox as on the canvas (a *set* taken out after its
  variable was chosen in the toolbox keeps that variable and its value); a
  call from *Your functions* comes with a default argument of each
  parameter's type. A block whose forms differ only by a dropdown or ⊕ part
  appears once
  ([03 §3.1](03-block-language.md#31-design-philosophy-few-blocks-much-power)).

**In M2:** the toolbox shows Program, Variables, Math, Logic, Text, Control,
Loops, Input / Output and Functions (with *Your functions*). The `create`
block, the counted loop and *define* get the first default name that is free
where they are dropped, and *Your functions* follows the functions as they
are defined (default names in
[03 §3.6](03-block-language.md#36-symbols-and-scoping)). *Make a variable*
offers `int`, `double`, `bool`, `char` and `string`. The menus of *set*
and *change* on the canvas list what is visible where the block is
([03 §3.6](03-block-language.md#36-symbols-and-scoping)). The continuous
flyout is `@blockly/continuous-toolbox` 7.0.9, the Blockly team's plugin and
its last release for Blockly 12: it passes the dependency policy
([08 §8.9](08-security.md#89-supply-chain)) and shows the dynamic categories
in the same scrolling flyout, so M2 needs no fallback to Blockly's category
toolbox. *Libraries* comes with library packs (M4), *Show advanced blocks* and
toolbox search in M3.

## 4.3 Code panel (live C++)

* A read-only CodeMirror 6 view of the generated C++ for the active module,
  with a file switcher (`main.cpp`, `main.hpp`, `b2c_support.hpp`).
* **Two-way highlighting.** Hovering or selecting a block highlights its C++
  range. Clicking a line in the code selects and scrolls to the block that
  produced it (via the source map, [06 §6.9](06-compiler-pipeline.md#69-source-maps)).
* Diagnostics are shown as gutter markers in the code panel as well as on
  blocks.
* *Copy all*, *Copy selection* and *Export project…*.
* *Copy all*, *Copy selection*, the setup page's *Copy* (§4.6) and the link
  dialog (§4.5) confirm with *Copied* for 3 seconds and announce every copy;
  *Could not copy* stays until the next attempt. The confirmation never moves
  the code.
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
| Problems panel | A sortable list: severity, message, module, block path ("when program starts › repeat until › if"). Clicking selects the block. |
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

**Parked blocks.** A block left loose on the canvas shows the ⚠ badge of
`B2C-W0505` and is listed in Problems, but ▶ Run stays enabled; the program
runs without it ([ADR-0012](../adr/0012-parked-blocks.md)).

**Compiler messages from the last build.** g++ and linker diagnostics stay on
their blocks and in Problems after the project changes, dimmed and marked
*from the last build*, for as long as the content hash differs from the
build's `projectHash`. A diagnostic is dropped when the block it points to is
deleted, and all of them are replaced by the next build.

**Block paths** in Problems name each level by a short label: the program
block's label (*when program starts*; `main()` with C++ labels, M3), the
function's name for a function definition, and for other blocks the friendly
label up to its first placeholder, with the current dropdown text filled in
(*when program starts › repeat until › if*).

**In M2:** *Learn more* opens the published diagnostics reference through
`open_help_link`; links to each code's entry and the bundled offline pages
come in M5. Quick fixes and squiggles under slot tokens come with expression
slots in M3, and *Change level…* with lint levels in M5.

**In M2** a canvas that cannot be read back as a project (a `B2C-E01xx`
problem, such as blocks nested too deeply) does not go unnoticed: a banner
says that the latest change was not checked, the code, Problems and Run keep
showing the last version that could be, and Build and Run refuse with a dialog
that names the problem until the change is undone; the banner's *Undo last
change* undoes it. Until then the editor also stays on that module: choosing
another module, or a problem in another one, does not switch, because a switch
would clear the undo history and keep a module that no save, build or recovery
snapshot can write. When the canvas went past a limit that editing can reach
(blocks nested too deeply, `B2C-E0104`; a project file over 32 MiB,
`B2C-E0101`; more than 100,000 blocks, `B2C-E0121`), the banner and a refused
Save name the limit and how to get back within it; any other such problem is
reported as a bug. When the compiler core was restarted after a crash, a
dismissible banner says so.

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
* **In M2** opening or creating another project resets the console: output
  still queued from the old program is dropped and the terminal is fully
  reset.
* **Between runs** the console keeps earlier output as scrollback and never
  writes over it. Every run starts with the terminal's modes reset (the
  alternate screen left, colours, cursor visibility, scroll region) without
  moving the cursor, below a dim *── New run ──* line when there is output
  since the last Clear, and the view scrolls to the end. On Windows, whose
  pseudoconsole clears the screen and paints each new program from the
  top-left corner, the console first scrolls everything it shows, that line
  included, into the scrollback.
* **Clear** empties the screen and the scrollback; while a program runs, the
  line with the cursor (such as a prompt waiting for input) stays.
* The console scrolls by itself, never the panel around it, at any display
  scale and while a program writes; output arriving while the user has
  scrolled up does not move the view (a new run does). The mouse wheel
  scrolls the scrollback by whole lines; on a program's full screen it sends
  arrow keys, and a program that asks for mouse events gets it. The terminal
  is fitted to whole rows and columns inside its padding; while the console
  is resized, rows follow at once and columns once the width has settled
  ([07 §7.6.2](07-toolchain-build-run.md#762-spawning)).
* Program output is available to screen readers: the rows on screen are read
  line by line, moving past the first or last row scrolls through the
  scrollback, and new output is announced in batches, at most once a second.
  Output that pours in faster than anyone can listen to (more than 16 KiB
  given to the terminal at once) is not announced; a second after such a
  flood the rows on screen are offered again. Clear, opening another project
  and each run start the screen reader's view afresh, so a run's output is
  announced however much the run before printed.

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
* Each install command has a *Copy* button, which confirms like the code
  panel's (§4.3).
* While the first discovery runs and no usable toolchain is known yet,
  ▶ Run stays enabled: the build waits for the discovery (at most 30 s,
  [07 §7.2](07-toolchain-build-run.md#72-discovery)) and the build output
  says so. Only when discovery has finished without a usable g++ does Run
  open the setup page.

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

**In M2:** keyboard navigation is the app's own editor plugin on Blockly 12's
core focus manager, cursor and shortcut registry (the maintained
`@blockly/keyboard-navigation` plugin would replace the validated clipboard,
drag blocks past the editing session and write HTML with `innerHTML`). The
arrow keys move between blocks and their fields and inputs, `Enter` edits a
field, `M` picks a block up so the arrow keys choose where it goes (the
places a pointer drag could drop it, type rule included; `Enter` puts it
there, `Escape` cancels; an empty C-block goes between two statements
where a pointer drag would wrap them, since wrapping
([03 §3.3](03-block-language.md#33-block-shapes)) comes to the keyboard
with the non-drag alternatives in M5), `T` opens the toolbox, whose
categories and blocks are reached with the arrow keys (adding a block from
it is one undo step; `↓`/`↑` also stop on its buttons, such as *Make a
variable*, its headings and its notes, and a getter is announced with the
group it is listed in), and copy, paste, duplicate, delete, undo and the
block menu (`Ctrl+Enter`)
act on the keyboard focus. `Ctrl+=` / `Ctrl+-` zoom the canvas around its
centre and `Ctrl+0` resets the zoom to 100 %, and the new zoom is announced;
`Ctrl` + wheel and a pinch zoom around the pointer, and the plain wheel
scrolls. The shortcuts `F5`, `Shift+F5`, `Ctrl+B` and `Ctrl+S` work, and
`Ctrl+Z` and `Ctrl+Y` undo and redo the block editor's changes also while the
focus is outside the canvas (§4.1); the command palette, remapping, Find, Go
to definition, Find references and Rename come in M5, and
Quick Insert in M3. Full screen-reader announcements for the workspace and
the accessibility audit are M5 work. Copy, cut, paste and duplicate
(`Ctrl+D`) use the validated clipboard format
([05 §5.12](05-project-format.md#512-clipboard-format)). `Ctrl+R`,
`Ctrl+Shift+R` and `Ctrl+F5` never reload the window, and the webview's own
context menu (which offers *Reload*) appears only over editable text, the
console and selected text.

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
  tests and reachable by keyboard, and the Tab order runs toolbar, toolbox,
  the toolbox's blocks, canvas, the canvas's zoom controls, the *Resize the
  toolbox* separator, then the docks. The canvas and the toolbox's blocks
  have names and describe their keys; a polite live region announces the
  block or place the keyboard reaches; Blockly's own animations, and the
  toolbox's scroll to a category, follow the reduced-motion setting; focus
  returns where it was when a dialog closes, the block canvas included; and
  it stays in the editor when the focused block is deleted or the toolbox's
  blocks are rebuilt.
  The manual screen-reader pass (NVDA, Orca) follows
  [the M2 accessibility checklist](../manual-tests/m2-accessibility.md). The
  High Contrast themes, the checked colour-blind-safe palette, UI scaling,
  24 px splitter targets and non-drag alternatives come in M5.
* **In M2** the app and the blocks follow the system's light or dark setting;
  the console is dark in both. Controls the webview draws itself, such as the
  Debug/Release drop-down, are styled explicitly, so their contrast holds in
  both themes; this is checked in the running app on both systems. A page's
  heading takes the focus when the page opens, and shows its focus ring once
  the keyboard is in use (after Tab).

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
  A save asked for while another is still running is never dropped: it runs
  after that one and writes what the editor shows then.
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
  about unsaved changes and then closes the current one. The current project
  is closed only once the other one is shown, so a cancelled dialog or a file
  that does not load leaves it open. A new project that has not been edited
  has no unsaved changes.
* **Unsaved changes.** Moving blocks counts as a change, because block
  positions are saved; scrolling and zooming do not, but the current viewport
  is written into the file whenever it is saved. Closing the window, opening
  another project and quitting all ask *Save*, *Don't save* or *Cancel*
  ([02 §2.6](02-architecture.md#26-process-model-and-concurrency)).
* **Recovery.** While the project has unsaved changes, a snapshot is written
  every 30 s and when the window loses focus ([05 §5.10](05-project-format.md#510-saving-and-recovery)).
  After a crash, the start page lists the snapshots of app instances that are
  no longer running and offers *Restore* or *Discard*. Several instances of
  the app can run at once without offering each other's snapshots. Restoring
  waits for, and holds back, New, Open and Save like they hold back each
  other: it asks about the open project's unsaved changes first and closes
  that project only once the restored one is shown. *Discard* asks for
  confirmation, because discarded work cannot be recovered. Autosave pauses
  while a save runs, and a save waits for a snapshot being written.
* **Changed on disk.** When the open file changes outside the app, a dialog
  offers *Reload* or *Keep mine (save as…)*. When the file was deleted or
  renamed, only *Keep mine* is offered. Saving never overwrites a file that
  changed on disk. *Not now* (or closing the dialog) changes nothing; the
  next save is refused again and asks again. When there are unsaved changes,
  *Keep mine* is the suggested answer and *Reload* says that it discards them.
  Reload waits for a save in progress, deletes the recovery snapshot of the
  changes it replaces, and checks trust again.
* **Untrusted project.** The Restricted Mode banner explains why Build and Run
  are disabled and offers *Trust…*, which opens the backend's native trust
  dialog, whose default button is *Stay in Restricted Mode*. The banner stays
  under the toolbar while the project is restricted. *Revoke trust* is on the
  Settings page, in a *This project* section that says why the open project
  is trusted (its own record, its folder, or created here); it is offered
  only for a project's own record.
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
  When the app starts with such notices, a banner under the toolbar says so
  until the Settings page is opened or the banner is dismissed.

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
| M5 | Outline, frames, search, rename (`F2`), go to definition (`F12`), find references (`Shift+F12`), snippets, minimap, tidy-up beyond *Clean up blocks*, collapse to signatures, back/forward and bookmarks; the debugger, breakpoints, trace glow and runtime errors on blocks (§4.11); localisation; the command palette and remappable shortcuts; Focus and Presentation modes; swapping and splitting docks; High Contrast themes and the checked colour-blind-safe palette; lint levels (*Change level…*, lint lists); build and run settings (timeouts, cache size, link mode, caps); the example gallery and the full template set; *Export diagnostics bundle* |
| M6 | The updater and the first-run update question, sandboxed runs, installers |
