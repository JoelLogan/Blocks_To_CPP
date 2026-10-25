# ADR-0017: An open type representation, typed catalog signatures and per-block headers

* Status: Proposed (M3 wave W0, items L14, L17 and L8; the points marked
  *owner to confirm* are still open)
* Date: 2026-10-24
* Amends [ADR-0013](0013-additive-format-keys.md): format version 2 holds
  both keys that M3 adds, *Textbook style* and constructed types (§4).
  Builds on [ADR-0004](0004-project-format.md) (the file stays JSON with
  array statement lists), [ADR-0015](0015-block-upgrades-on-load.md) (new
  types are compatible catalog changes) and
  [ADR-0012](0012-parked-blocks.md) (parked blocks are still checked
  against the catalog). ADR-0018 (typed slots, L15) builds on §2, §3 and §7
  of this ADR; [ADR-0019](0019-multi-module-header-split.md) places the
  per-block headers of §8 in each output file.

## Context

### What M3 needs

M3 is *Language breadth I*
([10 §10.1](../spec/10-roadmap.md#m3-language-breadth-i)): collections,
strings, structs, enums, files and errors, the type picker, typed slots and
"catalog schema extensions (including the catalog's `headers`)". The owner
chose all of §3.7.8's collections (M3-D2,
[next steps §3.3](../plans/next-steps.md#33-decisions-before-m3-starts)) and
ten assignments as the exit test (M3-D1). Their reference solutions
(`tests/assignments/*/reference.cpp`) use `std::vector<int>`,
`std::vector<std::string>`, `std::vector<Student>`,
`std::vector<std::vector<int>>`, `std::map<int, int>`, a struct, an
`enum class`, `std::ifstream`, and catch `std::invalid_argument`,
`std::out_of_range` and `std::runtime_error`.

The specification already describes the parts:

* the type picker, which builds `map<string, vector<int>>` in one field
  ([03 §3.5.1](../spec/03-block-language.md#351-the-type-picker)), and the
  Friendly and C++ labels of types (§3.5.2);
* gradual checking: errors only when certain, unknown types connect
  ([03 §3.5.3](../spec/03-block-language.md#353-type-checking-model-gradual));
* polymorphic phrase blocks: one `add (x) to [c]` for every container,
  whose lowering and dropdowns follow the static type (03 §3.1,
  [§3.7.8](../spec/03-block-language.md#378-collections));
* a sketch of the analyser's type with `Named`, `Generic`, `Param`,
  `Pointer`, `Ref`, `Function`, `Auto` and `Opaque`
  ([06 §6.6](../spec/06-compiler-pipeline.md#66-stage--types-flow-checks-and-lints));
* typed template inputs (`type = "range<T>"`,
  [03 §3.11.2](../spec/03-block-language.md#3112-template-lowering-library-blocks));
* includes collected from "each catalog entry and each type", with no
  transitive-include assumptions
  ([06 §6.10](../spec/06-compiler-pipeline.md#610-includes-and-support-helpers)).

Every new block depends on the type representation, so it is the critical
path of M3 (next steps §3.5).

### What the code does today

* **A closed type.** `b2c_ir::Type` has seven variants: `Void`, `Bool`,
  `Char`, `Int`, `Double`, `String` and `Error`
  (`crates/b2c-ir/src/types.rs:9-25`). `from_field` knows six names
  (`:56-66`); `cpp_name` and `friendly_name` return `&'static str`
  (`:29-52`). Outside test modules, 186 lines in 19 Rust source files name a
  variant. Six matches list every variant (`types.rs:29`, `:42`;
  `crates/b2c-lang/src/messages.rs:44`; `crates/b2c-codegen/src/cast.rs:42`;
  `crates/b2c-ipc/src/pipeline.rs:239`; `crates/b2c-core-wasm/src/scope.rs:149`),
  and two more list the set (`scope.rs:12-20`; the test list
  `crates/b2c-lang/src/typing.rs:216`). In TypeScript, 66 lines in 13 files
  use `StaticType` and its helpers, and the list of seven is copied three
  times (`packages/blockly-ext/src/checker/types.ts:12-23`,
  `apps/desktop/src/editor/services/symbols.ts:25-33`, the generated
  `packages/ipc-types/src/generated/types.ts:1267`).
* **Rules that assume the closed set.** `conversion` matches the five value
  types (`typing.rs:97-109`); `arithmetic_result` treats anything that is
  not `double` as `int` (`:122-129`); `binary_type` checks for text and
  otherwise does arithmetic (`crates/b2c-lang/src/lower/expr.rs:865-889`),
  so a container added with `+` would type as `int` without an error.
* **Three spellings of `std::string`.** Files write `std::string`
  (`catalog/core/variables.toml:15`); the JSON of `Type` that crosses to the
  editor writes `string` (serde `snake_case`;
  `crates/b2c-ir/src/scope_info.rs:250`, `crates/b2c-ipc/src/pipeline.rs:113`);
  catalog outputs write `string` (`crates/b2c-catalog/src/schema.rs:169`).
* **Types in files** are text: in type fields (`FieldValue::Text`,
  `crates/b2c-model/src/document.rs:470-480`) and in `extra.params[].type`.
  The catalog checks them against the field's or setting's `types` list
  (`crates/b2c-catalog/src/resolve.rs:532`, `:440-447`;
  `catalog/core/functions.toml:16-26`). The decoder accepts in a field only
  `true`/`false`, text, `{"ref"}` and `{"sym", "name"}`; any other object
  is read as a declaration, so a key it does not know is `E0110`, or
  `E0108` when a newer app saved the file
  (`crates/b2c-model/src/decode/workspace.rs:549-586`,
  `decode/mod.rs:280-309`). The format version is 1
  (`document.rs:18`), and the writer writes the stored version
  (`save.rs:293`). b2c-model already knows the rows of `params`: paste
  renames their symbols (`ids.rs:298-353`).
* **The catalog** has a closed `OutputType` (`schema.rs:150-242`), five
  input classes (`schema.rs:365-379`), and `headers` that are "informational;
  the generator decides" (`schema.rs:42-44`), checked only for their
  characters (`definitions.rs:179-190`). Under a newer catalog, only an
  unknown dropdown option counts as made by a newer version (`E0602`); an
  unknown type is `E0607` (`upgrade.rs:787-801`). `b2c-lang` does not
  depend on `b2c-catalog` (`crates/b2c-lang/Cargo.toml:11-14`): lowering is
  hand-written per block ID.
* **Headers.** Codegen collects `&'static str` headers itself
  (`crates/b2c-codegen/src/lower/mod.rs:31-37`, `:140-155`;
  `lower/stmt.rs:294`, `:309`, `:364`, `:372`, `:386`; `lower/expr.rs:226`,
  `:254`) and prints them (`emit.rs:83-91`, `:120-128`). The catalog's
  headers disagree with it: `text.literal` lists `<string>`
  (`catalog/core/text.toml:11`) though a literal needs none, `program.exit`
  lists `<cstdlib>` (`program.toml:22`) though in `main` it returns, and
  `math.random_int` lists `<random>` (`math.toml:78`) though the helper
  carries it.
* **Codegen types.** `CType` is closed, `Copy` and spelled as a
  `&'static str` (`cast.rs:18-75`). A read-only parameter is `const T&` only
  for `std::string` (`lower/mod.rs:210`).
* **The editor** uses no Blockly `check` strings. `B2cConnectionChecker`
  works out the dragged block's type (`staticOutputType`,
  `packages/blockly-ext/src/checker/output-type.ts:41-118`) and applies a
  copy of the analyser's rule (`conversion.ts:28-97`, used at
  `checker.ts:63-100`). The WASM core exports the rule as a 49-row table
  (`crates/b2c-core-wasm/src/scope.rs:47-58`, `lib.rs:205-212`), which a test
  compares with the copy (`apps/desktop/src/editor/sync/integration.test.ts:455-461`).
  A type field is a dropdown of the catalog's `types` that keeps an unknown
  value from a file (`packages/blockly-ext/src/fields/type.ts`,
  `options-base.ts:4-6`, `:64-67`).

### Constraints

* Project files are editor-independent, deterministic, safe to parse and
  bounded ([05 §5.1](../spec/05-project-format.md#51-requirements),
  [§5.6](../spec/05-project-format.md#56-validation-limits)); the limits are
  enforced in `b2c-model` before anything else.
* Every stage is pure, with no global mutable state, and user text reaches
  C++ only through typed leaves
  ([06 §6.1](../spec/06-compiler-pipeline.md#61-overview), invariant 2).
* `formatVersion` is the lowest version whose keys cover the file
  (ADR-0013). A compatible catalog change raises the catalog's minor
  version, and an unknown choice from a newer catalog is `E0602`
  (ADR-0015). An upgrade never adds nesting (ADR-0015).
* The main-thread core instance stays synchronous for `conversionTable`,
  and the checker reads `preview.blockTypes`
  ([ADR-0016](0016-preview-in-a-web-worker.md) §1). Blockly is extended only
  through documented seams ([ADR-0014](0014-extending-blockly.md)).
* W1 runs in packages split by crate, in parallel, after the contracts land
  (next steps §3.4, §3.5).

## Options considered

**How the analyser represents a type.**

1. **More closed variants** (`Vector(Box<Type>)`, `Map(Box<Type>, Box<Type>)`,
   …). Simple, but each new container touches every match: 19 files today.
2. **A recursive enum (chosen):** the primitives, one variant for library
   types with their arguments, one for user types by symbol ID. Adding a
   library type adds a table row, not a variant.
3. **Interned type IDs** in a per-analysis table. Equality is one integer
   compare, but codegen, the IPC mirror and the editor all need the table
   to read a type, and a global interner breaks 06 §6.1.
4. **Canonical strings** (`"std::vector<int>"`). Equality is cheap, but every
   rule parses, and user types need a second notation.

**How a project file writes a type.**

1. **C++ spelling with names** (`"std::vector<Point>"`). Renaming `Point`
   breaks every type that names it.
2. **C++ spelling with IDs** (`"std::vector<@sym_Pt12>"`). No format change,
   and M2 opens such a file with `E0607` on the block. But every walker
   (paste renaming, `refs`, the version rule) parses strings, a canonical
   spelling must be enforced on load, and the editor needs a second parser.
3. **JSON terms (chosen):** names stay strings, a template is a one-key
   object, a user type is `{"ref": id}` as everywhere else in the format.
   Walkers are structural and the form is canonical by construction. The
   object shape is new, so it needs a format version.

**How the catalog declares a polymorphic block.**

1. **Hand-written typing per block** in `b2c-lang` and in the editor's
   checker (today). Every new block needs Rust and TypeScript code that must
   agree, and the dropdown filters are a third copy.
2. **Patterns with type variables, classes and three type functions
   (chosen):** `type = "elem(C)"`, `where = { C = "indexable" }`. One small
   grammar, evaluated by the same rules in Rust and TypeScript.
3. **One signature per container** (`[[block.signature]]` for
   `std::vector<T>`, `std::deque<T>`, …). The most general, but six or seven
   signatures per phrase block, and the editor must try each.

**How connections are checked.**

1. **Blockly `check` strings.** Blockly intersects string sets: it cannot
   express a conversion (`int` into `double`) or "the element type of the
   list in field `LIST`", and every output would need `setCheck` after each
   analysis.
2. **The structured checker (chosen):** `B2cConnectionChecker` already
   decides by type; it learns the new types and the patterns.

**Where includes come from.**

1. **Codegen decides; catalog headers are informational** (today). The
   catalog misleads, and template blocks (M4) would have nowhere to declare
   theirs.
2. **Catalog headers are emitted (chosen):** a block's fixed headers and
   per-option headers come from the catalog; codegen adds the headers of the
   types it names, of the forms it chooses and of the helpers it calls.
3. **The catalog decides everything,** with conditions on types and modes.
   A small language of its own for little gain.

## Decision

### 1. The types of M3

| Kind | Types | JSON name | Friendly label | Header |
| --- | --- | --- | --- | --- |
| primitive | `void` (results only), `bool`, `char`, `int`, `double` | the C++ name | as in M2 | — |
| text | `std::string` | `std::string` | `string` | `<string>` |
| container | `std::vector<T>`, `std::deque<T>`, `std::set<T>`, `std::queue<T>`, `std::stack<T>`, `std::priority_queue<T>` | `std::vector`, … | `list of T`, `deque of T`, `set of T`, `queue of T`, `stack of T`, `priority queue of T` | `<vector>`, `<deque>`, `<set>`, `<queue>`, `<stack>`, `<queue>` |
| map | `std::map<K, V>` | `std::map` | `map from K to V` | `<map>` |
| file stream | `std::ifstream`, `std::ofstream` | the C++ name | `input file`, `output file` | `<fstream>` |
| error | `std::exception`, `std::runtime_error`, `std::invalid_argument`, `std::out_of_range` | the C++ name | `error`, `runtime error`, `invalid argument error`, `out-of-range error` | `<exception>`, `<stdexcept>` |
| user | a struct or an enum, by the symbol ID of its definition | `{"ref": id}` | its name | — |

* Containers nest: `std::vector<std::vector<int>>`,
  `std::map<std::string, std::vector<int>>`. An argument is a *value* type:
  not `void`, a stream or an error.
* Streams and errors are types the analyser gives to symbols that blocks
  declare (*open file … as [file]*, *catch … as [e]*). Type fields do not
  offer them in M3 (*owner to confirm*).
* M3 has no unsigned or sized integers: *length of* and every other size
  is an `int`, and the generated code converts `size()` with
  `static_cast<int>` where a comparison or the *strict* warnings
  (`-Wsign-compare`, `-Wconversion`, `-Wsign-conversion`;
  `crates/b2c-toolchain/src/command.rs:49-60`, `:278-283`) would otherwise
  warn (*owner to confirm*).
* **Not in M3:** classes (M4), pointers, references other than the
  parameter modes and the implicit `const&` of *for each* and *catch*,
  smart pointers, `std::function`, user templates (M4), `std::array`,
  `std::pair`, `std::tuple`, `std::optional`, `std::variant`, the unordered
  containers, `float`, `long double`, the `long` and `unsigned` integers,
  library-pack types (M4/M5) and the opaque type of Raw C++ (M4). Type
  aliases are not in M3's block list either; when they come, `{"ref": id}`
  to an alias stands for its target type, as in C++, and declarations keep
  the alias's name for printing.

### 2. The representation in Rust (`crates/b2c-ir/src/types.rs`)

```rust
/// A static type (06 §6.6). Equality, order and hash are structural.
/// At most 24 bytes; a clone never copies a type's arguments.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Type {
    Void, Bool, Char, Int, Double, String,
    /// A standard-library type with its template arguments, if any.
    Lib(LibApp),
    /// A user struct or enum: the symbol ID of its definition.
    Named(SymbolId),
    /// The type of an expression that already has an error; fits everything.
    Error,
}

/// Built only by `Type::lib`: the arity is right and every argument is a value type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LibApp { /* private: ty: LibType, args: Arc<[Type]> */ }
impl LibApp { pub fn ty(&self) -> LibType; pub fn args(&self) -> &[Type]; }

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LibType {
    Vector, Deque, Set, Map, Queue, Stack, PriorityQueue,
    IfStream, OfStream,
    Exception, RuntimeError, InvalidArgument, OutOfRange,
}
impl LibType {
    pub const ALL: [LibType; 13];
    pub const fn arity(self) -> usize;           // 1; 2 for Map; 0 for streams and errors
    pub const fn cpp_name(self) -> &'static str; // "std::vector": also its JSON name
    pub fn from_cpp_name(name: &str) -> Option<LibType>;
    pub const fn header(self) -> Header;
    pub const fn label(self) -> &'static str;    // "list of %1", "map from %1 to %2", "input file"
}

impl Type {
    pub const MAX_DEPTH: usize = 8;   // library types nested in one type
    pub const MAX_NAMES: usize = 32;  // names and references in one type
    /// `Ok(Type::Error)` when an argument is `Error`, so an error is reported once.
    pub fn lib(ty: LibType, args: Vec<Type>) -> Result<Type, TermError>;
    /// Looks the term's names up. `auto` is `Err(TermError::Auto)`: it is not a type.
    pub fn from_term(term: &TypeTerm) -> Result<Type, TermError>;
    pub fn to_term(&self) -> Option<TypeTerm>; // `None` for `Error`
    pub fn headers(&self) -> impl Iterator<Item = Header> + '_;
    /// `None` when not known: `Error`, or a `Named` that `facts` does not know.
    pub fn is_in(&self, class: TypeClass, facts: &dyn TypeFacts) -> Option<bool>;
    pub fn cpp<'a>(&'a self, facts: &'a dyn TypeFacts) -> impl fmt::Display + 'a;   // std::vector<Student>
    pub fn label<'a>(&'a self, facts: &'a dyn TypeFacts) -> impl fmt::Display + 'a; // list of Student
}

/// A type as files and the wire write it, before its names are looked up (§4).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypeTerm {
    Name(Box<str>),                 // "int", "std::string", "auto", "std::ifstream"
    Ref(SymbolId),                  // {"ref": "sym_x"}
    App(Box<str>, Box<[TypeTerm]>), // {"std::vector": "int"}, {"std::map": ["int", "int"]}
}
impl TypeTerm {
    pub fn name(name: &str) -> Result<TypeTerm, TermError>; // shape and limits only
    pub fn app(name: &str, args: Vec<TypeTerm>) -> Result<TypeTerm, TermError>;
    pub fn refs(&self) -> impl Iterator<Item = &SymbolId>;
    pub fn rename(&mut self, map: &BTreeMap<SymbolId, SymbolId>);
    pub fn has_app(&self) -> bool;
    /// The canonical compact JSON, e.g. `{"std::vector":"int"}`; equal terms, equal keys.
    pub fn key(&self) -> String;
}

/// What the type rules need to know about user types.
pub trait TypeFacts {
    fn user_type(&self, id: &SymbolId) -> Option<UserType<'_>>;
}
pub struct UserType<'a> { pub name: &'a str, pub kind: UserKind, pub comparable: bool, pub printable: bool }
pub enum UserKind { Struct, Enum }

/// Classes of types (§3), moved here from `b2c-catalog`'s `schema.rs:365-379`;
/// serde names in snake_case, as catalog files write them.
pub enum TypeClass { Any, Number, Integer, Bool, Text, Value, Collection, Sequence, Indexable,
                     Insertable, Searchable, Iterable, Comparable, Printable, Struct, Enum,
                     InputFile, OutputFile, Exception }

/// The type functions of catalog patterns (§3, §7).
pub enum TypeFn { Elem, Index, Key }
impl TypeFn { pub fn apply(self, ty: &Type) -> Option<Type>; }
```

* `TermError` names each problem: too deep, too many names, a bad name, a
  bad shape, an unknown name or library type, a wrong number of arguments,
  `auto`, and a type that cannot be an argument (`void`, a stream, an
  error).
* **No interning.** A library type's arguments sit behind an `Arc`, so a
  clone is a reference count. Equality and hashing walk the type, which the
  limits bound to 32 names; most types have one to three. `Named` holds a
  `SymbolId` (`Box<str>`, `crates/b2c-ir/src/ids.rs:36`), so its clone
  allocates; if the benchmarks show it, `SymbolId` moves to `Arc<str>`
  without an API change.
* **Display.** `cpp` writes `std::vector<std::vector<int>>` with the `std::`
  names in full and user types by their name; messages and the editor's C++
  labels use it, codegen does not (§8). `label` writes `list of list of int`
  and `map from string to list of int`; the first argument of a map is put
  in parentheses when it is itself a map. `cpp_name`, `friendly_name`,
  `from_field`, `is_numeric` and `header` of today's `Type` go;
  `from_field`'s two callers (`crates/b2c-lang/src/lower/mod.rs:753`,
  `access.rs:123`) use `from_term`.
* **Symbols.** `SymbolKind` (`crates/b2c-ir/src/sast.rs:523-541`) gains
  `Struct { fields: Vec<SymbolId>, comparable: bool, printable: bool }`,
  `Field { owner: SymbolId }`, `Enum { values: Vec<SymbolId> }` and
  `Enumerator { owner: SymbolId }`; `SymbolInfoKind` gains `struct`,
  `field`, `enum` and `enumerator` with the same data in camelCase. The
  type of a struct or enum symbol is `Named` of itself; an enumerator's is
  `Named` of its enum. `SymbolTable` implements `TypeFacts`. W1 adds these
  shapes; W2's struct and enum blocks fill them.
* **Headers** are a type of their own (`crates/b2c-ir/src/headers.rs`, §8).

### 3. Type rules

**Conversions.** `conversion(from, to)` (`typing.rs:97-109`) keeps its five
results and its five rules. A library or user type converts only to the
same type (rule 1, structural equality) and is `invalid` otherwise (rule 5,
*anything else*), so the match needs no new arm, and the documentation says
so. `std::vector<int>` does not convert to `std::vector<double>`, an enum
does not convert to `int` (`enum class`), and no struct converts to
another. `set`'s exception (`char` into `std::string`) stays. A *list of*
block takes its type from its place, and each item converts to the element
type by the same rule (§7).

**Classes.** `check` on an input and `where` on a type variable name one of
these; membership is `Type::is_in`.

| Class | Members |
| --- | --- |
| `any` | every type but `void` (M2) |
| `number`, `integer`, `bool`, `text` | as in M2: `int`, `double`, `char`; `int`, `char`; `bool`; `std::string`, `char` |
| `value` | what a variable can hold: `bool`, `char`, `int`, `double`, `std::string`, the containers and maps, structs and enums |
| `collection` | the six containers and `std::map` |
| `sequence` | `std::vector`, `std::deque` |
| `indexable` | `std::vector`, `std::deque`, `std::map` |
| `insertable` | the six containers |
| `searchable` | `std::vector`, `std::deque`, `std::set`, `std::map` |
| `iterable` | `std::vector`, `std::deque`, `std::set`, `std::map`, `std::string` |
| `comparable` | `bool`, `char`, `int`, `double`, `std::string`, enums, structs marked *comparable*, and `std::vector`, `std::deque`, `std::set`, `std::map`, `std::queue`, `std::stack` of comparable arguments |
| `printable` | `bool`, `char`, `int`, `double`, `std::string`, enums, structs marked *printable* |
| `struct`, `enum` | user structs, user enums |
| `input_file`, `output_file`, `exception` | `std::ifstream`; `std::ofstream`; the four errors |

`Error` is in every class, and a type whose facts are not known answers
`None` (unknown), so it connects (03 §3.5.3).

**Type functions.**

| Function | `vector<T>`, `deque<T>` | `set<T>` | `map<K, V>` | `queue<T>`, `stack<T>`, `priority_queue<T>` | `std::string` |
| --- | --- | --- | --- | --- | --- |
| `elem` (what it holds) | `T` | `T` | `V` | `T` | `char` |
| `index` (what *item* takes) | `int` | — | `K` | — | `int` |
| `key` (what *contains* and *remove* look for) | `T` | `T` | `K` | — | — |

**Requirements of containers.** The elements of `std::set` and
`std::priority_queue` and the keys of `std::map` must be `comparable`. Where
they are not, g++ rejects the program, so it is an error: `B2C-E0313`
(§9). Comparing two values (`==`, `<`) needs the same type in the
`comparable` class (`E0302` as now). Arithmetic needs `number` or `bool`
operands, checked explicitly instead of "not text"
(`lower/expr.rs:865-889`, `typing.rs:122-129`). *print*, *join* and
*write to file* need `printable` values (`B2C-E0314`). A defaulted
`operator<=>` gives a struct both `==` and `<` in C++20; for C++17,
codegen writes both operators for a *comparable* struct, so the class does
not depend on the standard. This was checked with g++ 13.3 in C++20 mode:
`std::set` of an `enum class` and of a struct with a defaulted `<=>`
compile, `==` on that struct compiles, `std::set` of a struct without it
fails, and `==` on `std::priority_queue` fails. The defaulted `<=>` also
needs `<compare>` when no other header brings it in (§8).

### 4. Project files and clipboard payloads

**Encoding.** A type is a *type term*:

```json
"TYPE": "int"
"TYPE": {"ref": "sym_Pt12"}
"TYPE": {"std::vector": {"std::vector": "int"}}
"TYPE": {"std::map": ["std::string", {"std::vector": {"ref": "sym_Pt12"}}]}
"params": [{"sym": "sym_g", "name": "grades", "type": {"std::vector": "int"}, "mode": "read_only"}]
```

* A name is a string: a primitive, `std::string`, a nullary library type,
  or `auto` and `void` where a field allows them. Every M2 file is
  therefore unchanged, and so are its content hash and its version.
* A user type is `{"ref": id}`, the format's one spelling of a reference.
* A template is an object with exactly one key, the template's C++ name,
  whose value is its argument, or an array of its two or more arguments.
  One argument is never written as a one-element array, and an empty array
  is never written: each type has one spelling.
* Terms appear in fields and in the `type` of `extra` rows (`params`, and
  the row keys W2 adds for struct fields and enum values).

**What checks what.** The loader (`b2c-model`) checks the term's shape and
limits; the catalog checks its names; the analyser checks its references.

* **Loader.** A name is 1 to 64 ASCII characters of identifiers joined by
  `::` or single spaces; a key follows the same rule. A term has at most
  `Type::MAX_DEPTH` (8) levels and `Type::MAX_NAMES` (32) names and
  references, else `B2C-E0140` (new) (*owner to confirm*). A wrong shape
  is `B2C-E0112`, an invalid ID `E0113`, a reserved key `E0127`, as for
  other values. The decoder reads an object with `ref` as a reference, one
  with `sym` or `name` as a declaration, and any other object as a type
  term (`FieldValue::Type(TypeTerm)`, always a template); it checks the
  `type` of the rows of the known row keys the same way, and keeps `extra`
  as JSON. The limits are within the JSON depth limit of 128, so they add
  no new resource bound; they keep every type walk shallow.
* **Catalog.** A name or template it does not know, a wrong number of
  arguments, `auto` or `void` where the field does not list them, or a
  type outside the field's or row's domain is `E0607` in a field and
  `E0614` in a parameter row, as today. From a newer catalog it is `E0602`
  instead, and the block is kept: `newer_catalog_problem`
  (`crates/b2c-catalog/src/upgrade.rs:787-801`) treats type fields and rows
  like dropdowns (ADR-0015, D18). So adding a type, a container or a type
  field's choice is a compatible catalog change, like a dropdown option.
* **Analyser.** A reference to a symbol that does not exist is `E0201`; to
  one that is not a struct or enum, `E0207`.

**Version.** Only the object shape of a constructed type is new to the
format, so it is a key of version 2:

```rust
// crates/b2c-model/src/version.rs
/// The lowest format version whose keys cover the document (ADR-0013): 2 when it
/// sets `options.usingNamespaceStd` or holds a template term (in a field or in the
/// `type` of an `extra` row), else 1.
pub fn required_format_version(document: &Document) -> u32;
pub const MAX_FORMAT_VERSION: u32 = 2; // replaces CURRENT_FORMAT_VERSION (document.rs:18)
```

The writer writes `required_format_version`, not the stored number
(`save.rs:293`), so a file that stops using templates goes back to
version 1. Version 2 needs no migration from version 1. ADR-0013 lists
version 2 for `options.usingNamespaceStd` and version 3 for `project.lints`
(M5), each later key taking the next number; this ADR puts both of M3's
keys into version 2, which no released app writes yet. Giving types
version 4 instead would make an M3 app accept `project.lints` without
acting on it, the silent drop that `E0110` exists to prevent
(*owner to confirm*).

**What an older app does.**

* M2 refuses a version 2 file as a whole with `E0108` (*made with a newer
  version*), before decoding.
* A version 1 file from M3 can still hold `{"ref": id}` in a type field or
  row (a struct variable). M2 decodes it and reports `E0607` or `E0614` on
  that block: its `newer_catalog_problem` covers only dropdowns. The block
  is kept, M2's editor keeps a value it does not know
  (`options-base.ts:4-6`) or shows the block as a placeholder
  (`packages/blockly-ext/src/mutators/state.ts:164-166`), and a save keeps
  the value. The message is less precise than `E0602`; it is not data loss.

**Upgrades.** M2 projects need none: their types are valid terms, their
version stays 1, and opening them marks nothing as changed. W1 raises the
catalog to 1.3.0 for the widened type fields (§7). A block migration that
rewrites a type keeps its references and may not deepen the term: under
ADR-0015 that counts as adding nesting.

**Clipboard payloads** ([05 §5.12](../spec/05-project-format.md#512-clipboard-format))
follow the same rule: `formatVersion` 2 when a payload holds a template term
or a `refs` entry of the new kinds `struct` and `enum`, else 1. Readers
accept 1 and 2 (today any version but 1 is refused,
`crates/b2c-model/src/clipboard.rs:201-216`). Paste renames the references
inside terms with the other references (`ids.rs:316-353`), lists them in
`refs` (`outside_refs`, `ids.rs:231`) and binds an outside one again by
qualified name and kind, as for variables.

The content hash covers terms like any other field
([05 §5.11](../spec/05-project-format.md#511-content-hash)). Types are not
part of the security summary or the security hash
([08 §8.3.1](../spec/08-security.md#831-how-trust-is-decided)).

### 5. The wire and the editor

* **One spelling.** `Type` serialises as its term, and `Error` as
  `"error"`. `std::string` is `"std::string"` everywhere: in files, in
  `SymbolInfo`, in `blockTypes`, in the conversion table and in the catalog
  (`output = "std::string"`; `string` goes) (*owner to confirm*).
  Deserialising a `Type` from the wire checks the shape, the limits and the
  names, and accepts `"error"`.
* **`b2c-ipc`** replaces its `StaticType` (`pipeline.rs:98-117`, `:239-251`)
  with `TypeJson`, a JSON value checked against `b2c-ir`'s by the existing
  mirror tests. Its TypeScript type is declared by hand through ts-rs's
  type override, as `dto/run.rs:375` does:

  ```ts
  export type TypeJson = string | { readonly [name: string]: TypeJson | readonly TypeJson[] };
  ```

* **`packages/blockly-ext/src/types/`** (new) holds the TypeScript copy of
  §2 and §3, which the checker, the dropdowns and the toolbox use:

  ```ts
  export type StaticType =
    | { readonly kind: 'prim'; readonly name: 'void' | 'bool' | 'char' | 'int' | 'double' | 'std::string' }
    | { readonly kind: 'lib'; readonly name: string; readonly args: readonly StaticType[] }
    | { readonly kind: 'named'; readonly id: string }
    | { readonly kind: 'error' };
  export function parseType(json: unknown): StaticType | null; // shape, limits and names, as in Rust
  export function toTypeJson(type: StaticType): TypeJson;
  export function typeKey(type: StaticType): string;           // = TypeTerm::key
  export function staticConversion(from: StaticType, to: StaticType): StaticConversion;
  export function inClass(type: StaticType, cls: TypeClass, facts: TypeFactsTs): boolean | null;
  export function applyTypeFn(fn: TypeFn, type: StaticType): StaticType | null;
  export function typeLabel(type: StaticType, facts: TypeFactsTs, mode: 'friendly' | 'cpp'): string;
  ```

  `parseType` builds fresh objects and refuses `__proto__`, `constructor`
  and `prototype` (05 §5.6). `TypeFactsTs` answers from the analysis's
  `SymbolInfo`s. `staticConversion` compares types by `typeKey`. The old
  `StaticType` union and its three copies go.
* **The conversion table.** `conversion_table()` keeps its name and row
  shape, over `b2c_ir::types::sample_types()`: the seven M2 types and a
  fixed list of library and user types, about 20 in all. The integration
  test compares every row with `staticConversion` instead of expecting
  49.
* **Type fields in Blockly.** The field's value is the term's key: the M2
  string for a name (`int`, `std::string`), the compact JSON for an object
  (`{"std::vector":"int"}`). Blockly's events and undo stay string-based,
  M2 presets and tests keep their values, and the document sync converts
  between the key and the term (`readB2cField`, `writeB2cField`). The type
  picker (W4) edits the same value; it stays a custom field, a seam
  ADR-0014 allows.

### 6. Connections

The editor keeps `B2cConnectionChecker` and sets no Blockly `check`
strings. While a block is dragged, its output type comes from its catalog
pattern (§7) or from the analysis, and the input's expected type from the
parent's pattern with the parent's variables bound. `conversionAllowed`
refuses only `invalid`, as now. "Any vector" is the pattern
`std::vector<T>` with `T` unbound, or the class `sequence`. An unbound
variable, an unknown type and `error` always connect. ADR-0018 decides when
the editor binds the parent's variables and which oracle answers.

### 7. The catalog schema (L17)

**New keys.**

```toml
[[block]]
id       = "coll.item"
version  = 1
category = "collections"
shape    = "reporter"
output   = "elem(C)"                  # a pattern (below)
where    = { C = "indexable" }        # each type variable and its class
label    = { friendly = "item %INDEX of %LIST", cpp = "%LIST.at(%INDEX)" }
lowering = "builtin"
help     = "The item at this place of the list, or the value for this key of the map."

[[block.field]]
name = "LIST"
kind = "symbol_ref"
type = "C"                            # binds C from the variable's type

[[block.input]]
name    = "INDEX"
check   = "any"
type    = "index(C)"                  # int for a list, the key type for a map
default = [{ num = "0" }]
```

```toml
# var.declare: a type field binds T; the value converts to it
[[block.field]]
name  = "TYPE"
kind  = "type"
types = ["int", "double", "bool", "char", "std::string", "auto"]  # the menu's first choices; auto only here
type  = "T"                                                       # also accepts every type in T's class
# with `where = { T = "value" }` on the block, and on the VALUE input: type = "T"

# func.define: rows accept their listed types and a class
[[block.extra]]
name       = "params"
kind       = "params"
max        = 16
types      = ["int", "double", "bool", "char", "std::string"]
type_class = "value"

# a dropdown whose options need different headers
[[block.field]]
name    = "FN"
kind    = "dropdown"
options = [["square root", "sqrt"], ["larger of", "max"]]
headers = { sqrt = ["<cmath>"], max = ["<algorithm>"] }
```

**The pattern grammar** (TOML strings; types written in C++ syntax):

```ebnf
pattern = "any"                                  (* the analyser decides; M2 *)
        | "number"                               (* int or double by the operands; output only; M2 *)
        | var                                    (* T, K, V, C, E, T2: [A-Z][0-9]? *)
        | name                                   (* int, std::string, std::ifstream *)
        | name "<" pattern { "," pattern } ">"   (* std::vector<T>, std::map<K, V> *)
        | ( "elem" | "index" | "key" ) "(" pattern ")"
        | "field:" NAME                          (* the type chosen in type field NAME; M2 *)
        | "symbol" [ ":" NAME ] ;                (* the type of the symbol in field NAME; M2 *)
```

* `type` goes on value inputs, on `type`, `symbol_ref` and `symbol_decl`
  fields, and applies to each part of a repeated input. `check` stays
  required on inputs and takes every class of §3. `output` takes a pattern;
  M2's values keep their meaning. The entries of `types` lists are written
  in the same C++ syntax (`std::vector<int>` may be one), and `catalog.json`
  exports them as `TypeJson`.
* **What a type field accepts:** its `types` (always; `auto` and `void`
  only from there) and, when it has a `type` pattern, every type that the
  pattern matches within its variables' classes. A row key accepts its
  `types` and its `type_class`. `math.convert`'s `TO`, with `types` and no
  pattern, stays as it is.

**What a signature means.** ADR-0018 decides where and when it is
evaluated; these rules are what it evaluates.

1. A variable is bound at the first part, in this order, whose pattern
   names it outside a function and whose type is known: fields in catalog
   order, value inputs in catalog order (a repeated input by index), then,
   for a variable in `output`, the type the block's place expects. Binding
   unifies the pattern with the part's type: a bare variable takes a type of
   its class; `std::vector<T>` takes `T` from a `std::vector`; a concrete
   type and a function bind nothing.
2. A type field holding `auto`, a field with nothing chosen, an empty input
   without default tokens and a part whose type is `error` bind nothing.
3. Every part is then checked against its resolved pattern: an input by
   `conversion(actual, resolved)` and its `check` class, a `symbol_ref`
   field by exact type (a reference is not converted) and the class, a type
   field by the class. A pattern that does not resolve (an unbound variable,
   a function that does not apply) accepts anything.
4. A `symbol_decl` field with a pattern declares its symbol with the
   resolved type (`error` when it does not resolve).
5. The output is the resolved `output` pattern; `any` and `number` keep
   their M2 meaning.

**Checked when the catalog loads** (`definitions.rs`): each variable is in
`where` once with a known class, and every variable in `where` is used;
a variable that appears only inside functions and not in `output` is an
error (nothing could bind it); patterns parse, name known types with the
right arity and nest at most 8 levels; `type` appears only on the parts
listed above; `field:NAME` names a type field and `symbol:NAME` a symbol
field; a predicate's output is `bool`; every header is in the header table
(§8), and per-option header keys are option values of that dropdown. The
category list (`schema.rs:65-84`) grows with W2's categories.

**The Rust contract** (`crates/b2c-ir/src/sig.rs`, new). `b2c-catalog`
builds it; `b2c-lang` and the editor's copy read it.

```rust
pub struct TypeVar(/* validated: [A-Z][0-9]? */);
pub enum TypePattern {
    Any, Number, Type(Type), Var(TypeVar), Lib(LibType, Vec<TypePattern>),
    Fn(TypeFn, Box<TypePattern>), Field(String), Symbol(Option<String>),
}
impl TypePattern {
    pub fn parse(text: &str) -> Result<TypePattern, PatternError>; // at most 128 bytes
    pub fn vars(&self) -> impl Iterator<Item = &TypeVar>;
}
pub struct BlockSig {
    pub vars: BTreeMap<TypeVar, TypeClass>,
    pub output: Option<TypePattern>,
    pub fields: BTreeMap<String, TypePattern>,
    pub inputs: BTreeMap<String, InputSig>, // a repeated input under its base name
    pub headers: Vec<Header>,
    pub option_headers: BTreeMap<String, BTreeMap<String, Vec<Header>>>, // field, option
}
pub struct InputSig { pub check: TypeClass, pub pattern: Option<TypePattern> }
pub struct SigTable { /* block type ID -> BlockSig */ }
impl SigTable { pub fn get(&self, block_type: &str) -> Option<&BlockSig>; }

pub enum Fit { Fits, Unknown, Mismatch }
pub trait PartTypes { fn field(&self, name: &str) -> Option<Type>; fn symbol(&self, field: Option<&str>) -> Option<Type>; }
pub struct TypeEnv { /* classes of the variables, and what is bound */ }
impl TypeEnv {
    pub fn new(sig: &BlockSig) -> TypeEnv;
    pub fn bind(&mut self, pattern: &TypePattern, actual: &Type, facts: &dyn TypeFacts) -> Fit;
    pub fn resolve(&self, pattern: &TypePattern, parts: &dyn PartTypes) -> Option<Type>;
    pub fn get(&self, var: &TypeVar) -> Option<&Type>;
}
```

* `b2c_catalog::Catalog::sigs(&self) -> &SigTable`, built when the catalog
  loads.
* `b2c_lang::analyze(document, sigs)` and
  `analyze_checked(document, catalog, unchecked, sigs)` take the table
  (`crates/b2c-lang/src/lib.rs:79`, `:104-108` today), so `b2c-lang` reads the
  catalog's contract without depending on `b2c-catalog`.
* `packages/catalog-gen/catalog.json` gains a `types` section (§1's table,
  from `LibType` and the primitives) and, per block, `typeVars`, `output`
  and each part's `type` as tagged JSON (`{"kind": "var", "name": "C"}`,
  `{"kind": "fn", "fn": "elem", "of": …}`, `{"kind": "type", "type":
  TypeJson}`, …), `typeClass` for rows, and `headers` and `optionHeaders`.
  The generated `catalog.ts` and the block reference follow.

### 8. Headers (L8)

* **A header is a typed leaf.**
  `pub struct Header(&'static str)` (`crates/b2c-ir/src/headers.rs`) is
  made only by `Header::standard(name) -> Option<Header>`, a lookup in a
  closed table of the C++ standard library's headers (C++17 to C++26, the
  `<cxxx>` forms only), each with the first standard that has it. It orders
  by its text, so includes sort as today. A file never supplies a header:
  the *use header* block (M3) chooses from the same table.
* **Catalog `headers` are emitted.** A block's `headers` are the headers its
  code needs in every form, whatever its fields, inputs and types; a
  dropdown's `headers` add those of the chosen option. `b2c-lang` adds the
  headers of every block it lowers (enabled and attached) to its item:
  `Item.headers: BTreeSet<Header>` in the SAST.
* **Codegen adds** the headers of every type it names, of every library
  member, operator or range-for it applies to a value that is not a
  literal, of the forms it chooses itself (`<cstdlib>` for `std::exit`,
  `<limits>` and `<string>` for the simple *ask*, `<compare>` for the
  defaulted `operator<=>` of a *comparable* struct), and of the helpers it
  calls. It writes them to the file that holds the code: an item's headers
  where the item's definition goes, a type's headers where the type is
  printed (ADR-0019 §4). `Usage.includes` becomes `BTreeSet<Header>`
  (`lower/mod.rs:31-37`).
* **Types print through typed pieces.** `CType` gains `Lib(LibType,
  Vec<CType>)` and `Named` with the user type's qualified `Ident`s, and
  loses `Copy`; the printer composes `&'static str` names and `Ident`s, and
  spells `std::` names by the rule of
  [06 §6.14.5](../spec/06-compiler-pipeline.md#6145-spelling-standard-names).
  A read-only parameter is `const T&` for every type that is not a scalar
  (`lower/mod.rs:210`).
* **W1 aligns the core catalog** so that no generated file changes:
  `text.literal`, `program.exit`, `math.random_int` and `io.ask` lose the
  headers that only some of their forms need, which codegen adds
  (*owner to confirm*).

### 9. Diagnostics

| Code | Title | Example message | Where |
| --- | --- | --- | --- |
| `B2C-E0140` (new) | Type nested too deeply | *"This type has more than 8 levels or more than 32 names."* | loader |
| `B2C-E0313` (new) | Values that can't be kept in order | *"A set keeps its items in order, so they must be comparable, and `Point` is not. Tick* comparable *on `Point`, or use a list."* | analyser |
| `B2C-E0314` (new) | Can't be printed | *"`Point` can't be printed. Tick* printable *on `Point`, or print its fields one by one."* / *"A list can't be printed in one go. Use* for each *to print its items."* | analyser |
| `E0201`, `E0207` | Not declared; wrong kind | a type that names a deleted struct; a reference to a variable as a type | analyser |
| `E0301` | Wrong type of value | also a part outside its class: *"`score` is a whole number, but this block needs a list, a set, a queue or a stack."* | analyser |
| `E0302`, `E0311` | Operators; conditional values | also library and user types (§3) | analyser |
| `E0602`, `E0607`, `E0614` | Newer block; field value; parameter | also unknown or disallowed types (§4) | catalog |

Messages name constructed and user types by their friendly label ("a list
of int", "a `Student`"), as the type field shows them; primitives keep their
M2 phrases ("a whole number"). `messages::a_type` returns a `String` and
takes the facts. Slot typing (ADR-0018) numbers its own codes from
`B2C-E0315`. Each code gets its entry in `docs/reference/diagnostics/`.

### 10. W1: contracts first, then one package per crate

**Contracts** (one pull request, merged before the packages start):
`crates/b2c-ir` in full, since every package needs it working: `types.rs`
(§2, with `sample_types()`), `sig.rs` (§7), `headers.rs` (§8), the SAST and
`SymbolInfo` additions (§2, `Item.headers`), with their unit tests; the
signature changes of `analyze` and `analyze_checked`; `catalog.json`'s new
shape and the TypeScript declarations of §5 (types only).

**Packages**, in parallel worktrees:

1. **`b2c-model`:** `FieldValue::Type`, the term decoder and limits
   (`E0140`), rows, the canonical writer, `required_format_version`, the
   clipboard version and `refs` kinds, renaming and `outside_refs`, the
   security corpus and the fuzz dictionary.
2. **`b2c-catalog`:** the schema keys, patterns and their checks,
   `type_class`, per-option headers and the header table, domain checks of
   type fields and rows, `E0602` for types, `Catalog::sigs`, the export,
   catalog 1.3.0 and the aligned TOML.
3. **`b2c-lang`:** the type churn of §3 (conversions, classes, operators,
   `E0313`, `E0314`), `from_term`, references to user types, messages, and
   `Item.headers`.
4. **`b2c-codegen`:** recursive `CType` and its printer, includes as
   `Header`s per file, `const T&` parameters.
5. **`b2c-ipc` and `b2c-core-wasm`:** `TypeJson`, the conversion table over
   the samples, `SymbolInfo`; `packages/ipc-types` regenerated.
6. **TypeScript:** `blockly-ext/src/types/`, the checker on `StaticType`
   objects, the type field's key, `catalog-gen`, and the desktop's uses
   (`services/symbols.ts`, `toolbox/values.ts`, `landing.ts`, `reshape.ts`,
   `contents.ts`).

W1 adds no block: every golden file stays byte-identical, which is the
first check of each package.

## Consequences

* **Easier.** A container, an argument type or a library type is a table
  row; a polymorphic block is a few lines of TOML that the analyser, the
  checker and the dropdown filters read alike. The types of all ten M3-D1
  assignments are covered. Headers have one source per construct.
* **Harder.** `Type` grows from 1 byte to about 24, and `Expr` grows with
  it. Rules can no longer assume that anything not
  text is a number; each names a class. Two copies of the rules exist (Rust
  and TypeScript), held together by one generated fixture (Tests). The
  catalog gains a small grammar.
* **Crates and packages.** `b2c-ir` (new modules `sig.rs`, `headers.rs`),
  `b2c-model` (decoder, writer, version, clipboard, ids), `b2c-catalog`
  (schema, definitions, resolve, upgrade, export, TOML), `b2c-lang`
  (typing, messages, lowering, `lib.rs` API), `b2c-codegen` (cast, lower,
  emit), `b2c-ipc`, `b2c-core-wasm`, `b2c-build` and `b2c-cli` (pass the
  `SigTable`), `packages/ipc-types`, `packages/b2c-core-wasm`,
  `packages/blockly-ext`, `packages/catalog-gen` and
  `apps/desktop/src/editor`.
* **Performance.** Type walks are bounded by 32 names; a clone is a
  reference count, except a `Named` (above). Binding a block's patterns
  touches a handful of nodes, in Rust per lowered block and in TypeScript
  per candidate connection. The gated benchmarks decide: the native
  pipeline and the webview preview at 1,000 blocks (N4), with the A/B job on
  the W1 pull request. The tables add a few kilobytes against the WASM
  size budget of 2,000,000 bytes (06 §6.13), which CI checks.
* **Security** (spec 08). No text from a file reaches C++ through a type:
  names are looked up in tables of `&'static str`, user types print the
  validated `Ident` of their declaration
  ([08 §8.4.1](../spec/08-security.md#841-identifiers)), and headers come
  only from the closed table. Terms are bounded at load (8 levels, 32
  names, 64-character names), every type walk is that shallow, and the
  TypeScript parser builds fresh objects. Types change neither the trust
  summary nor the security hash; library-pack types (M5) will, and need
  their own review then. The M3 threat-model review covers terms as a new
  input of the loader and the clipboard.
* **The M2 gap stays:** M2 reports a type it does not know as `E0607`, not
  `E0602` (§4). Version 2 files never reach that path.

### Spec sections to change

* [02 §2.5.1](../spec/02-architecture.md#251-requests-and-responses),
  *Types and casing*: `StaticType` becomes `TypeJson`; the new
  `SymbolInfoKind` kinds.
* [03 §3.5.1](../spec/03-block-language.md#351-the-type-picker): what M3's
  picker offers (§1); the rest stays for later milestones.
* [03 §3.5.2](../spec/03-block-language.md#352-friendly-and-c-display-modes):
  the table's `map from string to number` becomes `map from string to
  double`, and the M2 paragraph says that constructed types are stored as
  terms.
* [03 §3.5.3](../spec/03-block-language.md#353-type-checking-model-gradual):
  library and user types convert only to themselves; the classes.
* [03 §3.7.8](../spec/03-block-language.md#378-collections): *length of*
  gives an `int`.
* [03 §3.11.1](../spec/03-block-language.md#3111-catalog-format): `where`,
  `type`, output patterns, `type_class`, per-option `headers`, what
  `headers` mean, the classes of `check`, `std::string` instead of `string`
  in `output`.
* [03 §3.11.2](../spec/03-block-language.md#3112-template-lowering-library-blocks):
  the input `type = "range<T>"` becomes `type = "R"` with
  `where = { R = "iterable" }`.
* [03 §3.11.3](../spec/03-block-language.md#3113-block-versioning): a new
  type or a widened type field is a compatible change; an unknown type from
  a newer catalog is `E0602`.
* [05 §5.3](../spec/05-project-format.md#53-top-level-structure) and
  [§5.7](../spec/05-project-format.md#57-versioning-and-migration): version 2
  holds *Textbook style* and template terms.
* [05 §5.4](../spec/05-project-format.md#54-block-nodes): type terms in
  fields and rows.
* [05 §5.6](../spec/05-project-format.md#56-validation-limits): a row for
  type terms (8 levels, 32 names, `E0140`).
* [05 §5.12](../spec/05-project-format.md#512-clipboard-format): the
  version rule; `refs` kinds `struct` and `enum`.
* [06 §6.5](../spec/06-compiler-pipeline.md#65-stage--names-and-scopes):
  `SymbolInfo` types are `TypeJson`; the new kinds; what `conversion` does
  with library and user types.
* [06 §6.6](../spec/06-compiler-pipeline.md#66-stage--types-flow-checks-and-lints):
  the `Type` sketch becomes §2's, with `Param`, `Pointer`, `Ref`,
  `Function` and `Opaque` left for later milestones.
* [06 §6.8.2](../spec/06-compiler-pipeline.md#682-typed-text-parse-dont-validate):
  types and headers are printed from tables and validated names.
* [06 §6.10](../spec/06-compiler-pipeline.md#610-includes-and-support-helpers):
  where includes come from (§8) and the header table.
* [08 §8.4.5](../spec/08-security.md#845-expression-slots-and-templates):
  type terms and headers.
* [09 §9.2](../spec/09-quality-and-delivery.md#92-testing-strategy): the
  shared type fixture, kept fresh like `catalog.json`.

## Tests

**Rust.**

* `b2c-ir`: terms round-trip through canonical JSON with one spelling each
  (a one-element array, an empty array and an object with two keys are
  refused); 8 levels and 32 names pass, 9 and 33 do not; `from_term` and
  `to_term` round-trip over generated types (proptest); `Type::lib`'s
  arity, value and error rules; `conversion` for every pair of
  `sample_types()`; class membership, type functions, labels and C++
  spellings by table; `size_of::<Type>() <= 24`; patterns parse, print and
  refuse bad input; `TypeEnv` binds `T` from `std::vector<T>`, keeps the
  first binding, binds nothing through a function, and reports a class
  mismatch; the header table is sorted, unique and in `<…>` form.
* `b2c-ir` also writes `packages/blockly-ext/src/types/fixture.json`: the
  samples, their keys, conversions, classes under a sample set of facts,
  type functions, labels and pattern bindings. It is committed, and CI
  fails when it is stale, as for `catalog.json`.
* `b2c-model`: decoder cases for each shape and error (`E0112`, `E0113`,
  `E0127`, `E0140`); writer round-trip; `required_format_version` is 1 for
  every example, golden project and assignment that uses no template, and 2
  with a template in a field, in a row or with *Textbook style*; a
  version 3 file is `E0108`; clipboard versions 1 and 2 and the new `refs`
  kinds; renaming and `outside_refs` inside terms; new security-corpus
  files (deep, wide, odd keys, bidi in names); the `load` and `clipboard`
  fuzz dictionaries gain term tokens, and the decoder joins the mutation
  testing list.
* `b2c-catalog`: each new definition check; type-field and row domains;
  `E0602` for an unknown type under a newer catalog; `Catalog::sigs` covers
  every block; the export test regenerates `catalog.json`.
* `b2c-lang`: conversions of library and user types; operators on them
  (`E0302`);
  `E0313` and `E0314`; references to user types (`E0201`, `E0207`);
  `Item.headers` comes only from lowered blocks and includes options'
  headers; the SAST snapshots change only in `std::string`'s spelling.
* `b2c-codegen`: nested types and user names print right, also under
  *Textbook style*; type headers land in the file that prints the type;
  read-only parameters of library and user types are `const T&`. Every
  golden file stays byte-identical.
* `b2c-ipc`, `b2c-core-wasm`: the mirror test for `TypeJson`;
  `conversion_table` over the samples; `SymbolInfo` with `std::string`.
* A program that declares a variable of every sample value type, a
  *comparable* struct used on its own (which needs `<compare>`), and a
  `std::set` and a `std::map` keyed by that struct, compiles with
  `-fsyntax-only` under C++17 and C++20 with the *strict* warnings, on
  GCC 11 and GCC 15 in CI (with W1's catalog snapshot test).

**TypeScript (Vitest).**

* `blockly-ext/src/types/`: every case of the Rust fixture agrees;
  `parseType` refuses prototype keys, over-deep terms and unknown names.
* `apps/desktop/src/editor/sync/integration.test.ts`: `staticConversion`
  equals the core's table on every row.
* Checker: unknown types and `error` connect; a `std::vector<int>` value is
  refused where a whole number is required; M2's outputs (`symbol`,
  `field:NAME`, `number`) behave as before.
* Type field: an M2 value keeps its string; a template term round-trips
  through the field's key; a value the field does not know is kept.
* `catalog-gen`: patterns, classes, the `types` section and headers are
  generated.

**End to end and benchmarks.** W1 changes no visible behaviour beyond the
checker, so the existing flows run unchanged; W2's flows cover the new
blocks. The A/B job on the W1 pull request shows no gated regression.

## Owner to confirm

1. Format version 2 holds both M3 keys (*Textbook style* and template
   terms), instead of giving types the next number after `project.lints`
   (*owner to confirm*).
2. `std::string` is written `std::string` on the wire too, not `string` as
   in M2's `SymbolInfo`, so each type has one spelling everywhere
   (*owner to confirm*).
3. M3 has no unsigned or sized integers; sizes are `int`, converted with
   `static_cast<int>` where a warning would follow (*owner to confirm*).
4. Streams and errors are types of declared symbols only; type fields do
   not offer them in M3 (*owner to confirm*).
5. Catalog `headers` become the headers a block always needs and are
   emitted; W1 removes from `text.literal`, `program.exit`,
   `math.random_int` and `io.ask` the headers only some of their forms
   need, so that no golden file changes (*owner to confirm*).
6. The limits of a type: 8 levels and 32 names (*owner to confirm*).
