# M3 contracts: multi-module projects and headers

The exact shapes behind [ADR-0019](../../adr/0019-multi-module-header-split.md)
(L18). The ADR holds the decisions and their reasons; this file holds what
W1 to W4 build against: the Rust contract, the file layouts, the include
rule, the declaration graph, the build cache, the export manifest, the
codes, the versions, the work plan, the tests and the spec changes. It
builds on [types.md](types.md): `ItemHeaders` (§5.1), the code table (§6),
the version plan (§7) and the rule for the W1 contracts pull request
(§8.1).

A rule stated here is as binding as one in the ADR. If the two disagree,
the ADR wins and this file is corrected.

## 1. The W1 contract

These types land in the W1 contracts pull request and keep the workspace
and every test green (types.md §8.1). Every new field has a default that
changes no output, and codegen ignores `Program.layout` in W1. W2 fills in
the declaration order (§4), which for every M2 program is today's order;
nothing sets `shared`, a header, an include or a `module` key before W3
(§1.3). The `SHARED` catalog field is not part of W1 (§7.1).

### 1.1 `b2c-ir`

```rust
// text.rs: the only way a module name becomes part of a path.
/// `[a-z][a-z0-9_-]{0,63}`, not a Windows device name, not starting with
/// `b2c` (in any case).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct FileStem(Box<str>);
impl FileStem {
    pub fn new(name: &str) -> Result<FileStem, StemError>;
    pub fn as_str(&self) -> &str;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StemError { Pattern, DeviceName, Reserved }

/// The one Windows device-name check: `con`, `prn`, `aux`, `nul`,
/// `com0`–`com9`, `lpt0`–`lpt9` and the superscript-digit forms, in any
/// case, ignoring an extension and trailing spaces or dots. It is the
/// loader's rule (`crates/b2c-model/src/decode/workspace.rs:105-111`);
/// `b2c-model` and `b2c-build` (`build_dir.rs:398-405`) call this one.
pub fn is_device_name(name: &str) -> bool;

// sast.rs
pub struct Item {
    /* kind, origin, comment, headers (types.md §5.1), */
    /// Whether the definition goes into its module's header. Always `false`
    /// in a single-module project. Serialised only when `true`.
    pub shared: bool,
}
pub struct Program {
    /* standard, modules, symbols, */
    /// Which files each module gets, what they include and in which order
    /// their declarations are printed. Not serialised in the SAST JSON
    /// (`#[serde(skip)]`): the layout has its own snapshots.
    pub layout: Layout,
}

// layout.rs (new)
pub struct Layout {
    /// One per module, in project order.
    pub modules: Vec<ModuleFiles>,
}
impl Layout {
    /// One `.cpp` per module, no header, no includes; `source_order` lists
    /// the function items as codegen sorts them today (by name, then
    /// signature, then block ID). What W1 fills in.
    /// Takes the symbol table for the names and parameter types codegen's sort order needs.
    pub fn flat(modules: &[Module], symbols: &SymbolTable) -> Layout;
    /// More than one module.
    pub fn multi_module(&self) -> bool;
}
pub struct ModuleFiles {
    pub module: ModuleId,
    /// `FileStem::new(name)`. For a name it refuses (only the `b2c` prefix
    /// gets past the loader), `module<N>` with N the module's place in the
    /// project from 1, plus `_2`, `_3`, … while that clashes. That name is
    /// only ever shown next to `E0221`, which blocks Build, Run and Export.
    pub stem: FileStem,
    /// Whether `<stem>.hpp` exists.
    pub header: bool,
    /// Modules whose headers `<stem>.hpp` includes, sorted by stem.
    pub header_includes: Vec<ModuleId>,
    /// Other modules' types that `<stem>.hpp` forward-declares, sorted by
    /// namespace, then name.
    pub forward: Vec<SymbolId>,
    /// Modules whose headers `<stem>.cpp` includes, its own among them,
    /// sorted by stem.
    pub source_includes: Vec<ModuleId>,
    /// Step 3 of 06 §6.7 for `<stem>.cpp`: indices into `Module.items`, in
    /// the order their declarations are printed (§4.2). `main` is not listed.
    pub source_order: Vec<usize>,
    /// The same for `<stem>.hpp`: the shared items.
    pub header_order: Vec<usize>,
}

// source_map.rs: `None` for `b2c_support.hpp`. Serialised only when `Some`.
pub struct GeneratedFile { /* path, kind, */ pub module: Option<ModuleId>, /* contents */ }
pub struct FileMap { /* path, */ pub module: Option<ModuleId>, /* ranges */ }

// scope_info.rs: serialised only when `true`.
pub struct SymbolInfo { /* id, name, kind, ty, module, decl_block, */ pub shared: bool }
```

* `b2c-lang` fills `layout: Layout::flat(&modules, &symbols)` in W1. The struct
  literals that build a `Program`, `Item`, `GeneratedFile`, `FileMap` or
  `SymbolInfo` by hand (codegen's tests in `crates/b2c-codegen/tests/it/`,
  `crates/b2c-lang/src/lower/mod.rs`, `src/query.rs`, `tests/scope.rs`,
  `crates/b2c-codegen/src/lib.rs`) get the defaults in the same pull
  request.
* No field is serialised while it holds its default, so every SAST
  snapshot, the WASM JSON and the source map stay byte-identical.

### 1.2 `b2c-ipc` and `@blocks2cpp/ipc-types`

* The mirrors `GeneratedFile` (`crates/b2c-ipc/src/pipeline.rs:36`),
  `FileMap` (`:61`) and `SymbolInfo` (`:136`) gain the optional `module`
  and `shared` keys with the same serialisation rules, and
  `packages/ipc-types` is regenerated. The byte-identity tests of
  [02 §2.5.1](../../spec/02-architecture.md#251-requests-and-responses)
  keep passing.
* No command DTO carries these types, so `IPC_VERSION` does not change.
  Readers ignore unknown keys, so the source map stays version 1.

### 1.3 Who sets what, and when

| Field | W1 | Set by |
| --- | --- | --- |
| `Item::shared`, `SymbolInfo::shared` | `false` | `b2c-lang`, W3 (from the `SHARED` field in a multi-module project) |
| `ModuleFiles::source_order`, `header_order` | `Layout::flat` | `b2c-lang`'s declaration graph, W2 (§4) |
| `stem`, `header`, `header_includes`, `forward`, `source_includes` | `Layout::flat` | `b2c-lang`, W3 |
| `GeneratedFile::module`, `FileMap::module` | `None` | `b2c-codegen`, W3 |

`b2c-lang` reports every code of §9 and computes the layout; codegen only
prints it and never reports a user error.

## 2. Names and visibility

In a multi-module project (a single-module project behaves as today):

* **Visible.** A module sees its own definitions, shared or not, and every
  other module's shared definitions, in their namespaces, in any order. The
  scope query lists them, also in the parked scope
  ([06 §6.5](../../spec/06-compiler-pipeline.md#65-stage--names-and-scopes),
  ADR-0012).
* **Precedence.** A qualified name (`::area`, `geo::area`) resolves first to
  the module's own definition with that name. Another module's shared
  definition with it is visible only when the module has none. Since
  shared names are unique in the project (`E0210`, `E0211`) and a module's
  own shared and non-shared names never share a name (`E0213`), at most one
  candidate remains. The scope query, typed names
  ([06 §6.14.11](../../spec/06-compiler-pipeline.md#61411-names-typed-in-slots))
  and paste binding ([05 §5.12](../../spec/05-project-format.md#512-clipboard-format))
  all use this rule, so a pasted `::area` binds to the target module's own
  `area` when it has one and otherwise to the one shared `area`.
* **`E0206`.** A reference to another module's non-shared definition (by
  symbol ID, for example made before the definition was unshared). Text:
  *"`helper` belongs to module `utils`, which does not share it."* Quick
  fix *Share `helper`* (W4). The M2 meaning (no use across modules at all)
  goes.
* **Duplicates.** Two shared definitions with one name in one namespace
  are `E0210`, or `E0211` for functions until W2's overloads allow
  different parameters. Non-shared definitions of different modules may
  share a name.
* **`E0213`** ([06 §6.14.7](../../spec/06-compiler-pipeline.md#6147-names-that-must-change)):
  a non-shared name of a module may not be a shared name of its own module
  or of a module whose header its `.cpp` includes, directly or through
  other headers. It replaces the cross-module part of `E0211`. With the
  precedence rule, the only way to reach it is a reference made before the
  module defined its own name; the scope query never offers the hidden
  definition.
* **`E0214`.** A shared definition's header part (§3.3) names a non-shared
  definition of its own module. Text: *"`area` is shared, but its input
  `box` has the type `Box`, which is not. Share `Box` too."* Quick fix
  *Share `Box`* (W4). A function body, a variable's initialiser and a
  `const` constant's value are printed in the `.cpp` and may use anything
  of their module.
* **`E0221`.** A module whose name starts with `b2c` (in any case), in
  every project, single-module ones too. Text: *"Module names that start
  with `b2c` are kept for files that Blocks2Cpp writes. Rename the
  module."* The loader's rule does not change, so the file opens and can be
  fixed. The preview shows the file as `module<N>.cpp` beside the error.
* **Constant expressions.** In its own module, a `const` constant whose
  value is a constant expression is one after its definition, which the
  declaration graph prints first (§4.1). Another module sees only its
  `extern` declaration, so there only a `constexpr` constant is a constant
  expression (an array size, a `case` label). L10's constant evaluation
  (W1, [types.md §11.2](types.md#112-l10-constant-folding)) gives the
  values and `constant_expression`; W2 and W3 apply this rule with it.
* **Toolbox.** *Your functions*
  ([04 §4.2](../../spec/04-user-interface.md#42-toolbox)) shows the module's
  own functions, then each other module's shared functions that are not
  hidden by an own name, under that module's heading.

## 3. What each file contains

### 3.1 The file list

`GeneratedProject::files` and the source map list, for each module in
module order, its `.cpp` and then its `.hpp`, then `b2c_support.hpp`. A
single-module project lists one `.cpp`, as today. Every module gets its
`.cpp`, even one with nothing to generate (it holds the header comment;
an empty translation unit compiles clean with `-Wpedantic`).

### 3.2 The table

| Shared definition | `<stem>.hpp` | `<stem>.cpp` |
| --- | --- | --- |
| function | declaration (with default arguments) | definition |
| struct, enum | full definition | nothing |
| generated operators of a comparable or printable struct | hidden friends in the struct (types.md §5.2) | nothing |
| print support of a shared enum (always generated) | `operator<<` and `to_string` declared | defined |
| variable | `extern T name;` | `T name = …;` |
| `constexpr` constant | `inline constexpr T name = …;` | nothing |
| `const` constant | `extern const T name;` | `const T name = …;` |
| non-shared anything | nothing | in an anonymous namespace (`namespace geo { namespace { … } }` in a user namespace) |

Hidden friends are inline, so they may be defined in a header that several
translation units include; a plain free `operator==` there fails the link
with *multiple definition*. M4 adds classes (definition in the header,
template members in full, out-of-line non-template member bodies in the
`.cpp`), Raw C++ declarations (the text in the header), type aliases,
`std::array` and `std::list`.

### 3.3 Header parts, and include or forward-declare

The **header part** of a shared definition is what the table puts in the
header: a function's parameter and return types and default arguments, a
struct's field types, default values and ticks, a constant's type and (for
`constexpr`) its value, a variable's type.

`<stem>.hpp` includes the header of module `n` when a header part needs a
complete type or a definition from `n`:

* a default argument, or a `constexpr` constant's type or value, that names
  or builds anything of `n`;
* a struct field whose type names a type of `n`, unless the field gets a
  forward declaration under the next list;
* a field of a comparable or printable struct: its generated operators
  compare or print the field, which needs the complete type (checked: the
  C++17 hidden friends fail on `std::vector<Point>` with `Point` only
  declared);
* a field with a default value that names or builds a type or enumerator
  of `n`;
* (M4) a base class, a `std::array` element, an alias, an inline or
  template body.

A type of `n` gets a forward declaration instead (`struct Point;`,
`enum class Color;`, in its namespace) when it is named only in:

* a parameter or return type of a function declaration;
* the type of an `extern` declaration;
* a field whose type is `std::vector<T>`, at any depth of `std::vector`
  only, with `T` that type, when the field has no default value and the
  struct is neither comparable nor printable.

The standard allows an incomplete element type only in `std::vector`,
`std::list` and `std::forward_list` (C++17 `[vector.overview]`;
`[res.on.functions]` makes any other incomplete argument undefined);
libstdc++ also accepts `std::deque`, `std::set`, `std::map`, `std::queue`,
`std::stack` and `std::priority_queue`, but an exported project may meet
another standard library, so those fields include.

### 3.4 `<stem>.hpp`, in order

1. the header comment (project and module name, as for the `.cpp`);
2. `#pragma once`;
3. the standard headers the header parts need: the `decl` part of each
   shared item's `ItemHeaders` (types.md §5.1) and the types it names,
   sorted;
4. the project headers of `header_includes`, sorted by file name;
5. `#include "b2c_support.hpp"`, only when a header part calls a helper
   (a default argument, for example);
6. the forward declarations of `forward`, in their namespaces, sorted by
   namespace, then name;
7. the shared declarations in `header_order`, inside their namespace
   blocks. Never a directive
   ([06 §6.14.3](../../spec/06-compiler-pipeline.md#6143-where-directives-go)
   rule 1).

`types.md` §5.1 lists "a constant's value" under `decl`. For a `const`
constant the value is printed in the `.cpp`, so its headers are `body`
headers; only a `constexpr` constant's value is `decl`. Types.md is to say
so (§13 of this file).

### 3.5 `<stem>.cpp`, in order

1. the header comment;
2. the standard headers the `.cpp`'s own code needs (its items' `body`
   headers and the `decl` headers of its non-shared items), also when an
   included project header already includes them
   ([06 §6.10](../../spec/06-compiler-pipeline.md#610-includes-and-support-helpers));
3. the project headers of `source_includes`: its own (if it has one), the
   header of every module whose shared definitions its code refers to (in
   blocks, typed slots and, from M4, Raw C++ identifier tokens), and the
   headers from which those headers forward-declare, repeated until nothing
   is added, so that every type named in an included declaration is
   complete; sorted by file name;
4. `#include "b2c_support.hpp"` when the module uses a helper;
5. step 3 of 06 §6.7: the declarations of `source_order` (§4.2). The
   anonymous namespace is closed and reopened wherever the next
   declaration's visibility differs from the last one's, as 06 §6.7 already
   does around raw blocks. When the file has directives, the code opens
   with the anonymous-namespace block that holds them (06 §6.14.3), even if
   it then closes at once;
6. step 4: the non-shared function definitions in one anonymous-namespace
   block, then the shared ones, each sorted by name;
7. `int main()` last, at global scope, in the module that has the enabled
   *when program starts*.

A `.cpp` is never included, so its includes cannot form a cycle.

### 3.6 Invariant

A header depends only on its own module's shared definitions and on the
names, kinds and modules of the other modules' types and constants they
use. Editing a function body, a variable's initialiser, a `const`
constant's value or anything that is not shared never changes a header.

### 3.7 Helpers

`b2c_support.hpp` is written when the project has more than one module and
some file uses a helper; otherwise helpers stay inline (03 §3.9). Codegen
decides it from `program.modules.len()` and its own usage, which it tracks
per output file (`Usage`, `crates/b2c-codegen/src/lower/mod.rs:31-37`, per
`.cpp` and per `.hpp`). `CodegenOptions::helper_placement` and
`FrontendOptions::helper_placement` (`crates/b2c-build/src/frontend.rs:53-54`)
go, so the build and the preview cannot differ.

## 4. The declaration graph

Built by `b2c-lang` (`crates/b2c-lang/src/order.rs`, new) in W2, by the
package that brings structs and globals, with `E0420` and `E0421`. W3
extends it across modules (§4.3, §4.4) and derives the rest of the layout
from it. The graph is internal to `b2c-lang`; its results are
`ModuleFiles::source_order`, `header_order` and the include lists.

### 4.1 Nodes and edges

Nodes: a type definition (struct, enum; M4 class, alias, raw block), a
function declaration and a global variable or constant definition. Each
has its item, module, namespace and visibility. `main` is not a node.

| Edge from `a` to `b` | When | Printed |
| --- | --- | --- |
| complete | `a` holds `b` by value: a field (other than the forward-declared `std::vector` case of §3.3), a field of a comparable or printable struct, the type of a global; `a` is a constant or a variable of type `b` | `b`'s definition before `a` |
| declared | `a`'s default argument, default member initialiser or initialiser calls or names `b` | `b`'s declaration before `a` |
| forward | `a` names type `b` only in a parameter or return type, an `extern` type or the `std::vector` field case | a forward declaration of `b`, or its definition, before `a`; the forward declaration is printed only when the definition is not already before `a` |
| initialised | a global `a`'s initialiser uses global `b`: directly, in the body of a function it calls (followed through every call, transitively), in a default argument of a call, or in a default member initialiser of a type it builds (directly or as a field); or `a` uses `b`, a `const` constant, as a constant expression | `b`'s definition before `a` |

06 §6.7 step 3 today gives a global an edge only to the declaration of a
function it calls. The `initialised` edge closes that gap: with only the
declaration edge, `int a = f();` may be printed before the `int b = …;`
that `f` reads, and `a` is then set from a `b` not yet set (checked: it
prints `a=0`). The walk is conservative: a function that uses a global on
any path counts. A per-function summary of the globals it uses,
transitively, is computed once over the call graph's strongly connected
components, so the walk stays linear in calls and uses.

### 4.2 Order

* Step 3 of each file follows one topological order of the graph,
  restricted to that file's declarations (`source_order` for the `.cpp`:
  the non-shared declarations and the shared variables' and `const`
  constants' definitions; `header_order` for the `.hpp`: the shared
  declarations).
* When several declarations are ready: first those with the same
  visibility as the last one printed (so that non-shared runs stay in one
  anonymous-namespace block), starting with non-shared; then 06 §6.7's
  rule (raw blocks by block ID, then forward declarations, constants,
  types, functions and variables, each sorted by name).
* In a single-module project every declaration has one visibility, so the
  order is 06 §6.7's, and a program with only functions (every M2 program)
  prints exactly as today.
* Ordering never depends on canvas position or module order.

### 4.3 Cycles

* A cycle of `complete` edges through by-value fields is `E0420`; any
  other cycle in the graph, including one of `initialised` edges, is
  `E0421`, as 06 §6.7 specifies (first implemented with W2's structs). A
  `forward` edge is met by a forward declaration, so it never closes a
  cycle (two structs holding `std::vector`s of each other are fine).
* The modules whose headers include each other (§3.3) must form no cycle.
  A cycle is `E0422`, on each shared definition whose need closes it, with
  the chain as related locations: *"Modules `shapes` and `colours` need
  each other's shared definitions: `Shape` (in shapes) holds a `Palette`,
  and `Palette` (in colours) uses `kMaxShapes` (in shapes). Move one of
  them into the other module."* One cycle check over at most 256 modules.

### 4.4 Initialisation across modules (`E0423`)

A global set before the program starts is set in definition order inside
its translation unit, and in an order C++ leaves unspecified across
translation units. An `initialised` edge from a global of one module to a
global `b` of another is `E0423`, unless `b` is a `constexpr` constant or a
`const` constant whose value L10 proves a constant expression of a type it
evaluates (numbers, `bool`, `char`): C++ sets those before any code runs.
A variable (not `const`) is reported even with a constant value, since
another module's initialiser may change it first. Text: *"`greeting` is
set from `name` of module `people` before the program starts, and C++ does
not say which of the two is set first. Set `greeting` at the start of
'when program starts', or make `name` a constant with a fixed value."*
Calls into library packs and Raw C++ are not followed (M4). Reads in
`main` and in functions called only from `main` are never reported.

## 5. Building several translation units

### 5.1 `gen/` holds exactly the generated files

After `write_generated_tracked` (`crates/b2c-build/src/compile.rs:711`),
the build's `prepare` deletes every entry of `BuildDir::gen_dir()` that is
a regular file (by `symlink_metadata`, never through a link), whose name
passes the generated-name rule (`build_dir.rs:378-392`), and that this
build did not write. A deletion counts as a change (`files_differ`,
`compile.rs:809`). The pruning is a separate function called only there:
`write_generated_files` (`build_dir.rs:304-306`), which `b2c generate`
uses on a folder the user chose (`crates/b2c-cli/src/commands/project.rs:82`),
never deletes anything.

### 5.2 Steps

* **Single-module:** unchanged, one compile-and-link invocation
  (`compile.rs:904-909`).
* **Multi-module:** each translation unit (the modules' `.cpp` files in
  module order, then the IDE init unit) is compiled on its own, at most 8
  at once (`compile.rs:172`), then linked. Objects are passed relative to
  the compiler's working directory (`diag/`): `../obj/<stem>-<key12>.o`,
  or `../out/<stem>.o` without the cache. 257 objects with 64-character
  stems take 257 × 87 ≈ 22,400 characters, below Windows' 32,767.
* **Progress** counts reused units as compiled at once.
* **Limits** stay: per-unit time and memory, at most 256 modules
  ([05 §5.6](../../spec/05-project-format.md#56-validation-limits)), so at
  most 513 generated files and 257 translation units.

### 5.3 The object cache (if-time; *owner to confirm*)

* **Key:** SHA-256 of the toolchain fingerprint, the compile `argv` without
  its source and output paths, the unit's text, the text of every generated
  header in path order, and `b2c_support.hpp`
  ([07 §7.5.1](../../spec/07-toolchain-build-run.md#751-build-directory)).
  Every header, not only the included ones: simpler, and safe once Raw C++
  can include a project header (M4).
* **Files:** `obj/<stem>-<key12>.o` and `obj/<stem>-<key12>.messages.json`,
  the unit's `b2c_toolchain::diagnostics::ParsedOutput` (already
  `Serialize` and `Deserialize`). Both are written to temporary names in
  `obj/` and renamed after a successful compile.
* **Reuse:** a unit whose two files exist is not compiled. Its messages are
  read (at most 4 MiB, a regular file, parsed strictly); anything wrong is a
  miss and the unit is compiled. The messages are mapped again with the
  current source map, so a reused unit's warnings are still reported.
* **Failure and cancel:** a failed or cancelled build removes only its
  temporaries and the executable (today every step's `-o` output is
  removed, `compile.rs:801-803`, `:831-837`); cached objects stay, so after
  fixing an error in one unit only that unit compiles.
* **Pruning:** after a successful link, `obj/` keeps only that build's
  files (regular files matching the pattern, never through a link).
* Without the cache, a multi-module build compiles every unit into `out/`,
  in parallel, as M2 does.

## 6. Diagnostics across files

* Header code has source-map ranges like any other, on the blocks of its
  module. A forward declaration maps to the defining block of its type.
* An unmapped place in a file attaches to `FileMap::module` (this replaces
  the first-range lookup of `compile.rs:1352-1363`, which finds no module
  for a file without ranges); in `b2c_support.hpp` it attaches to the
  project.
* Every translation unit that includes a header reports that header's
  messages. The build keeps the first, in translation-unit order, and drops
  the later ones with the same code, severity, file, line, column and text.
  Include chains are not merged: `#include` lines have no source-map range
  (`crates/b2c-codegen/src/emit.rs:83-92`) and only related locations on a
  block are kept (`compile.rs:1374`). The build output keeps every unit's
  raw text.
* W4 adds friendly messages for `-Wunused-function` and `-Wunused-variable`
  in a multi-module `.cpp`: *"`helper` is not used in this module and not
  shared."*

## 7. Editor

### 7.1 The `SHARED` field

* A checkbox field named `SHARED`, default `false`, on every definition
  block that can be shared: `func.define` (`catalog/core/functions.toml:3-29`)
  gains it in W3; W2's struct, enum, global variable and global constant
  blocks have it from their first version. Namespace blocks have none
  (each member is shared or not); *when program starts* is never shared.
* The catalog checks that a field named `SHARED` is a checkbox defaulting
  to `false`. A file that leaves it out loads unchanged
  ([03 §3.11.3](../../spec/03-block-language.md#3113-block-versioning),
  ADR-0015). All of it ships in catalog 1.3.0 (§10).
* Label: Friendly *share with other modules*, C++ *in `<stem>.hpp`*. A
  checkbox at the end of the block. If a later wave adds the block's ⚙
  popover (03 §3.7.2 plans one for variables), the same field moves into it
  with no format change.
* The editor shows it only in a multi-module project (W3); until W3 it is
  never shown. Hidden, its value is kept.
* It is content: saved, hashed, copied with the block. It is not part of
  the security summary
  ([08 §8.3.1](../../spec/08-security.md#831-how-trust-is-decided)).

### 7.2 Modules and the C++ tab

* Modules can be added, renamed and deleted (M3,
  [04 §4.13](../../spec/04-user-interface.md#413-what-the-m2-editor-includes)).
  The rename and add dialogs refuse what the loader and `E0221` refuse.
  Renaming changes file names and `#include` lines only, because blocks
  refer to symbol IDs. Deleting a module whose shared definitions are used
  leaves `E0201` on each use.
* The C++ tab lists the files in the generated order (§3.1) and opens the
  `.cpp` whose `module` is the module on the canvas (this replaces the
  position lookup of `apps/desktop/src/app/panels.tsx:113-122`; the file
  switcher exists, `apps/desktop/src/panels/code/CodePanel.tsx:79-81`).

### 7.3 `use header of module` (if-time)

Including a module's header is automatic (§3.5). For an explicit include,
as courses write it, a separate block `use header of module [utils]`
(`org.use_module`) has one field of a new catalog field kind `module`. Its
value is a plain string holding the module's ID, so there is no new value
shape and no format version. The resolve stage checks it against the
document's modules (`E0607` when the ID is not one of them, for example
after a paste from another project, or after the module is deleted).
Renaming a module keeps its ID, so the include follows. It adds
`#include "<stem>.hpp"` to its module's `.cpp` when that module has a
header, and nothing otherwise. `use header` itself names standard headers
only (types.md §5.4).

## 8. Export (W3)

### 8.1 Layout

```text
<chosen folder>/<slug>/
├── src/<stem>.cpp, src/<stem>.hpp, src/b2c_support.hpp   (no init unit)
├── CMakeLists.txt     add_executable(<slug> …) with the .cpp files in module order
├── Makefile           one object per .cpp, each depending on its .cpp and every .hpp
├── README.md
└── .b2c-export.json   the manifest (§8.3)
```

No user text is written into `CMakeLists.txt` or the `Makefile` except the
slug. The project name appears in comments through the comment encoder and
in `README.md` as an escaped code span.

### 8.2 The slug

1. Lower-case ASCII letters and digits are kept; every other run of
   characters becomes one `-`.
2. Cut to 64 characters, then trim `-` at both ends.
3. If the result does not start with a letter (empty, or `2048-game`),
   prefix `project-`, then cut to 64 and trim again.
4. If the result is a device name or starts with `b2c`, use `project`.

The result always passes `FileStem::new`. Tests: `3D Shapes` gives
`project-3d-shapes`; a 70-character name whose 64th character is a space
gives no trailing `-`; `Con` gives `project`.

### 8.3 The manifest

```json
{ "format": "blocks2cpp/export-manifest", "formatVersion": 1,
  "files": [ { "path": "src/main.cpp", "sha256": "…64 hex…" }, … ] }
```

It lists every file the export wrote except itself, with the SHA-256 of
what it wrote. It is untrusted input: the folder may have been copied,
cloned or edited.

* **Read:** a regular file, not through a link, at most 256 KiB, parsed by
  the strict JSON parser (`b2c-model`); unknown keys, a duplicate path, a
  path in different case from another, or more than 520 entries refuse it.
* **Entries:** only `CMakeLists.txt`, `Makefile`, `README.md`,
  `src/b2c_support.hpp`, or `src/<stem>.cpp` and `src/<stem>.hpp` with a
  stem that passes `FileStem::new`. No other separator, no `..`, no
  absolute path.
* **Replace**, in order:
  1. The export folder and `src/` are opened without following links;
     either being a link or a junction refuses Replace.
  2. A manifest that is missing or fails the checks above refuses Replace:
     the folder counts as one Blocks2Cpp did not make.
  3. Every listed file that exists must be a regular file whose content
     still matches its hash; every file the new export writes that already
     exists must be listed. Otherwise Replace stops before writing and names
     the files.
  4. The new files are written atomically, the listed files the new export
     no longer writes (a deleted module's) are deleted, and the new manifest
     is written last.
* Nothing is ever deleted or overwritten that the manifest does not list
  with a matching hash (08 §8.6, M3-D3).
* `b2c generate --export` follows the same rules and replaces only with
  `--replace`. Plain `b2c generate` writes the generated files by name, as
  today, and never deletes anything.

## 9. Codes

The allocation of types.md §6, which ADR-0016 to ADR-0019 share. Rows for
ADR-0019:

| Code | Meaning | First emitted |
| --- | --- | --- |
| `B2C-E0206` | another module's definition that it does not share (new text) | W3 |
| `B2C-E0213` | a name that `::name` or `geo::name` would also reach (06 §6.14.7) | W3 |
| `B2C-E0214` | a shared definition's header part names a non-shared one (was `E0424` in the draft) | W3 |
| `B2C-E0221` | a module name starts with `b2c` | W3 |
| `B2C-E0420`, `B2C-E0421` | declaration cycles (06 §6.7) | W2 (structs) |
| `B2C-E0422` | modules' headers form a cycle | W3 |
| `B2C-E0423` | a global set before the program starts from another module's global | W3 |

Each code gets its reference entry in `docs/reference/diagnostics/` in the
pull request that first emits it; `E0206`'s entry
(`analyser.md`, *Function of another module*) is rewritten then.
`tools/check-diagnostics-docs.py` enforces it.

## 10. Versions

From the plan of types.md §7:

| What | ADR-0019's part |
| --- | --- |
| `formatVersion` | no change: no new key, and `use header of module` stores a string |
| catalog | `SHARED` and the `module` field kind ship in 1.3.0, with every other catalog change up to the M3 release. A `func.define` an M3 editor wrote carries the field (03 §3.11.3), so M2 reports it as from a newer catalog (`E0602`), as ADR-0015 intends |
| source map | version 1 with the optional `module` key, until M3-5 defines the compact form, which must carry the module |
| `IPC_VERSION` | no change |

## 11. Work plan

| Wave | Items | Needs |
| --- | --- | --- |
| W1 | §1: `b2c-ir` types with defaults, `is_device_name` with both callers switched (no behaviour change), the `b2c-ipc` mirrors, `ipc-types` regenerated | — |
| W2 | the declaration graph (§4.1, §4.2) with `E0420` and `E0421` in `b2c-lang`, filling `source_order`; codegen printing in that order; the `SHARED` field on the new definition blocks (never shown yet) | W1 |
| W3, first part | functions-only multi-module: `SHARED` on `func.define`, visibility and precedence, `E0206`, `E0213`, `E0214` for function signatures, `E0221`, headers and includes for functions, `b2c_support.hpp`, `gen/` pruning, the build's steps and de-duplication, `FileMap::module`, the module tabs, the C++ tab | W1 |
| W3, second part | structs, enums, globals and constants across modules: the rest of §3.2 and §3.3, `E0214` for fields and constants, `E0422`, `E0423`, the `shapes.b2c` golden; export (§8) | W2's definition blocks and graph |
| W3, if-time | the object cache (§5.3), `use header of module` (§7.3) | — |
| W4 | the *Share* quick fixes, the friendly `-Wunused-*` messages; if-time a *Move to module* command that keeps symbol IDs | W3 |

## 12. Tests

**Unit.**

* `b2c-ir`: `FileStem` accepts and refuses names (pattern, length, device
  names in any case and with an extension, the `b2c` prefix in any case);
  a property test that every accepted stem gives `.cpp` and `.hpp` names
  valid on Windows and Linux that never equal `b2c_support.hpp` or
  `b2c_ide_init.cpp`; `is_device_name` gives the loader's answers on the
  loader's tests; `Layout::flat`.
* `b2c-lang`:
  * graph and order: each edge kind of §4.1; `int a = f();` with `f`
    reading `b` prints `b` first; a struct default reading a global, and a
    default argument reading one, add `initialised` edges; `E0420` and
    `E0421` cycles; a single-module program with only functions keeps
    today's order;
  * each row of §3.3: an include or a forward declaration, including a
    field with a default value, a field of a comparable and of a printable
    struct, and each container other than `std::vector`;
  * `E0422` for two- and three-module cycles;
  * `E0423` direct, through a chain of calls, through a struct's default
    member initialiser (`Tally tally;`) and through a default argument;
    none for a `constexpr` source, a `const int k = 10;` source, or a read
    in `main`; one for `int count = 0;` and for a `const std::string`;
  * `E0214` for each header part of §3.3 that names a non-shared
    definition, none for a function body that does;
  * `E0206` for a non-shared use, none for a shared one; precedence: an own
    non-shared `area` hides another module's shared `area` in the scope
    query and for a typed name; the `E0213` cases; `E0221` in a single- and
    a multi-module project; a single-module program has no header and
    `shared` false whatever the field says.
* `b2c-core-wasm`: paste of a block calling `::area` into a module with its
  own `area` binds to it, and into a module without one to the shared one.
* `b2c-build`: de-duplication of a header message reported by two units
  (from recorded SARIF); the link `argv` for 256 modules with 64-character
  stems stays under 32,000 characters; `gen/` pruning deletes a stale
  `old.cpp`, keeps a link and a non-generated name, and never runs in
  `write_generated_files`; with the cache, an unreadable or oversized
  message file is a miss.
* `b2c-cli`: `b2c generate` into a folder holding an extra `utils.cpp` and
  `notes.hpp` leaves both in place.

**Snapshots** (`b2c-codegen`): a header and a `.cpp` for each row of
§3.2's table; include order; forward declarations; non-shared members of a
namespace; file directives with headers; `b2c_support.hpp`; a non-shared
global reading a shared global of its module (the shared one first, the
anonymous namespace reopened); a non-shared function using a shared `const`
constant as a `case` label (compiles: step 3 prints the constant before any
function definition).

**Properties.**

* Random multi-module programs (2 to 5 modules; shared and non-shared
  functions, structs with by-value and `std::vector` fields, with and
  without default values, comparable and printable ticks, constants,
  globals) under C++17 and C++20: without g++, generation is deterministic,
  the header include graph is acyclic whenever no error is reported, and
  editing a function body or a non-shared definition leaves every header
  byte-identical (§3.6). With g++, a bounded number of cases in CI: each
  header compiles alone through a one-line unit `#include "<stem>.hpp"`,
  and the program compiles and links with the debug configuration.
* Reordering the modules leaves every file's contents unchanged.

**Golden.**

* The 15 single-module examples and the green assignments generate the same
  bytes as before.
* A new multi-module example, `examples/shapes.b2c` (modules `main`,
  `geometry`, `report`): a shared struct, enum, `constexpr` constant,
  variable and function; same-named non-shared helpers in two modules; one
  namespace in two modules. Its folder holds `main.cpp`, `geometry.cpp`,
  `geometry.hpp`, `report.cpp`, `report.hpp`, `b2c_support.hpp` and the
  run data.
* `golden.rs` fails when a golden folder holds a `.cpp` or `.hpp` the
  generator no longer writes (with `B2C_UPDATE_GOLDEN` it deletes it), and
  compiles each `.hpp` alone as above. Run data never ends in `.cpp` or
  `.hpp`, so the rule is unambiguous.
* Run tests for the two order programs of the snapshots: they print the
  values the blocks say.

**Build** (`crates/b2c-build/tests/build_sessions.rs`): a multi-module
build compiles every unit and links; renaming a module leaves no old file
in `gen/` (or `obj/`); a warning in a header is reported once. With the
cache: after an edit inside one function exactly one unit is compiled; a
reused unit's warning is still reported; a change to a shared definition
recompiles every unit; an error in one unit, fixed, recompiles only that
unit; a cancelled build keeps the cached objects. A nightly job builds a
generated 256-module project on Windows and Linux.

**Export:** the exported `shapes` example builds with CMake and with the
`Makefile` on Linux, and with the `Makefile` under MSYS2 on Windows, and
prints the golden output; a project named
`x) execute_process(COMMAND false) #` exports scripts that contain only
the slug; a project named `2048 Game` exports as `project-2048-game`.
Security corpus for Replace: a manifest with `../x`, an absolute path, a
backslash, a duplicate, an unknown key, an oversized file; `src/` as a link
or junction; a listed file edited since the export; an unlisted
`src/utils.cpp` that the new export would write. Each refuses Replace and
leaves every file as it was.

**Fuzz.** The resolve and analyse targets get multi-module seeds;
computing the layout never panics and stays linear in the number of
declarations and uses.

**E2E** (Linux and Windows): add a module, define a function, share it,
call it from `main`, run and see its output; the C++ tab shows `utils.cpp`
and `utils.hpp`; clicking a header line selects the *define* block; rename
the module and run again; delete it and see `E0201` on the call.

**Benchmarks.** The native pipeline benchmark gets a case of 8 modules with
1,000 blocks in all, reported next to the 1,000-block single module. The
build tests count compile steps instead of timing them.

## 13. Spec sections to change

With the code, in the wave named (the spec owner table of next steps §3
takes these rows).

| Section | Change | Wave |
| --- | --- | --- |
| [02 §2.5.1](../../spec/02-architecture.md#251-requests-and-responses) | `GeneratedFile`, `FileMap` and `SymbolInfo` keys | W1 |
| [03 §3.7.1](../../spec/03-block-language.md#371-program) | `main` in any module | W3 |
| [03 §3.7.7](../../spec/03-block-language.md#377-functions) | functions across modules | W3 |
| [03 §3.7.13](../../spec/03-block-language.md#3713-organisation) | `use header` names standard headers; `use header of module` | W3 |
| 03 §3.8 item 1, [§3.9](../../spec/03-block-language.md#39-support-helpers) | the *shared* option, automatic includes, the toolbox, helper placement by module count; drop the "In M2" notes | W3 |
| [04 §4.2](../../spec/04-user-interface.md#42-toolbox) | *Your functions* with other modules' shared functions | W3 |
| [04 §4.3](../../spec/04-user-interface.md#43-code-panel-live-c) | the file list and the default file | W3 |
| [04 §4.13](../../spec/04-user-interface.md#413-what-the-m2-editor-includes) | module add, rename and delete land | W3 |
| [05 §5.6](../../spec/05-project-format.md#56-validation-limits) | module names starting with `b2c` load but do not build (`E0221`) | W3 |
| [05 §5.12](../../spec/05-project-format.md#512-clipboard-format) | qualified names of functions in a multi-module project; precedence | W3 |
| [06 §6.1](../../spec/06-compiler-pipeline.md#61-overview) | the layout is computed in stage ⑤ | W3 |
| [06 §6.5](../../spec/06-compiler-pipeline.md#65-stage--names-and-scopes) | visibility, precedence, scope query, `SymbolInfo::shared` | W3 |
| [06 §6.7](../../spec/06-compiler-pipeline.md#67-ordering-and-declarations) | the `initialised` edge and the tie-break (W2); the table, include or forward-declare, the `.cpp` order with the namespace reopened, `E0214`, `E0422`, `E0423`, the invariant, the empty `.cpp`; drop "In M2" (W3) | W2, W3 |
| [06 §6.9](../../spec/06-compiler-pipeline.md#69-source-maps) | `module`, header messages | W3 |
| [06 §6.10](../../spec/06-compiler-pipeline.md#610-includes-and-support-helpers) | includes per file (types.md owns the header source, W1) | W3 |
| [06 §6.11](../../spec/06-compiler-pipeline.md#611-export) | §8: slug, manifest, Replace | W3 |
| [06 §6.14.7](../../spec/06-compiler-pipeline.md#6147-names-that-must-change), [§6.14.11](../../spec/06-compiler-pipeline.md#61411-names-typed-in-slots) | precedence of a module's own name | W3 |
| [07 §7.5.1](../../spec/07-toolchain-build-run.md#751-build-directory) | `gen/` pruning; if the cache lands, its file names, message files, temporary names and pruning | W3 |
| [07 §7.5.2](../../spec/07-toolchain-build-run.md#752-invocation) | relative object paths | W3 |
| [07 §7.5.3](../../spec/07-toolchain-build-run.md#753-diagnostics-capture-and-mapping) | de-duplication; replay | W3 |
| [07 §7.6.3](../../spec/07-toolchain-build-run.md#763-ide-init-unit) | the init unit's stem is covered by the reserved `b2c` prefix | W3 |
| [08 §8.4.1](../../spec/08-security.md#841-identifiers) rule 6, [08 §8.6](../../spec/08-security.md#86-filesystem-safety) | the `b2c` prefix; `FileStem`; pruning; the manifest checks; export scripts | W3 |
| [09 §9.2](../../spec/09-quality-and-delivery.md#92-testing-strategy) | the golden layout and checks of §12 | W3 |

Also: the module documentation of `crates/b2c-core-wasm/src/clipboard.rs`
(*Qualified names in milestone M2*) and types.md §5.1 (a `const`
constant's value is a `body` header, §3.4).

## 14. Evidence

Checked with g++ 11 to 14, `-std=c++17` and `-std=c++20`,
`-Wall -Wextra -Wpedantic`:

* The layout of §3 compiles and links: a shared struct, enum,
  `inline constexpr` constant, `extern` variable and function, and
  same-named non-shared helpers in two modules' anonymous namespaces.
* Two headers that include each other, each needing the other's type, fail
  whichever is included first; a forward declaration fixes the case where
  one side only names the type in a function declaration.
* A header may declare a `std::vector<Point>` field with `Point` only
  forward-declared, but not with `= {}` or `{}` (*invalid use of incomplete
  type*), nor `std::vector<Color> cs = {Color::Red}` with
  `enum class Color;`. libstdc++ also accepts the other M3 containers
  without a default value, which the standard does not promise. The C++17
  hidden-friend comparisons fail on such a field; the C++20 defaulted
  `operator<=>` does not. Parameters, return types and `extern` types
  compile with only forward declarations.
* The draft's fixed `.cpp` order (every non-shared definition before the
  shared ones) set `int doubled = base * 2;` before `const int base =
  compute();` and printed `doubled=0`, and read an unconstructed
  `std::string`, with no warning; and `case kQuit:` before the shared
  `const int kQuit = 4;` failed (*not usable in a constant expression*).
  §3.5 and §4.2 replace it.
* `int a = f();` printed before the `int b = compute();` that `f` reads,
  in one file, prints `a=0` (§4.1).
* Across two units, a global set through a struct's default member
  initialiser or a default argument reading the other module's global
  prints `0` or `42` depending on the link order (§4.4).
* A non-inline `operator==` in a header included by two units fails the
  link with *multiple definition*; a hidden friend links.
* An unused function or variable in an anonymous namespace draws
  `-Wunused-function` or `-Wunused-variable`, an error under
  `warningsAsErrors`; the same function at global scope does not.
* Compiling a header as the main file warns `#pragma once in main file`,
  so the header-alone check compiles a one-line unit that includes it.
