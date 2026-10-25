# ADR-0018: Typed slots: what each input expects, worked out in one pass

* Status: Proposed (M3 wave W0, item L15; the points marked *owner to
  confirm* are still open)
* Date: 2026-10-25
* Builds on [ADR-0017](0017-open-type-representation.md): its types, rules
  and catalog signatures, including the pattern forms and keys used here
  (ADR-0017 §5). Uses the seams of [ADR-0014](0014-extending-blockly.md) and
  the store reads of [ADR-0016](0016-preview-in-a-web-worker.md) §1, §4 and
  §5. On acceptance, partly supersedes [ADR-0012](0012-parked-blocks.md)'s
  consequence that the checker accepts any value in a parked block's slots
  (§7). No format change ([ADR-0013](0013-additive-format-keys.md)); the
  catalog changes are compatible ([ADR-0015](0015-block-upgrades-on-load.md)).
* Contracts: the API, the normative steps, the editor's tables, the brace
  casts, the fixture, the benchmarks, the W1 plan, the tests and the spec
  changes are in [m3-contracts/slots.md](../plans/m3-contracts/slots.md).

## Context

Typed slots are item L15 of M3
([10 §10.1](../spec/10-roadmap.md#m3-language-breadth-i)) and, with the type
representation, its critical path. The spec asks for phrase blocks whose
inputs follow a container's type (`add (x) to [c]` takes what `c` holds,
[03 §3.7.8](../spec/03-block-language.md#378-collections)), `list of` and
brace lists typed from context
([03 §3.4](../spec/03-block-language.md#34-expression-slots)), `new [Point]
with …` slots from the fields
([03 §3.7.9](../spec/03-block-language.md#379-types-structs-classes-enums)),
and a checker backed by the type system
([03 §3.3](../spec/03-block-language.md#33-block-shapes)) under gradual
checking: errors only when certain, unknown types connect
([03 §3.5.3](../spec/03-block-language.md#353-type-checking-model-gradual)).
M3-4's test wants a text value refused by a parked `int` `set`
(`docs/plans/m2-polish-plan.md:1411-1415`). The M3-D1 assignments need
`add (mark) to [marks]`, `item (face) of [counts]`, `new [Student] with …`,
`[s] . [mark]`, `for each [word] in (words)`, a `return` of an enum and
`switch` on an `int` and an enum.

What the code does today:

* **Bottom-up only.** The analyser lowers an input without knowing its place
  (`crates/b2c-lang/src/lower/expr.rs:126-168`) and checks it against a type
  each block works out by hand (`create`, `set` with its `char` exception,
  call arguments, `return`) through `check_conversion`
  (`lower/convert.rs:90-140`); editable arguments have their own exact rule
  (`E0307`, `lower/expr.rs:572-613`).
* **The editor checks classes, not types:** `B2cConnectionChecker` compares
  the dragged block's type with the input's `check` class
  (`packages/blockly-ext/src/checker/checker.ts:75-94`), so
  `create [int] x = ("hi")` connects and is reported afterwards. It never
  checks loading, undo, redo or paste (`checker.ts:10-14`).
* **References.** The analyser gives an out-of-scope, hidden or wrong-kind
  reference no type and checks nothing against it (`lower/resolve.rs:48-114`,
  `lower/stmt.rs:223-233`); the editor keeps such a reference selected.
* The analysis gives the editor each value block's type and the symbols, but
  nothing for expression slots or parked blocks. Start values follow a
  `create` block's type, as one undoable event (`toolbox/reshape.ts`).

Constraints: gradual checking; pure, deterministic stages and one message per
mistake ([06 §6.1](../spec/06-compiler-pipeline.md#61-overview)); the checker
answers synchronously from the store, and no worker answer reaches the canvas
during a drag (ADR-0016); Blockly only through ADR-0014's seams; W1 adds no
blocks, and golden files stay byte-identical.

## Options considered

**Where the editor gets a slot's expected type.** (1) From the analysis
only: one implementation, but every answer is a preview old (the 50 ms
debounce plus about 43 ms of `update()` at 1,000 blocks) and a block dropped
since has none. (2) **Worked out in TypeScript from the canvas, with the
analysis for what only it knows (chosen):** current answers, at the cost of a
second copy of one function held to the first by a fixture. (3) A synchronous
WebAssembly call per candidate: the main instance keeps no analysis
(ADR-0016 §1), so each call would analyse the canvas, tens of times per move.

**How types flow.** (1) **One pass, local to each block (chosen):** variables
are bound from the block's own parts, a value's type flows up, and only a
place-typed block gets a type from its parent. (2) Global unification (an
`auto` typed by a later `add`): errors far from their cause, the same solver
in the editor, and C++ needs an initialiser for `auto` anyway. (3) Bottom-up
only: `list of` would need its own type field, against 03 §3.4 and §3.7.8.

**Start values.** Per-type catalog defaults would need types during catalog
resolution, which runs before the analyser; the editor writes them (chosen).

**What a refusal stands for.** Refusing every predictable error, `E0315`
included, refuses for information the user may be about to add. Chosen: only
conversion and class errors between known types, as M2.

## Decision

### 1. Where a slot's expected type comes from

An input's expected type is its pattern (ADR-0017 §5) resolved against the
block's bound variables, which come from a type field (`create [list of int]
s = (…)`), a variable or value the block refers to (`add (x) to [marks]`),
the parameters or fields of a symbol a field names (`param:F`, `member:F`),
the definition the block is in (`result`), or, for a place-typed block such
as `list of`, its place.

### 2. The rule: typing one block

`type_block` types one block from its signature, its parts' types, the type
its place expects and the facts about user types. The analyser and the
editor run the same rule, in Rust (in `b2c-lang`, beside `conversion`) and in
a TypeScript copy; nothing else decides an expected type.

1. **Bind from fields, then from value inputs,** each in catalog order, a
   repeated input part by part; the first part that binds a variable decides
   it. Empty parts, `auto` and place-typed values bind nothing. An unknown
   part, an erroneous one, a part whose binding fails, and a part whose class
   membership is not known (a user type the editor's facts do not know yet)
   bind the variable to `Error`. An `assign` input never binds.
2. **Bind from the place,** for a place-typed block only.
3. **Resolve and check.** An input expects its resolved pattern: `Error` when
   what it depends on is empty or already wrong, nothing when no part could
   give it a type. A value input is then outside its class, does not
   convert, or fits by a conversion; empty, unknown and erroneous parts fit
   `Unknown`.

Binding `Error` for an unknown binder makes "unknown is never worse than
known" true: replacing a part by an unknown one never turns another part's
fit into an error. Without it, two inputs of `T` (class `integer`) holding an
`int` and a `double` give a warning while the `int` binds `T`, but an error
once the `int` is unknown and the `double` binds `T`; an `assign` input that
bound `T` would do the same with a `char` among strings. A property test
checks the rule over generated signatures. It also keeps one message per
mistake. The rule is one pass with no backtracking (a later part that
disagrees is the mismatch), deterministic, linear in the block's parts, and
its variables never outlive their block.

### 3. In the analyser

* **Four steps per block:** read the fields and lower the inputs in catalog
  order, putting off inputs that hold a place-typed block; run `type_block`
  with the place the parent passed down; lower the inputs put off with their
  expected types as places; report each fit through the existing checks, so
  M2's blocks keep their wording. No scope position changes.
* **`value_block` takes a place.** A place-typed block without one is
  `B2C-E0315`, in a place of another kind `E0301`, in an `Error` place
  nothing more.
* **References** are read as today (`E0201` to `E0205`, `E0207`) and are
  `Error` when they cannot be used. **Editable parameters** keep their exact
  rule (`E0307`); `type_block` gives only their expected type.
* **Member lists.** A `members` field is read after `type_block`, among the
  bound type's members, not the scope; a symbol that is not one of them is
  `E0207`, whose meaning widens to cover it.
* **Unchecked blocks** (from a newer catalog, unknown, too old to upgrade):
  the set goes into lowering, so their values are `Error` and their inputs
  check nothing. A filter afterwards could not catch `E0301`, which is
  reported at the child.
* **Braces.** Codegen casts each brace item whose type is not the element
  type, except an integer, character or `bool` literal that fits the element
  on every target. Brace initialisation refuses a floating constant into an
  integer type whatever its value (`{2.0}` for a list of `int`), and `{-5}`
  into a list of `char` fails where `char` is unsigned (aarch64 Linux), both
  checked with g++ 11 to 14 in C++17 and C++20. List items then warn like
  variables (*owner to confirm*).
* `slot_types()` gives the type of each expression slot, beside
  `block_types()`. In W1, M2's `create`, `set`, call arguments and `return`
  gain patterns and go through `type_block` with unchanged snapshots; *change
  by*, the conditional value and arithmetic keep their rules. Parked blocks
  stay unlowered until M3-4 (W5), and report nothing after it (ADR-0012).

### 4. In the editor

* **What crosses.** `update()`'s JSON gains `slotTypes`; no new export, worker
  message or IPC change. Expected types do not cross.
* **Parts from the canvas** are read as the analyser would see them. A
  reference is known only when the scope lookup (ADR-0016's scope index, or
  for a block it lacks, its place) lists the symbol at that block with a
  kind the field allows; otherwise it is unknown, as the analyser's `Error`
  is. Values come from their signatures, from M2's rules for `any` and
  `number` outputs, or from the preview; whatever nothing answers is unknown.
* **Candidates.** For a move into an input, `candidateFit` types the parent
  as if the block were there, and a place-typed block with that input's
  expected type: what the analyser computes once the drop is made.
* **Gestures.** A pointer drag types after Blockly has taken the block out.
  The keyboard mover lists targets before unplugging, so its cache treats the
  block as gone. With Zelos, a value over an occupied input fades the old
  block instead of using an insertion marker; the faded block counts in its
  input for every other candidate.
* **Speed and staleness.** Blockly 12.5.1 asks only about connections within
  the snap radius, after its distance check, and a per-gesture cache keeps
  each typing. What only the analysis knows is one preview old, as in M2; a
  refusal in that window ends with the next answer and never changes the
  document.

### 5. Prevented or reported

**The editor prevents** what is certain: it refuses a drop or keyboard move
whose fit is a conversion or class error and allows the rest (narrowing, a
`bool` and number mix, an unknown type, a place-typed block with no place). A
refused place is not offered. Reference menus with a pattern list the symbols
that fit, a `members` menu the bound type's members, and a chosen reference
stays shown. Loading, undo, redo and paste stay unchecked.

**The analyser reports** the rest once, at the part that does not fit:
`E0301` for a value that does not convert, a part outside its class or a
place-typed block in the wrong place; `E0315` for one with no type from its
place; `E0207` for a member the type does not have; `W0518` and `W0519` as in
M2. A message names the part that bound the type (*"`marks` is a list of int,
so the item must be a whole number, but this is text."*). Nothing new is said
for an empty input (`E0430`), an `auto` that binds nothing (`E0309`) or a part
already in error.

### 6. Start values

The editor writes start values as tokens: `0`, `0.0`, `true`, `'a'` and `""`
for the M2 types, none for containers, structs, enums, streams and errors.
They are written when a block is built in the toolbox, when it lands, and when
the user changes a type or reference field: each input whose expected type
changed gets the new value while it still holds exactly the old one, or is
emptied when the new type has none. A typed value, a block, a draft and an
unknown type are left alone, and a connection never triggers this (*owner to
confirm*). Each replacement is one undoable event in the group of its cause;
the event now records its input's name.

### 7. Undo, paste, parked and disabled blocks

Undo and redo change only the canvas, from which every expected type is
computed; a redo the types would now refuse still works and is reported.
Paste carries no types: blocks are typed where they land, after their
references, members included, are bound again (ADR-0017 §4). Until M3-4 the
editor types a parked block from its own parts, so a text value no longer
snaps into a parked `int` `set`, while parked code still reports nothing; a
variable created inside the stack is unknown. A disabled block binds nothing
in its parent, and its own slots are typed like any other block's.

### 8. Diagnostic codes and versions

The same allocation as ADR-0017 §7:

| Code | Meaning | From | First emitted |
| --- | --- | --- | --- |
| `B2C-E0140` | type term too deep or too wide | ADR-0017 | W1 (loader) |
| `B2C-E0213`, `E0214`, `E0221` | `::name` reach (06 §6.14.7); shared header part names a non-shared definition; module name starts with `b2c` | spec; ADR-0019 | W3 |
| `B2C-E0313`, `E0314` | not comparable; not printable | ADR-0017 | W1 |
| `B2C-E0315` | no type from its place | ADR-0018 | W1 |
| `B2C-E0420`, `E0421` | declaration cycles (06 §6.7) | spec | W2 |
| `B2C-E0422`, `E0423` | header cycle; cross-module initialisation order | ADR-0019 | W3 |

`E0206`, `E0207` and `E0301` are reused with new text; the next free codes are
`E0141`, `E0219`, `E0222`, `E0316` and `E0424`. `E0315` lands in W1 with
`value_block`'s place and its reference entry: `codes.rs`'s sync test wants
an entry for every code defined, though no W1 block reaches it.

This ADR changes no format: its catalog keys ride in catalog 1.3.0 with
ADR-0017's, and `update()`'s JSON gains `slotTypes`, the core and the editor
shipping in one build (ADR-0016 §8).

### 9. W1

In ADR-0017's contracts pull request, which keeps the workspace green:
`slots.rs`'s API with a body answering `Unknown` until the `b2c-lang`
package, `slot_types()` as a type, and the TypeScript declarations. Then
`b2c-catalog` adds the patterns of M2's four blocks, `b2c-lang` the rule and
lowering, `b2c-core-wasm` `slotTypes`, and the TypeScript package the copy,
the checker, the menus, the caches and the start values. The brace casts come
with `list of` in W2.

## Consequences

* **Easier.** A block's slots are described once in its catalog entry, and
  the analyser, the checker, the menus and the start values read the same
  rule. Phrase blocks, struct construction, typed calls and returns need no
  hand-written typing. Wrong drops into `create`, `set`, call arguments and
  `return`, which M2 let through, are refused.
* **Harder.** Two copies of `type_block` must agree. The fixture holds the
  rule and the value types of `any` and `number` outputs; the mapping from
  the canvas to parts has only unit tests and the sampled no-false-refusal
  test, so a disagreement there can reach users. Lowering is no longer purely
  left to right, and the editor can be a preview behind for what only the
  analysis knows.
* **Visible in W1.** The editor refuses, in M2's typed inputs, drops the
  analyser reported as errors, and in parked blocks drops it will report once
  attached (*owner to confirm*).
* **Performance.** In Rust, one `type_block` per block with a signature,
  inside the analysis `update()` already runs. In TypeScript, a few typings
  per pointer move, cached per gesture; a keyboard listing types every free
  value input, up to 5,000. W1 adds `webview.drag-typed-1000.frame-p95-all`
  (reported, gated after 5 baselines) and a timed keyboard listing at 1,000
  blocks against N12's 100 ms, since `drag-5000` never previews a place. The
  Blockly 13 upgrade after M3 must measure typed checks over every connection
  pair at drag start, which Blockly 13 caches through the checker
  ([the spike](../plans/m3-spike-blockly13.md)). The per-block cache of
  06 §6.13 (M5) must key a block on the facts its typing reads.
* **Security** ([spec 08](../spec/08-security.md)). Slot typing reads only the
  document, the catalog and the analysis, adds no input and puts no text into
  C++: a cast is a typed node. The TypeScript side parses types with
  ADR-0017's bounded parser, keeps `slotTypes` in `Map`s, and never throws
  into Blockly: what it cannot read is unknown, which connects.
* **On acceptance,** ADR-0012's status and the ADR index read *Partly
  superseded by ADR-0018 (the checker types a parked block's slots)*.

## Tests

In the contracts (§9 there): `type_block` by table for every step, unknown
membership and the `assign` case included; property tests that it never
panics, is deterministic and that unknown is never worse than known; a slot
fixture written by `b2c-lang` that the TypeScript copy agrees with field by
field; lowering keeps M2's snapshots, still gives one `E0307` for a bad
editable argument and no `E0301` at an unchecked block; brace casts compile
clean with and without `-funsigned-char` on GCC 11 and 15; in the editor,
wrong drops into M2's typed inputs are refused, an out-of-scope reference
accepts anything, keyboard listings and faded blocks are typed right, and
start values follow field changes in one undo step; a sampled
no-false-refusal test over the example and assignment projects, with parked
and disabled parents checked apart; E2E flows on Linux and Windows; the two
benchmarks.

## Owner to confirm

1. The editor works out expected types in a TypeScript copy held to the Rust
   one by a fixture, so a type changed a moment ago counts at once
   (*owner to confirm*).
2. `list of` and `{…}` take their type only from their place; where it gives
   none (*print*, a `create` set to `auto`, and `for each [x] in (list of 1 2
   3)`, which C++ accepts with `<initializer_list>`) they are `E0315`
   (*owner to confirm*).
3. In W1, M2's `create`, `set`, call arguments and `return` refuse drops that
   M2 allowed and the analyser reported (*owner to confirm*).
4. Codegen casts brace items whose type is not the element type, instead of
   reporting every narrowing item as an error (*owner to confirm*).
5. Start values change only when a field changes or a block lands, and types
   without one leave the input empty (*owner to confirm*).
6. Parked blocks' slots are typed from their own parts, so the editor refuses
   wrong drops there before parked code reports anything (*owner to confirm*).
