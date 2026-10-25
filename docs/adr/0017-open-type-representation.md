# ADR-0017: An open type representation, typed catalog signatures and per-block headers

* Status: Proposed (M3 wave W0, items L14, L17 and L8; the points marked
  *owner to confirm* are still open)
* Date: 2026-10-25
* On acceptance, partly supersedes [ADR-0013](0013-additive-format-keys.md):
  format version 2 also holds type terms (§4). Builds on
  [ADR-0004](0004-project-format.md), [ADR-0012](0012-parked-blocks.md) and
  [ADR-0015](0015-block-upgrades-on-load.md). [ADR-0018](0018-slot-typing.md)
  evaluates the signatures of §5; [ADR-0019](0019-multi-module-header-split.md)
  places the headers of §6 in each output file.
* Contracts: signatures, tables, grammar, file shapes, messages, the W1 plan,
  the tests and the spec changes are in
  [m3-contracts/types.md](../plans/m3-contracts/types.md).

## Context

M3 is *Language breadth I*
([10 §10.1](../spec/10-roadmap.md#m3-language-breadth-i)): collections,
strings, structs, enums, files, errors, the type picker, typed slots and the
catalog's `headers`. The owner chose all of 03 §3.7.8's collections (M3-D2)
and ten assignments as the exit test (M3-D1,
[next steps §3.3](../plans/next-steps.md#33-decisions-before-m3-starts)),
which use vectors of `int`, text and structs, a vector of vectors, a
`std::map<int, int>`, an `enum class`, `std::ifstream` and three standard
exceptions. Every new block depends on the type representation: it is M3's
critical path ([next steps §3.5](../plans/next-steps.md#35-critical-path-and-risks)).
The spec sketches the parts in [03 §3.5](../spec/03-block-language.md#35-types),
[§3.7.8](../spec/03-block-language.md#378-collections),
[06 §6.6](../spec/06-compiler-pipeline.md#66-stage--types-flow-checks-and-lints)
and [§6.10](../spec/06-compiler-pipeline.md#610-includes-and-support-helpers).

What the code does today:

* `b2c_ir::Type` is closed: seven variants (`crates/b2c-ir/src/types.rs:9-25`),
  four exhaustive matches in other crates (`crates/b2c-lang/src/messages.rs:44`,
  `crates/b2c-codegen/src/cast.rs:42`, `crates/b2c-ipc/src/pipeline.rs:239`,
  a test in `crates/b2c-core-wasm/src/scope.rs:149`), three TypeScript copies,
  and rules that treat anything not text as a number
  (`crates/b2c-lang/src/lower/expr.rs:865-889`). `std::string` is spelt three
  ways: `std::string` in files, `string` in `Type`'s JSON and in catalog
  outputs.
* Types in files are text; the decoder reads any field object without `ref`
  as a declaration (`crates/b2c-model/src/decode/workspace.rs:549-586`). The
  format is version 1, the migration chain is empty
  (`crates/b2c-model/src/migrate.rs:24`), and the loader runs the chain up to
  the current version for every file (`load.rs:284-305`).
* The catalog's `check` classes `bool`, `integer` and `number` mean "converts
  without `Invalid`" (`packages/blockly-ext/src/checker/conversion.ts:87-92`,
  `crates/b2c-lang/src/lower/convert.rs:163-173`). Mixed numbers compare
  silently and `bool` against a number warns (`W0519`); only text against
  non-text is `E0302` (`lower/expr.rs:924-955`). Catalog `headers` are
  informational and disagree with codegen in four blocks.
* `b2c-lang` does not depend on `b2c-catalog`, and `b2c-ir` may depend on no
  workspace crate (`tools/check-layering.py`).

Constraints: files are deterministic, safe to parse and bounded
([05 §5.6](../spec/05-project-format.md#56-validation-limits)); stages are
pure and user text reaches C++ only through typed leaves
([06 §6.1](../spec/06-compiler-pipeline.md#61-overview)); `formatVersion` is
the lowest version whose keys cover the file (ADR-0013); a compatible
catalog change raises the catalog's minor version (ADR-0015); W1 runs in
parallel packages split by crate after the contracts land
([next steps §3.4](../plans/next-steps.md#34-waves)).

## Options considered

**The analyser's type.** (1) More closed variants: each container touches
every match. (2) **A recursive enum (chosen):** primitives, one variant for
library types with arguments, one for user types by symbol ID; a library type
is a table row. (3) Interned IDs: codegen, the IPC mirror and the editor all
need the table, and a global interner breaks 06 §6.1. (4) Canonical strings:
every rule parses, and user types need a second notation.

**How a file writes a type.** (1) C++ spelling with names: renaming a struct
breaks every type naming it. (2) C++ spelling with IDs: no format change, but
every walker parses strings and the editor needs a parser. (3) **JSON terms
(chosen):** names stay strings, a template is a one-key object, a user type is
`{"ref": id}`; walkers are structural and each type has one spelling, at the
cost of a format version.

**Polymorphic blocks in the catalog.** (1) Hand-written typing per block in
Rust and TypeScript (today), with dropdown filters as a third copy.
(2) **Patterns with type variables, classes and three type functions
(chosen):** `type = "elem(C)"`, `where = { C = "indexable" }`. (3) One
signature per container: six or seven per phrase block, tried in turn.

**Connections and includes.** Blockly `check` strings cannot express a
conversion or "the element type of this list"; the structured checker
(chosen) already decides by type. For includes, codegen alone leaves the
catalog misleading and the catalog alone needs a condition language; chosen:
the catalog's fixed and per-option headers are emitted, and codegen adds
those of the types, forms and helpers it chooses.

## Decision

### 1. The types of M3

The primitives (`void` for results only), `std::string`, six containers
(`std::vector`, `std::deque`, `std::set`, `std::queue`, `std::stack`,
`std::priority_queue`), `std::map`, two file streams, four standard errors,
and user structs and enums by the symbol ID of their definition. Containers
nest; an argument is a value type (not `void`, a stream or an error). Streams
and errors are the types of symbols blocks declare; type fields do not offer
them in M3 (*owner to confirm*). There are no unsigned or sized integers:
sizes are `int`, and codegen casts sizes and `int` indexes
(`static_cast<int>(c.size())`, `c.at(static_cast<std::size_t>(i))`) so the
code compiles clean under `-Wall` and the *strict* warnings at every level
(*owner to confirm*). Classes, pointers, user templates, `std::array`,
`std::pair`, `std::optional`, the unordered containers, other arithmetic
types, library-pack types and Raw C++'s opaque type come later.

### 2. The representation

`Type` becomes a recursive enum: the M2 types, `Lib(LibApp)` (a `LibType`
from a closed table, its arguments behind an `Arc`), `Named(SymbolId)` and
`Error`; at most 24 bytes, no interner. `TypeTerm` is a type as files and the
wire write it, before names are looked up. `TypeFacts` gives the rules what
they need about user types: name, kind, comparable, printable. A term has at
most 8 levels and 32 names (*owner to confirm*). The symbol table gains
struct, field, enum and enumerator symbols.

### 3. Type rules

* **Conversions** do not change: a library or user type converts only to
  itself.
* **Classes** name sets of types: M2's `any`, `number`, `integer`, `bool` and
  `text`, and `value`, `collection`, `sequence`, `indexable`, `insertable`,
  `searchable`, `keyed`, `adaptor`, `listable`, `sortable`, `iterable`,
  `switchable`, `comparable`, `printable`, `struct`, `enum`, `input_file`,
  `output_file` and `exception`. `Error` is in every class; a user type the
  facts do not know is neither in nor out, so it connects.
* **`check` and `where` differ.** In `where`, a class is membership. In an
  input's `check`, `bool`, `integer` and `number` keep their M2 meaning,
  "converts without `Invalid`", so a `double` count stays the warning
  `W0518`; `any` accepts all but `void`; the rest are membership.
* **Comparisons** keep M2's rule for `number` and `bool` operands and for
  text; any other pair must be one type in `comparable`, else `E0302`.
  **Arithmetic** needs `number` or `bool` operands, by name.
* **Requirements.** Set and priority-queue elements and map keys must be
  comparable: `B2C-E0313` where the type is written, since every insert or
  lookup would fail. *print*, *join* and *write* need printable values
  (`B2C-E0314`). A struct ticked *comparable* or *printable* counts only when
  its fields do; a field that does not is `E0313` or `E0314` at the tick, as
  g++ would otherwise delete the defaulted `<=>` or find no `operator<<`
  (checked with g++ 11 to 14).
* **Collections.** `elem`, `index` and `key` give an item, what *item of*
  takes and what *contains* looks for. Where one block cannot serve every
  container, 03 §3.7.8 splits: *remove item at* (vector, deque), *remove (x)*
  (set, map), *next item of* and *remove next* (queue, stack, priority
  queue). `std::map` is not iterable, since a range-`for` yields pairs M3
  cannot name; *for each key, value* lowers to a structured binding. *list
  of* has no map form in M3 (*owner to confirm*). `switch` takes `char`,
  `int` and enums.

### 4. Project files and the clipboard

* **Encoding.** A name is a string (`"int"`, `"std::string"`), a user type
  `{"ref": id}`, a template a one-key object named by its qualified C++ name
  whose value is its argument or an array of two or more
  (`{"std::map": ["int", {"std::vector": "int"}]}`). Every M2 file, its hash
  and its version stay as they are.
* **Who checks what.** The loader checks shape and limits (`B2C-E0140`); the
  catalog checks names (`E0607`, `E0614`, and `E0602` from a newer catalog,
  so a new type is a compatible change); the analyser checks references
  (`E0201`, `E0207`). Only a one-key object whose key is a qualified name is
  a template; any other object stays a declaration, so a damaged one keeps
  its M2 code.
* **Version.** A template term is a key of format version 2, beside
  *Textbook style*. Versions are cumulative, so giving types version 4 after
  M5's `project.lints` would oblige M3 to know `lints`. The writer computes
  the version from the keys present. The loader reads up to
  `MAX_FORMAT_VERSION` (2); every version from `OLDEST_UNMIGRATED_VERSION`
  (1) up loads without the migration chain, which covers only changes of
  meaning (none yet), and `migratedFrom` compares with that oldest version
  (*owner to confirm*).
* **Older apps.** M2 refuses a version 2 file as newer. A version 1 file from
  M3 may hold `{"ref": id}` in a type field; M2 reports `E0607` there (not the
  more precise `E0602`) and keeps the value.
* **Clipboard.** A payload is version 2 when it holds a template term or a
  `refs` entry of the new kinds `struct`, `enum`, `field` and `enumerator`;
  readers accept 1 and 2. Members are recorded with their owner
  (`::Student::mark`) and bound again through it.

### 5. Catalog signatures (L17)

A catalog entry declares type variables with their classes (`where`) and
patterns on its output, type and reference fields, value inputs (`type`) and
parameter rows (`type_class`). Patterns are variables, names, templates, the
three type functions, M2's `field:NAME` and `symbol`, and four new forms:
`symbol:NAME`, `param:F` (a call argument's parameter), `member:F` (a struct
initialiser's field) and `result` (what `return` gives back). Two keys
complete it: `assign` (assigned, not initialised, so `char` fits
`std::string`; never binds) and `members` (a reference field that lists the
fields or enumerators of the type bound to a variable).

* A variable is bound by the first part in catalog order whose type is known;
  the signature keeps that order.
* Empty parts, `auto` and place-typed values bind nothing; an unknown or
  erroneous part, or one whose binding fails, binds `Error`, so nothing that
  depends on it is checked twice.
* Only a *place-typed* block (its output variable is bound by no part, as in
  `list of`) takes its type from its place; any other output variable needs a
  required binder.
* An input whose pattern is not a fixed type has no catalog `default`; the
  editor writes start values (ADR-0018).

`b2c-ir` holds the signatures and `TypeEnv`, which binds and resolves;
`b2c-catalog` builds the `SigTable`; `b2c-lang` reads it through
`analyze_with` without depending on `b2c-catalog`. Checking a fit needs
`conversion`, which stays in `b2c-lang`, so ADR-0018's `type_block` and the
shared fixture's writer live there. The catalog refuses at load a definition
that breaks the grammar or these rules.

### 6. Headers (L8)

* A header is a typed leaf from a closed table of the standard headers; no
  file supplies one. `use header` chooses from it through a new `header`
  field kind; how it names a project module is ADR-0019's, before W2.
* Catalog `headers` are emitted: a block's are those every form needs, and a
  dropdown option adds its own. `b2c-lang` records them per item part, those
  of blocks printed with a declaration (signature, default arguments, a
  constant's value, default member values) apart from those printed only
  with a definition, so that ADR-0019 includes each where its code goes and a
  body edit never changes a header file.
* Codegen adds the headers of the types, forms and helpers it chooses, and
  writes the operators the ticks promise: the defaulted `<=>` in C++20, six
  hidden-friend comparisons in C++17 (with only `==` and `<`, `!=`, `>` and
  `<=` fail), an `operator<<` for a printable struct, and an `operator<<` or
  `to_string` for an enum that is printed, written or joined.
* W1 removes from `text.literal`, `program.exit`, `math.random_int` and
  `io.ask` the headers only some forms need, so no generated file changes
  (*owner to confirm*).

### 7. Diagnostic codes and versions

The same allocation stands in ADR-0018 and the contracts:

| Code | Meaning | From | First emitted |
| --- | --- | --- | --- |
| `B2C-E0140` | type term too deep or too wide | ADR-0017 | W1 (loader) |
| `B2C-E0213`, `E0214`, `E0221` | `::name` reach (06 §6.14.7); shared header part names a non-shared definition; module name starts with `b2c` | spec; ADR-0019 | W3 |
| `B2C-E0313`, `E0314` | not comparable; not printable | ADR-0017 | W1 |
| `B2C-E0315` | no type from its place | ADR-0018 | W1 |
| `B2C-E0316` | whole-number division or `mod` by a constant zero | L10 (types.md §11.2) | W1 |
| `B2C-E0420`, `E0421` | declaration cycles (06 §6.7) | spec | W2 |
| `B2C-E0422`, `E0423` | header cycle; cross-module initialisation order | ADR-0019 | W3 |
| `B2C-W0506` | a decimal result that is infinity or NaN | L10 (types.md §11.2) | W1 |

`E0206`, `E0207`, `E0301`, `E0303` (L16) and `E0517` (L10) are reused with new
text; the next free codes are `E0141`, `E0219`, `E0222` and `E0424`. Each code
gets its reference entry in the pull request that defines it.

Versions: `formatVersion` 2 holds *Textbook style* and type terms (and
`use header`'s module value if ADR-0019 needs a new shape), 3 is
`project.lints`; the catalog is 1.3.0 from W1 to the M3 release; the
clipboard is version 2; the source map stays 1 until M3-5; `IPC_VERSION` does
not change for ADR-0016 to ADR-0019; `update()` gains `slotTypes`.

### 8. W1

**The contracts pull request keeps the workspace and every test green:**
`b2c-ir` in full, the new `Type` beside its old helpers (deprecated until the
integration step) and with its serialisation unchanged; wildcard arms where
new variants break exhaustive matches; `b2c-model`'s new `FieldValue` and
`RefKind` variants as types only; `analyze_with` beside `analyze`; an empty
`Catalog::sigs()`; the TypeScript declarations. ADR-0018's and ADR-0019's W1
contracts ride in it under the same rule. Six packages follow in parallel:
`b2c-model` (with the version constant's users in `b2c-app` and
`b2c-core-wasm`), `b2c-catalog`, `b2c-lang` (a block without a signature keeps
its M2 rule, so merge order does not matter), `b2c-codegen`, the Rust wire
and the TypeScript side; the last two switch `Type`'s serialisation to terms
together. An integration pull request removes the deprecated helpers, the
wrappers and the M2 typing rules that patterns replaced. W1 adds no block,
and every golden file stays byte-identical.

## Consequences

* **Easier.** A container is a table row; a polymorphic block is a few lines
  of TOML that the analyser, the checker and the dropdowns read alike; headers
  have one source per construct. With ADR-0018 and the W2 block shapes in the
  contracts, the ten assignments are covered.
* **Harder.** `Type` grows from 1 byte to 24, and `Expr` with it. Rules name
  classes. Two copies of the rules, Rust and TypeScript, are held together by
  a generated fixture. The catalog gains a grammar, and 03 §3.7.8 three
  blocks.
* **Performance.** Type walks are bounded by 32 names; a clone is a reference
  count except for `Named`. M2's `create`, `set`, call and `return` get
  patterns in W1, so the gated native and preview benchmarks (N4) bind
  patterns from W1 on, and each W1 pull request's A/B job covers them and
  `webview.drag-5000.frame-p95-all`. The editor's per-candidate cost is
  ADR-0018's typed drag benchmark, and the first W2 pull request with
  container blocks adds them to the benchmark workspaces.
* **Security** (spec 08). No file text reaches C++ through a type: names are
  looked up in static tables, user types print their validated `Ident`
  ([08 §8.4.1](../spec/08-security.md#841-identifiers)), headers come from the
  closed table. Terms are bounded at load, and the TypeScript parser builds
  fresh objects and refuses prototype keys. Types change neither the trust
  summary nor the security hash. The M3 threat-model review covers terms as a
  new loader and clipboard input.
* **On acceptance,** ADR-0013's status and the ADR index read *Partly
  superseded by ADR-0017 (version 2 also holds type terms)*. L8 can move to
  an ADR of its own if the owner prefers.

## Tests

In the contracts (§9 there): terms, limits, classes, type functions and
conversions by table, with a fixture from `b2c-lang` that the TypeScript copy
agrees with; the decoder's three readings of an object; a version 1 golden
project loads byte for byte with `migratedFrom` null; clipboard versions 1
and 2; M2's comparison and check-class diagnostics unchanged; `E0313` and
`E0314`, ticks included; every golden file byte-identical; and a program with
every sample type, a comparable struct using all six operators, a printed and
joined enum and indexed access, compiled with the *strict* warnings in C++17
and C++20 on GCC 11 and 15.

## Owner to confirm

1. Format version 2 holds *Textbook style* and type terms, partly superseding
   ADR-0013's numbering; versions without a change of meaning load without a
   migration step (*owner to confirm*).
2. `std::string` is written `std::string` on the wire too (*owner to confirm*).
3. No unsigned or sized integers; codegen casts sizes and indexes so the code
   is clean at every warning level (*owner to confirm*).
4. Streams and errors are types of declared symbols only (*owner to confirm*).
5. Catalog `headers` are emitted, and four core blocks lose the headers only
   some forms need (*owner to confirm*).
6. A type has at most 8 levels and 32 names (*owner to confirm*).
7. 03 §3.7.8 gains *remove item at*, *remove (x)*, *next item of*, *remove
   next* and *for each key, value*; *list of* has no map form in M3
   (*owner to confirm*).
8. Ticked *comparable* or *printable* structs need qualifying fields, and
   C++17 gets six comparison operators (*owner to confirm*).
