# M3 contracts: W3, multi-module projects and namespaces

What the second part of wave W3 builds ([next steps §3.4 and
§3.8](../next-steps.md#34-waves)): multi-module projects with headers (L18,
[ADR-0019](../../adr/0019-multi-module-header-split.md)) in two parts,
functions across modules first, then structs, enums, globals and constants
across modules; the project name on the project settings page; and, if time
allows, the *namespace* and *use namespace* blocks with the *Textbook style*
setting (moved from W2, [w2.md §1.10](w2.md#110-not-in-w2)) and the
per-unit object cache. It builds on [modules.md](modules.md) (the Rust
contract, the file layouts, the include rule, the declaration graph, the
cache, the codes), [types.md](types.md) (types, headers per item part, the
code and version tables) and [ADR-0006](../../adr/0006-using-namespace.md)
with [06 §6.14](../../spec/06-compiler-pipeline.md#614-standard-names-and-using-namespace)
(the lookup model behind `using namespace`). It follows the shape of
[w2.md](w2.md). If this file and an ADR disagree, the ADR wins; if this file
and modules.md disagree, this file is the newer and §5.4 lists the change
the docs step makes in modules.md.

**Status (2026-10-28): reviewed, revised; owner decisions open (§6).**
Written against `8a3d8ec`. Three reviews (compiler and build; scope,
packaging and order; students and safety) and what became of each finding
are in §7. W3M-D10 to W3M-D13 are new with the review and must be answered
before R1 (§3.4).

The owner's decisions it follows: M3-D6 (must-have is the foundations, the
assignments' blocks, multi-module and run options; the rest is if-time and
moves to M4 without sign-off when the budget is spent); the six points of
ADR-0019 *Owner to confirm*, all accepted with the M3 plan on 2026-10-24
(next steps §3.3 and §3.7): new definitions start unshared, includes follow
use, `#pragma once`, the object cache if-time, a `b2c` module name is an
analyser error, and a module's own name hides another module's shared name
(point 6, which C++ allows only in part: §2.1, W3M-D10).

## 0. Where the code stands at `8a3d8ec`

Written down before the design, with the place that shows it. *Works*
means tested and in use; *ready* means built but not used yet.

| What | State | Evidence |
| --- | --- | --- |
| The W1 contract of modules.md §1: `FileStem`, `is_device_name`, `Item::shared`, `Layout` and `ModuleFiles`, `GeneratedFile::module`, `FileMap::module`, `SymbolInfo::shared` | ready, every field at its default | `crates/b2c-ir/src/text.rs:870-923`, `layout.rs:17-79`, `sast.rs:116`, `scope_info.rs:62` |
| The analyser fills `Layout::flat` (one `.cpp` per module, no header, stems from `FileStem` with the `module<N>` fallback) | works | `crates/b2c-lang/src/lower/mod.rs:108-115` |
| The declaration graph orders each module's types, globals and function declarations into `ModuleFiles::source_order`, with `E0420` and `E0421` | works, **per module only**: one graph per module, no edge between modules | `crates/b2c-lang/src/order.rs:504`, `:839-853` |
| The *initialised* edges of a global are walked once per global (not the per-function summary of modules.md §4.1) | works, linear per global, quadratic in the worst case over a module | w2.md §7.1 (C2 row), `order.rs:714` |
| Code generation prints `source_order` | works | `crates/b2c-codegen/src/lower/mod.rs:96-104` |
| Code generation names files itself (`source_path`, module ID fallback) instead of using `ModuleFiles::stem`, and leaves `module` at `None` | to change | `crates/b2c-codegen/src/lib.rs:138-150`, `:205-221` |
| An enum's `b2c::to_text` is printed as part of the enum's definition text, `inline`, in a `namespace b2c { … }` block right after it | works for one unit; inside an anonymous namespace it would make every `b2c::` ambiguous (§1.7) | `crates/b2c-codegen/src/lower/typedefs.rs:372-418`; expected text in `lower/tests.rs:520-575` |
| A constant is `constexpr` exactly when it is a `bool`, `char`, `int` or `double` whose value L10 proves constant; there is no user choice | works | `crates/b2c-lang/src/lower/globals.rs:283-310`; `crates/b2c-codegen/src/lower/globals.rs:49-53`; w2.md §2.7 |
| `b2c_support.hpp` behind `HelperPlacement::Header` | ready, tested by one snapshot (`two_modules_share_the_support_header`), used by no caller: the build, the preview and the benchmark all pass `Inline` | `crates/b2c-codegen/tests/it/snapshots.rs:954-974`; `crates/b2c-build/src/frontend.rs:77`; `crates/b2c-core-wasm/src/facade.rs:296` |
| Each module is analysed with its own function and global tables | works | `crates/b2c-lang/src/lower/mod.rs:386-399` |
| A call to, or a global of, another module is `E0206` (*"isn't supported yet"*); so is a type naming another module's struct or enum | works (M2 rule) | `crates/b2c-lang/src/lower/resolve.rs:277-297`, `lower/user_types.rs:127-150`; tests `crates/b2c-lang/tests/names.rs:190` |
| Two functions of different modules with one name are `E0211`; a global's name is unique in the whole program (`E0210`) | works (M2 rule, kept by W2) | `crates/b2c-lang/src/lower/mod.rs:788-812`; `names.rs:408`; w2.md §7.1 (C2 rows) |
| Definition blocks are top-level only: the catalog refuses a definition in a statement or value position (`E0604`), and a block stacked under a top-level block is a loose stack; the analyser takes definitions to be exactly the top-level blocks | works | `crates/b2c-catalog/src/schema.rs:170-195`, `resolve.rs:310`, `stack.rs:1`; `crates/b2c-lang/src/collect.rs:96`, `:151`; `crates/b2c-lang/src/query.rs:49`, `:530-540` |
| A `SHARED` checkbox on `type.struct`, `type.enum` and `var.global`, hidden in the editor and read only for damage | ready | `catalog/core/types.toml:49`, `:154`; `catalog/core/variables.toml:78`; `packages/blockly-ext/src/blocks/layout.ts:20-36`, `:119-128` |
| `SHARED` on `func.define` | missing | `catalog/core/functions.toml:3-60` |
| The build compiles several units one by one, at most 8 at once, then links; progress per unit | works | `crates/b2c-build/src/compile.rs:172`, `:898-937`; test `several_translation_units_compile_then_link` (`crates/b2c-build/tests/build_sessions.rs:358-400`) |
| Objects are written as absolute paths `out/<stem>.o` | works, but 257 absolute paths in a deep cache folder can pass Windows' 32,767-character limit | `compile.rs:918` |
| On a failed or cancelled build, `remove_outputs` deletes each step's `-o` argument as written, relative to the Blocks2Cpp process's own working folder; the compiler runs in `diag/` | works only while the paths are absolute | `compile.rs:832-845`, `:1287`; `crates/b2c-toolchain/src/command.rs:402-408` |
| `-iquote <build>/gen` is on every compile | works | `crates/b2c-toolchain/src/command.rs:315` |
| `gen/` holds exactly the generated files | **no**: `write_generated_tracked` never deletes, so a renamed module's old `.cpp` stays (it is not compiled, since only written sources are); it rewrites a file only when its content changed | `crates/b2c-build/src/build_dir.rs:293-345` |
| An unmapped place in a generated file attaches to the module of the file's first range, so a file with no ranges (an empty `.cpp`, a header with only includes) gets none | to change | `compile.rs:1352-1363` |
| A header message reported by several units | reported once per unit (no de-duplication yet) | `compile.rs:1315-1333` |
| The object cache (`obj/`) | missing | 07 §7.5.1 (M3) |
| Export writes every generated file into `src/`, lists the `.cpp` files in `add_executable` and the `Makefile`'s `SOURCES`; the `Makefile` compiles and links every source in **one** g++ command (`$(OUT): $(SOURCES) $(HEADERS)`, no object per file); manifest format 2 with the project's ID and the file names only; folder named by `export::folder_name`; CMake target `b2c_program` | works for several sources; no test exports a project with a header | `crates/b2c-build/src/export.rs:52-70`, `:221-247`, `:447`, `:566-577`, `:672-673`; w3-export-run.md §4 |
| Export's *Replace* deletes every file the earlier manifest lists and this export does not write, after only a link and regular-file check: no content check | works | `export.rs:819-830`, `:847-852` |
| The export test builds a fixed list of four single-module examples, and skips its `make` part when `make` is missing | works | `crates/b2c-build/tests/export.rs:20`, `:74-77` |
| The Windows CI job installs only `mingw-w64-ucrt-x86_64-gcc` and puts only `ucrt64\bin` on `PATH` | works | `.github/workflows/ci.yml:115-129` |
| The golden test compares the files generated (never lists the folder), builds the Debug configuration and fails on any warning | works | `crates/b2c-build/tests/golden.rs:101-135`; `tests/common/run_data.rs:232-253` |
| The program's name is the first module's name | works | `compile.rs:878-892` |
| The loader refuses a project with no modules or more than 256 (`E0120`) | works | `crates/b2c-model/src/limits.rs:10`; `crates/b2c-model/src/decode/project.rs:418-431` |
| The module switcher: one canvas per module, shown when there are two or more, labelled `<name>.cpp`, one tabbable toggle button per module; no add, rename or delete | works | `apps/desktop/src/editor/modules/ModuleSwitcher.tsx:1-55` |
| Showing another module reloads the one Blockly workspace and clears its undo history; while the canvas holds a change that cannot be read back (the banner), switching is refused | works | `apps/desktop/src/editor/sync/session.ts:417-460`, `:586-593`, `:630`; 04 §4.2 (*In M2* banner paragraph) |
| The C++ tab opens the module's `.cpp` by its position among the source files | works for `.cpp`-only output | `apps/desktop/src/app/panels.tsx:106-122` |
| The Problems panel has a Module column | works | `apps/desktop/src/panels/problems/problems.ts:16-38` |
| The type menus list only the shown module's structs and enums | works (W2 review) | w2.md §7.1, `apps/desktop/src/editor/services/symbols.ts:167-176` |
| The project is renamed from the top bar (`project.rename`, `checkProjectName`, at most 100 characters); the settings page passes the name through unchanged, calls `updateProjectInfo` only when `settingsChanged(…)` is true, and its no-editor branch spreads only the settings | works | `apps/desktop/src/app/layout/ProjectTitle.tsx:42-90`, `:232`; `apps/desktop/src/features/project-settings/ProjectSettingsPage.tsx:114-139` |
| Namespaces: no symbol kind, no block, no standard-name table (only `MACRO_NAMES` and `GLOBAL_NAMES`, from one compiler); standard names are always written in full | missing | `crates/b2c-ir/src/sast.rs:1104-1150`; `crates/b2c-ir/src/reserved_names.rs:2`, `:5`, `:1536`; `crates/b2c-codegen/src/cast.rs:26-28` |
| *Textbook style* (`options.usingNamespaceStd`) is reserved by ADR-0013 as a format-2 key but **refused by the loader** as unknown (`E0110`); `ProjectOptions` has no field and `required_format_version` does not count it | missing; tests pin the refusal | `crates/b2c-model/src/document.rs:102-118`; `version.rs:50-61`; `crates/b2c-model/tests/versions.rs:241-248` (`textbook_style_is_not_a_key_yet`), `tests/rules.rs:147-157`; 05 §5.3 |
| No limit on namespace depth exists; `MAX_QUALIFIED_NAME_LEN` (1,024) is sized for 14 levels | missing | `crates/b2c-model/src/limits.rs:22-25` |
| Paste gives pasted declarations fresh symbol IDs and binds names by M2's rule (no namespaces, no shared names) | works (M2 rule) | `crates/b2c-core-wasm/src/clipboard.rs:1-60`; 05 §5.12 |

**Checked again for this contract** (g++ 11, 13 and 14, `-std=c++17 -Wall
-Wextra -Wpedantic`, with `-Werror` where it says so), in addition to
modules.md §14; the checks marked *(review)* were made by the reviews (§7):

* A shared `const std::string kShop` declared `extern const` in the header
  and defined in its own `.cpp` links from another unit only when that
  `.cpp` includes its own header before the definition; without it the
  definition has internal linkage and the link fails (*undefined reference
  to `kShop`*).
* A shared enum in user namespace `geo` with `operator<<` declared in the
  header inside `geo` and `b2c::to_text(geo::Colour)` declared in the global
  `b2c` namespace, both defined in the `.cpp`, compiles and links with
  `-Werror` and prints `Green Red`.
* An overload set split across two modules' headers changes meaning with
  the includes: `show(1)` calls `show(double)` in a unit that includes only
  the first header and `show(int)` in one that includes both, silently (§2.1).
* Non-shared `geo::half` in `namespace geo { namespace { … } }` in two
  modules, a shared `geo::area`, and a file directive `using namespace std;
  using namespace geo;` inside the anonymous namespace: `::count` still
  reaches the module's own non-shared `count` next to `std::count`, and the
  program prints `6 3 2`.
* Objects compiled and linked with paths relative to a `diag/` working
  directory (`../out/main.o`) link and run.
* An unused function in an anonymous namespace is an error under
  `-Werror` (`-Wunused-function`), as modules.md §14 found.
* *(review)* `namespace { enum class C {A}; namespace b2c { inline
  std::string to_text(C); } }` in a unit that also sees `::b2c` (from
  `b2c_support.hpp` or another header): every `b2c::…` is *ambiguous*.
* *(review)* A header's `double scale(double, int);` next to the unit's own
  `namespace { double scale(double v, int d) {…} }`: `scale(1.0, 2)` is
  *ambiguous*, and `::scale(1.0, 2)` silently calls the header's function.
* *(review)* A header declaring `std::string to_text(Grade);` with the
  `.cpp` defining it `inline` fails the link from another unit; a hidden
  friend that prints an enum field before that enum's `operator<<` is
  declared does not compile.
* *(review)* `namespace { using namespace geo; }` before any `namespace geo`
  is *not a namespace-name*; with an empty `namespace geo {}` first it
  compiles under `-Werror`.
* *(review)* A header opening `namespace geo` next to the unit's own
  `namespace { int geo(int x) {…} }`: `geo(1)` is *ambiguous*.

## 1. Scope

### 1.1 How to read the tables

* **Item**: the piece of work, with *must* (M3-D6's must-have set: the
  multi-module projects and the project name) or *if n* (if-time, in the
  order of n: N first, then the cache C, then the sweep S; §6 W3M-D3).
* **What a student sees**: in the editor and in the C++ tab.
* **Files**: what the generator writes; §1.7 has the full table of what
  goes in a header and what in a `.cpp`.
* **Codes**: as in §1.9.
* **Package**: as in §3.2.

Every rule of ADR-0019 and modules.md §2 to §7 applies; this file names
the package that builds each one and adds what they leave open.

### 1.2 Functions across modules (part 1, must)

| Item | What a student sees | Files | Codes | Package |
| --- | --- | --- | --- | --- |
| Add, rename and delete modules | The module strip above the canvas (shown from one module on, not two as now) with ⊕ *Add module* and one *Module actions* menu for the shown module: *Rename…*, *Delete…*, *Move left*, *Move right* (the moves are an addition to ADR-0019 §9, §6 W3M-D9). Dialogs check the name as the loader and `E0221` do (§2.10). Deleting a module with blocks asks first and can be undone with *Undo delete* (§6 W3M-D6). ⊕ is disabled at 256 modules (*A project can have at most 256 modules*); *Delete…* is disabled on the only module (*A project needs at least one module*); deleting the module that holds *when program starts* says so in its dialog (the program then has no start, `E0405`). Every module edit is refused, with the banner's text, while the canvas holds a change that cannot be read back | each module its `<stem>.cpp` (an empty module too, with its header comment); the first module names the program. Adding the second module changes the first module's `main.cpp` (§2.13) | loader rules; `E0221` | E |
| *share with other modules* on *define* | A checkbox at the end of `func.define`, as on `type.struct`; shown only in a multi-module project, kept when hidden | a shared function's declaration (with its default values) in `<stem>.hpp`, its definition in `<stem>.cpp` | — | P0 (catalog), E (shown) |
| Calling another module's shared function | *Your functions* lists the module's own functions, then each other module's shared functions not hidden by an own name, under that module's heading; the call block looks as any call | `#include "utils.hpp"` in the calling module's `.cpp` | `E0206` for a non-shared one, new text and quick fix *Share* (W4) | F |
| Non-shared functions | Same-named helpers in two modules are fine | in an anonymous namespace in their `.cpp` | `E0213` when a non-shared name meets a shared name of a header its `.cpp` includes, for whatever reason (§2.1, W3M-D10) | F |
| A shared function whose inputs or result name something not shared | The *define* block's error | — | `E0214` (signature, default values) | F |
| `main` in any module | *when program starts* may be in any module | `int main()` last, at global scope, in that module's `.cpp` | `E0405`, `E0406` unchanged | F |
| `b2c_support.hpp` | The C++ tab lists it last | written when the project has two or more modules and some file uses a helper; single-module projects keep the inline section | — | P0 |
| Module names starting with `b2c` | `E0221` on the module; the file opens and can be fixed; the strip and the C++ tab both show the file as `module<N>.cpp`, the strip with an error mark and the module's name in its accessible name and tooltip | — | `E0221` (every project) | P0, E (strip) |
| The C++ tab | Its file picker lists the shown module's files first, then the other modules' files under their module names in module order, then `b2c_support.hpp`; opens the shown module's `.cpp`; clicking a header line selects its block, switching module | — | — | E |
| Building | Same Build and Run buttons; progress counts units | each unit compiled on its own, then linked | — | B |

### 1.3 Structs, enums, globals and constants across modules (part 2, must)

| Item | What a student sees | Files | Codes | Package |
| --- | --- | --- | --- | --- |
| Shared struct | The struct's *share with other modules* tick is shown (T turns it on); other modules' type menus offer it, under its module's name once W4's type field or picker groups by module (§3.3) | full definition in the header (generated operators as hidden friends) | `E0214` for a field type or default value naming something not shared; `E0420` across modules | T |
| Shared enum | As for structs | full definition in the header, followed at once by its `operator<<` (in its namespace) and the `b2c::to_text` declaration (at global scope), always; both defined, not `inline`, in the `.cpp` | `E0214` | T |
| Shared global variable | *global variable … share with other modules* | `extern T name;` in the header, `T name = …;` in the `.cpp` | `E0214` for a type naming something not shared; `E0423` when another module's global is set from it before the program starts | T |
| Shared constant | *global constant … share* | `inline constexpr T name = …;` in the header when L10 proves the value constant (w2.md §2.7) **and** the value names nothing non-shared; otherwise `extern const T name;` in the header and `const T name = …;` in the `.cpp`. Which form it takes is part of its header (§2.4) | another module may use it as a `case` label or a `check at compile time` only when it is `constexpr` (`E0424` otherwise); `E0214` only for its type | T |
| Include or forward-declare | Nothing to do: the generator decides | modules.md §3.3's rules, widened in §2.4; a forward declaration where it is enough | `E0422` for headers that would include each other, when no cycle of definitions explains it (§2.2) | T |
| One declaration order across the program | Moving blocks never changes the code | the declaration graph over all modules (§2.2) | `E0420`, `E0421` across modules | T |
| Duplicates | Two shared definitions with one name in one namespace are refused; non-shared ones of different modules are fine | — | `E0210`, `E0211` | F (functions), T (the rest) |

### 1.4 Namespaces (if 1; §6 W3M-D3)

| Item | What a student sees | Files | Codes | Package | Set |
| --- | --- | --- | --- | --- | --- |
| `program.namespace` *namespace %NAME* with a mouth for definitions | A definition block (Program category, after *use header*) holding *define*, *struct*, *enum*, *global*, and other *namespace* blocks, at most 14 deep; its definitions are named `geo::area` in other blocks' menus and labels (C++ mode) | `namespace geo { … }`, closed and reopened as the declaration order needs; non-shared members in `namespace geo { namespace { … } }`; the same namespace in several modules is one C++ namespace | `E0210` (a name taken in that namespace; a namespace named like any definition, §2.1), `E0213`, `E0215` (only with a directive), `E0219` (deeper than 14) | N | if 1 |
| `program.use_namespace` *use namespace %NS in this file* | A definition block on the canvas; the menu offers `std` and the program's top-level namespaces | `using namespace std;` after the includes and helpers (single module) or as the first lines of an anonymous-namespace block (multi-module), after an empty declaration of a user namespace no included header declares; never in a header | `I0523` (already in use), `E0201`/`E0207` (a namespace that is gone or not top-level), `E0203` (not visible), `E0215` | N | if 1 |
| *Textbook style* | A switch on the project settings page (*Language*: *Write `using namespace std;`*). In M3 it applies as one project edit with no preview; the problems it causes (`E0215`, `E0213`) appear in Problems, and turning it off takes them away (§6 W3M-D13) | `using namespace std;` in every `.cpp` that includes a standard header | `E0215`, `E0213` | N | if 1 |
| The lookup model, M3 subset | Code reads like a textbook where lookup allows; `::max(…)` or `std::count` where it does not; hovers explain (the hover itself is W4's) | spellings recorded in the SAST, printed by codegen | — | N | if 1 |
| `use namespace %NS here` | A statement block | `using namespace N;` at its place | `I0523` | S | if 3 |
| The standard sub-namespaces | `std::chrono`, `std::this_thread`, `std::filesystem`; from C++20 `std::numbers`, `std::ranges`, `std::views` in the menu | their header added | `E0703` before C++20 | S | if 3 |

### 1.5 The project name on the project settings page (must)

A *Project* section at the top of the page with one text field, *Name*,
checked as the top bar's rename is (`checkProjectName`, at most 100
characters, not empty after trimming), its problem shown under the field.
*Save changes* applies the name with the other settings as one edit
(`updateProjectInfo({ name, settings })`), so the top bar shows the new name
and `•`. The changed check counts the name, both save paths (with and
without an editor) carry it, and the name is written only when the field
was edited, so a top-bar rename made while the page was open is kept. The
top bar's *Rename project…* stays. The name reaches files only through the
comment encoder (header comments), the export's folder slug and the export
README's title (escaped by `markdown_text`), all as today. Package E.

### 1.6 The per-unit object cache (if 2, §6 D4)

modules.md §5.3 as written: key, `obj/<stem>-<key12>.o` with a
`.messages.json` beside it, reuse with replayed warnings, kept through a
failed or cancelled build, pruned after a successful link. Package C.

### 1.7 The generated files

Single-module projects generate exactly what they do today (the golden
files of the 24 examples and the ten assignments do not change by a
byte), except a module whose name starts with `b2c` (`E0221`). In a
multi-module project, for each module in module order, `<stem>.cpp`, then
`<stem>.hpp` when the module shares something, and last
`b2c_support.hpp` when some file uses a helper. Adding a second module
therefore changes the first module's `main.cpp` (§2.13).

| Shared definition | `<stem>.hpp` | `<stem>.cpp` |
| --- | --- | --- |
| function | declaration, with its default values | definition (no default values) |
| struct | full definition, with hidden-friend operators | nothing |
| enum | full definition, followed at once (one graph node) by `operator<<`'s declaration in the enum's namespace and, at global scope, `namespace b2c { std::string to_text(E); }`; always, so the header never depends on whether something prints it | `operator<<` and `b2c::to_text` defined, **not** `inline` |
| global variable | `extern T name;` | `T name = …;` (or `T name{};`) |
| `constexpr` constant | `inline constexpr T name = …;` | nothing |
| `const` constant | `extern const T name;` | `const T name = …;` |
| namespace member | the above inside `namespace geo { … }` | the above inside `namespace geo { … }` |
| non-shared anything | nothing | in an anonymous namespace (`namespace geo { namespace { … } }` for a namespace member) |

* **`b2c::to_text` is always at global scope.** For an enum in an anonymous
  namespace, a user namespace or both, the enclosing namespaces are closed
  before `namespace b2c { … }` and reopened after it, and the enum is named
  as the lookup model needs (`Colour`, `geo::Colour`). The enum's
  `operator<<` stays beside the enum, so argument-dependent lookup finds
  it. A non-shared enum's `b2c::to_text` stays `inline`, as today; a
  shared enum's is declared in the header and defined in the `.cpp`
  without `inline`. Single-module output does not change.
* **Include guard.** `#pragma once` (ADR-0019 owner point 3), as
  `b2c_support.hpp` already has. No `#ifndef` macro is derived from a
  module name.
* **`<stem>.hpp`, in order:** the header comment; `#pragma once`; the
  standard headers its header parts need (sorted); the project headers it
  includes (sorted by file name); `#include "b2c_support.hpp"` only when a
  header part calls a helper; forward declarations in their namespaces;
  the shared declarations in `header_order` inside their namespace blocks.
  Never a directive.
* **`<stem>.cpp`, in order:** the header comment; its own standard headers
  (also when an included header has them, 06 §6.10); the project headers it
  includes, sorted by file name, its own always among them when the module
  has one; `#include "b2c_support.hpp"` when it uses a helper; each user
  namespace that a directive names and no included header declares,
  declared empty (`namespace geo {}`), sorted by name (06 §6.14.3 rule 2);
  the directives' anonymous-namespace block (§2.12); step 3 in
  `source_order`, the anonymous namespace closed and reopened where
  visibility changes; step 4, the non-shared function definitions, then the
  shared ones; `int main()` last in its module.
* A `.cpp` always includes its own header, before its definitions: without
  it a `const` constant's definition has internal linkage and does not link
  (§0).

Example, part 1 (`unit_converter`, §4.1), the `convert` module:

```cpp
// convert.hpp
#pragma once

double rounded(double value, int digits = 1);
double to_celsius(double fahrenheit);
double to_fahrenheit(double celsius);
```

```cpp
// convert.cpp
#include <cmath>

#include "convert.hpp"

namespace {

double scale(double value, int digits);

}  // namespace

namespace {

double scale(double value, int digits) {
    return std::pow(10.0, digits) * value;
}

}  // namespace

double rounded(double value, int digits) {
    return std::round(scale(value, digits)) / std::pow(10.0, digits);
}

double to_celsius(double fahrenheit) {
    return (fahrenheit - 32) / 1.8;
}

double to_fahrenheit(double celsius) {
    return celsius * 1.8 + 32;
}
```

The exact text (blank lines, the closing comment) is the snapshot P0
records; the shape is binding.

### 1.8 Not in W3

* **To W4** (already in its row of next steps §3.4, or added by this
  file's docs step): the *Share* quick fixes; the friendly messages for
  `-Wunused-function` and `-Wunused-variable` in a multi-module `.cpp`;
  *Move to module* (if-time; §6 W3M-D12 for what M3 offers meanwhile);
  hovers explaining `::` and kept `std::` (06 §6.14.12); typed names in
  slots and `E0216` (06 §6.14.11), with Quick Insert; grouping other
  modules' types under module headings in the type field or picker (§3.3).
* **To M4:** classes in headers (out-of-line members, templates), Raw C++
  in headers and every Raw C++ rule of 06 §6.14.9 (`E0217`, `E0218`,
  `W0525`), library packs and their namespaces (§6.14.10), type aliases,
  `std::array` and `std::list` in headers, `E0514` (it needs bodies printed
  into headers, which only templates and classes have), the `I0524` rename
  offer; Textbook style's preview and *Rename all and turn on* (06
  §6.14.12; W3M-D13); the background probe of a toolchain newer than the
  standard-name table (06 §6.14.8): in M3 a toolchain newer than the
  table's newest GCC counts as an incomplete table, so nothing is shortened.
* **Not planned for M3:** nested namespaces nominated by *use namespace*
  (06 §6.14.3 rule 6 forbids it), namespace aliases, `using` declarations,
  inline namespaces, C++20 modules (`import`), per-module compiler flags,
  precompiled headers, a project-wide header, a header for a single-module
  project.
* **Format:** no new key name; this wave makes the reserved format-2 key
  `options.usingNamespaceStd` readable and writable (§2.9).

### 1.9 Diagnostics

New codes continue each group from types.md §6's next free code (`E0141`,
`E0219`, `E0222`, `E0429`; `W0509`); this wave takes `E0219` (namespace
depth) and needs no other beyond those modules.md §9 and 06 §6.14
allocated. P0 adds every constant to `crates/b2c-lang/src/codes.rs` under
`#[cfg_attr(not(test), allow(dead_code, reason = "emitted by W3's …"))]`, as
W2's P0 did; each package writes the *Message* lines of the codes it
emits in `docs/reference/diagnostics/analyser.md` (`E0703` in
`generator.md`, beside `E0701` and `E0702`); the integration step removes
the `allow`.

| Code | New or reused | When | Package |
| --- | --- | --- | --- |
| `B2C-E0206` | reused, new text | a reference to another module's definition that it does not share: *"`helper` belongs to module `utils`, which does not share it."* (functions in F; types, globals, constants in T) | F, T |
| `B2C-E0210` | reused | two shared definitions with one name in one namespace of the project; a user namespace with the name of any definition, shared or not, of any module in the same enclosing namespace (program-wide, so it never depends on includes); non-shared definitions of different modules no longer clash | F, T, N |
| `B2C-E0211` | reused, new texts | two shared functions with one name in different modules (§2.1: an overload set lives in one module); the M2 cross-module text goes | F |
| `B2C-E0213` | first emitted | a non-shared name that is also a shared name of its own module or of a module whose header its `.cpp` includes, directly or through other headers, for whatever reason the header is included (06 §6.14.7); for namespace members, the namespace's shared members so reached. The message names the cause (§2.1). If the owner chooses W3M-D10's alternative, any module's shared name in that namespace | F (functions), T (types, globals), N (namespace members) |
| `B2C-E0214` | first emitted | a shared definition's header part names a non-shared definition of its module: *"`area` is shared, but its input `box` has the type `Box`, which is not. Share `Box` too."* A shared constant's value never draws it (§1.3) | F (signatures, default values), T (fields, field defaults, constant and variable types) |
| `B2C-E0215` | first emitted | a namespace-scope type, or a user namespace, whose name a nominated namespace declares where it is referred to (`list`, `pair`, `chrono`), using only the names of the project's standard | N |
| `B2C-E0219` | new | a *namespace* block nested more than 14 deep (`MAX_NAMESPACE_DEPTH`, §2.12) | N |
| `B2C-E0221` | first emitted | a module name that starts with `b2c`, in every project | P0 |
| `B2C-E0420` | reused | cycles of by-value types, now across modules | T |
| `B2C-E0421` | reused | every other cycle of the declaration graph between definitions, now across modules, including default values that call each other (§2.2) | F (functions), T |
| `B2C-E0422` | first emitted | modules whose headers would include each other when no cycle of definitions explains it, on each shared definition that closes the cycle, with the chain as related locations | F (default values calling across modules), T (types and constants) |
| `B2C-E0423` | first emitted | a global set before the program starts from another module's global that is not a constant C++ sets before any code runs | T |
| `B2C-E0424` | reused | another module's `const` constant (not `constexpr`) as a `case` label or in *check at compile time* | T |
| `B2C-I0523` | first emitted | a *use namespace* block that another source already covers | N |
| `B2C-E0703` | first emitted (allocated by 06 §6.4 and §6.14.3) | a C++20 standard sub-namespace in a C++17 project | S |
| `B2C-E0201`, `E0207` | reused | *use namespace* naming a namespace that is gone, or a nested one | N |
| `B2C-E0203` | reused | *use namespace* naming a namespace the file cannot see (06 §6.14.3 rule 6) | N |
| `B2C-E0607` | reused | *use header of module* naming a module ID the document does not have | S |

### 1.10 Out-of-date text this wave removes

The *In M2* paragraphs of 03 §3.8 item 1 and §3.9, of 06 §6.7 and of
`crates/b2c-core-wasm/src/clipboard.rs` (its module documentation is W5's
file; §3.3), the *isn't supported yet* texts of `E0206` and `E0211`, and
the stale comment in `catalog/toolbox.toml:236` (*"is empty until its
package adds its blocks"*, next steps §3.8; P0).

## 2. Cross-cutting rules

### 2.1 Name resolution across modules

* **Visible.** A module sees its own definitions, shared or not, and every
  other module's shared definitions, in their namespaces, in any order
  (modules.md §2). The scope query lists them; the parked scope too.
* **Precedence.** A name resolves first to the module's own definition;
  another module's shared definition with that name is visible only when the
  module has none (ADR-0019 owner point 6). Blocks refer to symbol IDs, so
  precedence matters only for typed names, paste and the menus, which use
  this one rule. In C++ the hiding holds only while the module's `.cpp`
  does not include the other header (next bullet).
* **Qualified names.** A definition inside a namespace is `geo::area` in
  labels in C++ mode, in paste payloads and in the C++; a global one is
  `area`, written `::area` only where the lookup model (§2.12) needs it.
  Modules never appear in a C++ name: a module name is only in comments,
  file names and `#include` lines.
* **Collisions.**
  * Shared definitions are unique per namespace in the project (`E0210`),
    functions included (`E0211`): **an overload set lives in one module**.
    Two shared functions with one name in different modules are `E0211`
    (*"`show` is already shared by module `report`. Overloads of a shared
    function must be in one module, so rename this one."*; it says nothing
    of moving until a move that keeps callers bound exists, W3M-D12).
    Reason: a call's meaning would depend on which headers the calling file
    includes (§0's check), and a module's includes change when it starts
    using an unrelated definition of another module.
  * Non-shared definitions of different modules may share a name with each
    other.
  * A non-shared name may not be a shared name of its own module or of a
    module whose header its `.cpp` includes, directly or transitively
    (`E0213`). C++ cannot do otherwise: once `utils.hpp` is included,
    `scale(…)` is ambiguous between `::scale` and the anonymous namespace's
    `scale`, and `::scale` silently reaches the other module's (§0). Since
    includes follow use, `E0213` can appear on a definition the student did
    not touch, when the module starts using an unrelated shared definition
    of `utils` or of a module whose header `utils.hpp` includes. Its message
    therefore names the cause: *"`scale` is also shared by module
    `convert`, whose header `convert.hpp` this file includes because it
    uses `to_celsius`. Rename this `scale`."*, with related locations on
    that use, on the shared definition and, when transitive, on each
    include of the chain. Quick fixes (W4): rename, or *Share* when the
    shared one is in its own module. W3M-D10 asks the owner whether to
    apply `E0213` program-wide instead, which never depends on use.
  * Codegen never writes `::name` to reach an anonymous-namespace entity
    that an included header also declares (a guard with a test, F): the
    analyser has already reported `E0213` there.
  * A user namespace may not have the name of any definition of any module
    in the same enclosing namespace, shared or not (`E0210`, program-wide).
  * Globals: W2's program-wide uniqueness (`E0210`, w2.md §7.1) becomes the
    rules above.
* **Constant expressions.** In its own module a `const` constant with a
  constant value is a constant expression after its definition; another
  module sees only `extern const`, so there only a `constexpr` constant is
  (`E0424` otherwise). L10's `constant_expression` decides, together with
  §1.3's rule that a shared constant whose value names something
  non-shared is printed `extern const` (types.md §11.2).
* **Paste** binds `::area` to the target module's own `area` when it has
  one, otherwise to the one shared `area`; `geo::area` the same within
  `geo`. The code is W5's file (§3.3).

### 2.2 The declaration order across units

* T replaces the per-module graph by **one graph over the program**
  (`order.rs`): nodes keep their module, namespace and visibility; edges
  may cross modules. Each file's order is one topological order of the
  graph restricted to its declarations (modules.md §4.2), with the
  tie-break of 06 §6.7 and the visibility rule (same visibility as the last
  printed first, starting with non-shared). An enum's print support is part
  of the enum's node (§1.7).
* An edge to another module's definition is met by an include (complete
  or declared) or a forward declaration (forward), never by printing the
  definition again; it adds the module to `header_includes` or
  `source_includes` (§2.4).
* **One code per cycle.** A cycle between definitions is `E0420` (by-value
  types) or `E0421` (every other cycle, including two shared functions
  of different modules whose default values call each other), and only
  that. `E0422` is reported only for an include cycle between modules'
  headers that no cycle of definitions explains: for example, `a`'s
  shared function has a default value calling `b`'s `g`, while `b`'s shared
  function has one calling `a`'s `k`; or `a`'s struct holds `b`'s struct
  by value while `b`'s constant has `a`'s enum as its type.
* **Initialisation.** The *initialised* walk becomes the per-function
  summary over the call graph's strongly connected components (modules.md
  §4.1), computed once per program, so 256 modules with many globals stay
  linear in calls and uses. An *initialised* edge to another module's
  global is `E0423`, unless that global is a `constexpr` constant or a
  `const` constant of a type L10 evaluates whose value L10 proves
  constant (modules.md §4.4). Reads in `main` and in functions called only
  from `main` are never reported.
* **Order of definitions in a `.cpp`** never depends on canvas position or
  module order; reordering modules changes only the file order, the link
  order and the program's name.

### 2.3 One definition, `inline` and `extern`

* Non-shared definitions have internal linkage (anonymous namespace), so
  modules may reuse names and no definition can clash at link time.
* A shared function has one definition, in its module's `.cpp`; its default
  values are only in the header (C2's rule, w2.md §2.8).
* A shared struct or enum is defined once, in its header, which every unit
  includes through `#pragma once`; its generated operators are hidden
  friends, which are inline (a free `operator==` in a header fails the link,
  modules.md §14). A shared enum's `operator<<` and `b2c::to_text` are
  declared right after it in the header and defined once in the `.cpp`,
  not `inline` (an `inline` definition behind a non-inline declaration
  does not link from another unit, §0); declared right after it, so a
  printable struct's hidden friend that prints a field of that enum finds
  them.
* A shared `constexpr` constant is `inline constexpr` (C++17, the lowest
  standard a project takes), one entity in every unit.
* A shared `const` constant and a shared variable are `extern` in the
  header and defined in the module's `.cpp`, which includes its own header
  before the definition (§0).
* `b2c_support.hpp`'s helpers are `inline` functions or templates (already
  so, `crates/b2c-codegen/src/helpers.rs`); `random_int`'s engine is one
  across units, as an inline function's static is.
* `main` is never in a header, and only one enabled *when program starts*
  exists in the project (`E0405`, `E0406`).

### 2.4 Include order and cycles

* A header includes another module `n`'s header only when a header part
  needs a complete type or a declaration from it; otherwise it
  forward-declares (`struct Point;`, `enum class Colour;` in its
  namespace). modules.md §3.3's rules apply, with the field bullet widened
  to: **a default argument, a field's default value, or a constant's type
  or `constexpr` value that names or builds anything of `n`** (a type, an
  enumerator, a function, a constant or a variable), and a shared
  variable's type. A `.cpp` includes its own header, the headers whose
  shared definitions its code refers to, and, transitively, those its
  included headers forward-declare from, so every type it touches is
  complete.
* Project headers are sorted by file name after the standard headers;
  helpers last.
* The modules whose headers include each other form no cycle: a cycle that
  no cycle of definitions explains is `E0422` (§2.2) on each shared
  definition whose need closes it, with the chain as related locations and
  the advice to move one definition. A `.cpp` is never included, so `.cpp`
  includes cannot form a cycle.
* **A shared constant's form is part of its header.** Whether it is
  `inline constexpr` (value in the header) or `extern const` depends on
  L10's verdict and on what the value names (§1.3), with no user choice;
  an edit of its value that changes the form changes the header.
* **Invariant** (modules.md §3.6): editing a function body, a variable's
  starting value, the value of a shared constant printed `extern const`
  that stays so, or anything not shared never changes a header. A property
  test holds every package to it (§5.2), over edits that keep each shared
  constant's form.

### 2.5 How the preview shows several files

* The preview (W5's worker or the main thread) generates every file of the
  project on each update, as now; headers add a little text (ADR-0019,
  *Performance*). Nothing in `apps/desktop/src/editor/preview/` changes
  (W5's folder).
* The C++ tab (`apps/desktop/src/panels/code/`, `app/panels.tsx`) lists the
  files as §1.2 says, with the module's name, opens the `.cpp` whose
  `module` key is the shown module (replacing the position lookup of
  `panels.tsx:106-122`), and keeps a picked file per module, as
  `PickedFile` does now. A header the module does not have any more falls
  back to the module's `.cpp`. The `module` key comes from the source map,
  which M3-5's compact form must keep (§3.3).
* Clicking a header line selects its block: a function's declaration maps
  to its *define* block, a forward declaration to its type's defining block,
  switching the shown module when it is another module's.

### 2.6 Diagnostics across modules

* Analyser diagnostics already carry their module; the Problems panel's
  *Module* column shows it, and choosing a problem in another module shows
  that module, then reveals the block (`app/panels.tsx`, which calls the
  editor's reveal after setting `activeModuleId`).
* Compiler messages: header code maps like any other code; an unmapped
  place attaches to `FileMap::module` (`b2c_support.hpp` to the project); a
  header message reported by several units is kept once, from the first
  unit, by code, severity, file, line, column and text; the raw output
  keeps every unit's text (modules.md §6).
* Related locations of `E0213`, `E0214`, `E0422` and `E0423` may be in
  other modules; the problem's related list names the module of each.

### 2.7 The build

* **Single-module:** one compile-and-link invocation, unchanged.
* **Multi-module:** every unit compiled on its own (at most 8 at once),
  then linked. In the compiler's argv, objects are relative to its working
  directory (`diag/`): `../out/<stem>.o`, or `../obj/<stem>-<key12>.o` with
  the cache (§0's check). Each `Step` keeps the absolute path of its output,
  and everything that opens or deletes an output (`remove_outputs`,
  `output_of`, and with the cache C's temporaries, renames and pruning)
  uses that path, never the argv's text. 257 objects with 64-character
  stems keep g++'s link argv under about 26,000 characters (modules.md
  §5.2), which leaves room under Windows' 32,767 for what collect2 and ld
  add (CRT objects, `-L` paths, plugin options); the nightly 256-module
  Windows build (§5.3 item 8) is the proof, and a response file
  (`@objects.rsp`, which the driver, collect2 and ld accept) is the
  fallback if it fails.
* **`gen/` holds exactly the generated files:** after writing, the build
  deletes every regular file of `gen/` (by `symlink_metadata`, never through
  a link) whose name passes the generated-name rule and is not among this
  build's generated files (`GeneratedProject::files`), whether or not they
  were rewritten; a deletion counts as a change. `write_generated_files`,
  which `b2c generate` uses on a folder the user chose, never deletes.
* **Toolchain flags** do not change: `-iquote <build>/gen` finds project
  headers and only them (a module named `string` gives `string.hpp`, which
  cannot shadow `<string>`); every unit gets the same flags.
* **The cache** (if 2, C): as §1.6. Without it, every unit is compiled
  on every build that changed any generated file, in parallel, as now.
* **Limits** stay: 256 modules, so at most 513 generated files and 257
  units; per-unit time and memory limits.

### 2.8 Export

* Already right for several sources (§0): `src/<stem>.cpp`, `src/<stem>.hpp`
  and `src/b2c_support.hpp`; `add_executable(b2c_program …)` with the `.cpp`
  files in module order; the `Makefile` compiles and links every source in
  one g++ command that depends on every source and header. No user text
  reaches the scripts but the folder and program names, which come from
  `export::folder_name` and the first module's `FileStem`.
* That one command is about 75 characters per module: under the 32,767 of
  `CreateProcess` for 256 modules, but over cmd.exe's 8,191 for a project
  of more than about 100 modules with long names, and the `Makefile`'s
  header says it runs in cmd.exe. B keeps the one command and adds a
  README line, written only when the recipe line passes 8,000 characters,
  saying that the project needs the MSYS2 shell or CMake to build.
* **Replace keeps edited files.** The manifest (format 3) records each
  written file's SHA-256. *Replace* deletes or overwrites a file the
  earlier manifest lists only when its content still has the recorded hash;
  a file whose hash differs, or whose hash an older manifest does not
  record, is kept and named in the result (*kept because it was changed
  after the earlier export*), and the rest of the export goes on. This
  matters now that renaming or deleting a module removes a `.cpp` the
  exported README invites students to edit. B owns it.
* B tests with a hand-built `GeneratedProject` (two modules, one header):
  export, build with `make` and CMake where present, then *Replace* after a
  module rename and after a delete, each with an edited file kept. Each
  package that adds a multi-module example (F, T, N) adds it to the export
  test's `EXAMPLES` and builds it with `make` and CMake.
* When any exported `.cpp` holds a `using namespace` directive, the
  README says so and that headers never do (06 §6.14.12); N adds that line.
* modules.md §8 (slug, `<slug>` target, manifest format 1) is superseded by
  what w3-export-run.md §4 built; the docs step corrects it (§5.4).

### 2.9 The project format and versions

* **No new key name** (ADR-0013): modules, their names and the project name
  exist; `SHARED` is a catalog field; the namespace blocks are new blocks;
  *use namespace*'s field stores a value of the field kind `namespace`
  (as ADR-0019 does for `module`): a closed name with a leading `::`
  (`::std`, `::std::chrono`), which no symbol ID can contain, or a user
  namespace's symbol ID as a plain string. The analyser checks the value
  against the closed table or the symbol table (`E0201`, `E0207`), and
  codegen prints only the resolved name, never the stored text.
* **Textbook style** is `options.usingNamespaceStd`, reserved by ADR-0013
  as a format-2 key but refused by today's loader (§0). P0 makes it
  readable and writable under format 2 with no version bump:
  `ProjectOptions::using_namespace_std` (default false), its decoder, the
  canonical writer (written only when true), `required_format_version`
  returning 2 when it is true, and the content hash. A project that turns it
  on is saved as format 2, and an M2 app refuses it with `E0108`.
* **Catalog 1.3.0** (still unreleased) gains `SHARED` on `func.define`,
  `program.namespace`, `program.use_namespace`, the field kind `namespace`
  and, if S does it, `program.use_module` with the field kind `module`
  (modules.md §7.3's `org.use_module`, renamed by the docs step). An M2 app
  reports a *define* that carries `SHARED` as from a newer catalog
  (`E0602`), as ADR-0015 intends; W3M-D11 asks whether the editor writes
  `SHARED` only when it is true, so single-module projects stay
  M2-readable.
* **Source map** version 1 with the optional `module` key; `IPC_VERSION`
  unchanged; `packages/ipc-types` regenerated for the new symbol kind
  (`SymbolInfoKind::Namespace`, by N1, §7.1).
* **Module edits** (add, rename, delete, move) are edits of the document
  (§2.11's dialogs), saved like any other: a module has `id`, `name` and
  `workspace` (05 §5.3). A new module's ID is `mod_<random>` from the
  editor's ID source; its name is the first free of `module2`, `module3`, …
  The document operations refuse a 257th module and the deletion of the
  last one, as the strip does.

### 2.10 Security

* **Module names become file names** only through `FileStem`
  (`[a-z][a-z0-9_-]{0,63}`, no Windows device name, no `b2c` prefix in any
  case). The add and rename dialogs check a candidate by asking the core
  to read a candidate document (`core.canonical`, as `checkProjectName`
  does), so the pattern, device names and case come from the Rust rules
  and no TypeScript copy of the device list can drift; they refuse the
  `b2c` prefix and names equal ignoring case (05 §5.6), and suggest the
  lower-case form when capital letters are the only problem. A file that
  gets past the dialogs (hand-edited) is caught by the loader (pattern,
  device names, case) or by `E0221` (the prefix), which blocks Build, Run
  and Export. No path ever comes from a module name: `#include
  "<stem>.hpp"` lines and file names are printed from `FileStem` only.
* **Namespace names** are identifiers (the `Ident` rules), never file names
  or paths. A namespace named `std` or starting with `b2c` is refused
  (`E0220`, as other reserved names). Nesting is at most
  `MAX_NAMESPACE_DEPTH` = 14 (`E0219`), which keeps qualified names within
  `MAX_QUALIFIED_NAME_LEN` (a const assertion ties the two).
* **Placement in hand-edited files** (`crates/b2c-catalog/src/resolve.rs`,
  `E0604`): `program.namespace` only at the top level or in a namespace's
  mouth; only definition blocks in that mouth; `program.use_namespace`
  only at the top level; a top-level definition takes no stacked blocks (a
  definition's next connection exists only inside a mouth), so the format
  of single-module projects and M2's reading of them do not change.
* **Pruning** deletes only regular files with generated names in the build's
  own `gen/` (and, with the cache, `obj/`), never through a link; `b2c
  generate` never deletes.
* **Deleting outputs** uses the step's absolute path (§2.7), never a path
  relative to the process's working folder.
* **Export's Replace** deletes or overwrites only files its manifest lists
  with a matching hash (§2.8); renaming or deleting a module removes the
  old files only through that rule.
* **Cache files** (if 2): message files are regular files of at most
  4 MiB, parsed strictly; anything wrong is a miss.
* The threat-model review (W6) records the module-name path, pruning,
  output deletion, Replace and the cache; no new IPC command exists to
  review.

### 2.11 Accessibility

* The module strip is a toolbar labelled *Modules* with a roving tab
  index: one Tab stop for the module buttons (the shown one), arrow keys,
  Home and End move between them, Enter or Space shows a module. Each is a
  toggle button (`aria-pressed`). The context-menu key or Shift+F10 on a
  module button opens its actions; the same actions are on one *Module
  actions* menu button, which acts on the shown module; ⊕ *Add module* (a
  button with a text label for screen readers) is its own Tab stop. The
  menus follow the app's menu pattern (`app/layout/MainMenu.tsx`). Disabled
  items (⊕ at 256, *Delete…* on the only module) keep focus and give their
  reason as their accessible description and tooltip.
* When the strip overflows, an *All modules…* button opens a list with
  type-ahead that shows the chosen module; the strip also scrolls inside
  itself (the page never does) and keeps the shown module in view, at
  800 × 560 too.
* The add and rename dialogs are the app's dialogs: a labelled text field,
  the problem under it (`aria-invalid`, `aria-describedby`,
  `role="alert"`), focus trapped while open and returned to the strip;
  *Delete module* is a confirmation dialog naming the module, how many
  top-level blocks it holds and, if so, that it holds *when program
  starts*, with *Cancel* the default.
* Status messages go to the existing status region: *Module `utils`
  added; the code of the other modules changed* (for the second module,
  §2.13), *renamed to `tools`*, *deleted* with an *Undo delete* button
  (§6 W3M-D6).
* The *share with other modules* checkbox has its words as its accessible
  name, in Friendly and C++ mode.
* The settings page's *Name* field and *Write `using namespace std;`*
  switch follow the page's pattern (w3-export-run.md §5).

### 2.12 Namespaces and the lookup model (the M3 subset)

* **The block.** `program.namespace` is a definition block (no previous
  connection at the top level) with a `NAME` `symbol_decl` field and a
  statement-like mouth `DEFINITIONS` that takes definition blocks. The
  definition blocks gain a previous and next connection of the check
  `definition`, which only a namespace's mouth and another definition
  inside it accept. The format stores the definitions as the mouth's
  statement list (05 §5.4); a definition never stacks under a top-level
  block (§2.10). Nesting is allowed to 14 levels (`MAX_NAMESPACE_DEPTH`,
  new in `crates/b2c-model/src/limits.rs`, P0; `E0219` past it; the editor
  refuses the drop). This changes the catalog's placement rules
  (`schema.rs`'s `Shape::Definition` and `is_top_level`, `resolve.rs`,
  `stack.rs`) and the analyser's idea that definitions are exactly the
  top-level blocks (`collect.rs`, `query.rs`'s `DEFINITION_TYPES`,
  `lower/mod.rs`); N owns all of them (§3.2). §6 D2 records the shape
  question and the spike that settles it.
* **The IR.** `SymbolKind::Namespace { parent: Option<SymbolId> }` and its
  `SymbolInfoKind` (N1, §7.1); each `Item` gains `namespace:
  Option<SymbolId>` (the innermost; the parents come from the symbol
  table); each reference to a namespace-scope entity has a `Spelling`
  (`Plain`, `Global` for `::n`, `Qualified` for `geo::n`, `Full` for a
  standard name in full), kept in `Program::spellings` beside the SAST and
  keyed by the referring block and the symbol, default `Plain`/`Full` so
  that today's output is unchanged. P0 adds the types; N fills them.
* **One namespace in several modules** is one C++ namespace: each member
  is shared or not on its own; a namespace has no `SHARED`.
* **Lookup (06 §6.14.2), M3 subset.** Scopes are blocks, functions,
  enclosing user namespaces, the module's anonymous namespace, the global
  namespace, the nominated namespaces and, for calls, argument-dependent
  lookup over user namespaces and `std` (06 §6.14.6). No classes, packs or
  Raw C++ exist in M3, so their rules wait for M4. A reference is written
  plainly only when this lookup finds exactly the intended entity;
  otherwise `::name` or `geo::name`; a user function whose name is a
  nominated name is always qualified.
* **The standard-name table** (06 §6.14.8): `crates/b2c-ir/src/std_names.rs`
  generated by `tools/gen-std-names.py`, the union over g++ 11 to 15 per
  standard. Only g++ 11 to 14 are local, so the tool merges its local
  output with a run in the CI `gcc:15` container: a check-mode step in
  the `gcc` job of `.github/workflows/ci.yml` fails when the table lacks a
  name that GCC 15 declares (N adds it). The integration step regenerates
  from every local `g++-N` plus the GCC 15 list the CI step prints. Unlike
  `reserved_names.rs` (one compiler, 13.3.0), the table names every GCC it
  came from; a toolchain newer than its newest counts as an incomplete
  table (no shortening, §1.8).
* **Directives** (06 §6.14.3): never in a header; in a single-module
  `main.cpp` after the includes and the inline helpers, and in a
  multi-module `.cpp` as the first lines of an anonymous-namespace block;
  in both, after an empty declaration of each user namespace that a
  directive names and no included header declares (`namespace geo {}`,
  sorted by name; §1.7). A *use namespace* naming a namespace declared in
  another module's header adds that header. Textbook style writes the
  directive only where a standard header is included.
* **Spelling standard names** inside a region (06 §6.14.5): shortened only
  when no user declaration, generated local or included project header
  name is spelled like it and no other nominated namespace declares it.
  Spellings reach every place codegen prints a reference or a standard name
  (`cast.rs`, `emit.rs`, `helpers.rs`, `lower/expr.rs`, `lower/text.rs`,
  `stmt.rs`, `collections.rs` and the other lowering files), which is why
  that part of N runs after T (§3.4).
* **The `b2c::to_text` of an enum** in a user namespace, an anonymous
  namespace or both is printed at global scope with those namespaces closed
  and reopened (§1.7; w2.md §2.11; checked, §0).
* **E0215** uses only the names of the project's standard; the quick fix
  (capitalise) is W4's.
* **Textbook style in M3** is one project edit from the settings page,
  without 06 §6.14.12's preview; the page says under the switch that it may
  show problems that a rename fixes, and that turning it off again removes
  them (W3M-D13).

### 2.13 Projects that gain a second module, and M2 projects with several

The anonymous namespace and `b2c_support.hpp` depend on the module count.
So when a project gains its second module, or an M2 project with several
modules opens, the C++ of the existing module or modules changes: the inline
helpers move to `b2c_support.hpp`, each module's functions move into an
anonymous namespace (none is shared yet), and a function nothing calls
draws `-Wunused-function`, an error under *warnings as errors* (§0's check),
on blocks the student never touched. E's status message for the second
module says that the code of the other modules changed (§2.11); the release
notes say so (ADR-0019, *Compatibility*), and W4's friendly message names
the fix. Deleting back to one module restores the single-module text. A
call across modules, which M2 refused with `E0206`, now needs the function
shared; the new text says so.

## 3. Packages

### 3.1 P0: the contracts step (round R0)

Like W2's P0, it keeps the workspace and every test green and leaves each
package only its own files. One agent, Rust and a little TypeScript, size L:

* `b2c-ir`: `SymbolKind::Namespace` (with `SymbolTable::namespace_path`),
  `Item::namespace`, `Spelling` and the table `Program::spellings`
  (§2.12), all with defaults that change no output and serialise nothing
  while default. `SymbolInfoKind::Namespace` (IPC mirror, `packages/ipc-types`
  regenerated) moves to N1 (§7.1); until then `SymbolInfo::new` gives
  `None` for a namespace.
* `b2c-model`:
  * *Textbook style* readable and writable (§2.9): `ProjectOptions`
    (`crates/b2c-model/src/document.rs`), the decoder of `project.options`,
    the canonical writer, `version.rs` and the content hash; the tests
    `textbook_style_is_not_a_key_yet` (`tests/versions.rs`) and the unknown-key
    case of `tests/rules.rs` rewritten as round trips (format 1 when off,
    format 2 when on, an M2-format reader reports `E0108`);
  * `MAX_NAMESPACE_DEPTH = 14` in `limits.rs`, with a const assertion that
    a name of `MAX_IDENT_LEN` characters inside that many namespaces fits
    `MAX_QUALIFIED_NAME_LEN`;
  * the matching optional field in the TypeScript document types
    (`packages/b2c-core-wasm/src/bdm.ts`, a W5 carve-out agreed before P0
    starts, §3.3) and in `apps/desktop/src/app/testing/fixtures.ts`.
* `b2c-lang`:
  * `codes.rs` constants for §1.9 (with `E0219`) and their reference
    entries' headings;
  * `crates/b2c-lang/src/layout.rs` (new): `compute_layout` called instead
    of `Layout::flat` in `lower/mod.rs:108`, filling stems and calling the
    graph; the hooks F and T fill (`shared_items`, `header_parts`,
    `includes`) as functions returning nothing yet;
  * `E0221`, with its message and tests (single- and multi-module);
  * `Item::shared` set from the `SHARED` field in a multi-module project
    (false in a single-module one, whatever the field says);
  * the module-table split: one function and global table per module plus
    one table of shared definitions per namespace, so F and T only add
    lookups.
* `catalog/core/functions.toml`: `SHARED` on `func.define` (checkbox,
  default `false`, after `RETURNS`'s words: *share with other modules*);
  if the owner accepts W3M-D11, the catalog schema's flag that a field is
  written only when it differs from its default (`crates/b2c-catalog`,
  `packages/catalog-gen`), set on every `SHARED`;
  `packages/catalog-gen/catalog.json`, `packages/blockly-ext/src/generated/catalog.ts`
  and `docs/reference/blocks/` regenerated (generated files, §3.3);
  the stale comment of `catalog/toolbox.toml:236` fixed (§1.10).
* `b2c-codegen`:
  * file names from `ModuleFiles::stem`; `source_path` and `is_file_stem`
    go; `GeneratedFile::module` and `FileMap::module` set;
  * `emit::header_file` (the order of §1.7) printing whatever
    `header_order` lists, empty while nothing is shared;
  * helpers placed by module count (`b2c_support.hpp` from two modules on,
    for build and preview alike); `CodegenOptions::helper_placement` stays
    as an ignored field until W5's files can drop it (§3.3);
  * the anonymous namespace around non-shared declarations and definitions
    in a multi-module `.cpp`, closed and reopened where visibility changes,
    with an enum's `b2c::to_text` printed at global scope (§1.7);
  * namespace blocks printed around items (`namespace geo { … }`, reopened
    as the order needs; `namespace geo { namespace { … } }` for non-shared
    members);
  * snapshots: a two-module program with nothing shared; one with a
    module named `b2c_x`; a two-module program with a non-shared enum
    that is joined, plus `ask<int>` in the same unit (also compiled and run
    with g++ when present); a single-module program's `main.cpp` before
    and after a second, empty module is added.
* Every golden file byte-identical; the catalog snapshot test gains the
  `SHARED` variant of `func.define`.

### 3.2 The packages

Each package owns the files listed and writes nothing else, except its own
new test files and its anchors in `catalog/toolbox.toml`. A file listed for
two packages is edited in different rounds.

| Package | Scope | Owns | Size | Needs | Examples |
| --- | --- | --- | --- | --- | --- |
| **P0** | §3.1 | §3.1 | L | `8a3d8ec` | — |
| **B, build and export** | §2.7 (pruning, relative objects with absolute paths kept for deletion, de-duplication, `locate` by `FileMap::module`), §2.8 (the README length line, manifest format 3 and Replace keeping edited files, the tests with a hand-built `GeneratedProject`) | `crates/b2c-build/src/compile.rs`, `build_dir.rs`, `export.rs`; `crates/b2c-build/tests/build_sessions.rs`, `export.rs`; `crates/b2c-cli/src/commands/project.rs` (the `generate` test); `.github/workflows/ci.yml` (the Windows job installs `mingw-w64-ucrt-x86_64-make`, whose `mingw32-make.exe` lands in `ucrt64\bin`, already on `PATH`; `B2C_REQUIRE_MAKE=1` in the Linux `gcc` and Windows jobs) | M | — (R0, beside P0) | — |
| **F, functions across modules** | §1.2 but the editor rows; `E0206`, `E0211`, `E0213` (with its cause), `E0214`, `E0421` and `E0422` for functions; the `::name` guard (§2.1); the golden test's new checks | `crates/b2c-lang/src/lower/resolve.rs`, the function pre-pass and `declare_function`/`check_overloads` in `lower/mod.rs`, `query.rs` (scope query and precedence), `layout.rs` (functions' header parts and includes); `crates/b2c-codegen/src/lower/mod.rs` (calls and declarations across files), `emit.rs` (header declarations); `crates/b2c-lang/tests/modules_functions.rs` (new); `crates/b2c-build/tests/golden.rs` (a stale `.cpp` or `.hpp` in a golden folder fails, deleted under `B2C_UPDATE_GOLDEN`; each header compiled alone through a one-line unit; the M2 rule restated for multi-module output); `crates/b2c-build/tests/export.rs` (`unit_converter` in `EXAMPLES`); the `unit_converter` example and golden | L | P0, B | `unit_converter` |
| **E, the editor for modules** | §1.2's editor rows (limits, *Undo delete*, refusal in the banner state), §1.5, §2.5, §2.6's revealing, §2.11, §2.13's status message | `apps/desktop/src/features/project/modules/` (new: dialogs, commands, the document operations, the kept deleted module); `apps/desktop/src/editor/modules/` and the module operations of `editor/sync/session.ts` and `EditorWorkspace.tsx` (carve-outs, §3.3); `apps/desktop/src/app/editor-types.ts`, `app/panels.tsx`, `apps/desktop/src/panels/code/`; `apps/desktop/src/editor/services/symbols.ts` (other modules' shared functions and types, `multiModule()`); `packages/blockly-ext/src/blocks/layout.ts`, `register.ts` and `services.ts` (`SHARED` shown on `func.define` only; carve-out); `apps/desktop/src/editor/toolbox/contents.ts` (*Your functions* by module; carve-out); `apps/desktop/src/editor/sync/workspaceToBdm.ts` (`SHARED` written only when true, if W3M-D11 is accepted; carve-out); `apps/desktop/src/features/project-settings/` (the *Project* section; carve-out); `apps/desktop/e2e/specs/flows/modules.e2e.ts`, `project-name.e2e.ts` | L | P0; F merged before its E2E | — |
| **T, types, globals and constants across modules** | §1.3; §2.2; the rest of §2.3 and §2.4 | `crates/b2c-lang/src/order.rs` (the program graph, the summary), `layout.rs` (types' and constants' header parts, forward declarations, includes, `E0422`), `lower/user_types.rs`, `lower/globals.rs`, `consts.rs` (constant expressions across modules), `lower/resolve.rs` (the globals branch of `E0206`, after F); `crates/b2c-codegen/src/lower/typedefs.rs` (enum print support split between header and `.cpp`), `lower/globals.rs` (the constant forms), `emit.rs` (header definitions, `extern`); `crates/b2c-lang/tests/modules_types.rs` (new); `crates/b2c-build/tests/export.rs` (`gradebook`, `inventory` in `EXAMPLES`); `layout.ts` (`SHARED` shown on struct, enum and global; carve-out) and the type field's module grouping (carve-out, §3.3); `apps/desktop/e2e/specs/flows/modules-types.e2e.ts`; the `gradebook` and `inventory` examples | L | F, E merged | `gradebook`, `inventory` |
| **N1, namespaces: the block and the table** (if 1) | §2.12's block, placement and table; `E0219`; namespace symbols and nesting; `SymbolInfoKind::Namespace` (§7.1) | `crates/b2c-ir/src/scope_info.rs` and `crates/b2c-ipc/src/pipeline.rs` (`SymbolInfoKind::Namespace`, `packages/ipc-types` regenerated); `catalog/core/program.toml` (`program.namespace`, `program.use_namespace`); `crates/b2c-catalog/src/schema.rs`, `resolve.rs`, `stack.rs` (placement and the `namespace` field kind) and `packages/catalog-gen`; `crates/b2c-lang/src/collect.rs` (definitions inside a mouth), `crates/b2c-lang/src/lower/namespaces.rs` (new: namespace symbols, nesting, `E0219`, `E0210` for namespace names, the map from each definition to its namespace); `tools/gen-std-names.py` and `crates/b2c-ir/src/std_names.rs` (new); `.github/workflows/ci.yml` (the GCC 15 check step); `packages/blockly-ext/src/blocks/namespace.ts`, `fields/namespace.ts` (new files) and the `definition` connection check and depth refusal in `checker/` (carve-out) | M | P0; the D2 spike first | — |
| **N2, namespaces: lookup, spellings and directives** (if 1) | §1.4's other rows, §2.12's lookup, spellings and directives | `crates/b2c-lang/src/lower/lookup.rs` (new), `query.rs` (`DEFINITION_TYPES`, namespace members), `lower/mod.rs`, `lower/user_types.rs`, `lower/globals.rs` (copying `Item::namespace`, `E0210` per namespace); every lowering file of `crates/b2c-codegen` that prints a reference or a standard name, `emit.rs` (directives, empty namespace declarations) and `lower/names.rs` (new); `crates/b2c-build/src/export.rs` (the README line) and `tests/export.rs` (`shapes`, `textbook_grades`); the *Language* switch in `features/project-settings/` (carve-out); `apps/desktop/e2e/specs/flows/namespaces.e2e.ts`; the `shapes` and `textbook_grades` examples | L | N1, T merged | `shapes`, `textbook_grades` |
| **C, the object cache** (if 2) | §1.6 | `crates/b2c-build/src/compile.rs` (steps), `build_dir.rs` (`obj/`), new `crates/b2c-build/src/objects.rs`; `crates/b2c-build/tests/build_sessions.rs` (cache cases) | M | B, T merged | — |
| **S, if-time sweep** (if 3) | `use namespace … here`, the standard sub-namespaces, `use header of module` (modules.md §7.3) | N2's files and `catalog/core/program.toml` after N merged; the `module` field kind | S–M | N merged | — |

N1 and N2 are one branch: N1's commits stay unmerged, N2 rebases them on T
and continues, and the branch merges once, so cutting N (W3M-D3) leaves
nothing half-built on the wave branch.

### 3.3 Shared files and the W4/W5 boundary

* **Generated: regenerate, never hand-merge**, as w2.md §3.3: the catalog
  JSON and TypeScript, `docs/reference/blocks/`, the type and slot
  fixtures, `packages/ipc-types/src/generated/`, `std_names.rs`.
* **Snapshots** are per case; the integrator reviews new ones.
* **`catalog/toolbox.toml`** anchors: N1 after `program.use_header`
  (*Program*); S right after N's.
* **Examples and goldens**: each package adds its own example in one commit
  with its run data, its row in `examples/README.md` and its entry in the
  export test.
* **Spec, CHANGELOG, contracts, other READMEs**: only the docs step (§5.4);
  each package ends its last commit message with a *Spec changes* list.
* **The W4 and W5 cloud sessions' paths stay out** of every package but
  for these carve-outs, which the coordinator asks each session to agree
  to before R0 (the `bdm.ts` row) and R1 (the rest; §6 D1); a declined
  carve-out moves that work to the integration step, after the session's
  branch merged:

  | Session | Path | What W3 changes there | Package |
  | --- | --- | --- | --- |
  | W4 | `apps/desktop/src/editor/modules/` | the strip: ⊕, the *Module actions* menu, the roving tab index and *All modules…*, shown from one module on, limits, the `module<N>.cpp` label with an error mark | E |
  | W4 | `apps/desktop/src/editor/sync/session.ts`, `EditorWorkspace.tsx` | `addModule`, `renameModule`, `deleteModule`, `restoreModule` and `moveModule`, each refused while the canvas holds an unchecked change; nothing else | E |
  | W4 | `packages/blockly-ext/src/blocks/layout.ts`, `register.ts`, `services.ts` | `SHARED` shown on `func.define` when `services.multiModule()` says so, and every definition block laid out again when the module count crosses 1 and 2 (E); on struct, enum and global in R2 (T); nothing else | E, T |
  | W4 | `apps/desktop/src/editor/sync/workspaceToBdm.ts` | `SHARED` written only when true (only if W3M-D11 is accepted) | E |
  | W4 | `apps/desktop/src/editor/toolbox/contents.ts` | *Your functions* with other modules' shared functions under module headings | E |
  | W4 | `packages/blockly-ext/src/fields/type.ts`, or W4's type picker | other modules' shared structs and enums under module headings, from the `symbols.ts` provider E adds in R1; if declined, T's E2E checks only that the type is offered | T |
  | W4 | `apps/desktop/src/features/project-settings/` | the *Project* section (name; both save paths) and, in *Language*, the Textbook style switch with its note | E, N2 |
  | W4 | `packages/blockly-ext/src/blocks/namespace.ts`, `fields/namespace.ts` (new), `checker/` (the `definition` check, the depth refusal) | the namespace blocks | N1 |
  | W5 | `packages/b2c-core-wasm/src/bdm.ts` | the optional `usingNamespaceStd` option | P0 |
  | W5 | `crates/b2c-core-wasm/src/clipboard.rs`, `scope.rs` | the arms of `SymbolInfoKind::Namespace` (N1, §7.1); paste precedence (§2.1) and namespace-qualified names; a cut declaration pasted where its symbol ID is free keeps it (only if W3M-D12 (b) is chosen); the module documentation's *In M2* section | integration step, or W5 on request |
  | W5 | `crates/b2c-core-wasm/src/facade.rs`, `benches/pipeline/`, `tests/common/mod.rs` | `helper_placement: Inline` removed once codegen drops the field; the 8-module benchmark case (modules.md §12) | integration step |
  | W5 | M3-5's compact source map | keeps `FileMap::module` on each file and `shared` on each symbol (ADR-0019 §10, modules.md §10); the C++ tab and compiler mapping depend on it | W5 (coordination, before R1) |
  | W5 | the worker's scope index (`Analysis::scope_index_parts()`, which W5 adds on its branch) | must include other modules' shared symbols with their module and namespace; F exposes them through `query.rs` and tells the W5 session the shape; the integration step checks the worker's index includes them, with a test | F (coordination), integration step |

  Nothing in W5's `apps/desktop/src/editor/preview/` or the two toolbox
  files (`inflater.ts`, `continuous.ts`) changes.

### 3.4 Order

Two agents at a time. Each round's packages run in parallel worktrees and
merge in the order given; the second rebases on the first. The owner's
answers to W3M-D10 to W3M-D13 come before R1.

| Round | Packages | Merge order | Why |
| --- | --- | --- | --- |
| R0 | P0, B | P0, B | B touches only `b2c-build`, `b2c-cli` and CI; P0 the rest |
| R1 | F, E | F, E | E builds its UI against P0's plumbing and fixtures, then rebases on F for the E2E flow that calls across modules. Part 1 is done here |
| R2 | T, N1 | T (N1 stays on its branch) | N1 touches the catalog, `collect.rs`, new files and the editor's new block files, none of T's. Part 2 is done here |
| R3 | N2, C | C, N2 | N2 needs T's lowering and `emit.rs`; C touches only `b2c-build` |
| R4 | S | S | if 3, after N merged |

If-time work starts when an agent is free and is cut whole, in the order
N, C, S, when the budget is spent (M3-D6). Then the integration step
(§5.3), the docs step (§5.4), the review with lenses and adversarial
verification, and the fixer, as in every M3 wave. If N is cut, `shapes`
drops its namespace and moves to T, or to the integration step if T has
merged.

## 4. Acceptance

### 4.1 Example projects

Each is an example in `examples/` with its golden files and run data in
`tests/golden/<name>/` (one file per generated file, as `golden.rs` already
compares), written as w2.md §4.1 says, and generated, compiled, linked and
run by the golden test. **Every non-shared definition of an example is used
in its module**, since an unused one in an anonymous namespace is a warning
and the golden test fails on any warning.

| Example | Item | Modules and what each holds | Files | What it runs |
| --- | --- | --- | --- | --- |
| `unit_converter` (F) | functions across modules | `main` (the menu loop, *when program starts*); `convert` (shared `to_fahrenheit`, `to_celsius`, `rounded` with a default value; a non-shared `scale`, which `rounded` calls); `table` (shared `print_table`, which calls its own non-shared `scale` with another body) | `main.cpp`, `convert.cpp`, `convert.hpp`, `table.cpp`, `table.hpp`, `b2c_support.hpp` (*ask*) | a temperature table between two typed bounds, as `temperature_table` but split |
| `gradebook` (T) | structs and enums | `main`; `records` (shared struct `Student`, shared enum `Grade`, shared `constexpr int kPassMark = 50`, shared `grade_of(const Student&)`, non-shared `clamp_mark`, which `grade_of` calls); `report` (shared `print_report(const std::vector<Student>&)`, whose header forward-declares `Student`; it prints each grade with a `switch` on `Grade` and compares marks with `kPassMark`) | three `.cpp`, two `.hpp`, `b2c_support.hpp` | `student_records` split into modules, with a grade per student |
| `inventory` (T) | globals and constants | `main` (with a non-shared global `count`, read by its menu); `stock` (shared variable `item_count`, shared `const std::string kShopName` set from a function, shared `constexpr int kMaxItems` used as a `case` label in `main`; `add_item`, `remove_item` shared); `history` (a shared `record_change`, which updates a non-shared global `count` of its own) | three `.cpp`, two `.hpp`, `b2c_support.hpp` | a menu that adds and removes items and prints the shop name and count; no global set from another module's before `main` (`E0423` stays quiet) |
| `shapes` (N, after T) | namespace across modules | modules.md §12: `main`, `geometry`, `report`; namespace `geo` in `geometry` and `report`; a shared struct, enum, `constexpr` constant, variable and function; same-named non-shared helpers in two modules, each called in its module; `use namespace [geo] in this file` in `report` | `main.cpp`, `geometry.cpp`, `geometry.hpp`, `report.cpp`, `report.hpp`, `b2c_support.hpp` | areas and colours of shapes |
| `textbook_grades` (N) | Textbook style, `use namespace` | one module; *Textbook style* on; a user function `max` and a global `count` beside `std::max` and `std::count`, so the code shows `::max(…)` and `::count` where it must and `cout` elsewhere | `main.cpp` | `grade_average` in textbook style |

The project name has no example (it changes no code); its E2E flow and
the settings page's tests cover it (§5.2, E). The cache is covered by the
build tests (§5.2, C).

### 4.2 Toolchains

* **Linux, g++ 11 to 15:** the golden test generates, compiles the Debug
  configuration (any warning fails the test), links and runs every example
  (the CI `gcc` job runs `b2c-build`'s tests with `B2C_REQUIRE_GXX=1` on
  GCC 11 and 15, the Linux job on GCC 13). Each header also compiles alone
  through a one-line unit (`#include "<stem>.hpp"`), as modules.md §12 says
  (F adds it to `golden.rs`).
* **Windows, MinGW (MSYS2 UCRT64):** the same tests in the Windows CI job
  (`B2C_REQUIRE_GXX=1 cargo test`).
* **Export:** each multi-module example exported builds with `make` (and
  CMake where present) on Linux and with `mingw32-make` on Windows,
  printing the golden output; `B2C_REQUIRE_MAKE=1` in both CI jobs turns a
  missing `make` from a skip into a failure (B).
* **Every local `g++-N`:** the integration step's forms test (W2's
  `crates/b2c-codegen/src/lower/tests.rs`) gains a three-module program
  with every row of §1.7, C++17 and C++20.

## 5. Tests and checks

### 5.1 Every package

* As w2.md §5.1: `cargo fmt --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo test --workspace`; `pnpm
  desktop:check` and `pnpm test:web`; generated files fresh;
  `python3 tools/check-diagnostics-docs.py`, `tools/check-layering.py`;
  `pnpm exec markdownlint-cli2`, `node site/build.mjs
  --check-repo-markdown`.
* With g++: `B2C_REQUIRE_GXX=1 cargo test -p b2c-build --test
  catalog_syntax --test golden --test assignments --test export`; the
  assignments test still prints `M3 assignments: 10 of 10 green`, and
  every earlier golden file is byte-identical.
* Every code a package emits has a `b2c-lang` test with its exact message.
* The Linux E2E flows the package adds, on the built app, at the default
  size (1280 × 800), 1024 × 768 and 800 × 560
  (`B2C_E2E_WINDOW_SIZE`); Windows runs them in CI on the next push.
* A package with new blocks adds their IDs to `common::AFTER_M2`
  (`crates/b2c-lang/tests/common/mod.rs`) and regenerates that snapshot, as
  w2.md §5.1 says.

### 5.2 Per package

* **P0.** Every SAST snapshot and every golden file unchanged; the new IR
  fields serialise nothing at their defaults; `E0221` in a single- and a
  multi-module project, and the file shown as `module<N>.cpp`; a
  two-module program prints each module's code in an anonymous namespace
  and `b2c_support.hpp` when a helper is used, for the build and the
  preview options alike; a non-shared enum joined in a two-module program
  with `ask<int>` prints its `b2c::to_text` at global scope and compiles;
  `main.cpp` before and after a second, empty module; `func.define` loads
  with and without `SHARED`; Textbook style round-trips (format 1 off,
  format 2 on, `E0108` for an M2-format reader); the depth const assertion.
* **B.**
  * `gen/` pruning deletes a stale `old.cpp` and `old.hpp`, keeps a link
    and a file with another name, and never runs in `write_generated_files`
    (`b2c generate` into a folder with an extra `utils.cpp` leaves it); a
    second build with no edits deletes nothing and is up to date; a renamed
    module leaves no old file in `gen/`.
  * Objects are relative to `diag/` in the argv; a failed multi-module
    build, run with a process working folder that holds a decoy
    `../out/main.o`, removes the build's objects and leaves the decoy; g++'s
    link argv for 256 modules with 64-character stems stays under 26,000
    characters.
  * A header message reported by two units (from recorded SARIF) is kept
    once; an unmapped place in a file with no ranges attaches to that
    file's module; `b2c_support.hpp` to the project.
  * Export of a hand-built two-module `GeneratedProject` with a header,
    built with `make` (and CMake where present); *Replace* after a rename
    and after a delete removes the unchanged old files and keeps an edited
    one, naming it; an older manifest without hashes keeps every listed
    file it would delete; the README length line appears only past 8,000
    characters.
* **F.**
  * Analyser: visibility and precedence (an own non-shared `area` hides
    another module's shared `area` in the scope query and for paste
    binding by name); `E0206` (new text) for a non-shared function, none
    for a shared one; `E0211` for shared functions with one name in two
    modules, with the rename-only text; non-shared same-named functions in
    two modules accepted; `E0213` for a non-shared name meeting a shared
    one through an included header, directly and through a second header,
    and on an untouched own definition after an unrelated use of the other
    module is added, with the message naming that use and the related
    locations of the chain; `E0214` for a parameter type, a result type and
    a default value naming non-shared definitions (with W2's struct and
    enum blocks), none for a body that does; `E0421` only (no `E0422`) for
    two modules' shared functions whose default values call each other;
    `E0422` for `a`'s default value calling `b`'s `g` while `b`'s calls
    `a`'s `k`; `main` in the second module.
  * Layout: `header_includes`, `source_includes` (own header always),
    `header_order`; a single-module program has no header and `shared`
    false whatever the field says.
  * Codegen snapshots: a shared function with a default value (header
    only); a non-shared helper with the same name in two modules; a
    module with nothing to generate (its `.cpp` holds only the comment);
    the guard that never writes `::name` for an entity an included header
    also declares.
  * Properties (2 to 5 modules, functions only, C++17 and C++20):
    generation deterministic; header includes acyclic when no error;
    editing a function body leaves every header byte-identical;
    reordering modules leaves every file's text unchanged; with g++ (a
    bounded number of cases in CI), each header compiles alone and the
    program links.
  * Golden: a stale file in a golden folder fails; each header compiles
    alone. `unit_converter` golden, run and exported.
* **E.**
  * Vitest: the strip (⊕, *Module actions*, the roving tab index, Home and
    End, Shift+F10, `aria-pressed`, shown with one module, *All modules…*
    with type-ahead); a 256-module fixture (⊕ disabled with its reason, the
    strip and axe still pass); *Delete…* disabled on the only module; the
    add and rename dialogs refuse `Main` (suggesting `main`), `2d`, `con`,
    `b2c_x`, a name equal ignoring case and 65 characters, each with its
    message from the core; delete asks when the module has blocks and not
    when it is empty, and names *when program starts* when it holds it;
    *Undo delete* restores the module at its place with its IDs, so another
    module's calls bind again; every module edit is refused while the
    canvas holds an unchecked change; the document operations keep symbol
    IDs and the other modules' blocks; a rename changes no block; the C++
    tab opens the module's `.cpp` by `module` key, lists the shown module's
    files first, falls back when a picked header goes; `SHARED` shown on
    `func.define` only in a multi-module project and kept when hidden,
    shown on existing blocks when a second module is added and hidden (value
    kept) when it is deleted; *Your functions* groups other modules' shared
    functions and leaves out those hidden by an own name; the second
    module's status message; the settings page's *Name* field (empty, 101
    characters, a valid name saved with the settings as one edit with and
    without an editor, a name-only change saved, a top-bar rename made
    meanwhile kept when the field was not edited; *Cancel* discards); axe on
    the strip, the dialogs and the page.
  * E2E `modules.e2e.ts` (after F merged): add module `utils`; define a
    function, tick *share with other modules*; call it from `main` through
    *Your functions* under *utils*; Run and see its output; the C++ tab
    lists `main.cpp`, `utils.cpp`, `utils.hpp`; clicking the header's
    declaration selects the *define* block in `utils`; rename `utils` to
    `tools` and run again (`#include "tools.hpp"`); untick *share* and see
    `E0206` in Problems under module `main`, choose it and land on the
    call; delete the module (confirm) and see `E0201` on the call; *Undo
    delete* and see it run again; at 800 × 560 the canvas keeps its minimum
    height with the strip shown.
  * E2E `project-name.e2e.ts`: change the name on the settings page, save,
    see the new name and `•` in the top bar and in the C++ header comment;
    an empty name shows the problem and saves nothing.
* **T.**
  * Analyser: each row of modules.md §3.3 as widened in §2.4 (an include or
    a forward declaration), including field defaults that call another
    module's function and read its `constexpr` constant, its `extern const`
    constant and its variable, a field of a comparable and of a printable
    struct, and each container other than `std::vector`; `E0214` for each
    header part of a struct, an enum, a constant's type and a variable's
    type; a shared `constexpr`-able constant whose value names a non-shared
    constant printed `extern const`, with no error, and `E0424` where
    another module uses it as a `case` label; `E0422` for two- and
    three-module include cycles of types and constants that no definition
    cycle explains; `E0423` direct, through a chain of calls, through a
    struct's default member initialiser and through a default value, none
    for a `constexpr` source, for `const int k = 10;` or for a read in
    `main`, one for `int count = 0;` and for a `const std::string`;
    `E0424` for another module's `const` (not `constexpr`) constant as a
    `case` label, none for a `constexpr` one; `E0420` across modules;
    `E0206` for a type term naming another module's non-shared struct and
    for another module's non-shared global.
  * The summary: a program of 256 modules with 4,000 globals and calls
    analyses in time linear in calls and uses (a counting test, not a
    timing one).
  * Codegen snapshots: a header and a `.cpp` for each row of §1.7; include
    order; forward declarations; a non-shared global reading a shared
    global of its module (the shared one first, the anonymous namespace
    reopened); a non-shared function using a shared `const` constant as a
    `case` label; an enum's print support declared right after it and
    defined without `inline`; with g++, a printable shared struct with a
    field of a shared enum of the same module, printed and joined from
    another module; each header with a field default naming another
    module compiled alone.
  * Properties extended with structs (by value, in lists, with and without
    defaults, ticked), constants and globals: the four properties of F,
    the header invariant over edits that keep each shared constant's form.
  * E2E `modules-types.e2e.ts`: a shared struct in `records` is offered in
    `main`'s type menu (under *records* once the carve-out landed);
    unticking *share* gives `E0206` on its use and `E0214` on a shared
    function that takes it.
  * `gradebook` and `inventory` golden, run and exported.
* **N1.** The D2 spike's record; placement in hand-edited files (a
  namespace in a function body, a statement in a mouth, *use namespace* in
  a mouth, a definition stacked under a top-level definition: each
  `E0604`); namespace symbols and nesting; `E0219` at 15 levels, none at
  14; `E0210` for a namespace named like a non-shared function of another
  module; the field value `::std` against a user namespace whose symbol ID
  is `std`; `std_names.rs` fresh for the local g++ (a test runs the tool in
  check mode when g++ is present) and the CI GCC 15 step; TypeScript: the
  namespace block takes definitions and refuses statements, a definition
  snaps in and out, a drop deeper than 14 is refused.
* **N2.**
  * Analyser: `geo::area` in the scope query; one namespace in two modules;
    `E0210` for a name taken in a namespace; `E0213` for a non-shared member
    meeting a shared member through a header; `E0215` for `list`, `pair`,
    `chrono` only with a directive and only in the project's standard;
    `I0523` for each redundancy case of 06 §6.14.3 rule 5 that M3 can
    build; `E0203` for a namespace the file cannot see; the lookup model's
    spellings for each case of 06 §6.14.2 and §6.14.6 without classes (a
    user `max` called with doubles under a directive is `::max(…)`; a user
    `count` global with `std::count` kept; ADL into a user namespace).
  * Directives: *use namespace [geo] in this file* where `geo` has only
    non-shared members in this module, in a multi-module `.cpp` and in a
    single-module `main.cpp`, prints `namespace geo {}` first and compiles.
  * The case programs of ADR-0006's studies that need no class, pack or
    Raw C++ become regression tests (compiled and run under C++17 and
    C++20, with the directive and without, giving the same output).
  * A differential test: every example and every catalog block's default
    C++ generated with Textbook style off and on compiles and gives the same
    run output (06 §6.14.13).
  * The *use namespace* menu lists `std` and top-level namespaces only.
  * E2E `namespaces.e2e.ts`: put a function in a namespace block, call it
    from `main` and see `geo::area(…)`; add *use namespace [geo] in this
    file* and see `area(…)`; turn Textbook style on in the settings page
    and see `using namespace std;` and `cout`; a struct named `list` then
    shows `E0215`; turning it off removes it.
  * `shapes` and `textbook_grades` golden, run and exported.
* **C.** As modules.md §12 *Build*: after an edit inside one function
  exactly one unit compiles; a reused unit's warning is still reported; a
  change to a shared definition recompiles every unit (the key hashes every
  header, modules.md §5.3); an error in one unit, fixed, recompiles only
  that unit; a cancelled build keeps the cached objects and removes its
  temporaries by their absolute paths; an unreadable or oversized message
  file is a miss; `obj/` keeps only the last successful build's files;
  the link argv with `../obj/<stem>-<key12>.o` for 256 modules stays under
  26,000 characters.
* **S.** `use namespace … here` regions and `I0523` inside another region;
  `E0703` for `std::ranges` in C++17; `use header of module` adds the
  include and `E0607` after the module is deleted.

### 5.3 The integration step

After R3 (or R4), on the merged branch:

1. Regenerate every generated file (§3.3) and check nothing changed;
   `std_names.rs` from every local `g++-N` plus the CI GCC 15 list.
2. Remove the `dead_code` allow on the new codes (§1.9).
3. The full checks of §5.1 with g++; the every-g++ forms test with the
   three-module program of §4.2; the assignments test printing `M3
   assignments: 10 of 10 green`; every earlier golden file byte-identical.
4. The carve-outs W4 or W5 declined (§3.3), and W5's follow-ups: the
   paste precedence in `clipboard.rs` (and the kept symbol ID, if W3M-D12
   (b)); `helper_placement` dropped from `CodegenOptions`, `frontend.rs`,
   `facade.rs` and the benchmark, after W5's branch merged; the worker's
   scope index checked to include other modules' shared symbols, with a
   test; the native benchmark gains the 8-module case of modules.md §12
   (reported next to the 1,000-block single module).
5. The full Linux E2E suite at the default size, 1024 × 768 and 800 × 560,
   and the visual diff. Expected to change, and rebaselined from the CI
   candidates: every chrome and canvas scene with a project open (the strip
   from one module on, W3M-D7; for example `canvas-guessing-game`,
   `chrome-window-smallest`); `chrome-toolbox-program` and its dark
   version (N's block); every definition render if D2 falls back to a notch
   everywhere. New baselines for the module strip (one, two and five
   modules, light and dark).
6. The A/B job on the wave's range shows no gated regression in
   `native.pipeline.preview`, `webview.preview-1000.p95`,
   `webview.edit-1000.p95-all` and `webview.drag-5000.frame-p95-all`;
   `webview.drag-typed-1000.frame-p95-all` is gated here if it has its 5
   baselines (W2 §5.3 handed it on).
7. The fuzz targets' dictionaries gain the new block types and a
   multi-module seed; computing the layout never panics and stays linear
   in declarations and uses; the security corpus gains a project with 256
   modules, names at the limits (`a`, 64 characters, `module-2`, a
   device-like `com0x`), a 14-deep namespace nesting and a refused 15-deep
   one, each wrong placement of §2.10 (`E0604`) and a *use namespace*
   value `std` beside a namespace with that symbol ID.
8. A nightly case builds a generated 256-module project on Linux and
   Windows, and exports it and builds it with `make` under MSYS2's shell
   (`#[ignore]`d in the normal run, run by the nightly job).

### 5.4 The docs step

One agent, after the integration step, applies the packages' *Spec
changes* lists and these:

* **The spec**, the rows of modules.md §13 for W3 (03 §3.7.1, §3.7.7,
  §3.7.13, §3.8, §3.9; 04 §4.2, §4.3, §4.13; 05 §5.6, §5.12; 06 §6.1, §6.5,
  §6.7, §6.9, §6.10, §6.11, §6.14.7, §6.14.11; 07 §7.5.1 to §7.5.3, §7.6.3;
  08 §8.4.1, §8.6; 09 §9.2), plus: 03 §3.7.13 and 06 §6.14 (what M3 built
  of the lookup model and what waits for M4; "until this model lands in
  M3" removed; §6.14.7 says `E0213` follows includes, or applies
  program-wide per W3M-D10; §6.14.8 the GCC 15 check and the newer-toolchain
  rule; §6.14.12 Textbook style without the preview in M3), 04 §4.1 (the
  project name on the page; the module strip, its keyboard model and
  *Undo delete*), 05 §5.3 (Textbook style readable and written; the
  `namespace` field kind's `::` values), 05 §5.4 (definitions in a
  namespace's mouth), 10 §10.1 (the W3 rows done, the project name item
  closed).
* **The contracts:** modules.md §2 (the `E0213` claim corrected: it is
  reached whenever the `.cpp` includes a header that shares the name, for
  any reason; its message), §3.3 (the widened field and constant bullet; a
  variable's type; a shared constant's form as a header part), §3.6 (the
  invariant as §2.4 states it), §7.3 (`program.use_module`), §8 replaced by
  a pointer to w3-export-run.md §4 and this file's §2.8 (what was built),
  §11 (W3 rows done), §12 (tests moved here); types.md §6 (codes first
  emitted, `E0219` taken), §5.1 (a shared constant's form is a header
  part); w2.md §1.10 (namespace done); w3-export-run.md §4 (manifest
  format 3, Replace keeps edited files); ADR-0019 point 6, only if the
  owner changes it (W3M-D10); this file's status and §7.1.
* **Next steps** §3.4 (W3 done), a §3.9 for W3's record, §3.8's
  *Remaining* line for W3; W4's row gains the type field's module grouping
  if W4 declined the carve-out.
* `docs/reference/diagnostics/analyser.md` and `generator.md` checked
  against the code; `CHANGELOG.md` with the compatibility note of §2.13
  (and, unless W3M-D11 is accepted, that M2 shows a *define* written by M3
  as a placeholder); `examples/README.md`.

## 6. For the owner to confirm

| ID | Question | Recommendation |
| --- | --- | --- |
| W3M-D1 | How W3 edits files the W4 and W5 cloud sessions own | Ask each session to agree to the carve-outs of §3.3 (named files, named changes) before R0 (`bdm.ts`) and R1 (the rest), so E, T and N land their editor parts in their own rounds; whatever is declined moves to the integration step after that session's branch merges. The alternative, waiting for W4 to merge before any editor work, idles one of the two agents through R1 |
| W3M-D2 | The shape of the namespace block | A container (`namespace geo { … }` as courses write it) whose mouth takes definition blocks, which gain a `definition` connection (they show a notch inside a namespace and keep the hat look at the top level only if Blockly allows it). A half-day spike at the start of N1 settles: how a definition renders with and without a previous connection; how a definition inside a mouth is stored (05 §5.4); that a definition dropped under a top-level definition is refused (never a loose stack); the placement rules of `resolve.rs` and `E0604` for hand-edited files (§2.10); and what `collect.rs` and `query.rs` need. If the hat cannot be kept, definitions show a notch everywhere and the visual baselines change once (§5.3 step 5). If the spike is not clean within its half day, take the fallback, a *namespace* field on the five definition blocks, which needs only catalog fields and no new connection, but reads less like C++ |
| W3M-D3 | Which namespace parts are must-have | None: M3-D6 names multi-module projects as must-have, not namespaces, and no assignment needs them. Recommendation: N (the namespace block, *use namespace … in this file*, *Textbook style*, the lookup model) is the first if-time item (if 1), planned in R2 and R3 ahead of the cache (if 2) and the sweep S (if 3); when the budget is spent it moves to M4 whole, since Textbook style without the lookup model would break programs silently |
| W3M-D4 | The object cache | If-time (if 2), as ADR-0019 owner point 4 accepted: without it a multi-module build recompiles every unit in parallel, as M2 does |
| W3M-D5 | Overloads of a shared function in several modules | Refused (`E0211`): an overload set lives in one module, because a call's meaning would otherwise change with the calling file's includes (§0's check). A later wave can allow it with every module's header included wherever any is |
| W3M-D6 | Undo of module edits | Adding, renaming, moving and deleting a module are edits of the project (they mark it unsaved), confirmed in a dialog, and not on the canvas's Undo, which every module switch clears. Deleting keeps the deleted module (ID, name, workspace) in memory and offers *Undo delete* in the status message and in `≡` until the next module edit or the project closes; restoring puts it back at its place with its IDs, so other modules' references bind again. Every module edit is refused while the canvas holds an unchecked change. A project-level undo is not planned |
| W3M-D7 | The module strip with one module | Shown from one module on (⊕ must be reachable to add the second), with the single module's tab; the default size and 800 × 560 lose one strip's height of canvas, and every chrome and canvas baseline changes once. The alternative, ⊕ only in the `≡` menu while there is one module, keeps the canvas but hides how modules start |
| W3M-D8 | `b2c_support.hpp` and rebuilds | The support header holds only the helpers some file uses, so a body edit that first uses a helper changes it and, with the cache, recompiles every unit. Writing every helper in every multi-module project would avoid that at the cost of a longer header; recommended to keep "only what is used", which is what students read |
| W3M-D9 | *Move left* and *Move right*, and the program name after reordering | The moves are an addition to ADR-0019 §9 and modules.md §7.2 (which list add, rename and delete): they let a student choose which module names the program and the file order. The program keeps being named after the first module (as now); the moves say so in their tooltip. The alternative, naming it after the module with *when program starts*, changes M2's rule; dropping the moves leaves the name fixed by creation order |
| W3M-D10 | `E0213`: follow includes (ADR-0019 point 6, as C++ allows it) or apply it program-wide | Point 6 holds only while a module's `.cpp` does not include the other header; once it does (for any use), C++ needs the non-shared name changed, so `E0213` appears on a definition the student did not touch. **Recommended: program-wide** (a non-shared name may not be any module's shared name in the same namespace, the alternative ADR-0019 listed): it never appears as a side effect of another edit, is simpler to build and test, and costs only names a student could not reuse anyway once the headers meet. If the owner keeps point 6, F builds §2.1's include-following rule with the message that names the cause. Answer before R1; a change amends ADR-0019 point 6 in the docs step |
| W3M-D11 | Writing `SHARED` on every new *define* | Under 03 §3.11.3, every *define* M3 creates would carry `SHARED: false`, so M2 shows every function of an M3 project, single-module ones too, as a placeholder from a newer catalog and the program no longer builds there (students who use M3 at home and M2 at school). **Recommended:** the editor writes `SHARED` only when it is true (a catalog flag for this field, P0; E's change in `workspaceToBdm.ts`), so single-module projects and multi-module ones that share nothing stay M2-readable, and a project that shares a function reports `E0602` in M2, as intended. If kept as is, §2.13 and the CHANGELOG say so |
| W3M-D12 | Moving a definition to another module | Splitting a module is the main reason to use modules, but cut and paste gives pasted declarations fresh symbol IDs, so every call from another module breaks (`E0201`), and *Move to module* is W4 if-time. Options: (a) a minimal *Move to module…* in E (a document operation keeping block and symbol IDs, confirmed in a dialog), which needs a block context-menu entry in W4's files; (b) paste keeps a declaration's symbol ID when no symbol with that ID is in the document, so cut then paste keeps callers bound (W5's `clipboard.rs`, in the integration step or by W5 on request). **Recommended: (b)**, smaller and useful beyond modules. Until either lands, `E0211`'s text says only *rename* |
| W3M-D13 | Textbook style without its preview in M3 | 06 §6.14.12's preview and *Rename all and turn on* need a WASM call that analyses with changed options (W5's facade), a dialog behind W4's settings page and an undo step in W4's session. **Recommended:** in M3 the switch applies as one project edit with a note, the problems it causes appear in Problems, and turning it off removes them; the preview moves to M4 (§1.8), recorded in 06 §6.14.12 by the docs step. The alternative is three more carve-outs in N2's round |

## 7. Review record

The three reviews of this contract and what became of each finding: K for
the compiler and build review, P for the scope, packaging and order review,
U for the students and safety review. *Kept* means the finding held and
the change is in the section named.

| Finding | Outcome | Where |
| --- | --- | --- |
| K1 a non-shared enum's `b2c::to_text` inside the anonymous namespace | kept: always at global scope, namespaces closed and reopened; P0 snapshot and g++ case | §0, §1.7, §2.12, §3.1, §5.2 P0 |
| K2, U5 `E0213` reached by unrelated uses | kept: claim corrected, message names the cause with related locations, F tests for unrelated and transitive uses, codegen `::name` guard; program-wide alternative put to the owner | §1.9, §2.1, §5.2 F, §5.4, §6 W3M-D10 |
| K3 relative object paths and `remove_outputs` | kept: `Step` keeps the absolute path; deletion never uses the argv; decoy test | §0, §2.7, §2.10, §3.2 B, §5.2 B and C |
| K4 shared enum print support: not `inline`, right after the enum | kept: one graph node, defined without `inline`; T snapshot and g++ case | §1.3, §1.7, §2.2, §2.3, §5.2 T |
| K5 directive before its namespace exists | kept: empty declaration step in both orders; N2 tests | §1.4, §1.7, §2.12, §5.2 N2 |
| K6 include rule misses field defaults naming functions, constants, globals | kept: the bullet widened; T cases compiled alone; modules.md §3.3 in the docs step | §2.4, §5.2 T, §5.4 |
| K7 automatic `constexpr` makes a constant's form a header part | kept: form is a header part, invariant and property limited to edits that keep it; a value naming something non-shared prints `extern const`, no `E0214` | §1.3, §1.9, §2.1, §2.4, §5.2 T |
| K8, P4 the `Makefile` misdescribed; B given examples that arrive later | kept: one command described, cmd.exe length recorded with a README line; B tests a hand-built project; each example's package adds it to the export test. Per-object rules were not taken: they shorten no link line, which is the length that matters | §0, §2.8, §3.2, §5.2 B |
| K9 pruning wording | kept: "not among this build's generated files"; no-edit build test | §2.7, §5.2 B |
| K10 a non-shared definition named like a namespace | kept: `E0210` program-wide for namespace names; N1 test | §1.4, §1.9, §2.1, §5.2 N1 |
| K11 one default-value cycle gives `E0421` and `E0422` | kept: one code per cycle; F's `E0422` test changed and an `E0421`-only test added | §1.9, §2.2, §2.4, §5.2 F |
| K12 `E0214` omits a variable's type | kept | §1.3, §1.9, §2.4, §5.2 T |
| K13 link length measured on g++'s argv only | kept: 26,000-character bound, nightly Windows build as proof, response file as fallback | §2.7, §5.2 B and C, §5.3 |
| K14 small inconsistencies | kept: "before its definitions"; the example complete and in name order; C's test says every unit | §0, §1.7, §2.3, §5.2 C |
| P1, U1 *Textbook style* is refused by the loader | kept: P0 makes the key readable and writable, rewrites the two tests, adds the TypeScript field (a W5 carve-out) | §0, §1.8, §2.9, §3.1, §3.3, §5.2 P0 |
| P2, U8 the container needs catalog, format and analyser changes; placement in hand-edited files | kept: the D2 spike widened to the format and the analyser, with a fallback deadline; N1 owns `schema.rs`, `resolve.rs`, `stack.rs`, `collect.rs`; placement rules stated; corpus cases | §0, §2.10, §2.12, §3.2 N1, §5.3, §6 W3M-D2 |
| P3 T and N in parallel edit the same files | kept: N split into N1 (R2, no file of T's) and N2 (R3, after T, owning the codegen lowering files and `emit.rs`), one branch merged once; C moved beside N2 | §2.12, §3.2, §3.4 |
| P5 golden-test changes without an owner | kept: F owns `golden.rs`; §4.2 says Debug, any warning fails | §3.2 F, §4.2, §5.2 F |
| P6 namespace rows labelled must | kept: N is *if 1*, before C and S; D3 reworded | §1.1, §1.4, §3.4, §6 W3M-D3 |
| P7 Textbook style's preview falls on W4 and W5 files | kept by narrowing: no preview in M3, put to the owner | §1.4, §1.8, §2.12, §6 W3M-D13 |
| P8 the standard-name table cannot be built from local compilers | kept: GCC 15 check step in CI, merged by the integration step; the probe to M4 with the incomplete-table rule | §1.8, §2.12, §3.2 N1, §5.3 |
| P9 type menus need a W4 file | kept: carve-out to T on the type field or W4's picker; E2E checks only the offer until then | §1.3, §1.8, §3.3, §5.2 T |
| P10 `SHARED` shown everywhere in R1 | kept: E shows it on `func.define` only and lays blocks out again across 1 and 2 modules; T turns it on for struct, enum, global | §3.2, §3.3, §5.2 E |
| P11 visual baselines not planned | kept: the changing scenes listed; an 800 × 560 E2E check | §5.2 E, §5.3, §6 W3M-D7 |
| P12 Windows `make` and the silent skip | kept: the UCRT `make` package (its `mingw32-make.exe` is in `ucrt64\bin`) and `B2C_REQUIRE_MAKE=1`. MSYS's own `make` was not taken: `usr\bin` on `PATH` would put coreutils' `link` ahead of the MSVC linker that cargo uses | §3.2 B, §4.2 |
| P13, U6 the last module and the 256-module limit | kept: disabled with reasons, checked in the document operations, *when program starts* named; Vitest cases with a 256-module fixture | §1.2, §2.9, §2.11, §5.2 E |
| P14 M3-5's compact map must carry the module | kept: a W5 coordination row; the scope-index row reworded with an integration test | §2.5, §3.3, §5.3 |
| P15 examples must use every non-shared definition; 24 examples | kept: the rule and where each is used; 24 | §1.7, §4.1 |
| P16 small corrections | kept: `E0703` first emitted, in `generator.md`; `E0203` for N; the moves marked as an addition (D9); the `toolbox.toml` comment to P0; the globals branch of `resolve.rs` to T; IDs `program.namespace`, `program.use_namespace`, `program.use_module` (no `org.` block exists) | §1.2, §1.9, §1.10, §2.9, §3.1, §3.2, §6 W3M-D9 |
| U2 deleting a module cannot be undone; edits during an unchecked change | kept: *Undo delete* until the next module edit or close; every module edit refused in the banner state | §1.2, §2.11, §3.3, §5.2 E, §6 W3M-D6 |
| U3 no supported move between modules, yet `E0211` advises one | kept: `E0211` says *rename* only; the move put to the owner with a recommendation | §2.1, §1.8, §3.3, §6 W3M-D12 |
| U4 Replace has no hash check | kept: manifest format 3 with SHA-256; edited or unknown files kept and named | §0, §2.8, §2.10, §5.2 B, §5.4 |
| U7 no 14-level limit exists | kept: `MAX_NAMESPACE_DEPTH` in P0 with a const assertion, `E0219`, the editor refuses the drop, 15-deep corpus case | §0, §1.9, §2.10, §2.12, §3.1, §5.3 |
| U9 the strip's keyboard model | kept: roving tab index, one *Module actions* menu, Shift+F10, *All modules…* with type-ahead; C++ tab lists the shown module's files first | §1.2, §2.5, §2.11, §3.3, §5.2 E |
| U10 the namespace field's value is ambiguous | kept: closed names stored with a leading `::`; codegen prints only resolved names; corpus case | §2.9, §5.2 N1, §5.3 |
| U11 a second module changes `main.cpp` | kept: §2.13 widened; status message; P0 snapshot | §1.2, §1.7, §2.11, §2.13, §3.1 |
| U12 `SHARED` on every new *define* | kept as an owner decision, recommending it be written only when true | §2.9, §3.1, §3.3, §6 W3M-D11 |
| U13 README title, save path, `E0221` labels, dialog checks | kept: all four | §1.2, §1.5, §2.10, §5.2 E |

### 7.1 Corrections made with P0 and the packages

Where P0's code, or a package's, shows this contract wrong, the contract is
corrected in place and the correction is recorded here with its reason.

| Correction | Reason | Where |
| --- | --- | --- |
| `SymbolInfoKind::Namespace` and its IPC mirror move from P0 to N1; `SymbolInfo::new` returns `None` for a namespace until then | `crates/b2c-core-wasm/src/clipboard.rs` and `scope.rs`, W5's files, match `SymbolInfoKind` without a wildcard, so a new variant breaks W5's build; no symbol is a namespace before N1 | §2.9, §2.12, §3.1, §3.2 N1, §3.3 (P0) |
| A reference's `Spelling` is kept in `Program::spellings`, keyed by the referring block and the symbol, not as a field of each SAST reference | a field on every reference node would change every constructor and the SAST snapshots; a table beside the SAST, like the layout, is not serialised and is empty until N2 fills it | §2.12 (P0) |
| `Layout::flat` gives `module<N>` also to a module whose stem an earlier module already has, and code generation names files only by `ModuleFiles::stem` | `source_path`'s own de-duplication went with it (§3.1); the loader refuses such names, but code generation must never write two files with one name | §3.1 (P0) |
| `emit::header_file` prints a shared enum's definition only (its `operator<<` and `b2c::to_text` are T's, §1.7) and a shared global as `extern` or `inline constexpr`; a `.cpp` leaves out what its header declares, but still defines a shared global that is not `constexpr` | what P0 prints for each kind `header_order` can list; F and T refine it | §3.1 (P0) |

#### From the W4 and W5 merges

W4's branch (`w4-editor-tools.md` §11) and W5's second part
(`w5-performance.md` §10) merged before R1. What they ask of W3, and the
package that takes each:

* **E** (W4 §11 row 3): `blockDefinition` in `blocks/register.ts` now
  caches one layout per label mode; showing `SHARED` keys that cache by
  the label mode **and** `services.multiModule()`. When the module count
  crosses 1 and 2, the definition blocks are laid out again through
  `EditorSession.relabel()` (as `installLabelMode` does on a label
  change), not a second rebuild path, and `multiModule` goes into the
  toolbox's `dynamicContentsKey` beside W4's `modeKey`. T does the same
  for struct, enum and global in R2.
* **N1** (W4 §11 row 8): `program.namespace` and `program.use_namespace`
  each get a gating decision in `editor/gating/table.ts` (`BASIC_BLOCKS`,
  W4's recommendation, or a `REQUIREMENTS` row if 03 §3.7.1 makes them
  advanced; the audit test fails for a block with neither, and for a new
  checkbox not in `BASIC_FIELDS`), and their words (`namespace`,
  `using`) in `editor/search/synonyms.ts`. S does the same for its
  blocks.
* **E** (W5 §10.3): `AnalysisState.scope: ScopeIndex | null`, set by the
  preview pipeline; `analysisScope` in `services/symbols.ts` reads it and
  falls back to the core when it is `null`, and the core host's view of
  the index goes.
* **E** (W5 §10.4): the hold's one line, `isBusy: () =>
  isDragging(this.workspace)` in the pipeline options of
  `EditorSession`'s constructor, beside E's module operations in
  `sync/session.ts`.
* **F** (W5 §10.6 row 5): if F makes other modules' shared symbols
  visible through `ScopeIndex::visible`'s function lists or a list per
  module, `ScopeIndexParts` (`query.rs`, appended after `impl Analysis`)
  gains one method, `crates/b2c-core-wasm/src/scope_index.rs` one key and
  the package's lookup one step; the lookup tests compare with `scope_at`
  for every block. The integration step's test (§5.3) checks the worker's
  index includes them.
* **The integration step** (§5.3):
  * W5 §10.1, lowering parked stacks (M3-4): W5's exact proposal for
    `lower/mod.rs`, `scope.rs` and `query.rs`, after N2, the last
    package to edit those functions.
  * W5 §10.2, the clipboard arguments of worker.md §5: only if the
    `copy-1000`, `paste-1000` or `duplicate-1000` benchmarks are over
    100 ms with the worker on; with the `clipboard.rs` row of §3.3.
  * W5 §10.5: `pnpm test:tools` (`node --test tools/`) in a CI frontend
    job, B's `ci.yml` having merged in R0.
