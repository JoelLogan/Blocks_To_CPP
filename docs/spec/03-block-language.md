# 3. The Block Language

> Status: **Draft v0.1** · This chapter defines what users can build and how each block maps to C++.

## 3.1 Design philosophy: few blocks, much power

Scratch makes users build `a * b + 3` from three nested operator blocks and
three value blocks. That is fine for 10-line programs and painful for real
ones. Blocks2Cpp keeps the snap-together model for **structure** (programs,
functions, classes, control flow, statements) and offers denser ways to fill
in **details**:

| Technique | Example | Saves |
|-----------|---------|-------|
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

## 3.2 Notation used in this document

| Notation | Meaning |
|----------|---------|
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
|-------|--------------|------|----------|
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

**Expand / collapse.** Right-click → *Expand into blocks* converts the parsed
expression into nested operator/value blocks. *Collapse to text* does the
reverse whenever the block subtree is expressible in slot syntax. This lets
teachers show structure and lets experts type quickly.

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
|----------|-----|
| `list of int` | `std::vector<int>` |
| `map from string to number` | `std::map<std::string, double>` |
| `print (x)` | `std::cout << (x)` |
| `add (x) to [v]` | `v.push_back(x)` |
| `read-only reference to Player` | `const Player&` |

### 3.5.3 Type checking model (gradual)

The analyser implements a **gradual** type system that approximates C++:

* **Known types** (fundamental types, library-pack types with metadata, user
  types) are checked: implicit numeric conversions, derived-to-base, `const`
  correctness, simplified overload resolution, and unification of generic
  parameters (`std::vector<T>` with `T = Student`).
* **Opaque types** (from Raw C++ or `auto` deduced from opaque expressions) are
  compatible with everything. Checking is deferred to g++, whose errors are
  still mapped back to blocks.
* **Errors are reported only when certain.** A false positive blocks the user,
  so ambiguity becomes a warning or is deferred to g++.
* **Narrowing** (e.g. `double` → `int`) is allowed with a warning and a quick
  fix ("convert explicitly"), mirroring `-Wconversion` in friendly language.

## 3.6 Symbols and scoping

* **Declarations own symbols.** A `create variable` block, function parameter,
  `for` loop variable, `catch` variable, field or function definition creates a
  symbol with a stable ID (`sym_…`).
* **References point to IDs.** Getter blocks and expression tokens reference
  `sym_…` IDs. Renaming is safe and instant. Deleting a declaration turns its
  references into errors ("`score` no longer exists") with a quick fix to
  re-create it.
* **Scope follows C++.** A variable is visible from its declaration to the end
  of the enclosing statement list, including nested blocks. Symbol dropdowns
  list **only** the symbols in scope at that block. Dragging a block out of
  scope marks its references as errors in place.
* **Unlike Scratch, there are no implicit globals.** *Make a variable* in the
  Variables category inserts a declaration at the keyboard cursor (or at the
  top of the selected function). Globals require an explicit top-level
  `global variable` block, and the analyser suggests `const` / `constexpr`
  where possible.
* **Identifiers are validated** ([08 §8.4.1](08-security.md#841-identifiers)):
  ASCII `[A-Za-z][A-Za-z0-9_]{0,63}`, not a keyword, not reserved, and not a
  standard macro name. Duplicate names in one scope are errors with a rename
  quick fix. Shadowing an outer name is a warning.

## 3.7 Block catalog by category

Each category has a colour **and** an icon (so colour is never the only cue),
and the colours are validated for colour-blind distinguishability and contrast
in light, dark and high-contrast themes. Categories marked *(Adv)* are hidden
until **View → Show advanced blocks** is enabled, which keeps the beginner
toolbox small.

| Category | Icon | Contents |
|----------|------|----------|
| Program | ▶ | `main`, command-line arguments, exit |
| Variables | 𝑥 | declare, set, change, getters, constants |
| Math | ∑ | arithmetic, comparison, math functions, random, conversions |
| Logic | ◇ | and/or/not, true/false, conditional value |
| Text | “ ” | strings, chars, formatting |
| Control | ⑂ | if/else-if/else, switch, wait, stop |
| Loops | ↻ | repeat, while/until, for range, for each, forever, break/continue |
| Input / Output | ⌨ | print, ask, read, formatting manipulators |
| Functions | ƒ | define, return, lambda, *My Blocks* (calls, auto-generated) |
| Collections | ☰ | list/map/set/queue/stack… polymorphic blocks |
| Types | ◆ | struct, class, enum, alias, members, object creation |
| Files | 🗎 | streams, line iteration, filesystem |
| Errors | ⚠ | try/catch, throw, error message |
| Time & Random | ⏱ | clock, stopwatch, sleep, random helpers |
| Memory *(Adv)* | ➚ | pointers, references, smart pointers, move |
| Concurrency *(Adv)* | ⇉ | threads, locks, atomics, async |
| Generics *(Adv)* | ⟨T⟩ | template parameters, constraints |
| Organisation | ▣ | namespaces, includes, `using`, `static_assert`, comments |
| Raw C++ *(Adv)* | `C++` | raw statement / expression / top-level |
| Libraries | 📦 | blocks from library packs, grouped by pack |

The tables below show representative blocks. The complete, normative list is
the catalog itself ([§3.11](#311-the-catalog-and-library-packs)), from which
the reference documentation in `docs/reference/blocks/` is **generated**.

### 3.7.1 Program

| Block | Generates |
|-------|-----------|
| `when program starts { … }` | `int main() { … return 0; }` (implicit `return 0`) |
| `when program starts with arguments [args] { … }` | `int main(int argc, char* argv[]) { const std::vector<std::string> args(argv, argv + argc); … }` |
| `stop program with exit code (0)` | `return 0;` inside `main`, otherwise `std::exit(0);` |

A project has exactly one `main`; a second one is an analyser error.

### 3.7.2 Variables

| Block | Generates |
|-------|-----------|
| `create [int ▾] [score] = (0) ⚙` | `int score = 0;` (`⚙`: `const`, `constexpr`, `static`) |
| `create [auto ▾] [a], [b] ⊕ from (pairValue)` | `auto [a, b] = pairValue;` (structured binding) |
| `set [score ▾] to (expr)` | `score = expr;` |
| `change [score ▾] by (1)` | `score += 1;` (and `++score;` when the value is the literal 1) |
| `[score ▾] [*= ▾] (2)` | `score *= 2;` (all compound operators) |
| `(score)` | `score` |
| `global [constant ▾] [double] [kGravity] = (9.81)` *(top level)* | `constexpr double kGravity = 9.81;` |

### 3.7.3 Math and Logic

| Block | Generates | Notes |
|-------|-----------|-------|
| `(a) [+ ▾] (b)` | `a + b` | `+ − × ÷ mod` → `+ - * / %`. Parentheses inserted only when precedence requires. |
| `(a) [< ▾] (b)` | `a < b` | |
| `<a> [and ▾] <b> ⊕` | `a && b && c` | Variadic |
| `not <a>` | `!a` | |
| `[sqrt ▾] of (x)` | `std::sqrt(x)` | abs, floor, ceil, round, sqrt, cbrt, pow, exp, log, log10, sin, cos, tan, asin, acos, atan, atan2, hypot, min, max, clamp |
| `if <c> then (a) else (b)` | `c ? a : b` | |
| `random integer from (1) to (6)` | `b2c::random_int(1, 6)` | Support helper ([§3.9](#39-support-helpers)) |
| `(x) as [int ▾]` | `static_cast<int>(x)` | |
| `text (s) as number [int ▾]` | `std::stoi(s)` | Throws on bad input. The tooltip explains this and the Errors category shows how to catch it. |
| `(x) as text` | `std::to_string(x)` | |

**Analyser lints in this area:** integer division assigned to a floating-point
variable ("did you mean `7.0 / 2`?"), `==` on floating-point values, signed vs
unsigned comparison, and possible overflow of a literal into its target type
(which is an error).

### 3.7.4 Text

| Block | Generates |
|-------|-----------|
| `"hello"` | `"hello"` (escaped per [08 §8.4.2](08-security.md#842-string-and-character-literals)) |
| `join (a) (b) ⊕` | Idiomatic per part: `std::string(a) + b`, `+ std::to_string(n)`, `+ std::string(1, c)`; `std::format` when the *formatting style* setting is `format` and the toolchain supports it |
| `format ("{} scored {}") with (name) (score) ⊕` | `std::format("{} scored {}", name, score)` (C++20 with GCC 13+; otherwise an `std::ostringstream` helper) |
| `length of (s)` | `s.size()` |
| `letter (i) of (s)` | `s.at(i)` |
| `part of (s) from (i) length (n)` | `s.substr(i, n)` |
| `(s) [contains ▾] (t)` | `s.find(t) != std::string::npos` (or `s.contains(t)` under C++23); also starts with / ends with |
| `[uppercase ▾] (s)` / `trim (s)` / `split (s) by (",")` | Support helpers |
| `replace all (a) with (b) in (s)` | Support helper |

### 3.7.5 Control and Loops

| Block | Generates |
|-------|-----------|
| `if <c> then { } ⊕else if ⊕else` | `if (c) { } else if (d) { } else { }` |
| `switch on (v) case [1] { } ⊕ default { }` | `switch (v) { case 1: { … break; } default: { … } }`. `break` is automatic unless *fall through ☑*. Case labels must be constants or enum values. |
| `repeat (10) times { }` | `for (int i = 0; i < 10; ++i) { }` (a hidden counter with a unique, readable name). A count that could change while the loop runs (it calls a function, or reads a variable the body may change) is evaluated once: `for (int i = 0, n = b2c::random_int(1, 6); i < n; ++i)`. The same applies to a `for` loop's end and step. |
| `repeat while <c> { }` / `repeat until <c> { }` | `while (c) { }` / `while (!(c)) { }` (simplified to the inverse operator where that is exact, e.g. `!(a == b)` → `a != b`; ordering comparisons on decimal numbers are not inverted, because `!(x < y)` and `x >= y` differ when a value is NaN) |
| `do { } while <c>` | `do { } while (c);` |
| `for [i] from (1) [to ▾] (10) [step (1)]` | `to` → `i < 10`, `through` → `i <= 10`, `down to` → `i >= 10` with `i -= step`. The direction is explicit, so a runtime step sign is never needed. `step` is hidden until expanded. |
| `for each [item] in (list) [read-only ▾]` | `for (const auto& item : list)`; *copy* → `auto item`, *editable* → `auto& item` |
| `for each [key], [value] in (map)` | `for (const auto& [key, value] : map)` |
| `forever { }` | `while (true) { }` (info lint if no `break`, `return` or exit inside) |
| `break` / `continue` / `return (x)` | Validated by the analyser: only inside loops, and the `return` type is checked |
| `wait (0.5) seconds` | `std::this_thread::sleep_for(std::chrono::duration<double>(0.5));` |

### 3.7.6 Input / Output

| Block | Generates | Notes |
|-------|-----------|-------|
| `print (a) (b) ⊕ [no separator ▾] ☑ new line` | `std::cout << a << b << '\n';` | `bool` prints as `true`/`false` (via `(b ? "true" : "false")`); 8-bit integers print as numbers (unary `+`); separators can be spaces or commas; `print to error stream` uses `std::cerr` |
| `ask ("Name? ") and save answer in [name ▾]` | string: `std::cout << "Name? "; std::getline(std::cin >> std::ws, name);` | `std::ws` avoids the classic "getline after `>>`" bug |
| (same, numeric target) `[keep asking until valid ▾]` | `age = b2c::ask<int>("Age? ");` | The helper asks again after invalid input, with a short message such as *"Please enter a whole number."* At end of input (e.g. stdin from a file) it prints *"Input ended"* and exits with code 1 instead of looping forever. `simple` mode emits `std::cin >> age;`. |
| `read [word ▾] into [w]` | `std::cin >> w;` / `std::getline(std::cin, w);` | |
| `show numbers with (2) decimal places` | `std::cout << std::fixed << std::setprecision(2);` | |

### 3.7.7 Functions

| Block | Generates |
|-------|-----------|
| `define [greet] with ([string] name [read-only ▾]) ([int] times = (1)) ⊕ returns [nothing ▾] { }` | `void greet(const std::string& name, int times = 1) { }`. Parameter pass modes: *copy*, *editable* (`&`), *read-only* (`const&`), *moved* (`&&`). |
| *(auto-generated, in My Blocks)* `greet (name: "Ada") (times: 3)` | `greet("Ada", 3);` A statement block when the result is `void`, a reporter otherwise. A reporter can be used as a statement via *ignore result*. |
| `return (value)` | `return value;` |
| `function ([auto] x) ⊕ capturing [nothing ▾] returns [auto] { }` | `[](auto x) { … }`. Capture options: nothing, everything by reference `[&]`, everything by copy `[=]`, or chosen variables. |
| `function of ([auto] x) giving (x * 2)` | `[](auto x) { return x * 2; }` (single-expression form) |

Overloads (same name, different parameters), default arguments and recursion
are supported. **Definition order on the canvas never matters**: the generator
emits forward declarations automatically ([06 §6.7](06-compiler-pipeline.md#67-ordering-and-declarations)).

### 3.7.8 Collections

Polymorphic phrase blocks work across container types. Lowering picks the
idiomatic operation for the static type, and the dropdowns only show what that
type supports.

| Block | vector / deque | set | map | queue / stack / priority_queue |
|-------|----------------|-----|-----|--------------------------------|
| `add (x) to [c]` | `c.push_back(x)` | `c.insert(x)` | — (use `set key`) | `c.push(x)` |
| `item (i) of [c]` | `c.at(i)` | — | `c.at(k)` | `c.front()` / `c.top()` |
| `set item (i) of [c] to (x)` | `c.at(i) = x` | — | `c[k] = x` | — |
| `remove item (i) from [c]` | `c.erase(c.begin() + i)` | `c.erase(x)` | `c.erase(k)` | `c.pop()` |
| `length of [c]` | `c.size()` | `c.size()` | `c.size()` | `c.size()` |
| `[c] contains (x)` | `std::ranges::find(c, x) != c.end()` (C++20) | `c.contains(x)` (C++20) | `c.contains(k)` | — |
| `[c] is empty` | `c.empty()` | ← | ← | ← |
| `clear [c]` | `c.clear()` | ← | ← | (helper) |
| `sort [c] [ascending ▾] ⊕ by (lambda)` | `std::ranges::sort(c)` / `std::ranges::sort(c, std::greater{})` / with comparator | — | — | — |
| `list of (a) (b) (c) ⊕` | `{a, b, c}` initialiser (type from context) | ← | `{{k, v}, …}` | — |

Indexing is bounds-checked (`.at()`) by default. A block context-menu option,
*fast unchecked access*, switches to `[]`. Indexing is always zero-based, as
in C++, and tooltips say so.

**Generic member block.** `[expr ▾] . [member ▾] (args…)` lists the fields
and methods of the expression's static type, taken from user types and
library-pack metadata. Argument slots reshape to the chosen method's
signature. Through `.` and `->` (chosen automatically for pointers and smart
pointers), this one block reaches most of the standard library without a
dedicated block for each method.

### 3.7.9 Types (structs, classes, enums)

| Block | Generates |
|-------|-----------|
| `struct [Point] fields: [double] x = (0) ⊕ ☐ printable ☐ comparable` | `struct Point { double x = 0; … };` *printable* auto-generates `operator<<`; *comparable* adds `auto operator<=>(const Point&) const = default;` (C++20) |
| `new [Point] with x: (1) y: (2)` | `Point{.x = 1, .y = 2}` (C++20 designated initialisers), or `Point{1, 2}` in C++17 mode. Slots are generated from the fields, in declaration order. |
| `class [Player] inherits [public ▾] [Entity] ⊕ { members }` | `class Player : public Entity { … };` |
| ↳ `field [private ▾] [int] [health] = (100) ⚙` | Member variable. Members are grouped into `public:` / `protected:` / `private:` sections, keeping their order within each section. |
| ↳ `method [public ▾] [takeDamage] ([int] amount) ⊕ returns [nothing ▾] ⚙ { }` | `⚙`: `const`, `static`, `virtual`, `override`, `final`, `= 0` (abstract), `noexcept`, `[[nodiscard]]` |
| ↳ `when created with ([string] name) ⊕ ⚙ set [health ▾] to (100) ⊕ { }` | Constructor with a member-initialiser list (`⚙`: `explicit`, `= default`, `= delete`) |
| ↳ `when destroyed { }` | Destructor |
| ↳ `operator [+ ▾] ([const Player&] other) returns [Player] { }` | `Player operator+(const Player& other) const { … }` |
| `this object` / `[obj ▾] . [field ▾]` | `this` / `obj.field` (or `obj->field` automatically) |
| `enum [Color] values: [Red] [Green] [Blue] ⊕` | `enum class Color { Red, Green, Blue };` plus an auto-generated `to_string(Color)` / `operator<<` when the enum is printed |
| `[Color ▾] :: [Red ▾]` | `Color::Red` |
| `type alias [Grid] = [list of list of int ▾]` | `using Grid = std::vector<std::vector<int>>;` |

**Safety defaults:** a class with any `virtual` method automatically gets
`virtual ~Class() = default;` unless it declares its own destructor (shown in
the C++ view and in the class block's `⚙`). Copy/move special members can be
`= default` / `= delete` via the class `⚙`.

### 3.7.10 Files

| Block | Generates |
|-------|-----------|
| `open file ("scores.txt") for [reading ▾] as [file]` | `std::ifstream file("scores.txt");` (`writing` → `std::ofstream`; `appending` → `std::ofstream file(path, std::ios::app)`), plus an *if it failed* mouth: `if (!file) { … }` |
| `read line from [file] into [line]` | `std::getline(file, line)` (a predicate, usable in `repeat while`) |
| `for each line [line] in file ("data.txt") { } ⊕ if it can't be opened { }` | `std::ifstream` + `while (std::getline(...))` |
| `write (a) (b) ⊕ to [file] ☑ new line` | `file << a << b << '\n';` |
| `file ("x") exists` / `delete file` / `create folder` / `for each file in folder` | `std::filesystem::exists(...)` etc. |

### 3.7.11 Errors

| Block | Generates |
|-------|-----------|
| `try { } catch [any error ▾] as [e] { } ⊕` | `try { } catch (const std::exception& e) { }`. The dropdown lists standard exception types, user exception classes and `...` (*anything*). |
| `throw [runtime error ▾] ("message")` | `throw std::runtime_error("message");` |
| `error message of [e]` | `e.what()` |
| `rethrow` | `throw;` (only valid inside a catch) |

### 3.7.12 Memory, Concurrency, Generics *(Advanced)*

| Block | Generates |
|-------|-----------|
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
|-------|-----------|
| `namespace [geometry] { definitions }` | `namespace geometry { … }` |
| `use header [<cmath> ▾]` | `#include <cmath>`. Usually unnecessary, because includes are computed automatically. The dropdown offers standard headers, library-pack headers and project modules only. |
| `use namespace [std ▾] in this file` (module top level) · `use namespace [std ▾] here` (inside a function) | `using namespace std;` for this module's `.cpp`, or from this point to the end of the statement list. The dropdown offers `std`, the standard sub-namespaces of the project's standard (`std::chrono`, `std::this_thread`, `std::filesystem`; `std::numbers`, `std::ranges`, `std::views` from C++20) and the module's top-level namespaces. Never in a header or a class or namespace block (enforced). The project setting *Textbook style* does the same for every `.cpp`. How names are then spelled, and the few that must be renamed: [06 §6.14](06-compiler-pipeline.md#614-standard-names-and-using-namespace). |
| `check at compile time <cond> ("message")` | `static_assert(cond, "message");` |
| Block comment (Blockly comment bubble) | `// comment` lines above the statement (sanitised, [08 §8.4.3](08-security.md#843-comments)) |

## 3.8 Organisation features (editor)

Clear organisation is a core requirement, not polish:

1. **Modules = tabs = files.** Each module becomes `name.cpp`, plus `name.hpp`
   when it has *shared* definitions. Using another module's shared
   definitions adds the `#include` automatically, and the toolbox's
   *My Blocks* shows each module's functions under a module heading.
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
    Inserting it remaps symbol IDs and resolves references by name in the
    target scope.

## 3.9 Support helpers

Some blocks generate a call into a tiny, readable **support library** of
helpers in the `b2c` namespace: `random_int`, `random_real`, `ask<T>`, `trim`,
`split`, `to_upper`, `replace_all`, `Stopwatch` and similar.

* Helpers are plain C++ with Doxygen comments, versioned with the catalog, and
  emitted **only if used**.
* **Placement.** Single-module projects get the helpers inline in a clearly
  delimited section at the top of `main.cpp`, so a student can submit one
  file. Multi-module projects get them in `b2c_support.hpp`.
* A project setting, **Prefer plain standard C++**, swaps helpers for inline
  standard code wherever a reasonable inline form exists (e.g. `std::cin >>
  x;` for `ask`).
* Helpers are covered by the same golden and unit tests as the generator.

## 3.10 Raw C++ blocks

| Block | Shape | Use |
|-------|-------|-----|
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

All block types are defined declaratively in TOML under `catalog/`. A build
step (`packages/catalog-gen`) generates typed Blockly definitions and the
block reference docs, and the Rust compiler loads the same files. This gives
one source of truth for both editor and compiler.

```toml
# catalog/core/io.toml
[[block]]
id        = "io.print"              # stable, namespaced ID; never reused
version   = 1                       # bumped on breaking change; migrations required
category  = "io"
shape     = "statement"
label     = { friendly = "print %items %sep %newline", cpp = "std::cout << %items %sep %newline" }
lowering  = "builtin:io.print"      # hand-written Rust lowering for core blocks
headers   = ["<iostream>"]
help      = "io/print.md"

[[block.input]]
name    = "items"
kind    = "variadic-value"          # renders ⊕ / ⊖
min     = 1
max     = 32
type    = "printable"               # type class checked by the analyser
default = { text = "Hello, world!" }

[[block.field]]
name    = "sep"
kind    = "dropdown"
options = [["no separator", "none"], ["spaces", "space"], ["commas", "comma"]]
default = "none"

[[block.field]]
name    = "newline"
kind    = "checkbox"
default = true
```

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
name = "list"
kind = "value"
type = "range<T>"
mode = "lvalue"            # must be a variable/member path, see rule 2 below
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

Block IDs are stable and never reused. A breaking change to a block's
inputs/fields bumps `version`, and the catalog ships a migration (pure
function, BDM → BDM) from every older version. Removing a block type
requires a deprecation period of one minor release in which the block still
loads, shows a deprecation badge, and offers an automatic replacement.

## 3.12 C++ coverage matrix

| C++ feature | Support in 1.0 | How |
|-------------|----------------|-----|
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

```
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
#include <limits>
#include <random>
#include <string>

// ---- Blocks2Cpp support helpers (only those used) --------------------
namespace b2c {
/// Returns a uniformly distributed integer in [low, high].
inline int random_int(int low, int high) { /* … */ }
/// Prints `prompt` and reads a T, re-prompting until the input is valid.
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

```
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

### 3.13.3 Classes, inheritance, polymorphism

```
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
|-------|----------------|
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
