# M3 contracts: typed slots

The exact shapes behind [ADR-0018](../../adr/0018-slot-typing.md) (L15). The
ADR holds the decisions and their reasons; this file holds the API, the
normative steps, the editor's tables, the codegen rule for braces, the M2
blocks' patterns, the fixture, the benchmark, the W1 plan, the tests and
the spec changes.

It builds on [types.md](types.md): the pattern grammar and keys (§4.1,
§4.2), the meaning of a signature (§4.3), `BlockSig`, `PartType`,
`BlockParts`, `Fit`, `Resolution` and `TypeEnv` (§4.4), the classes (§1.3),
the codes (§6) and the versions (§7). ADR-0018 adds nothing to the
grammar. If this file and the ADR disagree, the ADR wins.

## 1. `crates/b2c-lang/src/slots.rs`

New, re-exported from `lib.rs`. It lives in `b2c-lang` because it calls
`conversion` (`typing.rs:97-109`), which stays there, and `b2c-ir` may
depend on nothing (`tools/check-layering.py`).

```rust
/// How a part fits what its block expects of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotFit {
    /// Nothing to check: an empty, unknown or erroneous part, a value typed from its place,
    /// an expectation that does not resolve, or class membership that is not known.
    Unknown,
    /// It fits by this conversion (never `Invalid`); `Narrowing` and `BoolNumber` warn.
    Converts(Conversion),
    /// Its type is outside the class the part requires (`Fit::OutsideClass`, or its `check`).
    OutsideClass,
    /// It cannot fit: a wrong shape for the pattern (`Fit::BadShape`), or `Conversion::Invalid`.
    Invalid,
}
impl SlotFit {
    /// `OutsideClass` and `Invalid`: what the analyser reports and the editor refuses.
    pub const fn is_error(self) -> bool;
}

/// A value input part's expected type and how its value fits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputTyping {
    /// `None`: nothing gives this input a type (`Resolution::Open`). `Some(Type::Error)`:
    /// what would give it one is missing or already wrong (`Resolution::Missing`).
    pub expects: Option<Type>,
    pub fit: SlotFit,
}

/// Which part bound a variable, for messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binder { Field(String), Input(String), Place }

/// The result of typing one block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockTyping {
    /// Each bound variable, its type (`Error` when an unknown or failed part bound it) and its binder.
    pub bound: BTreeMap<TypeVar, (Type, Binder)>,
    /// Every value input part the block has, by part name.
    pub inputs: BTreeMap<String, InputTyping>,
    /// Every `type` and `symbol_ref` field that has a pattern or `members`.
    pub fields: BTreeMap<String, SlotFit>,
    /// What each `symbol_decl` field with a pattern declares (`Type::Error` if it does not resolve).
    pub declares: BTreeMap<String, Type>,
    /// The block's value type: `None` for `any` and `number` outputs, which keep their M2
    /// rules; `Some(Type::Error)` for an output that does not resolve.
    pub output: Option<Type>,
    /// A place-typed block: how its place's type fits its output variable (`Unknown` without one).
    pub place: SlotFit,
}

/// Types one block. Pure and deterministic, never panics, linear in the block's parts.
pub fn type_block(
    sig: &BlockSig,
    parts: &dyn BlockParts,
    place: Option<&Type>,
    facts: &dyn TypeFacts,
) -> BlockTyping;

// crates/b2c-lang/src/typing.rs
/// `conversion` where a value is assigned (`assign = true`): also `char` to `std::string`,
/// as `Widening`.
pub fn assignment(from: &Type, to: &Type) -> Conversion;
/// Whether an input whose `check` is `class` accepts a value of type `ty` (types.md §1.3):
/// `bool`, `integer` and `number` by conversion, `any` every type but `void`, the others by
/// membership. `None` when not known.
pub fn check_accepts(class: TypeClass, ty: &Type, facts: &dyn TypeFacts) -> Option<bool>;
```

## 2. The rule, step by step

Normative for both copies. `TypeEnv` (types.md §4.4) does the binding and
resolving; `type_block` adds the checks.

1. **Bind from fields,** in `order`. For each `type` or `symbol_ref` field
   with a pattern (not a `members` field):
   * `Known(t)`, `t` not `Error`: `TypeEnv::bind`. `Fits` binds the
     pattern's unbound variables (a variable already bound is left alone:
     first binding wins). `OutsideClass` and `BadShape` record that part's
     failure and bind its unbound variables to `Error`. `Unknown` (class
     membership not known: a user type the facts do not know) binds its
     unbound variables to `Error` too.
   * `Known(Error)` or `Unknown`: bind the pattern's unbound variables to
     `Error`.
   * `Empty`, `Auto`, `FromPlace`: bind nothing.
2. **Bind from value inputs,** in `order`, a repeated input part by part
   (`ITEM0`, `ITEM1`, …), the same way. An `assign` input never binds.
3. **Bind from the place.** A place-typed block binds `place_var` with
   `TypeEnv::bind_place`. `None` leaves it unbound. A place outside the
   variable's class makes `place` `OutsideClass` and binds `Error`; a place
   whose membership is not known makes `place` `Unknown` and binds `Error`.
4. **Resolve and check.** Every pattern is resolved
   (`TypeEnv::resolve`, with the part's index for `param:` and `member:`):
   `Type(t)` gives `t`; `Missing` gives `Error`; `Open` gives nothing. An
   unbound variable resolves to `Missing` when one of the parts that could
   bind it is `Empty`, and to `Open` when the only ones are `Auto` or
   `FromPlace` parts, or when there is none. A type function that does not
   apply to its argument's type resolves to `Missing`. The answers of `field:`,
   `symbol`, `param:`, `member:` and `result` are `Type(t)` for `Known(t)`
   and `Missing` otherwise. Then:
   * a part that is `Empty`, `Auto`, `FromPlace`, `Unknown` or `Known(Error)`
     fits `Unknown`;
   * an input's `expects` is `Some(t)`, `Some(Error)` or `None` for
     `Type(t)`, `Missing` or `Open`;
   * a value input holding `Known(t)` is, in this order: the recorded
     failure of its own binding (`OutsideClass` or `Invalid`); `OutsideClass`
     when `check_accepts` is `Some(false)`; `Unknown` when it is `None`;
     otherwise `conversion(t, e)` (`assignment(t, e)` with `assign`) when
     `expects` is `Some(e)`, which gives `Invalid` or `Converts(…)`;
     `Unknown` when `expects` is `None`;
   * a `type` field: the recorded failure of its binding, else
     `Converts(Same)`;
   * a `symbol_ref` field with a pattern: the recorded failure of its
     binding; `Invalid` when an earlier part bound its variable to another
     type (a reference is never converted); else `Converts(Same)`;
   * a `members` field: when its variable is bound to a type other than
     `Error`, `chosen_member(field, owner)`; `Known(t)` fits
     `Converts(Same)`, anything else `Unknown`;
   * `declares`: each `symbol_decl` field's resolved pattern, `Error` for
     `Missing` or `Open`;
   * `output`: `None` for `any` and `number`; else the resolved `output`,
     `Error` for `Missing` or `Open`; `symbol:F` on a `members` field
     resolves to that field's `chosen_member` answer.

**Guarantees.**

* One pass, no backtracking: each part is bound or checked once, and a
  later part that disagrees with a binding is the mismatch, never the
  earlier one.
* Deterministic and bounded: the result depends only on the arguments; the
  work is linear in the block's parts; each pattern is at most 8 levels.
* Unknown is never worse than known: replacing any part by `Unknown` never
  turns another part's fit from a non-error into an error. It holds because
  an `Unknown` binder binds its variables to `Error`, and a part that is
  not a binder only changes its own fit. Without that rule it fails: with
  `where = { T = "integer" }` and two inputs of `T` holding an `int` and a
  `double`, the `double` is a narrowing (a warning) while the `int` binds
  `T`, but outside the class (an error) once the `int` is `Unknown` and the
  `double` binds `T`. `assign` inputs never binding closes a second case
  (`std::string`, `char`, `std::string`: the `char` would bind `T`).
* Variables are local to one block. Between blocks, types flow up (a
  value's type to its parent) and down (an expected type into a
  place-typed block) only.

## 3. In the analyser

* **Four steps per block** with a signature:
  1. read its fields and lower its value inputs in catalog order, as now,
     except that an input holding a place-typed block (or, with W4's slot
     editor, a brace list) is put off and is `FromPlace`;
  2. run `type_block` with the place the parent passed down;
  3. lower the inputs put off, each with its resolved `expects` as its
     place;
  4. report each part's fit through the existing checks (`check_conversion`
     and its messages, `lower/convert.rs:90-140`), so M2's blocks keep
     their wording.

  The caller keeps the scope around the four steps: `create` runs them
  inside its pending window, where the new variable's own name is hidden
  (`lower/stmt.rs:153-156`). The order of lowering changes but no position
  in the scope index does; only inputs that are put off move in the order of
  diagnostics, and no block puts one off before W2.
* **`value_block` takes a place:**
  `fn value_block(&mut self, block: &Block, place: Option<&Type>) -> Expr`
  (`lower/expr.rs:173`). A place-typed block whose place is `None` has type
  `Error` and is `E0315`; one whose place is outside its class has type
  `Error` and is `E0301`; one whose place is `Error` has type `Error` and
  nothing more is said. Any other block ignores its place.
* **Part types.** A reference is read as today: `E0201` to `E0205` and
  `E0207` (`lower/resolve.rs:48-114`), then `Known(Error)` when it cannot be
  used. `E0430` for an empty required input and a damaged type field
  (`lower/mod.rs:744-771`) give `Empty` and `Known(Error)`. A disabled block
  in an input is `Empty`. `result` comes from `ItemCtx`
  (`lower/mod.rs:118-128`): the function's return type (`Empty` for `void`),
  `int` in `main`; M3-4's `ItemCtx::Parked` gives `Unknown`.
* **Editable parameters.** An argument for an editable (`&`) parameter keeps
  `check_argument` (`lower/expr.rs:572-613`: a changeable variable of exactly
  the parameter's type, else `E0307`), as *change by* keeps its rule;
  `type_block` gives only its `expects`, and its fit is not reported.
* **`members` fields** are read after `type_block`: the chosen symbol is
  looked up among the members of the type bound to the field's variable,
  not in the scope. A symbol that is not one of them is `E0207` (*"`speed`
  is not a field of `Student`."*). When the variable is unbound or `Error`,
  nothing more is said.
* **No signature, no check.** Since the integration step (types.md §8.3)
  `create`, `set`, call arguments and `return` have no M2 rules left: with a
  table that lacks their signatures (`SigTable::empty()`, hand-built tables
  in tests), they check nothing of their values' types.
* **Blocks the catalog kept unchecked** (made by a newer catalog, unknown,
  too old to upgrade): `analyze_checked_with` passes the `unchecked` set
  into lowering, so an unchecked block's value is `Error` and an unchecked
  parent's inputs are lowered but check nothing. A filter after the
  analysis could not do this: `E0301` is reported at the value
  (`lower/convert.rs:90-94`), the child, not at the unchecked parent. The
  filter for `E0430` (`lib.rs:104-135`) stays.
* **`slot_types()`** gives the type of each expression slot that was
  lowered, by block and input, recorded by `Lowerer::slot`
  (`lower/expr.rs:618`). `block_types()` stays as it is.
* **Expression slots** take a place too: a brace list at the top of a slot
  is place-typed, and inside a slot a call's arguments get their
  parameters' types as places. This lands with W4's slot editor, which
  brings brace tokens.
* **Parked blocks** stay unlowered until M3-4 (W5), which lowers them in its
  sandbox with the same code; they still report nothing (ADR-0012).

### 3.1 Braces and casts (codegen, W2)

Codegen writes each item of a brace list (`list of`, `new … with`, `{…}`)
whose static type is not the element type as `static_cast<T>(item)`. The
one exception is an integer, character or `bool` literal whose value fits
the element type on every target: any `int` literal into `int` or
`double`, 0 to 127 into `char`, 0 and 1 into `bool`. A floating literal
into an integer, `char` or `bool` element is always cast.

The analyser applies the conversion rule as for a variable: narrowing
warns (`W0518`), `bool` and numbers mixed warn (`W0519`), widening is
silent. With the casts, g++ never meets a narrowing conversion inside
braces.

Checked with g++ 11, 12, 13 and 14, `-std=c++17` and `-std=c++20`, `-Wall
-Wextra -Wpedantic -fsyntax-only`:

| Code | Default | `-funsigned-char` |
| --- | --- | --- |
| `std::vector<int> v = {1.5};` | error | error |
| `std::vector<int> v = {2.0};`, `= {1e3};` | error | error |
| `P p{.x = 2.0};` with `int x` (C++20) | error | error |
| `std::vector<int> v = {d};` with `double d`; `std::vector<double> v = {i};` with `int i` | `-Wnarrowing` warning | same |
| `std::vector<char> v = {-5};` | clean | error |
| `std::vector<char> v = {200};` | error | clean |
| `std::vector<bool> v = {2};` | error | error |
| `{1, 2.5}` for `double`; `{65, 97, 127, 0}` for `char`; `{1, 0}` for `bool`; `{static_cast<double>(i)}` | clean | clean |

Brace initialisation refuses a floating constant into an integer type
whatever its value, and the signedness of `char` depends on the target
(unsigned on aarch64 Linux, best-effort in 01 N1), hence the 0 to 127 rule.

## 4. In the editor

### 4.1 What crosses

The JSON of `update()` gains `slotTypes`, the analysis's `slot_types()`,
next to `blockTypes`. It travels inside the worker's `updateJson` unchanged
(ADR-0016 §3), and the store keeps it with the preview. No new export, no
worker message, no IPC change.

```ts
// packages/b2c-core-wasm/src/api.ts
export interface PreviewResult {
  // … as now; blockTypes with TypeJson (types.md §3.1)
  /** The static type of each expression slot the analysis lowered, by block ID, then input name. */
  slotTypes: Record<string, Record<string, TypeJson>>;
}
```

### 4.2 The TypeScript copy (`packages/blockly-ext/src/types/slots.ts`)

```ts
export type PartType =
  | { readonly kind: 'empty' }
  | { readonly kind: 'auto' }
  | { readonly kind: 'fromPlace' }
  | { readonly kind: 'known'; readonly type: StaticType }
  | { readonly kind: 'unknown' };

export interface BlockParts {
  field(name: string): PartType;
  input(part: string): PartType;
  count(base: string): number;
  param(field: string, index: number): PartType;
  member(field: string, index: number): PartType;
  chosenMember(field: string, owner: StaticType): PartType;
  result(): PartType;
}

/** `b2c_lang::SlotFit`: a conversion that fits, `invalid`, `outsideClass` or `unknown`. */
export type SlotFit = StaticConversion | 'outsideClass' | 'unknown';
export type Binder =
  | { readonly kind: 'field'; readonly name: string }
  | { readonly kind: 'input'; readonly name: string }
  | { readonly kind: 'place' };

export interface BlockTyping {
  readonly bound: ReadonlyMap<string, { readonly type: StaticType; readonly binder: Binder }>;
  readonly inputs: ReadonlyMap<string, { readonly expects: StaticType | null; readonly fit: SlotFit }>;
  readonly fields: ReadonlyMap<string, SlotFit>;
  readonly declares: ReadonlyMap<string, StaticType>;
  readonly output: StaticType | null;
  readonly place: SlotFit;
}

/** `def` is the part of a catalog definition that holds its signature keys (`SignatureDef`);
 *  the signature built from it is kept per definition. */
export function typeBlock(def: SignatureDef, parts: BlockParts, place: StaticType | null, facts: TypeFactsTs): BlockTyping;
/** The same for a signature already built (`blockSignature(def)`). */
export function typeBlockSig(sig: BlockSigTs, parts: BlockParts, place: StaticType | null, facts: TypeFactsTs): BlockTyping;
/** `invalid` and `outsideClass`. */
export function isFitError(fit: SlotFit): boolean;
/** Start values (§6); null for a type that has none. */
export function startTokens(type: StaticType): readonly TokenJson[] | null;
```

### 4.3 The oracle and the Blockly side

```ts
// packages/blockly-ext/src/services.ts: TypeOracle gains four optional methods
export interface TypeOracle {
  /** As in M2: the analysis's type of a value block, then the type of the symbol it names. */
  outputTypeOf(block: Blockly.Block): StaticType | null;
  /** The analysis's type of an expression slot (`slotTypes`), or null. */
  slotTypeOf?(blockId: string, input: string): StaticType | null;
  /** A symbol of the latest analysis, or null. */
  symbol?(symId: string): SymbolInfo | null;
  /** Whether the symbol is visible at the block (and input): the scope index of ADR-0016 §4, or
   *  for a block the index lacks, its place (`toolbox/scope.ts:239-272`). Null when not known. */
  visibleAt?(symId: string, block: Blockly.Block, input: string | null): boolean | null;
  /** What the type rules need to know about user types, from the latest analysis. */
  facts?(): TypeFactsTs;
}

// packages/blockly-ext/src/checker/slots.ts (new)
/** Each block's typing and value type, for one gesture (a drag, a keyboard move's listing). */
export class SlotCache {
  constructor(oracle: TypeOracle, options: { readonly absent?: Blockly.Block });
}
/** A block's typing from its parts on the canvas now; null for a block outside the catalog. */
export function blockTyping(block: Blockly.Block, oracle: TypeOracle, cache?: SlotCache): BlockTyping | null;
/** The block's value type, 'fromPlace' for a place-typed block, or null when not known. */
export function valueType(block: Blockly.Block, oracle: TypeOracle, cache?: SlotCache): StaticType | 'fromPlace' | null;
/** How `child` would fit in `parent`'s value input `input`, typed as if it were there. */
export function candidateFit(parent: Blockly.Block, input: string, child: Blockly.Block, oracle: TypeOracle, cache?: SlotCache): SlotFit;
/** The running gesture's cache, which the dragger and the keyboard mover set and clear. */
export function currentSlotCache(): SlotCache | null;
export function setCurrentSlotCache(cache: SlotCache | null): void;
/** Starts the gesture's cache with the checker's oracle; `endSlotGesture` drops it. */
export function beginSlotGesture(options?: { readonly absent?: Blockly.Block }): SlotCache;
export function endSlotGesture(): void;
```

The editor's oracle (`apps/desktop/src/editor/services/symbols.ts`) answers
all four new methods. They are optional so that an oracle without them, a
test's, stays valid: it answers nothing, which types every reference and
expression slot as unknown, so everything connects there. Every call into
the oracle is guarded, and a failure answers unknown.

**Parts read from the canvas:**

| Part | `PartType` |
| --- | --- |
| A type field | its value's term (types.md §3.2), `auto`, or `empty` |
| A `symbol_ref` field | `empty` when nothing is chosen; the symbol's type from the preview's `symbols` when `visibleAt` is true and the symbol's kind is one of the field's `kinds`; `unknown` otherwise (out of scope, hidden, of another kind, or not known to the preview): what the analyser's `Known(Error)` amounts to |
| A `members` field | `chosenMember`: the symbol's type when the preview lists it as a member of `owner`, else `unknown` |
| An input holding a block | `empty` for a disabled block; `fromPlace` for a place-typed block; otherwise its value type: from its own signature (recursively), from M2's rules for `any` and `number` outputs (`output-type.ts`), or from the preview's `blockTypes`; `unknown` when none answers |
| An input holding an expression shadow | one literal: its type; one reference: as a `symbol_ref` field; a brace list: `fromPlace`; other tokens: the preview's `slotTypes`, else `unknown` |
| An empty input | the type of its catalog default tokens, or `empty` |
| `param:F`, `member:F` | the parameter's or field's symbol type, from the preview's `symbols` |
| `result` | up the parents to the root: `int` under *when program starts*, the `RETURNS` type under *define* (`empty` for `void`), `unknown` loose on the canvas or in the toolbox |

**Gestures.**

* **Pointer drags.** `B2cBlockDragger` (`apps/desktop/src/editor/sync/drag.ts:348-402`)
  makes a cache when a drag starts, after Blockly has taken the block out,
  and drops it when the drag ends.
* **Keyboard moves.** The mover lists its targets while the moved block is
  still connected (`keyboard/mover.ts:178`, before `block.unplug` at
  `:442`), so its cache is made with `absent` set to the moved block, which
  then counts as missing from its current input. The listing runs the type
  rule for every free value input on the canvas, at any distance, up to
  `MAX_DROP_TARGETS` (5,000; `keyboard/targets.ts:23`, `:125`).
* **Replacement previews.** With the Zelos renderer, Blockly 12.5.1 never
  puts an insertion marker in a value input
  (`shouldUseMarkerPreview`); a value dragged over an occupied input fades
  the block there (`previewReplacement`) and leaves it connected. The faded
  block counts in its own input for every other candidate and is replaced
  only for its own input.

**Candidates.** For a drag or a keyboard move that would put `child` into
`parent`'s input `I`, `candidateFit` types `parent` as if `child` were in
`I`:

* a place-typed `child`: `I` is `fromPlace`; the parent's typing gives `I`'s
  `expects` `e`; `child` is typed with the place `e`; the answer is
  `child`'s `place`;
* any other `child`: `I` holds `child`'s value type (`unknown` when it is not
  known), and the answer is `I`'s fit.

A place-typed parent gets its own place the same way from its parent, up
the chain. M2's `conversionAllowed` (`packages/blockly-ext/src/checker/conversion.ts:75-97`)
becomes the case of a parent without patterns.

**Menus.** A `symbol_ref` field with a pattern lists the symbols in scope
whose type binds that pattern on its own (class and shape: *add … to* lists
the lists, sets, queues and stacks; *item … of* the lists and maps), without
looking at the block's inputs, which are checked against the field and not
the reverse. A `members` field lists the members of its variable's type, and
nothing while the variable is unbound: left for W2, whose struct blocks
bring the first `members` field (W1 lists the plain scope there, as M2
did; the review of W1 found it, finding TD-3). A reference already chosen stays
shown (`packages/blockly-ext/src/fields/symbol-ref.ts:12-14`). Type fields
offer the types their pattern allows. M2's `kinds` filters stay for M2's
blocks.

**Where reports show.** The analyser's slot diagnostics show where every
analyser diagnostic shows: the block's badge, the Problems panel and the
slot's tooltip (03 §3.4, [04 §4.4](../../spec/04-user-interface.md#44-diagnostics-ux)).

**Speed.** Blockly 12.5.1 asks only about connections within the snap
radius, after its own distance check (`ConnectionDB.searchForClosest`;
`checker.ts:132-140`), so a pointer move types a few nearby parents. The
cache keeps each block's typing and value type, each candidate's fit and
each visibility answer for one gesture; under ADR-0016 §5 no preview is
applied during a drag, so its answers stay valid, and it notes the preview
it was made from and starts again if that changes. Types parsed from a
preview are kept per preview object, as `symbolsById` keeps symbols
(`services/symbols.ts`). Outside a gesture, a typing is computed when
something asks (a menu, a tooltip, a start value), for one block.

A candidate's fit types only what it needs, since a scope query (the
core's `symbolsInScope`, which serialises every visible symbol) costs
about 50 µs with 10 visible variables and 0.9 ms with 300:

* in an input without a pattern (`print`'s items, `repeat`'s count), the
  candidate's class alone (`checkAccepts`), as M2's checker did: no typing
  of the parent and no query;
* in a patterned input, the parent's binding parts and that input, not
  its other inputs (`typeBlockSig`'s `only`);
* the checker asks `candidateRefused`, which works the fit out first as if
  every reference were visible and asks about scopes only to confirm a
  refusal (a reference that is not visible is `unknown`, which can only
  turn a refusal into a fit);
* the oracle answers "visible" for a variable whose `create` comes before
  the block in a statement list holding it, with nothing on the way that
  could hide its name, without a query; and keeps each query's answer
  until the next preview.

On a 1,000-block module of 100 `create` and 898 `set` blocks (100
variables), one keyboard listing's typed checks went from 271 ms and 898
queries to about 25 ms and none (W1 review, finding ED-1).

## 5. M2 blocks' patterns in W1

Package 2 adds them; package 3 lowers the blocks through `type_block` once
the table has them (types.md §8).

| Block | Signature |
| --- | --- |
| `var.declare` | `where = { T = "value" }`; `TYPE`, `NAME` and `VALUE` `type = "T"` |
| `var.set` | `where = { T = "value" }`; `VAR` and `VALUE` `type = "T"`; `VALUE` `assign = true` |
| `func.call`, `func.call_stmt` | `ARG` `type = "param:FUNC"` |
| `func.return` | `VALUE` `type = "result"` |

*change by*, the conditional value and arithmetic keep their own rules and
get no pattern. Their SAST and diagnostic snapshots must not change.

## 6. Start values

* `startTokens(type)` gives `0`, `0.0`, `true`, `'a'` and `""` for `int`,
  `double`, `bool`, `char` and `std::string` (M2's,
  `toolbox/values.ts:11-27`), and none for containers, maps, structs,
  enums, streams and errors in M3. A `create` block of a list type with no
  value declares an empty list (`std::vector<int> scores;`); W4 may add `{}`
  once slots accept braces.
* **In the toolbox,** each empty value input with a known `expects` gets its
  start value when the flyout block is built (presets and chosen references
  count), as M2 does for call arguments.
* **On a field change** by the user (a type or `symbol_ref` field, on the
  canvas or in the flyout), each value input of that block whose `expects`
  changed, and recursively each item of a place-typed block in such an
  input, gets the new type's start value while it still holds exactly the
  old type's. When the new type has none, the start value is removed and
  the input left empty. A value the user typed, a block, a draft and an
  unknown type are left alone. A connection never triggers this.
* **On landing** (`toolbox/landing.ts`), the same rule runs with the
  `expects` of the block's place, where `result` first becomes known: a
  value input still empty gets the start value of the type its place
  expects, when known (`return` gets `0` in `main` and `0.0` in a function
  returning `double`; nothing in a `void` function or loose), in the drop's
  undo step (`landingStartValues` in `toolbox/reshape.ts`).
* **What W1 left as in M2.** Start values in the toolbox, when the flyout
  block is built, are still M2's (call arguments only); with M2's blocks
  only `return` lacks one, and landing fills it. `create`'s `TYPE` and
  `set`'s `VAR` keep their own M2 code in `toolbox/reshape.ts`, which gives
  the same results as the rule above. Start values read a reference's type
  whether or not the symbol is visible there (M2's behaviour), unlike the
  checker, which reads `visibleAt`.
* **Events.** Each replacement is one `StartValueChange`
  (`toolbox/reshape.ts:67-108`) in the group of the change that caused it.
  The event gains the input's name, since `run()` and `startValueOf()` read
  only `VALUE` today (`:40-53`, `:96-105`); an item of a nested place-typed
  block is its own block's event. M2's tests stay.

## 7. The slot fixture

A `b2c-lang` test writes `packages/blockly-ext/src/types/slot-fixture.json`:

* hand-written cases for every rule of §2, including unknown class
  membership (facts that do not know a `Named` type), the two
  counterexamples of §2 and the `members` field;
* for each catalog block with a pattern, a seeded sample of part
  combinations (at most 50 per block), each with its signature as
  `catalog.json` writes it, its parts, place and facts, and the full
  `BlockTyping`, binders included;
* for each sampled block whose output is `any` or `number`, the value type
  the analyser gives it in a small document built around it, so the M2
  output rules the TypeScript copy keeps (`output-type.ts:146-309`) are held
  to the analyser too.

It is committed, and CI fails when it is stale, as for `catalog.json`. It
does not cover the canvas-to-`PartType` table of §4.3: unit tests and the
sampled no-false-refusal test (§9) do.

## 8. Benchmarks

* `webview.drag-typed-1000.frame-p95-all` (new in W1): a number block
  dragged slowly down a column of 200 `create` blocks of alternating types
  in a 1,000-block module (the rest `set` of those variables and prints of
  them, by turns, so the checker meets references), so that every move
  asks about typed slots;
  reported until it has 5 baselines, then gated, as D20's pooled metric was
  (a later change to `typed-slots-metrics.ts` and the benchmarks' README).
* `webview.keyboard-list-1000.ms` (new in W1, reported): the time to list
  the keyboard mover's targets for a number block in the same workspace,
  against N12's 100 ms at 1,000 blocks, read from the User Timing span
  `b2c:keyboard-targets`. An A/B base build without the span skips the
  listing with a note and still measures the drag.
* `webview.drag-5000.frame-p95-all` stays gated; it drags a definition across
  empty canvas and never previews a place
  (`docs/plans/m2-polish-plan.md:1097`), so it does not see typed slots.
* The native benchmark's generated module gains W2's blocks with W2.
* The Blockly 13 upgrade (after M3) must measure typed checks over every
  connection pair at drag start: Blockly 13 caches every pair the checker
  allows (`cacheAllConnectionPairs`,
  [the spike](../m3-spike-blockly13.md)), and with typed slots each pair is
  a typing.

## 9. W1 plan and tests

**Done** in W1 (types.md §8), with the benchmarks and the E2E flows; §6
says what start values still do as in M2.

**Contracts** (in the contracts pull request, types.md §8.1, under its
green rule): `slots.rs`'s API and its test tables, with a body that answers
`Unknown` everywhere until package 3; `Analysis::slot_types()` and
`Preview::slot_types` (the types); the TypeScript declarations
(`PreviewResult.slotTypes`, `types/slots.ts` and `checker/slots.ts` types,
`TypeOracle`'s new methods, `packages/blockly-ext/src/services.ts:79-82`).

**Packages** (types.md §8.2, each with this part):

1. `b2c-catalog`: the patterns of §5 on M2's blocks, with the load checks of
   types.md §4.3.
2. `b2c-lang`: `type_block`, the four steps, `value_block`'s place, part
   types, editable arguments, `members` fields, the `unchecked` set,
   `slot_types()`, `E0315` with its reference entry, the slot fixture writer;
   M2's blocks through `type_block` with unchanged snapshots.
3. `b2c-codegen`: nothing in W1; the brace casts come with `list of` in W2.
4. `b2c-core-wasm`: `slotTypes` in `update()`.
5. TypeScript: `types/slots.ts` against the fixture; `checker/slots.ts`;
   `B2cConnectionChecker`'s type rule on `candidateFit`; the reference menus;
   the caches in `B2cBlockDragger` and the keyboard mover; the oracle
   (`apps/desktop/src/editor/services/symbols.ts`); start values
   (`toolbox/reshape.ts`, `values.ts`, `landing.ts`, `plugin.ts`); the two
   benchmarks.

**Rust tests.**

* `slots.rs`, by table: each step of §2; first binding wins; `Auto`,
  `Empty` and `FromPlace` bind nothing; `Unknown`, `Known(Error)`, a failed
  binding and unknown membership bind `Error`; a class and a shape mismatch;
  a pattern that does not resolve accepts its class; a place-typed block
  with a place, without one, with one outside its class and with one whose
  membership is unknown; `expects` is `Error`, not `None`, when its binder is
  empty (so `set [ ] to (list of …)` gives only `E0430`, never `E0315`);
  `assign` (`char` into `std::string`) never binds; `param:`, `member:` and
  `result`, also out of range and for `void`; repeated inputs by index; the
  `check` classes `bool`, `integer` and `number` by conversion; a `members`
  field.
* Property tests (proptest) over generated signatures, part types and
  places: `type_block` never panics and gives the same result twice;
  replacing any part by `Unknown` never turns another part's fit into an
  error; a candidate typed in place gives the fit that typing the block with
  that part gives.
* Lowering: a place-typed value is lowered after its siblings and its scope
  position is unchanged; `E0315`; `slot_types()`; unchecked blocks produce
  no `E0301` or `E0315` at their parts or values; an editable argument still
  gives exactly one `E0307` for a `double`, a `char`, a literal and `i + 1`
  (`crates/b2c-lang/tests/types.rs:360-391`); a `members` field naming a
  symbol that is not a field is `E0207`; the SAST, diagnostic and golden
  snapshots of M2's examples are unchanged after W1.
* `b2c-catalog`: the forms on the right parts, `members` on a `struct` or
  `enum` variable, an unbound output variable refused, `default` refused on
  a pattern that is not a fixed type; `order` is the catalog order;
  `place_var` set exactly for place-typed blocks.
* `b2c-codegen` (W2): brace items are cast as §3.1 says; every case of §3.1,
  `{2.0}`, `{1e3}` and a negative `char` item included, compiles with
  `-fsyntax-only` under C++17 and C++20 with the *strict* warnings and no
  `-Wnarrowing`, also with `-funsigned-char`, on GCC 11 and 15 in CI.
* `b2c-core-wasm`: `update()` carries `slotTypes`; an expression slot's type
  is listed, a block's is not.

**TypeScript tests (Vitest).**

* `types/slots.ts`: every case of the slot fixture agrees, field by field.
* The checker: text is refused in `create [int]`'s value, in `set [score]` of
  an `int` variable, in an `int` parameter's argument and in `return` of a
  function that returns `double`; a `char` is allowed in `set` of a text
  variable; `int` into `double` and `double` into `int` are allowed; an
  unknown type and `error` connect; `set [score] to (…)` with `score` out of
  scope accepts text; a candidate is typed in place (dropping into the
  binding input of a two-input block with another value already there); a
  disabled candidate counts as empty and connects; M2's `any`, `number` and
  `field:` outputs behave as before. Left for W2, which brings the first
  place-typed block and the first `members` field: a place-typed block
  connects where no place type exists and is refused in an `int` place, and
  `chosenMember` refuses a field of another struct.
* Gestures: the keyboard listing treats the moved block as absent from its
  input; a faded block counts in its own input for other candidates and is
  replaced only for its own.
* Menus: a reference field with a pattern lists only symbols that fit, and
  keeps a chosen one that no longer does. W2: a `members` field lists the
  members of the bound type, and nothing while it is unbound.
* Start values: `int` to `double` swaps `0` for `0.0` (M2's test stays);
  `int` to a list removes `0`; a typed value is kept; undo and redo restore
  both changes as one step; a dropped value never changes a sibling's start
  value; an argument's start value follows a changed function.
* The cache: one typing per parent per drag in a 1,000-block workspace
  (counted with a spy); a new preview restarts it; it is gone after the
  drag.
* **No false refusal** (with the real core, `apps/desktop/src/editor/sync/`,
  skipped when the core is not built, as `testCore` does): for every example
  project, and each assignment project once it exists, a seeded sample of
  (value block, value input) pairs of one module whose parent is attached
  and enabled; wherever `candidateFit` is an error, the document with that
  connection made has an analyser error at the moved block or at that
  input. Parked and disabled parents are left out of this test: the editor
  refuses a wrong drop there that the analyser reports only once the stack
  is attached and enabled (ADR-0012). A second test attaches each refused
  parked parent and checks the error then.

**End to end** (Linux and Windows). A text block taken towards
`create [int]`'s value does not snap and snaps into `create [string]`'s;
changing the type swaps the start value and Ctrl+Z restores it; a value of
the wrong type does not snap into a call's argument, and the keyboard mover
does not offer that place.

## 10. Spec sections to change

Sections that ADR-0017 also changes are edited once in W1 for both
(types.md §10). The W1 rows were applied at the end of W1 (2026-10-25); the
W2, W4 and W5 rows wait for their work.

| Section | Change | Wave |
| --- | --- | --- |
| [03 §3.3](../../spec/03-block-language.md#33-block-shapes) | the checker compares a value with its input's expected type; a parked block's slots are typed from its own parts until M3-4, then from the analysis | W1 |
| [03 §3.4](../../spec/03-block-language.md#34-expression-slots) | brace lists where a place gives the type, a call's arguments being places; the start value of an expression shadow follows its input's expected type; literal suffixes do not come in M3 | W4 (braces), W1 (start values) |
| [03 §3.5.3](../../spec/03-block-language.md#353-type-checking-model-gradual) | "unification of generic parameters" becomes the one-pass rule and its limits | W1 |
| [03 §3.6](../../spec/03-block-language.md#36-symbols-and-scoping) | reference menus filtered by the field's pattern (W1); `members` menus (W2, with the first `members` field) | W1, W2 |
| [03 §3.7.8](../../spec/03-block-language.md#378-collections) | `list of` takes its type from its place (`E0315` without one, also in *for each*); casts inside braces | W2 |
| [03 §3.7.9](../../spec/03-block-language.md#379-types-structs-classes-enums) | `new … with` slots from the struct's fields (`member:`) | W2 |
| [03 §3.11.1](../../spec/03-block-language.md#3111-catalog-format) | place-typed outputs; how `check` accepts a value; the catalog order of parts (with ADR-0017's keys) | W1 |
| [04 §4.2](../../spec/04-user-interface.md#42-toolbox) | start values follow every input's expected type (`docs/spec/04-user-interface.md:170-177`); ADR-0019 changes *Your functions* in W3 | W1 |
| [06 §6.4](../../spec/06-compiler-pipeline.md#64-stage--lowering-to-the-semantic-ast) | lowering passes place types down; place-typed values are lowered last in their block; unchecked blocks | W1 |
| [06 §6.5](../../spec/06-compiler-pipeline.md#65-stage--names-and-scopes) | `slot_types()` beside `block_types()`; expected types and candidates in the checker; `set`'s exception as `assign` | W1 |
| [06 §6.6](../../spec/06-compiler-pipeline.md#66-stage--types-flow-checks-and-lints) | *Inference*: the one-pass rule replaces "Generic library signatures unify" | W1 |
| [06 §6.13](../../spec/06-compiler-pipeline.md#613-performance-in-the-editor) | the per-block cache's key includes the facts typing reads (with ADR-0016's changes) | W5 |
| [09 §9.2](../../spec/09-quality-and-delivery.md#92-testing-strategy) | the slot fixture, the no-false-refusal test, the typed drag and keyboard benchmarks | W1 |
| `docs/reference/diagnostics/analyser.md` | `E0315` (W1); `E0301`'s new message shapes; `E0207`'s wider meaning (W2) | W1, W2 |
