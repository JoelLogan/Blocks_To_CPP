# M3 contracts: types, catalog signatures and headers

The exact shapes behind [ADR-0017](../../adr/0017-open-type-representation.md)
(L14 types, L17 catalog signatures, L8 headers), and the two last language
foundations of W1, L16 (`+` on text) and L10 (constant folding), in §11.
The ADR holds the decisions
and their reasons; this file holds what W1's packages build against: the
Rust and TypeScript signatures, the tables, the grammar, the file shapes,
the messages, the W1 plan, the test list and the spec changes. Typed slots
(ADR-0018) build on §1, §4 and §6 here; their own contract is
[slots.md](slots.md).

A rule stated here is as binding as one in the ADR. If the two disagree,
the ADR wins and this file is corrected.

## 1. Types (`crates/b2c-ir/src/types.rs`)

### 1.1 The types of M3

| Kind | Types | JSON name | Friendly label | Header |
| --- | --- | --- | --- | --- |
| primitive | `void` (results only), `bool`, `char`, `int`, `double` | the C++ name | as in M2 | — |
| text | `std::string` | `std::string` | `string` | `<string>` |
| container | `std::vector<T>`, `std::deque<T>`, `std::set<T>`, `std::queue<T>`, `std::stack<T>`, `std::priority_queue<T>` | `std::vector`, … | `list of T`, `deque of T`, `set of T`, `queue of T`, `stack of T`, `priority queue of T` | `<vector>`, `<deque>`, `<set>`, `<queue>`, `<stack>`, `<queue>` |
| map | `std::map<K, V>` | `std::map` | `map from K to V` | `<map>` |
| file stream | `std::ifstream`, `std::ofstream` | the C++ name | `input file`, `output file` | `<fstream>` |
| error | `std::exception`, `std::runtime_error`, `std::invalid_argument`, `std::out_of_range` | the C++ name | `error`, `runtime error`, `invalid argument error`, `out-of-range error` | `<exception>`, `<stdexcept>` |
| user | a struct or an enum, by the symbol ID of its definition | `{"ref": id}` | its name | — |

A template argument is a *value* type: not `void`, a stream or an error.

**Not in M3:** classes (M4), pointers, references other than the parameter
modes and the implicit `const&` of *for each* and *catch*, smart pointers,
`std::function`, user templates (M4), `std::array`, `std::pair`,
`std::tuple`, `std::optional`, `std::variant`, the unordered containers,
`float`, `long double`, the `long` and `unsigned` integers, library-pack
types (M4/M5) and the opaque type of Raw C++ (M4). Type aliases are not in
M3's block list either; when they come, `{"ref": id}` to an alias stands for
its target type, as in C++, and declarations keep the alias's name for
printing.

### 1.2 Rust

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
    /// Class membership (§1.3). `Some(true)` for `Error`, which is in every class;
    /// `None` when not known: a `Named` that `facts` does not know, or a library type
    /// whose answer depends on such an argument.
    pub fn is_in(&self, class: TypeClass, facts: &dyn TypeFacts) -> Option<bool>;
    pub fn cpp<'a>(&'a self, facts: &'a dyn TypeFacts) -> impl fmt::Display + 'a;   // std::vector<Student>
    pub fn label<'a>(&'a self, facts: &'a dyn TypeFacts) -> impl fmt::Display + 'a; // list of Student
}

/// The seven M2 types and a fixed list of library and user types (about 20), for the
/// conversion table and the fixture (§9).
pub fn sample_types() -> Vec<Type>;

/// A type as files and the wire write it, before its names are looked up (§2).
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
    /// `Name(n)` gives `n` itself (`int`, `std::string`); every other term gives its
    /// canonical compact JSON (`{"std::vector":"int"}`, `{"ref":"sym_x"}`).
    /// Equal terms give equal keys.
    pub fn key(&self) -> String;
}

/// What the type rules need to know about user types.
pub trait TypeFacts {
    fn user_type(&self, id: &SymbolId) -> Option<UserType<'_>>;
}
pub struct UserType<'a> {
    pub name: &'a str,
    pub kind: UserKind,
    /// Ticked *comparable* and every field's type is comparable (§1.6).
    pub comparable: bool,
    /// Ticked *printable* and every field's type is printable (§1.6).
    pub printable: bool,
}
pub enum UserKind { Struct, Enum }

/// Classes of types (§1.3), moved here from `b2c-catalog`'s `schema.rs:365-379`
/// (which re-exports it); serde names in snake_case, as catalog files write them.
pub enum TypeClass {
    Any, Number, Integer, Bool, Text, Value,
    Collection, Sequence, Indexable, Insertable, Searchable, Iterable,
    Keyed, Adaptor, Listable, Sortable, Switchable,
    Comparable, Printable, Struct, Enum, InputFile, OutputFile, Exception,
}

/// The type functions of catalog patterns (§1.4, §4).
pub enum TypeFn { Elem, Index, Key }
impl TypeFn { pub fn apply(self, ty: &Type) -> Option<Type>; }
```

* `TermError` names each problem: too deep, too many names, a bad name, a
  bad shape, an unknown name or library type, a wrong number of arguments,
  `auto`, and a type that cannot be an argument (`void`, a stream, an
  error).
* `Named` holds a `SymbolId` (`Box<str>`, `crates/b2c-ir/src/ids.rs:36`),
  so its clone allocates; if the benchmarks show it, `SymbolId` moves to
  `Arc<str>` without an API change.
* `cpp` writes `std::vector<std::vector<int>>` with the `std::` names in
  full and user types by their name; messages and the editor's C++ labels
  use it, codegen does not (§5). `label` writes `list of list of int` and
  `map from string to list of int`; the first argument of a map is put in
  parentheses when it is itself a map.
* **Symbols.** `SymbolKind` (`crates/b2c-ir/src/sast.rs:523-541`) gains
  `Struct { fields: Vec<SymbolId>, comparable: bool, printable: bool }`,
  `Field { owner: SymbolId }`, `Enum { values: Vec<SymbolId> }` and
  `Enumerator { owner: SymbolId }`. `SymbolInfoKind`
  (`crates/b2c-ir/src/scope_info.rs:49`) gains `struct`, `field`, `enum` and
  `enumerator` with the same data in camelCase; the two flags are the
  effective ones of `UserType`. The type of a struct or enum symbol is
  `Named` of itself, an enumerator's is `Named` of its enum, and a field's is
  its declared type. `SymbolTable` implements `TypeFacts`.

### 1.3 Classes

| Class | Members (`where`, and `check` except where noted) |
| --- | --- |
| `any` | every type but `void` |
| `number` | `int`, `double`, `char`. In `check`: every type that converts to `double` without `Invalid` |
| `integer` | `int`, `char`. In `check`: every type that converts to `int` without `Invalid` |
| `bool` | `bool`. In `check`: every type that converts to `bool` without `Invalid` |
| `text` | `std::string`, `char` |
| `value` | what a variable can hold: `bool`, `char`, `int`, `double`, `std::string`, the six containers, `std::map`, structs and enums |
| `collection` | the six containers and `std::map` |
| `sequence` | `std::vector`, `std::deque` |
| `indexable` | `std::vector`, `std::deque`, `std::map` |
| `insertable` | `std::vector`, `std::deque`, `std::set`, `std::queue`, `std::stack`, `std::priority_queue` |
| `searchable` | `std::vector`, `std::deque`, `std::set`, `std::map` |
| `keyed` | `std::set`, `std::map` |
| `adaptor` | `std::queue`, `std::stack`, `std::priority_queue` |
| `listable` | `std::vector`, `std::deque`, `std::set` |
| `sortable` | `std::vector` and `std::deque` of a comparable element |
| `iterable` | `std::vector`, `std::deque`, `std::set`, `std::string` (not `std::map`: see §1.5) |
| `switchable` | `char`, `int`, enums |
| `comparable` | `bool`, `char`, `int`, `double`, `std::string`, enums, structs whose `UserType::comparable` holds, and `std::vector`, `std::deque`, `std::set`, `std::map`, `std::queue`, `std::stack` whose arguments are all comparable |
| `printable` | `bool`, `char`, `int`, `double`, `std::string`, enums, structs whose `UserType::printable` holds |
| `struct`, `enum` | user structs; user enums |
| `input_file`, `output_file`, `exception` | `std::ifstream`; `std::ofstream`; the four errors |

* `Error` is in every class. A `Named` the facts do not know answers
  `None`, and so does a library type whose answer depends on one; `None`
  connects (03 §3.5.3).
* For a container, `comparable` is `false` as soon as one argument is not
  comparable, `true` when all are, and `None` otherwise.
* `switchable` leaves out `bool`: g++ warns about a `switch` on a `bool`
  (`-Wswitch-bool`, on by default), and an `if` says it better.

### 1.4 Type functions

| Function | `vector<T>`, `deque<T>` | `set<T>` | `map<K, V>` | `queue<T>`, `stack<T>`, `priority_queue<T>` | `std::string` |
| --- | --- | --- | --- | --- | --- |
| `elem` (what one item is) | `T` | `T` | `V` | `T` | `char` |
| `index` (what *item* takes) | `int` | — | `K` | — | `int` |
| `key` (what *contains* and *remove (x)* look for) | `T` | `T` | `K` | — | — |

A function applied to `Error` gives `Error`. A function that does not apply
gives `None`; with the classes of `where` chosen as in §1.5 that cannot
happen, and `TypeEnv::resolve` treats it as `Resolution::Missing`.

### 1.5 The collection blocks of W2

How 03 §3.7.8's rows map onto classes and patterns. W2 writes the blocks;
the rows that change in the spec are marked *changed*.

| Block | `C` (`where`) | Patterns | Lowering |
| --- | --- | --- | --- |
| `add (x) to [c]` | `insertable` | `x`: `elem(C)` | `push_back`, `insert` or `push` |
| `item (i) of [c]` | `indexable` | `i`: `index(C)`; output `elem(C)` | `c.at(i)` |
| `set item (i) of [c] to (x)` | `indexable` | `i`: `index(C)`; `x`: `elem(C)`, `assign` | `c.at(i) = x` (vector, deque), `c[k] = x` (map) |
| `next item of [c]` (*changed*: the queue/stack column of *item of*) | `adaptor` | output `elem(C)` | `c.front()` (queue), `c.top()` (stack, priority queue) |
| `remove item at (i) from [c]` (*changed*: only vector and deque) | `sequence` | `i`: `index(C)` | `c.erase(c.begin() + i)` |
| `remove (x) from [c]` (*changed*: new row, set and map) | `keyed` | `x`: `key(C)` | `c.erase(x)` |
| `remove next from [c]` (*changed*: new row, queue/stack column of *remove*) | `adaptor` | — | `c.pop()` |
| `length of [c]` | `collection` | output `int` | `static_cast<int>(c.size())` |
| `[c] contains (x)` | `searchable` | `x`: `key(C)` | `std::ranges::find(c, x) != c.end()` or `c.contains(x)` (C++20) |
| `[c] is empty`, `clear [c]` | `collection` | — | `c.empty()`, `c.clear()` (a helper for adaptors) |
| `sort [c] [ascending ▾]` | `sortable` | — | `std::ranges::sort(c)` |
| `list of (a) (b) ⊕` (*changed*: no map form in M3) | `listable`, from its place | items: `elem(C)` | `{a, b}` |
| `for each [x] in (c)` | `iterable` | declares `x`: `elem(C)` | `for (const auto& x : c)` |
| `for each key [k], value [v] in [m]` (*changed*: new row) | none | `m`: `std::map<K, V>`; declares `k`: `K`, `v`: `V` | `for (const auto& [k, v] : m)` (C++17) |

* A map is filled with *set item … of*; a map form of *list of* waits until
  pairs exist (after M3).
* `std::map` is not `iterable`: a range-`for` over it yields
  `std::pair<const K, V>`, which M3 cannot name, so `for (const int& v : m)`
  does not compile (checked with g++ 11 and 14). *for each key, value* covers
  `dice_statistics` (`tests/assignments/dice_statistics/reference.cpp:40`).
* `switch`: its operand has `type = "T"` with `where = { T = "switchable" }`;
  each case label is a repeated value part with `type = "T"`, holding a
  literal or an enumerator (`[Action] :: [Deposit]`). That a label is a
  constant, and that no two are equal, the analyser checks with its own rule
  (W2 allocates a code if it needs one).
* `[Color ▾] :: [Red ▾]`: field `ENUM` (`symbol_ref`, kind enum,
  `type = "E"`, `where = { E = "enum" }`), field `VALUE` (`symbol_ref`,
  `members = "E"`); output `E`.
* `new [Student] with name: (…) mark: (…)`: type field `TYPE` with
  `type = "S"`, `where = { S = "struct" }`; repeated input `FIELD` with
  `type = "member:TYPE"`; output `S`.
* `[s] . [mark]`: input `OBJ` with `type = "S"`, `where = { S = "struct" }`;
  field `MEMBER` (`symbol_ref`, `members = "S"`); output `symbol:MEMBER`.

### 1.6 Operators, requirements and generated operators

* **Conversions.** `conversion(from, to)` (`crates/b2c-lang/src/typing.rs:97-109`)
  keeps its five results and five rules. A library or user type converts
  only to the same type (rule 1, structural equality) and is `Invalid`
  otherwise (rule 5); the match needs no new arm, and its documentation
  says so.
* **Comparisons** (`==`, `!=`, `<`, `<=`, `>`, `>=`), in this order:
  1. both operands in `number` or `bool`: as in M2
     (`crates/b2c-lang/src/lower/expr.rs:924-955`): the usual conversions,
     `W0519` for `bool` against a number and for ordering two `bool`s,
     `W0511` for `==` or `!=` with a `double`;
  2. text with text: `bool`, as now;
  3. text with anything else: `E0302`, as now;
  4. otherwise both operands must have the same type, and that type must be
     `comparable`, else `E0302`.
* **Arithmetic** needs operands in `number` or `bool` (membership), checked
  explicitly instead of "not text" (`lower/expr.rs:865-889`,
  `typing.rs:122-129`); text keeps its M2 messages, anything else is
  `E0302`.
* **Requirements.** The element of `std::set` and `std::priority_queue` and
  the key of `std::map` must be `comparable`: `E0313` where the type is
  written (the type field, the row). Declaring such a set compiles, but
  every insert, lookup or ordering fails, so the declaration is reported.
  *print*, *join* and *write to file* need `printable` values (`E0314`).
* **Ticks.** A struct ticked *comparable* or *printable* whose field's type
  is not comparable or printable is `E0313` or `E0314` at the tick, naming
  the field, and the class answers `false` for it. Without this, g++
  deletes the defaulted `<=>` (`std::set<Outer>` with a non-comparable
  field fails to compile on g++ 11 to 14) or finds no `operator<<` for a
  list field. `UserType`'s flags are worked out when the analyser declares
  the structs; a struct reached again while its flags are worked out counts
  as not comparable and not printable.
* **Generated operators** (codegen, §5.2): a comparable struct gets the
  defaulted `auto operator<=>(const S&) const = default;` in C++20, and six
  hidden friends (`==`, `!=`, `<`, `>`, `<=`, `>=`, built on `std::tie`) in
  C++17; with only `==` and `<`, `!=`, `>` and `<=` do not compile in C++17,
  and `>` and `<=` not in C++20 either (checked with g++ 11 and 14). A
  printable struct gets a hidden-friend `operator<<`. An enum that is
  printed or written gets an `operator<<`, and one that is joined a
  `to_string`: `std::cout << Color::Red` does not compile for an
  `enum class`.

### 1.7 Messages

Messages name constructed and user types by their friendly label ("a list
of int", "a `Student`"), as the type field shows them; primitives keep their
M2 phrases ("a whole number"). `messages::a_type` returns a `String` and
takes the facts.

| Code | Example message |
| --- | --- |
| `B2C-E0140` | *"This type has more than 8 levels or more than 32 names."* |
| `B2C-E0313` | *"A set keeps its items in order, so they must be comparable, and `Point` is not. Tick* comparable *on `Point`, or use a list."* / *"`Team` is ticked* comparable*, but its field `members` holds `Player`, which is not comparable. Tick* comparable *on `Player` too, or untick it here."* |
| `B2C-E0314` | *"`Point` can't be printed. Tick* printable *on `Point`, or print its fields one by one."* / *"A list can't be printed in one go. Use* for each *to print its items."* |
| `E0301` | also a part outside its class: *"`score` is a whole number, but this block needs a list, a set, a queue or a stack."* |
| `E0302` | *"Can't compare a list of int with a list of double."* |
| `E0201`, `E0207` | a type that names a deleted struct; a reference to a variable as a type, or to a user type of a kind the class does not hold |

## 2. Files and the clipboard (`b2c-model`)

### 2.1 Encoding

```json
"TYPE": "int"
"TYPE": {"ref": "sym_Pt12"}
"TYPE": {"std::vector": {"std::vector": "int"}}
"TYPE": {"std::map": ["std::string", {"std::vector": {"ref": "sym_Pt12"}}]}
"params": [{"sym": "sym_g", "name": "grades", "type": {"std::vector": "int"}, "mode": "read_only"}]
```

* A name is a string: a primitive, `std::string`, a nullary library type,
  or `auto` and `void` where a field allows them. Every M2 file is unchanged,
  and so are its content hash and its version.
* A user type is `{"ref": id}`, the format's one spelling of a reference.
* A template is an object with exactly one key, the template's qualified
  C++ name, whose value is its argument, or an array of its two or more
  arguments. One argument is never written as a one-element array, and an
  empty array is never written: each type has one spelling.
* Terms appear in type fields and in the `type` of `extra` rows (`params`,
  and the row keys W2 adds for struct fields and enum values).
* The content hash covers terms like any other field
  ([05 §5.11](../../spec/05-project-format.md#511-content-hash)). Types are
  not part of the security summary or the security hash
  ([08 §8.3.1](../../spec/08-security.md#831-how-trust-is-decided)).
* **Upgrades.** M2 projects need none: their types are valid terms, their
  version stays 1, and opening them marks nothing as changed. A block
  migration that rewrites a type keeps its references and may not deepen the
  term: under ADR-0015 that counts as adding nesting.

### 2.2 The decoder

* `FieldValue` (`crates/b2c-model/src/document.rs:470-480`) gains
  `Type(TypeTerm)`, always a template; a type field holding a name stays
  `Text` and one holding a user type stays `Ref`.
  `FieldValue::type_term(&self) -> Option<TypeTerm>` reads any of the three
  as a term, and the rows' `type` is read with the same helper.
* An object in a field is read (`decode/workspace.rs:549-586`):
  1. with the key `ref`: a reference, as now;
  2. with exactly one key that is a qualified name (it contains `::` and
     passes the name rule below): a template term;
  3. otherwise: a declaration, as now, so an unknown key is still `E0110`,
     or `E0108` when a newer app saved the file (ADR-0013).

  So a damaged declaration such as `{"sm": "sym_x", "name": "score"}` keeps
  its M2 code instead of turning into a template called `sm`.
* **Names.** 1 to 64 ASCII characters: identifiers joined by `::` or single
  spaces. A template's key follows the same rule and must be qualified.
* **Limits.** At most `Type::MAX_DEPTH` (8) levels of templates and
  `Type::MAX_NAMES` (32) names and references in one term, else `B2C-E0140`.
  Both sit within the JSON depth limit of 128, so they add no new resource
  bound; they keep every type walk shallow.
* **Errors.** A wrong shape (a one-element or empty array, a number, a
  nested `null`) is `E0112`, an invalid ID `E0113`, a reserved key `E0127`,
  as for other values.
* The decoder checks the `type` of the rows of the known row keys the same
  way and keeps `extra` as JSON.
* The catalog checks names (`E0607` in a field, `E0614` in a row, `E0602`
  from a newer catalog); the analyser checks references (`E0201` for a
  missing symbol, `E0207` for one that is not a user type of a kind the
  part's class holds).

### 2.3 Versions

```rust
// crates/b2c-model/src/version.rs
/// The newest format version this build reads and writes (replaces
/// `CURRENT_FORMAT_VERSION`, `document.rs:18`).
pub const MAX_FORMAT_VERSION: u32 = 2;
/// The oldest version that loads without a migration step. Versions from here to
/// `MAX_FORMAT_VERSION` differ only by added keys (ADR-0013).
pub const OLDEST_UNMIGRATED_VERSION: u32 = 1;
/// The lowest format version whose keys cover the document (ADR-0013): 2 when it
/// sets `options.usingNamespaceStd` or holds a template term (in a field or in the
/// `type` of an `extra` row), else 1.
pub fn required_format_version(document: &Document) -> u32;
```

* **Loading** (`crates/b2c-model/src/load.rs:284-305`): a version above
  `MAX_FORMAT_VERSION` is `E0108`, as now. A version from
  `OLDEST_UNMIGRATED_VERSION` to `MAX_FORMAT_VERSION` loads with no
  migration step. Only an older version runs the chain, and only up to
  `OLDEST_UNMIGRATED_VERSION`: `migrate::upgrade` runs
  `run_chain(root, from, OLDEST_UNMIGRATED_VERSION, MIGRATIONS)`
  (`migrate.rs:77-78`). With no meaning change today the chain never runs,
  so a version 1 file never meets the empty chain's `NoPath`. A future
  change of meaning raises `OLDEST_UNMIGRATED_VERSION` and adds its steps
  (identity steps for the purely additive versions in between).
* **Writing.** The writer writes `required_format_version`, not the stored
  number (`save.rs:293`), so a file that stops using templates goes back to
  version 1.
* **`migratedFrom`** (`crates/b2c-app/src/projects.rs:229-232`) compares with
  `OLDEST_UNMIGRATED_VERSION`, so it reports only real migrations
  (05 §5.7); its test at `:725` stands as it is.
* **`version_info().format_version`** (`crates/b2c-core-wasm/src/facade.rs:71`)
  reports `MAX_FORMAT_VERSION`.
* **TypeScript.** `CURRENT_FORMAT_VERSION` in
  `packages/b2c-core-wasm/src/bdm.ts:23` becomes `MAX_FORMAT_VERSION = 2`,
  with its export (`index.ts:70`) and test, in the same pull request.

### 2.4 The clipboard

* **Version.** `formatVersion` 2 when a payload holds a template term or a
  `refs` entry of a new kind, else 1. Readers accept 1 and 2: today any
  version below the current one is refused
  (`crates/b2c-model/src/clipboard.rs:201-216`), which the same rule as
  §2.3 replaces.
* **`refs` kinds.** `RefKind` (`clipboard.rs:83-92`) gains `struct`, `enum`,
  `field` and `enumerator`.
* **Names.** A struct or enum is recorded by its qualified name
  (`::Student`, as functions are recorded today); a field or an enumerator
  by its owner's qualified name and its own (`::Student::mark`,
  `::Action::Deposit`).
* **Binding again** (`crates/b2c-core-wasm/src/clipboard.rs:262-320`):
  * a struct or enum binds to the visible user type with that qualified
    name and kind, by the same rule as a function;
  * a field or enumerator keeps its ID when its owner is the same symbol at
    the target; when the owner was bound to another type, it binds to that
    type's member of the same name; otherwise it is unresolved.
* **Walkers.** Paste renames the references inside terms with the other
  references (`ids.rs:316-353`), and `outside_refs` (`ids.rs:231-252`)
  lists them.

## 3. The wire and the editor

### 3.1 One spelling

* `Type` serialises as its term, and `Error` as `"error"`. `std::string`
  is `"std::string"` everywhere: files, `SymbolInfo`, `blockTypes`,
  `slotTypes`, the conversion table and the catalog
  (`output = "std::string"`; `string` goes).
* Deserialising a `Type` checks the shape, the limits and the names, and
  accepts `"error"`.
* `b2c-ipc` replaces its `StaticType` (`crates/b2c-ipc/src/pipeline.rs:98-117`,
  `:239-251`) with `TypeJson`, a JSON value checked against `b2c-ir`'s by
  the existing mirror tests. Its TypeScript type is declared through
  ts-rs's override, as `dto/run.rs:375` does:

  ```ts
  export type TypeJson = string | { readonly [name: string]: TypeJson | readonly TypeJson[] };
  ```

* `packages/b2c-core-wasm/src/bdm.ts:210`'s union becomes
  `FieldValue = boolean | string | SymbolDecl | SymbolRef | TypeJson`.

### 3.2 The TypeScript copy (`packages/blockly-ext/src/types/`)

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
export function checkAccepts(cls: TypeClass, type: StaticType, facts: TypeFactsTs): boolean | null;
export function applyTypeFn(fn: TypeFn, type: StaticType): StaticType | null;
export function typeLabel(type: StaticType, facts: TypeFactsTs, mode: 'friendly' | 'cpp'): string;
```

* `parseType` builds fresh objects and refuses `__proto__`, `constructor`
  and `prototype` (05 §5.6). `TypeFactsTs` answers from the analysis's
  `SymbolInfo`s. `staticConversion` compares types by `typeKey`. The old
  `StaticType` string union and its three copies
  (`packages/blockly-ext/src/checker/types.ts:12-23`,
  `apps/desktop/src/editor/services/symbols.ts:25-33`, the generated
  `packages/ipc-types/src/generated/types.ts`) go.
* **The conversion table.** `conversion_table()`
  (`crates/b2c-core-wasm/src/scope.rs:47-58`) keeps its name and row shape,
  over `sample_types()`. The integration test
  (`apps/desktop/src/editor/sync/integration.test.ts:455-461`) compares every
  row with `staticConversion` instead of expecting 49.
* **Type fields in Blockly.** The field's value is the term's key (§1.2):
  `int`, `std::string`, `{"std::vector":"int"}`, `{"ref":"sym_x"}`. Blockly's
  events and undo stay string-based, M2 presets and tests keep their
  values, and the document sync converts between key and term
  (`readB2cField`, `writeB2cField`). The type picker (W4) edits the same
  value as a custom field (ADR-0014).

## 4. Catalog signatures (`crates/b2c-ir/src/sig.rs`, `b2c-catalog`)

### 4.1 Keys

```toml
[[block]]
id       = "coll.item"
version  = 1
category = "collections"
shape    = "reporter"
output   = "elem(C)"                  # a pattern (§4.2)
where    = { C = "indexable" }        # each type variable and its class
label    = { friendly = "item %INDEX of %LIST", cpp = "%LIST.at(%INDEX)" }
lowering = "builtin"
help     = "The item at this place of the list, or the value for this key of the map."

[[block.field]]
name = "LIST"
kind = "symbol_ref"
type = "C"                            # binds C from the variable's type

[[block.input]]
name  = "INDEX"
check = "any"
type  = "index(C)"                    # int for a list, the key type for a map; no default (§4.3)
```

```toml
# var.declare: a type field binds T; the value converts to it
[[block.field]]
name  = "TYPE"
kind  = "type"
types = ["int", "double", "bool", "char", "std::string", "auto"]  # the menu's first choices; auto only here
type  = "T"                                                       # also accepts every type in T's class
# with `where = { T = "value" }` on the block, and `type = "T"` on NAME and VALUE

# var.set: the reference binds T; the value is assigned
[[block.input]]
name   = "VALUE"
check  = "any"
type   = "T"
assign = true                         # char also fits std::string; never binds

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

# the member block's member list
[[block.field]]
name    = "MEMBER"
kind    = "symbol_ref"
members = "S"                         # lists the fields of the struct bound to S
```

* `type` goes on value inputs, on `type`, `symbol_ref` and `symbol_decl`
  fields, and applies to each part of a repeated input. `check` stays
  required on inputs and takes every class of §1.3. `output` takes a
  pattern; M2's values keep their meaning. The entries of `types` lists are
  written in the same C++ syntax (`std::vector<int>` may be one), and
  `catalog.json` exports them as `TypeJson`.
* **What a type field accepts:** its `types` (always; `auto` and `void` only
  from there) and, when it has a `type` pattern, every type the pattern
  matches within its variables' classes. A row key accepts its `types` and
  its `type_class`. `math.convert`'s `TO`, with `types` and no pattern,
  stays as it is.
* `assign = true` on a value input: the value is assigned, not used to
  initialise, so a `char` also fits a `std::string`, as C++ allows
  (`s = 'a';` compiles, `std::string s = 'a';` does not;
  `typing.rs:20-27`). An `assign` input never binds a variable; it is only
  checked against it. Used by `set` and *set item … of*. Default `false`.
* `members = "S"` on a `symbol_ref` field: the field lists the members of
  the user type bound to `S` instead of the symbols in scope: a struct's
  fields, an enum's enumerators (M4 adds methods). `S`'s class must be
  `struct` or `enum`. The field binds nothing.
* `headers` on a block and on a dropdown's options (§5).

### 4.2 Grammar

TOML strings; types are written in C++ syntax.

```ebnf
pattern = "any"                                  (* the analyser decides; M2 *)
        | "number"                               (* int or double by the operands; output only; M2 *)
        | var                                    (* T, K, V, C, E, S, T2: [A-Z][0-9]? *)
        | name                                   (* int, std::string, std::ifstream *)
        | name "<" pattern { "," pattern } ">"   (* std::vector<T>, std::map<K, V> *)
        | ( "elem" | "index" | "key" ) "(" pattern ")"
        | "field:" NAME                          (* the type chosen in type field NAME; M2 *)
        | "symbol"                               (* the type of the symbol in the block's one symbol field; M2 *)
        | "symbol:" NAME                         (* the type of the symbol in field NAME; new *)
        | "param:" NAME                          (* repeated input: parameter i of the function in field NAME; new *)
        | "member:" NAME                         (* repeated input: field i of the struct in field NAME; new *)
        | "result" ;                             (* what a return gives back here; new *)
```

* `i` is the part's index (`ARG2` is parameter 2). `param:`, `member:` and
  `result` resolve to `Missing`, so anything fits, when the function or
  struct is not known or has fewer parameters or fields (the analyser
  reports the count itself); `result` also when the function returns `void`
  or the block is in no definition (parked, in the toolbox). A
  pattern is at most 128 bytes and 8 levels deep.
* `param:` names a `symbol_ref` field; `member:` a `type` or `symbol_ref`
  field. Both appear only on a repeated value input; `result` only on a
  value input.

### 4.3 What a signature means, and what the catalog checks

**Rules** (where and when they are evaluated is ADR-0018's):

1. **Binding.** A variable is bound by the first part, in catalog order
   (fields and value inputs as the catalog lists them, a repeated input
   part by part), whose pattern names it outside a function and whose type
   is known. Binding unifies the pattern with the part's type: a bare
   variable takes a type of its class; `std::vector<T>` takes `T` from a
   `std::vector`; a concrete type and a function bind nothing. An `assign`
   input and a `members` field never bind.
2. **What binds nothing.** A type field holding `auto`, a field with nothing
   chosen, an empty input and a value typed from its place bind nothing and
   leave the variable for a later part. A part whose type is unknown or
   `Error`, or whose binding fails (outside the class, or the wrong shape),
   binds the variable to `Error`, so nothing that depends on it is checked
   (ADR-0018 §2 gives the reason).
3. **The place.** Only a *place-typed* block binds from its place: one whose
   output is a bare variable that no part binds (in parts it appears only
   inside a type function, or not at all). The catalog works this out when
   it loads. Every other variable in `output` must be bound by a part that
   is not optional; the catalog refuses the definition otherwise.
4. **Checking.** Each part is checked against its resolved pattern (in
   `b2c-lang`, ADR-0018): an input by `conversion` (or `assignment` with
   `assign`) and its `check` class; a `symbol_ref` field by exact type (a
   reference is not converted) and the class; a type field by the class. A
   pattern that does not resolve accepts anything its class accepts.
5. **Declarations.** A `symbol_decl` field with a pattern declares its
   symbol with the resolved type (`Error` when it does not resolve).
6. **Output.** The output is the resolved `output` pattern; `any` and
   `number` keep their M2 meaning.

**Checked when the catalog loads** (`definitions.rs`):

* each variable is in `where` once with a known class, and every variable in
  `where` is used;
* a variable that appears only inside functions and not in `output` is an
  error (nothing could bind it); rule 3's output rule;
* patterns parse, name known types with the right arity and nest at most 8
  levels;
* `type` appears only on the parts §4.1 lists; `field:NAME` names a type
  field, `symbol:NAME` a symbol field, and `param:`, `member:` and `result`
  sit where §4.2 says;
* `members` names a variable whose class is `struct` or `enum`; `assign`
  sits only on value inputs;
* an input whose pattern is not a fixed type (it holds a variable, a type
  function or a `field:`, `symbol`, `param:`, `member:` or `result` form) has
  no `default`: one fixed token cannot fit every type it may expect, and
  resolution would fill it in (`crates/b2c-catalog/src/resolve.rs:616-621`);
  the editor writes start values instead (ADR-0018);
* a predicate's output is `bool`;
* every header is in the header table (§5), and per-option header keys are
  option values of that dropdown.

The category list (`schema.rs:65-84`) grows with W2's categories.

### 4.4 The Rust contract

`b2c-catalog` builds it; `b2c-lang` and the editor's copy read it.
`b2c-ir` binds and resolves; checking a fit needs `conversion`, which stays
in `b2c-lang`, so `type_block` lives there (ADR-0018, [slots.md](slots.md)).

```rust
pub struct TypeVar(/* validated: [A-Z][0-9]? */);
pub enum TypePattern {
    Any, Number, Type(Type), Var(TypeVar), Lib(LibType, Vec<TypePattern>),
    Fn(TypeFn, Box<TypePattern>), Field(String), Symbol(Option<String>),
    Param(String), Member(String), Result,
}
impl TypePattern {
    pub fn parse(text: &str) -> Result<TypePattern, PatternError>; // at most 128 bytes
    pub fn vars(&self) -> impl Iterator<Item = &TypeVar>;
}

pub enum PartName { Field(String), Input(String) }
pub struct InputSig {
    pub check: TypeClass,
    pub pattern: Option<TypePattern>,
    /// `assign = true`: assigned, not initialised (`char` fits `std::string`); never binds.
    pub assign: bool,
}
pub struct BlockSig {
    pub vars: BTreeMap<TypeVar, TypeClass>,
    pub output: Option<TypePattern>,
    pub fields: BTreeMap<String, TypePattern>,
    pub inputs: BTreeMap<String, InputSig>, // a repeated input under its base name
    /// The parts that have a pattern, in catalog order: the order they bind in.
    pub order: Vec<PartName>,
    /// The output's variable when no part binds it: the block takes its type from its place.
    pub place_var: Option<TypeVar>,
    /// `symbol_ref` fields that list the members of the user type bound to a variable.
    pub members: BTreeMap<String, TypeVar>,
    pub headers: Vec<Header>,
    pub option_headers: BTreeMap<String, BTreeMap<String, Vec<Header>>>, // field, option
}
pub struct SigTable { /* block type ID -> BlockSig */ }
impl SigTable {
    pub fn empty() -> SigTable;
    pub fn get(&self, block_type: &str) -> Option<&BlockSig>;
}

/// What one part of a block is when the block is typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartType {
    /// Nothing chosen or connected, or a disabled block.
    Empty,
    /// `auto` in a type field.
    Auto,
    /// A value that takes its type from its place: typed after the block.
    FromPlace,
    /// A known type. `Type::Error` binds the variable to `Error` and fits everything.
    Known(Type),
    /// Not known to the caller (the editor, for something the analysis has not seen).
    Unknown,
}

/// The parts of the block being typed. A repeated input is asked part by part (`ITEM2`).
pub trait BlockParts {
    fn field(&self, name: &str) -> PartType;
    fn input(&self, part: &str) -> PartType;
    /// How many parts the repeated input `base` has now.
    fn count(&self, base: &str) -> usize;
    /// `param:F`: parameter `index` of the function chosen in field `field`.
    fn param(&self, field: &str, index: usize) -> PartType;
    /// `member:F`: field `index` of the struct chosen in field `field`.
    fn member(&self, field: &str, index: usize) -> PartType;
    /// A `members` field: the chosen member's type when it is a member of `owner`.
    fn chosen_member(&self, field: &str, owner: &Type) -> PartType;
    /// `result`: what a `return` gives back where the block is.
    fn result(&self) -> PartType;
}

/// How a binding went.
pub enum Fit { Fits, Unknown, OutsideClass, BadShape }

/// A resolved pattern.
pub enum Resolution {
    Type(Type),
    /// Something it depends on is empty, unknown or already wrong: `Error` fits.
    Missing,
    /// Nothing could give it a type (no pattern, or only `auto` and place-typed values).
    Open,
}

pub struct TypeEnv { /* classes of the variables, what is bound and by which part */ }
impl TypeEnv {
    pub fn new(sig: &BlockSig) -> TypeEnv;
    /// Unifies `pattern` with `part` (rules 1 and 2). `Unknown` when class membership is not known.
    pub fn bind(&mut self, pattern: &TypePattern, part: &PartType, by: PartName, facts: &dyn TypeFacts) -> Fit;
    /// Binds the place-typed block's variable to its place's type (rule 3).
    pub fn bind_place(&mut self, place: Option<&Type>, facts: &dyn TypeFacts) -> Fit;
    /// `index` is the part's index for `param:` and `member:`.
    pub fn resolve(&self, pattern: &TypePattern, parts: &dyn BlockParts, index: Option<usize>) -> Resolution;
    /// The binder is `None` when the place bound the variable (a place has no part name).
    pub fn get(&self, var: &TypeVar) -> Option<(&Type, Option<&PartName>)>;
}
```

* `b2c_catalog::Catalog::sigs(&self) -> &SigTable`, built when the catalog
  loads.
* `b2c_lang::analyze_with(document, &SigTable)` and
  `analyze_checked_with(document, catalog, unchecked, &SigTable)` take the
  table (`crates/b2c-lang/src/lib.rs:79`, `:104-108` today), so `b2c-lang`
  reads the catalog's contract without depending on `b2c-catalog`
  (`tools/check-layering.py`). The callers are `b2c-build`
  (`frontend.rs:173`) and `b2c-core-wasm` (`facade.rs:294`); `b2c-cli` runs
  analysis only through `b2c-build`.

### 4.5 `catalog.json`

`packages/catalog-gen/catalog.json` gains:

* a `types` section (§1.1, from `LibType` and the primitives);
* per block: `typeVars`, `output`, `order`, `placeVar`, `members`,
  `headers` and `optionHeaders`;
* per part: `type` as tagged JSON (`{"kind": "var", "name": "C"}`,
  `{"kind": "fn", "fn": "elem", "of": …}`, `{"kind": "type", "type": TypeJson}`,
  `{"kind": "param", "field": "FUNC"}`, `{"kind": "member", "field": "TYPE"}`,
  `{"kind": "result"}`, …); per input `assign`; per row `typeClass`.

`order` carries the catalog order, so both copies bind in the same order.
The generated `catalog.ts` and the block reference follow.

## 5. Headers (`crates/b2c-ir/src/headers.rs`, codegen)

### 5.1 The types

```rust
/// A standard header, made only by `Header::standard`. Orders by its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Header(&'static str);
impl Header {
    /// A lookup in the closed table of the C++ standard library's headers
    /// (C++17 to C++26, the `<cxxx>` forms only), each with the first standard that has it.
    pub fn standard(name: &str) -> Option<Header>;
    pub fn since(self) -> CppStandard;
    pub fn as_str(self) -> &'static str;
}

/// The catalog headers of an item, by where its code is printed.
pub struct ItemHeaders {
    /// Blocks printed with the declaration: the signature, default arguments, a
    /// `constexpr` constant's value, default member initialisers.
    pub decl: BTreeSet<Header>,
    /// Blocks printed only with the definition: function bodies, a variable's
    /// initialiser, a `const` constant's value (ADR-0019 prints it in the `.cpp`).
    pub body: BTreeSet<Header>,
}
```

* `Item.headers: ItemHeaders` in the SAST; `b2c-lang` adds the catalog
  headers of every block it lowers (enabled and attached), with the chosen
  options' headers, to the part of the item the block's code is printed in.
* Codegen puts `decl` headers where the declaration is printed and `body`
  headers where the definition is (ADR-0019 §4), so a body edit never
  changes a header file.
* `Usage.includes` (`crates/b2c-codegen/src/lower/mod.rs:31-37`) becomes
  `BTreeSet<Header>`.
* `CType` (`cast.rs:18-75`) gains `Lib(LibType, Vec<CType>)` and `Named`
  with the user type's qualified `Ident`s, and loses `Copy`; the printer
  composes `&'static str` names and `Ident`s and spells `std::` names by
  [06 §6.14.5](../../spec/06-compiler-pipeline.md#6145-spelling-standard-names).
  A read-only parameter is `const T&` for every type that is not a scalar
  (`lower/mod.rs:210`).

### 5.2 Forms codegen chooses, with their headers

| Form | Code | Headers |
| --- | --- | --- |
| a type it names | `std::vector<int>` | the type's (§1.1) |
| a library member, operator or range-`for` on a value that is not a literal | `c.push_back(x)` | the value's type's |
| a size | `static_cast<int>(c.size())` | — |
| an `int` index into a vector, deque or string | `c.at(static_cast<std::size_t>(i))`; a non-negative literal stays as it is | `<cstddef>` |
| *exit* outside `main` | `std::exit(n)` | `<cstdlib>` |
| the simple *ask* | as in M2 | `<limits>`, `<string>` |
| a comparable struct, C++20 | `auto operator<=>(const S&) const = default;` | `<compare>` |
| a comparable struct, C++17 | six hidden friends on `std::tie` | `<tuple>` |
| a printable struct | a hidden-friend `operator<<` | `<ostream>` |
| an enum that is printed or written | `operator<<` with each enumerator's name | `<ostream>` |
| an enum that is joined | `to_string(Color)` | `<string>` |
| a helper it calls | as in M2 | the helper's |

Hidden friends are defined in the class, so they are inline and fit in a
shared header (ADR-0019). With `int` indexes, `-Wsign-conversion` (the
*strict* level) warns on `v.at(i)`, and `-Wsign-compare` (from `-Wall`) on
`i < v.size()`; both forms above compile clean (checked with g++ 11 and
14). The generated code does not depend on the warning level.

### 5.3 The core catalog in W1

`text.literal` (`catalog/core/text.toml:11`), `program.exit`
(`program.toml:22`), `math.random_int` (`math.toml:78`) and `io.ask` lose
the headers only some of their forms need, which codegen adds (§5.2). The
catalog's headers are then a subset of what codegen emits for every core
block, so the golden files stay byte-identical.

### 5.4 `use header`

The block's header part is a field of the new kind `header`: a text value
the catalog checks against the header table (`E0607` when it is not there,
`E0602` from a newer catalog), lowered to an include of that header. A
module is not a header: `use header` names standard headers only, and
ADR-0019 gives a module its own if-time block, *use header of module*,
which stores the module ID as a plain string (no new value shape, no
format change).

## 6. Diagnostic codes

The same allocation stands in ADR-0017, ADR-0018 and here.

| Code | Meaning | From | First emitted |
| --- | --- | --- | --- |
| `B2C-E0140` | type term too deep or too wide | ADR-0017 | W1 (loader) |
| `B2C-E0213` | name reachable through `::name` (06 §6.14.7) | spec, used by ADR-0019 | W3 |
| `B2C-E0214` | a shared header part names a non-shared definition (was `E0424`) | ADR-0019 | W3 |
| `B2C-E0221` | a module name starts with `b2c` | ADR-0019 | W3 |
| `B2C-E0313` | not comparable | ADR-0017 | W1 |
| `B2C-E0314` | not printable | ADR-0017 | W1 |
| `B2C-E0315` | no type from its place | ADR-0018 | W1 |
| `B2C-E0316` | whole-number division or `mod` by a constant zero | L10 (§11.2) | W1 |
| `B2C-E0420`, `B2C-E0421` | declaration cycles (06 §6.7) | spec | W2 (structs) |
| `B2C-E0422` | modules' headers form a cycle | ADR-0019 | W3 |
| `B2C-E0423` | cross-module initialisation order | ADR-0019 | W3 |
| `B2C-W0506` | a decimal result that is infinity or NaN (division by a constant zero, a constant calculation too large) | L10 (§11.2) | W1 |

* **Reused with new text:** `E0206` (ADR-0019's new meaning), `E0207`
  (ADR-0018: a member the type does not have; ADR-0017: a reference to a
  user type of a kind the class does not hold), `E0301` (ADR-0017 and
  ADR-0018: class messages), `E0303` (L16: only text with what is not text
  or a character) and `E0517` (L10: also a constant `int` calculation or
  conversion that does not fit).
* **Next free:** `E0141`, `E0219`, `E0222`, `E0424`; `W0507` to `W0509`.
* Each code gets its reference entry in `docs/reference/diagnostics/` in the
  pull request that first defines it, which `tools/check-diagnostics-docs.py`
  and `codes.rs:183-201` enforce. So `E0313`, `E0314` and `E0315` land in W1
  with `b2c-lang`'s code that reports them, although no W1 block can reach
  them.

## 7. Versions

The same plan stands in ADR-0017, ADR-0018 and here.

| What | After M3 | Decided by |
| --- | --- | --- |
| `formatVersion` | 2 holds `options.usingNamespaceStd` and type template terms; 3 is `project.lints` (M5) | ADR-0017, partly superseding ADR-0013's numbering |
| catalog version | 1.3.0 from W1 (ADR-0017's widened type fields and keys, ADR-0018's keys; ADR-0019's `SHARED` field lands in W3); every catalog change up to the M3 release ships as 1.3.0 | ADR-0017 |
| clipboard `formatVersion` | 2 (template terms; `refs` kinds `struct`, `enum`, `field`, `enumerator`); readers accept 1 and 2 | ADR-0017 |
| source map | 1, with an optional `module` key, until M3-5 defines the compact form | ADR-0019 |
| `IPC_VERSION` | unchanged by ADR-0016 to ADR-0019; W3's run options and close-input raise it | — |
| `update()` JSON | gains `slotTypes`; the core and the editor ship in one build (ADR-0016 §8) | ADR-0018 |
| worker protocol, scope index | 1, internal | ADR-0016 |

## 8. W1 plan

### 8.1 The contracts pull request

**Rule: the contracts pull request keeps the workspace and every test
green**, so each package starts from a green tree. ADR-0018's and
ADR-0019's W1 contracts ride in the same pull request under the same rule
(ADR-0019's with their defaults: not shared, the layout derived, no
module).

* `b2c-ir` in full, except `Type`'s serialisation: `types.rs` (§1.2, with
  `sample_types()`), `sig.rs` (§4.4, with `TypeEnv` implemented),
  `headers.rs` (§5.1), the SAST and `SymbolInfo` additions (§1.2,
  `Item.headers` empty), with their unit tests. `Type` gains `Lib` and
  `Named`; `cpp_name`, `friendly_name`, `from_field`, `is_numeric` and
  `header` stay, deprecated, answering a placeholder for the new variants
  that no W1 path produces. `Type`'s serde stays as it is (§8.2,
  packages 5 and 6).
* A wildcard arm in each exhaustive match the new variants break:
  `crates/b2c-lang/src/messages.rs:44-53`, `crates/b2c-codegen/src/cast.rs:42-50`,
  `crates/b2c-ipc/src/pipeline.rs:239-250` and the test match in
  `crates/b2c-core-wasm/src/scope.rs:149-157`; and arms for the new
  `FieldValue`, `RefKind` and `SymbolInfoKind` variants wherever a match
  is exhaustive (`b2c-model`'s `save.rs`, `security.rs`, `ids.rs`;
  `b2c-catalog`'s `resolve.rs`, `upgrade.rs`; `b2c-lang`'s `access.rs`;
  `b2c-core-wasm`'s `clipboard.rs`).
* `b2c-model`, types only: `FieldValue::Type`, `FieldValue::type_term`, the
  four `RefKind`s. The decoder does not produce them yet.
* `b2c-lang`: `analyze_with` and `analyze_checked_with`; `analyze` and
  `analyze_checked` stay as wrappers that pass `SigTable::empty()`. The
  production callers (`b2c-build`'s `frontend.rs:173`, `b2c-core-wasm`'s
  `facade.rs:294`) move to the new forms with `Catalog::sigs()`.
* `b2c-catalog`: `Catalog::sigs()`, returning an empty table until
  package 2; `TypeClass` re-exported from `b2c-ir`.
* TypeScript, types only: `packages/blockly-ext/src/types/` declarations
  (§3.2), `bdm.ts`'s `FieldValue` and `TypeJson`, and the optional keys of
  `catalog.json` (§4.5).

### 8.2 Packages, in parallel worktrees

1. **`b2c-model`:** the term decoder and limits (`E0140`), rows, the
   canonical writer, `version.rs` and the loader rule (§2.3), the clipboard
   version, kinds and names (§2.4), renaming and `outside_refs`, the
   security corpus and the fuzz dictionaries. It also changes the version
   constant's users in the same pull request: `b2c-app`'s `projects.rs`,
   `b2c-core-wasm`'s `facade.rs` and `bdm.ts`.
2. **`b2c-catalog`:** the schema keys, patterns and their load checks,
   `type_class`, `assign`, `members`, `order`, `place_var`, per-option
   headers, the header table check, the `header` field kind, domain checks
   of type fields and rows, `E0602` for types, `Catalog::sigs`, the export,
   catalog 1.3.0, the aligned TOML (§5.3) and the patterns on M2's blocks
   ([slots.md](slots.md) §6).
3. **`b2c-lang`:** the type churn of §1.6 (classes, comparisons,
   arithmetic, `E0313`, `E0314`, ticks), `from_term`, references to user
   types, messages, `ItemHeaders`, ADR-0018's `slots.rs` and lowering, and
   both fixture writers (§9). A block the table does not know keeps its M2
   rule, so the package merges green whether package 2 has merged or not.
4. **`b2c-codegen`:** recursive `CType` and its printer, includes as
   `Header`s per file and per item part, `const T&` parameters, the forms
   of §5.2.
5. **`b2c-ipc` and `b2c-core-wasm`:** `TypeJson`, the conversion table over
   the samples, `SymbolInfo`, `slotTypes`; `packages/ipc-types`
   regenerated.
6. **TypeScript:** `blockly-ext/src/types/`, the checker on `StaticType`
   objects, the type field's key, `catalog-gen`, and the desktop's uses
   (`services/symbols.ts`, `toolbox/values.ts`, `landing.ts`, `reshape.ts`,
   `contents.ts`).

Packages 5 and 6 switch `Type`'s serialisation to terms in one pull
request that they own together, since the wire changes on both sides at
once; it also updates the insta snapshots, where only `std::string`'s
spelling changes.

### 8.3 The integration pull request

After every package has merged, the integrator removes the deprecated
`Type` helpers, the `analyze` wrappers and the M2 rules of `create`, `set`,
call arguments and `return` (now covered by their patterns), and moves the
benchmarks, tests and fuzz targets (`crates/b2c-core-wasm/benches/pipeline/main.rs:93`,
`tests/common/mod.rs:124`, `fuzz/fuzz_targets/frontend.rs:43`) to the real
table. Every golden file stays byte-identical and every snapshot as it was.

## 9. Tests

**Rust.**

* `b2c-ir`: terms round-trip through canonical JSON with one spelling each
  (a one-element array, an empty array and an object with two keys are
  refused); 8 levels and 32 names pass, 9 and 33 do not; `from_term` and
  `to_term` round-trip over generated types (proptest); `Type::lib`'s
  arity, value and error rules; class membership (with `Error` and unknown
  facts), type functions, labels and C++ spellings by table; `key` of a name
  is the name; `size_of::<Type>() <= 24`; patterns parse, print and refuse
  bad input; `TypeEnv` binds `T` from `std::vector<T>`, keeps the first
  binding, binds nothing through a function, reports `OutsideClass` and
  `BadShape` apart, binds `Error` for an unknown or failed part, and
  resolves with an index; the header table is sorted, unique and in `<…>`
  form.
* `b2c-lang` writes `packages/blockly-ext/src/types/fixture.json`: the
  samples, their keys, conversions, classes under a sample set of facts
  (one of which does not know a `Named` type), `check` acceptance, type
  functions, labels and pattern bindings. It lives in `b2c-lang` because
  `conversion` does, and it is committed; CI fails when it is stale, as for
  `catalog.json`.
* `b2c-model`: decoder cases for each shape and error (`E0112`, `E0113`,
  `E0127`, `E0140`), and the three object readings of §2.2 with rules tests
  in `crates/b2c-model/tests/rules.rs` (a damaged declaration keeps `E0110`
  or `E0108`); writer round-trip; `required_format_version` is 1 for every
  example, golden project and assignment that uses no template, and 2 with
  a template in a field, in a row or with *Textbook style*; a version 1
  golden project loads byte for byte, with `migratedFrom` null and no
  `NoPath`; a version 3 file is `E0108`; clipboard versions 1 and 2 and the
  four new kinds, with a field and an enumerator bound again by owner and
  name; renaming and `outside_refs` inside terms; new security-corpus files
  (deep, wide, odd keys, bidi in names); the `load` and `clipboard` fuzz
  dictionaries gain term tokens, and the decoder joins the mutation testing
  list.
* `b2c-catalog`: each load check of §4.3 (a `default` on `index(C)` is
  refused); type-field and row domains; `E0602` for an unknown type under a
  newer catalog; `Catalog::sigs` covers every block, with `order` the
  catalog order; the export test regenerates `catalog.json`.
* `b2c-lang`: conversions of library and user types; M2's comparison cases
  give the same diagnostics as before (`crates/b2c-lang/tests/types.rs:103`),
  and two library or user operands follow rule 4 (`E0302`); arithmetic on a
  container is `E0302`; `E0313` for a set element, a map key and a tick
  with a bad field; `E0314` for *print* and for a tick; references to user
  types (`E0201`, `E0207`); a `double` in an `integer` input is `W0518`, not
  an error; `ItemHeaders` comes only from lowered blocks, includes options'
  headers, and a body-only header never reaches `decl`; the SAST snapshots
  change only in `std::string`'s spelling (in the serialisation pull
  request).
* `b2c-codegen`: nested types and user names print right, also under
  *Textbook style*; type headers land in the file that prints the type;
  read-only parameters of library and user types are `const T&`. Every
  golden file stays byte-identical.
* `b2c-ipc`, `b2c-core-wasm`: the mirror test for `TypeJson`;
  `conversion_table` over the samples; `SymbolInfo` with `std::string`.
* A program compiles with `-fsyntax-only` under C++17 and C++20 with the
  *strict* warnings, on GCC 11 and GCC 15 in CI (with W1's catalog snapshot
  test): a variable of every sample value type; a comparable struct used
  with all six comparison operators (which needs `<compare>` in C++20 and
  the hidden friends in C++17); a `std::set` and a `std::map` keyed by it;
  a printable struct and an enum printed and joined; an indexed access and
  a size compared with an `int`.

**TypeScript (Vitest).**

* `blockly-ext/src/types/`: every case of the fixture agrees; `parseType`
  refuses prototype keys, over-deep terms and unknown names.
* `apps/desktop/src/editor/sync/integration.test.ts`: `staticConversion`
  equals the core's table on every row.
* The checker: unknown types and `error` connect; a `std::vector<int>` value
  is refused where a whole number is required; M2's outputs (`symbol`,
  `field:NAME`, `number`) behave as before.
* Type field: an M2 value keeps its string; a template term round-trips
  through the field's key; a value the field does not know is kept.
* `catalog-gen`: patterns, classes, `order`, the `types` section and headers
  are generated.

**End to end and benchmarks.** W1 changes no visible behaviour beyond the
checker (ADR-0018), so the existing flows run unchanged; W2's flows cover
the new blocks. The A/B job on each W1 pull request shows no gated
regression in `native.pipeline.preview`, `webview.preview-1000.p95`,
`webview.edit-1000.p95-all` and `webview.drag-5000.frame-p95-all`. W1 gives
M2's `create`, `set`, call and `return` patterns, so the native and preview
benchmarks already bind patterns; the per-candidate cost in the editor is
measured by ADR-0018's `webview.drag-typed-1000.frame-p95-all`. The first W2
pull request that brings container blocks adds them to the benchmark
workspaces and runs the A/B job.

## 10. Spec sections to change

The wave that changes each section. Sections that ADR-0018 also changes are
marked *with 0018*: W1 edits them once, for both.

| Section | Change | Wave |
| --- | --- | --- |
| [02 §2.5.1](../../spec/02-architecture.md#251-requests-and-responses) | `StaticType` becomes `TypeJson`; the new `SymbolInfoKind` kinds | W1 |
| [03 §3.5.1](../../spec/03-block-language.md#351-the-type-picker) | what M3's picker offers (§1.1) | W4 |
| [03 §3.5.2](../../spec/03-block-language.md#352-friendly-and-c-display-modes) | `map from string to number` becomes `map from string to double`; constructed types are stored as terms | W1 |
| [03 §3.5.3](../../spec/03-block-language.md#353-type-checking-model-gradual) | library and user types convert only to themselves; the classes; *with 0018* (the one-pass rule) | W1 |
| [03 §3.7.5](../../spec/03-block-language.md#375-control-and-loops) | `switch` on `char`, `int` and enums | W2 |
| [03 §3.7.8](../../spec/03-block-language.md#378-collections) | *length of* gives an `int`; the rows of §1.5 marked *changed* (*next item of*, *remove item at*, *remove (x)*, *remove next*, *list of* without a map form, *for each key, value*); *with 0018* (`list of` from its place) | W1 (text), W2 (blocks) |
| [03 §3.7.9](../../spec/03-block-language.md#379-types-structs-classes-enums) | a ticked struct's fields must be comparable or printable; the generated operators of §1.6; *with 0018* (`new … with`) | W2 |
| [03 §3.7.13](../../spec/03-block-language.md#3713-organisation) | `use header`'s header field (§5.4) | W2 |
| [03 §3.11.1](../../spec/03-block-language.md#3111-catalog-format) | `where`, `type`, output patterns, the forms and keys of §4.1-§4.2, `type_class`, per-option `headers`, what `headers` mean, the classes of `check` and `where`, `std::string` instead of `string` in `output`; *with 0018* | W1 |
| [03 §3.11.2](../../spec/03-block-language.md#3112-template-lowering-library-blocks) | `type = "range<T>"` becomes `type = "R"` with `where = { R = "iterable" }` | W1 |
| [03 §3.11.3](../../spec/03-block-language.md#3113-block-versioning) | a new type or a widened type field is a compatible change; an unknown type from a newer catalog is `E0602` | W1 |
| [05 §5.3](../../spec/05-project-format.md#53-top-level-structure), [§5.7](../../spec/05-project-format.md#57-versioning-and-migration) | version 2 holds *Textbook style* and template terms; versions without a change of meaning load without a migration step, and `migratedFrom` compares with the oldest such version | W1 |
| [05 §5.4](../../spec/05-project-format.md#54-block-nodes) | type terms in fields and rows; how an object is read (§2.2) | W1 |
| [05 §5.6](../../spec/05-project-format.md#56-validation-limits) | a row for type terms (8 levels, 32 names, `E0140`) | W1 |
| [05 §5.12](../../spec/05-project-format.md#512-clipboard-format) | the version rule; `refs` kinds `struct`, `enum`, `field`, `enumerator`; qualified member names | W1 |
| [06 §6.5](../../spec/06-compiler-pipeline.md#65-stage--names-and-scopes) | `SymbolInfo` types are `TypeJson`; the new kinds; what `conversion` does with library and user types; *with 0018* | W1 |
| [06 §6.6](../../spec/06-compiler-pipeline.md#66-stage--types-flow-checks-and-lints) | the `Type` sketch becomes §1.2's (`Param`, `Pointer`, `Ref`, `Function`, `Opaque` later); comparisons and arithmetic (§1.6); *with 0018* | W1 |
| [06 §6.8.2](../../spec/06-compiler-pipeline.md#682-typed-text-parse-dont-validate) | types and headers are printed from tables and validated names | W1 |
| [06 §6.10](../../spec/06-compiler-pipeline.md#610-includes-and-support-helpers) | where includes come from (§5), the header table, `ItemHeaders`; ADR-0019 adds the per-file placement in W3 | W1 |
| [08 §8.4.5](../../spec/08-security.md#845-expression-slots-and-templates) | type terms and headers | W1 |
| [09 §9.2](../../spec/09-quality-and-delivery.md#92-testing-strategy) | the shared type fixture, kept fresh like `catalog.json`; *with 0018* | W1 |
| [03 §3.4](../../spec/03-block-language.md#34-expression-slots), [§3.7.3](../../spec/03-block-language.md#373-math-and-logic), [§3.7.4](../../spec/03-block-language.md#374-text) | `+` joins text with text or a character (§11.1); the analyser lints of §3.7.3 gain division by a constant zero, constant overflow and infinite decimal results (§11.2) | W1 |
| [06 §6.6](../../spec/06-compiler-pipeline.md#66-stage--types-flow-checks-and-lints) | arithmetic on text (§11.1); constant folding after the flow checks, `E0316`, `W0506`, `E0517` for constant calculations (§11.2) | W1 |
| [08 §8.4.4](../../spec/08-security.md#844-numbers) | `E0517` also covers a constant calculation that does not fit (§11.2) | W1 |

## 11. Text with `+` and constant folding (L16, L10)

### 11.1 L16: `text + "x"` where C++ allows it

**The rule** (`typing.rs` `joins_text`, `lower/expr.rs` `binary_type`). `a +
b` where one side is `std::string` and the other `std::string` or `char`
(the class `text`, §1.3), in either order, joins text and has the type
`std::string`. That is exactly what `std::string`'s `operator+` takes.
Everything else keeps its M2 rule:

* text with a number, a `bool`, `void`, a library or a user type is
  `E0303` (new text below): `name + 1` does not compile, and `"x" + 1`
  compiles as pointer arithmetic;
* `-`, `*`, `/` and `%` on text stay `E0302`;
* two `char`s add as `int`s, as in C++ (`'a' + 'b'` is 195);
* comparisons do not change (§1.6).

The rule reads types only, as every other rule and the editor's checker
do. Quoted text has the type `std::string`, as it has for comparisons, where
M2 already accepts `"abc" < "abd"` and prints `std::string("abc") < "abd"`
(`crates/b2c-lang/tests/compile.rs`). The same printing makes `"a" + "b"`
mean what the expression says, and the code panel shows the student the C++
it needs. Refusing two quoted texts would make the rule depend on how a
value is written, which the editor cannot see in an operand's type.

**The C++ forms** (codegen `lower/expr.rs` `binary`, the rule M2 used for
comparisons):

| Operands | C++ |
| --- | --- |
| a `std::string` object on either side: a variable, a call, a *join*, a `+` of text, a `?:` with one such value | as written: `name + "!"`, `"Hi " + name`, `name + mark`, `'(' + name`, `name + (flag ? 'y' : 'n')` |
| no object: two quoted texts, quoted text and a character, a `?:` of quoted texts | the left side made one: `std::string("a") + "b"`, `std::string("a") + 'b'`, `std::string(1, 'b') + "a"`, `std::string(flag ? "x" : "yy") + "z"` |

A chain needs it once: `std::string("a") + "b" + "c"`. The joining `+`
adds `<string>` (§5.2, an operator on a value). Every form compiles with g++
11 and 14, C++17 and C++20, `-Wall -Wextra -Wpedantic -Wshadow -Wconversion
-Werror`, and prints the joined text. Without the conversion, `"a" + "b"`
does not compile, and `"a" + 'b'`, `'b' + "a"` and `"a" + 1` compile
without a warning as pointer arithmetic.

**The editor.** `math.arithmetic` keeps `check = "number"` on `A` and `B`,
so the catalog does not change: in blocks, text is joined with *join*, whose
C++ label already shows `+`, and a text block offered to an operator block
whose menu is mostly `−`, `×`, `÷` and *mod* would mostly lead to errors. `+`
on text comes from typed expressions (slots, M3's slot editor and Quick
Insert) and from files, and there the analyser and the editor agree: a
slot's type comes from `slotTypes`, and `staticOutputType` gives a
`math.arithmetic` with `OP = add` and text operands `std::string`
(`checker/output-type.ts` `arithmeticType`); an operand it does not know
yet gives "not known". The slot fixture's `outputs` (§9) hold
`math.arithmetic` over every pair of the six operand types, so the three
joining pairs are checked against the analyser.

**Codes and messages.** `E0303` keeps its code with new text, naming the
other side: *"Text can't be added to a whole number with +: + joins text
only with text or a character. Use the 'join' block to put text and other
values together."*

**Tests.** `b2c-lang`: `joins_text` by table; every form above and
`"a" + "b" + "c"` types as `std::string` with no diagnostic, in slots and in
blocks; `'a' + 'b'` is `int`; text with a number, a `bool` or a decimal is
`E0303` with the text above; `s - "a"` stays `E0302`; the diagnostics
tour's message; a program with every form compiles (`tests/compile.rs`).
`b2c-codegen` (`tests/it/text_plus.rs`): the printed form of each, `<string>`,
`-fsyntax-only` under C++17 and C++20 with the strict warnings, and the
built program's output. TypeScript: `arithmeticType` for each operand pair,
`OP`, quoted text and an unknown operand; the regenerated slot fixture.

### 11.2 L10: constant folding

**What folds** (`crates/b2c-lang/src/consts.rs`). Every expression of type
`bool`, `char`, `int` or `double` built from number, `true`/`false` and
character literals; `const` variables whose starting value folds (the
*create … constant* block, and in W2 `const` and `constexpr` globals through
the same table); the signs `-` and `+` and `not`; `+ − × ÷ mod`; the six
comparisons; `and` and `or`; the conditional value; and *convert* (`as
int`, `as double`). Blocks and slots fold alike. Calls, *random*, text,
*join*, parameters, loop counters and variables that are not `const` do not
fold.

**Semantics**: C++'s, the same on every target Blocks2Cpp supports. `int` is
32-bit two's complement (LP64 on Linux and macOS, LLP64 on Windows;
`static_assert(sizeof(int) == 4)` holds with g++ 11 and 14), `double` is
IEEE 754 binary64.

* The usual arithmetic conversions: `bool` and `char` become `int`; with a
  `double` on one side, both are `double`.
* `int` arithmetic is exact, and a result outside `int` is reported, not
  wrapped: C++ leaves it undefined (g++ warns `-Woverflow` and wraps in
  practice). `/` and `%` truncate towards zero. The smallest `int` divided
  by -1, its `mod` -1 and its negation overflow.
* `double` arithmetic is IEEE 754. A result that is infinity or NaN is
  reported and does not fold further.
* Conversions: `double` to `int` truncates, and a value outside `int` is
  reported for *convert* (C++ leaves it undefined; g++ says nothing). An
  `int` becomes a `char` only from 0 to 127; anything else is no constant
  and is not reported, since `B2C-W0518` already warns and the value depends
  on whether the target's `char` is signed. The implicit conversion to a
  `const` variable's type follows the same rules and is never reported.
* `&&`, `||` and `?:` follow C++'s order of evaluation, and an operand C++
  does not evaluate is not checked: the right side of `&&` and `||` when the
  left side is a constant that decides, and the value of `?:` that a
  constant condition does not choose (g++ does not warn there either).
  `false && x` folds to `false`; `x > 0 || true` does not fold.
* Statements are all checked, also under `if <false>` (`W0502` is the lint
  for code that cannot run).
* One mistake, one report: a reported part is no constant further up, and
  an expression of type `Error` never folds.
* It runs on each item after the flow checks (`lower/mod.rs`), once its
  nesting is known to be within the limits, which bounds the recursion.

**Codes and messages.**

| Code | Severity | When | Why |
| --- | --- | --- | --- |
| `B2C-E0316` | error | a whole-number `/` or `mod`, also *change … divided by* and *mod*, whose divisor is a constant 0, also when the other side is not constant | the program crashes there (`SIGFPE`; C++ leaves it undefined, g++ only warns `-Wdiv-by-zero`); the free code of the types group |
| `B2C-E0517` (reused) | error | a constant `int` calculation whose result does not fit; *convert* of a constant `double` that does not fit an `int` | the fault of a literal that does not fit (03 §3.7.3: "possible overflow of a literal into its target type (which is an error)"); one code for one idea |
| `B2C-W0506` | warning | a `double` division (also *change … divided by*, and an `int` divided by `0.0`) whose divisor is a constant 0; a constant `double` calculation too large | IEEE 754 defines the result (infinity or NaN), so it is not an `E` code (06 §6.6), but it is hardly ever meant; a lint level can raise it; the first free code of `W05xx` |

* `E0316`: *"This divides a whole number by zero: the value it divides by
  is always 0. That stops the program with a crash. Check the value you
  divide by."*; for `mod`: *"This works out the remainder ('mod') of a
  division by zero: …"*
* `E0517`: *"This calculation gives 2147483648, which is too large for a
  whole number (the largest is 2147483647), so C++ gives an unpredictable
  result. Use smaller values, or decimal numbers."* (*too small … the
  smallest is -2147483648* below); the smallest `int`'s `mod` -1 says why;
  *convert*: *"This turns 10000000000 into a whole number, but it does not
  fit in one (whole numbers go from -2147483648 to 2147483647), so C++ gives
  an unpredictable result. Use smaller values, or keep it a decimal
  number."*
* `W0506`: *"This divides by zero: the value it divides by is always 0, so
  the result is infinity or 'nan' (not a number) instead of a number you can
  use. Check the value you divide by."* / *"This calculation gives a result
  too large for a decimal number (the largest is about 1.8e308), so it
  becomes infinity. Check the values."*

**What it exposes** (for ADR-0019's rule, modules.md §4.4, and for W2):

```rust
// b2c_lang
pub enum ConstValue { Bool(bool), Char(u8), Int(i32), Double(f64) } // a char is ASCII, a double finite
impl ConstValue { pub fn ty(self) -> Type; }
pub struct Constant {
    pub value: ConstValue,
    /// An expression: whether C++ counts it as a constant expression. A `const`
    /// variable: whether its starting value is one (constant initialisation).
    pub constant_expression: bool,
}
/// "This expression is a constant of type `value.ty()` with value `value`."
pub fn constant_value(expr: &Expr, constants: &BTreeMap<SymbolId, Constant>) -> Option<Constant>;
impl Analysis { pub fn constants(&self) -> &BTreeMap<SymbolId, Constant>; } // every const variable that folds
```

`constant_expression` differs from folding only where C++ does: reading a
`const` variable is a constant expression only for `bool`, `char` and `int`
(\[expr.const\]; for a `const double`, g++ says *"the value of 'half' is not
usable in a constant expression"*), so `const double b = half * 2` folds to
1.0 with `constant_expression` false. W3's `E0423` exempts a `const`
constant whose `constant_expression` is true: C++ sets it before any code
runs. W2's `constexpr` constants and `switch` case labels (§1.5) need
`constant_expression`; their codes are W2's.

**The editor** changes nothing: the reports come with the analysis.

**Tests** (`crates/b2c-lang/tests/consts.rs`, unit tests in `consts.rs`):
each report in slots, blocks and *change* blocks, with its text and
severity; the cases that are not reported (not constant, not zero, in
range, the smallest `int`, operands C++ does not evaluate); values by C++
rules (truncation, signs of `%`, conversion to the variable's type, `char`
only in ASCII); `constant_expression` for an `int`, a `const double` read
and a literal `double`; `constant_value` agrees with the table; no M2
example reports anything new (the golden C++ is unchanged). A property test
compares the analyser with a reference evaluator in exact `i128` and `f64`
arithmetic on random expressions over literals, a constant, a `const
double` and a variable that is not constant: the same reports (none when
nothing constant is at fault) and the same value and `constant_expression`.
400 random constant expressions are compiled with g++ as `constexpr`
values with a `static_assert` of the folded value, under C++17 and C++20.
