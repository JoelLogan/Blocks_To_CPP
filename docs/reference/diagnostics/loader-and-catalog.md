# Loading and catalog diagnostics

These problems are found before any C++ is generated, in the first two stages
of the [translation pipeline](../../spec/06-compiler-pipeline.md#61-overview):

* **`B2C-E01xx`: loading.** `b2c-model` reads the project file with the
  limits and rules of [spec §5.6](../../spec/05-project-format.md#56-validation-limits).
  A file with any of these problems does not open, so nothing in it can run
  or be built. The loader reports every problem it finds, not just the first
  one (at most 1,000, then `B2C-E0199`). Problems with the bytes, the JSON
  syntax and the format header stop loading right away, because everything
  after them would be guesswork.

  Pasted blocks ([spec §5.12](../../spec/05-project-format.md#512-clipboard-format))
  go through the same checks with the same codes, so a paste with any of
  these problems inserts nothing. Their messages say "the pasted data"
  instead of "the project file". One code exists only for pastes:
  `B2C-E0138`, data that is not Blocks2Cpp clipboard data.
* **`B2C-E06xx`: catalog.** `b2c-catalog` checks every block against its
  definition in the block catalog ([spec §6.3](../../spec/06-compiler-pipeline.md#63-stage--resolve-catalog)).
  The project still opens, with the problem shown on the block, but it cannot
  be built until it is fixed.

All of them are **errors**. Text from the project file that appears in a
message is quoted, cut after 40 characters, and invisible or control
characters in it are shown as `\u{XXXX}`, so a message can never hide or fake
text.

Messages name the place in the file as a path such as
`"modules[0].workspace.viewport.scale"`, or relative to the block the problem
is in, such as `"fields.NAME.name" in this block`.

The malicious-project regression suite in
[`tests/security/projects/`](../../../tests/security/projects/README.md) has a
crafted file for most of these codes, and
[`tests/security/clipboard/`](../../../tests/security/clipboard/README.md) has
crafted clipboard payloads.

## Loading the project file (`B2C-E01xx`)

### B2C-E0101: the project file is too large

Project files can be at most 32 MiB. The size is checked before the file is
parsed, and Blocks2Cpp stops reading a file one byte after the limit, so an
oversized file cannot use up memory (which is also why the message does not
say how large the file is). Real projects are far smaller (typically under
1 MiB).

> The project file is larger than 33554432 bytes (32 MiB), the most a project file can be.

**Fix:** check that you opened the right file. Split a very large program
into several projects.

### B2C-E0102: the file is not UTF-8 text

Project files are UTF-8 text without a byte order mark (BOM). Files saved as
UTF-16, in a legacy code page, or with a BOM are refused. The message gives
the position of the first byte that is not UTF-8.

> The project file is not valid UTF-8 text (line 1, column 1). Save it with the UTF-8 encoding.

> The project file starts with an invisible byte order mark (BOM). Save it as UTF-8 without a BOM.

**Fix:** save the file as "UTF-8" (not "UTF-8 with BOM" or "Unicode") in your
text editor.

### B2C-E0103: the file is not valid JSON

The file does not follow the JSON syntax. The message says what was expected
and where (line and column). Comments, trailing commas, `NaN`, numbers too
large for any number type, half of a UTF-16 surrogate pair (`\ud800`) and
anything after the end of the data are all refused.

> The project file is not valid JSON: expected a key in double quotes, but found 'o' (line 3, column 3).

**Fix:** correct the file at the position shown, or restore it from a backup
(`<name>.b2c.bak`) or version control.

### B2C-E0104: nesting is too deep

Lists and objects are nested more than 128 levels deep. Statement lists keep
the nesting of a file equal to the logical nesting of the program, which is
far smaller; very deep nesting is only used to crash parsers.

> Lists and objects in the project file are nested more than 128 levels deep (line 1, column 3127). Real projects need far fewer levels, so the file was not opened.

**Fix:** move deeply nested blocks into functions.

### B2C-E0105: a key appears twice in one object

The same key appears twice in one JSON object (also when spelled differently
with escapes, such as `"form\u0061t"` and `"format"`). Programs disagree
about which value counts, so a file could look different to the editor and to
a reviewer. Every repeated key is listed.

> The key "format" appears twice in the same object (line 4, column 3). Programs disagree about which value counts, so each key may appear only once.

**Fix:** remove one of the two entries.

### B2C-E0106: the file holds too many values

The file holds more than 4,194,304 JSON values (numbers, strings, lists and
objects). No real project comes close; the limit stops files designed to
exhaust memory.

> The project file holds more than 4194304 values (line 1, column 8388610), far more than any real project, so it was not opened.

**Fix:** check that you opened the right file.

### B2C-E0107: not a Blocks2Cpp project

The file is JSON, but its `"format"` is not `"blocks2cpp/project"`, or the
file is not a JSON object at all.

> This file is not a Blocks2Cpp project: its "format" is "blockly/workspace", not "blocks2cpp/project".

**Fix:** open a `.b2c` project file. Other formats need converting first.

### B2C-E0108: made with a newer version of Blocks2Cpp

The project uses a newer file format than this version of Blocks2Cpp
understands (any whole number above the current format version, however
large). It is refused as a whole instead of being half loaded, so nothing in
it is lost or misread.

> This project was made with a newer version of Blocks2Cpp (needs ≥ 0.3.0; it uses project format 2, and this version reads format 1). Update Blocks2Cpp to open it.

Blocks copied in a newer version of Blocks2Cpp can use a newer clipboard
format, and are refused the same way when pasted:

> These blocks were copied from a newer version of Blocks2Cpp (they use clipboard format 2, and this version reads format 1). Update Blocks2Cpp to paste them.

**Fix:** update Blocks2Cpp.

### B2C-E0109: the format version is invalid

`"formatVersion"` is missing, is not a whole number, or names a version that
never existed or that this version cannot upgrade from. (Older versions are
upgraded automatically; see [spec §5.7](../../spec/05-project-format.md#57-versioning-and-migration).)

> The project file has no valid "formatVersion" (a whole number such as 1), so it cannot be read.

> This project uses project format 0, which this version of Blocks2Cpp cannot read or upgrade.

**Fix:** restore the file from a backup or version control.

### B2C-E0110: unknown key

The file has a key that the project format does not define. Unknown keys are
refused instead of ignored, so a file cannot carry settings that one tool
would act on and another would silently drop. In particular, projects never
contain compiler flags, include or library paths, or commands
([ADR-0005](../../adr/0005-no-compiler-flags-in-projects.md)). Tools can keep
their own data in the `"x-ext"` object, which is preserved but never
interpreted.

> "project" has an unknown key "junk0". Remove it or check its spelling.

**Fix:** remove the key or correct its spelling. Machine-specific settings
belong in the app's settings, not in the project.

### B2C-E0111: required key missing

A key that the format requires is missing, such as a block's `"id"` or
`"type"`, or a module's `"workspace"`.

> "modules[0]" is missing "workspace".

**Fix:** add the key. If the file was edited by hand, compare it with a file
saved by Blocks2Cpp.

### B2C-E0112: wrong kind of value

A value has the wrong kind (text where a number belongs, for example), is
not one of the allowed choices, or is a number out of range for its key.
`"x-ext"`, when present, must be an object. A block's `"stack"` (the blocks
stacked below a loose block on the canvas) is never an empty list: a block
with nothing below it has no `"stack"` key, so every project has one
spelling.

> "project.run.workingDirectory" should be one of "project" or "sandbox", but it is the text "/etc".

> "stack" in this block is an empty list. Leave "stack" out when no blocks are stacked below the block.

**Fix:** use one of the values the message lists, or remove an empty
`"stack"`.

### B2C-E0113: invalid ID

Block, frame, note, symbol, module and project IDs are 1 to 32 characters
from `A`–`Z`, `a`–`z`, `0`–`9` and `_`. IDs appear in diagnostics and in the
editor, so they cannot carry markup or control characters.

> "statements.BODY[0].id" in this block is not a valid ID: "b1\" onmouseover=\"alert(1)" is not 1 to 32 characters from A–Z, a–z, 0–9 and _.

**Fix:** give the item a plain ID. Blocks2Cpp generates valid IDs itself.

### B2C-E0114: two blocks share an ID

Every block, frame and note in the project needs its own ID; problems,
selections and edits refer to blocks by ID. The message points at the second
use and names the first one as related.

> Another block, frame or note already uses the ID "b_same". Every block, frame and note needs its own ID.

**Fix:** give one of them a new ID. This usually happens when blocks are
copied by hand in a text editor; copying in Blocks2Cpp creates fresh IDs.

### B2C-E0115: a symbol is declared twice

A symbol ID (the identity of a variable, parameter or function) is declared
by more than one block or parameter row. References use the symbol ID, so it
must be unique.

> The symbol ID "s_same" is declared more than once. Every variable, parameter and function needs its own symbol ID.

**Fix:** give one of the declarations a new symbol ID and update the
references that should point at it.

### B2C-E0116: two modules share an ID

> Another module already uses the ID "mod_main". Every module needs its own ID.

**Fix:** give one of the modules a new ID.

### B2C-E0117: module name cannot be a file name

Each module becomes a file named after the module, so its name must be 1 to
64 characters: a lower-case letter first, then lower-case letters, digits,
`_` or `-`. This rules out paths (`../`, `C:\`), dots, spaces and characters
that some file systems treat specially.

> The module name "../../etc/passwd" cannot be used as a file name. Use 1 to 64 characters: a lower-case letter first, then lower-case letters, digits, _ or -.

**Fix:** rename the module.

### B2C-E0118: module named like a Windows device

`con`, `prn`, `aux`, `nul`, `com0`–`com9` and `lpt0`–`lpt9` (also with the
superscript digits `¹`, `²` and `³`, and in any letter case) are device names
on Windows: a file with such a name, in any folder and with any extension,
refers to a device instead of a file.

> The module name "NUL" is reserved by Windows for a device, so it cannot be used as a file name. Choose another name.

**Fix:** rename the module, for example `console` instead of `con`.

### B2C-E0119: two modules with the same name

Two modules have names that differ only in letter case (or not at all).
Their files would overwrite each other on Windows and macOS, where file names
ignore case.

> Two modules are named "Util" (ignoring upper and lower case). Their files would overwrite each other on Windows, so every module needs a different name.

**Fix:** rename one of the modules.

### B2C-E0120: no modules, or too many

A project has 1 to 256 modules. When there are too many, only the first 256
are checked.

> The project has no modules. It needs at least one (usually called "main").

> The project has 257 modules, but at most 256 are allowed. Only the first 256 were checked.

**Fix:** add a `main` module, or merge modules.

### B2C-E0121: too many blocks

A project has at most 100,000 blocks, counting blocks inside other blocks.
The editor is designed for projects of up to about 5,000 blocks.

> The project has more than 100000 blocks, which is the most a project can have. Split it into smaller projects.

**Fix:** split the program into several projects.

### B2C-E0122: too many tokens in an expression

An expression slot holds at most 512 tokens (numbers, names, operators and
brackets).

> This expression has 513 parts (tokens), but a slot can hold at most 512. Split it into smaller pieces.

**Fix:** store parts of the expression in variables first.

### B2C-E0123: text or name too long

Text in a project is at most 64 KiB (65,536 bytes) per string; names of
variables, parameters and functions are at most 64 characters.

> "fields.NAME.name" in this block is 65 characters long, but names can be at most 64.

**Fix:** shorten the text or name. Large amounts of data belong in a file the
program reads.

### B2C-E0124: NUL character in text

Text contains the NUL character (U+0000). C and C++ treat it as the end of a
string, so text with NUL would mean different things to different tools.

> "fields.NAME.name" in this block contains the NUL character (U+0000), which is not allowed in project text.

**Fix:** remove the character.

### B2C-E0125: control character in text

Text contains a control character other than tab and new line, such as a
carriage return, backspace or escape. Such characters can hide text or
change how a terminal shows it.

> "comment.text" in this block contains a carriage return (U+000D), which is not allowed in project text. Only tabs and new lines are allowed.

**Fix:** remove the character. To print special characters, use escape
sequences in a text block.

### B2C-E0126: bidirectional control character in text

Text contains a Unicode bidirectional control (U+061C, U+200E, U+200F,
U+202A–U+202E or U+2066–U+2069). These invisible characters can make text
display in a different order than it really has ("Trojan Source",
CVE-2021-42574), so code could look harmless and do something else
([spec §8.4](../../spec/08-security.md#84-code-injection-through-block-content)).

> "comment.text" in this block contains an invisible left-to-right isolate character (U+2066). Such characters can make text look different from what it really is, so they are not allowed.

**Fix:** remove the character. Right-to-left text itself (Arabic, Hebrew) is
fine; only the invisible control characters are refused.

### B2C-E0127: reserved key

The keys `__proto__`, `constructor` and `prototype` are refused everywhere
outside `"x-ext"`. JavaScript code that merges parsed objects can be tricked
by them ("prototype pollution").

> "extra" in this block uses the key "prototype", which is not allowed in project files because it could be used to tamper with the editor.

**Fix:** remove the key.

### B2C-E0128: canvas position on a nested block

Only blocks directly on a module's canvas have a position (`"x"` and `"y"`);
blocks inside other blocks are placed by their parent, and blocks in a
`"stack"` sit below the block that holds the stack.

> This block is inside another block, so it cannot have a canvas position ("x" and "y"). Remove them.

> This block is stacked below another block, so it cannot have a canvas position ("x" and "y"). Remove them.

**Fix:** remove `"x"` and `"y"` from the nested or stacked block.

### B2C-E0129: coordinate out of range

Canvas positions are whole numbers from −10,000,000 to 10,000,000; frame
widths and heights from 0 to 10,000,000.

> "x" in this block is 2147483648, but it must be between -10000000 and 10000000.

**Fix:** move the block, frame or note closer to the rest of the canvas.

### B2C-E0130: zoom out of range

The saved viewport zoom (`"scale"`) is between 0.1 and 4.0.

> "modules[0].workspace.viewport.scale" is 1e308, but the zoom must be between 0.1 and 4.0.

**Fix:** set the zoom within the range, or remove the `"viewport"`.

### B2C-E0131: too many parts in a block

A block has at most 64 ⊕ parts: counts in `"extra"` are at most 64, and lists
in `"extra"` (such as function parameters) have at most 64 entries.

> "extra.itemCount" in this block is 4294967295, but a block can have at most 64 parts.

**Fix:** split the block, for example into several `print` blocks.

### B2C-E0132: invalid preprocessor define name

The name of a preprocessor define (`project.build.defines`) must be a valid
identifier ([spec §8.4.1](../../spec/08-security.md#841-identifiers)): letters,
digits and underscores, starting with a letter, not a C++ keyword and not a
reserved or predefined name. It becomes a `-D` compiler argument, so anything
else could smuggle in other compiler options.

> The preprocessor define name "X -fplugin=evil.so" cannot be used: a name can only contain letters, digits and underscores (found ' ').

**Fix:** choose a plain name such as `GAME_VERSION`.

### B2C-E0133: invalid library name

Library names (`project.build.libraries`) are 1 to 64 characters from
letters, digits and `_ + . -`. They are matched against the library profiles
on your machine and are never passed to the compiler as text.

> The library name "-Wl,--wrap=main" is not valid: use 1 to 64 letters (A–Z, a–z), digits and the characters _ + . and -.

**Fix:** use the library's name, such as `sfml-graphics`.

### B2C-E0134: preprocessor define set twice

> The preprocessor define "OK" is set more than once. Keep only one of them.

**Fix:** remove one of the defines.

### B2C-E0135: too much free-form data

`"extra"` maps and `"x-ext"` together hold more than 500,000 values. Real
projects store one or two values per block there.

> This project stores more than 500000 values of free-form data in "extra" and "x-ext", which is far more than any real project needs.

**Fix:** remove the extra data; tools should keep large data in their own
files.

### B2C-E0136: invalid library pack reference

A library pack listed in `project.build.packs` has an ID that is not 1 to 64
characters (a lower-case letter first, then lower-case letters, digits, `_`
or `-`), or a version that is not a version requirement such as `^1.0`,
`>=1.2, <2` or `*`. Pack IDs name folders and block namespaces, so they
cannot contain paths.

> The library pack ID "../../evil" is not valid: use 1 to 64 characters, a lower-case letter first, then lower-case letters, digits, _ or -.

**Fix:** use the pack's ID and a version requirement.

### B2C-E0137: library pack listed twice

> The library pack "std" is listed more than once. Keep only one entry for it.

**Fix:** keep one entry, with the version requirement you want.

### B2C-E0138: not Blocks2Cpp clipboard data

Only for pastes. The pasted data is JSON, but its `"format"` is not
`"blocks2cpp/clipboard"`, it has no `"format"`, or it is not a JSON object at
all. Data copied from another program, and whole project files, are refused:
a paste only accepts blocks copied in Blocks2Cpp
([spec §5.12](../../spec/05-project-format.md#512-clipboard-format)).

> The pasted data is not Blocks2Cpp blocks: its "format" is "blockly/clipboard", not "blocks2cpp/clipboard".

> The pasted data is a whole Blocks2Cpp project, not copied blocks. Open it as a project instead.

**Fix:** copy the blocks in Blocks2Cpp and paste again. To use a project
file, open it.

### B2C-E0139: blocks stacked below a block that is not on the canvas

A block that sits directly on a module's canvas, with no place in a program
yet, can keep the statement blocks attached below it in its `"stack"` (and
so can a block directly in pasted data). Blocks inside other blocks, and
blocks that are themselves in a stack, cannot have a stack: their
statements belong in a statement list. (Whether the block holding the stack
is a statement block is checked later, against the catalog.)

> This block is inside another block, so it cannot have a "stack": only a block directly on the canvas can have blocks stacked below it. Move the stacked blocks into the statement list they belong to.

> This block is itself in a "stack", so it cannot have a "stack" of its own. Put all the stacked blocks in the stack of the first block.

**Fix:** move the stacked blocks into the statement list they belong to,
or into the stack of the first block of the canvas stack.

### B2C-E0199: more problems than are listed

At most 1,000 problems are listed for one file; this note counts the rest.

> 500 more problem(s) were found but are not listed. Fix the problems above and load the file again.

**Fix:** fix the listed problems first. So many problems usually mean the
file is not a project, or was damaged.

## Checking blocks against the catalog (`B2C-E06xx`)

### B2C-E0600: no block definitions

The block catalog is empty or could not be loaded, so no block can be
checked. This is a bug in Blocks2Cpp, not in your project.

> No block definitions are available (the block catalog is empty or could not be loaded), so no block can be checked. This is a bug in Blocks2Cpp.

**Fix:** reinstall Blocks2Cpp and [report the bug](../../../CONTRIBUTING.md).

### B2C-E0601: unknown block type

The block's type is not in the catalog. Block types are namespaced by pack
(`sfml.window.open` comes from the pack `sfml`), so the message names the
pack that is probably missing. The block is kept unchanged (greyed out in the
editor), and saving the project preserves it; the blocks inside it are still
checked.

> This block has the type "sfml.window.open", which this version of Blocks2Cpp does not know. Missing pack: "sfml". Install that library pack, or update Blocks2Cpp if the block comes from a newer version.

> This block has the type "Not A Type", which is not a valid block type. Block types look like "io.print".

**Fix:** install the library pack, update Blocks2Cpp, or delete the block.

### B2C-E0602: block from a newer version

The block was saved by a newer version of its definition than this version
of Blocks2Cpp knows.

> This block was made with a newer version of Blocks2Cpp: it is version 2 of "io.print", and this version knows only up to version 1. Update Blocks2Cpp to use it.

**Fix:** update Blocks2Cpp.

### B2C-E0603: block version cannot be upgraded

The block was saved with an older version of its definition. Blocks2Cpp
upgrades such blocks automatically
([spec §3.11.3](../../spec/03-block-language.md#3113-block-versioning)), but
here no upgrade exists (for example for version 0, which never existed), or
the upgrade failed because the block does not have the shape its version
promises. The block is kept unchanged.

> This block is version 0 of "control.break", which this version of Blocks2Cpp cannot upgrade to version 1.

> This block is version 1 of "io.print", and upgrading it from version 1 failed: the separator is unknown.

**Fix:** replace the block with a new one from the toolbox.

### B2C-E0604: block in the wrong place

The block's shape does not fit where it is: `when program starts` and
function definitions sit directly on the canvas; statement blocks go in
statement lists (inside `when program starts`, a function or a loop); value
blocks go into inputs.

> This "io.print" block is not inside "when program starts" or a function, so it would never run. Move it inside one, or delete it.

> A "program.main" block must sit directly on the canvas, not inside another block.

> This "io.print" block is a step, not a value, so it cannot be plugged into an input.

**Fix:** move the block where the message says.

### B2C-E0605: unknown field

The block has a field its definition does not have.

> This block has no field "COLOUR". Remove it or check its spelling.

**Fix:** remove the field or correct its name.

### B2C-E0606: required field missing

A field without a default value is missing, such as the name a `create
variable` block declares or the variable a `set` block changes.

> This block needs to know which variable or function it uses: its field VAR is missing.

**Fix:** choose a value for the field in the editor.

### B2C-E0607: field value does not fit

A field's value is not allowed for its kind: a dropdown value that is not one
of the options, a type that is not in the type list, a checkbox that is not
true or false, an empty number, or a name declaration or reference of the
wrong shape.

> The field SEP is "none\"); system(\"id", but it must be one of "none", "space" or "comma".

**Fix:** pick one of the listed values.

### B2C-E0608: unknown value input

The block has a value input its definition does not have, or a numbered
input beyond the block's ⊕ count (`ITEM5` on a `print` block with one item).
Inputs beyond the count would be invisible in the editor.

> This block has no input "ITEM5": it has 1 ITEM input(s), set by "extra.itemCount".

**Fix:** remove the input, or raise the count in `"extra"`.

### B2C-E0609: required value input missing

A value input that has no default and is not optional is empty.

> This block needs a value in its input VALUE.

**Fix:** put a value or an expression into the input.

### B2C-E0610: unknown statement input

The block has a statement list its definition does not have, a numbered list
beyond its ⊕ count, or a list that its settings switch off (such as `ELSE`
on an `if` block without an else part).

> This block has the part "ELSE", but "extra.hasElse" is false, so it has no such part. Set "extra.hasElse" to true or remove the part.

**Fix:** remove the statement list, or change the block's settings.

### B2C-E0611: unknown setting in `extra`

The block's `"extra"` has a key its definition does not have.

> This block has no setting "colour" in "extra". Remove it or check its spelling.

**Fix:** remove the key.

### B2C-E0612: required setting missing in `extra`

A setting without a default is missing, such as a function's parameter list
(`"params"`, which is `[]` for a function without parameters).

> This block is missing its setting "params" in "extra".

**Fix:** add the setting.

### B2C-E0613: setting in `extra` out of range

A count is not a whole number within the block's range, a flag is not true or
false, a parameter list is not a list, or a function has more parameters than
allowed.

> "extra.itemCount" of this block is 0, but it must be a whole number from 1 to 32.

> This block has 17 parameters, but at most 16 are allowed.

**Fix:** use a value within the range given.

### B2C-E0614: malformed parameter

A row of a function's parameter list is not an object with exactly `"sym"`
(a symbol ID), `"name"`, `"type"` (one of the allowed parameter types) and
`"mode"` (`"copy"`, `"editable"` or `"read_only"`).

> Parameter 2 of this block has the mode "rvalue", but the mode must be "copy", "editable" or "read_only".

**Fix:** correct the parameter in the function's settings.

### B2C-E0620: broken block definition

The catalog's own definition of a block type is inconsistent, so blocks of
that type cannot be checked. It is reported once per block type. This is a
bug in Blocks2Cpp or in a library pack, not in your project.

> The block catalog's definition of "io.print" is broken (ITEM repeats by "missing", which is not a count in extra), so blocks of this type cannot be checked. This is a bug in the catalog.

**Fix:** update Blocks2Cpp or the library pack, and report the bug.

### B2C-E0699: more block problems than are listed

At most 10,000 block problems are listed for one project; this note counts
the rest.

> 120 more problem(s) with blocks were found but are not listed. Fix the problems above first.

**Fix:** fix the listed problems first.
