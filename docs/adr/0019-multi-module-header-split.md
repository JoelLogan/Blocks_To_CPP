# ADR-0019: Multi-module projects: shared definitions and one header per module

* Status: Proposed (M3 wave W0, item L18; the points marked *owner to
  confirm* are still open)
* Date: 2026-10-24, revised 2026-10-25 after review
* Completes the header split of
  [06 §6.7](../spec/06-compiler-pipeline.md#67-ordering-and-declarations) and
  the object cache of
  [07 §7.5.1](../spec/07-toolchain-build-run.md#751-build-directory). Builds
  on [ADR-0006](0006-using-namespace.md) (no directive in a header; the
  anonymous namespace of a multi-module `.cpp`),
  [ADR-0015](0015-block-upgrades-on-load.md) (a compatible catalog field)
  and [ADR-0017](0017-open-type-representation.md) (types, headers per item
  part, the shared code and version tables). Ends M2's rule that every
  module is compiled on its own. The exact shapes, the work plan, the full
  test list and the spec changes are in
  [the contracts](../plans/m3-contracts/modules.md).

## Context

M3 brings "multi-module projects with headers" and "export to CMake/Make"
([10 §10.1](../spec/10-roadmap.md#m3-language-breadth-i)), and multi-module
projects are in its must-have set (M3-D6). None of the ten M3-D1
assignments has a second module, so the first rule is that a single-module
project generates what it does today: the assignments and the 15 golden
examples must not change by one byte.

The specification sketches the design: modules are tabs and files, a module
becomes `name.cpp` plus `name.hpp` when it shares definitions, and using a
shared definition adds the `#include`
([03 §3.8](../spec/03-block-language.md#38-organisation-features-editor));
06 §6.7 has the header table and the anonymous namespace; 07 §7.5.1 has a
content-addressed `obj/`. It leaves open how *shared* is stored, what a
header includes, cycles, the order of initialisation across modules,
reserved names, repeated header messages and export's names.

What the code does today:

* **Generation.** One `<module>.cpp` per module with its functions
  forward-declared and sorted by name (`crates/b2c-codegen/src/lib.rs:116-160`,
  `emit.rs:143-152`); no module header; helpers inline in the build and the
  preview (`crates/b2c-build/src/frontend.rs:77`).
* **Names.** The loader checks the module name pattern and device names
  (`crates/b2c-model/src/decode/workspace.rs:98-111`); the build also
  refuses `b2c_ide_init` (`crates/b2c-build/src/build_dir.rs:381-392`), so
  a module of that name fails the build as an internal error.
* **Analyser.** `Item` has no visibility (`crates/b2c-ir/src/sast.rs:94-101`);
  a call to another module is `E0206`
  (`crates/b2c-lang/src/lower/resolve.rs:220-233`). There is no declaration
  graph: `E0420`, `E0421` and `E0213` exist only in the spec.
* **Build.** Units compile in parallel into `out/<stem>.o` and link
  (`crates/b2c-build/src/compile.rs:897-934`), with no object cache; a
  failed build removes every output (`compile.rs:801-803`).
  `write_generated_tracked` never removes a stale file
  (`build_dir.rs:320-345`) and also writes the folder a user gives
  `b2c generate` (`crates/b2c-cli/src/commands/project.rs:82`). Nothing
  implements the export manifest of 06 §6.11.

Checked with g++ 11 to 14
([evidence](../plans/m3-contracts/modules.md#14-evidence)): the layout below
compiles and links. A header may name another module's type in a function
or `extern` declaration with only a forward declaration, but not in a field
with a default value; the standard promises incomplete elements only for
`std::vector`, `std::list` and `std::forward_list`. Printing non-shared
definitions before shared ones sets globals from values not yet set,
silently. Across units, a global read through a default member initialiser
or a default argument changes with the link order.

## Options considered

**What crosses a module boundary.** An explicit *shared* option per
definition (chosen; the spec's sketch): one tick or quick fix, after which a
header depends only on its own module, non-shared names may repeat, and it
is the `.h` / `.cpp` split courses teach. Sharing whatever another module
uses needs no step, but an edit in one module changes another's header and
linkage. Sharing everything makes every name unique in the program and puts
every function in a header.

**How one module uses another.** Includes follow use (chosen; 03 §3.8), or
a required `use module [utils]` block, which adds a block to remember and
an error for a forgotten one, for a fact the generator already knows.

**Header layout.** One header per module that includes what it needs
complete and forward-declares the rest, with a cycle between modules an
error (chosen). One project header avoids cycles, but courses and graders
do not expect it and every shared change touches it. Headers without
includes are not self-contained, so a submitted header fails alone.

**Include guard.** `#pragma once` (chosen, as the spec and
`b2c_support.hpp` have it), or `#ifndef` guards, whose macro needs a name
derived from the module name; `a-b` and `a_b` are distinct modules with the
same obvious macro.

**Order inside a `.cpp`.** One block of non-shared definitions before the
shared ones (the first draft), or 06 §6.7's one topological order over both,
reopening the anonymous namespace where needed (chosen: the fixed block set
globals from values not yet set and broke `case` labels).

**An own name against another module's shared name.** The module's own name
wins (chosen), or `E0213` for every shared name in the project whatever is
included, which turns an edit in one module into an error in another.

**Build.** A per-unit object cache, or recompiling every unit in parallel
after any change, as M2 does (the cache costs replayed warnings and pruning).

## Decision

### 1. Modules

A module is one canvas (a tab), with the ID and name of its entry in
`modules`. It is the unit of a translation unit (`<stem>.cpp`), of at most
one header (`<stem>.hpp`), of privacy and of `use namespace … in this
file`. The module order decides only the order of the files, the link order
and the executable's name (the first module's), never a file's contents.
Every module gets its `.cpp`. A project with one module is a
**single-module project**: one `<stem>.cpp`, no header, no anonymous
namespace, inline helpers, and *shared* hidden and without effect.

### 2. Sharing

Every definition block that can be shared has a checkbox field `SHARED`,
default `false`. `func.define` gains it in W3; W2's struct, enum, global
variable and global constant blocks have it from their first version; M4's
classes and top-level Raw C++ declarations will. Namespace blocks have none
(each member is shared or not), and *when program starts* is never shared.
New blocks start unshared (*owner to confirm*). The editor shows the field
only in a multi-module project. It is content (saved, hashed, copied), not
part of the security summary, and a compatible catalog change (ADR-0015),
so an M2 file loads with every definition unshared.

### 3. Names and visibility

In a multi-module project a module sees its own definitions and every other
module's shared definitions, in their namespaces, in any order. A name
resolves first to the module's own definition; another module's shared
definition with that name is visible only when the module has none
(*owner to confirm*). The scope query, typed names and paste binding follow
this rule, so a pasted `::area` binds to the target module's own `area`
when it has one, and otherwise to the one shared `area`.

* Using another module's non-shared definition is `E0206`, with the new
  text *"`helper` belongs to module `utils`, which does not share it"* and
  the quick fix *Share `helper`* (W4).
* Shared names are unique per namespace in the project (`E0210`, `E0211`).
  A non-shared name may not be a shared name of its own module or of a
  module whose header its `.cpp` includes (`E0213`, 06 §6.14.7).
* A shared definition's **header part** (a function's signature and default
  arguments; a struct's field types, default values and ticks; a constant's
  type and `constexpr` value) may name only shared, standard or library
  names. Otherwise it is `E0214`: *"`area` is shared, but its input `box`
  has the type `Box`, which is not. Share `Box` too."*
* Another module's constant is a constant expression only when it is
  `constexpr` (L10 applies this).
* A module whose name starts with `b2c` is `E0221`, in every project. The
  loader's rule does not change, so the file opens and can be fixed
  (*owner to confirm*). This closes the `b2c_ide_init` gap, and it is the
  one change to a single-module project's output.

### 4. What each file contains

| Shared definition | `<stem>.hpp` | `<stem>.cpp` |
| --- | --- | --- |
| function | declaration | definition |
| struct, enum | full definition; generated operators as hidden friends | nothing |
| print support of a shared enum (always generated) | declared | defined |
| variable | `extern T name;` | `T name = …;` |
| `constexpr` constant | `inline constexpr T name = …;` | nothing |
| `const` constant | `extern const T name;` | `const T name = …;` |
| non-shared anything | nothing | in an anonymous namespace |

**Include or forward-declare.** A header includes another module's header
when a header part needs a complete type or a definition from it: a default
argument, a `constexpr` constant, a by-value field, a field with a default
value, a field of a comparable or printable struct, or a field whose type is
not built from `std::vector` alone. A type named only in a function
declaration, an `extern` declaration or a `std::vector` field without a
default value in a struct without ticks gets a forward declaration. A
`.cpp` includes its own header, the headers whose shared definitions it
uses and, transitively, the headers those forward-declare from, so every
type it touches is complete. Catalog headers follow where code is printed:
an item's `decl` headers (ADR-0017) go with its declaration, its `body`
headers with its definition.

**Invariant.** A header depends only on its own module's shared definitions
and on the names, kinds and modules of the other modules' types and
constants they use. Editing a function body, an initialiser or anything not
shared never changes a header.

**Helpers** go to `b2c_support.hpp` when the project has more than one
module and some file uses one. Codegen decides it from the module count and
its own usage, so the build and the preview cannot differ.

### 5. Order, cycles and initialisation

* From W2, `b2c-lang` builds one **declaration graph** over the program
  (06 §6.7 step 3). A global depends on everything its initialiser uses,
  followed through the bodies of the functions it calls, the default
  arguments of those calls and the default member initialisers of the types
  it builds. Each file prints its declarations in one topological order of
  the graph, shared and non-shared together; the `.cpp` closes and reopens
  its anonymous namespace where the visibility changes, as 06 §6.7 already
  does around raw blocks. A cycle is `E0420` (by value) or `E0421`, as
  06 §6.7 specifies (not yet implemented; W2 with structs).
* The modules whose headers include each other form no cycle. A cycle is
  `E0422`, on each shared definition that closes it, with the chain as
  related locations.
* C++ does not order initialisation across translation units. A global
  whose initialiser, followed as above, uses another module's global is
  `E0423`, unless that global is a `constexpr` constant or a `const`
  constant whose value L10 proves constant, which C++ sets before any code
  runs. Reads in `main` are never reported.

### 6. `main`, namespaces and `using namespace`

One enabled *when program starts* in the project, in any module (`E0405`,
`E0406` unchanged). Its `int main()` is defined last, at global scope, in
that module's `.cpp`, never in a header. Modules are not C++ namespaces: a
module name appears only in comments, file names and `#include` lines, and
namespace blocks with one name in several modules are one C++ namespace.
ADR-0006 stands: no directive in a header (`E0514`), file directives open
the anonymous namespace, and a `use namespace` naming a namespace that
another module's header declares adds that header.

### 7. Building

* `gen/` holds exactly the generated files: the build deletes regular files
  with generated names that it did not write, never through a link and only
  in its build folder. `b2c generate` never deletes anything.
* A single-module build is unchanged. A multi-module build compiles each
  unit on its own and links, with objects passed relative to the compiler's
  working directory so that 257 units fit Windows' command line.
* The object cache of 07 §7.5.1 is if-time (*owner to confirm*): M3-D6's
  must-have set names multi-module projects, not their cache. When built,
  it replays a reused unit's warnings from a size-bounded file, writes
  objects by rename, and keeps them through a failed or cancelled build.

### 8. Diagnostics across files

Header code maps to its blocks like any other code. An unmapped place in a
file attaches to the file's module, and in `b2c_support.hpp` to the
project. A header message that several units report is kept once, from the
first unit. Include chains are not merged into related locations, because
`#include` lines have no source-map range; the raw output keeps them.

### 9. Editor and export

* Modules can be added, renamed and deleted (M3). Renaming changes only file
  names and `#include` lines, since blocks refer to symbol IDs. The C++ tab
  opens the `.cpp` whose `module` key is the module on the canvas.
* `use header` names standard headers only. An explicit
  `use header of module [utils]` block (if-time) stores the module ID as a
  plain string, checked by the resolve stage, so the format does not change.
* Export (W3) writes `src/`, `CMakeLists.txt`, a `Makefile`, `README.md` and
  `.b2c-export.json` into a folder named by a slug of the project name that
  always passes `FileStem`; no other user text reaches the build scripts.
* The manifest is untrusted: it may list only the export's own file names,
  each with the SHA-256 of what was written. *Replace* refuses a bad
  manifest, a linked `src/`, a listed file changed since the export or an
  unlisted file in the way, and otherwise touches only listed files whose
  content still matches.

### 10. Formats, versions and codes

No format change: no new key, and `formatVersion` stays (ADR-0013).
`SHARED` and the `module` field kind ship in catalog 1.3.0 with ADR-0017's
and ADR-0018's changes. The source map stays version 1, with an optional
`module` key, until M3-5's compact form, which must carry it. `IPC_VERSION`
does not change. Codes, from the allocation ADR-0016 to ADR-0019 share:
`E0206` (new text), `E0213`, `E0214`, `E0221`, `E0422` and `E0423` in W3;
`E0420` and `E0421` in W2.

## Consequences

**Where the work lands** ([plan](../plans/m3-contracts/modules.md#11-work-plan)).
W1 adds the IR types and IPC keys with defaults, changing no output. W2
builds the declaration graph. W3 brings functions-only multi-module projects
right after W1, and the rest (types, globals, `E0422`, `E0423`, export) after
W2's definition blocks. W4 adds the *Share* quick fixes.

**What gets harder.** There are two kinds of definition, and a function
nobody uses yet draws a g++ warning in a multi-module project but not in a
single-module one. Using one definition of another module includes its whole
header, so a private name can start to clash (`E0213`). `E0422` asks the
user to move a definition between modules. The analyser computes the layout
before generation, which 06 §6.1 placed in stage ⑥. The initialisation walk
is conservative: a function that uses a global on any path counts.

**Compatibility.** An M2 project with several modules opens unchanged, but
its generated C++ changes: every module's functions move into an anonymous
namespace, so a function nothing calls draws `-Wunused-function`, which
fails the build under the project's `warningsAsErrors` setting. The M3
release notes say so. A single-module project's output changes only when
its module name starts with `b2c`.

**Performance.** Headers add little text to each preview, which ADR-0016's
worker carries. Without the cache, a multi-module build recompiles every
unit in parallel, as now.

**Security** ([08](../spec/08-security.md)). File names come only from
`FileStem`, and the `b2c` prefix is reserved. `gen/` pruning deletes only
regular files with generated names in the build folder. Project headers are
found only through `-iquote` and end in `.hpp`, so a module named `string`
cannot shadow `<string>`. Export writes no user text into its build
scripts, and Replace deletes only what an untrusted manifest proves the
export wrote. Cached message files are size-bounded and parsed strictly.
M3's threat-model review (W6) records these, with the manifest as a row.

## Tests

The full list is in the
[contracts](../plans/m3-contracts/modules.md#12-tests). In short: unit tests
for `FileStem`, the declaration graph and its order, each include rule, each
new code and the precedence rule; codegen snapshots for each row of the
table and for the two order programs found in review; property tests that
generation is deterministic, header includes are acyclic, a body edit never
changes a header and each header compiles alone; the golden examples
unchanged plus a three-module `shapes.b2c`; build tests for pruning,
de-duplication and, with the cache, reuse; export builds with CMake and
Make, with a security corpus for Replace; E2E for adding, sharing, renaming
and deleting a module.

## Owner to confirm

1. New definitions start unshared; using one from another module needs the
   tick or the *Share* quick fix.
2. Includes follow use. No *use module* block is required, and the explicit
   `use header of module` block is if-time.
3. `#pragma once`, not `#ifndef` guards.
4. The object cache is if-time; the alternative makes it must-have.
5. A module name that starts with `b2c` is an analyser error in every
   project, so the file still opens, rather than a load error.
6. A module's own name hides another module's shared name, rather than
   `E0213` across the whole project.
