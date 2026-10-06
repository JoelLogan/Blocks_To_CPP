# Manual test: M2 accessibility

This is the manual accessibility pass for milestone M2 (the desktop editor
MVP). [Spec §9.2](../spec/09-quality-and-delivery.md#92-testing-strategy) asks for a screen-reader
check with NVDA on Windows and Orca on Linux at each milestone, and
[spec §4.8](../spec/04-user-interface.md#48-accessibility) sets the bar: WCAG
2.2 AA for the app's chrome, ARIA roles and labels on every panel and
dialog, everything reachable with the keyboard, and colour never the only
signal.

The automated tests already cover a lot, so this pass covers only what they
cannot. What they check:

- axe-core runs on every panel, dialog, page and banner in the component
  tests (`apps/desktop/src/**/*.test.tsx`), and on the editor window
  (`src/app/layout/accessibility.test.tsx`).
- The Tab order through the window and the block editor is tested in
  `src/app/layout/accessibility.test.tsx`, `src/app/layout/tabOrder.test.tsx`,
  `src/panels/accessibility.test.tsx` and
  `src/features/accessibility.test.tsx`.
- The block editor's keys are tested in `src/editor/keyboard/*.test.ts`.
- Contrast ratios of the colour tokens in both themes, the focus rings,
  target sizes and reduced motion are tested in
  `src/app/layout/styles.test.ts`, which reads the style sheets.

What the tests cannot check: what a screen reader actually says, how the real
webview (WebView2 or WebKitGTK) draws focus and colours, zoom and reflow, and
the operating system's reduced-motion setting. That is this pass.

Run it on a release build of the commit you are signing off, once on Windows
with NVDA and once on Linux with Orca. Record the result in the sign-off
table at the end and file an issue for each failure, quoting the step.

## Before you start

1. Install the build under test and a supported g++ (GCC 11 or newer).
2. Windows: install the current NVDA release and use its default settings
   (browse mode on, *Speak typed characters* on). Linux: use Orca from the
   distribution (GNOME), with *Speak object under mouse* off.
3. Set the display scale to 100% and the window to at least 1280 × 800.
4. Open the *Guessing Game* template: *Start page → Guessing Game*, then save
   it anywhere. Most steps use it.
5. Keep a pointer device out of reach. Every step below is done with the
   keyboard only, unless the step says otherwise.

## 1. Start page and window

1. Start the app. The start page appears and the screen reader announces the
   heading **Start**.
2. Press Tab repeatedly. The focus visits, in this order: the main menu
   button, the toolbar, then the start page's *Empty project*,
   *Hello World* (and the other templates), *Open…*, each recent project and
   its *Remove … from the list* button, then the status bar. Each control is
   announced with its name and role ("Open…, button").
3. Every focused control shows a clearly visible focus ring.
4. Open a template with Enter. The editor replaces the start page and the
   focus is not lost (the next Tab continues from the toolbar or the
   workspace, not from the top of the page).
5. *Main menu → Close project*. The start page comes back and the screen
   reader announces **Start** again.

## 2. Tab order through the editor

With the Guessing Game open, put the focus on the main menu button and press
Tab repeatedly. Expected order:

1. Main menu
2. Debug / Release drop-down
3. Run (F5), Stop (Shift+F5), Build (Ctrl+B), Settings
4. The toolbox (its selected category)
5. The toolbox's blocks (announced as **Blocks to add**)
6. The block canvas (announced as **Block canvas**, followed by the first
   block, for example "block “main”")
7. *Resize the C++ panel* (a separator), *Hide the C++ panel*
8. The C++ panel: *File* drop-down, *Copy all*, the code
9. *Resize the bottom panel*, the bottom tabs (Problems, Console, Build
   output: one stop, arrow keys switch tabs), *Hide the bottom panel*
10. The open tab's contents
11. The status bar's toolchain button

Shift+Tab goes the same way back. Check that:

- [ ] Every stop is announced with a name and a role.
- [ ] No stop is skipped, and the focus never disappears (nothing announced,
  no ring visible).
- [ ] Hidden things are never reached: a collapsed panel's contents, a tab
  that is not open.
- [ ] With a panel hidden (*Hide the C++ panel*), Tab goes from the canvas
  to its *Show the C++ panel* button, never into the hidden panel.

## 3. Block editor keys

These keys come from `KEY_MAP` in
`apps/desktop/src/editor/keyboard/help.ts`; a test keeps this table and that
map the same. Tab into the canvas and try each one. After each key the screen
reader announces where the focus is or what happened.

### On the canvas

| Keys | Action |
| --- | --- |
| ↓ / ↑ | Next or previous block |
| → / ← | Next or previous part of a block (fields and inputs) |
| Enter or Space | Edit the field; on the canvas itself, open the toolbox |
| M | Move the block (choose where it goes) |
| T | Open the toolbox to add a block here |
| Delete or Backspace | Delete the block |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy, cut or paste blocks |
| Ctrl+D | Duplicate the block |
| Ctrl+Z / Ctrl+Y | Undo or redo |
| Ctrl+Enter | Open the block’s menu |

### While moving a block

| Keys | Action |
| --- | --- |
| ↓ / ↑ (or → / ←) | Next or previous place for the block |
| Enter or Space | Put the block there |
| Escape | Leave the block where it was |

### In the toolbox

| Keys | Action |
| --- | --- |
| ↓ / ↑ | Next or previous category |
| → or Enter | Go to the category’s blocks |
| Escape | Back to the canvas |

### In the toolbox's blocks

| Keys | Action |
| --- | --- |
| ↓ / ↑ | Next or previous block or button |
| Enter or Space | Add the block (then choose where it goes), or press the button |
| ← | Back to the categories |
| Escape | Back to the canvas |

Check that:

- [ ] Blocks are announced by their text, for example "block “create int
  variable guess”", and a field by its name and value.
- [ ] M on the `print` block in `main`, then ↓, announces each place it could
  go ("after “…”", "into “…”", "loose on the canvas"); Enter puts it there;
  Escape puts it back where it was. A held block is drawn with a dashed
  outline.
- [ ] T, a category, a block and Enter adds the block where the focus was and
  starts a move, so you can choose where it goes.
- [ ] *Make a variable* in the toolbox's blocks opens the app's dialog; the
  focus goes into the dialog and comes back afterwards.
- [ ] Ctrl+C then Ctrl+V pastes a copy; Ctrl+Z undoes it.
- [ ] Tab always leaves the canvas, also while moving a block (the move is
  cancelled).
- [ ] Clicking a block with the pointer, then using the arrow keys, works
  without the yellow keyboard ring until a key is pressed.

## 4. Panels

1. Problems: make an error (empty a value input), open the Problems tab.
   - [ ] The grid is announced as **Problems** with its column headings. The
     arrow keys move between cells; Enter on a row selects the block in the
     canvas.
   - [ ] Each severity is read as a word ("Error", "Warning", "Info"); the
     icon is not read separately.
   - [ ] *Show C++ compiler message* expands and collapses (after a failed
     build) and its state is announced.
2. Console: run the program (F5).
   - [ ] While it runs, typing goes to the program, Tab is sent to the
     program, and Ctrl+Tab leaves the console. The terminal's description
     says so.
   - [ ] After it ends, Tab leaves the console as usual, and the exit state
     is announced in words ("Finished (exit code 0)").
   - [ ] A notice ("Running with IDE helpers", "Process group only") opens
     with Enter and its explanation is read.
3. Build output: build (Ctrl+B).
   - [ ] The log is one Tab stop, announced as **Build output**, and scrolls
     with the arrow keys.
4. C++ panel:
   - [ ] The code is read line by line with the arrow keys; *Copy all*
     reports "Copied".

## 5. Dialogs and pages

1. *Main menu → Close project* with unsaved changes. The dialog is announced
   with its title and text; the focus is on its first answer; Tab stays
   inside the dialog; Escape cancels; the focus returns to where it was.
2. *Settings*: the page's heading is announced; each setting is reachable;
   a group of choices is one stop and the arrow keys choose within it;
   *Back to the editor* returns the focus to the workspace.
3. The status bar's toolchain button opens the toolchain page; the same
   checks as for Settings apply.

## 6. Colour, contrast, zoom and motion

1. Switch the operating system between light and dark. Repeat in each:
   - [ ] All text is readable; every control's edge (drop-downs, text
     fields) and the selected bottom tab's mark are visible.
   - [ ] The focus ring is visible on every control, on the canvas, in the
     toolbox and in the panels.
2. Zoom: set the display scale to 200% (or the webview zoom with Ctrl++).
   - [ ] Nothing is cut off or overlaps; panels can still be resized and
     hidden; the dialogs fit the window.
3. Reflow: make the window 640 px wide (320 CSS px at 200%).
   - [ ] The start page and the dialogs reflow without scrolling sideways.
     (The editor itself needs more width; that is expected in M2.)
4. Turn on *Reduce motion* (Windows: *Settings → Accessibility → Visual
   effects → Animation effects* off; GNOME: *Settings → Accessibility →
   Reduce Animation*).
   - [ ] Dragging and connecting blocks shows no wiggle or connection
     animation, and deleting a block shows no shrinking animation.
   - [ ] Nothing slides or fades: toolbox categories, dialogs and panels
     change at once.
5. Colour is never the only signal:
   - [ ] Errors, warnings and infos have icons and words everywhere
     (Problems, gutter markers, the status bar).
   - [ ] "Not buildable" in the C++ panel and a non-zero exit in the console
     have an icon and words.

## 7. Target sizes

- [ ] Toolbar buttons, panel buttons, tabs, dialog buttons and the start
  page's buttons are at least 24 × 24 CSS pixels (check with the webview's
  developer tools in a debug build).
- [ ] Known limitation in M2: the splitter bars between the canvas and the
  panels are 6 px wide. They can be focused and moved with the arrow keys,
  and every panel has a *Hide* button, so a pointer user does not depend on
  the bar. Larger hit areas and a non-drag alternative for the bars and for
  block dragging (WCAG 2.5.7) are part of the M5 audit.

## Sign-off

| Platform | Screen reader | Build (commit) | Date | Tester | Result | Issues |
| --- | --- | --- | --- | --- | --- | --- |
| Windows 11 | NVDA | | | | | |
| Linux (GNOME) | Orca | | | | | |
