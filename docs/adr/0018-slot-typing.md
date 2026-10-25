# ADR-0018: Typed slots: what each input expects, worked out in one pass

* Status: Proposed (M3 wave W0, item L15; the points marked *owner to
  confirm* are still open)
* Date: 2026-10-24
* Builds on [ADR-0017](0017-open-type-representation.md): its types (§1,
  §2), type rules (§3) and catalog signatures (§7). Adds to its §7 three
  pattern forms, two catalog keys and the catalog order of a signature's
  parts, and says how a `check` class accepts a value (§2 below); ADR-0017
  is to take these in before it is accepted. Uses the seams of
  [ADR-0014](0014-extending-blockly.md) and the store reads of
  [ADR-0016](0016-preview-in-a-web-worker.md) §1 and §5 (no new export, no
  worker message). Keeps [ADR-0012](0012-parked-blocks.md) (parked code
  reports nothing) and [ADR-0013](0013-additive-format-keys.md) (no format
  change); its catalog changes are compatible ones
  ([ADR-0015](0015-block-upgrades-on-load.md)).

## Context

### What M3 needs

Typed slots are item L15 of M3's foundations
([10 §10.1](../spec/10-roadmap.md#m3-language-breadth-i),
[next steps §3.2](../plans/next-steps.md#32-scope)). With the type
representation (L14) they are the critical path of M3: every new block
depends on them ([next steps §3.5](../plans/next-steps.md#35-critical-path-and-risks)).

The specification asks for:

* phrase blocks whose inputs follow the static type of a container:
  `add (x) to [c]` takes what `c` holds
  ([03 §3.1](../spec/03-block-language.md#31-design-philosophy-few-blocks-much-power),
  [§3.7.8](../spec/03-block-language.md#378-collections));
* `list of (a) (b) ⊕`, whose type comes "from context" (§3.7.8), and brace
  lists, accepted "only where the expected type is known"
  ([03 §3.4](../spec/03-block-language.md#34-expression-slots));
* `new [Point] with x: (1) y: (2)`, whose slots "are generated from the
  fields" ([03 §3.7.9](../spec/03-block-language.md#379-types-structs-classes-enums));
* a connection checker "backed by the type system"
  ([03 §3.3](../spec/03-block-language.md#33-block-shapes)) under the
  gradual model: errors only when certain, unknown types connect
  ([03 §3.5.3](../spec/03-block-language.md#353-type-checking-model-gradual));
* "unification of generic parameters (`std::vector<T>` with
  `T = Student`)" (03 §3.5.3); "Generic library signatures unify"
  ([06 §6.6](../spec/06-compiler-pipeline.md#66-stage--types-flow-checks-and-lints));
* M3-4's test "a text value no longer snaps into a parked int `set`"
  (`docs/plans/m2-polish-plan.md:1411-1415`).

ADR-0017 fixed the types, the conversion rule, the classes and the pattern
language of signatures. It left to this ADR where and when a signature is
evaluated, when the editor binds a parent's variables, and which oracle
answers (ADR-0017 §6, §7).

The typed slots of the M3-D1 assignments
([next steps §3.3](../plans/next-steps.md#33-decisions-before-m3-starts);
block shapes as 03 §3.7 sketches them and the assignments' READMEs list
them, final in W2):

| Assignment | Slot | Its type comes from |
| --- | --- | --- |
| `grade_average`, `matrix_sum` | `add (mark) to [marks]`, `add (row) to [matrix]` | what the chosen list holds (`int`; `std::vector<int>`) |
| `dice_statistics` | `item (face) of [counts]`, `set item (face) of [counts] to (…)` | the map's key and value types |
| `student_records` | `new [Student] with name: (…) mark: (…)`, `[s] . [mark]` | the struct's fields |
| `word_count`, `student_records` | `for each [word] in (words)` | declares `word` with what the list holds |
| `bank_menu` | `return (…)` in a function that returns the enum `Action` | the function the block is in |
| all ten | `create [int] count = (0)`; `set [x] to (…)` | the type field; the variable's type |

### What the code does today

* **Bottom-up only.** The analyser lowers an input without knowing its
  place (`crates/b2c-lang/src/lower/expr.rs:126-168`), types it, and only
  then checks it against a type that each block's lowering works out by
  hand: `create` (`lower/stmt.rs:134-199`), `set` with its `char` exception
  (`:223-233`), call arguments (`lower/expr.rs:516-616`), `return`
  (`lower/stmt.rs:450-495`), counts and conditions
  (`lower/convert.rs:164-173`), all through `check_conversion`
  (`:90-140`). No block can take its type from its place.
* **The editor checks classes, not types.** `B2cConnectionChecker` works
  out the dragged block's type
  (`packages/blockly-ext/src/checker/output-type.ts:41-75`, or the analysis
  through the oracle, `apps/desktop/src/editor/services/symbols.ts:167-189`)
  and compares it with the input's catalog `check` class only
  (`checker.ts:75-94`, `conversion.ts:75-97`). So `create [int] x = ("hi")`
  and `set [score] to ("hi")` connect, and the analyser reports `E0301`
  afterwards. For `check`, the classes `bool`, `integer` and `number` mean
  "converts to `bool`, `int`, `double`" (`conversion.ts:87-92`,
  `lower/convert.rs:164-173`); `text` and `any` are sets.
* **When the rule applies.** Only while the user drags a block or moves it
  with the keyboard (`checker.ts:140`;
  `apps/desktop/src/editor/keyboard/targets.ts:113-130`), never for
  connections the program makes: loading, undo and redo
  (`checker.ts:10-14`) and paste (`editor/clipboard/insert.ts:255`).
* **Cost per move.** Blockly 12.5.1 asks the checker about every
  connection near the dragged one on each pointer move
  (`ConnectionDB.searchForClosest`, read in its compiled build);
  `canConnectWithReason` applies Blockly's own checks, distance included,
  before the type rule (`checker.ts:132-140`).
* **What the analysis tells the editor.** The type of each value block
  (`crates/b2c-lang/src/query.rs:348-412`; `blockTypes` in the preview,
  `crates/b2c-core-wasm/src/facade.rs:178-182`,
  `packages/b2c-core-wasm/src/api.ts:85-90`) and the symbols. An expression
  slot has no entry. Parked blocks are not lowered, so they have no types
  (ADR-0012; M3-4 at `docs/plans/m2-polish-plan.md:1411`).
* **Start values.** When a `create` block's type or a `set` block's
  variable changes, a value that is still the old type's start value
  becomes the new type's
  (`apps/desktop/src/editor/toolbox/reshape.ts:1-19`), recorded as an
  event of its own in the same group so that undo restores both
  (`:67-108`). A call from *Your functions* comes with an argument of each
  parameter's type (`toolbox/values.ts:1-27`).

### Constraints

* Gradual checking (03 §3.5.3): an error only when certain; an unknown
  type connects; a false refusal blocks the user.
* Every stage is pure and deterministic and never panics; one mistake
  gives one message ([06 §6.1](../spec/06-compiler-pipeline.md#61-overview)).
* The checker answers synchronously from the store. Under ADR-0016 the main
  thread keeps no analysis, and no answer from the worker reaches the
  canvas during a drag (ADR-0016 §1, §5).
* Blockly is extended only through ADR-0014's seams.
* W1 adds no blocks, and every golden file stays byte-identical
  (ADR-0017 §10).

## Options considered

**Where the editor gets a slot's expected type.**

1. **From the analysis only.** The preview lists each input's expected
   type and the checker reads it. One implementation. But every answer is
   one preview old: after the user changes `create [int]` to
   `[list of int]`, the slot keeps refusing lists until the next preview
   (the 50 ms debounce of
   [06 §6.13](../spec/06-compiler-pipeline.md#613-performance-in-the-editor)
   plus about 43 ms of `update()` at 1,000 blocks, ADR-0016's Context), and
   a block dropped since has no answer at all.
2. **Worked out in TypeScript from the canvas, with the analysis for what
   only it knows (chosen).** The editor evaluates the parent's signature
   on the blocks as they are now; the analysis supplies the types of
   symbols, of blocks the editor cannot type itself, and of expression
   slots. Answers are current. The cost is a second implementation of one
   function, held to the first by a generated fixture (Tests).
3. **A synchronous WebAssembly call per candidate.** The main instance
   keeps no analysis (ADR-0016 §1), so each call would serialise and
   analyse the canvas: tens of calls per pointer move. Rejected.

**How types flow between blocks.**

1. **One pass, local to each block (chosen).** A block's variables are
   bound from its own parts, left to right; a value's type flows up to its
   parent; only a block whose type comes from its place gets one from its
   parent. No variable outlives its block.
2. **Global unification** across blocks (an `auto` variable typed by a
   later `add (1) to [x]`). More programs typed, but an error can appear
   far from its cause, an answer depends on the whole program, and C++
   itself needs an initialiser for `auto`. The editor would need the same
   solver. Rejected.
3. **Bottom-up only.** No place types, so `list of` would need a type
   field of its own. Contradicts 03 §3.4 and §3.7.8. Rejected for M3; a
   `list of [type]` block can still come if the usability sessions ask
   for it.

**Start values that follow a slot's type.**

1. **Per-type catalog defaults,** filled in by catalog resolution.
   Resolution (stage ②) would need types, which the analyser (stage ⑤)
   computes. Rejected.
2. **The editor writes them (chosen),** as explicit tokens, when a block
   is built in the toolbox, when it lands, and when a field change changes
   what an input expects: M2's rule for `create` and `set`, generalised.

**What a refused drop stands for.**

1. **Every error the editor can predict,** including a list with nothing
   around it to give it a type (`E0315`). This refuses for missing
   information, which the user may be about to add.
2. **Only conversion and class errors between known types (chosen).**
   What M2 refuses, applied to expected types.

## Decision

### 1. Where a slot's expected type comes from

An input's expected type is its pattern (ADR-0017 §7) resolved against the
block's bound variables. Five sources feed it:

| Source | Pattern | Example |
| --- | --- | --- |
| A type field of the block | `T`, `field:NAME` | `create [list of int] scores = (…)`: `VALUE` expects `std::vector<int>` |
| A variable or value the block refers to | `C`, `elem(C)`, `index(C)`, `key(C)`, `symbol:NAME` | `add (x) to [marks]`: `x` expects what `marks` holds |
| The parameters or fields of a symbol a field names | `param:F`, `member:F` (new) | `average (scores)`: the first parameter's type; `new [Student] with mark: (…)`: `int` |
| The definition the block is in | `result` (new) | `return (…)` in a function that returns `Action` |
| The block's place | the output variable of a place-typed block | `list of (1) (2)` in a slot that expects `std::vector<int>` |

Within one block, fields bind before value inputs, each in catalog order,
and the first part that binds a variable decides it (ADR-0017 §7 rule 1).

### 2. Additions to ADR-0017's signatures

**Three pattern forms,** added to ADR-0017 §7's grammar:

```ebnf
pattern = …                 (* ADR-0017 *)
        | "param:" NAME     (* repeated input: the type of parameter i of the function chosen in field NAME *)
        | "member:" NAME    (* repeated input: the type of field i of the struct chosen in field NAME *)
        | "result" ;        (* what a `return` gives back here: the function's return type; int in main *)
```

* `i` is the part's index (`ARG2` is parameter 2). Each form resolves to
  nothing when the function or struct is not known or has fewer
  parameters or fields, and `result` also when the function returns
  `void` or the block is in no definition (parked, in the toolbox). A part
  whose pattern resolves to nothing accepts any value of its class
  (ADR-0017 §7 rule 3).
* They appear only on value inputs; `param:` and `member:` only on a
  repeated input. `param:` names a `symbol_ref` field; `member:` a `type`
  or `symbol_ref` field. The catalog refuses them elsewhere when it loads.

**Two keys.**

* `assign = true` on a value input: the value is assigned, not used to
  initialise, so a `char` also fits a `std::string`, as C++ allows
  (`s = 'a';` compiles, `std::string s = 'a';` does not;
  `crates/b2c-lang/src/typing.rs:22-27`). Used by `set` and *set item …
  of*. Default `false`.
* `members = "S"` on a `symbol_ref` field: the field lists the fields of
  the user type bound to `S` (in M4 also its methods) instead of the
  symbols in scope. `S`'s class must be `struct`. Used by `[s] . [mark]`,
  whose output is `symbol:MEMBER`.

**Place-typed blocks.** A block is *place-typed* when its `output` is a
bare variable that no field or input binds (in parts it appears only
inside a type function, or not at all): `list of` has `output = "C"` and
items with `type = "elem(C)"`. The catalog works this out when it loads.
Any other variable in `output` must be bound by a part that is not
optional; the catalog refuses a definition where it is not. So an
output's variable is never left unbound unless the block is place-typed
or an error has already been reported. This narrows ADR-0017's rule 1,
whose last step ("the type the block's place expects") applies only to
place-typed blocks.

**Order.** ADR-0017's `BlockSig` keeps `fields` and `inputs` in
`BTreeMap`s, ordered by name, while its rule 1 binds in catalog order.
`BlockSig` gains `order`: the parts that have a pattern, in catalog order.

**How `check` accepts a value.** For `check`, the classes `bool`,
`integer` and `number` keep their M2 meaning: they accept every value that
converts to `bool`, `int` or `double` without `Invalid`, so a `double` in
a count warns (`W0518`) and is not an error. `any` accepts every type but
`void`. Every other class, `text` included, is membership
(`Type::is_in`). In `where`, every class is membership. ADR-0017 §3's
table gives the members of `bool`, `integer` and `number`, which is right
for `where` but would make M2's warnings errors if `check` used it.

**Defaults.** An input whose pattern contains a variable or a type
function has no catalog `default`: one fixed token cannot fit every type
the input may expect. The editor fills such an input with a start value
(§7). The catalog refuses a `default` there when it loads.

The Rust additions (`crates/b2c-ir/src/sig.rs`, ADR-0017's module):

```rust
pub enum TypePattern {
    // ADR-0017: Any, Number, Type, Var, Lib, Fn, Field, Symbol
    Param(String),  // `param:F`
    Member(String), // `member:F`
    Result,         // `result`
}

pub struct BlockSig {
    // ADR-0017: vars, output, fields, inputs, headers, option_headers
    /// The fields and value inputs that have a pattern, in catalog order: the order they bind in.
    pub order: Vec<PartName>,
    /// The output's variable when no part binds it: the block takes its type from its place.
    pub place_var: Option<TypeVar>,
    /// `symbol_ref` fields that list the members of the user type bound to a variable.
    pub members: BTreeMap<String, TypeVar>,
}

pub enum PartName { Field(String), Input(String) }

pub struct InputSig {
    pub check: TypeClass,
    pub pattern: Option<TypePattern>,
    /// `assign = true`: assigned, not initialised (`char` fits `std::string`).
    pub assign: bool,
}

pub trait PartTypes {
    // ADR-0017: field, symbol
    fn param(&self, field: &str, index: usize) -> Option<Type>;
    fn member(&self, field: &str, index: usize) -> Option<Type>;
    fn result(&self) -> Option<Type>;
}

impl TypeEnv {
    /// ADR-0017's `resolve`, with the index of a repeated input's part (for `param:`, `member:`).
    pub fn resolve(&self, pattern: &TypePattern, parts: &dyn PartTypes, index: Option<usize>) -> Option<Type>;
}
```

`catalog.json` carries them: the pattern kinds
`{"kind": "param", "field": "FUNC"}`, `{"kind": "member", "field": "TYPE"}`
and `{"kind": "result"}`; per block `order`, `placeVar` and `members`; per
input `assign`.

### 3. The rule: typing one block

`type_block` types one block from its signature, the types of its parts,
the type its place expects and the facts about user types. The analyser
and the editor run the same rule (in Rust and in a TypeScript copy), and
nothing else decides an expected type.

```rust
// crates/b2c-lang/src/slots.rs (new), re-exported from lib.rs. It lives in b2c-lang
// because it calls `conversion` (typing.rs), which ADR-0017 keeps there.

/// What one part of a block is when the block is typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartType {
    /// Nothing chosen or connected, or a disabled block.
    Empty,
    /// `auto` in a type field.
    Auto,
    /// A value that takes its type from its place: typed after the block, with the part's
    /// expected type as its place.
    FromPlace,
    /// A known type. `Type::Error` binds nothing and fits everything.
    Known(Type),
    /// Not known to the caller (the editor, for something the analysis has not seen).
    Unknown,
}

/// The parts of the block being typed. A repeated input is asked for part by part (`ITEM2`).
pub trait BlockParts {
    fn field(&self, name: &str) -> PartType;
    fn input(&self, part: &str) -> PartType;
    /// How many parts the repeated input `base` has now.
    fn count(&self, base: &str) -> usize;
    /// `param:F`: parameter `index` of the function chosen in field `field`.
    fn param(&self, field: &str, index: usize) -> PartType;
    /// `member:F`: field `index` of the struct chosen in field `field`.
    fn member(&self, field: &str, index: usize) -> PartType;
    /// `result`: what a `return` gives back where the block is.
    fn result(&self) -> PartType;
}

/// How a part fits what its block expects of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotFit {
    /// Nothing to check: an empty, unknown or erroneous part, a value typed from its place,
    /// or an expectation that does not resolve.
    Unknown,
    /// It fits by this conversion (never `Invalid`); `Narrowing` and `BoolNumber` warn.
    Converts(Conversion),
    /// Its type is outside the class the part requires.
    OutsideClass,
    /// It cannot fit: a wrong shape for the pattern, or `Conversion::Invalid`.
    Invalid,
}

impl SlotFit {
    /// `OutsideClass` and `Invalid`: what the analyser reports and the editor refuses.
    pub const fn is_error(self) -> bool;
}

/// A value input part's expected type and how its value fits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputTyping {
    /// `None`: nothing gives this input a type. `Some(Type::Error)`: what would give it one
    /// is missing or already wrong (step 4).
    pub expects: Option<Type>,
    pub fit: SlotFit,
}

/// Which part bound a variable, for messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binder { Field(String), Input(String), Place }

/// The result of typing one block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockTyping {
    /// Each bound variable, its type and the part that bound it.
    pub bound: BTreeMap<TypeVar, (Type, Binder)>,
    /// Every value input part the block has, by part name.
    pub inputs: BTreeMap<String, InputTyping>,
    /// Every `type` and `symbol_ref` field that has a pattern.
    pub fields: BTreeMap<String, SlotFit>,
    /// What each `symbol_decl` field with a pattern declares (`Type::Error` if it does not resolve).
    pub declares: BTreeMap<String, Type>,
    /// The block's value type; `None` for `any` and `number` outputs, which keep their own
    /// rules, and for an output that does not resolve.
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

/// `conversion` where a value is assigned (`assign = true`): also `char` to `std::string`,
/// as `Widening`.
pub fn assignment(from: &Type, to: &Type) -> Conversion;

/// Whether an input whose `check` is `class` accepts a value of type `ty` (§2); `None` when
/// that is not known (a user type the facts do not know).
pub fn check_accepts(class: TypeClass, ty: &Type, facts: &dyn TypeFacts) -> Option<bool>;
```

The steps, normative for both copies:

1. **Bind from fields,** in `order`. A type field holding a known type and
   a `symbol_ref` field naming a symbol of known type bind their pattern
   with ADR-0017's `TypeEnv::bind`. `Empty`, `Auto`, `FromPlace`,
   `Unknown` and `Known(Error)` bind nothing (ADR-0017 rule 2), and so
   does a type outside the variable's class.
2. **Bind from value inputs,** in `order`, a repeated input part by part
   (`ITEM0`, `ITEM1`, …), the same way.
3. **Bind from the place.** A place-typed block binds `place_var` to its
   place's type, if it has one. When that type is outside the variable's
   class, `place` is `OutsideClass` and the variable stays unbound.
4. **Resolve and check.** Every pattern is resolved against what is bound
   (`TypeEnv::resolve`, with `BlockParts` behind `PartTypes` answering
   `field:`, `symbol:`, `param:`, `member:` and `result`). Then:
   * an `Empty`, `Auto`, `FromPlace`, `Unknown` or `Known(Error)` part
     fits `Unknown`;
   * an input's `expects` is its resolved pattern. A pattern that does not
     resolve gives `Some(Type::Error)` when what it depends on is missing
     or already wrong: a part that could bind its variable, or the answer
     of `param:`, `member:` or `result`, is `Empty`, `Unknown` or
     `Known(Error)`, or is a part whose own fit is an error. It gives
     `None` when nothing could give it a type: no pattern, or only `Auto`
     and `FromPlace` parts to bind from. `Error` fits everything, so a
     choice still to make, or a mistake already reported, never leads to
     a second message;
   * a value input holding a known type `t` is `OutsideClass` when its
     `check` does not accept `t` (§2) or its own binding failed on a
     class; `Invalid` when its own binding failed on a shape (a
     `std::deque<int>` for `std::vector<T>`); otherwise, when `expects` is
     a type `e`, it is `conversion(t, e)` (`assignment(t, e)` with
     `assign`), which gives `Invalid` or `Converts(…)`; when `expects` is
     `None`, `Unknown`;
   * a `type` field is `OutsideClass` when its variable's class does not
     hold its type, else `Converts(Same)`. The analyser does not report
     this: the catalog reports a type outside a field's domain
     (`E0607`, ADR-0017 §4);
   * a `symbol_ref` field is judged like a type field for the class; when
     an earlier part already bound its variable to another type, it is
     `Invalid` (a reference is never converted, ADR-0017 rule 3);
   * `declares` holds each `symbol_decl` field's resolved pattern, or
     `Type::Error` when it does not resolve (ADR-0017 rule 4);
   * `output` is the resolved `output`, or `None` for `any` and `number`,
     which keep their M2 rules in both copies (ADR-0017 rule 5).

**Limits, and what they guarantee.**

* **One pass, no backtracking.** Each part is bound or checked once. A
  variable, once bound, is never bound again; a later part that disagrees
  is a mismatch at that part, never at the earlier one.
* **Deterministic and bounded.** The result depends only on the arguments.
  The work is linear in the block's parts, and each pattern is at most 8
  levels deep (ADR-0017 §7).
* **Unknown is never worse than known.** Replacing any part by `Unknown`
  never turns another part's fit from a non-error into an error. This is
  the gradual rule in one sentence, and a property test checks it.
* **Variables are local to one block.** A variable is bound and used
  within one block's typing and never outlives it. Between blocks, types
  flow only up (a value's type to its parent) and down (an expected type
  into a place-typed block); nothing is unified across blocks.

### 4. In the analyser

* **Four steps per block.** A block with a signature is lowered in this
  order:
  1. its fields are read and its value inputs lowered, in catalog order,
     as now, except that an input holding a place-typed block (or, with
     W4's slot editor, a brace list `{…}`) is put off and is `FromPlace`;
  2. `type_block` runs, with the place type the parent passed down;
  3. the inputs put off are lowered, each with its resolved expected type
     as its place;
  4. each part's fit is reported through the existing checks
     (`check_conversion` and its messages, `lower/convert.rs:90-140`), so
     M2's blocks keep their wording.

  The caller keeps the scope around the four steps: `create` runs them
  inside its pending window, where the new variable's own name is hidden
  (`lower/stmt.rs:153-156`). So the order of lowering changes but no
  position in the scope index does. Only the inputs that are put off move
  in the order of diagnostics, and no block puts one off before W2.
* **`value_block` takes a place:**
  `fn value_block(&mut self, block: &Block, place: Option<&Type>) -> Expr`
  (`lower/expr.rs:173`). A place-typed block whose place is `None` has
  type `Error` and is reported (`E0315`); one whose place type is outside
  its class has type `Error` and is reported (`E0301`, §6); one whose
  place is `Error` has type `Error` and nothing more is said. Any other
  block ignores its place.
* **Part types.** Reading a field or an input reports what it reports
  today (`E0201` to `E0204` for references, `E0430` for an empty required
  input, a damaged type field at `lower/mod.rs:744-771`) and then gives
  `Known(Error)`. A disabled block in an input is `Empty`. `result` comes
  from the item being lowered (`ItemCtx`, `lower/mod.rs:118-128`): the
  function's return type (`Empty` for `void`), `int` in `main`; M3-4's
  `ItemCtx::Parked` gives `Unknown`.
* **Expression slots** take a place too. A brace list at the top of a slot
  is place-typed, and inside a slot a call's arguments get their
  parameters' types as places. This lands with W4's slot editor, which
  brings brace tokens; W1's parser has none.
* **Braces and conversions.** Inside braces C++ refuses a narrowing
  conversion of a constant that does not fit and warns about any other
  narrowing conversion, `int` to `double` included. Checked with g++ 11,
  12, 13 and 14 in C++17 and C++20 (`-Wall -Wextra -Wpedantic`): `{1.5}`
  for a `std::vector<int>` is an error; `{d}` with a `double d` and `{i}`
  with an `int i`, for a `std::vector<int>` and a `std::vector<double>`,
  are `-Wnarrowing` warnings; `{1, 2.5}` for a `std::vector<double>`,
  `{65}` for a `std::vector<char>`, `{1}` for a `std::vector<bool>` and
  `{static_cast<double>(i)}` are clean. Designated initialisers
  (`P{.x = d}`) behave the same (g++ 11 and 14, C++20). So codegen writes
  each item of a brace list (`list of`, `new … with`, `{…}`) whose type is
  not the element type as `static_cast<T>(item)`, unless it is a literal
  whose value the element type holds exactly. The analyser applies §3's
  conversion as for a variable: narrowing warns (`W0518`), `bool` and
  numbers mixed warn (`W0519`), widening is silent. g++ never meets a
  narrowing conversion inside braces (*owner to confirm*).
* **Blocks the catalog kept unchecked** (made by a newer catalog, unknown,
  too old to upgrade): `analyze_checked` (`crates/b2c-lang/src/lib.rs:104-135`)
  leaves out `E0301` and `E0315` on them as it leaves out `E0430`, and
  their value has type `Error`. Their definition is not this catalog's.
* **M2's blocks in W1.** Their catalog entries gain patterns (§10), and
  their lowering takes the expected type from `type_block` instead of
  working it out by hand. *change by*, the conditional value and
  arithmetic keep their own rules and get no pattern. Their SAST and
  diagnostic snapshots must not change.
* **What the analysis exports.** `Analysis::slot_types()` gives the type of
  each expression slot that was lowered, by block and input, recorded by
  `Lowerer::slot` (`lower/expr.rs:618`). `block_types()` stays as it is.
* **Parked blocks** stay unlowered until M3-4 (W5). M3-4 lowers them in its
  sandbox with the same code, so they get block and slot types, and they
  still report nothing (ADR-0012).

### 5. In the editor

**What crosses.** The JSON of `update()` gains `slotTypes`, the analysis's
`slot_types()`, next to `blockTypes`. It travels inside the worker's
`updateJson` unchanged (ADR-0016 §3), and the store keeps it with the
preview. There is no new export, no worker message and no IPC change.
Expected types do not cross: the editor works them out (option 2).

```ts
// packages/b2c-core-wasm/src/api.ts
export interface PreviewResult {
  // … as now; blockTypes with ADR-0017's TypeJson
  /** The static type of each expression slot the analysis lowered, by block ID, then input name. */
  slotTypes: Record<string, Record<string, TypeJson>>;
}
```

**The TypeScript copy** of §3 sits beside ADR-0017's `types/` module:

```ts
// packages/blockly-ext/src/types/slots.ts (new)
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
  result(): PartType;
}

/** `b2c_lang::SlotFit`: a conversion that fits, `invalid`, `outsideClass` or `unknown`. */
export type SlotFit = StaticConversion | 'outsideClass' | 'unknown';

export interface BlockTyping {
  readonly bound: ReadonlyMap<string, StaticType>;
  readonly inputs: ReadonlyMap<string, { readonly expects: StaticType | null; readonly fit: SlotFit }>;
  readonly fields: ReadonlyMap<string, SlotFit>;
  readonly declares: ReadonlyMap<string, StaticType>;
  readonly output: StaticType | null;
  readonly place: SlotFit;
}

export function typeBlock(
  def: BlockDefJson,
  parts: BlockParts,
  place: StaticType | null,
  facts: TypeFactsTs,
): BlockTyping;
/** `invalid` and `outsideClass`. */
export function isFitError(fit: SlotFit): boolean;
/** §7's start values; null for a type that has none. */
export function startTokens(type: StaticType): readonly TokenJson[] | null;
```

**The Blockly side** (`packages/blockly-ext/src/checker/slots.ts`, new)
reads a block's parts from the canvas:

| Part | `PartType` |
| --- | --- |
| A type field | its value's term (ADR-0017 §5), `auto`, or `empty` |
| A `symbol_ref` field | the symbol's type from the preview's `symbols`; `empty` when nothing is chosen; `unknown` when the preview does not know the symbol |
| An input holding a block | `empty` for a disabled block or one of Blockly's insertion markers; `fromPlace` for a place-typed block; otherwise its value type: from its own signature (recursively), from M2's rules for `any` and `number` outputs (`output-type.ts`), or from the preview's `blockTypes`; `unknown` when none answers |
| An input holding an expression shadow | one literal: its type; one reference: the symbol's type; a brace list: `fromPlace`; other tokens: the preview's `slotTypes`, else `unknown` |
| An empty input | the type of its catalog default tokens, or `empty` |
| `param:F`, `member:F` | the parameter's or field's symbol type, from the preview's `symbols` |
| `result` | up the parents to the root: `int` under *when program starts*, the `RETURNS` type under *define* (`empty` for `void`), `unknown` on the canvas or in the toolbox |

```ts
// packages/blockly-ext/src/services.ts: TypeOracle gains three methods
export interface TypeOracle {
  /** As in M2: the analysis's type of a value block, then the type of the symbol it names. */
  outputTypeOf(block: Blockly.Block): StaticType | null;
  /** The analysis's type of an expression slot (`slotTypes`), or null. */
  slotTypeOf(blockId: string, input: string): StaticType | null;
  /** A symbol of the latest analysis, or null. */
  symbol(symId: string): SymbolInfo | null;
  /** What the type rules need to know about user types, from the latest analysis. */
  facts(): TypeFactsTs;
}

// packages/blockly-ext/src/checker/slots.ts (new)
/** Each block's typing and value type, for one gesture (a drag, a keyboard move's listing). */
export class SlotCache {
  constructor(oracle: TypeOracle);
}
/** A block's typing from its parts on the canvas now; null for a block outside the catalog. */
export function blockTyping(block: Blockly.Block, oracle: TypeOracle, cache?: SlotCache): BlockTyping | null;
/** The block's value type, 'fromPlace' for a place-typed block, or null when not known. */
export function valueType(
  block: Blockly.Block,
  oracle: TypeOracle,
  cache?: SlotCache,
): StaticType | 'fromPlace' | null;
/** How `child` would fit in `parent`'s value input `input`, typed as if it were there. */
export function candidateFit(
  parent: Blockly.Block,
  input: string,
  child: Blockly.Block,
  oracle: TypeOracle,
  cache?: SlotCache,
): SlotFit;
/** The running gesture's cache, which the dragger and the keyboard mover set and clear. */
export function currentSlotCache(): SlotCache | null;
export function setCurrentSlotCache(cache: SlotCache | null): void;
```

**Candidates.** For a drag or a keyboard move that would put `child` into
`parent`'s input `I`, `candidateFit` types `parent` as if `child` were in
`I`:

* a place-typed `child`: `I` is `fromPlace`, the parent's typing gives
  `I`'s expected type `e`, and `child` is typed with the place `e`; the
  answer is `child`'s `place`;
* any other `child`: `I` holds `child`'s value type (`unknown` when it is
  not known), and the answer is `I`'s fit.

A place-typed parent gets its own place the same way from its parent, up
the chain. This is what the analyser computes once the drop is made,
first-binding-wins order included, so the editor refuses a drop only where
the analyser would then report an error at that input.

**Speed.** Blockly asks only about connections within the snap radius,
after its own distance check (Context), so each move types a few nearby
parents. A `SlotCache` keeps each block's typing and value type for one
gesture: `B2cBlockDragger` (`apps/desktop/src/editor/sync/drag.ts:348-402`)
makes one when a drag starts, after Blockly has taken the block out, and
drops it when the drag ends; the keyboard mover makes one per listing of
targets (`keyboard/targets.ts`). Under ADR-0016 §5 no preview is applied
during a drag, so the cached answers stay valid; the cache also notes the
preview it was made from and starts again if that changes. Types parsed
from a preview are kept per preview object, as `symbolsById` keeps symbols
(`services/symbols.ts:41-51`). Outside a gesture, a typing is computed when
something asks (a menu, a tooltip, a start value), for one block. No pass
over the canvas runs on an edit.

**Staleness.** The analysis's answers (symbol types, `blockTypes`,
`slotTypes`) are those of the last applied preview, as in M2. A block
edited since is typed from the canvas, except for what only the analysis
knows (a symbol declared since, an expression typed since), which is
`unknown`, or one preview old, until the next answer. A refusal in that
window ends with the next answer and never changes the document.

### 6. Prevented or reported

**The editor prevents** what is certain:

* **Drops.** The checker refuses a pointer drag or a keyboard move when
  `candidateFit` is `invalid` or `outsideClass`, and allows everything
  else: a narrowing or a `bool`/number mix (the analyser warns), an
  unknown type, a place-typed block with no place type (the analyser
  reports `E0315`). As in M2, a refused place is simply not offered: no
  snap, no preview, not listed for the keyboard. M2's `conversionAllowed`
  (`conversion.ts:75-97`) becomes the case of a parent without patterns.
* **Menus.** A `symbol_ref` field with a pattern lists the symbols in scope
  whose type binds that pattern on its own (class and shape: *add … to*
  lists the lists, sets, queues and stacks; *item … of* the lists and
  maps), without looking at the block's inputs, which are checked against
  the field and not the reverse. A field with `members = "S"` lists the
  members of `S`'s type, and nothing while `S` is unbound. A reference
  already chosen stays shown, as in M2
  (`packages/blockly-ext/src/fields/symbol-ref.ts:12-14`). Type fields
  offer the types their pattern allows (ADR-0017 §7). M2's `kinds` filters
  stay for M2's blocks.
* **Program connections** stay unchecked: loading, undo, redo and paste
  (`checker.ts:10-14`, `clipboard/insert.ts:255`) never apply the type
  rule, so a project, an undo step or a clipboard payload is never refused
  for a type the analyser reports (Blockly throws when a saved connection
  is refused).

**The analyser reports** everything else, once, at the part that does not
fit:

| Situation | Code | Example message |
| --- | --- | --- |
| A value that does not convert to what its slot expects | `E0301` | *"`marks` is a list of int, so the item must be a whole number, but this is text."* |
| A value or a variable outside the part's class | `E0301` | *"`score` is a whole number, but this block needs a list, a set, a queue or a stack."* (ADR-0017 §9) |
| A place-typed block in a place of another kind | `E0301` | *"A list can't go where a whole number is needed."* |
| A place-typed block where nothing gives a type (*print*, a `create` set to `auto`) | `B2C-E0315` (new) | *"This list doesn't say what it holds, and nothing around it does. Put it in a* create *block with a list type, or give it to a list variable."* |
| A member the type does not have | `E0207` | *"`speed` is not a field of `Student`."* |
| Narrowing; true/false mixed with numbers | `W0518`, `W0519` | as in M2 |

* `B2C-E0315` *No type from its place* is the only new code: everything
  else is a conversion or class error (`E0301`) or ADR-0017's `E0313` and
  `E0314`. It lands with its first use, `list of` in W2, with its entry in
  `docs/reference/diagnostics/analyser.md`. The next free code is
  `E0316`.
* When another part bound the expected type, the message names it
  (*"`marks` is a list of int, so …"*, from `BlockTyping::bound`). Types
  are named as ADR-0017 §9 says: friendly labels, and M2's phrases for
  primitives.
* They show where every analyser diagnostic shows: the block's badge, the
  Problems panel and the slot's tooltip (03 §3.4,
  [04 §4.4](../spec/04-user-interface.md#44-diagnostics-ux)).
* Nothing new is reported for an empty input (`E0430` as now), an `auto`
  that binds nothing (`E0309` as now) or a part whose type is already
  `Error`.

### 7. Start values

M2's rule (`toolbox/reshape.ts`) becomes general:

* **Values.** `startTokens(type)` gives `0`, `0.0`, `true`, `'a'` and `""`
  for `int`, `double`, `bool`, `char` and `std::string` (M2's,
  `toolbox/values.ts:11-27`), and none for containers, maps, structs,
  enums, streams and errors in M3. A `create` block of a list type with no
  value declares an empty list (`std::vector<int> scores;`); W4 may add
  `{}` once slots accept braces.
* **In the toolbox,** each empty value input with a known expected type
  gets its start value when the flyout block is built (presets and chosen
  references count), as M2 does for call arguments.
* **On a field change** by the user (a type or `symbol_ref` field, on the
  canvas or in the flyout), each value input of that block whose expected
  type changed, and recursively each item of a place-typed block in such
  an input, gets the new type's start value while it still holds exactly
  the old type's. When the new type has none, the start value is removed
  and the input left empty. A value the user typed, a block, a draft and
  an unknown type are left alone. A connection never triggers this, so
  dropping a value never rewrites a sibling (*owner to confirm*).
* **On landing** (a block from the toolbox dropped on the canvas,
  `toolbox/landing.ts`), the same rule runs with the expected types of
  the block's place, where `result` first becomes known.
* Each replacement is one `StartValueChange` event
  (`toolbox/reshape.ts:67-108`) in the group of the change that caused it,
  so one undo restores both and redo makes both again.

### 8. Undo, copy and paste, parked stacks

* **Undo and redo** change only the canvas, and every expected type is
  computed from the canvas, so they need nothing more. A redo whose
  connection the types would now refuse still works (§6), and the analyser
  reports it; start-value changes come back with the change that caused
  them (§7).
* **Copy and paste** carry no types. Pasted blocks are typed where they
  land, after their references are bound again by name and kind
  ([05 §5.12](../spec/05-project-format.md#512-clipboard-format)). A
  reference that binds to a variable of another type is reported by the
  analyser, and start values are not swapped.
* **Parked stacks.** Until M3-4 (W5), the editor types a parked block from
  its own parts: type fields, references to symbols the analysis knows
  (the parked scope, ADR-0012 point 5), nested blocks. A variable created
  inside the parked stack is `unknown`, so slots that depend on it accept
  anything, and `result` is `unknown`. Once M3-4 lowers parked stacks,
  their symbols and types come from the analysis as for attached blocks,
  and M3-4's test holds for variables of the stack too. Parked code still
  reports nothing (ADR-0012): a mismatch there shows as a refused drop and
  is reported once the stack is attached.
* **Disabled blocks** bind nothing in their parent. The editor types their
  own slots from their parts like any other block's; the analyser does not
  lower them
  ([06 §6.4](../spec/06-compiler-pipeline.md#64-stage--lowering-to-the-semantic-ast)).

### 9. Formats and versions

No change to the project format, the clipboard format, the IPC contract,
the worker protocol or the scope index. The catalog's additions ride in
catalog 1.3.0 with ADR-0017's: no block's version changes and no file
changes, so they are compatible (ADR-0015). The JSON of `update()` gains
`slotTypes`; the core and the editor ship in one build, which the build
guard already enforces (ADR-0016 §8). No upgrade path is needed.

### 10. W1: contracts first, then the packages

**Contracts,** in ADR-0017's contracts pull request:

* `crates/b2c-ir/src/sig.rs`: §2's additions;
* `crates/b2c-lang/src/slots.rs` complete, with its unit tests and the
  writer of the slot fixture (Tests), since both copies are checked
  against it;
* `Analysis::slot_types()` and `Preview::slot_types` (the types; the
  packages fill them);
* the TypeScript declarations: `PreviewResult.slotTypes`;
  `types/slots.ts` and `checker/slots.ts` (types only); `TypeOracle`'s
  new methods (`packages/blockly-ext/src/services.ts:79-82`).

**Packages** (ADR-0017 §10's, each with this ADR's part):

1. **`b2c-catalog`:** the forms and keys, `order`, `place_var`, the load
   checks of §2, and patterns on M2's blocks:
   * `var.declare`: `where = { T = "value" }`; `TYPE`, `NAME` and `VALUE`
     `type = "T"`;
   * `var.set`: `where = { T = "value" }`; `VAR` and `VALUE` `type = "T"`,
     `VALUE` `assign = true`;
   * `func.call`, `func.call_stmt`: `ARG` `type = "param:FUNC"`;
   * `func.return`: `VALUE` `type = "result"`.
2. **`b2c-lang`:** the four steps, `value_block`'s place, part types,
   `slot_types()`, `analyze_checked`'s filter, M2's blocks through
   `type_block` with unchanged snapshots.
3. **`b2c-codegen`:** nothing in W1; the brace casts come with `list of`
   in W2.
4. **`b2c-core-wasm`:** `slotTypes` in `update()`.
5. **TypeScript:** `types/slots.ts` against the fixture; `checker/slots.ts`;
   `B2cConnectionChecker`'s type rule on `candidateFit`; the reference
   menus; the cache in `B2cBlockDragger` and the keyboard mover; the
   oracle answering `slotTypeOf`, `symbol` and `facts` from the preview
   (`apps/desktop/src/editor/services/symbols.ts`); start values
   (`toolbox/reshape.ts`, `values.ts`, `landing.ts`, `plugin.ts`).

W2 then writes its blocks as signatures (with the casts of §4 and
`E0315`), W4 brings brace lists in slots, and W5's M3-4 lowers parked
stacks.

## Consequences

* **Easier.** A block's slots are described once, in its catalog entry, and
  the analyser, the checker, the menus and the start values read the same
  rule. Phrase blocks over every container, struct construction, typed
  calls and returns need no hand-written typing in Rust or TypeScript. The
  editor refuses wrong drops into `create`, `set`, call arguments and
  `return`, which M2 let through.
* **Harder.** Two copies of `type_block` must agree; the fixture and the
  no-false-refusal test hold them together, so a disagreement shows as a
  failing test rather than in use. Lowering is no longer purely left to
  right: place-typed values come last in their block. For what only the
  analysis knows, the editor can be one preview behind.
* **Visible changes in W1.** The editor refuses, in M2's typed inputs,
  drops that the analyser reported as errors (*owner to confirm*). Nothing
  else visible changes, and every golden file stays byte-identical.
* **Crates and packages.** `b2c-ir` (`sig.rs`); `b2c-catalog` (schema,
  definitions, export, TOML); `b2c-lang` (`slots.rs`, lowering, `query.rs`,
  `lib.rs`, codes, messages); `b2c-codegen` (brace casts, W2);
  `b2c-core-wasm` (`facade.rs`); `packages/b2c-core-wasm` (`api.ts`);
  `packages/blockly-ext` (`types/`, `checker/`, `fields/symbol-ref.ts`,
  `services.ts`); `packages/catalog-gen`; `apps/desktop/src/editor`
  (`services/`, `toolbox/`, `sync/drag.ts`, `keyboard/targets.ts`).
* **Performance.** In Rust, one `type_block` per block with a signature,
  linear in its parts, inside the analysis that `update()` already runs:
  no extra pass and no cache. In TypeScript, a few typings per pointer
  move, cached per gesture, and one per field change. None of this is
  measured yet. W1's A/B job compares `native.pipeline.preview`,
  `webview.preview-1000.p95`, `webview.edit-1000.p95-all` and
  `webview.drag-5000.frame-p95-all`, and W1 adds a drag benchmark that does
  reach typed slots (Tests), since `drag-5000` drags a definition across
  empty canvas and never previews a place
  (`docs/plans/m2-polish-plan.md:1097`). The per-block cache of 06 §6.13
  (M5) must key a block's lowering on the facts its typing reads (the
  symbols, signatures and struct fields it refers to), not only on its own
  structure.
* **Security** ([spec 08](../spec/08-security.md)). Slot typing reads only
  the document, the catalog and the analysis. It adds no input and puts no
  text into C++: a cast is a typed CAST node printed from `CType`
  (ADR-0017 §8,
  [06 §6.8.2](../spec/06-compiler-pipeline.md#682-typed-text-parse-dont-validate)).
  The TypeScript side parses every type with ADR-0017's `parseType` (fresh
  objects, no prototype keys, bounded), keeps `slotTypes` in `Map`s keyed
  by block ID, and never throws into Blockly: whatever it cannot read is
  `unknown`, which connects. Every walk is bounded: patterns by 8 levels,
  the chains of place types and value types by the 128 levels of
  [05 §5.6](../spec/05-project-format.md#56-validation-limits)
  (`output-type.ts:19`). The analyser's fuzz targets and its never-panics
  test over the security suite cover the new lowering.

### Spec sections to change

* [03 §3.3](../spec/03-block-language.md#33-block-shapes): the checker
  compares a value with its input's expected type; a parked block's slots
  are typed from its own parts until M3-4, then from the analysis.
* [03 §3.4](../spec/03-block-language.md#34-expression-slots): brace lists
  where a place gives the type, a call's arguments being places; literal
  suffixes do not come with typing in slots in M3 (ADR-0017 has no
  unsigned, `long` or `float` type for them); the start value of an
  expression shadow follows its input's expected type.
* [03 §3.5.3](../spec/03-block-language.md#353-type-checking-model-gradual):
  "unification of generic parameters" becomes the one-pass rule of §3 and
  its limits.
* [03 §3.6](../spec/03-block-language.md#36-symbols-and-scoping): reference
  menus filtered by the field's pattern; `members` menus.
* [03 §3.7.8](../spec/03-block-language.md#378-collections): `list of` takes
  its type from its place (`E0315` without one); conversions inside
  braces.
* [03 §3.7.9](../spec/03-block-language.md#379-types-structs-classes-enums):
  `new … with` slots from the struct's fields (`member:`).
* [03 §3.11.1](../spec/03-block-language.md#3111-catalog-format): the forms
  `param:`, `member:` and `result`; `assign`; `members`; place-typed
  outputs and the rule for other output variables; how `check` accepts a
  value; no `default` on an input whose pattern has a variable; the
  catalog order of parts.
* [04 §4.2](../spec/04-user-interface.md#42-toolbox): start values follow
  every input's expected type (the paragraph at
  `docs/spec/04-user-interface.md:170-177`).
* [06 §6.4](../spec/06-compiler-pipeline.md#64-stage--lowering-to-the-semantic-ast):
  lowering passes place types down; place-typed values are lowered last in
  their block.
* [06 §6.5](../spec/06-compiler-pipeline.md#65-stage--names-and-scopes):
  `slot_types()` beside `block_types()`; how the checker uses expected
  types and candidates; `set`'s exception as `assign`.
* [06 §6.6](../spec/06-compiler-pipeline.md#66-stage--types-flow-checks-and-lints):
  *Inference*: the one-pass rule replaces "Generic library signatures
  unify".
* [06 §6.13](../spec/06-compiler-pipeline.md#613-performance-in-the-editor):
  the per-block cache's key includes the facts typing reads.
* [09 §9.2](../spec/09-quality-and-delivery.md#92-testing-strategy): the
  slot fixture, the no-false-refusal test, the typed drag benchmark.
* The diagnostics reference (`docs/reference/diagnostics/analyser.md`):
  `E0315` (W2), `E0301`'s new message shapes, `E0207` for members.

## Tests

**Rust.**

* `b2c-lang` `slots.rs`, by table: each step of §3; first binding wins;
  `Auto`, `Empty`, `Unknown` and `Error` bind nothing; a class and a shape
  mismatch; a pattern that does not resolve accepts its class; a
  place-typed block with a place, without one and with one outside its
  class; `expects` is `Error`, not `None`, when its binder is empty or
  wrong (so `set [ ] to (list of …)` gives only `E0430`, never `E0315`);
  `assign` (`char` into `std::string`); `param:`, `member:` and
  `result`, also out of range and for `void`; repeated inputs by index;
  the `check` classes `bool`, `integer` and `number` by conversion.
* Property tests (proptest) over generated signatures, part types and
  places: `type_block` never panics and gives the same result twice;
  replacing any part by `Unknown` never turns another part's fit into an
  error; a candidate typed in place gives the fit that typing the block
  with that part gives.
* **The slot fixture.** A `b2c-lang` test writes
  `packages/blockly-ext/src/types/slot-fixture.json`: hand-written cases
  for every rule above, and for each catalog block with a pattern a seeded
  sample of part combinations (at most 50 per block), each with its
  signature as `catalog.json` writes it, its parts, place and facts, and
  the full `BlockTyping`. It is committed, and CI fails when it is stale,
  as for `catalog.json`.
* `b2c-lang` lowering: a place-typed value is lowered after its siblings,
  and its recorded scope position is unchanged; `E0315`; `slot_types()`;
  `analyze_checked` leaves out `E0301` and `E0315` on unchecked blocks;
  the SAST, diagnostic and golden snapshots of M2's examples are
  unchanged after W1.
* `b2c-catalog`: the load checks (the forms on the right parts, `members`
  on a `struct` variable, an unbound output variable refused, `default`
  refused on a variable pattern); `order` is the catalog order;
  `place_var` set exactly for place-typed blocks; the export test
  regenerates `catalog.json`.
* `b2c-codegen` (W2): brace items are cast where §4 says; the generated
  code of every brace case compiles with `-fsyntax-only` under C++17 and
  C++20 with the *strict* warnings and no `-Wnarrowing`, on GCC 11 and 15
  in CI.
* `b2c-core-wasm`: `update()` carries `slotTypes`; an expression slot's
  type is listed, a block's is not.

**TypeScript (Vitest).**

* `types/slots.ts`: every case of the slot fixture agrees.
* The checker: text is refused in `create [int]`'s value, in
  `set [score]` of an `int` variable, in an `int` parameter's argument and
  in `return` of a function that returns `double`; a `char` is allowed in
  `set` of a text variable; `int` into `double` and `double` into `int`
  are allowed (the second warns); an unknown type and `error` connect; a
  candidate is typed in place (dropping into the binding input of a
  two-input block with another value already there); a place-typed block
  connects where no place type exists and is refused in an `int` place;
  M2's `any`, `number` and `field:` outputs behave as before.
* Insertion markers count as empty, including a drag whose preview
  replaces a block in another input of the same parent.
* Menus: a reference field with a pattern lists only symbols that fit, and
  keeps a chosen one that no longer does.
* Start values: `int` to `double` swaps `0` for `0.0` (M2's test stays);
  `int` to a list removes `0`; a typed value is kept; undo and redo
  restore both changes as one step; a dropped value never changes a
  sibling's start value.
* The cache: one typing per parent per drag in a 1,000-block workspace
  (counted with a spy); a new preview restarts it; it is gone after the
  drag.
* **No false refusal** (with the real core, `apps/desktop/src/editor/sync/`,
  skipped when the core is not built, as `testCore` does): for every
  example project, and each assignment project once it exists, a seeded
  sample of (value block, value input) pairs of one module; wherever
  `candidateFit` is an error, the document with that connection made has
  an analyser error at the moved block or at that input.

**End to end** (Linux and Windows). A text block taken towards
`create [int]`'s value does not snap and snaps into `create [string]`'s;
changing the type swaps the start value and Ctrl+Z restores it; a value of
the wrong type does not snap into a call's argument, and the keyboard
mover does not offer that place.

**Benchmarks.** `webview.drag-typed-1000.frame-p95-all` (new): a number
block dragged slowly down a column of 200 `create` blocks of alternating
types in a 1,000-block module, so that every move asks about typed slots;
reported until it has 5 baselines, then gated, as D20's pooled metric was.
The native benchmark's generated module gains W2's blocks with W2.

## Owner to confirm

1. The editor works out expected types itself, in a TypeScript copy of one
   function held to the Rust one by a fixture, instead of reading them
   from the analysis, so that a type changed a moment ago counts at once
   (*owner to confirm*).
2. `list of` and `{…}` take their type only from their place. Where the
   place gives none (*print*, a `create` set to `auto`) they are the error
   `E0315`, not a `std::vector` of their first item's type
   (*owner to confirm*).
3. In W1, M2's `create`, `set`, call arguments and `return` get typed
   slots, so the editor refuses drops that M2 allowed and the analyser
   then reported as errors (*owner to confirm*).
4. Codegen makes conversions inside braces explicit with `static_cast`, so
   list items get the same warnings as variables instead of g++'s stricter
   narrowing rule; the alternative is to report every narrowing item as an
   error (*owner to confirm*).
5. Start values change only when a field changes or a block lands, never
   because a value was dropped into a sibling input, and types without a
   start value (lists, structs, enums) leave the input empty
   (*owner to confirm*).
