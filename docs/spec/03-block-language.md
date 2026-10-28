# 3. The Block Language

> Status: **Draft v0.1** · This chapter defines what users can build and how each block maps to C++.

## 3.1 Design philosophy: few blocks, much power

Scratch makes users build `a * b + 3` from three nested operator blocks and
three value blocks. That is fine for 10-line programs and painful for real
ones. Blocks2Cpp keeps the snap-together model for **structure** (programs,
functions, classes, control flow, statements) and offers denser ways to fill
in **details**:

| Technique | Example | Saves |
| ----------- | --------- | ------- |
| **Inline typed fields** | `create [int ▾] [score] = (0)` declares, types and initialises in one block | 2–3 blocks per declaration |
| **Type picker field** | `[map from string to list of int ▾]` is one field, not nested type blocks | 3–6 blocks per type |
| **Expression slots** | Type `price * qty + tax` directly into any value slot | 4–10 blocks per expression |
| **Variadic `⊕ / ⊖` slots** | `print (a) (b) (c) ⊕` and `join (x) (y) ⊕` | All the "join" chaining |
| **Operator dropdowns** | `(a) [< ▾] (b)` switches between `< <= > >= == !=` in place | Swapping blocks to change an operator |
| **Polymorphic phrase blocks** | `add (x) to [things ▾]` works for vector, set, queue, stack and deque; lowering picks `push_back`, `insert` or `push` | Learning one block per container |
| **Context-aware member block** | `[player ▾] . [health ▾]` / `[list ▾] . [push_back ▾] (x)`, with dropdowns filtered by the static type | Hundreds of per-method blocks |
| **Compound blocks** | `for each line [line] in file ("data.txt") { }` | Open, loop, getline and close |
| **Smart defaults (shadows)** | Every slot is pre-filled with a sensible editable value | Dragging literals around |
| **Quick Insert** | Press `Ctrl+Space` (or just start typing) and enter `int x = 5`, `for i = 0 to 10`, `v.push_back(3)` | Toolbox hunting entirely |
| **Expand / collapse expressions** | Turn a text expression into nested blocks (for learning) and back | Choosing between beginner and expert form |

Rule of thumb for catalog design: **a block represents one C++ idea a
programmer would say out loud.** "Read a number from the user" is one block.
"Open a stream, check it, read with `>>`, clear on failure, ignore the rest of
the line and retry" is what that block *generates*.

**One idea, one block, one toolbox entry.** Variants of one idea that share a
shape are choices on one block (a dropdown, a ⊕/⊖ part or a checkbox), never
separate toolbox entries. A separate block type is used only when the
variants differ in shape (a call used as a statement or as a value) or in
what the block holds (*forever* has no condition). A toolbox preset may fill
in a value, but never stands in for a choice the block offers in place. In
M3, *with arguments* is a checkbox on *when program starts* (a compatible
field), not a second block.

## 3.2 Notation used in this document

| Notation | Meaning |
| ---------- | --------- |
| `[text ▾]` | Dropdown field |
| `[text]` | Editable text field (identifier, label) |
| `(value)` | Value input (round slot): accepts a block **or** typed expression text |
| `<cond>` | Boolean input (hexagonal slot), likewise accepting a block or text |
| `{ … }` | Statement input (the mouth of a C-block) |
| `⊕ / ⊖` | Add / remove a repeated part (variadic mutator) |
| `☑ / ☐` | Checkbox field |
| `⚙` | Modifiers popover (e.g. `const`, `static`, `virtual`) |

## 3.3 Block shapes

| Shape | Look (Zelos) | Role | Examples |
| ------- | -------------- | ------ | ---------- |
| **Definition (hat)** | Rounded top, no previous connection | Top-level construct. Position on canvas does not matter. | `when program starts`, `define function`, `struct`, `class`, `enum`, `global constant` |
| **Container** | Hat with sections | Definition holding member rows/blocks | `class` (fields, methods, constructors) |
| **Statement** | Puzzle notch top and bottom | One C++ statement | `print`, `set`, `add to list` |
| **C-block** | Statement with one or more mouths | Compound statement | `if/else`, loops, `try/catch`, `with lock` |
| **Reporter** | Rounded | Expression producing a value | `(a + b)`, `length of [list]`, `[player].[health]` |
| **Predicate** | Hexagonal | Expression producing `bool` | `<a < b>`, `<list contains x>` |
| **Expression C-block** | Reporter with a mouth | Value containing statements | `lambda` |
| **Raw C++** | Hazard-striped border, `C++` badge | Escape hatch ([§3.10](#310-raw-c-blocks)) | Raw statement / expression / top-level |

Connection compatibility is enforced by a custom Blockly `ConnectionChecker`
backed by the type system ([§3.5](#35-types)). Shapes stop structurally
invalid programs, and types stop most semantic errors.

**Typed slots** ([ADR-0018](../adr/0018-slot-typing.md)). For a value
dragged or moved with the keyboard towards an input, the checker types the
parent block as if the value were already there, with the block's catalog
signature ([§3.11.1](#3111-catalog-format)) and the types the last analysis
knows, and refuses the drop only when the value certainly cannot fit: a
conversion or class error between known types, such as text into
`create [int]`, into `set` of an `int` variable, into an `int` parameter's
argument, or into `return` under *when program starts* or in a function
that returns a number. Unknown and erroneous types, a narrowing (`double`
into `int`), a mix of `bool` and numbers, and a block that takes its type
from a place that gives none all connect, and the analyser reports what is
wrong. The keyboard mover offers no place the checker would refuse, and
lists places with the moved block counted as absent from its current
input. Loading, undo, redo and paste are never checked. A parked block's
slots are typed from its own parts, so a wrong drop into a parked parent is
refused although parked blocks are not analysed. What only the analysis
knows is one preview old, as for every editor answer; a refusal in that
window ends with the next preview and never changes the document.

**Placing blocks.** A dragged block snaps to the nearest place that accepts it
when its connection is within about 44 screen pixels of that place, and never
less than 48 or more than 96 canvas units away, so a place is as easy to hit
at every zoom. A grey preview shows where it will go. While a statement is
dragged, the preview stays on its place until another place is 20 canvas units
nearer, so it does not jump between two places of a list that are almost as
near; a dragged value always goes to the nearest slot, since a block's first
slot lies only 8 units from the slot that holds the block. A loose block
dropped so that it only looks connected is still moved aside by Blockly's
usual 28 units. A drop with the pointer anywhere over the toolbox (its
categories and its open list of blocks alike) or over the trash can deletes
only the dragged blocks and never connects them. The toolbox counts at any
height: a drop with the pointer left of its right edge deletes, also above or
below the editor (over the toolbar or the bottom dock). The pointer decides,
not how much of the dragged blocks lies over the toolbox. While a drop would
delete, no place is previewed and the dragged blocks are shown faded. While
the pointer is left of the toolbox's right edge, the whole toolbox is tinted
as well; over the trash can, the toolbox is not tinted and the trash can opens
its lid instead.

An empty C-block (its first mouth holds nothing) dropped with its top on a
statement takes the statement's place and wraps that statement and everything
below it in the same list, as in Scratch: the first statement of a list or
one in the middle. Dropped with its mouth at the top of a loose stack, it
wraps the stack. An empty C-block is never inserted between two statements;
at the end of a list, with nothing below to wrap, it is added there. The
preview shows the C-block around the statements it will wrap. Hats and
definitions never take statements out of a list. Wrapping is a pointer
gesture: a block moved with the keyboard is inserted, and the keyboard
equivalent of wrapping comes in M5.

**Parked blocks.** As in Scratch, a statement or value block, or a stack of
them, can be left anywhere on the canvas. Only statements attached to *when
program starts* or to a function run; definitions are never parked. Parked
blocks are saved, show the ⚠ badge of `B2C-W0505` (*not attached*), are not
analysed until they are attached and never stop Run
([ADR-0012](../adr/0012-parked-blocks.md)). The block catalog still checks
them, so an unknown or damaged block is an error wherever it is. A block in
which nothing is chosen yet (an *ask* without its variable, straight from
the toolbox) is not damaged: it asks for the choice only once it is
attached. Until M3, a variable created inside a parked stack can be chosen
in the blocks below it only once the stack is attached.

## 3.4 Expression slots

Every value and boolean input accepts either a block or **typed text**. Typed
text is parsed by our own expression parser (Rust, also running as WASM in the
editor). It is resolved against in-scope symbols and type-checked as you type.
The slot shows the expression with syntax colouring. Errors underline the
offending token, and the message appears in the slot tooltip and the Problems
list.

**Everything typed in a slot is real C++ syntax**, so users learn the actual
language. A deliberately restricted subset is accepted:

```ebnf
expr        = conditional ;
conditional = logical_or [ "?" expr ":" conditional ] ;
logical_or  = logical_and { ( "||" | "or" ) logical_and } ;
logical_and = bit_or { ( "&&" | "and" ) bit_or } ;
bit_or      = bit_xor { "|" bit_xor } ;
bit_xor     = bit_and { "^" bit_and } ;
bit_and     = equality { "&" equality } ;
equality    = relational { ( "==" | "!=" ) relational } ;
relational  = shift { ( "<" | "<=" | ">" | ">=" ) shift } ;      (* "<=>" allowed in C++20 mode *)
shift       = additive { ( "<<" | ">>" ) additive } ;
additive    = multiplicative { ( "+" | "-" ) multiplicative } ;
multiplicative = unary { ( "*" | "/" | "%" ) unary } ;
unary       = ( "!" | "not" | "-" | "+" | "~" | "*" | "&" ) unary | postfix ;
postfix     = primary { call_args | "[" expr "]" | ( "." | "->" ) identifier } ;
call_args   = "(" [ expr { "," expr } ] ")" ;
primary     = literal | qualified_name [ template_args ] | "(" expr ")"
            | cast_keyword "<" type ">" "(" expr ")" | brace_list | "this" | "nullptr" | "true" | "false" ;
cast_keyword = "static_cast" | "const_cast" | "dynamic_cast" ;    (* reinterpret_cast: Raw C++ only *)
brace_list  = "{" [ expr { "," expr } [ "," ] ] "}" ;              (* only where the expected type is known *)
qualified_name = [ "::" ] identifier { "::" identifier } ;
```

**Deliberately not accepted** in slots, because each belongs in a statement
block or in Raw C++:

* Assignment, compound assignment, `++` / `--` (side effects belong to visible
  statement blocks)
* The comma operator, `new` / `delete`, lambdas (use the lambda block),
  `sizeof...`, `reinterpret_cast`, `goto` labels
* Preprocessor tokens, `;`, `{ }` statement braces, attributes, comments

**Name resolution is strict.** Every identifier must resolve to an in-scope
symbol (a variable, parameter, function, type, enum value or member) or to a
name declared by a loaded library pack. Unknown identifiers are errors, not
pass-through text. Together with the fact that output is **re-printed from the
parsed AST, never copied**, this means a slot cannot smuggle arbitrary code
into the program. See [08 §8.4](08-security.md#84-code-injection-through-block-content).

**Storage.** A slot's content is stored as a flat **token list** in which
symbol references are stored by *symbol ID*, not by name:

```json
{ "expr": [ {"ref": "sym_7Qk2"}, {"op": "*"}, {"num": "2"}, {"op": "+"}, {"ref": "sym_P01x"} ] }
```

Renaming a variable therefore updates every expression that uses it. Invalid
drafts are stored too (with a `"draft": true` flag), so in-progress text is
never lost. The token list is flat, so parser recursion depth is bounded by
expression depth (limit 64) and not by JSON nesting.

A token is an object with exactly one key. The kinds are `num`, `str`, `chr`,
`ref`, `op`, `kw` and `text`; a new kind needs a new format version
([05 §5.7](05-project-format.md#57-versioning-and-migration)). Values are
checked when the slot is parsed, not when the file loads: an `op` or `kw`
outside this section's grammar, or a `num` that is not an accepted literal,
is a slot error (`E0310`, …), and the file still loads. In M2 the accepted
forms are the operators `+ - * / % == != < <= > >= && || ! and or not ?:`,
parentheses, `true` and `false`, and numbers written as decimal digits with
an optional fraction and exponent (a whole number has no leading zero, except
a lone `0`), or as `0x`/`0b` integers, with `'` separators. Suffixes and the
other operators ([08 §8.4.4](08-security.md#844-numbers)) come with typing in
slots (M3). A `num` may start with one `-` or `+`, which lowering applies as
a unary operator; the literal itself (`NumLit`) never carries a sign. The
file keeps numbers as typed; the generated C++ prints them in canonical form
([08 §8.4.4](08-security.md#844-numbers)).

**Text with `+`.** `+` joins text with text or a character, in either
order, as C++'s `std::string` does: `name + "!"`, `'(' + name` and
`"a" + "b"` are text. When neither side is a `std::string` object, the
generated code makes the left one so (`std::string("a") + "b"`), since two
quoted texts cannot be added in C++. Text with a number is `B2C-E0303`: the
*join* block puts text and numbers together
([§3.7.4](#374-text)).

**Expand / collapse.** Right-click → *Expand into blocks* converts the parsed
expression into nested operator/value blocks. *Collapse to text* does the
reverse whenever the block subtree is expressible in slot syntax. This lets
teachers show structure and lets experts type quickly.

**In M2** there is no slot editor yet: typing into slots, *Expand* and
*Collapse* arrive with Quick Insert in M3. Until then a value input that holds
tokens (or nothing) shows an internal *shadow* block, which is never saved as
a `{"block": …}` input:

* a single literal token (`num`, `str`, `chr`) is an editable literal;
* a number typed with a leading `-` is stored as one `num` token;
* a single reference (`ref`) is a dropdown of the symbols in scope that fit
  the input;
* any other token list is shown read-only, as text, and is kept unchanged;
* an input that is absent from the file stays absent until the user edits it.

A value the user edits is saved as a single-token `expr`. The start value an
empty input gets (from the toolbox, on landing or when a field of its block
changes) follows the type the input expects
([04 §4.2](04-user-interface.md#42-toolbox)).

## 3.5 Types

### 3.5.1 The type picker

Types are **fields, not blocks**. Clicking a type field opens a popover with:

* **Search box.** Typing `map<string, vector<int>>` or `list of int` parses
  directly via the type parser.
* **Common.** `int`, `double`, `bool`, `char`, `string`, `auto`
* **Numbers.** `short`, `long`, `long long`, the `unsigned` variants, `float`,
  `long double`, `std::size_t`, `std::int8_t` … `std::uint64_t`
* **Collections.** list (`std::vector`), fixed array (`std::array<T, N>`), map,
  unordered map, set, unordered set, deque, queue, stack, priority queue,
  pair, tuple
* **Wrappers.** optional, variant, `unique_ptr`, `shared_ptr`, `weak_ptr`,
  `std::function<R(Args…)>`, `std::atomic`
* **My types.** User structs, classes, enums and aliases from all modules
* **Library types.** From loaded library packs (e.g. `std::thread`,
  `std::ifstream`, `sf::RenderWindow`)
* **Modifiers.** `const`, reference `&`, pointer `*` (with `const`
  placement), C array `[N]` (Advanced)

### 3.5.2 Friendly and C++ display modes

A global toggle (**View → Block labels: Friendly | C++**) switches how types
and block labels read. Both modes generate identical code.

| Friendly | C++ |
| ---------- | ----- |
| `list of int` | `std::vector<int>` |
| `map from string to double` | `std::map<std::string, double>` |
| `print (x)` | `std::cout << (x)` |
| `add (x) to [v]` | `v.push_back(x)` |
| `read-only reference to Player` | `const Player&` |

**In M2** only the Friendly labels exist; the C++ mode arrives in M3. Type
dropdowns show `int`, `double`, `bool`, `char`, `string` and `auto`, with
`std::string` displayed as `string`. The stored values are the catalog's type
names (`std::string`), so files do not change when the C++ mode arrives. A
constructed type or a struct or enum is stored as its type term
([05 §5.4](05-project-format.md#54-block-nodes)), and in the editor a type
field's value is the term's key: a name stays as it is (`int`,
`std::string`), and any other term is its compact JSON
(`{"std::vector":"int"}`, `{"ref":"sym_x"}`). A block whose type field holds
such a type, read from a file, loads as itself; the type picker that offers
them comes later in M3.

**In M3**, until wave W4's type picker, the type menus are interim (wave
W2). After a field's own types, *create*'s and a parameter row's menus
offer *list of int*, *list of double* and *list of string*
(`std::vector<int>`, `std::vector<double>`, `std::vector<std::string>`).
Every type field then lists the module's structs and enums that its class
allows (a field of class `value`, such as *create*'s, *global*'s, a
parameter's, a struct field's or *define*'s result: both; *new … with*'s:
structs only), sorted by name, then a list of each where the class allows
lists (*list of Student*). A user type is stored by its symbol and shown by
its current name, so a rename shows at once. A menu that lists only the
module's types shows *choose a struct first* while the module has none.
Maps and lists of lists come from the project file and paste until the type
picker. The block reference writes the list types in C++ syntax.

### 3.5.3 Type checking model (gradual)

The analyser implements a **gradual** type system that approximates C++:

* **Known types** (fundamental types, library-pack types with metadata, user
  types) are checked: implicit numeric conversions, derived-to-base, `const`
  correctness, simplified overload resolution, and the one-pass typing of
  blocks with catalog patterns ([ADR-0018](../adr/0018-slot-typing.md)):
  each type variable takes the type of the first part, in catalog order,
  that gives one (`T = Student` from a list of `Student`), a later part that
  disagrees is the mismatch, and a block such as `list of` takes its type
  from its place. There is no backtracking, and the editor's copy of the
  rule gives the analyser's answers.
* **Library and user types** (lists, maps, sets, structs, enums, …) convert
  only to the same type: a list of `int` is not a list of `double`.
  **Classes** name sets of types for catalog patterns and checks (`value`,
  what a variable can hold; `comparable`; `printable`; `collection`; …;
  [06 §6.6](06-compiler-pipeline.md#66-stage--types-flow-checks-and-lints)).
  A value outside its input's class is an error (`B2C-E0301`).
* **Opaque types** (from Raw C++ or `auto` deduced from opaque expressions) are
  compatible with everything. Checking is deferred to g++, whose errors are
  still mapped back to blocks. So is a type the editor does not know yet,
  and the type of a part that already has an error.
* **Errors are reported only when certain.** A false positive blocks the user,
  so ambiguity becomes a warning or is deferred to g++.
* **Narrowing** (e.g. `double` → `int`) is allowed with a warning and a quick
  fix ("convert explicitly"), mirroring `-Wconversion` in friendly language.

## 3.6 Symbols and scoping

* **Declarations own symbols.** A `create variable` block, global, function
  parameter, `for` loop variable, *for each* item, file line or opened file,
  `catch` variable, struct, field, enum, enum value or function definition
  creates a symbol with a stable ID (`sym_…`).
* **References point to IDs.** Getter blocks and expression tokens reference
  `sym_…` IDs. Renaming is safe and instant. Deleting a declaration turns its
  references into errors ("`score` no longer exists") with a quick fix to
  re-create it. The editor keeps the last-known name of a deleted symbol for
  the rest of the session. A file stores only the ID, so after a reload such a
  reference shows *missing (`sym_…`)* with the analyser's badge (`B2C-E0201`,
  or `E0202`–`E0204`).
* **Scope follows C++.** A variable is visible from its declaration to the
  end of the enclosing statement list, including nested blocks. Symbol
  dropdowns list **only** the symbols in scope at that block. Dragging a
  block out of scope marks its references as errors in place; a *set* or
  *change* just taken from the toolbox is the exception: if its variable is
  not visible where it is dropped, it takes the first fitting variable
  visible there, in the same Undo step (a getter keeps its variable and
  shows `B2C-E0203`; [04 §4.2](04-user-interface.md#42-toolbox)). The
  dropdowns of blocks that change a variable (*set*, *change*, *flip*,
  *ask*, *set … of*, *read line*'s line and the collection blocks that
  change their collection) list only what may be changed there: they leave
  out constants (also a *catch* part's error and *with arguments*' list), a
  `for` loop's counter inside its loop, a read-only *for each* item, key or
  value, a file loop's line and read-only parameters. *change* lists only
  variables of a number, character or text type, and takes text only with
  *add to the end*; a true/false variable is changed with *flip*
  ([§3.7.2](#372-variables)). A reference field with a type pattern
  ([§3.11.1](#3111-catalog-format)) lists only the symbols whose type fits
  the pattern on its own (*add … to* lists the lists, sets, queues and
  stacks), whatever the block's inputs hold, which are checked against the
  reference and not the reverse. A reference already chosen stays shown. Any
  other type mismatch is left to the analyser. A field that names a member
  (*… of* and *set … of*: a field of a struct; *enum value*: a value of an
  enum) lists the members of the struct or enum its block binds, in row
  order: the struct plugged into *… of*, the chosen variable's struct in
  *set … of*, the enum chosen in *enum value*, whose own menu lists the
  module's enums (never a variable of an enum type). While nothing binds it,
  the menu shows *choose a struct first* or *choose an enum first*; a member
  the type lacks stays shown and is `B2C-E0207`. The editor gets these lists
  from the scope query of
  [06 §6.5](06-compiler-pipeline.md#65-stage--names-and-scopes). A parked
  block's dropdowns list what is visible at the end of `main`'s body (of the
  enabled *when program starts*, never a disabled one; the module's
  functions when there is no `main`); a block inside a disabled statement
  lists what is visible at that statement. A block the analysis has not seen
  yet (just dropped or pasted) lists what its place gives it: what the
  statement before it, or the list or input around it, sees, or, on the
  canvas, the end of `main`'s body. A block still in the toolbox has no
  place in the program: its dropdowns list every variable (or function) of
  the module shown that it may take, grouped by the function that declares
  it, like the Variables category
  ([04 §4.2](04-user-interface.md#42-toolbox)).
* **Unlike Scratch, there are no implicit globals.** *Make a variable* in the
  Variables category asks for a name and a type (`int` by default) and
  inserts a declaration before the selected statement (or at the top of the
  selected function). Globals need an explicit *global* block, a definition
  ([§3.7.2](#372-variables)), and a constant global is `constexpr` wherever
  C++ allows it. With nothing selected, *Make a variable* inserts at the top
  of `main` (creating `main` if there is none). The Variables category lists
  every variable of the module, the globals first under *Everywhere*, then
  grouped by the function that declares it, and does not depend on the
  selection
  ([04 §4.2](04-user-interface.md#42-toolbox)).
* **Default names** are the first free name in scope where the block is
  dropped: `value`, `value2`, `value3`, … for a new variable or global; `i`,
  `j`, `k` for a counted loop; `myFunction` for a new function; `item` for
  a *for each* item, `key` and `value` for a map's pair, `line` for a file's
  line, `file` for an opened file, `e` for a caught error, `args` for *with
  arguments*, and `MyStruct` and `MyEnum` for a new struct and enum. A new
  row of a struct is a field `int field` (then `field2`, …) and a new value
  of an enum `Value` (then `Value2`, …), named by the ⊕ that adds it among
  its definition's rows, as is the error of a new *catch* part. A counted
  loop that wraps statements when it is dropped ([§3.3](#33-block-shapes))
  also leaves out the names they declare, nested ones included: wrapped
  around `create i` or a `for i` loop, it counts with `j`.
* **Identifiers are validated** ([08 §8.4.1](08-security.md#841-identifiers)):
  ASCII `[A-Za-z][A-Za-z0-9_]{0,63}`, not a keyword, not reserved, and not a
  standard macro name. Duplicate names in one scope are errors with a rename
  quick fix. Shadowing an outer name is a warning.

## 3.7 Block catalog by category

Each category has a colour **and** an icon (so colour is never the only
cue), and the colours are checked for contrast (white text and the canvas)
and for distance (every two categories at least 6 apart in OKLab) in the
light and dark themes; checking them under simulated colour-blindness, and
the high-contrast themes, come with the colour-blind-safe palette in M5
([04](04-user-interface.md)). An icon uses only
characters that the default sans fonts of Linux and Windows draw (DejaVu
Sans, and Segoe UI or Segoe UI Symbol), so it never shows as an empty box; a
test in `b2c-catalog` keeps the icons to an allowlist of such characters.
A new icon joins only after three checks: its DejaVu Sans glyph is checked
with fontTools, it falls in a Unicode block that Microsoft documents for
Segoe UI or Segoe UI Symbol, and it is seen drawn in the Windows visual-diff
candidates. Categories marked *(Adv)* are hidden until **View → Show
advanced blocks** is enabled, which keeps the beginner toolbox small.

| Category | Icon | Contents |
| ---------- | ------ | ---------- |
| Program | ▶ | `main`, command-line arguments, exit, headers, compile-time checks |
| Variables | x | declare, set, change, flip, getters, globals and constants |
| Math | ∑ | arithmetic, comparison, math functions, random, conversions |
| Logic | ◇ | and/or/not, true/false, conditional value |
| Text | “ ” | strings, chars, formatting |
| Control | ⋔ | if/else-if/else, switch, going on into the next case, wait |
| Loops | ↻ | repeat, while/until, do … then repeat, for range, for each, forever, break/continue |
| Input / Output | ⌨ | print, ask, read, formatting manipulators |
| Functions | ƒ | define, return, lambda, *Your functions*: a call block for each function you define, kept up to date (grouped under a module heading only when the project has more than one module) |
| Collections | ☰ | list/map/set/queue/stack… polymorphic blocks |
| Types | ◆ | struct, class, enum, alias, members, object creation |
| Files | ▤ | streams, line iteration, filesystem |
| Errors | ⚠ | try/catch, throw, error message |
| Time & Random | ⏱ | clock, stopwatch, sleep, random helpers |
| Memory *(Adv)* | ➚ | pointers, references, smart pointers, move |
| Concurrency *(Adv)* | ⇉ | threads, locks, atomics, async |
| Generics *(Adv)* | ⟨T⟩ | template parameters, constraints |
| Organisation | ▣ | namespaces, `using`, comments |
| Raw C++ *(Adv)* | `C++` | raw statement / expression / top-level |
| Libraries | 📦 | blocks from library packs, grouped by pack |

The tables below show representative blocks. The complete, normative list is
the catalog itself ([§3.11](#311-the-catalog-and-library-packs)), from which
the reference documentation in `docs/reference/blocks/` is **generated**.

**In M2** the catalog has the blocks of `catalog/core/` only: *create* with a
*constant* checkbox (no `constexpr`/`static`), *set*, *change* with the
operators `+= -= *= /= %=` in its menu, getters; numbers, arithmetic,
comparison, *random integer*, *as int/double*; and/or/not, true/false,
conditional value; text, character, *join*; *if/else if/else*; *repeat*,
*repeat while/until*, *for*, *forever*, *leave loop*, *skip to next round*;
*print*, *ask*; *define* (copy, editable and read-only parameters; a
read-only number, character, true/false or enum is passed by value, and any
other type as `const T&`), *return*, calls; *when program starts*, *stop
program*. *create* and a function's parameters take every type a variable
can hold, such as a list of numbers (stored as a type term,
[05 §5.4](05-project-format.md#54-block-nodes)); their menus offer M2's
types until the type picker arrives.

**In M3** (wave W2) the catalog has 86 blocks in 13 categories: the four
new categories, Collections, Types, Files and Errors, follow Functions, and
a category without entries is not shown in the toolbox and gets no page in
the block reference. The blocks W2 built are those of §3.7.1 to §3.7.11 and
§3.7.13 that the sections' *In M3* notes name; the type menus offer lists
and the project's own types until the type picker arrives
([§3.5.2](#352-friendly-and-c-display-modes)). What W2 left, each with its
reason in the [W2 contract](../plans/m3-contracts/w2.md#110-not-in-w2):
*namespace* and *use namespace* (wave W3, with modules); the ⚙ popover,
*format … with* and the unchecked `[]` index (wave W4, with feature
gating); *moved* parameters, the generic member block, the file blocks that
do not open a file, a random seed, the clock, the stopwatch and *read word*
(M4); and the structured binding (after M3). *catch anything* is not
planned ([§3.7.11](#3711-errors)). *wait* is in Control and *random
decimal* in Math, and *use header* and *check at compile time* in Program;
the Time & Random and Organisation categories come with their other
blocks. Classes, lambdas and the advanced categories are M4's
([10 §10.1](10-roadmap.md#101-milestones)).

### 3.7.1 Program

| Block | Generates |
| ------- | ----------- |
| `when program starts ☐ with arguments { … }` | `int main() { … return 0; }` (implicit `return 0`) |
| `when program starts ☑ with arguments [args] { … }` | `int main(int argc, char* argv[]) { const std::vector<std::string> args(argv, argv + argc); … }` |
| `stop program with exit code (0)` | `return 0;` inside `main`, otherwise `std::exit(0);` |

A project has exactly one `main`; a second one is an analyser error.

*with arguments* is a checkbox on the one block (§3.1), whose short name
stays *when program starts*. Ticked, it shows the name of a read-only list
of text (`args` by default) whose first item is the program's own name;
unticked, the name is hidden and kept. The list encloses the body as a
function's parameters do: changing it is `B2C-E0308`, a ticked box without
a name `B2C-E0430`, and a variable of its name created at the top of the
body `B2C-E0210` (inside a nested block it hides the list, `B2C-W0501`).
`argc` and `argv` take other names when the body uses those. The Program
category also holds *use header* and *check at compile time*
([§3.7.13](#3713-organisation)).

### 3.7.2 Variables

| Block | Generates |
| ------- | ----------- |
| `create [int ▾] variable [score] = (0) constant ☐` | `int score = 0;` (`const int` when ticked; the ⚙ popover with `constexpr` and `static` replaces the checkbox in wave W4 as a compatible change). The type is any type a variable can hold; without a value, a list is created empty: `std::vector<int> scores{};` |
| `create [auto ▾] [a], [b] ⊕ from (pairValue)` | `auto [a, b] = pairValue;` (structured binding, after M3) |
| `set [score ▾] to (expr)` | `score = expr;` |
| `change [score ▾] [by ▾] (1)` | `score += 1;` (`++score;` for *by* the literal 1); the menu also gives *down by* (`-=`), *times* (`*=`), *divided by* (`/=`) and *mod* (`%=`), *add to the end* (`+=`, text or a character added to a text variable: `s += "!";`), and, with *Show advanced blocks*, the bitwise forms *bitwise and* (`&=`), *bitwise or* (`\|=`), *bitwise xor* (`^=`), *shifted left by* (`<<=`) and *shifted right by* (`>>=`) for whole numbers. In C++ label mode every option shows its operator. For number, character and text variables |
| `flip [done ▾]` | `done = !done;` (for true/false variables) |
| `(score)` | `score` |
| `global [constant ▾] [double] [kGravity] = (9.81)` *(definition)* | `constexpr double kGravity = 9.81;`; a variable: `int count = 0;` |

The toolbox does not repeat these blocks per variable: it shows one *set*,
one *change* and one *flip* block whose menu chooses the variable
([04 §4.2](04-user-interface.md#42-toolbox)). *change*'s menu lists the
operators the chosen variable's type takes (the arithmetic ones for numbers
and characters, *add to the end* for text), the bitwise forms only for an
`int` variable and only with *Show advanced blocks*, and always the operator
the block already has; a variable of another type is `B2C-E0302`, an amount
of another type `B2C-E0301`. *flip* is listed while a true/false variable
that may change exists. **In M3** (wave W2) these blocks are built as the
table shows, but for the ⚙ popover (wave W4) and the structured binding
(after M3, since pairs come later).

**Globals.** *global* is a definition, wherever it is on the canvas: a
*variable* or a *constant* of any type a variable can hold but `auto` (a
global's type is printed before any value is worked out), with an optional
value; its menu offers what *create*'s does, without `auto`. A constant
needs a value (`B2C-E0430`) and can't be changed (`B2C-E0308`); it is
`constexpr` when it is a `bool`, `char`, `int` or `double` with a value
fixed when the program is built, and `const` otherwise (`const std::string
kName = "Ann";`), and its value can be a `case` label. Globals are declared
after the module's structs and enums and before its functions, so every
function, default value and body of the module sees them. A global's name is
unique among the program's globals, structs, enums and functions
(`B2C-E0210`); a local variable, parameter or caught error with its name
hides it (`B2C-W0501`). A global set from a function, or from a struct's
default values, comes after every global that function or default reads,
whatever the order on the canvas; a global whose value needs itself,
directly or through functions and other globals, is `B2C-E0421`. Its *share
with other modules* checkbox is hidden until wave W3.

### 3.7.3 Math and Logic

| Block | Generates | Notes |
| ------- | ----------- | ------- |
| `(a) [+ ▾] (b)` | `a + b` | `+ − × ÷ mod` → `+ - * / %`. Parentheses inserted only when precedence requires. The block takes numbers; in a typed expression `+` also joins text with text or a character, giving text ([§3.4](#34-expression-slots)). |
| `(a) [< ▾] (b)` | `a < b` | |
| `<a> [and ▾] <b> ⊕` | `a && b && c` | Variadic |
| `not <a>` | `!a` | |
| `[round ▾] of (x)` | `std::round(x)` | *round down*, *round up*, *round*, *square root*, *cube root*, *e to the power*, *natural log*, *log base 10*, *sin*, *cos*, *tan*, *asin*, *acos*, *atan* → `std::floor`, `std::ceil`, `std::round`, `std::sqrt`, `std::cbrt`, `std::exp`, `std::log`, `std::log10`, `std::sin`, … (`<cmath>`); angles in radians; always a decimal number |
| `absolute value of (x)` | `std::abs(x)` | The type of its input; `<cstdlib>` for a whole number, `<cmath>` for a decimal number |
| `[larger ▾] of (a) and (b)` | `std::max<int>(a, b)` | *larger*, *smaller* (`std::min`). The result is the arithmetic type of the inputs, as `+` gives it (`char` and `bool` count as `int`), and is the template argument, so mixed inputs compile: `std::min<double>('a', 2.5)` |
| `[power ▾] of (a) and (b)` | `std::pow(a, b)` | *power*, *atan2*, *hypotenuse* (`std::hypot`); always a decimal number |
| `(v) kept between (lo) and (hi)` | `std::clamp<int>(v, lo, hi)` | The type as for *larger*. A lower limit above the higher one stops a debug build |
| `if <c> then (a) else (b)` | `c ? a : b` | |
| `random integer from (1) to (6)` | `b2c::random_int(1, 6)` | Support helper ([§3.9](#39-support-helpers)) |
| `random decimal from (0.0) to (1.0)` | `b2c::random_real(0.0, 1.0)` | Support helper |
| `(x) as [int ▾]` | `static_cast<int>(x)` | |
| `text (s) as number [int ▾]` | `std::stoi(s)` / `std::stod(s)` | In the Text category. The number at the start of the text (`12abc` gives 12); a text that does not start with one throws an *invalid argument* error, and a number too large for its type an *out-of-range* error. The tooltip explains this and the Errors category shows how to catch it. |
| `(x) as text` | `std::to_string(n)` | In the Text category. Written as *print* writes it: `b2c::to_text(x)` for a decimal number (with the decimal places *show numbers with* chose) and an enum, `std::string(1, c)` for a character, `std::string(b ? "true" : "false")`, and text as it is. A struct, printable or not, is `B2C-E0314` ("… can't be turned into text in one go. Join its fields instead."), as in *join* |

**Analyser lints in this area:** integer division assigned to a floating-point
variable ("did you mean `7.0 / 2`?"), `==` on floating-point values, signed vs
unsigned comparison, and possible overflow of a literal into its target type
(which is an error). The analyser also works out every constant calculation
(literals, constants, operators, *convert*) with C++'s rules: a whole-number
division or *mod* by a value that is always 0 is an error (`B2C-E0316`: the
program would crash), a constant whole-number calculation or *as int* that
does not fit an `int` is an error like a literal that does not fit
(`B2C-E0517`, `2147483647 + 1`), and a decimal division by a constant 0 or a
constant decimal result too large gives infinity or NaN, a warning
(`B2C-W0506`), or the error `B2C-E0517` when a division's infinity or NaN
goes straight into a whole number, which C++ leaves undefined.

### 3.7.4 Text

| Block | Generates |
| ------- | ----------- |
| `"hello"` | `"hello"` (escaped per [08 §8.4.2](08-security.md#842-string-and-character-literals)) |
| `character 'a'` | `'a'` (one ASCII character; use text for anything else) |
| `join (a) (b) ⊕` | Idiomatic per part: `std::string(a) + b`, `+ std::to_string(n)` for whole numbers and `+ b2c::to_text(x)` for decimal numbers, which writes the number exactly as `print` would, `+ std::string(1, c)`; `std::format` when the *formatting style* setting is `format` and the toolchain supports it |
| `format ("{} scored {}") with (name) (score) ⊕` | `std::format("{} scored {}", name, score)` (C++20 with GCC 13+; otherwise an `std::ostringstream` helper; wave W4) |
| `length of (s)` | `static_cast<int>(s.size())`; a character is `B2C-E0301` ("Use a text value here.") |
| `letter (i) of (s)` | `s.at(static_cast<std::size_t>(i))`, a character; a literal position as written (`s.at(0)`) |
| `part of (s) from (i) length (n)` | `s.substr(static_cast<std::size_t>(i), static_cast<std::size_t>(n))`, a literal as written; fewer characters when the text ends first |
| `(s) [contains ▾] (t)` | `s.find(t) != std::string::npos`; *starts with* and *ends with* are `s.starts_with(t)` and `s.ends_with(t)` in C++20, and the helpers `b2c::starts_with(s, t)` and `b2c::ends_with(s, t)` in C++17 |
| `[lowercase ▾] (s)` / `trim (s)` | `b2c::to_lower(s)`, `b2c::to_upper(s)` (only the letters A to Z change), `b2c::trim(s)` (spaces and tabs at both ends): support helpers |
| `split (s) by (",")` | `b2c::split(s, ",")`, a list of text; two separators in a row give an empty part, and an empty separator one part for each character |
| `replace all (a) with (b) in (s)` | `b2c::replace_all(s, a, b)`; an empty first text changes nothing |

In a typed expression, `+` joins text with text or a character, in either
order, as C++ does (`"Hi " + name`, `name + '!'`). When neither side is a
`std::string` object, the left side is made one: `std::string("a") + "b"`,
`std::string(1, 'b') + "a"`. Text with a number needs *join*
(`B2C-E0303`).

The text these blocks give is a `std::string` object and is used as it is:
*for each* over *lowercase* of a text is
`for (const auto& letter : b2c::to_lower(text))`, and a *join* that starts
with one does not wrap it in `std::string(…)`. **In M3** (wave W2) every
block of this table is built but *format … with*, which comes with wave
W4's feature gating (it needs GCC 13 and C++20, or a fallback); *as text*
and *text as number* are in the Text category
([§3.7.3](#373-math-and-logic)).

### 3.7.5 Control and Loops

| Block | Generates |
| ------- | ----------- |
| `if <c> then { } ⊕else if ⊕else` | `if (c) { } else if (d) { } else { }`. One block: *else if* and *else* are added and removed with ⊕/⊖ on the block. |
| `switch on (v) case (1) { } case (2) { } ⊕ default { }` | `switch (v) { case 1: { … break; } default: { … } }`. Each case is a `{ }` scope ending in an automatic `break;`, left out after *return*, *stop program*, *throw*, *throw the error again*, *skip to next round* or *go on into the next case*. Case values must be fixed when the program is built. |
| `go on into the next case` | `[[fallthrough]];` The last block of a case that is not the last part: the next case's blocks run too. |
| `repeat (10) times { }` | `for (int i = 0; i < 10; ++i) { }` (a hidden counter with a unique, readable name). A count that could change while the loop runs (it calls a function, or reads a variable the body may change) is evaluated once: `for (int i = 0, n = b2c::random_int(1, 6); i < n; ++i)`. The same applies to a `for` loop's end and step. |
| `repeat [while ▾] <c> { }` (choose *until* in the menu) | `while (c) { }` / `while (!(c)) { }` (simplified to the inverse operator where that is exact, e.g. `!(a == b)` → `a != b`; ordering comparisons on decimal numbers are not inverted, because `!(x < y)` and `x >= y` differ when a value is NaN) |
| `do { } then repeat [while ▾] <c>` | `do { } while (c);`; *until* inverts the condition as *repeat until* does. The condition sees only what is visible before the block, not the variables its blocks create. |
| `for [i] from (1) [to ▾] (10) [step (1)]` | `to` → `i < 10`, `through` → `i <= 10`, `down to` → `i >= 10` with `i -= step`. The direction is explicit, so a runtime step sign is never needed. `step` is hidden until expanded. |
| `for each [item] in (list) [read-only ▾]` | `for (const auto& item : list)`; *copy* → `auto item` (`bool item` over a list of true/false values, whose items `auto` would take as references into the list), *editable* → `auto& item`. Over a list, deque, set or text, in order |
| `for each key [key] value [value] in [map ▾]` | `for (const auto& [key, value] : map)`, smallest key first; neither can be changed |
| `forever { }` | `while (true) { }` (info lint if no `break`, `return` or exit inside) |
| `break` / `continue` / `return (x)` | *leave loop*, *skip to next round*, *return*. Validated by the analyser: only inside loops, and the `return` type is checked |
| `wait (0.5) seconds` | `std::this_thread::sleep_for(std::chrono::duration<double>(0.5));` (`<chrono>`, `<thread>`), in the Control category |

**switch.** The value switched on is a whole number, a character or a value
of an enum (another type is `B2C-E0301`). Case values are value inputs typed
by it, starting empty; ⊕ adds a case (each copy joined by the word *case*,
as *else if* joins the *if* block's), and the default part comes and goes
with ⊕/⊖. A case value must be fixed when the program is built: a literal, a
constant with a fixed value, an enum value or a calculation of them
(`B2C-E0424` otherwise), and no two may be equal (`B2C-E0425`, `'A'` and
`65` too). On an enum, the case values are the enum's values, printed in
full (`case Action::Deposit: {`); a whole number or another enum's value is
`B2C-E0301`. A switch on an enum without a default part that misses some of
its values warns `B2C-W0508`, naming them, and gets `default: { break; }` so
that g++'s `-Wswitch` stays quiet; one with a case for every value gets no
default part. There is no *fall through ☑* checkbox: falling through is the
block *go on into the next case*, which must be the last block of a case and
not in the last part (`B2C-E0427`). *leave loop* whose nearest loop or
switch is a switch is `B2C-E0401`, since in C++ it would leave only the
switch. A switch ends every path (for `B2C-E0410`) only with a default part
whose parts all end it: on an enum with a returning case for every value,
the value may still be none of them.

**for each.** Its collection is a value input: a list, deque or set, or
text (one character at a time). A map is `B2C-E0301` ("Use 'for each key,
value' for a map."), and *list of* there has no type (`B2C-E0315`). A
read-only item is `B2C-E0308` when changed; *editable* needs a variable
that may change (`B2C-E0308`) holding a list or a deque whose items are not
true/false values (`B2C-E0301`: g++ cannot bind `auto&` to the items of a
`std::vector<bool>`). Inside a *for each* or *for each key, value* over a
variable, *add*, *remove item at*, *remove* and *clear* on that variable
are `B2C-E0308`: they may free what the loop is going through (undefined
behaviour in C++); *set item* and *sort* keep every item where the loop
can reach it. The item is `[[maybe_unused]]` when the body
never mentions it, and a pair's key and value when the body mentions
neither. **In M3** (wave W2) every block of this table is built, *wait* and
*go on into the next case* in Control and *do … then repeat* after *repeat
while* in Loops.

### 3.7.6 Input / Output

| Block | Generates | Notes |
| ------- | ----------- | ------- |
| `print (a) (b) ⊕ [no separator ▾] new line ☑ to [the console ▾]` | `std::cout << a << b << '\n';` | `bool` prints as `true`/`false` (via `(b ? "true" : "false")`); 8-bit integers print as numbers (unary `+`); separators can be spaces or commas; *the error stream* writes to `std::cerr` |
| `ask ("Name? ") and save answer in [name ▾]` | *keep asking until valid* (default): `name = b2c::ask_line("Name? ");` reads one line; an empty line asks again | Leading spaces are dropped. `simple` mode emits `std::cout << "Name? "; std::getline(std::cin >> std::ws, name);`, where `std::ws` avoids the classic "getline after `>>`" bug |
| (same, numeric target) `[keep asking until valid ▾]` | `age = b2c::ask<int>("Age? ");` | The helper asks again after invalid input, with a short message such as *"Please enter a whole number."* An answer is valid only when the whole line, apart from spaces, is one value of the variable's type: `3.5` or `12abc` for a whole number asks again with *Please enter a whole number.* At end of input (e.g. stdin from a file) it prints *"Input ended"* and exits with code 1 instead of looping forever. `simple` mode emits `std::cin >> age;` followed by `std::cin.ignore(std::numeric_limits<std::streamsize>::max(), '\n');`, which drops the rest of the line, so a later `ask` in either mode reads the next line. Both helpers first recover from a failed `simple` read (for example a word typed for a number): they clear the stream and drop that line. |
| `read [word ▾] into [w]` | `std::cin >> w;` / `std::getline(std::cin, w);` | M4 |
| `show numbers with (2) decimal places` | `std::cout << std::fixed << std::setprecision(2);` | From there on, every decimal number is printed with that many decimal places, also by *join* and *as text* (`b2c::to_text` copies the console's format); whole numbers do not change, and neither do files |

**In M3** (wave W2) *show numbers with* is built. *read word*, with the
other console stream blocks, and the clock and the stopwatch of the Time &
Random category wait for M4: *ask* reads every input the M3 assignments
have, and the clock and the stopwatch need time points and a `Stopwatch`
helper that no M3 type can hold.

### 3.7.7 Functions

| Block | Generates |
| ------- | ----------- |
| `define [greet] with ([string] name [read-only ▾]) ([int] times = (1)) ⊕ returns [nothing ▾] { }` | `void greet(const std::string& name, int times = 1) { }`. Parameter pass modes: *copy*, *editable* (`&`), *read-only* (`const&`; a number, character, `bool` or enum by value), *moved* (`&&`, M4). A parameter, and the result, take every type a variable can hold. |
| *(auto-generated, under Your functions)* `greet (name: "Ada") (times: 3)` | `greet("Ada", 3);` A statement block when the result is `void`, a reporter otherwise. A reporter can be used as a statement via *ignore result*. |
| `return (value)` | `return value;` |
| `function ([auto] x) ⊕ capturing [nothing ▾] returns [auto] { }` | `[](auto x) { … }`. Capture options: nothing, everything by reference `[&]`, everything by copy `[=]`, or chosen variables. |
| `function of ([auto] x) giving (x * 2)` | `[](auto x) { return x * 2; }` (single-expression form) |

Recursion is supported. **Definition order on the canvas never matters**:
the generator emits forward declarations automatically
([06 §6.7](06-compiler-pipeline.md#67-ordering-and-declarations)).

**Default values.** A parameter row may end with a default value, `= (…)`,
typed by the row's own parameter. It is printed in the function's
declaration only (`void greet(const std::string& name, int times = 1);`),
and a call may then leave out the last arguments that have one: the call's
⊖ goes down to the required ones. Once a row has a default, every later row
needs one (`B2C-E0426`). A default sees the module's definitions (its
globals, functions, structs and enums), not the parameters (`B2C-E0203`);
it can't name a global or call a function that has the name of its own
input or an earlier input of the same function (`B2C-E0205`: from an input
on, C++ finds the input), nor call its own function (`B2C-E0421`). An
*editable* parameter's default must be a variable of its type that may
change (`B2C-E0307`), as its argument must.

**Overloads.** Two functions of one module may share a name when a call can
tell them apart by its argument types, counting the arguments a default
value lets a call leave out and ignoring pass modes; otherwise the second
is `B2C-E0211`. A call names its function by symbol ID, never by name. On a
call to a function whose name another function of the module shares, the
generator casts each argument to its parameter's type, writes quoted text
as `std::string("…")` and a *list of* with its type
(`show(std::vector<int>{1})`), so g++ picks the same function; calls to
other functions print their arguments as before. *Your functions* labels
each function that shares its name with its parameter types (*area (int,
int)*), and so do a call's function menu and the call block itself.
Functions of different modules still need different names until
wave W3, since every module is compiled on its own
([§3.8](#38-organisation-features-editor)).

**In M3** (wave W2) default values, overloads and results of any type are
built; *moved* parameters wait for M4, where they are useful with *move
(x)*, and lambdas for M4 too.

### 3.7.8 Collections

Polymorphic phrase blocks work across container types. Lowering picks the
idiomatic operation for the static type, and the dropdowns only show what that
type supports.

| Block | vector / deque | set | map | queue / stack / priority_queue |
| ------- | ---------------- | ----- | ----- | -------------------------------- |
| `add (x) to [c]` | `c.push_back(x)` | `c.insert(x)` | — (use `set item`) | `c.push(x)` |
| `item (i) of [c]` | `c.at(static_cast<std::size_t>(i))` | — | `c.at(k)` | — |
| `next item of [c]` | — | — | — | `c.front()` / `c.top()` |
| `set item (i) of [c] to (x)` | `c.at(static_cast<std::size_t>(i)) = x` | — | `c[k] = x` | — |
| `remove item at (i) from [c]` | `b2c::remove_at(c, i)` (out of range: `std::out_of_range`, as `.at()`) | — | — | — |
| `remove (x) from [c]` | — | `c.erase(x)` | `c.erase(k)` | — |
| `remove next from [c]` | — | — | — | `c.pop()` |
| `length of [c]` | `static_cast<int>(c.size())` | ← | ← | ← |
| `[c] contains (x)` | `std::ranges::find(c, x) != c.end()` (C++20), `std::find(c.begin(), c.end(), x) != c.end()` (C++17) | `c.contains(x)` (C++20), `c.count(x) != 0` (C++17) | `c.contains(k)`, `c.count(k) != 0` | — |
| `[c] is empty` | `c.empty()` | ← | ← | ← |
| `clear [c]` | `c.clear()` | ← | ← | `c = {};` |
| `sort [c] [smallest first ▾] ⊕ by (lambda)` | `std::ranges::sort(c)` / `std::ranges::sort(c, std::greater{})` (C++20), `std::sort(c.begin(), c.end())` / with `std::greater<>{}` (C++17, and a list of true/false values in C++20, whose iterators `std::ranges::sort` refuses) / with comparator | — | — | — |
| `list of (a) (b) (c) ⊕` | `{a, b, c}` initialiser (type from its place) | ← | — | — |

A map is filled with *set item … of*, and its pairs are visited with *for
each key, value* ([§3.7.5](#375-control-and-loops)); a map form of *list of*
waits until pairs exist (after M3).

The collection field lists only the variables whose type the block takes
(*add … to*: lists, deques, sets, queues, stacks and priority queues), and a
block that changes its collection lists only those that may be changed
there ([§3.6](#36-symbols-and-scoping)). *add* and *set item* start their
value with the item type's start value once the collection is chosen, and
*item* and *set item* their position with `0` for a list. A
value must be the item's type (*contains* on a list looks for an item, on a
set or a map for a key), the items of a list searched by *contains* must be
comparable (`B2C-E0313`), and *sort* takes a list or deque of comparable
items, smallest or largest first. *list of* takes its type from its place,
such as the value of a *create* whose type is chosen or an input that
expects a list (`B2C-E0315` where the place gives none); it holds 0 to 32
items, 2 when it is taken from the toolbox, each cast as a value converted
to the item type is (`{static_cast<double>(i), 2.5}`; a literal that fits
stays as written), and is printed with its type where C++ cannot tell it,
such as the list *contains* looks for (`std::vector<int>{1, 2}`). **In M3**
(wave W2) every row of this table is built but the lambda comparator of
*sort* (M4, with lambdas), and the generic member block below waits for
M4's `std` pack, whose metadata it lists.

Indexing is bounds-checked (`.at()`) by default. A block context-menu option,
*fast unchecked access*, switches to `[]` (wave W4; until then every index
is checked). Indexing is always zero-based, as
in C++, and tooltips say so. Sizes and indexes are `int`, as M3 has no
unsigned types, so the generated code casts them
(`static_cast<int>(c.size())`, `c.at(static_cast<std::size_t>(i))`; a
non-negative literal index stays as written) and compiles clean at every
warning level.

**Generic member block.** `[expr ▾] . [member ▾] (args…)` lists the fields
and methods of the expression's static type, taken from user types and
library-pack metadata. Argument slots reshape to the chosen method's
signature. Through `.` and `->` (chosen automatically for pointers and smart
pointers), this one block reaches most of the standard library without a
dedicated block for each method.

### 3.7.9 Types (structs, classes, enums)

| Block | Generates |
| ------- | ----------- |
| `struct [Point] has [double] x = (0) ⊕ ☐ comparable ☐ printable` | `struct Point { double x = 0; … };`, and `T name{};` for a field without a default value. *comparable* adds `auto operator<=>(const Point&) const = default;` (C++20) or the six comparison operators as hidden friends built on `std::tie` (C++17); *printable* adds a hidden-friend `operator<<` that prints `Point{x: 1, y: 2.5}` |
| `new [Point] with x: (1) y: (2)` | `Point{.x = 1, .y = 2}` (C++20 designated initialisers, an empty part left out), or `Point{1, 2}` in C++17 mode (`{}` for an empty part before a filled one). Parts are generated from the fields, in declaration order, and labelled by them. |
| `class [Player] inherits [public ▾] [Entity] ⊕ { members }` | `class Player : public Entity { … };` |
| ↳ `field [private ▾] [int] [health] = (100) ⚙` | Member variable. Members are grouped into `public:` / `protected:` / `private:` sections, keeping their order within each section. |
| ↳ `method [public ▾] [takeDamage] ([int] amount) ⊕ returns [nothing ▾] ⚙ { }` | `⚙`: `const`, `static`, `virtual`, `override`, `final`, `= 0` (abstract), `noexcept`, `[[nodiscard]]` |
| ↳ `when created with ([string] name) ⊕ ⚙ set [health ▾] to (100) ⊕ { }` | Constructor with a member-initialiser list (`⚙`: `explicit`, `= default`, `= delete`) |
| ↳ `when destroyed { }` | Destructor |
| ↳ `operator [+ ▾] ([const Player&] other) returns [Player] { }` | `Player operator+(const Player& other) const { … }` |
| `this object` | `this` |
| `[field ▾] of (obj)` / `set [field ▾] of [obj ▾] to (value)` | `obj.field` / `obj.field = value;` (or `obj->field` automatically, M4) |
| `enum [Color] values [Red] [Green] [Blue] ⊕` | `enum class Color { Red, Green, Blue };` plus a generated `operator<<` when the enum is printed, written to a file or a field of a printable struct, and `b2c::to_text(Color)` when it is joined or turned into text |
| `[Color ▾] [Red ▾]` | `Color::Red` |
| `type alias [Grid] = [list of list of int ▾]` | `using Grid = std::vector<std::vector<int>>;` |

**Structs and enums in M3** (wave W2). The struct, *new … with*, *… of*,
*set … of*, enum and enum value blocks are built, in the Types category;
classes, type aliases, `this` and `->` come in M4. A struct's fields and an
enum's values are rows added with ⊕ and removed with ⊖, at most 64: a new
field is `int field` (then `field2`, …), a new value `Value` (then `Value2`,
…). A field takes any type a variable can hold (its menu offers M2's five
types and the project's structs and enums, and lists of them), and may end
with a default value, `= (…)`, typed by its own field; a default sees the
module's globals and functions, not the other fields, and can't name a
global or call a function called like a field of the same struct
(`B2C-E0205`: inside a struct, C++ finds the field). A field or value named
twice, a field named like its struct or any other struct or enum, a struct
or enum named like another definition or a function, and a variable,
function input, loop item, caught error or program's arguments named like a
struct or enum (C++ would find that name instead of the type) are
`B2C-E0210`; structs that hold each
other by value are `B2C-E0420`, and any other cycle between definitions
`B2C-E0421`. *comparable* and *printable* need fields that are themselves
comparable or printable (`B2C-E0313`, `B2C-E0314` at the tick, naming the
field). Types are printed in the order their definitions need, whatever the
order on the canvas: enums and structs before the functions, a struct after
the types it holds, and `struct B;` first when a struct holds a later one
inside a list.

*new … with* follows its struct as a call follows its function: ⊕ is offered
until it has a part for each field, each part's start value comes from its
field, and a count of parts other than the struct's fields is `B2C-E0306`
("`Student` has 2 fields, but this block has 1 part. …"). Each part is cast
as a value converted to its field's type is. In C++17 an empty part before a
filled one whose field has a default value is `B2C-E0430`, since `{}` would
skip the default (C++20 leaves the field out and keeps it). The struct menu
of *new … with* lists the module's structs (*choose a struct first* while
there is none). *… of* takes its struct as a value input, so it reads a
field of any struct value (an item of a list, a call's result); its field
menu lists the fields of the struct plugged in ([§3.6](#36-symbols-and-scoping)),
and a field the struct lacks is `B2C-E0207`. *enum value*'s first menu lists
the module's enums, its second their values. *join* and *as text* refuse a
struct, printable or not (`B2C-E0314`: join its fields instead), and an enum
joined or turned into text uses `b2c::to_text`, an inline function in
`namespace b2c` right after the enum, so a user function `to_string(Color)`
compiles beside it. A struct's and an enum's *share with other modules*
checkbox is hidden until wave W3.

**Safety defaults:** a class with any `virtual` method automatically gets
`virtual ~Class() = default;` unless it declares its own destructor (shown in
the C++ view and in the class block's `⚙`). Copy/move special members can be
`= default` / `= delete` via the class `⚙`.

### 3.7.10 Files

| Block | Generates |
| ------- | ----------- |
| `open file ("scores.txt") for [reading ▾] as [file] ⊕ if it can't be opened { }` | `std::ifstream file("scores.txt");` (*writing* → `std::ofstream`; *adding to the end* → `std::ofstream file("scores.txt", std::ios::app);`), plus, with the part, `if (!file) { … }` |
| `read line from [file ▾] into [line ▾]` | `b2c::read_line(file, line)` (`std::getline` without a final `'\r'`): a predicate, true when a line was read, false at the end of the file |
| `for each line [line] in file ("data.txt") { } ⊕ if it can't be opened { }` | `{ std::ifstream b2c_file("data.txt"); if (!b2c_file) { … } std::string line{}; while (b2c::read_line(b2c_file, line)) { … } }` |
| `write (a) (b) ⊕ to [file ▾] new line ☑` | `file << a << b << '\n';` |
| `file ("x") exists` / `delete file` / `create folder` / `for each file in folder` | `std::filesystem::exists("x")` (a file or a folder); the other three in M4 |

A path that is not a full path is looked for in the folder the program runs
in: the project folder, or its sandbox folder when the run settings choose
it or the project is not saved yet. A program reads and writes only the
files its blocks name, as the user (decision M3-D3), and no block deletes a
file. **In M3** (wave W2) the first five blocks are built, in the Files
category; *delete file*, *create folder* and *for each file in folder* act
on files no block opened, so they wait for M4's filesystem pack and its
threat review.

* **for each line** reads a text file line by line, without the line
  breaks. Both it and *read line* read with `b2c::read_line`, which drops
  the `'\r'` that a file saved with Windows line endings leaves at the end
  of each line on Linux and macOS (on Windows the stream drops it), so a
  file gives the same lines on every system. Its line is read-only and visible in the body only; the stream has
  a hidden name (`b2c_file`, then `b2c_file2` for a loop inside it), and the
  whole loop is a block of its own, so the file closes after it. Without
  the *if it can't be opened* part, nothing runs when the file can't be
  opened; the toolbox's block has the part.
* **open file** declares a stream: `std::ifstream` for *reading*,
  `std::ofstream` otherwise. It is visible in its *if it can't be opened*
  part and after the block in its list, and closes at the end of that list.
  C++ declares it from its name on, also in its own path, so a path that
  reads a variable or global of the stream's name is `B2C-E0205` (it would
  read the new stream), as *create*'s starting value is.
* **read line** is the bare `b2c::read_line(file, line)` where it is a
  condition of its own (*if*, *else if*, *repeat while*, *do … then
  repeat while*), and `static_cast<bool>(b2c::read_line(file, line))`
  elsewhere (negated by *until*, in a calculation, printed). Its line must
  be a text variable that may change (`B2C-E0308`).
* **write** writes its values as *print* shows them in the console (`true`
  and `false` for true/false values), and refuses what can't be printed
  (`B2C-E0314`); untick *new line* to go on writing on the same line.
* A file part takes only a stream of its kind: *read line* one opened for
  reading, *write* one opened for writing or adding (`B2C-E0301`: "This is an
  output file, but this block needs an input file here, …").

### 3.7.11 Errors

| Block | Generates |
| ------- | ----------- |
| `try { } catch [std::exception ▾] as [e] { } ⊕` | `try { } catch (const std::exception& e) { }`. The dropdown lists the standard errors by their C++ names (`std::exception`, which catches every standard error, `std::invalid_argument`, `std::out_of_range` and `std::runtime_error`), and the user exception classes in M4. |
| `throw [std::runtime_error ▾] with message ("message")` | `throw std::runtime_error("message");` (also `std::invalid_argument` and `std::out_of_range`) |
| `message of [e ▾]` | `e.what()`, used as text |
| `throw the error again` | `throw;` (only valid inside a catch part: `B2C-E0402`) |

A *try* has 1 to 8 catch parts, added with ⊕ (each copy joined by the word
*catch*) and removed with ⊖; a new part's error is named `e` (then `e2`, …).
The parts are tried in order, and `std::exception` catches every standard
error. Each part's error is visible in its own part only, and can't be
changed (`B2C-E0308`). A part that never runs, because an earlier part
catches the same error or `std::exception`, warns `B2C-W0507` (g++'s
`-Wexceptions` warns too). *throw* and *throw the error again* end their
path, as *return* does (`B2C-W0502` for the blocks after them, and no
`B2C-E0410`). There is no *catch anything* (`catch (...)`): `std::exception`
catches every standard error and has a message, which `...` has not.
**In M3** (wave W2) the four blocks are built, in the Errors category;
user exception classes come with classes in M4.

### 3.7.12 Memory, Concurrency, Generics *(Advanced)*

| Block | Generates |
| ------- | ----------- |
| `address of [x]` / `value at (p)` | `&x` / `*p` |
| `make [unique ▾] [Player] with (args) ⊕` | `std::make_unique<Player>(args)` / `std::make_shared<…>` |
| `move (x)` | `std::move(x)` (the analyser warns on later use of a moved-from variable) |
| `null` / `(p) is null` | `nullptr` / `p == nullptr` |
| `new [T] (args)` / `delete (p)` | Raw `new`/`delete`, available only with *manual memory ☑* in project settings; each use gets a lint suggesting smart pointers |
| `start thread [t] running { }` | `std::jthread t([&] { … });` (joins automatically at scope end) |
| `wait for thread [t]` | `t.join();` |
| `with lock on [m] ⊕ { }` | `{ const std::scoped_lock lock(m); … }` |
| `run in background (fn) as [future]` / `result of [future]` | `std::async(std::launch::async, fn)` / `future.get()` |
| *(on any definition)* `for any type [T] ⊕ [that is ▾ a number]` | `template <typename T>` with optional C++20 constraint (`std::integral`, `std::floating_point`, `std::totally_ordered`, …) |

### 3.7.13 Organisation

| Block | Generates |
| ------- | ----------- |
| `namespace [geometry] { definitions }` | `namespace geometry { … }` |
| `use header [<cmath> ▾]` | `#include <cmath>`. Usually unnecessary, because includes are computed automatically. The dropdown offers standard headers, library-pack headers and project modules only. A definition, wherever it is on the canvas; its header joins the module's `#include` lines once, merged with the headers the blocks bring. |
| `use namespace [std ▾] in this file` (module top level) · `use namespace [std ▾] here` (inside a function) | `using namespace std;` for this module's `.cpp`, or from this point to the end of the statement list. The dropdown offers `std`, the standard sub-namespaces of the project's standard (`std::chrono`, `std::this_thread`, `std::filesystem`; `std::numbers`, `std::ranges`, `std::views` from C++20) and the module's top-level namespaces. Never in a header or a class or namespace block (enforced). The project setting *Textbook style* does the same for every `.cpp`. How names are then spelled, and the few that must be renamed: [06 §6.14](06-compiler-pipeline.md#614-standard-names-and-using-namespace). |
| `check at compile time <cond> message [check failed]` | `static_assert(cond, "check failed");` A statement; the message is a text field, escaped as quoted text is. |
| Block comment (Blockly comment bubble) | `// comment` lines above the statement (sanitised, [08 §8.4.3](08-security.md#843-comments)) |

**In M3** (wave W2) *use header* and *check at compile time* are built, in
the Program category after *stop program*; *namespace* and *use namespace*
come with modules in wave W3, with the textbook-style setting. *use header*
offers the standard headers of the header table (another header is
`B2C-E0607`). *check at compile time* checks its condition as *if* does (a
number there warns `B2C-W0519`); a condition that is not fixed when the
program is built, because it reads a variable or calls a function, is
`B2C-E0424`, and one that is always false `B2C-E0428`. Its message must be
one a string literal can hold (`B2C-E0312` otherwise: a NUL character, or
over 64 KiB).

## 3.8 Organisation features (editor)

Clear organisation is a core requirement, not polish:

1. **Modules = tabs = files.** Each module becomes `name.cpp`, plus `name.hpp`
   when it has *shared* definitions. Using another module's shared
   definitions adds the `#include` automatically, and the toolbox's
   *Your functions* offers a call block for each function you define, kept
   up to date (grouped under a module heading only when the project has more
   than one module).
   **In M2** every module is compiled on its own: functions cannot be called
   across modules (`E0206`), their names are unique in the program
   (`E0211`), and helpers are inline in each file. Since wave W2 of M3, two
   functions of one module may share a name as overloads
   ([§3.7.7](#377-functions)); functions of different modules still may not
   until wave W3. Nor may a type name another module's struct or enum
   (`E0206`), and the type menus list only the module's own.
2. **Order-independent definitions.** Top-level definition blocks can sit
   anywhere on the canvas. The generator sorts them by dependency and adds
   forward declarations.
3. **Frames.** Titled, coloured rectangles that group top-level blocks
   ("Input handling", "Physics"). Moving a frame moves its contents. Frames
   optionally emit `// ===== Title =====` banner comments.
4. **Outline panel.** A tree of Modules → Types → Functions → Members.
   Clicking a node scrolls to and selects the block.
5. **Navigation.** *Go to definition* (from any call/reference), *Find all
   references*, *Rename symbol* (everywhere, including Raw C++ token-level
   matches after confirmation), back/forward history, bookmarks.
6. **Search.** `Ctrl+F` searches block text, identifiers and comments in the
   current module. `Ctrl+Shift+F` searches all modules.
7. **Tidy up.** Auto-arrange top-level blocks into columns: types, then
   functions, then `main`. Choose per-module or per-frame.
8. **Collapse.** Collapse any block to a one-line summary (`define greet(name,
   times) …`), or use *Collapse all definitions* to see only signatures.
9. **Minimap** of the current module (toggleable).
10. **Disable block.** A disabled block is greyed out and excluded from
    generation, which is useful for experiments. The C++ view shows nothing
    for it.
11. **Snippets.** Save a selection as a reusable snippet (stored per-user).
    Inserting it remaps symbol IDs and resolves references by qualified name
    in the target scope ([06 §6.14.11](06-compiler-pipeline.md#61411-names-typed-in-slots)).

## 3.9 Support helpers

Some blocks generate a call into a tiny, readable **support library** of
helpers in the `b2c` namespace: `random_int`, `random_real`, `ask<T>`,
`to_text`, `trim`, `split`, `to_upper`, `replace_all`, `Stopwatch` and
similar.

**In M3** (wave W2) the helpers are `random_int`, `ask<T>` and `ask_line`
(M2), `to_text` for decimal numbers (written as `std::cout` would write
them now), and `split`, `to_lower`, `to_upper`, `trim`, `replace_all`,
`starts_with` and `ends_with` (C++17 only; C++20 has the members),
`random_real`, `remove_at` and `read_line`. `split` keeps empty parts and gives one part
for each character for an empty separator; `to_lower` and `to_upper` change
the letters A to Z only; `trim` drops spaces and tabs; `replace_all` with an
empty old text changes nothing; `remove_at` throws `std::out_of_range` for a
position the list does not have, as `.at()` does; `read_line` is
`std::getline` without a final `'\r'`. Each is compiled and run
under C++17 and C++20 at the strict warning level. An enum's `to_text` is
generated with the enum, not a helper ([§3.7.9](#379-types-structs-classes-enums)),
and `Stopwatch` comes with the stopwatch in M4.

* Helpers are plain C++ with Doxygen comments, versioned with the catalog, and
  emitted **only if used**.
* **Placement.** Single-module projects get the helpers inline in a clearly
  delimited section at the top of `main.cpp`, so a student can submit one
  file. Multi-module projects get them in `b2c_support.hpp`. **In M2** every
  module is compiled on its own: functions cannot be called across modules
  (`E0206`), their names are unique in the program (`E0211`; since wave W2
  of M3, overloads of one module excepted), and helpers are inline in each
  file.
* A project setting, **Prefer plain standard C++**, swaps helpers for inline
  standard code wherever a reasonable inline form exists (e.g. `std::cin >>
  x;` for `ask`).
* Helpers are covered by the same golden and unit tests as the generator.

## 3.10 Raw C++ blocks

| Block | Shape | Use |
| ------- | ------- | ----- |
| `C++ statements { code }` | Statement | Any statements, e.g. a `goto`, a coroutine body, platform calls |
| `C++ expression (code) of type [T ▾]` | Reporter | An expression with a declared result type (or `opaque`) |
| `C++ declarations { code }` | Definition | Macros, unions, specialisations, `extern "C"` blocks, … |

* Edited in a CodeMirror modal with C++ highlighting. Shown on the canvas with
  a hazard-striped border, a `C++` badge, and the first line as a preview.
* **Not parsed by our analyser** beyond tokenisation (used for rename
  tracking, bidi/invisible-character checks and the `using namespace` checks of
  [06 §6.14.9](06-compiler-pipeline.md#6149-raw-c)). They are treated as
  opaque, g++ checks them, and diagnostics still map back to the block.
* **Trust impact.** A project containing Raw C++ shows a persistent
  `Contains Raw C++ (n)` indicator, and the Restricted Mode dialog lists the
  raw blocks for review ([08 §8.3](08-security.md#83-workspace-trust-and-restricted-mode)).
* Quick Insert offers *Insert as Raw C++* when typed text cannot be parsed as a
  supported statement. It is never done silently.

## 3.11 The catalog and library packs

### 3.11.1 Catalog format

All block types are defined declaratively in TOML under `catalog/`. The Rust
compiler (`b2c-catalog`) loads and validates these files, and the editor's
block definitions and the block reference are generated from the validated
result. This gives one source of truth for both editor and compiler.

```toml
# catalog/core/io.toml
[[block]]
id       = "io.print"                  # stable, namespaced ID; never reused
version  = 1                           # bumped on breaking change; migrations required
category = "io"
shape    = "statement"
label    = { friendly = "print %ITEM … %SEP new line %NEWLINE to %STREAM", cpp = "std::cout << %ITEM << …;" }
lowering = "builtin"                   # hand-written Rust lowering for core blocks
headers  = ["<iostream>"]
help     = "Shows values in the console."

[[block.extra]]                        # mutator state, saved in the block's "extra"
name    = "itemCount"
kind    = "count"                      # count | flag | params
min     = 1
max     = 32
default = "1"

[[block.input]]
name    = "ITEM"                       # repeated: ITEM0, ITEM1, … (itemCount inputs)
check   = "any"                        # type class for the editor's connection checker
repeat  = { count = "itemCount" }
default = [{ str = "Hello, world!" }]  # tokens shown (as a shadow) while the input is absent

[[block.field]]
name    = "SEP"
kind    = "dropdown"
options = [["no separator", "none"], ["spaces", "space"], ["commas", "comma"]]
default = "none"

[[block.field]]
name    = "NEWLINE"
kind    = "checkbox"                   # the label's words next to it say what it means
default = true

[[block.field]]
name    = "STREAM"
kind    = "dropdown"
options = [["the console", "out"], ["the error stream", "err"]]
default = "out"

# catalog/core/math.toml
[[block]]
id       = "math.random_int"
version  = 1
category = "math"
shape    = "reporter"
output   = "int"                       # required for reporters and predicates
label    = { friendly = "random integer from %LOW to %HIGH", cpp = "b2c::random_int(%LOW, %HIGH)" }
lowering = "builtin"                   # no headers: the helper brings its own
help     = "A random whole number between the two values, including both."

# catalog/core/variables.toml (excerpt)
[[block]]
id    = "var.declare"
where = { T = "value" }                # each type variable and its class

[[block.field]]
name  = "TYPE"
kind  = "type"
types = ["int", "double", "bool", "char", "std::string", "auto"]  # the menu's first choices
type  = "T"                            # binds T; also takes every other type in T's class

[[block.field]]
name = "NAME"
kind = "symbol_decl"
type = "T"                             # declares the variable with T

[[block.input]]
name  = "VALUE"
check = "any"
type  = "T"                            # converts to T
```

* **Tables and keys.** `[[block]]` has `id`, `version`, `category`, `shape`
  (`hat`, `definition`, `statement`, `reporter`, `predicate`), `label`,
  `lowering`, `headers`, `help`, `output` and `where`. `[[block.field]]`
  declares fields (`dropdown`, `checkbox`, `text`, `number`, `type`,
  `symbol_decl`, `symbol_ref`, `header`), with a `type` pattern on `type`,
  `symbol_ref` and `symbol_decl` fields, `members` on a `symbol_ref`, and
  per-option `headers` on a dropdown, and `repeat` on a `dropdown`,
  `checkbox`, `type` or `symbol_decl` field (parts `ERROR0`, `ERROR1`, …,
  each with the field's default); `[[block.input]]` value inputs (a
  `check` type class, `optional`, `repeat`, `default` tokens, a `type`
  pattern and `assign`); `[[block.statement]]` statement inputs (`repeat`,
  and `when` for an input present only while an `extra` flag is set); and
  `[[block.extra]]` the mutator state (`count`, `flag`, or the row kinds
  `params`, `fields` and `values`: a `params` or `fields` row `{sym, name,
  type}` takes its `types` and its `type_class`, a `values` row is `{sym,
  name}`, each row declares its symbol, and an extra of a row kind has that
  kind as its name). A `repeat` counts an `extra` count, or the rows of a
  row extra (`repeat = { count = "params" }`: one part per row). A type
  field whose class no listed type fits may list none (`types = []`): its
  menu then holds only the module's own types, it has no default, and it
  may be absent from a block (the analyser reports the missing choice,
  `B2C-E0430`). Unknown keys are errors.
* **Signatures** ([ADR-0017](../adr/0017-open-type-representation.md),
  [ADR-0018](../adr/0018-slot-typing.md)). `where` names each type variable
  (a capital letter and an optional digit: `T`, `K`, `C`, `T2`) and its
  class. A pattern is `any` or `number` (M2's meanings), a variable, a type
  in C++ syntax (`int`, `std::string`, `std::vector<T>`), a type function of
  a pattern (`elem(C)`, what one item is; `index(C)`, what *item* takes;
  `key(C)`, what *contains* looks for), `field:NAME` (the type chosen in
  that field), `symbol` or `symbol:NAME` (the type of the chosen symbol),
  `param:NAME` (a call's argument: the matching parameter of the function in
  that field), `member:NAME` (the matching field of the struct in that
  field) or `result` (what a `return` gives back where it is); at most 128
  bytes and 8 levels. A block is typed in one pass: each variable is bound
  by the first part, in catalog order (a repeated input part by part), whose
  type is known. `param:` and `member:` may name the block's own declaration
  field on an input repeated by its `params` or `fields` rows (a default
  value): part *i* takes the type of row *i*. A repeated `symbol_decl` field
  may have the pattern `field:F`, `F` a repeated type field with the same
  count, and its part *i* takes the type chosen in `F`'s part *i* (*try*'s
  error variables). `auto`, an empty part and a value typed from its place
  bind nothing, and an unknown or wrong part binds the variable to an error,
  so nothing that depends on it is checked. A block whose output is a
  variable that no part binds takes its type from its **place**: the type
  its input expects (`B2C-E0315` where the place gives none). Each part is
  then checked against its resolved pattern: an input by the conversion rule
  (with `assign = true`, by assignment, so a `char` also fits a
  `std::string`, as `s = 'a'` does in C++) and by its `check` class, a
  reference by its exact type, a type field by its class; a pattern that
  does not resolve accepts what its class accepts. A type field takes the
  types of its list and every other type its pattern allows (`create` and a
  function's parameters take every type a variable can hold), and an
  `assign` input or a `members` field never binds.
* **How `check` accepts a value.** In `where`, a class is membership. In an
  input's `check`, `bool`, `integer` and `number` keep their M2 meaning,
  *converts without being invalid*, so a decimal count is only the warning
  `W0518`; `any` accepts every type but `void`; every other class is
  membership. A value outside the input's class is `B2C-E0301`. A block the
  catalog kept without a check (unknown, from a newer catalog, too old to
  upgrade) checks nothing at its inputs, and its value has an unknown type.
* **Checked when the catalog loads.** Each variable is declared once with a
  known class and used; every variable of `output` is bound by a part that
  must be filled, unless the block takes its type from its place; each
  pattern parses, names known types with the right number of arguments and
  sits on a part that may have it; `members` names a struct or enum
  variable; an input whose pattern is not one fixed type has no `default`
  (the editor gives it a start value instead); a predicate's output is
  `bool`; and every header is in the header table. A broken definition is
  `B2C-E0620`. The catalog then builds each block's signature, which the
  analyser and the editor read
  ([06 §6.3](06-compiler-pipeline.md#63-stage--resolve-catalog)).
* **Headers.** A block's `headers` lists the standard headers every form of
  it needs, and a dropdown's `headers` adds those of a chosen option; the
  forms the generator chooses bring their own
  ([06 §6.10](06-compiler-pipeline.md#610-includes-and-support-helpers)). A
  `header` field takes a header name. Every header comes from the closed
  table of the C++ standard library's headers, and every catalog header is
  emitted.
* **Label templates.** In both labels, `%NAME` marks where the field or input
  `NAME` goes, and a bare `…` marks where a repeated group continues: the
  repeated inputs (`ITEM0`, `ITEM1`, …) or the repeated parts of a block such
  as the *else if* branches of `control.if`. The words between a group's
  last repeated part and `…` are shown before each later copy, as *else if*
  is (*switch*'s *case*, *try*'s *catch*); the words before the group are
  shown once. Statement inputs that the label does not mention are placed
  after it, in catalog order. The blocks with a variable number of parts get
  ⊕/⊖ mutators from `blockly-ext`, keyed by block ID: *items* (`io.print`,
  `text.join`, `logic.operation`, `coll.list_of`, `file.write`), *branches*
  (`control.if`, `control.switch`, `error.try`, and with a flag and no
  repeated group `file.for_each_line` and `file.open`), *call arguments*
  (`func.call`, `func.call_stmt`, and `type.new`, whose parts its struct's
  fields name) and *rows* (`func.define`'s parameters, `type.struct`'s
  fields, `type.enum`'s values); their text still comes from the catalog
  label. The editor hides a field named `SHARED`, and the label's words
  before it, until wave W3 shows it in projects with several modules.
* **Messages.** Catalog messages use a block's short name: its friendly
  label up to the first field or input (*print*, *repeat*, *when program
  starts*). A message about a block whose label starts with a field or input
  says *this block*. Messages never show a block's catalog ID, except for an
  unknown block type (`E0601`) and a broken catalog definition (`E0620`, a
  bug report). Block paths in Problems name blocks differently, with the
  current dropdown text filled in and a function's name for its definition
  ([04 §4.4](04-user-interface.md#44-diagnostics-ux)). An optional `short`
  key for a shorter name is left for the localisation work of M5.
* **Output types.** `output` is required for `reporter` and `predicate`
  shapes and forbidden for the others. It is a pattern: M2's values are
  `any`, `bool`, `int`, `double`, `number`, `char`, `std::string`, `symbol`
  and `field:<NAME>`, and text is spelled `std::string` here as everywhere.
  `symbol` means the type of the referenced symbol (`var.get`: the variable's
  type; `func.call`: the function's return type), which the editor takes from
  the live analysis ([06 §6.5](06-compiler-pipeline.md#65-stage--names-and-scopes)).
  `field:<NAME>` means the type chosen in field `NAME` (`math.convert`'s
  `TO`). The editor's connection checker refuses a connection only when the
  value certainly cannot fit ([§3.3](#33-block-shapes)); unknown and
  erroneous types always connect, and the analyser reports the problem. The
  editor applies the same rules through a TypeScript copy held to the
  analyser by two committed fixtures
  ([09 §9.2](09-quality-and-delivery.md#92-testing-strategy)).
* **Toolbox.** `catalog/toolbox.toml` lists the toolbox categories in the
  order of §3.7 (in M2: Program, Variables, Math, Logic, Text, Control, Loops,
  Input / Output and Functions; in M3 also Collections, Types, Files and
  Errors). A category without entries is valid, and the editor does not show
  it. Each category has a name, an icon, a colour
  token (its ID; the colour values live in the editor theme) and, for
  *Variables* and *Functions*, a dynamic kind whose entries the editor fills
  in ([04 §4.2](04-user-interface.md#42-toolbox)). Its ordered entries name a
  block, with an optional label and an optional *preset* of field values,
  `extra` values and input tokens: for example `var.declare` as an `int`
  starting at `0`, and `io.ask` with the prompt `"Your answer: "`. A block
  has at most one entry (§3.1). `b2c-catalog` validates the toolbox: every
  entry names an existing block, no block has two entries, no entry label
  equals a category name (the editor's tests check the same for its dynamic
  headings), every catalog block is reachable through an entry or a dynamic
  category (`var.get`, `var.set`, `var.change` and `var.flip` through Variables;
  `func.call` and `func.call_stmt` through Functions), and every
  preset value fits its field, `extra` or input. It also checks each block's
  friendly label: every field and non-repeated value input appears in it,
  and a checkbox is next to the words naming it.
* **Generation.** A `b2c-catalog` test exports the validated catalog and
  toolbox as `packages/catalog-gen/catalog.json`. The build-time package
  `catalog-gen` reads only that JSON (it never parses TOML, so all validation
  stays in Rust) and generates the typed Blockly definitions
  (`packages/blockly-ext/src/generated/catalog.ts`, with each friendly label
  pre-parsed into text, argument and repeat parts) and the block reference in
  `docs/reference/blocks/`. Both outputs and the JSON are committed, and CI
  fails when any of them is stale. The JSON has a `types` section (the
  primitives and library types, with their arity, label and header), and
  each block its signature keys: `typeVars` (`where`), `output` as the
  pattern's text, `order` (the parts with a pattern, in catalog order),
  `placeVar`, `members`, `headers` and `optionHeaders`; each part's `type`
  as tagged JSON (`{"kind": "var", "name": "T"}`), an input's `assign` and a
  row's `typeClass`. Each key is left out while empty. Which fields are
  references or declarations and which inputs repeat, the editor reads from
  the definitions themselves.

### 3.11.2 Template lowering (library blocks)

Library blocks lower through **typed templates** instead of hand-written Rust:

```toml
# catalog/std/algorithm.toml
[[block]]
id       = "std.algorithm.reverse"
version  = 1
category = "collections"
shape    = "statement"
label    = { friendly = "reverse %list", cpp = "std::ranges::reverse(%list)" }
headers  = ["<algorithm>"]
requires = { standard = "c++20" }
template = "std::ranges::reverse({list});"

[[block.input]]
name  = "list"
kind  = "value"
type  = "R"                # with where = { R = "iterable" } on the block
mode  = "lvalue"           # must be a variable/member path, see rule 2 below
```

Template rules, enforced when a pack is loaded and again at expansion:

1. **Holes** `{name}` are filled with the **emitted C++ of an
   already-validated sub-expression**, never with raw field text. The hole is
   parenthesised unless the substituted expression is primary (a name,
   literal, call or member access).
2. **Evaluate-once.** A hole used more than once must have `mode = "lvalue"`
   (accepting only side-effect-free variable/member paths). Otherwise the
   expander binds it first: `{ auto&& b2c_tmp = (expr); … }`.
3. **Template text is tokenised by a C++ lexer at pack load.** Packs are
   rejected for preprocessor directives, unbalanced brackets, `;` in
   expression templates, string literals containing unescaped holes, or
   comment tokens.
4. **Types and methods.** Packs may declare types with methods, fields and
   generic parameters. This metadata powers the type picker, the generic
   member block and the analyser.
5. **Link requirements.** A pack may *request* link libraries by name (e.g.
   `sfml-graphics`). Names are validated (`[A-Za-z0-9_+.-]{1,64}`) and are
   satisfied by a machine-local **library profile** that provides include and
   library directories ([07 §7.4.4](07-toolchain-build-run.md#744-libraries-and-library-profiles)).
   Packs never supply raw compiler flags.

The standard-library blocks are themselves packaged as the built-in `std`
pack, so the mechanism is exercised on every build of our own code.
User-installed packs are **code** and are subject to workspace trust
([08 §8.3](08-security.md#83-workspace-trust-and-restricted-mode)).

### 3.11.3 Block versioning

Block IDs are stable and never reused. **Compatible** changes (a field,
`extra` key or input with a default; a dropdown option; label, help or
colour text) keep `version` and raise `generator.catalog`'s minor version.
A new type, a type field or parameter row that takes more types, and a
new header in the table are compatible changes too. The catalog is 1.1.0
since M2's labels name every field, 1.2.0 since *change* (`var.change`) has
its operator menu `OP`, and 1.3.0 since M3's type signatures, header table
and wider type fields (every catalog change up to the M3 release ships as
1.3.0). A file that leaves
the new key out loads with the default, and the editor keeps leaving it out
while the block keeps that default; blocks a newer editor creates write it.
When a document or clipboard payload names a `generator.catalog` newer than
this app's, a block with an unknown field, input, `extra` key or option, or
a type or header this app does not know or does not allow there, is
reported as made by a newer catalog (`B2C-E0602`, not `E0605`, `E0607` or
`E0614`), shown as a placeholder and kept unchanged on save.

**Breaking** changes raise `version` and ship a migration (a pure BDM → BDM
function). **Merging or retiring a block type** ships a *replacement* from
the old type and version to its successor that keeps the block's ID,
position, comment, flags, stack and nested blocks, and lists the old ID as
retired ([ADR-0015](../adr/0015-block-upgrades-on-load.md)). Before 1.0 an
exact replacement takes the place of a deprecation period. Migrations and
replacements run when a document is loaded, before the editor or compiler
see it ([05 §5.7](05-project-format.md#57-versioning-and-migration)), so the
editor never shows an upgradable block as a placeholder. Each has a
golden-file test.

Retired so far: *update* (`var.update` v1, `[score ▾] [*= ▾] (2)`), which
catalog 1.2.0 merged into *change*. Its replacement makes it the *change*
block with the same operator chosen in the menu, its value as the amount
(`VALUE` becomes `BY`), and the same C++.

## 3.12 C++ coverage matrix

| C++ feature | Support in 1.0 | How |
| ------------- | ---------------- | ----- |
| Fundamental types, literals, `auto`, `const`, `constexpr` | ✅ Native | Type picker, declaration blocks |
| All operators | ✅ Native | Operator blocks + expression slots |
| `if`/`switch`/`while`/`do`/`for`/range-`for`/`break`/`continue`/`return` | ✅ Native | Control and Loops |
| `goto`, labels | ⚙ Raw | Deliberately not a block |
| Functions: overloads, defaults, references, recursion | ✅ Native | Functions |
| Lambdas, captures, `std::function` | ✅ Native | Lambda blocks, type picker |
| Structs, classes, access control, constructors, destructors, initialiser lists | ✅ Native | Types |
| Inheritance (incl. multiple), `virtual`/`override`/`final`/abstract | ✅ Native | Class `inherits ⊕`, method `⚙` |
| Operator overloading, `friend`, static members | ✅ Native | Class members, `⚙` |
| Enums (scoped + unscoped), type aliases, namespaces | ✅ Native | Types, Organisation |
| Templates (function/class), concepts as constraints | ✅ Native (simplified) | Generics |
| Variadic templates, specialisation, SFINAE, CRTP | ⚙ Raw | Raw declarations |
| Pointers, references, smart pointers, move semantics | ✅ Native | Memory |
| Raw `new`/`delete` | ✅ Native (opt-in) | Behind *manual memory* setting |
| C arrays, unions, bit-fields | ◐ Partial / ⚙ Raw | C arrays via type picker (Adv); unions and bit-fields in Raw |
| `std::string`, `string_view`, formatting, `<iomanip>` | ✅ Native + pack | Text, I/O |
| Containers (all standard sequence/associative/adaptors), `pair`/`tuple`/`optional`/`variant` | ✅ Native + pack | Collections, type picker |
| `<algorithm>`, `<numeric>`, ranges | ✅ Pack | Library blocks + generic member/function blocks |
| Streams, files, `<filesystem>` | ✅ Native + pack | I/O, Files |
| Exceptions | ✅ Native | Errors |
| Threads, mutexes, atomics, `async`/`future`, condition variables | ✅ Native subset + pack | Concurrency |
| `<chrono>`, `<random>` | ✅ Native + pack | Time & Random |
| Multiple files / headers | ✅ Native | Modules |
| Command-line arguments, exit codes | ✅ Native | Program |
| Preprocessor macros, conditional compilation | ⚙ Raw | `#include` is managed natively |
| C++20 modules (`import`) | ❌ Not in 1.0 | GCC module support is still maturing; headers are used instead |
| Coroutines, inline assembly | ⚙ Raw | |
| Third-party libraries (SFML, raylib, …) | ✅ Library packs + library profiles | [§3.11](#311-the-catalog-and-library-packs), [07 §7.4.4](07-toolchain-build-run.md#744-libraries-and-library-profiles) |
| Calling existing C/C++ code | ✅ *Declare external function/type* blocks | The signature becomes a callable block, and the code is added via a library profile or Raw declarations |

## 3.13 Worked examples

### 3.13.1 Guessing game (input, random, loops, branching)

```text
when program starts
│ create [int] [secret] = (random integer from (1) to (100))
│ create [int] [guess] = (0)
│ print ("Guess a number from 1 to 100!")
│ repeat until <guess == secret>                     ← typed into the slot
│ │ ask ("Your guess: ") and save answer in [guess]
│ │ if <guess < secret> then
│ │ │ print ("Too low!")
│ │ else if <guess > secret>
│ │ │ print ("Too high!")
│ │ else
│ │ │ print ("Correct!")
```

That is 11 blocks. The equivalent Scratch-style program, with one block per
token, needs about 30.

```cpp
// Generated by Blocks2Cpp from project "Guessing Game", module "main".
#include <iostream>
#include <random>
#include <sstream>
#include <string>

// ---- Blocks2Cpp support helpers (only those used) --------------------
namespace b2c {
/// Returns a uniformly distributed integer in [low, high].
inline int random_int(int low, int high) { /* … */ }
/// Prints `prompt` and reads one line holding a T, asking again until it is valid.
template <typename T> T ask(const std::string& prompt) { /* … */ }
}  // namespace b2c
// -------------------------------------------------------------------------

int main() {
    int secret = b2c::random_int(1, 100);
    int guess = 0;
    std::cout << "Guess a number from 1 to 100!" << '\n';
    while (guess != secret) {
        guess = b2c::ask<int>("Your guess: ");
        if (guess < secret) {
            std::cout << "Too low!" << '\n';
        } else if (guess > secret) {
            std::cout << "Too high!" << '\n';
        } else {
            std::cout << "Correct!" << '\n';
        }
    }
    return 0;
}
```

### 3.13.2 Structs, lists, sorting with a lambda

```text
struct [Student] fields: [string] name, [int] score

when program starts
│ create [list of Student] [students] = (list of (new Student with name:("Ada") score:(93))
│                                                 (new Student with name:("Grace") score:(88)))
│ sort [students] by (function of ([const Student&] a) ([const Student&] b) giving (a.score > b.score))
│ for each [s] in (students) [read-only]
│ │ print (s.name) (": ") (s.score)
```

```cpp
#include <algorithm>
#include <iostream>
#include <string>
#include <vector>

struct Student {
    std::string name;
    int score = 0;
};

int main() {
    std::vector<Student> students = {
        Student{.name = "Ada", .score = 93},
        Student{.name = "Grace", .score = 88},
    };
    std::ranges::sort(students, [](const Student& a, const Student& b) { return a.score > b.score; });
    for (const auto& s : students) {
        std::cout << s.name << ": " << s.score << '\n';
    }
    return 0;
}
```

**In M3** this example cannot be built yet: *sort* has no comparator until
lambdas arrive in M4, so W2's *sort* takes a list of comparable items,
smallest or largest first. The `student_records` example
([`examples/student_records.b2c`](../../examples/student_records.b2c))
sorts its students by mark with a function of its own instead, with a
*struct* whose fields start empty (`int mark{};`), *new … with*, *… of* and
*for each*.

### 3.13.3 Classes, inheritance, polymorphism

```text
class [Shape]
│ method [public] [area] returns [double] ⚙{const, abstract}
│ method [public] [describe] returns [string] ⚙{const}  { return (join ("Area: ") (area())) }

class [Circle] inherits [public] [Shape]
│ field [private] [double] [radius]
│ when created with ([double] r) ⚙{explicit}  set [radius] to (r)
│ method [public] [area] returns [double] ⚙{const, override}  { return (3.14159 * radius * radius) }

when program starts
│ create [list of unique_ptr to Shape] [shapes] = (list of)
│ add (make [unique] [Circle] with (2.0)) to [shapes]
│ for each [shape] in (shapes) [read-only]
│ │ print (shape.describe())                          ← `.` becomes `->` automatically
```

```cpp
#include <iostream>
#include <memory>
#include <string>
#include <vector>

class Shape {
public:
    virtual ~Shape() = default;  // added automatically: class has virtual methods
    virtual double area() const = 0;
    std::string describe() const {
        return "Area: " + std::to_string(area());
    }
};

class Circle : public Shape {
public:
    explicit Circle(double r) : radius(r) {}
    double area() const override {
        return 3.14159 * radius * radius;
    }

private:
    double radius;
};

int main() {
    std::vector<std::unique_ptr<Shape>> shapes = {};
    shapes.push_back(std::make_unique<Circle>(2.0));
    for (const auto& shape : shapes) {
        std::cout << shape->describe() << '\n';
    }
    return 0;
}
```

## 3.14 Quick Insert (type-to-block)

`Ctrl+Space`, or typing while the canvas has focus, opens a command-style
popup at the keyboard cursor (or at the mouse position). It offers:

1. **Fuzzy search** over block labels (both display modes), synonyms and C++
   keywords. For example, `cout` finds *print*, `vector` finds the list
   blocks, and `endl` finds *print* with *new line*.
2. **Statement parsing** of common one-liners, inserted as proper blocks:

   | Typed | Inserted block |
   | ------- | ---------------- |
   | `int x = 5` | `create [int] [x] = (5)` |
   | `x = y + 1` / `x += 2` / `x++` | `set` / `change` / `change by 1` |
   | `print "hi", x` or `cout << "hi" << x` | `print ("hi") (x)` |
   | `if x > 3` / `while running` / `repeat 10` | C-block with the condition filled in, cursor inside |
   | `for i = 0 to 10` / `for (int i = 0; i < n; ++i)` | `for [i] from (0) to (10)` |
   | `for (auto& p : players)` / `for p in players` | `for each [p] in (players) [editable]` |
   | `return x` / `break` / `continue` | corresponding block |
   | `greet("Ada", 3)` / `v.push_back(3)` | call / member-call block |

3. If parsing fails, the popup explains why and offers *Insert as Raw C++*
   (explicit, never automatic).

Quick Insert uses the same Rust parser as expression slots, so its behaviour
and error messages match exactly.
