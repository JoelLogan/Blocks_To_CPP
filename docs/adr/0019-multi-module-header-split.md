# ADR-0019: Multi-module projects: shared definitions and one header per module

* Status: Proposed (M3 wave W0, item L18; the points marked *owner to
  confirm* are still open)
* Date: 2026-10-24
* Confirms and completes the header split of
  [06 §6.7](../spec/06-compiler-pipeline.md#67-ordering-and-declarations) and
  the object cache of
  [07 §7.5.1](../spec/07-toolchain-build-run.md#751-build-directory). Builds
  on [ADR-0006](0006-using-namespace.md) (no directive in a header; the
  anonymous namespace of a multi-module `.cpp`),
  [ADR-0013](0013-additive-format-keys.md) and
  [ADR-0015](0015-block-upgrades-on-load.md) (no format change: one
  compatible catalog field). Ends M2's rule that every module is compiled on
  its own.

## Context

### What M3 needs

M3 brings "multi-module projects with headers" and "export to CMake/Make"
([10 §10.1](../spec/10-roadmap.md#m3-language-breadth-i)). The plan lists the
header split as L18; wave W3 builds it and waits for this ADR, and W1 carries
its contracts ([next steps §3.4](../plans/next-steps.md#34-waves)).
Multi-module projects are in M3's must-have set (M3-D6).

None of the ten M3-D1 assignments needs a second module
([`tests/assignments/`](../../tests/assignments/README.md)). So the first
rule is that a single-module project generates exactly what it does today:
the assignments and the 15 golden examples must not change by one byte.

The specification already sketches the design:

* [03 §3.8](../spec/03-block-language.md#38-organisation-features-editor)
  item 1: modules are tabs and files; a module becomes `name.cpp`, plus
  `name.hpp` when it has *shared* definitions; using another module's shared
  definitions adds the `#include` by itself.
* [03 §3.9](../spec/03-block-language.md#39-support-helpers): helpers inline
  in a single-module project, in `b2c_support.hpp` otherwise.
* 06 §6.5 gives each symbol a visibility (module-local or shared); 06 §6.7
  has the table of what goes into `module.hpp` and `module.cpp`, puts
  non-shared definitions in an anonymous namespace and uses `#pragma once`;
  [06 §6.14.3](../spec/06-compiler-pipeline.md#6143-where-directives-go) and
  §6.14.7 keep directives out of headers and define `E0213`.
* 07 §7.5.1 lists `gen/main.hpp` and a content-addressed `obj/` with its key.
* [08 §8.4.1](../spec/08-security.md#841-identifiers) rule 6 and
  [08 §8.6](../spec/08-security.md#86-filesystem-safety): file names come
  only from validated module names.

It leaves open: how *shared* is stored; what a header includes and what it
only forward-declares; cycles between modules; constants that are not
constant expressions; the order in which globals of different modules are
initialised; reserved file names; a message in a header that several
translation units report; the warnings of a cached object; how the app finds
a module's files; the golden layout for several files; and export's names.

### What the code does today

* **Generation.** One `<module>.cpp` per module, in module order
  (`crates/b2c-codegen/src/lib.rs:11-13`, `:122-137`), and `b2c_support.hpp`
  when helpers are placed in a header (`lib.rs:138-150`, with
  `#pragma once` at `crates/b2c-codegen/src/emit.rs:167`). The helpers are
  `inline` (`crates/b2c-codegen/src/helpers.rs:69`), so many translation
  units can include them. There is no module header: a module's functions
  are forward-declared in its own `.cpp` (`emit.rs:143-152`).
* **Helper placement** is always inline in the build and in the preview
  (`crates/b2c-build/src/frontend.rs:77`,
  `crates/b2c-core-wasm/src/facade.rs:286`); only codegen tests use the
  header, among them a two-module snapshot whose `dice.cpp` defines `roll`
  at global scope (`crates/b2c-codegen/tests/it/snapshots.rs:954-978`).
* **File names.** The loader checks module names against
  `[a-z][a-z0-9_-]{0,63}`, Windows device names and case-insensitive clashes
  (`crates/b2c-model/src/decode/workspace.rs:98-110`, `:153-189`; `E0117`
  to `E0119`). Codegen checks the stem again and renames a module that would
  be `b2c_support` (`lib.rs:164-192`). The build folder refuses any other
  name and the init unit's stem `b2c_ide_init`
  (`crates/b2c-build/src/build_dir.rs:381-392`), so a module named
  `b2c_ide_init` passes the loader and the generator and then fails the
  build as an internal error. The device-name check exists twice
  (`workspace.rs:106-110`, `build_dir.rs:398-405`).
* **IR.** `Program { standard, modules, symbols }`,
  `Item { kind, origin, comment }` and `ItemKind` `Main | Function`
  (`crates/b2c-ir/src/sast.rs:71-111`): no visibility. `SymbolInfo` names
  the declaring module (`crates/b2c-ir/src/scope_info.rs:27-43`).
* **Analyser.** One `main` in the whole project
  (`crates/b2c-lang/src/lower/mod.rs:345-384`, `E0405`/`E0406`); function
  names unique in the whole program (`lower/mod.rs:452-470`, `E0211`); a
  call to another module's function is `E0206`
  (`crates/b2c-lang/src/lower/resolve.rs:220-233`). The scope index keeps,
  per module, the functions its calls may use
  (`crates/b2c-lang/src/query.rs:76-78`).
* **Source maps.** `GeneratedFile { path, kind: Source | Header, contents }`
  and `FileMap { path, ranges }` (`crates/b2c-ir/src/source_map.rs:20-55`);
  lookups are per file (`source_map.rs:89-114`). The build attaches an
  unmapped place in a generated file to the module of that file's first
  range (`crates/b2c-build/src/compile.rs:1352-1363`), which finds no module
  for a file without ranges.
* **Build.** One translation unit is compiled and linked in one invocation;
  several are compiled in parallel, at most 8 at once (`compile.rs:171-172`,
  `:1008`), into `out/<stem>.o` (`compile.rs:917`) and then linked
  (`compile.rs:897-934`). There is no object cache: a build that is not up
  to date recompiles every translation unit (`compile.rs:661`, `:698-758`).
  Messages are collected per translation unit, in their order, without
  de-duplication (`compile.rs:1056-1073`); include chains become related
  locations (`compile.rs:1367-1395`). `gen/` is passed with `-iquote`
  (`crates/b2c-toolchain/src/command.rs:253-254`).
  `write_generated_tracked` never removes a file the project no longer
  generates (`build_dir.rs:320-345`). The executable is named after the
  first module (`compile.rs:877-895`).
* **App.** The C++ tab finds a module's `.cpp` by its position among the
  source files (`apps/desktop/src/app/panels.tsx:113-122`) and already has a
  file switcher (`apps/desktop/src/panels/code/CodePanel.tsx:79-81`).
* **Golden test.** Every generated file is compared with
  `tests/golden/<name>/<path>` (`crates/b2c-build/tests/golden.rs:102-136`);
  a checked-in file that the generator no longer writes goes unnoticed.
* **Export.** `b2c generate --export` drops the banner and writes the files
  flat (`crates/b2c-cli/src/commands/project.rs:55-93`).

Checked with g++ 13 (`-std=c++17` and `c++20`, `-Wall -Wextra -Wpedantic`)
while drafting:

* the layout of Decision §4 compiles and links: a shared struct, enum,
  `inline constexpr` constant, `extern` variable and function, and
  same-named non-shared helpers in two modules' anonymous namespaces;
* two headers that include each other, each needing the other's type, fail
  whichever is included first; a forward declaration fixes the case where
  one side only names the type in a function declaration;
* a header may declare `std::vector<Point>` with `Point` only
  forward-declared, but a `.cpp` that creates such an object needs
  `Point`'s header;
* an unused function or variable in an anonymous namespace draws
  `-Wunused-function` or `-Wunused-variable`; the same function at global
  scope in a single-module `main.cpp` does not;
* compiling a header as the main file warns `#pragma once in main file`.

## Options considered

**What crosses a module boundary.**

1. **An explicit *shared* option per definition (chosen; the spec's
   sketch).** One step (a tick, or the quick fix of `E0206`) before another
   module can use a definition. In return a header depends only on its own
   module, non-shared names may repeat across modules, and it is the `.h` /
   `.cpp` split that courses teach.
2. **Automatic: shared when another module uses it.** No step. But a
   module's header and the linkage of its definitions change when another
   module is edited, an `E0213` clash can appear in a module because of an
   edit elsewhere, and each such edit changes a header and recompiles most
   translation units.
3. **Everything shared** (M2's global layout plus headers). Simplest. Every
   name must be unique in the program, no helper stays private, and every
   function lands in a header.

**How one module uses another.**

1. **Automatic includes (chosen; 03 §3.8).** Referring to a shared
   definition adds the include; `use header [module]` adds one explicitly.
2. **A `use module [utils]` block** before anything of `utils` is visible.
   It mirrors `#include` and keeps menus short, but adds a block to remember
   and an error for a forgotten one, for a fact the generator already knows.

**Header layout.**

1. **One header per module** that includes the headers whose definitions it
   needs complete and forward-declares the other types it names; a cycle
   between modules is an error (chosen).
2. **One project header** with every shared declaration in one dependency
   order. No cycle between modules can occur, but it is not the layout
   courses teach or graders expect, and every shared change touches it.
3. **Headers without includes,** each `.cpp` including all of them in one
   global order. Headers are then not self-contained (a submitted or
   exported header fails when included alone), and the cycle remains.

**Include guard.** `#pragma once` (chosen, as the spec and
`b2c_support.hpp` have it) or `#ifndef` guards. Guards are standard C++ and
many courses teach them, but the macro needs a name derived from the module
name, and `a-b` and `a_b` are both valid, distinct module names whose obvious
macro is the same.

**Build.** A per-translation-unit object cache (chosen, as 07 §7.5.1 has
it), or recompiling every translation unit whenever anything changed. Without
the cache an edit to one module recompiles all of them in parallel; with it,
only those whose text or headers changed, at the cost of replaying the
warnings of reused objects and pruning old ones.

## Decision

### 1. Modules

* A module is one canvas of the project (a tab), with the ID and name of its
  entry in `modules` ([05 §5.3](../spec/05-project-format.md#53-top-level-structure)).
  It is the unit of a translation unit (`<stem>.cpp`), of at most one header
  (`<stem>.hpp`), of privacy (what it does not share is invisible elsewhere)
  and of `use namespace … in this file`.
* The module order decides only the order of the generated files and of the
  source map, the link order and the executable's name (the first module's,
  as now). It never changes a file's contents.
* Every module gets its `.cpp`, even one with nothing to generate (it holds
  the header comment), so the files follow the tabs.
* A project with one module is a **single-module project**. Its output is
  exactly today's: one `<stem>.cpp`, no header, no anonymous namespace,
  inline helpers. The *shared* option has no effect there and is hidden.

### 2. Sharing

* Every definition block that can be shared has a checkbox field `SHARED`,
  default `false`:
  * `func.define` gains it now (`catalog/core/functions.toml:4-29`). It is
    a compatible change: the catalog's minor version goes up and a file
    that leaves the field out loads unchanged
    ([03 §3.11.3](../spec/03-block-language.md#3113-block-versioning),
    ADR-0015);
  * the struct, enum, alias, global variable and global constant blocks of
    W2 have it from their first version, and so do classes and top-level
    Raw C++ declarations in M4;
  * a namespace block has none: each of its members is shared or not;
  * *when program starts* is never shared.
* Label: Friendly *share with other modules*, C++ *in `<stem>.hpp`*. It
  sits in the block's ⚙ popover once L17 provides one, and until then is a
  checkbox at the end of the block (the same field). In a single-module
  project it is hidden and its value kept.
* New blocks start unshared (*owner to confirm*).
* `SHARED` is content: saved, hashed, copied with the block. It is not part
  of the security summary
  ([08 §8.3.1](../spec/08-security.md#831-how-trust-is-decided)).

### 3. Names and visibility

In a multi-module project:

* A shared definition is visible in every module, in its namespace, in any
  order, like a module's own functions. A non-shared definition is visible
  only in its module. The analyser sets `Item::shared` from the field and
  sets it to `false` in a single-module project.
* A reference to another module's non-shared definition is `E0206`, which
  now means *"`helper` belongs to module `utils`, which does not share it."*
  The quick fix *Share `helper`* ticks the field (W4). The old meaning (no
  call across modules at all) goes.
* Shared definitions of all modules share their namespace: two of them with
  one name in one namespace are `E0210`, or `E0211` for functions until W2's
  overloads allow different parameters. Non-shared definitions of different
  modules may share a name. A non-shared name may not be a shared name of
  its own module or of a module whose header its `.cpp` includes, directly
  or through other headers (`E0213`, 06 §6.14.7, which replaces the
  cross-module part of `E0211`).
* What a shared definition puts in the header (a function's parameter and
  return types and default arguments, a struct's field types and default
  values, a constant's type and value) may name only shared definitions,
  standard or library-pack names. Naming a non-shared one of its own module
  is `E0424` (new): *"`area` is shared, but its input `box` has the type
  `Box`, which is not. Share `Box` too."* The quick fix *Share `Box`* (W4).
  A function body is in the `.cpp` and may use anything of its module.
* Another module's constant is a constant expression (an array size, a
  `case` label) only when it is `constexpr`; W2's constant evaluation
  (L10) treats it so, without a new code.
* A module whose name starts with `b2c` is `E0221` (new): *"Module names
  that start with `b2c` are kept for files that Blocks2Cpp writes. Rename
  the module."* The loader's rule does not change, so such a file still
  opens and can be fixed (*owner to confirm*).
* The scope query lists other modules' shared definitions, also in the
  parked scope (06 §6.5, [ADR-0012](0012-parked-blocks.md)); `SymbolInfo`
  gains `shared` (written only when `true`). The toolbox's *Your functions*
  shows the module's own functions, then each other module's shared
  functions under its module heading.

### 4. What each file contains

In a multi-module project, `<stem>.hpp` exists when the module shares at
least one definition. In order:

1. the header comment (project and module name, as for the `.cpp`);
2. `#pragma once`;
3. the standard and library-pack headers that the header's own code needs
   (the per-construct headers of ADR-0017, L8), sorted;
4. the project headers it needs (below), sorted by file name;
5. `#include "b2c_support.hpp"`, only when header code uses a helper;
6. forward declarations of other modules' types that it names but does not
   need complete (`struct Point;`, `enum class Color;`), in their namespaces,
   sorted by name;
7. the shared declarations, in the dependency order of 06 §6.7 step 3,
   inside their namespace blocks. Never a directive (06 §6.14.3 rule 1).

| Shared definition | `<stem>.hpp` | `<stem>.cpp` |
| --- | --- | --- |
| function | declaration | definition |
| struct, enum, alias | full definition | nothing |
| print support of a printable struct, or of any shared enum | `operator<<` (and `to_string` for an enum) declared | defined |
| variable | `extern T name;` | `T name = …;` |
| `constexpr` constant | `inline constexpr T name = …;` | nothing |
| `const` constant | `extern const T name;` | `const T name = …;` |
| class (M4) | definition; template members in full | out-of-line non-template member bodies |
| Raw C++ declarations (M4) | the text | nothing |

A shared enum always gets its print support, not only when something prints
it, so that the header does not depend on other modules.

**Include or forward-declare.** A header includes the header of module `n`
when one of its declarations needs a complete type or a definition from
`n`: a by-value field, a base class, a `std::array` element, the type and
value of a constant defined in the header, a default argument, an inline or
template body (06 §6.7 step 3's dependency kinds), or any use of an alias of
`n` (aliases cannot be forward-declared). Any other mention of a struct,
class or enum of `n` (a parameter or return type in a function declaration,
a pointer, reference or smart pointer, an element of `std::vector`,
`std::list` or `std::forward_list`, the type of an `extern` declaration)
gets a forward declaration instead.

`<stem>.cpp`, in order:

1. the header comment;
2. the standard and library-pack headers that the `.cpp`'s own code needs,
   also when an included project header already includes them (06 §6.10);
3. the project headers: its own (if it has one), the header of every module
   whose shared definitions its code refers to (in blocks, typed slots,
   `use header [module]` blocks and, from M4, Raw C++ identifier tokens),
   and the headers from which those headers forward-declare, repeated until
   nothing is added, so that every type named in an included declaration is
   complete; sorted by file name;
4. `#include "b2c_support.hpp"` when the module uses a helper;
5. the code, as 06 §6.7 and §6.14.3 lay it out: the anonymous namespace
   that opens with the file directives and holds the non-shared definitions
   (with their forward declarations; non-shared members of namespace `geo`
   in `namespace geo { namespace { … } }`), the definitions of the shared
   functions, variables and constants in their namespaces, and `int main()`
   last, at global scope.

**Invariant.** A header depends only on its own module's shared definitions
and on the names and modules of the other modules' types and constants they
use. Editing a function body, or anything that is not shared, never changes
a header.

**Helpers.** `b2c_support.hpp` in every multi-module project that uses a
helper, inline otherwise (03 §3.9). The generator decides it from the
module count, so the option leaves `FrontendOptions` and the build and the
preview cannot differ.

### 5. Cycles and the order of initialisation

* The analyser builds one dependency graph over the declarations of the
  whole program (06 §6.7 step 3); a cycle in it is `E0420` or `E0421`, as
  now. The graph of modules whose headers include each other (§4) must have
  no cycle either. A cycle there is `E0422` (new), on each shared definition
  whose need closes it, with the chain as related locations: *"Modules
  `shapes` and `colours` need each other's shared definitions: `Shape` (in
  shapes) holds a `Palette`, and `Palette` (in colours) uses `kMaxShapes`
  (in shapes). Move one of them into the other module."* A `.cpp` is never
  included, so its includes cannot form a cycle.
* A global variable or constant whose value is not a constant expression is
  set when the program starts, and C++ does not say in which order across
  modules. Its value may read another module's global, directly or through
  the project's functions it calls (followed through every call), only when
  that global is a `constexpr` constant. Otherwise
  it is `E0423` (new): *"`greeting` is set from `name` of module `people`
  before the program starts, and C++ does not say which of the two is set
  first. Set `greeting` at the start of 'when program starts', or make
  `name` a constant with a fixed value."* Calls into library packs and Raw
  C++ are not followed. Within one module the order of 06 §6.7 holds.

### 6. `main`

One enabled *when program starts* in the project, in any module (`E0405`
and `E0406` unchanged). Its `int main()` is defined at global scope at the
end of that module's `.cpp`, is never shared and never appears in a header.

### 7. Namespaces and `using namespace`

Modules are not C++ namespaces: a module name appears only in comments,
file names and `#include` lines, so `-` in a name never matters. Namespace
blocks with one name at one level in several modules are one C++ namespace.
ADR-0006 stands: no directive in a header (`E0514`), file directives open
the anonymous namespace of a multi-module `.cpp`, and a `use namespace`
block naming a namespace that another module's header declares adds that
header (06 §6.14.3).

### 8. Contracts for W1

`b2c-ir`:

```rust
// text.rs: the only way a module name becomes part of a path.
pub struct FileStem(Box<str>);
impl FileStem {
    /// `[a-z][a-z0-9_-]{0,63}`, not a Windows device name, not starting with `b2c`.
    pub fn new(name: &str) -> Result<FileStem, StemError>;
    pub fn as_str(&self) -> &str;
}
/// The one device-name check; `b2c-model` and `b2c-build` call it.
pub fn is_device_name(stem: &str) -> bool;

// sast.rs
pub struct Item { /* kind, origin, comment, */ pub shared: bool }
pub struct Program { /* standard, modules, symbols, */ pub layout: Layout }

// layout.rs (new)
pub struct Layout {
    /// More than one module and some helper used: `b2c_support.hpp`.
    pub helpers_in_header: bool,
    /// One per module, in project order.
    pub modules: Vec<ModuleFiles>,
}
pub struct ModuleFiles {
    pub module: ModuleId,
    /// The module's name. For a name `FileStem::new` refuses (only the
    /// `b2c` prefix gets past the loader; `E0221`, so only the preview
    /// generates), `module<N>` with N its place in the module order, plus
    /// `_2`, `_3`, … while that clashes; this replaces `source_path`.
    pub stem: FileStem,
    /// Whether `<stem>.hpp` exists.
    pub header: bool,
    /// Modules whose headers `<stem>.hpp` includes, sorted by stem.
    pub header_includes: Vec<ModuleId>,
    /// Other modules' types that `<stem>.hpp` forward-declares.
    pub forward: Vec<SymbolId>,
    /// Modules whose headers `<stem>.cpp` includes (its own among them),
    /// sorted by stem.
    pub source_includes: Vec<ModuleId>,
}

// source_map.rs: `None` for `b2c_support.hpp`.
pub struct GeneratedFile { /* path, kind, */ pub module: Option<ModuleId>, /* contents */ }
pub struct FileMap { /* path, */ pub module: Option<ModuleId>, /* ranges */ }
```

* `GeneratedProject::files` and the source map list, for each module in
  module order, its `.cpp` and then its `.hpp`, then `b2c_support.hpp`.
* `b2c-lang` computes `Layout` with the dependency graph and reports
  `E0206`, `E0213`, `E0221` and `E0422` to `E0424`; the generator only
  prints it and never reports a user error.
* `b2c-codegen` reads `program.layout`; `CodegenOptions::helper_placement`
  and `FrontendOptions::helper_placement` (`frontend.rs:53-54`) go. Usage
  (includes, helpers) is tracked per output file, not per module.
* `b2c-ipc` mirrors `GeneratedFile` and `FileMap`
  (`crates/b2c-ipc/src/pipeline.rs:36`, `:61`;
  `crates/b2c-ipc/src/generate.rs:53`) with an optional `module`, and
  `@blocks2cpp/ipc-types` is regenerated. Both keys are optional and
  readers ignore unknown keys, so the source map stays version 1.

### 9. Building several translation units

* **`gen/` holds exactly the generated files.** After writing them, the
  build deletes every other regular file in `gen/` whose name is a generated
  `.cpp` or `.hpp` name, never through a link, and a deletion counts as a
  change (`files_differ`, `compile.rs:809`).
* **Single-module:** unchanged, one compile-and-link invocation, no object
  cache.
* **Multi-module:** each translation unit (the modules' `.cpp` files in
  module order, then the IDE init unit) compiles to
  `obj/<stem>-<key12>.o`. The key is the SHA-256 of the toolchain
  fingerprint, the compile `argv` without its source and output paths, the
  unit's text, the text of every generated header in path order and
  `b2c_support.hpp` (07 §7.5.1). Every header, not only the included ones:
  simpler, and safe once Raw C++ can include a project header (M4). A shared
  change recompiles every unit; an edit inside a function recompiles one
  (*owner to confirm* that the cache is must-have).
* **Warnings survive the cache.** Next to each object the build keeps
  `obj/<stem>-<key12>.messages.json`, the unit's parsed compiler output
  (`b2c_toolchain::diagnostics::ParsedOutput`). A reused unit is not
  compiled; its messages are read (at most 4 MiB, parsed strictly; anything
  wrong counts as a miss and the unit is compiled) and mapped again with the
  current source map. Both files are written by atomic rename after a
  successful compile (07 §7.5.4). After a successful link, `obj/` keeps only
  that build's files.
* **Link.** Objects in module order, then the init unit, passed relative to
  the compiler's working directory (`../obj/…`), so 257 objects with
  64-character stems take about 22,400 characters, below Windows' limit of
  32,767 for a command line.
* **Progress** counts reused units as compiled at once.
* **Limits** stay: per-unit time and memory, at most 8 at once, at most 256
  modules ([05 §5.6](../spec/05-project-format.md#56-validation-limits)).

### 10. Diagnostics and source maps across files

* Header code has source-map ranges like any other, on the blocks of its
  module. A forward declaration maps to the defining block of its type.
* `FileMap::module` names the file's module. An unmapped place in a file
  attaches to it (this replaces the first-range lookup,
  `compile.rs:1352-1363`); in `b2c_support.hpp` it attaches to the project.
* Every translation unit that includes a header reports that header's
  messages. The build keeps the first, in translation-unit order, and drops
  the others with the same code, severity, file, line, column and text,
  merging their include chains into its related locations (still at most
  16).

### 11. Editor

* Modules can be added, renamed and deleted (M3,
  [04 §4.13](../spec/04-user-interface.md#413-what-the-m2-editor-includes)).
  The rename dialog refuses what the loader and `E0221` refuse. Renaming
  changes file names and `#include` lines only, because blocks refer to
  symbol IDs. Deleting a module whose shared definitions are used leaves
  `E0201` on each use.
* The C++ tab lists the files in the generated order (§8). It opens the
  `.cpp` whose `module` is the module on the canvas (this replaces the
  position lookup, `panels.tsx:113-122`).

### 12. Export (W3)

* `src/` holds exactly the generated files, without the init unit.
  `CMakeLists.txt` lists the `.cpp` files in module order. The `Makefile`
  builds one object per `.cpp`, each depending on its `.cpp` and on every
  `.hpp` (the object cache's rule).
* The folder name, the CMake project and target and the `Makefile`'s
  program come from a slug of the project name: ASCII letters and digits in
  lower case, every other run of characters one `-`, trimmed, at most 64
  characters; `project` when that is empty, a device name or starts with
  `b2c`. It passes `FileStem::new`. No other user text is written into
  `CMakeLists.txt` or the `Makefile`; the project name appears in comments
  through the comment encoder and in `README.md` as an escaped code span.
* `.b2c-export.json` lists every file written, so *Replace* also removes the
  files of a module deleted since the last export.

### 13. Format and versions

* No format change: no new key, `formatVersion` unchanged (ADR-0013).
  `SHARED` raises the catalog's minor version (ADR-0015). An M2 file loads
  with every definition unshared. A `func.define` that an M3 editor wrote
  carries the field (03 §3.11.3), so in M2 it is reported as made by a newer
  catalog (`E0602`), as ADR-0015 intends.
* Opening an M2 project with several modules changes its generated C++ (the
  other modules' functions move into an anonymous namespace), not the file
  and not what the program does: M2 could not call across modules.
* The source map stays version 1 (§8).

### 14. Diagnostic codes

| Code | Meaning | Since |
| --- | --- | --- |
| `E0206` | Another module's definition that it does not share (changed meaning) | M2, changed in M3 |
| `E0213` | A name that `::name` or `geo::name` would also reach (06 §6.14.7) | M3 |
| `E0221` | Module name starting with `b2c` | M3 |
| `E0422` | Modules that need each other's shared definitions | M3 |
| `E0423` | A global set before the program starts from another module's global | M3 |
| `E0424` | A shared definition whose header part names a non-shared one | M3 |

Each gets an entry in `docs/reference/diagnostics/analyser.md`.

## Consequences

**Where the work lands.**

* W1 (contracts): `b2c-ir` (`FileStem`, `is_device_name`, `Layout`,
  `Item::shared`, the `module` keys, `SymbolInfo::shared`), `b2c-ipc` and
  `ipc-types`, the `SHARED` field of `func.define` and the catalog's minor
  version.
* W3: `b2c-lang` (visibility, the codes above, `Layout`), `b2c-codegen`
  (headers, forward declarations, includes per file, helper placement),
  `b2c-build` (`gen/` pruning, the object cache and message replay, relative
  link paths, de-duplication, `FileMap::module`), `b2c-model` and
  `b2c-build` calling the one device-name check, `b2c-cli` and the app's
  export (§12), the module tabs and the C++ tab.
* W2's definition blocks carry `SHARED` from their first version. W4 adds
  the *Share* quick fix and friendly messages for `-Wunused-function` and
  `-Wunused-variable` in a multi-module `.cpp` (*"`helper` is not used in
  this module and not shared"*). A *Move to module* command that keeps
  symbol IDs, which would make `E0422` one click, is a W4 if-time item.

**What gets harder.**

* Two kinds of definition. A function nobody uses yet draws a g++ warning
  in a multi-module project and none in a single-module one.
* Using one definition of another module includes its whole header, so a
  private name can start to clash (`E0213`) with an unrelated shared one.
* `E0422` asks the user to move a definition between modules.
* The analyser computes the dependency graph before generation, which
  06 §6.1 placed in stage ⑥.

**Performance.** The preview generates every module's files on each update;
headers add little text, and ADR-0016's worker carries the work. A build
after an edit inside one function compiles one translation unit and links;
a change to a shared definition compiles all of them, as a build does today.
`obj/` holds one object and one message file per unit.

**Security** ([08](../spec/08-security.md)).

* File names come only from `FileStem`. The `b2c` prefix is reserved, which
  closes the `b2c_ide_init` gap found above.
* `gen/` pruning deletes only regular files with generated names in the
  build folder and never follows a link (08 §8.6).
* Project headers are found only through `-iquote` and end in `.hpp`, so a
  module named `string` cannot shadow `<string>`.
* 256 modules give at most 513 generated files and 257 translation units,
  each under the existing per-invocation limits.
* Export writes no user text into `CMakeLists.txt` or the `Makefile`, so a
  hostile project name cannot inject CMake or `make` commands.
* Cached message files are size-bounded and parsed strictly; a bad one only
  costs a compile.
* M3's threat-model review (W6) records these.

**Specification sections to change** (W3 updates them with the code; this
ADR does not):

* [03 §3.7.1](../spec/03-block-language.md#371-program): `main` in any
  module.
* [03 §3.7.7](../spec/03-block-language.md#377-functions): functions across
  modules.
* [03 §3.7.13](../spec/03-block-language.md#3713-organisation): `use header`
  with a module stores its ID and adds the include when it has a header.
* 03 §3.8 item 1 and §3.9: the *shared* option, automatic includes, the
  toolbox, helper placement by module count; drop the "In M2" notes.
* [04 §4.2](../spec/04-user-interface.md#42-toolbox) (*Your functions*),
  [04 §4.3](../spec/04-user-interface.md#43-code-panel-live-c) (the file
  list and the default file).
* 05 §5.6: module names starting with `b2c` load but do not build (`E0221`).
* [06 §6.5](../spec/06-compiler-pipeline.md#65-stage--names-and-scopes)
  (visibility, scope query, `SymbolInfo::shared`), 06 §6.7 (the table,
  include or forward-declare, `E0422` to `E0424`, the invariant, the empty
  `.cpp`; drop "In M2"), [06 §6.9](../spec/06-compiler-pipeline.md#69-source-maps)
  (`module`, header messages), [06 §6.10](../spec/06-compiler-pipeline.md#610-includes-and-support-helpers)
  (includes per file), [06 §6.11](../spec/06-compiler-pipeline.md#611-export)
  (§12), and 06 §6.1's table (layout computed in stage ⑤).
* 07 §7.5.1 (`gen/` pruning, the message files, `obj/` pruning, the key's
  `argv` without source and output paths),
  [07 §7.5.2](../spec/07-toolchain-build-run.md#752-invocation) (relative
  object paths),
  [07 §7.5.3](../spec/07-toolchain-build-run.md#753-diagnostics-capture-and-mapping)
  (de-duplication, replay).
* 08 §8.4.1 rule 6 (`b2c` prefix) and 08 §8.6 (`FileStem`, pruning,
  export scripts).
* [09 §9.2](../spec/09-quality-and-delivery.md#92-testing-strategy) (the
  golden layout and checks below).

## Tests

**Unit.**

* `b2c-ir`: `FileStem` accepts and refuses names (pattern, length, device
  names in any case, the `b2c` prefix in any case); a property test that
  every accepted stem gives `.cpp` and `.hpp` names that are valid on
  Windows and Linux and never equal `b2c_support.hpp` or `b2c_ide_init.cpp`.
* `b2c-lang`: each dependency kind of §4 gives an include or a forward
  declaration; `E0422` for two- and three-module cycles; `E0423` direct and
  through a chain of calls, and none for a `constexpr` source or a read in
  `main`; `E0424` for each header part of §3 that names a non-shared
  definition, and none for a function body that does; `E0206` for a
  non-shared use and none for a shared one; the `E0213` cases; `E0221`; a
  single-module program has no header and
  `shared` false whatever the field says.
* `b2c-build`: de-duplication of a header message reported by two units
  (from recorded SARIF); the link `argv` for 256 modules with 64-character
  stems stays under 32,000 characters; an unreadable or oversized message
  file is a cache miss.

**Snapshots** (`b2c-codegen`): a header and a `.cpp` for each row of §4's
table, include order, forward declarations, non-shared members of a
namespace, file directives with headers, and `b2c_support.hpp`.

**Properties.**

* Random multi-module programs (2 to 5 modules; shared and non-shared
  functions, structs with by-value and `std::vector` fields, constants):
  without g++, generation is deterministic, the include graph of headers is
  acyclic whenever no error is reported, and editing a function body or a
  non-shared definition leaves every header byte-identical (§4's
  invariant). With g++, a bounded number of cases in CI: each header
  compiles alone through a one-line unit `#include "<stem>.hpp"`, and the
  program compiles and links with the debug configuration.
* Reordering the modules leaves every file's contents unchanged.

**Golden.**

* The 15 single-module examples and the green assignments generate the same
  bytes as before.
* A new multi-module example, `examples/shapes.b2c` (modules `main`,
  `geometry`, `report`): a shared struct, enum, `constexpr` constant,
  variable and function; same-named non-shared helpers in two modules; one
  namespace in two modules. Its folder holds `main.cpp`, `geometry.cpp`,
  `geometry.hpp`, `report.cpp`, `report.hpp`, `b2c_support.hpp` and the run
  data.
* `golden.rs` fails when a golden folder holds a `.cpp` or `.hpp` the
  generator no longer writes (with `B2C_UPDATE_GOLDEN` it deletes it), and
  compiles each `.hpp` alone as above. Run data never ends in `.cpp` or
  `.hpp`, so the rule is unambiguous.

**Build** (`crates/b2c-build/tests/build_sessions.rs`): a multi-module
build compiles every unit and links; after an edit inside one function
exactly one unit is compiled; a reused unit's warning is still reported;
a change to a shared definition recompiles every unit; renaming a module
leaves no old file in `gen/` or `obj/`; a warning in a header is reported
once, with its include chain. A nightly job builds a generated 256-module
project on Windows and Linux.

**Export** (W3): the exported `shapes` example builds with CMake and with
the `Makefile` on Linux, and with the `Makefile` under MSYS2 on Windows, and
prints the golden output; a project named
`x) execute_process(COMMAND false) #` exports scripts that contain only the
slug.

**Fuzz.** The resolve and analyse targets get multi-module seeds; computing
the layout never panics and stays linear in the number of declarations.

**E2E** (Linux and Windows): add a module, define a function, share it,
call it from `main`, run and see its output; the C++ tab shows
`utils.cpp` and `utils.hpp`; clicking a header line selects the *define*
block; rename the module and run again; delete it and see `E0201` on the
call.

**Benchmarks.** The native pipeline benchmark gets a case of 8 modules with
1,000 blocks in all, reported next to the 1,000-block single module. The
build tests count compile steps instead of timing them.

## Owner to confirm

1. New definitions start unshared; using one from another module needs the
   tick or the *Share* quick fix. The alternative ticks it by default in
   multi-module projects (*owner to confirm*).
2. Includes follow use; there is no *use module* block (*owner to
   confirm*).
3. `#pragma once`, not `#ifndef` guards (*owner to confirm*).
4. The object cache is in M3's must-have set; the alternative leaves it
   if-time and recompiles every unit after any change (*owner to confirm*).
5. A module name starting with `b2c` is an analyser error, so the file still
   opens, rather than a load error (*owner to confirm*).
