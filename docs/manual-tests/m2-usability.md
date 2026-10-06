# Manual test: M2 usability ("without reading docs")

Milestone M2 ends when "a first-time user builds and runs the guessing game
([03 §3.13.1](../spec/03-block-language.md#3131-guessing-game-input-random-loops-branching))
without reading docs"
([10 §10.1](../spec/10-roadmap.md#m2-desktop-editor-mvp)). The end-to-end
tests prove that the game can be built and run on both systems
([09 §9.2](../spec/09-quality-and-delivery.md#92-testing-strategy)): they
drag the blocks, play the game and check every line it prints. They cannot
prove that a person who has never seen the app finds the way on their own.
This protocol checks that, with at least three first-time users on Windows
and Linux.

The second part of this page is the **exclusion review** of the milestone
demo: nothing that a later milestone adds may have a button, menu item or
shortcut in M2
([04 §4.13](../spec/04-user-interface.md#413-what-the-m2-editor-includes)).

## Who takes part

- **At least three participants**, and both systems: at least one session on
  Windows and at least one on Linux. Three on each system is better.
- **First-time users**: people who have never used Blocks2Cpp, never seen its
  code or documentation, and did not take part in an earlier session. Prefer
  the people the app is for: learners with little or no C++. People who
  work on Blocks2Cpp cannot take part.
- Each participant agrees to the session, knows what is written down and may
  stop at any time. Participants are named only by an ID (`P1`, `P2`, …):
  no names, contact details or recordings go into the repository. A screen
  recording, if the participant agrees to one, stays outside the repository
  and is deleted once the notes are written.

## What you need

1. A build of the commit under test (`pnpm desktop:build`, or the executable
   only with `pnpm --filter @blocks2cpp/desktop tauri build --no-bundle`; see
   the [desktop app's README](../../apps/desktop/README.md#build)). Write the
   commit into each session record.
2. A computer with g++ 11 or newer installed and on the path, so the status
   bar names it when the app starts. (The setup page is covered by the
   end-to-end tests. To try it with a participant as well, run that session
   on a machine without g++ and add the time it takes to install it.)
3. A fresh profile: no settings, recent projects, trust records or recovery
   snapshots from earlier runs. Delete these folders before each session
   ([02 §2.7](../spec/02-architecture.md#27-persistence-locations)):
   - Windows: `%APPDATA%\Blocks2Cpp\` and `%LOCALAPPDATA%\Blocks2Cpp\`;
   - Linux: `~/.config/blocks2cpp/`, `~/.cache/blocks2cpp/` and
     `~/.local/state/blocks2cpp/` (or their `$XDG_…_HOME` equivalents).
4. A screen of at least 1280 × 800, a mouse, and the app started and showing
   its start page before the participant sits down. No browser window, no
   documentation and no example projects open.
5. A clock, the [task card](#the-task-card), the [hint list](#hints) and the
   [session record](#session-record) for notes.

## The task card

Give the participant this card, printed or on paper, and nothing else:

> **Build a number-guessing game in Blocks2Cpp, then play it once.**
>
> The computer picks a secret whole number from 1 to 100. You type guesses.
> After each guess it says "Too low!", "Too high!" or "Correct!", and it
> stops when you find the number.

Read it out loud too. Then say: "Please think aloud while you work: say what
you are looking for and what you expect to happen. I cannot help you with
the app, but I will answer questions about the game itself."

## Rules for the moderator

- **Do not explain the app.** Answer only questions about the game's rules
  (the range, what the messages say). To any other question say: "What
  would you try?"
- **No documentation**: no user guide, no website, no web search, no help
  from anyone else.
- **Hints only when stuck.** When the participant has made no progress for
  5 minutes, or asks for help twice in a row, give the next hint from the
  list below and note the time and the hint. Never give more than one hint
  at a time.
- **Prompt thinking aloud** when the participant is silent for a while: "What
  are you looking at now?"
- **Stop after 45 minutes** of work, or when the participant wants to stop.
- Note everything that slows the participant down, even when they recover on
  their own.

### Hints

Give them in this order, each at most once:

1. "The blocks are in the categories on the left."
2. "A block can be dropped into the slot of another block."
3. "Variables you created can be found in the Variables category."
4. "A block's small menus and the ⊕ buttons change what it does."
5. "The toolbar at the top runs the program; what the program prints
   appears at the bottom."

## What counts as success

The session is a **success without help** when, within 45 minutes and
without a hint, the participant's program:

1. builds and runs from ► Run (or F5);
2. picks a random number in the range from 1 to 100 (another range the
   participant chose on purpose counts too);
3. asks for guesses again and again, answering each with _Too low!_, _Too
   high!_ or _Correct!_ (or words with the same meaning);
4. stops after the right guess, and the console's header says _Finished
   (exit code 0)_;

and the participant has played it once to the end.

A session that needed hints is a **success with help**; one that did not end
with a working game is **not a success**. Both still count as sessions, and
their stumbling points are the most useful result.

## Session record

Copy this block once per session into the [results](#results) section and
fill it in.

```text
Participant: P_    System: Windows 11 / Linux (distribution, desktop)
Build (commit): ________    g++: ________    Date: ________    Moderator: ________
Experience: none / some programming / some C++ (as the participant says)

Times (minutes from reading the card):
  first block on the canvas: ____   first successful build: ____
  first run: ____                   game played to "Correct!": ____
  total: ____

Hints given (number, time): ________
Result: success without help / success with help / not a success

Stumbling points (time, where, what the participant tried, what got them going again):
  1.
  2.

Messages and problems shown (codes or texts) and whether the participant understood them:

Quotes worth keeping:

Ease (participant's answer to "Overall, how easy or hard was this task?",
1 = very hard, 7 = very easy): ____
```

Look out especially for:

- finding the right category and block (the _repeat until_ and _if … else_
  entries, the comparison block in Math, the variables in Variables);
- dropping a block into a slot (the variables into the comparison, the
  random number into the variable's value);
- naming a variable, editing numbers and texts, choosing `=` in the
  comparison and the variable in the _ask_ block;
- adding the _else if_ part with ⊕;
- understanding an error badge, the Problems list and why ► Run is held back;
- finding ► Run, typing into the console, and reading the result in its
  header.

## After the sessions

1. File an issue for each stumbling point, quoting the session and the time,
   with the labels `usability` and `M2`.
2. **M2's check passes** when at least three sessions, on both systems
   together, were a success without help. Sessions with help or without
   success do not count toward the three: fix what stopped those
   participants, or have the owner accept it, and run new sessions with
   people who have not taken part yet.
3. A stumbling point that two or more participants hit is fixed, or accepted
   by the owner with a reason, before M2 is declared done, even when the
   check passes.
4. Write the outcome into the [sign-off](#sign-off) table and update the
   status note in [10 §10.1](../spec/10-roadmap.md#101-milestones).

## Results

No session has been run yet. Add one filled-in session record per session
here.

## Exclusion review

[04 §4.13](../spec/04-user-interface.md#413-what-the-m2-editor-includes) and
[10 §10.1](../spec/10-roadmap.md#101-milestones) list what milestones M3 to
M6 add. M2 must have **no UI entry point** for any of it: no greyed-out menu
item, button or shortcut that leads nowhere. The review was first run on the
code of commit `3b91b38` (2026-10-06); the milestone demo confirms it in the
running app on both systems and ticks the last column.

**How the code was checked:**

- **Commands.** The app's command registry is a closed list
  (`CommandId` in `apps/desktop/src/app/commands.ts`): `project.new`,
  `project.open`, `project.save`, `project.saveAs`, `project.close`,
  `build.start`, `run.start`, `run.stop`, `run.again`, `settings.open`,
  `problems.focusFirstError`, `edit.copy`, `edit.cut` and `edit.paste`.
- **Menus, buttons and pages.** The **≡** menu has _New project…_, _Open…_,
  _Save_, _Save as…_ and _Close project_. The toolbar has the Debug /
  Release choice, ► Run, ■ Stop, Build and Settings. The pages are the start
  page, the editor, Settings (code style, Run on errors, console, toolchain,
  build cache, this project's trust) and the toolchain page. The C++ panel
  has a file switcher, _Copy all_ and _Copy selection_; the bottom panel
  Problems, Console and Build output; the status bar the save state, the
  configuration, the C++ standard, Restricted Mode and the compiler.
- **Keys.** The app's shortcuts are F5, Shift+F5, Ctrl+B and Ctrl+S (the
  reload keys are cancelled), and the block editor's keys are those of
  `apps/desktop/src/editor/keyboard/help.ts`: arrows, Enter and Space, M, T,
  Delete, Escape, Ctrl+C, Ctrl+X, Ctrl+V, Ctrl+D, Ctrl+Z, Ctrl+Y and
  Ctrl+Enter. Nothing handles Ctrl+Space, Ctrl+Shift+P, Ctrl+F, F2, F11, F12
  or Shift+F12.
- **Toolbox.** `catalog/toolbox.toml` has the nine categories Program,
  Variables, Math, Logic, Text, Control, Loops, Input / Output and Functions
  with the 32 blocks of the M1 catalog; there is no _Libraries_ category, no
  advanced-blocks switch and no search field.
- **Blockly's own menus.** The clipboard plugin replaces Blockly's copy,
  paste and _Duplicate_; the rest of Blockly 12.5.1's default context menus
  stay: on the canvas _Undo_, _Redo_, _Clean up Blocks_, _Collapse Blocks_,
  _Expand Blocks_ and _Delete … Blocks_; on a block _Duplicate_, _Add
  Comment_, _Inline Inputs_ / _External Inputs_, _Collapse Block_ /
  _Expand Block_, _Disable Block_ and _Delete Block_ (_Help_ stays hidden:
  no block has a help URL).
- A search of the frontend's source (`apps/desktop/src`,
  `packages/blockly-ext/src`) for the names of the features below found no
  label, button or handler for any of them.

| Milestone | Feature                                                                                                                                             | Entry point in M2 (code review)                                                                                                                      | Confirmed at the demo |
| --------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------- |
| M3        | Typing in expression slots, _Expand into blocks_ / _Collapse to text_, Quick Insert (`Ctrl+Space`)                                                  | None: slots hold blocks and single values; no slot editor, no such menu items, no `Ctrl+Space`                                                       | [ ]                   |
| M3        | The type picker popover                                                                                                                             | None: type fields are plain menus (`int`, `double`, `bool`, `char`, `string`, `auto`)                                                                | [ ]                   |
| M3        | Collection, string, struct, enum, file, error, random and time blocks; globals, constants, math functions, conversions and organisation blocks      | None: the toolbox holds only the 32 M1 blocks                                                                                                        | [ ]                   |
| M3        | Multi-module tabs and headers (adding, renaming, deleting modules)                                                                                  | None: a project with several modules gets a switcher without add, rename or delete                                                                   | [ ]                   |
| M3        | _Export_ to CMake / Make (`project_export_dialog`)                                                                                                  | None: no menu item, and no such IPC command                                                                                                          | [ ]                   |
| M3        | The friendly g++ message catalog; quick fixes; _Copy bug report_                                                                                    | None: g++ messages are shown as g++ wrote them; a suspected generator bug is labelled without a button                                               | [ ]                   |
| M3        | Textbook style; the Friendly / C++ label switch; _Show advanced blocks_; toolbox search                                                             | None                                                                                                                                                 | [ ]                   |
| M3        | The project settings dialog (standard, options, configurations, defines); run options (arguments, working directory, stdin file, external terminal) | None: Debug / Release is a session choice in the toolbar, as M2 specifies                                                                            | [ ]                   |
| M3        | Ending a program's input; feature gating in the UI                                                                                                  | None                                                                                                                                                 | [ ]                   |
| M4        | Classes, lambdas, memory, templates and concurrency blocks                                                                                          | None                                                                                                                                                 | [ ]                   |
| M4        | Library packs and the _Libraries_ category; library profiles; the generic member block                                                              | None                                                                                                                                                 | [ ]                   |
| M4        | Raw C++ blocks and the _Contains Raw C++ (n)_ indicator                                                                                             | None (the trust dialog lists the Raw C++ blocks a file holds, as 08 §8.3.1 specifies)                                                                | [ ]                   |
| M4        | Extra compiler flags and environment pass-through settings                                                                                          | None                                                                                                                                                 | [ ]                   |
| M5        | Outline, frames, search, rename (`F2`), go to definition (`F12`), find references (`Shift+F12`), back/forward and bookmarks                         | None: no panels, menu items or keys                                                                                                                  | [ ]                   |
| M5        | Snippets, minimap                                                                                                                                   | None                                                                                                                                                 | [ ]                   |
| M5        | Tidy-up and collapse to signatures                                                                                                                  | Not M5's features, but Blockly's own _Clean up Blocks_ and _Collapse Block(s)_ / _Expand Block(s)_ are in its context menus; they work and are saved | [ ]                   |
| M5        | The debugger, breakpoints, trace glow and runtime errors on blocks                                                                                  | None                                                                                                                                                 | [ ]                   |
| M5        | Localisation; the command palette (`Ctrl+Shift+P`) and remappable shortcuts                                                                         | None                                                                                                                                                 | [ ]                   |
| M5        | Focus and Presentation modes; swapping, splitting and remembering docks                                                                             | None: docks can be resized and hidden only                                                                                                           | [ ]                   |
| M5        | High Contrast themes and the checked colour-blind-safe palette                                                                                      | None: the light and dark themes follow the system                                                                                                    | [ ]                   |
| M5        | Lint levels (_Change level…_, lint lists); build and run settings (timeouts, cache size, link mode, caps); code style beyond the indent width       | None: Settings has the indent width, Run on errors, the console's scrollback and _Clear build cache_ only                                            | [ ]                   |
| M5        | The example gallery and the full template set; _Help_, the offline help and _Export diagnostics bundle_                                             | None: the start page offers _Empty project_ and _Hello World_; _Learn more_ in Problems opens the published diagnostics reference, as M2 specifies   | [ ]                   |
| M6        | The updater and the first-run update question; sandboxed runs; installers                                                                           | None                                                                                                                                                 | [ ]                   |

**Findings for the demo:**

1. **Blockly's _Clean up Blocks_ and _Collapse_ / _Expand_.** These Blockly
   built-ins are on the canvas and block menus. They are not dead entry
   points: clean-up lines the top-level blocks up in a column (their new
   positions are saved), and collapsed blocks are saved and shown collapsed
   again. They are simpler than M5's tidy-up (types, then functions, then
   `main`) and _Collapse all definitions_. The demo decides whether M2 keeps
   them or hides them until M5.
2. **_Inline Inputs_ / _External Inputs_** is offered on blocks with two or
   more value inputs (for example the comparison). It changes only how the
   block is drawn, and the change is not saved: the block is drawn inline
   again after a reload. Not an excluded feature; worth an issue (save it, or
   remove the item).

## Sign-off

| Check                                         | Build (commit) | Date | Reviewer | Result | Issues |
| --------------------------------------------- | -------------- | ---- | -------- | ------ | ------ |
| Usability sessions (at least 3, both systems) |                |      |          |        |        |
| Exclusion review at the milestone demo        |                |      |          |        |        |
