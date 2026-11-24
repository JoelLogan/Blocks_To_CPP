# M3 contracts: W4, the editor tools

The second part of wave W4 ([next steps §3.4](../next-steps.md#34-waves)
and §3.2, *Editor*), after its first part
([w4-appearance-expert.md](w4-appearance-expert.md): Appearance, the
expert-mode settings and the gating service) and after W2's 86 blocks. It
wires the gating into W2's blocks (that contract's §6, *Needs from W2*),
and adds the type picker, Quick Insert and toolbox search, quick fixes,
*Copy bug report* and the friendly g++ messages. It is built on
`m3/w4-editor-tools`, from `8a3d8ec` on `ccr-01f82d9b-v70u20`, while the
coordinator builds W3's second part (multi-module) and another session W5.

## 1. Scope

### 1.1 In this slice

1. **Gating in W2's blocks** (w4-appearance-expert.md §6):
   * the C++ label mode in the blocks (§6.1): a block whose C++ template
     names its parts is laid out from it, every operator and function menu
     shows its C++ text, type fields show C++ type names; a mode change
     rebuilds the shown blocks without an undo step and without a change to
     the project;
   * the toolbox hides the entries and categories the gate refuses (§6.2),
     and rebuilds once when the mode, the standard or the toolchain changes;
   * the field menus filter their options through the gate (§6.3; already
     wired by W2's package C2 for every `b2c_dropdown`, kept);
   * every W2 block and option checked against the gating table (§2.1
     below), with a test that every catalog block, dropdown option and type
     has a decision, so a new block cannot land unclassified; the AWAITED
     rows closed or handed on.
2. **The type picker** (03 §3.5.1, M3's part): a type field opens a
   keyboard-first popover instead of a flat menu. It offers the field's own
   types, the module's structs and enums (the symbol service's list), and
   the collections the field's class allows, built step by step (*list of*
   ▸ *int*), with a search box that also reads typed C++ (`vector<int>`,
   `map<string, vector<int>>`) and friendly text (`list of int`). It
   replaces the interim menus of W2 (w2.md §6 item 5), so maps, sets, lists
   of lists and the other collections can be chosen.
3. **Quick Insert and toolbox search** (03 §3.14, 04 §4.2): one search
   index over the catalog's blocks (Friendly and C++ labels, the help text,
   synonyms and C++ keywords) and the module's variables and functions.
   `Ctrl+Space` on the canvas opens Quick Insert at the keyboard cursor or
   the selection; Enter inserts the block there as one undo step. A search
   box above the toolbox filters the flyout live with the same index. A
   few one-line statements are read directly (§2.4).
4. **Quick fixes** (04 §4.4) for the diagnostics with an obvious fix
   (§2.5), offered in the Problems panel and in the block's context menu,
   each applied as one undoable edit.
5. ***Copy bug report*** (07 §7.5.3 step 4) on a generator-bug diagnostic:
   a report the user reads before it is copied, with no project text unless
   they tick it (08 §8.11). And the **friendly g++ messages** (07 §7.5.3
   step 3): the common g++ and linker messages that reach a student read as
   plain text; the raw text stays one click away (*Show C++ compiler
   message*).
6. **What the editor side of the coordinator's three W4 items needs** (the
   ⚙ popover, *format … with*, the unchecked `[]` index): §7.

### 1.2 Not in this slice

* The ⚙ popover (`constexpr`, `static`), *format … with* and the unchecked
  `[]` index themselves: they touch `catalog/` and the code generator, so
  the coordinator builds them after W3 (§7 lists what the editor needs).
* Typing in expression slots and Quick Insert's full statement parsing with
  the Rust parser (03 §3.14 item 2): the slots are read-only until the
  expression-slot work, and a parser call needs `b2c-core-wasm` (W5's). §2.4
  reads a fixed set of one-liners in TypeScript; the rest waits (decision
  W4E-4).
* *Insert as Raw C++*: Raw C++ is M4's (03 §3.7, the advanced categories).
* The catalog's `advanced` and `requires` keys (w4-appearance-expert.md
  §6.4): `catalog/` is the coordinator's. §6 proposes them; until then the
  table in `editor/gating/table.ts` holds every row.
* Squiggles under slot tokens, *Change level…* (M5), per-code *Learn more*
  pages (M5).
* Cross-module types in the picker: W3's second part adds them to the
  symbol service; the picker lists whatever `userTypes()` gives (§6.2).
  *Done by W3's package T:* `userTypes()` gives the shown module's structs
  and enums, then each other module's shared ones with the module's name
  (`PickerUserType.moduleName`, the provider's optional `otherModuleName`),
  which the picker lists under a heading per module (*Module: utils*)
  after *My types*; a module's own type hides another module's shared one of its
  name ([w3-modules.md §7.1](w3-modules.md#71-corrections-made-with-p0-and-the-packages)).

## 2. Design

### 2.1 Gating in W2's blocks

**The audit.** Every catalog block, dropdown option and type is classified,
in `editor/gating/table.ts`: a row (a requirement) or *basic* (offered
always). A test walks `BLOCK_DEFS` and fails for a block, an option or a
type field's type that is neither, so a block added later needs a decision.
The decisions (W4E-1 for the owner):

| Key | Requirement | Why |
| --- | --- | --- |
| `option:var.change.OP=and,or,xor,shl,shr` | advanced | w2.md §1.9 (kept) |
| `block:program.use_header` | advanced | blocks add their headers themselves (03 §3.7.1); a student rarely needs it |
| `block:program.check` | advanced | `static_assert`, an organisation block (03 §3.7.1) |
| `block:control.fallthrough` | advanced | falling through is rare and easy to misuse; a case without it is the beginner's form |
| `option:text.find.OP=starts,ends` | C++20 with fallback | `starts_with`/`ends_with`; codegen writes `b2c::starts_with` before C++20: always offered |
| `block:coll.contains` | C++20 with fallback | `contains`; `count` before C++20: always offered |
| `block:text.format` | C++20 and `stdFormat` | the catalog's `requires`, since the coordinator built the block (§7); no longer AWAITED |
| `category:memory`, `concurrency`, `generics`, `raw` | advanced | AWAITED, M4 |

Every other W2 block and option is basic; the audit list in `table.ts`
names each, so the decision is visible in one place. A requirement with a
fallback changes nothing the editor offers; it documents the form and is
what the catalog's `requires` will say (§6.1).

**C++ labels** (w4-appearance-expert.md §6.1). `blockly-ext` gains
`blocks/labels.ts` (moved from the app's `gating/labels.ts`, which
re-exports it): `labelParts`, `blockLabelParts`, `hasFullCppLabel`,
`CPP_OPTION_LABELS`, `optionLabel`; and `blocks/labelMode.ts`:
`setBlockLabelMode(mode)` and `blockLabelMode()` (default `friendly`). A
block's `init` reads the mode: `blockLayout(def, parts)` lays it out from
`blockLabelParts(def, mode)`, and each `b2c_dropdown` gets its options'
C++ texts. A type field shows C++ names in the C++ mode (`typeLabel(…,
'cpp')`: `std::vector<int>`, `std::string`). The layout keeps every input's
and field's name, so a block's state is the same in both modes (a test
over every catalog block).

*Rebuilding on a mode change.* The editor's session gains `relabel()`: it
reads the shown module from the canvas, and loads it again with Blockly's
events off, keeping block IDs, the undo stack, the view and the selection;
it marks nothing changed and runs no preview. The toolbox items carry the
label mode (`labels: 'cpp'` on `B2cBlockInfo`, not part of the block's
state), so the flyout's kept blocks (the inflater's key) are built again.
Mode changes come from the store (`editorMode`), with the key `modeKey`.

*Alternatives considered.* (a) Re-laying each block out in place (moving
fields between inputs): Blockly has no API for it, and every mutator would
need to re-run; a reload with events off is what the module switch does
already. (b) Keep the Friendly layout and show only the C++ option texts:
less useful, and §6.1 asks for the layouts.

**The toolbox** (§6.2). `contents.ts`'s builders take the mode:
`entryItems(category, mode)` skips an entry whose `blockGate` refuses it,
`isShownCategory(category, mode)` adds `categoryGate`, `toolboxDefinition`
and `initialToolboxDefinition` take the mode. `dynamicContentsKey` gains
`modeKey(mode)`, so the continuous toolbox (W5's file, unchanged) rebuilds
once when it changes; the plugin's store subscription calls
`refreshIfChanged` when the settings, the project's standard or options, or
the toolchains change, and `updateToolbox` when the set of shown categories
changes.

### 2.2 The type picker

`packages/blockly-ext/src/fields/type-picker/`:

* `model.ts` (pure): the picker's entries for a field: its catalog
  `types` (*Common*), the module's structs and enums (*My types*), and the
  collection constructors its class allows (*Collections*: list, deque,
  set, map, queue, stack, priority queue), each built with `typeFromTerm`
  and kept only when `inClass(type, class, facts)` is not `false`. A
  constructor opens a step for each argument, whose entries are the value
  types an argument may be (a map's key and a set's or a priority queue's
  element must be *comparable*); the step's breadcrumb reads *map from ▸
  string to ▸ …*. Depth is limited by `MAX_TYPE_DEPTH`.
* `parse.ts` (pure): reads typed text as a type: C++ (`int`, `string`,
  `std::string`, `vector<int>`, `std::map<std::string, int>`, nested) and
  friendly (`list of int`, `map from string to list of int`, `set of
  Student`), with the module's struct and enum names. It never evaluates
  anything; unknown names give no result.
* `picker.ts`: the popover, in Blockly's `DropDownDiv` like any field
  editor. A search box (`role="combobox"`, `aria-autocomplete="list"`,
  `aria-controls`, `aria-activedescendant`), a listbox of entries with group
  headings (`role="group"` with `aria-labelledby`), and a polite live
  region that says how many types match. Keys: typing filters, Up and Down
  move, Enter chooses (a constructor opens its next step), Right opens a
  constructor's step, Left or Backspace in an empty box goes back a step,
  Escape closes without a change, Tab leaves the popover as Escape does.
  Every text is a DOM text node (`textContent`), never HTML.
* `type.ts` changes in one place: `showEditor_` opens the picker instead of
  the menu; a choice is `setValue(termKey(term))` (one Blockly change event,
  so one undo step, and start values reshape as now). The fixed and user
  options (`getOptions`) stay for keyboard navigation, tests and loading.

The picker offers types, never checks a project: a type from a file that it
would not offer still loads and shows (W4A-6).

*Alternatives considered.* (a) A cascading `FieldDropdown` submenu per
constructor: Blockly's menus have no nested submenus with typing, and a
search box is the spec's (03 §3.5.1). (b) A React popover: the field lives
in `blockly-ext`, which has no React, and the DropDownDiv already handles
placement, focus return and closing on scroll.

### 2.3 The search index

`apps/desktop/src/editor/search/`:

* `index.ts`: `searchEntries(mode)` gives one entry per offered toolbox
  block (gated, §2.1) with its texts: the Friendly label with placeholders
  removed, the C++ label, the help text's first sentence, the category name,
  and synonyms (`synonyms.ts`: `cout` → *print*, `cin` → *ask*, `vector` →
  the list blocks, `endl` → *print* with *new line*, `if`, `else`, `for`,
  `while`, `return`, `push_back`, `size`, `sqrt` …). Entries for the module's
  variables (a getter) and functions (a call) come from the symbol source.
* `fuzzy.ts`: a small scorer (no dependency): case-insensitive; a whole word
  scores most, then a word prefix, then a subsequence with gaps penalised;
  ties by label length and catalog order. It is linear in the number of
  entries times the query length, and the index is rebuilt only when the
  mode or the dynamic contents key changes.

### 2.4 Quick Insert and toolbox search

**Quick Insert** (`apps/desktop/src/editor/quick-insert/`): a React popup
mounted by `EditorWorkspace`, opened by `Ctrl+Space` on the canvas (a
shortcut in the keyboard plugin's registry) or the canvas's context menu
item *Insert a block…*. It is a combobox with a listbox of up to 12
results, at the keyboard cursor's block or the selection (else the top
left of the view). Enter inserts the chosen block:

* with a statement block selected, after it (or into the statement input
  the cursor is on); with a value input reached by the cursor, into it
  when the block's output fits (the checker decides); otherwise on the
  canvas at the view's top left, free of other blocks;
* as one undo step (one event group); the block is selected and the
  keyboard cursor goes to it; it is announced (*Inserted “print”*).

A few one-liners are read as statements (`parseLine`, pure): `int x = 5`
(and the other catalog types) → *create*, `x = 5` → *set*, `x += 2`, `x
-= 2`, `x++`, `x--` → *change*, `print "hi"` / `cout << "hi"` → *print*,
`repeat 10`, `while true`, `return x`, `break`, `continue`. A value is a
number, a quoted text, `true`/`false` or a variable's name in scope, given
as the slot's start value; anything else falls back to the search results,
with the note *Quick Insert reads simple lines only for now*. A line names
variables by the symbols in scope at the insertion point; a new name in
`int x = 5` becomes the declaration's name (checked like a typed name).

**Toolbox search**: `Ctrl+F` on the canvas, or *Search blocks…* in the
canvas's menu, opens a search field (`<input type="search">`, labelled
*Search blocks*) over the top of the toolbox. Typing filters the flyout
live: the toolbox installs one category, *Search results*, with the
matching blocks, best first, or the note *No block matches.*; Escape gives
the categories back. Down or Enter goes into the results, where the
toolbox's own keys add a block. It uses the same entries and scorer, and
the flyout shows the entries' own toolbox items (presets included), so a
result is dragged out as from its category.

*Why not a field always above the toolbox:* the toolbox's DOM is Blockly's
(its focus manager treats focus inside it as focus on the toolbox), and a
permanent bar above the canvas would move every canvas and toolbox scene.
The field shows only while searching, over the toolbox's single *Search
results* row.

*Alternative considered:* one shared popup for both. The spec gives the
toolbox its own live filter (04 §4.2), and dragging from the flyout must
keep working while it filters, so the filter shows the toolbox's own items.

### 2.5 Quick fixes

`apps/desktop/src/editor/quick-fixes/`: `fixesFor(diagnostic, context)`
gives zero or more `QuickFix { id, title, apply(workspace) }`. Which codes
get one (W4E-3):

| Code | Fix | How |
| --- | --- | --- |
| `B2C-E0201` (a variable reference) | *Create variable `score`* | a `create` with the reference's symbol ID and last-known name, type `int` (the analysis no longer knows its type: a different one is reported on the `create`, where it is changed), at the start of the body that holds the reference (its function or *when program starts*). Offered only when every reference to the lost symbol is in that one body: references in two bodies, or one outside any body (a global's start value), have no single place for it |
| `B2C-E0410` | *Add a `return` block at the end* | a `return` with the function's result type's start value, at the end of the function's body. Offered only for a result type with a start value (`int`, `double`, `bool`, `char`, `std::string`): for a list, a struct or an enum an empty `return` would only give `B2C-E0403` |
| `B2C-W0508` | *Add cases for `List`, `Quit`* | each missing enumerator goes into an empty case first, then the switch's mutator gains a case for each of the rest, up to its 64 parts (at most 32 per fix); the title names those it adds, and no fix is offered when there is no room |
| `B2C-E0405` | *Add “when program starts”* | the program block, on the canvas at the view's top left |
| `B2C-E0403` | *Give back a value* | the start value of the function's result type in the `return`'s input |

The others have no single obvious fix (a wrong type, a loop exit outside a
loop, a duplicate name: the right change depends on intent), or one that
deletes the user's blocks (an unreachable block), which a quick fix should
not do.

Each fix runs in one Blockly event group, so one Undo takes it back, and
selects the block it made. It is offered in the Problems panel's *Details*
cell (*Fix: …*, a button) and in the block's context menu. A fix is offered
only while its precondition holds (the reference still dangles, the body
still lacks a return), checked when offered and again when applied.

### 2.6 Friendly g++ messages

`apps/desktop/src/features/build-run/friendly.ts` (pure): rules keyed by the
diagnostic's option (`C:-Wconversion`, with `-Werror=` read as `-W`) and by
anchored patterns of the first line of g++'s message (read from `raw`). A
rule gives the plain-language text, with the quoted names (`‘score’` or
`'score'`) put in. The rules (W4E-5 extends them):

| Match | Friendly text |
| --- | --- |
| `fatal error: X: No such file or directory` | “The header `X` was not found. Check the name in the *use header* block, or install the library that has it.” (not a generator bug). Only g++'s own message at a place in a file (`main.cpp:1:10: X: …`, `X` without spaces): the linker's `cannot find -lX: No such file or directory` and the driver's message without a place keep the generator-bug label |
| linker `cannot open output file …: Permission denied` | “The program file could not be written. Is the program still running? Stop it, then build again.” (not a generator bug) |
| linker `undefined reference to 'f'` | 07's text: “`f` is declared but has no body …” |
| `no match for 'operator<<'` | 07's text: “Can't print a value of this type …” |
| `-Wconversion`, `-Wfloat-conversion` | “A value may lose information when it is converted here (…).” |
| `-Wsign-conversion`, `-Wsign-compare` | “A whole number that may be negative is mixed with a size or count here.” |
| `-Wshadow` | “A name here hides another with the same name.” |
| `-Wunused-variable`, `-Wunused-but-set-variable` | “`x` is created but never used.” |
| `-Wmaybe-uninitialized`, `-Wuninitialized` | “`x` may be used before it has a value.” |
| `-Wdiv-by-zero` | “This divides by zero.” |
| `-Woverflow` | “A number here is too large for its type.” |
| `-Wdouble-promotion` | “A decimal number is widened to `double` here.” |
| `-Wimplicit-fallthrough` | “One case runs on into the next.” |
| `-Wnull-dereference` | “This may use a null pointer.” |

The friendly text replaces the message's g++ part: a warning reads “g++
warns: ” and the friendly text; a generator-bug error keeps its label and
“please report it”, and gains the friendly text after it. `raw` is
unchanged, so *Show C++ compiler message* shows g++'s own words. A message
no rule matches is left as it is. Tests use recorded fixtures (GCC 13 here;
the patterns are the same in 11 and 15, checked against their message
catalogues).

### 2.7 *Copy bug report*

`apps/desktop/src/features/bug-report/`: a pure `bugReport(input,
{ includeProject })` and a dialog. On a generator-bug diagnostic the Problems
panel offers *Copy bug report…* (in the row's message cell, inside the grid's
one Tab stop: F2 on the cell moves into it, Escape back; not on a stale row,
whose C++ is no longer the C++ that was built), which opens the dialog: the report as it
will be copied (read-only, in a `<pre>`), a checkbox *Include my blocks and
the generated C++* (unticked), *Copy* and *Cancel*. *Copy* puts the text on
the clipboard (a `copy` event in the page; nothing is sent anywhere) and
says so.

What it collects, always: the app's version, the catalog version, the
platform, the toolchain's kind, version and probed capabilities (no path),
the project's C++ standard and build configuration name, the diagnostic's
code and source, and g++'s message with every quoted name and every path
replaced (`‘…’`, `<path>`), and without its source-excerpt lines.

With the box ticked, also: the message and raw text as they are (paths
still replaced by `<project>`, `<build>` and `~`), the block subtree of the
diagnostic's block (its top block, serialised, IDs kept), and the generated
C++ of its module. Nothing else from the project (no other modules, no
name, no file path).

## 3. Files

### 3.1 New (owned)

* `packages/blockly-ext/src/blocks/labels.ts`, `labelMode.ts` and tests.
* `packages/blockly-ext/src/fields/type-picker/` (`model.ts`, `parse.ts`,
  `picker.ts`, `index.ts`, tests).
* `apps/desktop/src/editor/search/`, `quick-insert/` (Quick Insert and
  the toolbox search field), `quick-fixes/`.
* `apps/desktop/src/features/build-run/friendly.ts`,
  `apps/desktop/src/features/bug-report/`.
* E2E: `e2e/specs/flows/editor-tools.e2e.ts` (the four flows of §8 in one
  spec).
* This contract.

### 3.2 Changed

* `blockly-ext`: `blocks/layout.ts` (parts as a parameter),
  `blocks/register.ts` (the mode, the C++ option texts), `blocks/css.ts`
  (the picker's rules), `fields/values.ts` (C++ type names),
  `fields/type.ts` (the picker: **tell the coordinator**, §6.2),
  `mutators/index.ts` (exports `asUndoStep`), `index.ts` (exports).
* `apps/desktop/src/editor/`: `gating/` (labels re-exported, the audit
  table, `installLabelMode`), `toolbox/contents.ts`, `plugin.ts` (the mode,
  the search results, the contents context for search and fixes),
  `landing.ts` (`noteCreatedFromItem`), `reshape.ts` (`giveStartValue`),
  `sync/session.ts` (`relabel`), `EditorWorkspace.tsx` (the mode, Quick
  Insert's and toolbox search's mount). The shortcuts and menu items are
  registered by the new plugins themselves, not in `keyboard/` or
  `menus/`.
* `apps/desktop/src/panels/problems/` (the *Details* actions),
  `apps/desktop/src/features/build-run/` (friendly messages).
* Outside the listed paths, additive: `apps/desktop/src/app/panels.tsx`
  (passes the quick-fix and bug-report callbacks to the panel),
  `apps/desktop/src/app/editorPlugins.ts` (the new plugins).

## 4. IPC and project format

None. Quick fixes and Quick Insert make ordinary block edits; the type
picker stores type terms the format already has (05 §5.4); the bug report
uses `app_info` and the toolchain list already in the store. No new
dependency.

## 5. Security, accessibility, platforms, performance

* **Security.** Every text the picker, Quick Insert, toolbox search and
  the dialogs show is set as text, never HTML. Typed text is never
  evaluated: `parse.ts` and `parseLine` build type terms and blocks only
  from the catalog's names and the symbols in scope, and a name typed for a
  declaration passes the same check as one typed in its field. The bug
  report holds no project text unless ticked, and no path ever (§2.7); it is
  copied, never sent. The friendly messages put quoted names in as text.
* **Accessibility.** The picker and Quick Insert follow the ARIA combobox
  pattern with a listbox (one Tab stop, `aria-activedescendant`, a polite
  count); toolbox search is a labelled search field; the quick-fix buttons
  are buttons with their fix as their name; the dialog is the app's
  accessible dialog. Everything works without a mouse; axe runs on each.
* **Windows.** Nothing system-specific: the clipboard goes through the
  page's `copy` event as the editor's clipboard does; paths in the bug
  report are replaced for both separators.
* **Performance at 1,000 blocks.** The relabel reload costs what a module
  switch costs (one `loadModule` of the shown module with events off, then a
  rebuild of the block index); neither is measured at 1,000 blocks yet, and
  no benchmark times a label change (a `webview.relabel-1000` benchmark beside
  the selection one is a follow-up). The search index is per mode and
  dynamic key, not per keystroke (the toolbox keeps its entries per contents
  key, which holds the mode, and a test checks one build for three queries);
  a quick fix touches one block; the toolbox's gate check is per entry, once
  per rebuild.

## 6. Needs from the coordinator

### 6.1 The catalog's gating keys

As w4-appearance-expert.md §6.4: `advanced = true` on a block or an option,
and `requires = { standard = "c++20", capability = "stdFormat", fallback =
true }`, in `catalog/core/*.toml`, `b2c-catalog`'s schema and
`catalog.json`, with `packages/catalog-gen` writing them into
`BLOCK_DEFS`. Then `gating/table.ts` reads them for blocks and options and
keeps only categories and types; the audit test reads the catalog. The rows
of §2.1 are the values to write. `E0703` (06 §6.10) reads the same keys.
*Done by the coordinator* (§7.1): `requires` is a list of words
(`requires = ["c++20", "fallback"]`), an option's keys sit on its field, by
value (`advanced = ["and", "or"]`, `requires = { starts = ["c++20",
"fallback"] }`), and `BLOCK_DEFS` carries them as a block's `requirement`
and a field's `requirements`.

### 6.2 Types in the picker (`fields/type.ts`, `symbols.ts`)

* `fields/type.ts` changes in `showEditor_` only (the picker) and in
  `getText_` (C++ names); `userTypeOptions()` and `getOptions()` are
  unchanged. W3 adds cross-module types to the same menus: put them in
  `SymbolProvider.userTypes()` (the symbol service), which the picker reads;
  each `SymbolInfo` with its `module`, so the picker can group *My types* by
  module. Proposal for `symbols.ts`: `userTypes()` gives the structs and
  enums visible in the module shown (its own and those shared with it), not
  only its own; no other change.
* The picker groups a type from another module under its module's name
  when `symbol.module` differs from the shown module (no change needed for
  that).

### 6.3 Spec sections

§9 lists the sections to change.

## 7. The editor side of the coordinator's W4 items

* **The ⚙ popover** (`constexpr`, `static` on *create* and *global*):
  needs a field kind for a popover of ticks (a `b2c_options` field drawn as
  ⚙, holding an object `{constexpr: bool, static: bool}` or two checkbox
  fields hidden behind it), its accessible popover (a group of checkboxes
  in the DropDownDiv, like the type picker), `CPP_OPTION_LABELS`-like C++
  texts for the C++ label mode (`constexpr`, `static`), and gating rows
  (`static` advanced?). The editor side can reuse the picker's popover
  shell (`type-picker/picker.ts` exposes `openPopover`). *Done by the
  coordinator* (§7.1): on *create* only, as two checkboxes behind one ⚙.
* ***format … with*** (`text.format`): its AWAITED row is ready
  (`block:text.format`: C++20 and `stdFormat`); the toolbox hides it on an
  older standard or a toolchain without `std::format`, with `gateText`'s
  reason in its tooltip when shown. A `%` argument list is a repeated input
  (the variadic mutator). The friendly-message table gains `std::format`'s
  errors. *Done by the coordinator* (§7.1): the row is the catalog's
  `requires = ["c++20", "stdFormat"]`, AWAITED keeps the M4 categories
  only, and the pattern is a text field.
* **The unchecked `[]` index**: a context-menu item on *item … of* (and
  *set item*), *Use unchecked `[]`*, toggling a field (`CHECKED`); the
  menu item comes through `editor/menus/registry.ts`, gated as advanced
  (`option:coll.item.CHECKED=false`), and shown as `[]` vs `.at()` in the
  C++ label mode. *Done by the coordinator* (§7.1): the catalog's `menu`
  and `mark` keys describe the field, the item reads *Fast unchecked
  access ([] instead of .at())*, and the block shows *unchecked* in both
  label modes.

### 7.1 Corrections made by the coordinator

Where the coordinator's pieces (next-steps.md §3.10 item 2) show this
contract wrong, it is corrected in place and the correction is recorded
here with its reason.

| Correction | Reason | Where |
| --- | --- | --- |
| `requires` is a list of words, not a table: `requires = ["c++20", "stdFormat"]`, with `"fallback"` for a form the generator writes before C++20 (`catalog.json` still writes an object, `{"standard": "c++20", "fallback": true}`) | the catalog reads a list of words with the reader it has for `headers`; a table, or a map of tables on a field, added about 20 KB to the WebAssembly core (2,016,903 bytes against 1,998,537) | §6.1 |
| *Fast unchecked access* is a checkbox `CHECKED` (default ticked, `omit_default`, `advanced = ["false"]`) with two new catalog keys: `menu`, the menu item's words, and `mark`, the words the block shows while the field is away from its default (*unchecked*). blockly-ext hides the checkbox and draws the mark after it (`blocks/marks.ts`); `menus/registry.ts` adds one item per such field, ticked (✓) while on, offered where `optionOffered` allows the value it sets and always back to the default | the friendly-label rule needs every field drawn with words beside it; a field set from the menu is the exception, and saying so in the catalog keeps the rule, the layout and the menu generic | §7 |
| *format … with*'s pattern is a text field (`FORMAT`, default `{} scored {}`), not a value input; its values are the repeated input `ITEM` (`itemCount`, 1 to 32, the items mutator). The analyser checks the pattern: a place per value (`B2C-E0306`), braces in pairs and each place `{}` or `{:[[fill]align][width][.precision][letter]}` fitting its value (`B2C-E0312`), and values that are numbers, characters, true/false values or text (`B2C-E0314`). The SAST is a `TextCall` with the new `TextOp::Format`, the pattern as its first argument | `std::format` checks its pattern when the program is built (C++20's `consteval` format string), so a pattern from a variable would not compile, and a wrong one gives g++'s longest messages; the analyser's subset is what a student needs | §7 |
| No `std::ostringstream` fallback (03 §3.7.4 had one): under C++17 the catalog stage reports `B2C-E0703` from the block's `requires`, the first block to report it; the toolchain capability is the editor's gate and, for a compiler without `<format>` (GCC 11, 12), a friendly message that names GCC 13 | the code generator does not know the toolchain, so a fallback by standard would still fail on GCC 12 in C++20; the analyser has the signatures, not the catalog's keys, so the resolve stage reads them | §6.1, §7 |
| The block's tooltip is its help text, which says it needs C++20 and GCC 13; `gateText`'s reason is not added to it | the toolbox shows the block only where the gate allows it, so the reason would show only on a block already on the canvas, which the build reports anyway | §7 |
| The ⚙ popover is on *create* only: two checkboxes, `CONSTEXPR` (basic) and `STATIC` (`advanced = ["true"]`), with the catalog key `popover` (the tick's words) and a `mark` each, both `omit_default`; blockly-ext hides them, shows the ticked marks and a ⚙ (`B2cGearField`, unnamed so never saved) that opens a group of checkboxes in Blockly's drop-down (`blocks/gear.ts`), each change one undo step. The SAST's `VarDecl` gains `is_constexpr` and `is_static` (`constexpr` sets `is_const` too, so *set* refuses it with `B2C-E0308`), and the C++ is `[static] [constexpr\|const] T x = v;` | a `b2c_options` field holding an object would be a new field kind and a new value shape in the project format; two checkboxes need neither. *global* keeps W2's choice (a constant global is `constexpr` when its value allows it, w2.md §2.7), and a global's linkage is the module work's (an unshared global of a project with several modules is in an anonymous namespace) | §7 |
| The *constant* checkbox stays on *create*, beside the ⚙ (03 §3.7.2 said the popover replaces it) | a beginner's `const` should not hide behind ⚙; the ticks only add to it | §7 |
| A `constexpr` variable must be a whole number, a decimal number, a character or a true/false value with a value fixed when the program is built (`B2C-E0424`, two new messages); with no value it is `constexpr int x{};`. A `constexpr` decimal number read in another `constexpr` value is reported, though C++ accepts it | L10 counts a `const double` read as not a constant expression (w2.md §2.7), and does not tell a `constexpr` one apart; C++ has no `constexpr` text, list or struct variable a program can keep | §7 |
| The C++ label mode shows *unchecked* after `numbers.at(i)`, not `numbers[i]` | a C++ template is one form; giving the mode a second template per field value is more than the mark needs, and the C++ panel shows the code | §7 |
| A map's *item of* stays `.at(k)` when unchecked; *set item* on a map was `[]` already | `m[k]` adds a missing key and does not compile on a read-only map | §7 |
| An option's `advanced` and `requires` are keys of its field, by value (`advanced = ["and", "or", "xor", "shl", "shr"]` on *change*'s `OP`; `requires = { starts = ["c++20", "fallback"] }` on *find*'s), and also apply to a checkbox's `"true"` and `"false"`; `BLOCK_DEFS` merges each into one requirement, a block's `requirement` and a field's `requirements` by value, the shape of `gate.ts`'s `Requirement`. A requirement names a standard newer than C++17 or a capability, and a field's default is never a value the editor could hide | an option is a `[label, value]` pair, which has no room for keys; per-value keys on the field are how its `headers` are written already | §6.1 |

## 8. Tests

* **blockly-ext (Vitest):** the label parts moved with their tests; a block
  built in the C++ mode has its inputs in the C++ order and serialises to
  the same state as in the Friendly mode (every catalog block); dropdowns
  show C++ option texts; the picker's model (entries per class, comparable
  keys, depth), `parse.ts` (C++ and friendly forms, nesting, unknown names,
  limits), and the popover's keys and ARIA in happy-dom.
* **Desktop (Vitest):** the audit (every block, option and type decided;
  AWAITED rows absent from the catalog); the toolbox without advanced
  entries, with them, and rebuilt once on a mode change; `relabel` keeps
  undo, selection, view and the document's hash; the search scorer and
  index; Quick Insert's `parseLine` and insertion places (one undo step);
  each quick fix (offered, applied, undone in one step, not offered when
  its precondition fails); the friendly rules against the fixtures; the bug
  report with and without the tick (no path, no quoted name, no source line
  without it); the Problems panel's actions and the dialog with axe.
* **E2E** (at 800 × 560, 1024 × 768 and the default size): the type
  picker (choose *map from string to list of int* with the keyboard; the
  C++ panel shows `std::map<std::string, std::vector<int>>`); Quick Insert
  (with the program block selected, `Ctrl+Space`, type `int total = 5`,
  Enter; `create total` lands in its body; one `Ctrl+Z` takes it away); a
  quick fix (delete a `create`, fix *Create variable*, undo); the C++ labels
  (switch *Block labels* to C++ in Settings, the program block reads
  `int main(`, back to *when program starts*; the project has no unsaved
  changes before or after).
* **Visual diff:** the C++ mode is off by default and the toolbox gains a
  search field: the toolbox scenes change (§10 lists them).

## 9. Spec sections to change later (`docs/spec/` is the coordinator's)

* 03 §3.5.1: the picker as built (the groups, the steps, typed C++ and
  friendly text); §3.5.2: the C++ mode's layouts and option texts, the
  rebuild without an undo step.
* 03 §3.7: the advanced blocks of §2.1.
* 03 §3.14: Quick Insert as built (`Ctrl+Space`, the context-menu item, the
  one-liners read; the rest with expression slots).
* 04 §4.2: toolbox search; the gated entries hidden.
* 03 §3.8 item 6 (Search: `Ctrl+F` searches the module's block text,
  identifiers and comments) and 04 §4.7 (the default shortcuts: `Ctrl+F`
  Find): `Ctrl+F` on the canvas opens toolbox search until the module's
  Find lands (M5), per W4E-9.
* 04 §4.4: the quick fixes; *Copy bug report*.
* 07 §7.5.3: the friendly rules, where they run (the frontend), and that
  the raw text is kept.
* 08 §8.11: the bug report's contents and the tick.
* 10 §10.1 M3: what landed.

## 10. Decisions for the owner

| ID | Question | Recommendation |
| --- | --- | --- |
| W4E-1 | Which W2 blocks are advanced | *use header*, *check at compile time*, *go on into the next case* and *change*'s bitwise forms; everything else basic (§2.1) |
| W4E-2 | How Quick Insert opens | `Ctrl+Space` and the canvas's context menu; not by typing a letter on the canvas, since `M` and `T` are the keyboard's move and toolbox keys (04 §4.7) |
| W4E-3 | Which diagnostics get a quick fix | The five of §2.5; none that deletes blocks |
| W4E-4 | Quick Insert's statement parsing | A fixed set of one-liners in TypeScript now; the Rust parser's (03 §3.14) with expression slots |
| W4E-5 | Where the friendly g++ messages are made | In the frontend, from `raw` and the option, so the backend's messages and the IPC stay as they are; moved into `b2c-build` if the CLI should show them too |
| W4E-6 | The bug report's default | Nothing from the project unless ticked; quoted names and paths always replaced |
| W4E-7 | The type picker's first view | *Common*, *My types*, *Collections*; a search box focused |
| W4E-8 | Toolbox search's field | Shown on demand (`Ctrl+F`, *Search blocks…*), over the toolbox, rather than always above it (§2.4: Blockly's toolbox DOM and focus, and every canvas scene would move) |
| W4E-9 | Toolbox search's key, which the spec gives to the module's Find (03 §3.8 item 6, 04 §4.7, M5) | `Ctrl+F` on the canvas opens toolbox search now, since Find does not exist yet; when Find lands in M5, either Find takes `Ctrl+F` back and toolbox search keeps *Search blocks…* (and another key), or one search field finds both blocks to add and blocks on the canvas. The other choice is another key now (keeping `Ctrl+F` free), at the cost of a less familiar key until M5 |

## 11. Coordination with W3

The rows of `w3-modules.md` §3.3 (on `ccr-01f82d9b-v70u20`, `42a5f16`) that touch this branch's
paths (asked for on 2026-10-28; the answer is the record). Each says what
this branch changed there, so W3's packages build on it.

| # | Path | Answer | Notes |
| --- | --- | --- | --- |
| 1 | `apps/desktop/src/editor/modules/` | **Agree** | This branch does not change the module strip. |
| 2 | `editor/sync/session.ts`, `EditorWorkspace.tsx` | **Agree** | This branch adds `EditorSession.relabel()` (the shown module built again with events off, keeping IDs, undo, view and selection) and, in `attachEditor`, `installLabelMode` before the session and the Quick Insert and toolbox search mounts in the canvas portal. The module operations sit beside them; a module switch or reload needs nothing of `relabel`, which reads the shown module only. |
| 3 | `blockly-ext` `blocks/layout.ts`, `register.ts`, `services.ts` | **Agree**, with one request | `blockLayout(def, parts)` now takes the label parts (Friendly or C++), and `blockDefinition` caches one layout per label mode. Showing `SHARED` must key that cache by the label mode **and** `multiModule()`. For "every definition block laid out again" please use `EditorSession.relabel()` (call it when the module count crosses 1 and 2, as `installLabelMode` calls it on a label change), not a second rebuild path; the toolbox follows through `modeKey`, so put `multiModule` in the toolbox's `dynamicContentsKey` too. |
| 4 | `editor/sync/workspaceToBdm.ts` | **Agree** | Not changed here. |
| 5 | `editor/toolbox/contents.ts` | **Agree** | `functionsContents` now reads the editor's mode (`contextMode`) and leaves out entries the gate refuses; keep that, and add the shared functions after the module's own. The search (`editor/search/`) reads `categoryContents`, so the shared calls are found by Quick Insert and toolbox search with no further change; their titles come from the symbols' names. |
| 6 | `fields/type.ts`, or the type picker | **Agree**: in the picker | The picker takes its list from `SymbolProvider.userTypes()` (the `symbols.ts` provider; `B2cTypeField.pickerContext()`), and `PickerUserType` already carries `module`. Add the headings in `fields/type-picker/model.ts` (a group per other module, e.g. `{ kind: 'type', group: 'mine', module }`, ordered after the shown module's types) and `picker.ts` (one heading per group); `type.ts` needs no change beyond what this branch made (`showEditor_`, `pickerContext`). |
| 7 | `features/project-settings/` | **Agree** | Not changed here. |
| 8 | `blocks/namespace.ts`, `fields/namespace.ts`, `checker/` | **Agree**, with one request | New blocks must get a gating decision: the test in `editor/gating/gating.test.ts` fails for a catalog block, dropdown option or type that is neither in `BASIC_BLOCKS` / `BASIC_OPTIONS` / `BASIC_TYPES` nor a row of `REQUIREMENTS` (`editor/gating/table.ts`). Please add *namespace* and *use namespace* there (recommendation: basic, or advanced if 03 §3.7.1 makes them so) and, optionally, their words to `editor/search/synonyms.ts` (`namespace`, `using`). |

## 12. Status

On `m3/w4-editor-tools`, from `8a3d8ec`, on 2026-10-28 to 2026-10-30.

**Done.**

* The contract (`f507c6e`; corrected as built in `40d131b`, with §11's
  answers to W3).
* Gating in W2's blocks (`abb3689`): C++ labels (layouts from the C++
  template where it names each part once and the block has no mutator
  parts; C++ option texts; C++ type names), `EditorSession.relabel()`, the
  toolbox hiding what the gate refuses and following the mode, and the
  audit of every catalog block, option and type (`BASIC_*` and the rows of
  §2.1). AWAITED keeps `block:text.format` (the coordinator's, §7) and the
  M4 categories.
* The type picker (`d06e793`), Quick Insert and toolbox search
  (`6192152`), quick fixes (`9bff391`), the friendly g++ messages and
  *Copy bug report* (`ad7361e`), the E2E flow and the fixes it and CI found
  (`167de44`, `ee2d2a2`).

**The coordinator's pieces** (next-steps.md §3.10 item 2), on
`ccr-01f82d9b-v70u20` from `adaae91`, 2026-11-24; corrections in §7.1.

* The catalog's gating keys (§6.1): `advanced` and `requires` on blocks
  and, by value, on dropdowns and checkboxes, checked by `b2c-catalog`,
  exported in `catalog.json` and `BLOCK_DEFS`; `gating/table.ts` reads its
  block and option rows from them and keeps by hand only the categories,
  the types and `block:text.format` (AWAITED). The generated C++ is
  unchanged. The WebAssembly core grew by 3,968 bytes (1,998,537).
* The unchecked `[]` index (§7): `CHECKED` on *item of* and *set item*,
  `[]` for a list's or deque's position in the generator (the SAST's
  `checked`), the block's mark and menu item; the every-block project uses
  it, so every g++ compiles it. The golden C++ of the examples is
  unchanged. The core is 2,002,695 bytes (+4,158). No E2E flow: the
  flows run against the built app under xvfb, which this session does not
  build; the menu item and the mark are covered in Vitest.
* The ⚙ popover (§7): `CONSTEXPR` and `STATIC` on *create*, checked by the
  analyser (`B2C-E0424`) and written as `static` and `constexpr`; the
  every-block project declares a `static constexpr int`, so every g++
  compiles it, and a codegen test runs a `static` counter. The golden C++
  of the examples is unchanged. The core is 2,005,923 bytes (+3,228). No
  E2E flow, as above.
* *format … with* (§7): `text.format` in Text after *join*, the analyser's
  checks of its pattern and values, `std::format` in the generator,
  `B2C-E0703` from the catalog stage under C++17, the friendly message for
  a compiler without `std::format`, and `block:text.format` out of
  AWAITED. A codegen test builds and runs it with every g++ here that has
  `std::format` (13 and 14; GCC 11 is skipped), and the every-block
  project leaves out the blocks that need more than C++17. The golden C++
  of the examples is unchanged. The core is 2,011,983 bytes (+6,060;
  +17,414 for the four pieces). No E2E flow, as above.

**Not done, and why.**

* Quick Insert reads a fixed set of one-liners (W4E-4); full statement
  parsing waits for expression slots and a parser call in
  `b2c-core-wasm` (W5's).
* The spec sections (§9): the coordinator's paths. The catalog's
  `advanced`/`requires` keys (§6.1) are done by the coordinator (above).
* The ⚙ popover, *format … with* and the `[]` index: the coordinator's
  (§7 lists what the editor needs); done by the coordinator (above).
* The friendly rules are checked against GCC 13's messages (recorded) and
  the wording GCC 11 and 15 use for the same options; no GCC 11 or 15 was
  installed here to record them.
* The E0201 fix creates an `int` (the analysis no longer knows the
  variable's type).

**Checks.** Locally: `pnpm -r` typecheck, lint and format; blockly-ext
Vitest (473); desktop Vitest with coverage and two workers (2,337 passed;
four toolbox and mover tests timed out at the 20 s flyout limit under that
load and pass on their own, 62 of 62; which four was not recorded. On the
merged head after the review fixes, the whole suite under coverage with two
workers timed nothing out: the slowest flyout tests were `keyboard/mover`
14.4 s, `toolbox/reuse` 12.4 s and `toolbox/plugin` 6.3 s, with Quick
Insert's file at 4.8 s and the quick fixes' at 2.2 s, recorded beside their
timeouts and in `toolbox/testing.ts`); `test:isolation`; the four
`tools/check-*.py`; markdownlint. No Rust changed, so the Rust checks are
CI's (green on `6192152`). The WASM core was built without wasm-opt. E2E
under xvfb: `editor-tools` at the default size, 1024 × 768 and 800 × 560
(4 of 4 each); `typed-slots`, `control`, `errors`, `structs-enums`,
`functions-globals`, `editor`, `settings`, `appearance` and `clipboard` at
1024 × 768 and 800 × 560 (17 of 17 each); `typed-slots` also at the
default size.

**CI.** On `ad7361e`: CI green; Desktop failed the frontend tests (the
panels loaded Blockly) and, on Linux, the `typed-slots` and `selection`
flows, all three fixed in `167de44` and `ee2d2a2`, and the visual diff
(below). On `ee2d2a2` (the last code change; later commits change this
contract only, which Desktop does not run for): CI, CodeQL and
OSV-Scanner green; Desktop green but for the visual-diff step on both
systems (the frontend tests with coverage, both app builds, and every E2E
flow: Linux 57 files passed, Windows 56 passed and 1 skipped, 203 tests).

**Visual diff, for the coordinator to rebaseline** (no baseline was
updated). *use header* and *check at compile time* are advanced now
(W4E-1), so the Program flyout loses them and the toolbox shown at start
changes: `canvas-guessing-game`, `chrome-menu-open`,
`chrome-toolbox-program` and `chrome-window-smallest`, light and dark, on
Linux (36,156, 5,221 and 5,060, 13,878 and 13,257, 13,880 and 13,248
pixels in Desktop run 41059385161), and the four light ones on Windows
(36,319, 7,257, 15,297 and 15,297 pixels). No other scene differs. The C++ mode is off by default
and toolbox search shows only on demand, so no other scene changes.

**Merge notes.**

* Files outside this contract's list: `apps/desktop/src/app/panels.tsx`
  (the Problems panel's fixes and bug report), `app/quickFixes.ts` (new),
  `app/editorPlugins.ts` and `app/features.test.ts` (the two plugins),
  `e2e/specs/flows/typed-slots.e2e.ts`, `e2e/specs/flows/lib/editor.ts`
  (`choosePickerType`) and the flows README.
* `packages/blockly-ext/src/fields/type.ts` changed (the picker; §6.2,
  §11 row 6); `blocks/layout.ts` and `register.ts` changed (§11 row 3).
* Expected conflicts: `toolbox/contents.ts` and `toolbox/plugin.ts` if W3
  or W5 changed them (keep `contextMode`, `withLabelMode`, the mode in
  `dynamicContentsKey`, and the plugin's definition comparison);
  `EditorWorkspace.tsx` (the label mode and the portal's children);
  `gating/table.ts` gains a row or a `BASIC_BLOCKS` entry for every new
  block.
