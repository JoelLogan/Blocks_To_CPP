# Analyser diagnostics

The analyser (crate `b2c-lang`) checks a project after its blocks have been
matched against the catalog. It reports problems with names, types, program
structure and flow, plus a few lints. See
[spec §6.4–6.6](../../spec/06-compiler-pipeline.md#64-stage--lowering-to-the-semantic-ast)
for the design and [§6.12](../../spec/06-compiler-pipeline.md#612-diagnostics-model)
for the diagnostics model.

* **Errors** block building and running. The analyser reports an error only
  when it is certain: when g++ would reject the generated code, or when the
  generated code would not mean what the blocks say.
* **Warnings** point at code that is legal C++ but probably a mistake.
* **Info** notes never need action.

Every diagnostic points at a block, and at the part of the block where the
problem is: a field (such as `NAME`), a value input (such as `COND0`), or a
range of pieces (tokens) inside an expression typed into a slot. The
expression with the problem gets the type *unknown*, and nothing involving
an unknown value is reported again, so one mistake gives one message.

Examples use the block notation of [spec §3.2](../../spec/03-block-language.md#32-notation-used-in-this-document):
`[x]` is a field, `(value)` a value input, `<cond>` a condition.

Codes by area:

| Codes | Area |
|-------|------|
| `B2C-E02xx` | Names and scopes |
| `B2C-E03xx` | Types and conversions |
| `B2C-E04xx` | Structure, flow and expression slots |
| `B2C-W05xx`, `B2C-I05xx`, `B2C-E0517` | Lints and literal overflow |

## B2C-E0201 Not declared

* **Severity:** error
* **Meaning:** a block or expression refers to a variable or function that is
  declared nowhere in the project. This happens when the block that created
  it was deleted, or when a project file was edited by hand.
* **Example:** the `create [int] [score]` block was deleted, but
  `print (score)` still refers to it.
* **Message:** "This refers to a variable or function that doesn't exist (any
  more). Choose another one, or create it again."
* **How to fix:** pick another variable in the block's dropdown, or create the
  variable again with a `create variable` block.

## B2C-E0202 Used before it is created

* **Severity:** error
* **Meaning:** a variable is used above the block that creates it, in the same
  group of blocks (or a group nested inside it). A variable exists only from
  its `create` block onwards. Also reported when an `auto` variable is used in
  its own starting value, because its type is not known yet.
* **Example:**

  ```text
  print (total)
  create [int] [total] = (0)
  ```

* **Message:** "`total` is used before it is created. Move this block below
  the block that creates `total`."
* **How to fix:** move the `create` block above the first block that uses the
  variable.

## B2C-E0203 Out of scope

* **Severity:** error
* **Meaning:** a symbol is used outside the part of the program where it
  exists: a variable created inside an `if` or a loop is used after it, a
  variable of `when program starts` is used in a function, a function's input
  is used outside the function, or a `for` counter is used outside its loop
  (including in the loop's own start, end or step).
* **Example:**

  ```text
  if <ready> then
  │ create [int] [bonus] = (10)
  print (bonus)
  ```

* **Message:** "`bonus` can't be used here: it only exists from the block that
  creates it to the end of that group of blocks (including the blocks nested
  inside them)."
* **How to fix:** create the variable earlier, outside the inner group of
  blocks, and only *set* it inside. To share a value with a function, pass it
  as an input.

## B2C-E0204 Created in a disabled or unattached block

* **Severity:** error
* **Meaning:** the symbol is created by a block that is disabled (or inside a
  disabled block), or by a block that is not attached to `when program
  starts` or a function. Such blocks are not part of the program.
* **Example:** `create [int] [lives]` is disabled, but `print (lives)` is not.
* **Message:** "`lives` is created in a disabled block. Enable that block, or
  choose something else."
* **How to fix:** enable the block (or attach it to the program), or stop
  using the symbol.

## B2C-E0205 Hidden by another name

* **Severity:** error
* **Meaning:** blocks refer to symbols by identity, but C++ code refers to
  them by name. Here the block refers to one symbol, while a newer symbol with
  the same name hides it, so the C++ code would use the newer one. Also
  reported when a call refers to a function that a variable with the same
  name hides.
* **Example:**

  ```text
  create [int] [x] = (1)
  if <ready> then
  │ create [int] [x] = (x + 1)      ← this `x + 1` means the outer x
  ```

  In C++, `int x = x + 1;` reads the new, uninitialised `x`.
* **Message:** "This refers to an outer `x`, but a newer variable also called
  `x` hides it here, so C++ would use the newer one. Rename one of them."
* **How to fix:** give one of the two symbols a different name.

## B2C-E0206 Function of another module

* **Severity:** error
* **Meaning:** a call refers to a function defined in another module. Sharing
  functions between modules needs headers, which arrive in a later version.
* **Message:** "`helper` is a function of another module (`utils`). Using
  functions from other modules isn't supported yet, so move it into this
  module."
* **How to fix:** move the function into the module that uses it.

## B2C-E0207 Wrong kind of symbol

* **Severity:** error
* **Meaning:** a function is used as a value without calling it, a variable is
  called like a function, or a block tries to change a function.
* **Example:** typing `area * 2` in a slot, where `area` is a function.
* **Message:** "`area` is a function, not a value. To run it and use its
  result, call it: use its call block or write area() in an expression."
* **How to fix:** call the function (`area(3, 4) * 2`), or pick a variable
  instead.

## B2C-E0210 Duplicate name

* **Severity:** error
* **Meaning:** two variables (or function inputs) with the same name exist in
  the same scope, which C++ does not allow. A function's body counts as the
  same scope as its inputs, and a `for` loop's body as the same scope as its
  counter.
* **Example:**

  ```text
  create [int] [count] = (0)
  create [int] [count] = (1)
  ```

* **Message:** "There is already a variable called `count` here. Choose
  another name."
* **How to fix:** rename one of them.

## B2C-E0211 Duplicate function

* **Severity:** error
* **Meaning:** the project defines two functions with the same name, in the
  same module or in different modules. Functions with the same name but
  different inputs (overloads) are not supported yet, and every function is
  visible to the whole program when it is linked, so two modules can't both
  define one with the same name either.
* **Example:** `define [area] with ([double] r)` and `define [area] with
  ([double] w) ([double] h)`.
* **Message:** "There is already a function called `area` in this module.
  Functions with the same name (overloads) aren't supported yet, so rename
  one of them." (or "… in module `shapes`" when the other one is there)
* **How to fix:** rename one of the functions, e.g. `circle_area` and
  `rectangle_area`.

## B2C-E0212 Duplicate symbol ID

* **Severity:** error
* **Meaning:** two blocks declare a symbol with the same internal ID. This
  only happens in a damaged project file; the first declaration is kept.
* **Example:** a project file edited by hand, where two `create variable`
  blocks both declare `"sym": "sym_score"`.
* **Message:** "This block declares a symbol that another block already
  declares (they share an ID). The project file may be damaged: delete this
  block and create it again."
* **How to fix:** delete the reported block and create it again.

## B2C-E0220 Invalid name

* **Severity:** error
* **Meaning:** a name is not a valid C++ identifier for user code
  ([spec §8.4.1](../../spec/08-security.md#841-identifiers)). A name must start
  with a letter (A–Z or a–z), contain only letters, digits and underscores,
  be at most 64 characters long, contain no `__`, and not be a C++ keyword, a
  standard macro (such as `NULL`) or a name reserved by Blocks2Cpp (`main`,
  `std`, anything starting with `b2c`). Function names must also not clash
  with C library names such as `abs` or `time`, or with the C++ standard
  library functions that read numbers from text (`stoi`, `stod`, …): a call
  such as `stoi(text)` would match both functions.
* **Example:** `create [int] [2fast]`, or `define [abs]`.
* **Message:** "`int` can't be used as a name: `int` is a C++ keyword."
* **How to fix:** choose another name.

## B2C-E0301 Wrong type of value

* **Severity:** error
* **Meaning:** a value of one type is used where another type is needed, and
  C++ cannot convert it (or would convert it to something unexpected): text
  where a number or true/false value is needed, a number or true/false value
  where text is needed, a question for `ask` that is not text, or a `convert`
  block given text. A character is not text either, except that `set` can
  give a text variable a single character (`s = 'a';` is valid C++, while a
  text variable can't *start* with a character value).
* **Example:** `create [int] [age] = ("twelve")`.
* **Message:** "`age` needs a whole number, but this is text. Text is not
  turned into a number automatically."
* **How to fix:** use a value of the right type. Use `join` to turn numbers
  into text.

## B2C-E0302 Operator used with the wrong values

* **Severity:** error
* **Meaning:** an operator does not work with these values: arithmetic or
  `mod` on text, comparing text with a number or character, `and`/`or`/`not`
  on text, or `change by` on a text variable.
* **Example:** `(name) - (1)` where `name` is text.
* **Message:** "Can't subtract text: - only works with numbers."
* **How to fix:** use values of the right type, or a block meant for text
  (such as `join`).

## B2C-E0303 Text added with +

* **Severity:** error
* **Meaning:** `+` is used with text. In C++, `"a" + "b"` does not compile and
  `text + number` does something unexpected, so Blocks2Cpp joins text with the
  `join` block instead.
* **Example:** `"Score: " + score` typed in a slot.
* **Message:** "Text can't be added with +. Use the 'join' block to put text
  and other values together."
* **How to fix:** use `join ("Score: ") (score)`.

## B2C-E0304 mod with decimal numbers

* **Severity:** error
* **Meaning:** `mod` (`%`) only works with whole numbers; C++ rejects it for
  `double`.
* **Example:** `(7.5) mod (2)`.
* **Message:** "'mod' only works with whole numbers, but this uses a decimal
  number. Use the 'convert' block to make it a whole number first."
* **How to fix:** convert the decimal number with `(x) as [int]`.

## B2C-E0305 No value

* **Severity:** error
* **Meaning:** a function that gives back nothing (`void`) is used where a
  value is needed.
* **Example:** `create [int] [x] = (greet ("Ada"))` where `greet` returns
  nothing.
* **Message:** "`greet` gives back nothing, so it can't be used as a value.
  Use the statement version of the block to just run it."
* **How to fix:** use the statement call block, or make the function return a
  value.

## B2C-E0306 Wrong number of values for a function

* **Severity:** error
* **Meaning:** a call gives a function more or fewer values than it has
  inputs. Default values for inputs are not supported yet.
* **Example:** `area (5)` typed in a slot, where `area` has the inputs
  `width` and `height`.
* **Message:** "`area` needs 2 values, but 1 is given."
* **How to fix:** add or remove values in the call block (⊕ / ⊖).

## B2C-E0307 Editable input needs a variable

* **Severity:** error
* **Meaning:** the function can change an *editable* input (`T&` in C++), so
  the call must give a variable of exactly the input's type that may be
  changed, not a value, a calculation, a constant or a loop counter.
* **Example:** `add to (score + 1) (5)` where the first input of `add to` is
  editable.
* **Message:** "The `total` input of `add_to` is editable (the function may
  change it), so it needs a variable, not a value or calculation."
* **How to fix:** pass a variable, or change the input's mode to *copy* or
  *read-only*.

## B2C-E0308 Can't be changed

* **Severity:** error
* **Meaning:** a block changes something that may not change: a constant, the
  counter of a `for` loop, or a read-only function input.
* **Example:** `set [max] to (10)` where `max` was created with *const* ticked.
* **Message:** "`max` was created as a constant, so it can't be changed.
  Untick 'const' in its 'create' block if it needs to change."
* **How to fix:** untick *const*, use another variable, or change the input's
  mode.

## B2C-E0309 auto without a starting value

* **Severity:** error
* **Meaning:** a variable of type *auto* takes its type from its starting
  value, so it needs one.
* **Example:** `create [auto] [x]`.
* **Message:** "`x` has the type 'auto', so it needs a starting value to take
  its type from. Give it a value, or choose a type."
* **How to fix:** give it a value, or choose a type such as `int`.

## B2C-E0310 Not a number

* **Severity:** error
* **Meaning:** a number block or a number in an expression is not valid C++
  number syntax. Whole numbers are written like `42`, `0x2A` or `0b101`
  (with optional `'` separators); decimal numbers like `3.14`, `.5` or `1e-3`.
  A leading `0` is not allowed, because C++ would read the number as octal.
* **Example:** `1.2.3`, `012`.
* **Message:** "`1.2.3` is not a number. Write whole numbers like 42 and
  decimal numbers like 3.14."
* **How to fix:** correct the number.

## B2C-E0311 Different kinds of values in a conditional value

* **Severity:** error
* **Meaning:** the two choices of `if … then … else` (or `?:`) have types with
  no common type, such as text and a number.
* **Example:** `if <won> then ("yes") else (0)`.
* **Message:** "Both choices must be the same kind of value, but one is text
  and the other is a whole number."
* **How to fix:** make both choices the same kind of value.

## B2C-E0312 Invalid text or character

* **Severity:** error
* **Meaning:** a text value contains the NUL character or is longer than
  64 KiB, or a character value is not exactly one plain ASCII character.
* **Example:** `letter 'é'`, `letter 'ab'`.
* **Message:** "This character can't be used: a character value must be a
  plain ASCII character; use text for 'é'."
* **How to fix:** use a text value for anything other than one ASCII
  character.

## B2C-E0401 Outside a loop

* **Severity:** error
* **Meaning:** `leave loop` (`break`) or `skip to next round` (`continue`) is
  not inside a loop. A function body is never inside the caller's loop.
* **Message:** "'leave loop' can only be used inside a loop."
* **How to fix:** move the block into a loop, or remove it.

## B2C-E0403 return needs a value

* **Severity:** error
* **Meaning:** a `return` block has no value, but the function gives back a
  value. In `when program starts`, `return` needs an exit code.
* **Example:** `return` (without a value) in `define [area] … returns
  [double]`.
* **Message:** "`area` must give back a decimal number, so this 'return' needs
  a value."
* **How to fix:** give the `return` block a value, or use `stop program with
  exit code` in `when program starts`.

## B2C-E0404 return with a value in a function that gives back nothing

* **Severity:** error
* **Meaning:** a `return` block has a value, but the function returns
  nothing.
* **Example:** `return (1)` in `define [greet] … returns [nothing]`.
* **Message:** "`greet` gives back nothing, so this 'return' can't have a
  value. Remove the value, or change what the function gives back."
* **How to fix:** remove the value, or change the function's *returns* type.

## B2C-E0405 No 'when program starts' block

* **Severity:** error
* **Meaning:** the project has no enabled `when program starts` block, so the
  program has nowhere to begin.
* **Example:** a project that only defines functions, or whose `when program
  starts` block is disabled.
* **Message:** "Add a 'when program starts' block: every program begins
  there." (or, when the block exists but is disabled: "The 'when program
  starts' block is disabled. Enable it so the program has a place to begin.")
* **How to fix:** add (or enable) a `when program starts` block.

## B2C-E0406 More than one 'when program starts' block

* **Severity:** error
* **Meaning:** a program can only begin in one place. The first block (by
  module, then by block ID) is used; the others are reported but still
  checked.
* **Example:** two `when program starts` blocks on the canvas, one of them
  left over from an experiment.
* **Message:** "There is more than one 'when program starts' block. A program
  can only begin in one place, so keep just one of them."
* **How to fix:** keep just one `when program starts` block; turn the others
  into functions.

## B2C-E0410 Missing return

* **Severity:** error
* **Meaning:** a function that gives back a value can reach its end without a
  `return` block. A path also ends at `stop program` or in a loop that never
  ends (`forever` without `leave loop`, or `repeat while <true>`).
* **Example:**

  ```text
  define [sign] with ([int] n) returns [int]
  │ if <n > 0> then
  │ │ return (1)
  ```

* **Message:** "`sign` must give back a whole number, but it can reach its end
  without a 'return' block. Add a 'return' block at the end (or on every
  path)."
* **How to fix:** add a `return` block at the end, or an `else` part that
  returns.

## B2C-E0430 Incomplete or damaged block

* **Severity:** error
* **Meaning:** a block lacks something the analyser needs: a value in a value
  input, a name, a chosen variable or function, or a valid setting (type,
  dropdown choice, checkbox, function input row). Also reported when the
  block in a value input is disabled.
* **Example:** an `if` block whose condition slot is empty, or a `set` block
  with no variable chosen.
* **Message:** "This block needs a condition." / "Choose a variable in this
  block." / "`pow` is not one of the choices for this setting."
* **How to fix:** fill in the missing part, or choose the setting again.

## B2C-E0431 Blocks nested too deeply

* **Severity:** error
* **Meaning:** blocks are nested more than 64 levels deep (statements and
  values together); blocks below that level are not analysed. Also reported
  when the values of a block expand to too many levels together with the
  blocks around them: an `and` block with 32 conditions is 31 levels of
  `&&`, so a few such blocks nested in each other are too deep to turn into
  C++, although the blocks themselves are not.
* **Example:** 70 `if` blocks, each inside the previous one; or eight `and`
  blocks with 32 conditions each, each the first condition of the next.
* **Message:** "These blocks are nested too deeply (more than 64 levels). Move
  some of them into a function." / "The values in this block are nested too
  deeply, together with the blocks around it, to be turned into C++. Split
  them up using variables, or move some of the blocks into a function."
* **How to fix:** move part of the blocks into a function, or store
  intermediate values in variables.

## B2C-E0440 Expression syntax

* **Severity:** error
* **Meaning:** text typed into a slot does not follow the expression grammar
  ([spec §3.4](../../spec/03-block-language.md#34-expression-slots)). The
  message names the problem and the diagnostic points at the pieces involved:
  a missing value or operator, an unclosed `(`, a `?` without `:`, a stray
  `,` or `:`, `( )` after something that is not a function, or an operator
  that slots do not accept. Slots only compute values: `=`, `+=` and `++`
  change variables (use `set` and `change` blocks), and bit operators,
  member access (`.`, `->`, `::`), indexing and keywords other than `true`
  and `false` are not available yet.
* **Example:** `x = 1` in a condition.
* **Message:** "`=` changes a variable, which an expression can't do. Use a
  'set' block to change a variable, or `==` to compare."
* **How to fix:** correct the expression as the message suggests.

## B2C-E0441 Unfinished expression

* **Severity:** error
* **Meaning:** a slot holds an unfinished draft, or a word that is neither a
  known variable or function nor quoted text.
* **Example:** `score + bonsu` typed in a slot, where no variable is called
  `bonsu`.
* **Message:** "`foo` isn't understood here. Use a variable or function that
  exists, put text in quotes, or finish typing the expression."
* **How to fix:** finish the expression; create the variable first if it is
  meant to be one.

## B2C-E0442 Expression too long

* **Severity:** error
* **Meaning:** a slot has more than 512 pieces (numbers, names, operators,
  brackets).
* **Example:** a formula typed or pasted into one slot that adds up 300
  numbers.
* **Message:** "This expression is too long (599 parts; the limit is 512).
  Split it up using variables."
* **How to fix:** split the expression using variables.

## B2C-E0443 Expression nested too deeply

* **Severity:** error
* **Meaning:** an expression typed in a slot has more than 64 levels of
  operators, brackets, signs, calls or conditional values inside each other.
  In a chain such as `a + b + c + …`, each operator is one level, because C++
  reads it as `((a + b) + c) + …`; so a slot can add up at most 65 values in
  one chain. The diagnostic points at the piece where the limit is passed.
* **Example:** `f(f(f(…f(1)…)))` with 65 calls, or 66 numbers joined by `+`.
* **Message:** "This expression is too complex: it has more than 64 levels of
  operators, brackets or calls inside each other (in a chain such as a + b +
  c, each operator counts as a level). Split it up using variables."
* **How to fix:** split the expression using variables, e.g. compute partial
  sums first.

## B2C-W0501 Name hides another

* **Severity:** warning
* **Meaning:** a new variable (or loop counter, or function input) has the
  same name as a variable from an enclosing block, or as a function. This is
  legal C++, but it is easy to mix the two up, and the hidden function can't
  be called where the variable exists.
* **Message:** "`x` hides another variable called `x` from an enclosing block.
  This works, but it is easy to mix them up, so consider another name."
* **How to fix:** choose a different name.

## B2C-W0502 Unreachable block

* **Severity:** warning
* **Meaning:** a block can never run, because the block before it never
  completes: it always returns, stops the program, leaves or skips the loop
  (`leave loop`, `skip to next round`), or repeats forever. Reported once,
  on the first such block.
* **Example:**

  ```text
  return (total)
  print ("done")      ← never runs
  ```

* **Message:** "This block can never run: the program never gets past the
  block before it, which always returns, stops the program, leaves or skips
  the loop, or repeats forever. Remove this block or move it."
* **How to fix:** remove the block, or move it before the block that jumps
  away.

## B2C-W0503 Used before it has a value

* **Severity:** warning
* **Meaning:** a number, character or true/false variable created without a
  starting value is read before any block gives it one, so it still has its
  default value (0 or false). Also reported when a variable is used in its own
  starting value. The check is simple and cautious: it warns only when no
  path to the use assigns the variable, and assignments anywhere in a loop
  count for the whole loop. Passing the variable to an *editable* function
  input counts as giving it a value.
* **Example:**

  ```text
  create [int] [count]
  change [count] by (1)
  ```

* **How to fix:** give the variable a starting value in its `create` block,
  or set it first.

## B2C-W0510 Whole-number division where a decimal is expected

* **Severity:** warning
* **Meaning:** two whole numbers are divided, so the result is rounded towards
  zero (7 / 2 gives 3), and then used where a decimal number is expected
  (stored in a decimal variable, mixed with decimals, or converted to a
  decimal).
* **Example:** `create [double] [average] = (total / count)` with whole-number
  `total` and `count`.
* **How to fix:** write one side as a decimal (`7.0 / 2`), or convert one side
  to a decimal number before dividing.

## B2C-W0511 Exact comparison of decimal numbers

* **Severity:** warning
* **Meaning:** `=`/`==` or `≠`/`!=` compares decimal numbers. Rounding makes
  this unreliable: 0.1 + 0.2 is not exactly 0.3.
* **Example:** `if <(price * 3) = (0.3)> then`, where `price` is a decimal
  number.
* **Message:** "Checking decimal numbers for exact equality is unreliable
  because of rounding (0.1 + 0.2 is not exactly 0.3). Check whether their
  difference is very small instead."
* **How to fix:** check whether the difference is very small instead, for
  example `(a - b) < 0.000001 and (b - a) < 0.000001`.

## B2C-I0513 Loop that never stops

* **Severity:** info
* **Meaning:** a `forever` loop contains no `leave loop`, `return` or `stop
  program` block, so it runs until the program is closed. That is fine for
  programs meant to run until stopped.
* **Example:** `forever { print ("tick") }`.
* **Message:** "This 'forever' loop never stops: there is no 'leave loop',
  'return' or 'stop program' block inside it. That is fine if the program
  should run until it is closed."
* **How to fix:** nothing, if that is intended; otherwise add a way out.

## B2C-E0517 Number too large

* **Severity:** error
* **Meaning:** a number does not fit its type: whole numbers (`int`) go from
  -2147483648 to 2147483647, and decimal numbers up to about 1.8 × 10³⁰⁸.
  (`-2147483648` itself is fine; it is generated as `-2147483647 - 1`,
  because C++ reads it as a minus sign applied to 2147483648, which is too
  large.)
* **Example:** `create [int] [big] = (3000000000)`.
* **Message:** "`3000000000` is too large for a whole number (the largest is
  2147483647). Write it as a decimal number, such as 3000000000.0, if you
  need it."
* **How to fix:** use a smaller number, or a decimal number.

## B2C-W0518 Value may lose information

* **Severity:** warning
* **Meaning:** a value is stored where it may not fit: a decimal number into a
  whole number (the part after the decimal point is dropped), or a number
  into a character. C++ does this silently. Also reported when a count, loop
  bound, random-number bound or exit code, which should be whole numbers, is
  a decimal number.
* **Example:** `create [int] [half] = (7.5)`, or `repeat (2.5) times`.
* **Message:** "`half` needs a whole number, but this is a decimal number, so
  the part after the decimal point will be dropped. Use the 'convert' block
  to show that this is intended." / "The number of times to repeat should be
  a whole number, but this is a decimal number. Use the 'convert' block to
  make it a whole number."
* **How to fix:** if this is intended, show it with the `(x) as [int]` block;
  otherwise use a variable of the right type.

## B2C-W0519 True/false values mixed with numbers

* **Severity:** warning
* **Meaning:** a true/false value is used as a number (it becomes 1 or 0),
  including in a `convert` block, or a number is used as a condition (0
  counts as false, anything else as true). C++ allows both, but they are
  usually mistakes, such as `1 < x < 3`, which compares the result of `1 < x`
  with 3.
* **Example:** `if <1 < x < 3> then`.
* **Message:** "This puts true/false values in order with < or >, which treats
  them as 1 and 0. Is that what you meant?"
* **How to fix:** use a comparison (`x ≠ 0`), or combine comparisons with
  `and`: `1 < x and x < 3`.

## B2C-W0521 Comment left out

* **Severity:** warning
* **Meaning:** a block's comment is longer than 64 KiB, so it is not copied
  into the C++ code.
* **Example:** a whole text file pasted into a block comment.
* **Message:** "This block's comment is left out of the C++ code: text can be
  at most 65536 bytes long. Shorten it."
* **How to fix:** shorten the comment.

## B2C-W0522 Loop step not positive

* **Severity:** warning
* **Meaning:** the step of a `for` loop is zero or negative. The direction
  (`to`, `through` or `down to`) already decides whether the loop counts up or
  down, so the step must be more than 0; otherwise the loop never finishes or
  never starts.
* **Example:** `for [i] from (10) down to (0) step (-1)`.
* **Message:** "The step must be more than 0: the direction ('to', 'through'
  or 'down to') decides whether the loop counts up or down. With this step
  the loop would never finish or never start."
* **How to fix:** use a positive step (`step (1)`).
