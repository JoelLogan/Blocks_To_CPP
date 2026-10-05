# 6. Translation Pipeline: Blocks → C++

> Status: **Draft v0.1** · Crates: `b2c-ir` (shared types), `b2c-model`, `b2c-catalog`, `b2c-lang`, `b2c-codegen` · Related ADRs: [0003](../adr/0003-rust-core-native-and-wasm.md), [0010](../adr/0010-wasm-delivery-under-the-csp.md)

## 6.1 Overview

```text
 .b2c JSON / IPC payload / clipboard
        │
  ① Load & validate            b2c-model     limits, schema, UTF-8/text rules, migrations          → Document
  ② Resolve catalog            b2c-catalog   block types, versions, packs, field/extra validation  → ResolvedDocument
  ③ Lower                      b2c-lang      blocks → Semantic AST (SAST), expression parsing      → Sast
  ④ Resolve names & scopes     b2c-lang      symbol table, scope tree, reference binding           → Sast + Symbols
  ⑤ Type-check & lint          b2c-lang      gradual types, overloads, flow checks, lints          → TypedSast + Diagnostics
  ⑥ Desugar & order            b2c-codegen   SAST → C++ AST (CAST), dependency ordering, header split → Cast
  ⑦ Emit                       b2c-codegen   pretty-print, includes, helpers, source map            → GeneratedProject
```

The types passed between stages (the SAST, symbols, typed text leaves,
diagnostics and source maps) live in `b2c-ir`, so each stage depends only on
the contract it consumes, not on the crate that produced it.

Every stage is a **pure, deterministic function**: no I/O, no clock, no
randomness, and no global mutable state. The same code runs natively in the
backend and CLI, and as WebAssembly in the editor. A stage that finds errors
still produces output where it can, so the editor can always show as much C++
as possible. Building, however, requires stages ①–⑤ to report zero errors.

**The live preview is best-effort.** Once stage ① succeeds, the editor's
preview (`b2c-core-wasm`) continues through catalog and analyser errors and
always returns C++, with `/* error */` placeholders where a part could not be
generated, and `buildable: false`. Only a load failure gives no files. The
analyser must never panic on a document whose resolution failed; a test runs
it on every file of the security suite that loads, and on the examples with a
dangling reference added. The preview
composes the stages exactly as the build does (`b2c_build::run_frontend`):
the same generator options, with the workspace's app version, the banner, the
inline helpers and the indent width from the settings. Two tests keep them
equal: the preview's files must match the golden files of `tests/golden/`,
and `b2c-app` compares the preview's files with the build's `gen/` files for
every example at indent widths 2 and 4.

**Core invariants:**

1. **Only stage ⑦ produces C++ text,** and it accepts only typed CAST nodes.
2. **User-provided text reaches C++ through a validating type:** `Ident`,
   `StrLit`, `CharLit`, `NumLit`, `Comment` or `RawCode`. There is no
   `String`-to-output path ([§6.8.2](#682-typed-text-parse-dont-validate)).
3. **Output is re-printed from ASTs, never spliced from input strings.** The
   only exceptions are Raw C++ blocks and trusted catalog templates, and both
   carry their provenance into the source map so the UI can mark them.
4. **Determinism.** The same document, catalog version, options and
   toolchain capabilities (including any probed standard names,
   [§6.14.8](#6148-the-standard-name-table)) always produce byte-identical
   output. This is enforced by tests that generate
   twice, and in different block orders, then compare.

## 6.2 Stage ① Load and validate

* A strict JSON parser of our own that **rejects duplicate keys** at every
  level (reporting where they are), enforces the depth limit and a cap on the
  number of values while parsing, and gives a line and column for every
  error. The limits from
  [05 §5.6](05-project-format.md#56-validation-limits) are checked before
  allocation-heavy work (the size check happens before parsing).
* Strongly typed structs with `deny_unknown_fields`. Field values are decoded
  into enums and newtypes, never left as untyped JSON. The decoder reports
  every problem it finds, not just the first.
* Format migrations (the `formatVersion` chain) run on the parsed JSON tree
  before decoding, because an older file may not fit the current types. The
  upgraded tree then goes through the full decoder and validation.
* Output: `Document`, plus load diagnostics (`B2C-E01xx`).

## 6.3 Stage ② Resolve catalog

* Each block's `type@v` is looked up in the catalog (core + std pack + project
  packs). Older versions are migrated. Unknown types produce a placeholder
  "missing block" (shown greyed out with a *Missing pack: X* badge, and
  preserved on save) plus diagnostic `B2C-E0601`.
* Fields are validated against their kinds: dropdown values must be one of the
  options, numbers must be in range, checkboxes must be booleans, identifiers
  go through `Ident::new`, and text through the text rules.
* `extra` (mutator state) is validated against the block's declared schema,
  and the ⊕ counts must match the inputs present.

## 6.4 Stage ③ Lowering to the Semantic AST

The SAST is a **language-level** tree that sits closer to the blocks than to C++
syntax. Every node carries `origin: Origin { block: BlockId, part: Part }`,
where `Part` is `Whole | Field(name) | Input(name) | Token(range)`.

Main node kinds (abridged):

```rust
enum Item {                               // top-level
    Main(MainDef), Function(FunctionDef), Struct(StructDef), Class(ClassDef),
    Enum(EnumDef), Alias(AliasDef), Global(GlobalDef), Namespace(NamespaceDef),
    ExternFunction(ExternDecl), RawDecl(RawCode),
}
enum Stmt {
    VarDecl(VarDecl), Assign(Assign), CompoundAssign(CompoundAssign),
    If(If), Switch(Switch), While(While), DoWhile(DoWhile), Repeat(Repeat),
    ForRange(ForRange), ForEach(ForEach), Forever(Body),
    Break, Continue, Return(Option<Expr>), ExprStmt(Expr),
    Print(Print), Ask(Ask), FileOpen(FileOpen), ForEachLine(ForEachLine),
    Try(Try), Throw(Throw), Rethrow, Lock(LockBlock), ThreadStart(ThreadStart),
    Template(TemplateStmt),               // library-pack statement
    Raw(RawCode),
}
enum Expr {
    Literal(Literal), SymbolRef(SymbolId), Unary(..), Binary(..), Ternary(..),
    Call(Call), MemberAccess(..), Index(..), Cast(..), Lambda(Lambda),
    Construct(Construct), ListLiteral(..), Template(TemplateExpr), Raw(RawExpr),
}
```

* **Expression slots** are parsed here. The token list
  ([03 §3.4](03-block-language.md#34-expression-slots)) is parsed by a
  hand-written recursive-descent / precedence-climbing parser with an explicit
  depth limit of 64. It produces the same `Expr` nodes that blocks produce, so
  later stages cannot tell a typed expression from a block-built one.
* **Library-pack blocks** lower to `Template` nodes that hold the parsed
  template and their argument `Expr`s ([03 §3.11.2](03-block-language.md#3112-template-lowering-library-blocks)).
* **Disabled blocks** are dropped here, though their symbols are still
  declared for diagnostics.

## 6.5 Stage ④ Names and scopes

* Builds a **scope tree**: project → module → namespace → class → function →
  block statement lists → loop/catch headers.
* Declares each symbol with its kind, visibility (module-local or *shared*),
  and declaration position. In statement lists, a symbol is visible only
  *after* its declaration.
* Binds every `SymbolRef` and verifies it is visible at the reference point
  (`B2C-E0201` not declared, `E0202` used before declaration, `E0203` out of
  scope).
* Checks for duplicate declarations (`E0210`), shadowing (`W0501`), and
  identifier rules ([08 §8.4.1](08-security.md#841-identifiers)).
* Exposes a **scope query API** to the editor:
  `symbols_in_scope(block_id, input) -> Vec<SymbolInfo>`. It feeds variable
  dropdowns, Quick Insert and the member block.

**Scope query.** `b2c_lang::Analysis` answers from an index recorded while
lowering, from the same scope stack the analyser resolves names with, so a
dropdown never offers a symbol the analyser would reject:

* `symbols_in_scope(block, None)`, or with the name of a value input or
  field, gives what is visible **at the block**: declarations before it in the
  same and the enclosing statement lists, a loop's counter inside its body,
  the parameters inside their function, and every function (definition order
  never matters).
* `symbols_in_scope(block, Some(statement input))` gives what is visible at
  the **start of that statement list**, including the loop counter and
  parameters that the block itself declares.
* Declarations in disabled blocks are left out. An unknown block, or one
  outside the SAST (a loose block), gives an empty list. The result is sorted
  by name, then by ID.
* `symbol_infos()` lists every symbol, and `block_types()` gives the static
  type of each value block, which the editor's connection checker uses
  together with `conversion(from, to)`, the analyser's own conversion rule
  (`same`, `widening`, `narrowing`, `boolNumber` or `invalid`).

`SymbolInfo` lives in `b2c-ir`, and its JSON is the same everywhere (WASM, IPC
and tests):

```json
{ "id": "s_guess", "name": "guess", "kind": "variable", "isConst": false,
  "type": "int", "module": "mod_main", "declBlock": "b003" }
```

`kind` is `variable` (with `isConst`), `parameter` (with `mode`: `copy`,
`editable` or `read_only`), `loopVariable`, or `function` (with `params`, the
parameter symbol IDs, and `returns`, a type). Types are `void`, `bool`,
`char`, `int`, `double`, `string` or `error`. The WASM core answers scope
queries from the analysis cached by its last preview.

## 6.6 Stage ⑤ Types, flow checks and lints

* **Types:**

  ```rust
  enum Type {
      Void, Bool, Char(CharKind), Int(IntKind), Float(FloatKind), String, StringView,
      Named(TypeId), Generic(TypeId, Vec<Type>), Param(TypeParamId),
      Pointer(Box<Type>, Cv), Ref(Box<Type>, RefKind, Cv), Function(FnSig), Auto, Opaque,
  }
  ```

* **Inference:** `auto` declarations take the initialiser type. Generic
  library signatures unify (`std::vector<T>::push_back(const T&)`).
* **Conversions:** follow the C++ standard conversion rules for built-ins.
  User conversions (constructors / conversion operators) are considered when
  known. `Opaque` is compatible with everything (deferring to g++).
* **Overloads:** simplified resolution (exact → promotion → conversion). When
  the result is ambiguous, we emit nothing and let g++ decide.
* **Flow checks:** missing `return` on some path (`E0410`), `break`/`continue`
  outside a loop (`E0401`), unreachable statements after
  `return`/`break`/`throw` (`W0502`), use before initialisation for scalars
  (`W0503`), use after `move` (`W0504`), `rethrow` outside a catch (`E0402`).
* **Lints:** integer division into a floating-point context (`W0510`), `==`
  on floating-point values (`W0511`), signed/unsigned comparison (`W0512`),
  `forever` without an exit (`I0513`), raw `new` without a matching owner
  (`W0515`), capturing locals by reference in a detached context (`W0516`),
  and the name-spelling notes of
  [§6.14](#614-standard-names-and-using-namespace) (`I0523`, `I0524`,
  `W0525`). Literal overflow (`E0517`) is an error.
* **Lint levels.** Every analyser warning and info code (`W05xx` and
  `I05xx`, including the flow checks above) can be set to `off`, `info`,
  `warning` or `error`:
  * in the project file (`project.lints`,
    [05 §5.3](05-project-format.md#53-top-level-structure)), for everyone who
    opens the project;
  * in the machine settings (`settings.json`,
    [05 §5.9](05-project-format.md#59-machine-local-data)), for every project
    on that computer.

  The machine value wins when both are set, and a code set in neither keeps
  its default level. The project file cannot lower `W0520` (hidden characters
  in Raw C++), because that warning defends against the file itself
  ([05 §5.3](05-project-format.md#53-top-level-structure)). A code at `error`
  blocks Run and makes `b2c check` fail like any other error; a code at `off`
  is not reported. The analyser takes the effective levels as an input and
  never reads either file itself. Levels never change the generated code, so
  they are not part of the project hash
  ([05 §5.11](05-project-format.md#511-content-hash)).
* **Errors (`E` codes) cannot be turned off or lowered.** An `E` code means
  that g++ would reject the generated code or that the code would not mean
  what the blocks say. That includes literal overflow (`E0517`) and the
  name-spelling errors of [§6.14](#614-standard-names-and-using-namespace)
  (`E0213`, `E0215`–`E0218` and `E0514`). A `W` or `I` code raised to `error`
  can be lowered again, and the machine value still wins.

## 6.7 Ordering and declarations

Users never have to think about declaration order. Per translation unit:

1. **Includes** (sorted: standard headers, then library-pack headers, then
   project headers).
2. **Support helpers** used by this TU (inline section or `#include
   "b2c_support.hpp"`), then the file's `using namespace` directives, if any
   ([§6.14.3](#6143-where-directives-go)).
3. **Declarations**, in one topological order over: forward declarations of
   classes that are only referenced indirectly, type definitions, declarations
   of all free functions and function templates (so call order never matters),
   global variable and constant definitions, and top-level Raw C++
   declarations blocks.
   * A type definition depends on the complete types of its by-value fields,
     base classes and `std::array<T, N>` elements (pointers, references and
     smart pointers need only a forward declaration). It also depends on the
     declaration of every function, variable and constant that its default
     member initialisers, in-class static member initialisers, array bounds,
     template arguments and its methods' default arguments use.
   * A function declaration depends on what its default arguments use; its
     parameter and return types need only forward declarations.
   * A global variable or constant depends on its complete type and on
     everything its initialiser uses.
   * A top-level Raw C++ declarations block depends on every user type,
     function, variable and constant whose name occurs in it as an identifier
     token. A user declaration that uses an external function or type
     (*Declare external …*) depends on the raw blocks that declare that name
     ([§6.14.9](#6149-raw-c)).
   * A by-value cycle produces error `E0420` (*"Player contains a Team which
     contains a Player; make one of them a pointer or reference"*); any other
     cycle produces `E0421`.
   * When several declarations are ready, raw blocks come first (by block
     `id`), so a specialisation such as `std::hash<Point>` directly follows
     `Point`. Then come class forward declarations, constants, types,
     functions and variables, each sorted by name.
4. **Function and method definitions.** Methods are defined inside the class
   body when every type they use is complete at that point and every free
   function and global variable they refer to is already declared
   ([§6.14.4](#6144-emission-order-for-qualified-names)). Otherwise they are
   emitted **out of line** after all declarations
   (`void Player::attack(Team& t) { … }`). A method of a shared non-template class whose body contains a
   `use namespace … here` block is always emitted out of line in the module's
   `.cpp` ([§6.14.3](#6143-where-directives-go)). Free functions are sorted by
   name, and `main` comes last.

Top-level Raw C++ declarations are always emitted at global scope. In a
multi-module `.cpp` the anonymous namespace is closed before a raw block and
reopened after it. When the file has directives, its code always opens with
the anonymous-namespace block that holds them
([§6.14.3](#6143-where-directives-go)), and later blocks still see them.

Ordering never depends on canvas position, so moving blocks around never
changes the generated code.

**Header split (multi-module projects).** A definition marked *shared* (on its
`⚙`) goes into `module.hpp`:

| Construct | `module.hpp` | `module.cpp` |
| ----------- | -------------- | -------------- |
| Shared struct/class/enum/alias | full definition | out-of-line non-template method bodies |
| Shared function | declaration | definition |
| Shared template function / class | full definition | — |
| Shared global | `extern` declaration (or `inline constexpr` for constants) | definition |
| Non-shared anything | — | inside an anonymous `namespace { }` (internal linkage, preventing cross-module name clashes); non-shared members of a user namespace go in `namespace geo { namespace { … } }` |

Headers use `#pragma once`. Single-module projects emit only `main.cpp`
without an anonymous namespace, to keep student code simple.

## 6.8 Stage ⑥–⑦ Desugaring and emission

### 6.8.1 Desugaring SAST → CAST

The CAST is a **syntax-level** C++ AST: declarations, statements, expressions,
types, and the typed text leaves below. Desugaring makes every language-level
construct concrete. For example:

* `Repeat(n, body)` → `for (int <fresh> = 0; <fresh> < n; ++<fresh>)`, where
  `<fresh>` is a readable unique name (`i`, `i2`, … unless taken, or
  `repeat_i`).
* `Print` → a chain of `<<` with per-type adjustments (bool, 8-bit integers,
  enums with auto-generated `operator<<`).
* `ForEachLine` → `std::ifstream` declaration + `if (!stream)` branch + `while
  (std::getline(...))`.
* Precedence is resolved here. `Binary` nodes get parentheses only where C++
  precedence/associativity requires them, plus a short whitelist of clarity
  parentheses (`a & b == c` style traps, mixed `&&`/`||`).
* Control-flow bodies **always** get braces.

### 6.8.2 Typed text: parse, don't validate

```rust
/// A C++ identifier that passed every rule in docs/spec/08-security.md §8.4.1.
/// The only constructor is `Ident::new`; there is no `From<String>`.
pub struct Ident(Box<str>);

/// Arbitrary validated text, escaped by the emitter (§8.4.2).
pub struct StrLit(Box<str>);
pub struct CharLit(char);

/// A numeric literal matching the C++ literal grammar, range-checked for its type.
pub struct NumLit { repr: Box<str>, ty: NumType }

/// Comment text; sanitised by the emitter (§8.4.3).
pub struct Comment(Box<str>);

/// Unchecked code. Constructed only by lowering a Raw C++ block or expanding a
/// catalog template; carries its origin so the source map can flag it.
pub struct RawCode { text: Box<str>, origin: Origin, provenance: RawProvenance }
```

The emitter's write functions accept only these types, so there is no
function in `b2c-codegen` that writes an arbitrary `&str` into the output.
This is enforced by code review and by a Clippy `disallowed_methods`
configuration that bans `Printer::write_str` outside the token module.

### 6.8.3 Pretty-printer

* A Wadler-style document algebra (groups, nesting, soft line breaks) with a
  configurable line width (default 100).
* Style options: indent width (2/4/tab, default 4), brace style (attached
  default, or Allman), pointer alignment (`int* p` default), spaces in
  template brackets (never).
* Output always ends with exactly one newline and has no trailing whitespace.
* A `:` is always followed by a space when the next token begins with `::`
  (conditional operator, range-`for`, `case` labels, base clauses, bit-field
  widths, member initialiser lists): `c ? 1 : ::count`, `for (int v : ::data)`,
  because `:::` lexes as `::` `:`. `<::` needs no space, because C++11 lexes it
  as `<` `::`.
* File header comment (the second line is omitted in *Export*). There are no
  timestamps.

  ```cpp
  // Generated by Blocks2Cpp <version> from project "<name>", module "<module>".
  // Edit the blocks, not this file – changes here are overwritten.
  ```

## 6.9 Source maps

Every emitted CAST node with an `origin` records its output range
(`b2c_ir::source_map`; written next to the build as `sourcemap.json`):

```json
{
  "version": 1,
  "files": [
    { "path": "main.cpp",
      "ranges": [
        { "start": { "line": 3, "column": 1 }, "end": { "line": 18, "column": 2 },
          "module": "mod_main", "block": "b001", "part": { "kind": "whole" } },
        { "start": { "line": 9, "column": 13 }, "end": { "line": 9, "column": 27 },
          "module": "mod_main", "block": "b004", "part": { "kind": "input", "name": "COND0" } }
      ] }
  ]
}
```

Positions are 1-based; columns count UTF-8 bytes to match GCC's column units
(the frontend converts them for display). `end` is exclusive. Within a file,
ranges are sorted by start and properly nested: every item, parameter,
statement (with its comment lines and nested bodies) and expression has one.

* **Diagnostic lookup:** find the **innermost** range containing the location.
  Because ranges nest, the ranges containing a position form a chain, and the
  innermost is the one that starts last. When only a line is known (linker
  errors, stack traces), the outermost range starting on that line (usually
  the statement) is used, or the innermost range covering a continuation
  line. An unmapped location (e.g. inside a support helper) attaches to the
  module.
* **Raw code:** ranges inside Raw C++ blocks map to the raw block plus a line
  offset, so the Raw editor can underline the exact line.
* The editor uses the same map for hover highlighting and click-to-block.

## 6.10 Includes and support helpers

* Each catalog entry and each type in pack metadata declares its required
  headers. The emitter collects the headers of all **used** constructs
  (including those implied by types, e.g. `std::vector` → `<vector>`), then
  deduplicates and sorts them. There are no transitive-include assumptions:
  if `std::string` is used, `<string>` is included even when `<iostream>`
  happens to pull it in.
* Helpers are selected by usage, together with their own header dependencies,
  and emitted per [03 §3.9](03-block-language.md#39-support-helpers).
* Feature gating: constructs that need a newer standard or library feature
  (e.g. `std::format`, `std::print`, `contains`) check the project's standard
  **and** the selected toolchain's probed capabilities
  ([07 §7.3](07-toolchain-build-run.md#73-capability-probing)), and fall back
  to an equivalent older form when one exists. Otherwise they report
  `E0703`, which names the needed standard or GCC version. (`E0701` and
  `E0702` are the generator's own errors: a placeholder in generated code,
  and libraries not yet supported; see the
  [diagnostics reference](../reference/diagnostics/generator.md).)

## 6.11 Export

*Export as C++ project* (UI and `b2c generate --export`) writes:

```text
<chosen folder>/<project-slug>/
├── src/                 # generated .cpp / .hpp (no "do not edit" banner)
├── CMakeLists.txt       # cmake ≥ 3.20, CXX_STANDARD from project, warnings, link libs by name
├── Makefile             # plain g++ build for environments without CMake
└── README.md            # build instructions, required libraries, generated-by note
```

* The exported code is identical to what the IDE builds, minus the IDE-only
  init unit ([07 §7.6](07-toolchain-build-run.md#76-running-programs)).
* Library profile paths are machine-specific and are **not** exported by
  default. An opt-in checkbox adds them to `CMakeLists.txt` as cache
  variables, with a warning.
* Export writes into a new subfolder and refuses to overwrite non-empty
  folders unless *Replace* is confirmed (and then only replaces files it
  previously generated, recorded in a `.b2c-export.json` manifest).

## 6.12 Diagnostics model

```rust
pub struct Diagnostic {
    pub code: DiagCode,             // e.g. B2C-E0201, or GCC-mapped "C:<option or hash>"
    pub severity: Severity,         // Error | Warning | Info, after lint levels (§6.6)
    pub message: MessageKey,        // i18n key + typed args; rendered in the UI
    pub primary: Location,          // block + part (+ token range)
    pub related: Vec<(Location, MessageKey)>,
    pub fixes: Vec<QuickFix>,       // structured BDM edits, applied via the editor's undo stack
    pub source: DiagSource,         // Analyser | Compiler | Linker | Runtime
    pub raw: Option<String>,        // original compiler text when source != Analyser
}
```

| Code range | Area |
| ------------ | ------ |
| `E01xx` | Loading / format / migration |
| `E02xx`, `W05xx` | Names, scopes, lints |
| `E03xx` | Types and conversions |
| `E04xx` | Structure and flow |
| `E06xx` | Catalog and packs |
| `E07xx` | Feature gating / codegen limits |
| `T1xxx` | Toolchain problems (not found, too old, broken install) |
| `C:*` | Compiler and linker diagnostics mapped from GCC ([07 §7.5.3](07-toolchain-build-run.md#753-diagnostics-capture-and-mapping)) |
| `R:*` | Runtime events (crash, uncaught exception) |

Every code has an entry in `docs/reference/diagnostics/` (generated from the
message catalog plus hand-written explanations). CI fails if a code lacks
documentation. A code keeps its letter when a lint level changes its
severity: `W0510` set to `error` is still `W0510`.

## 6.13 Performance in the editor

* Analysis and codegen run per **module** on a debounced change (50 ms).
  Lowered SAST is cached per top-level block by structural hash, so editing
  one function re-lowers only that function. Name resolution, type checking
  and emission run per module. The target is p95 < 50 ms at 1,000 blocks
  ([N4](01-overview.md#non-functional-goals)).
* The WASM module (~1–2 MB, optimised with `wasm-opt -Oz`) is loaded once.
  Data crosses the JS ↔ WASM boundary as compact JSON. If profiling shows
  boundary costs dominate, it can switch to a binary format (`postcard`).
* **Delivery under the CSP** ([ADR-0010](../adr/0010-wasm-delivery-under-the-csp.md)):
  `connect-src` forbids fetching the `.wasm` file, so the build embeds the
  optimised bytes as base64 in a separate JavaScript chunk that the app
  imports lazily from `'self'`. The bytes are instantiated asynchronously
  (`WebAssembly.instantiate` through wasm-bindgen's init), which
  `'wasm-unsafe-eval'` allows; they are never fetched and never put in a
  `data:` URL. After a WebAssembly trap the module is instantiated afresh.
* **Size budget:** at most 2,000,000 bytes after `wasm-opt -Oz`, checked in
  CI; the gzip size is reported only.
* Modules over 2,000 blocks run analysis in a Web Worker.
* **In M2** the analysis runs on the main thread, for the whole module on
  each debounced change, behind an asynchronous API. The per-block cache and
  the Web Worker come in M5, or earlier if the 1,000-block benchmark in the
  webview misses the N4 target.

## 6.14 Standard names and `using namespace`

Generated C++ normally writes every standard-library name in full
(`std::cout`, `std::string`). Two features let a project use `using namespace`
instead, for the look that many courses and textbooks use (decision Q4: off by
default; [ADR-0006](../adr/0006-using-namespace.md)):

* **Textbook style**, a project setting (`options.usingNamespaceStd`,
  [05 §5.3](05-project-format.md#53-top-level-structure)), writes
  `using namespace std;` in every generated `.cpp` file.
* The **`use namespace` block** ([03 §3.7.13](03-block-language.md#3713-organisation))
  does the same for one module (`use namespace [N] in this file`, at the
  module's top level) or from its position to the end of its statement list
  (`use namespace [N] here`, inside a function).

Both change only how names are *spelled*, never what the program does: with or
without them, generated code calls the same functions and prints the same
output. Raw C++ is opaque text, so it is checked (§6.14.9) rather than
rewritten. Every rule below was established with compiled test programs on
g++ 11–14 and clang 18 (with libstdc++ and libc++), C++17/20/23, in three
independent design studies and two rounds of adversarial review (§6.14.13).

### 6.14.1 Terms

* A **directive** is a `using namespace N;` line the generator writes. N is
  `std`, a standard sub-namespace from a closed list (`std::chrono`,
  `std::this_thread`, `std::filesystem`; from C++20 also `std::numbers`,
  `std::ranges`, `std::views`), or a top-level user namespace. (`std::literals`
  is not offered: `std` already brings every literal suffix, and blocks have
  none.)
* A **region** is the part of a file where a directive is in effect. A *file
  region* (Textbook style or a file block) covers the module's `.cpp` after its
  directives. A *statement region* runs from a `use namespace … here` block to
  the end of its statement list, including nested blocks and lambdas written
  after it.
* The **nominated names** at a point are the names that the namespaces whose
  directives are in effect there declare: from the standard-name table
  (§6.14.8) for standard namespaces, and for user namespaces from the symbol
  table plus every identifier token of a Raw C++ block placed inside one of the
  namespace's blocks (such a block can declare members that the analyser cannot
  see).

### 6.14.2 One lookup model decides every spelling

The analyser decides how every reference to a namespace-scope entity
(function, variable, constant, enumerator, type, namespace) is written, using
one model of C++ unqualified lookup at the position where the reference is
emitted: block scopes, the enclosing class and all its bases (except bases that
depend on a template parameter, which C++ never searches), enclosing user
namespaces, the module's anonymous namespace, the global namespace, the
nominated namespaces and, for calls, argument-dependent lookup (§6.14.6). A
reference is written plainly only when this lookup finds exactly the intended
entity (for a function, exactly its overload set). Otherwise it is written:

* `::name` for an entity declared directly in the global namespace or in the
  module's anonymous namespace;
* `geo::name` for a member of user namespace `geo`, with `::geo::name` when
  `geo` itself is contested;
* `Side::left` for an unscoped enumerator;
* for a member inherited from a base class that depends on a template
  parameter, always, because lookup never searches such a base (with a
  directive, a plain `max(a, b)` there silently means `std::max`):
  * `this->m` for a non-static member used in a non-static member function,
    constructor or destructor, or in a lambda that captures `this`;
  * `D::m` otherwise, where `D` is the class being defined (static members,
    static member functions, static data member initialisers, lambdas that do
    not capture `this`);
  * with explicit template arguments, `this->template m<…>(…)` or
    `D::template m<…>(…)`.

**Functions are never left to overload resolution.** A reference to a user,
external or library-pack function whose name is also a nominated name at that
point, or is declared by an associated namespace of an argument (§6.14.6), is
written `::f(…)` or `geo::f(…)`, even when overload resolution would pick the
user's function today: with `using namespace std;`, `max(1.5, 2.5)` calls
`std::max<double>` instead of the user's `int max(int, int)`.

**Classes the analyser cannot see into.** Library-pack metadata lists the
methods and fields a pack class exposes to blocks, not every member it has
([03 §3.11.2](03-block-language.md#3112-template-lowering-library-blocks)
rule 4), and external and opaque types have no member list at all. So a
reference to a namespace-scope function or variable from inside a class that
has, directly or indirectly, a library-pack, external or opaque base is always
written `::f` or `geo::f`, with or without directives. This covers its member
bodies (also out of line), default member initialisers, and lambdas and nested
classes inside it.

The spelling is recorded in the typed SAST and printed by the generator.
Because the same model runs with and without directives, turning Textbook style
off never removes a qualification that is still needed: a global `max` called
from a method of a class that has its own `max` member is `::max(…)` either way.
Locals, parameters, members and template parameters are found before nominated
names and are written plainly. A namespace-scope entity hidden by a local,
parameter or member is written `::name` or `geo::name`, so `E0205` remains only
for a hidden local, parameter or member, which C++ cannot qualify (until this
model lands in M3, `E0205` covers every hidden name). Info `I0524` explains a
`::` once per declaration and offers an optional rename.

In an out-of-line member definition the class name in the declarator is
written plainly unless it is contested. When it needs `::`, the definition
uses a trailing return type (`auto ::Point::mirror() const -> ::Point`),
because `::Point ::Point::mirror()` parses as one qualified name.

### 6.14.3 Where directives go

1. **Never in a header.** No generated `.hpp` and not `b2c_support.hpp`
   contains a directive, and code emitted into a header is in no region (it
   spells every standard name in full). A `use namespace … here` block in any
   body that is emitted into a header is error `E0514`: shared function
   templates (including functions with `auto` parameters), every member of a
   shared class template, and lambdas in initialisers of shared constants, in
   default member initialisers of shared classes or in default arguments of
   shared functions. A method of a shared non-template class whose body
   contains such a block is defined out of line in the module's `.cpp`
   ([§6.7](#67-ordering-and-declarations) step 4), so it is not `E0514`. A
   library pack can put a directive in front of header code, which §6.14.10
   covers.
2. **File directives** (Textbook style and file blocks) are written once per
   namespace per `.cpp`, after every `#include`, after the inline
   support-helper section and after any top-level Raw C++ `#include` block
   (§6.14.9), in a fixed order: `std`, then standard sub-namespaces in list
   order, then user namespaces by name. A file region therefore always covers
   all of the file's code after the helpers.
   * Single-module `main.cpp`: at global scope.
   * Multi-module `.cpp`: never at global scope, but as the first lines of an
     anonymous-namespace block that opens the file's code (`namespace {` then
     `using namespace std;` …). Later code (the module's non-shared
     definitions, its shared definitions and `main`) sees the nominated names,
     because using-directives are transitive. Yet `::name` still reaches the
     module's own non-shared names exactly: qualified lookup stops at the
     anonymous namespace, which declares the name, before following its
     directive (and that is exact only because of `E0213`, §6.14.7). With the
     directive at global scope, `::name` would also find the nominated
     namespace's names for non-shared names (which is why §6.14.10 restricts
     packs that nominate a namespace).
   * Every user namespace that a `use namespace` block of the module names
     (file or statement block) and that no included header declares is first
     declared empty (`namespace geo {}`) right after the helpers, so its
     directive can join the others and a statement directive never precedes
     its namespace. A block naming a namespace declared in another module's
     header adds that header's `#include`.
   * Non-shared members of a user namespace are emitted as
     `namespace geo { namespace { … } }`, never as `namespace { namespace geo
     { … } }`, which would make `geo` ambiguous whenever `geo` also has shared
     members. Anonymous-namespace blocks at global scope hold only the
     module's non-shared global-namespace definitions; they are closed around
     Raw C++ declarations ([§6.7](#67-ordering-and-declarations)).
   * Textbook style writes the directive only in `.cpp` files that include a
     standard header (directly, through `b2c_support.hpp` or through a project
     header, not counting the `<cstddef>` below). An explicit
     `use namespace [std]` block (`in this file` or `here`) in a file with none
     adds `#include <cstddef>`, so the namespace is declared. The IDE init unit
     ([07 §7.6.3](07-toolchain-build-run.md#763-ide-init-unit)) never has one.
3. **Statement directives** are written at the block's position. Every
   control-flow body is braced, so the C++ scope is exactly the block's
   statement list. The namespace a statement block names is always declared
   before any function body (rule 2).
4. **Placement.** `use namespace … in this file` sits directly on the canvas
   (a module's top level), and `use namespace … here` is a statement block, so
   it can only be in the statement list of a function, method, constructor,
   destructor, operator, `main` or lambda. Neither can be in a class or
   namespace block, where a directive would leak into every later reopening of
   the namespace. The block shapes enforce this, and a hand-edited file gets the
   catalog's `E0604` (block in the wrong place).
5. **Redundant blocks.** A `use namespace` block is redundant when, at its own
   position, it is inside that namespace or another source's directive already
   covers it. Sources count in this order: Textbook style (only in files where
   it writes the directive), then the file block with the smallest block `id`,
   then the outermost earlier statement block whose region contains the
   position. A redundant block emits nothing and gets
   info `I0523` (already in use); it takes effect again when its source goes
   away. Examples are a duplicate file block, a statement block inside the
   region of another statement block or of a file directive, and a
   `use namespace [geo] here` block in a function inside namespace `geo`. In
   the source map, the directive line belongs to the source in effect: the
   block, or the project setting (clicking it opens Project settings).
6. **What can be nominated.** Only top-level user namespaces (nominating a
   nested one can silently replace a global during lookup). Library-pack
   namespaces are not offered. `std::numbers`, `std::ranges` and `std::views`
   need C++20 (`E0703` otherwise). A standard sub-namespace adds its header:
   `<chrono>`, `<thread>`, `<filesystem>`, `<numbers>`, `<ranges>`. The block's
   field stores either one of these closed names or the symbol ID of a user
   namespace, never free text; a missing, invisible or nested namespace is
   `E0201`, `E0203` or `E0207`.

### 6.14.4 Emission order for qualified names

A qualified name (`::name`, `geo::name`, `Side::left`) in a template is bound
where the template is defined, and in a region an undeclared `::name` silently
means `std::name`. So every qualified reference is emitted after a declaration
of its entity. Declarations follow the dependency order of
[§6.7](#67-ordering-and-declarations) step 3, which covers function templates,
default arguments, default member initialisers, in-class initialisers and array
bounds. The bodies of in-class methods and class-template members that refer to
a user function or global variable are emitted out of line after all
declarations (§6.7 step 4). The generator checks this and refuses to emit a
qualified reference that precedes its entity's declaration (an internal error,
reported like a placeholder).

### 6.14.5 Spelling standard names

Outside every region, and in all header and helper code, standard names are
written in full. Inside a region that nominates N, the generator writes
`N::A::…::n` as the shortest remaining suffix only when lookup provably finds
the standard entity. For the first component of the shortened name:

1. No declaration in the translation unit other than the entity referred to is
   spelled like it. That covers user declarations of any kind and scope
   (including generator-chosen locals and names declared by included project
   headers), library-pack globals, members of library-pack classes that are
   bases of user classes, and identifiers in Raw C++ blocks of the translation
   unit.
2. If N is not `std` itself, it is not a name that the standard headers declare
   in the global namespace (`GLOBAL_NAMES`, for example `remove`, `rename`,
   `abs`, `floor`). Such a C function joins overload resolution: under
   `using namespace std::filesystem`, a shortened `remove("x")` would call C's
   `remove`, which returns 0 on success and never throws. For N = `std` these
   names are the same entities and are safe.
3. No other namespace nominated at that point declares it.
4. For a call, every argument's associated namespaces (§6.14.6) are `std`, its
   inline and implementation namespaces, or user namespaces: never a pack
   namespace, an opaque type, or a type that depends on a template parameter
   (including `auto` parameters).
5. The name is not used as a value (a function passed by name), is not
   `std::move` or `std::forward`, and the position is not in the body, an
   out-of-line member, a default member initialiser, or a lambda or nested
   class of a class that has, directly or indirectly, a base class not declared
   by user blocks (a standard-library or library-pack class).

Otherwise the full name is written. `std::…` is always valid, because no
declaration, generated or Raw C++, may introduce another namespace named `std`
(§6.14.9).

Library-pack template text ([03 §3.11.2](03-block-language.md#3112-template-lowering-library-blocks))
inside a region is spelled like generated code: identifiers that name pack
globals follow §6.14.2, and `std::` qualified-ids are shortened only for the
built-in `std` pack. A pack template that contains any other unqualified
identifier from the standard-name table fails at pack load unless the pack lists
it as an ADL customisation point.

### 6.14.6 Argument-dependent lookup, with or without directives

An argument's associated namespaces are computed as in C++
([basic.lookup.argdep]), after resolving type aliases: the namespace of its
class, of all direct and indirect base classes and enclosing classes, of the
template arguments of the class and its bases (recursively), and of the
parameter and return types of function types. A call to a user function `f` is
written `::f(…)` or `geo::f(…)`:

* when an associated namespace of an argument, other than `f`'s own, declares a
  function named `f`, including a hidden friend of one of its classes (a friend
  function defined in a class, which only argument-dependent lookup finds):
  standard namespaces, including sub-namespaces and `__gnu_cxx`, per the
  standard-name table; library packs per their metadata, which lists the
  function names and hidden friends of each pack namespace (§6.14.10); user
  namespaces per the symbol table; or
* when an argument's type depends on a template parameter or is `auto`.

This applies whatever the settings, and a call spelled `geo::f(…)` this way
is never shortened. It replaces the `stoi`/`stod` part of `E0220`: functions may
then use those names, and their calls are written `::stoi(…)`. Without it, a
class derived from
`std::vector<std::string>` passed to a user function `data` silently calls
`std::data`, and a user `draw(const lib::Sprite&)` silently loses to the pack's
`lib::draw`.

### 6.14.7 Names that must change

* Error **`E0215`**: a type declared at namespace scope (global, anonymous or a
  user namespace) whose name is a nominated name at a point where it is
  referred to (for example `list`, `pair`, `byte`), and a user namespace whose
  name a nominated namespace declares as a namespace or a type (a class,
  enumeration or type alias, or a class or alias template; for example
  `chrono`, `string`, `byte`), because a name before `::` is looked up among
  namespaces and types. Qualifying type names would need fragile spellings
  (out-of-line definitions such as `auto ::array::origin() -> Point`), and GCC
  silently resolves some of them to the standard entity: class template
  argument deduction picks `std::pair` for the user's global `pair`, and a user
  type named like a standard namespace is taken as that namespace before `::`.
  Member types and user namespaces named like standard functions (`count`,
  `sort`) are never contested, and `E0215` uses only the names that exist in
  the project's standard (so a C++17 project may keep a type named `span`;
  changing the standard re-runs the check and is previewed). The quick fix
  capitalises the name (`List`, `Pair`): apart from `std::FILE`, which user
  code cannot use anyway (`E0220`), and `std::chrono`'s month and weekday
  names, the standard library has no capitalised names. If the name is already
  capitalised or the new name is taken, the fix appends `2`, `3`, … and
  re-validates; when the clash is with a nominated user namespace, the first
  fix offered is *Remove this `use namespace` block*.
* Error **`E0213`**, needed with or without directives: a name that user
  blocks define at namespace scope (not with *Declare external …*) must not
  also be declared by anything else that its spelling `::name` or `geo::name`
  would reach. Those declarations are:
  * for every name in the global namespace or a module's anonymous namespace:
    library-pack globals and the declared names of top-level Raw C++
    declarations blocks (§6.14.9), whatever their signatures (`::draw(depth)`
    cannot separate the user's `draw(int)` from a pack's `draw(double)`), and
    the standard headers' C globals (`GLOBAL_NAMES`; the
    `Ident::check_namespace_scope` rule applies to anonymous-namespace names
    too);
  * for a module's non-shared name, also the module's own shared names (an
    overload set may not be split between shared and non-shared) and the
    shared names of every module whose header is included directly or
    transitively. In a translation unit that includes a library pack that
    nominates a namespace (§6.14.10), the global namespace nominates it too, so
    a non-shared function, variable or constant must also not be named like
    anything that namespace declares (for `std`, the names of the project's
    standard);
  * for a non-shared member of user namespace `geo` (emitted in
    `namespace geo { namespace { … } }`): `geo`'s shared members declared by
    the module's own header or by any header it includes, directly or
    transitively;
  * when building, the names that a header included by a Raw C++ block, or a
    library pack without a complete list of its global names, puts into the
    global namespace (found by the probe of §6.14.9).

  `::name` and `geo::name` reach a user entity exactly only because of this
  rule. Names in different namespaces never clash: a non-shared `geo::count`
  next to a global `count` is fine. Quick fixes: share all overloads (for a
  non-shared name), or rename.
* Names that cannot be renamed never get `E0215`: library-pack globals of every
  kind and types declared with *Declare external type*. When contested they are
  written `::name`, also as the first component of a qualified name (`::byte`,
  `::ranges::sort` for a pack namespace named `ranges`), and class templates
  among them always get explicit template arguments.
* `E0213` replaces the cross-module part of `E0211` once projects have several
  modules (M3): two non-shared functions in different modules may share a
  name.

### 6.14.8 The standard-name table

`crates/b2c-ir/src/std_names.rs`, generated by `tools/gen-std-names.py` like
`reserved_names.rs`, lists every name that each listed standard namespace
declares: the union over GCC 11–15, C++17 to C++26, strict and GNU modes, and
all standard headers, not just the included ones (which names clash depends on
transitive includes, and those differ between GCC versions). Each entry records
its kind (namespace, type, class template, function, object) and the first
standard that has it, for messages. Names are found by compile probes
(`namespace p { using std::NAME; }`, `namespace a = std::NAME;`), never by
parsing headers; probes are run in chunks, and a probe that crashes the
compiler counts as "no". Hidden friends cannot be probed, so the table also
lists, as functions, every identifier that follows `friend` in the
preprocessed headers (a superset, which can only add a harmless `::`). A CI
job per GCC version fails when the probed names are not all in the checked-in
table. Spelling decisions for functions, variables, enumerators and standard
names (§6.14.2, §6.14.5, §6.14.6) use the union over all standards, which can
only add a harmless `std::` or `::`. Type and namespace names, like `E0215`,
use the names of the project's standard, so a type that keeps its name in a
C++17 project (`span`) is written plainly.

When the selected toolchain is newer than the table, a background job (not one
of the 10-second probes of [07 §7.3](07-toolchain-build-run.md#73-capability-probing))
preprocesses that toolchain's standard headers and probes only identifiers not
yet in the table, with its own time limit; the result is cached by toolchain
fingerprint and becomes one of the toolchain capabilities. Until it has
finished, the table counts as incomplete: no standard name is shortened, every
reference to a user namespace-scope name in a region is qualified as in
§6.14.2, and `E0215` uses the built-in table.

### 6.14.9 Raw C++

Raw C++ is emitted verbatim in the region of its position and may rely on the
directive (`cout << …` in a raw statement compiles inside a region). Because the
analyser cannot see into it, these rules apply:

* **Declared names.** The analyser recognises the names a raw block declares
  from its tokens:
  * an identifier at bracket depth 0 of the block (the body of
    `extern "C" { … }` counts as depth 0), not after `::`, `.` or `->`,
    that directly follows an identifier, a type keyword, `using`, `namespace`,
    `>`, `*`, `&` or `,` and is directly followed by `(`, `[`, `=`, `;`, `,` or
    `{`;
  * a name after `struct`, `class`, `union` or `enum`, unless it is only
    forward-declared (`struct Point;`);
  * every macro name of `#define`.

  Recognition errs towards finding too many names, which can only cause an
  `E0213` that a rename resolves.
* Error **`E0217`**: Raw C++ must not produce a using-directive, a
  using-enum-declaration or a using-declaration at namespace or block scope.
  Member using-declarations in a class body (`using Base::Base;`), alias
  declarations (`using T = …;`) and namespace aliases
  (`namespace fs = std::filesystem;`) are allowed. Raw C++ also must not
  declare a namespace named `std` except at global scope (a `std::hash`
  specialisation), and must not reopen a namespace that a `use namespace` block
  nominates. (A raw block placed inside one of that namespace's own blocks is
  allowed; its identifiers count as nominated names, §6.14.1.) The check runs
  on the macro-expanded output when building: the generator preprocesses each
  `.cpp` with the selected g++ and maps line markers back to the raw block. The
  editor checks raw tokens beforehand, including `#define` replacement lists
  and the joined text of adjacent raw blocks.
* **`#include`** is allowed only in a top-level Raw C++ declarations block that
  contains nothing but preprocessor lines (`#include`, `#define`, `#undef`,
  `#pragma` and conditional directives) and comments. Anywhere else it is
  `E0217`, with the quick fix *Move the `#include` lines into a C++
  declarations block of their own*. Such a block is emitted right after the
  generated `#include` lines, before the helpers and the directives, so the
  included file is never inside a region. The analyser cannot see what the
  header declares; it may even contain `using namespace std;`, as many course
  headers do. So in a translation unit with such a block:
  * no standard name is shortened, and every reference to a user
    namespace-scope name is qualified as in §6.14.2, with or without a
    directive (`W0525` explains this on the block);
  * when building, the generator compiles one probe made of the file's
    `#include` lines (generated and raw) followed by
    `namespace b2c_probe { using ::N; }` for each of the module's non-shared
    namespace-scope names N, and by `char N;` for each name the module
    declares directly in the global namespace, in chunks as in §6.14.8. A
    non-shared name whose probe compiles, or a global name whose probe fails,
    is declared or made visible at global scope by an included file. That is
    `E0213`, reported on the declaration and on the raw block (*"`legacy.h`
    also declares `count`"*).

  A translation unit that includes a library pack without a complete list of
  its global names is treated the same way (§6.14.10).
* Error **`E0218`**: a raw identifier (not after `::`, `.` or `->`, and not
  declared in the same raw block) that names a function, called or used as a
  value, whose meaning a directive can change. Inside a region, that is a
  nominated name that is also declared at namespace scope by a user function,
  a library-pack function, a function declared by another Raw C++ block, or a
  C global (`GLOBAL_NAMES`). In a translation unit whose region the analyser
  cannot know (one with a raw `#include` or a pack without a complete list of
  global names), the same applies everywhere to every name in the
  standard-name table. Examples:
  * raw `max(price, cost)` calls the user's function without the directive and
    `std::max<double>` with it;
  * raw `abs(-2.5)` returns 2 without it and 2.5 with it on libstdc++;
  * `transform(…, toupper)` stops compiling.

  Quick fixes insert `::` or `std::`, each shown as a diff and applied after
  confirmation. For a C global only `std::` is offered, because `::abs` names
  different overloads in libstdc++ and libc++.
* Identifiers in raw text count as declarations for §6.14.5 (1), so generated
  code next to them keeps `std::`.
* Top-level Raw C++ declarations are emitted at global scope, never inside an
  anonymous namespace, in the dependency order of
  [§6.7](#67-ordering-and-declarations) step 3.
* Warning `W0525`: raw code inside a region that mentions any other contested
  user name (suggesting `::name`), and raw code outside every region that uses
  a standard function or object name unqualified and does not declare it.

### 6.14.10 Library packs

* Pack metadata lists the pack's global names, the function names in each of
  its namespaces, and the hidden friends of its classes (every identifier that
  follows `friend` in its preprocessed headers). It is generated by compile
  probes when the pack is built and checked when the pack is loaded. Without
  that list, a translation unit that includes the pack is treated like one with
  a raw `#include` (§6.14.9).
* A pack whose headers nominate a namespace (for example a header that contains
  `using namespace std;`, as some course libraries do) is detected at pack load
  and must declare `nominates = ["std"]`. A pack whose headers nominate any
  other namespace is rejected. A translation unit that includes such a pack has
  a std file region from the `#include` on, whatever the settings. That
  directive is at global scope, so:
  * in a multi-module project, a non-shared name in that translation unit must
    not be a standard name (`E0213`, §6.14.7), because `::name` would also find
    the standard entity;
  * project headers are included after pack headers (§6.7 step 1). So when any
    module includes such a pack, the code of every generated header is
    analysed as if it were in a std region: contested references are written
    `::f(…)`, `geo::f(…)` and `Side::left`, and `E0215` applies to the types
    they name. Standard names in headers are still written in full.

### 6.14.11 Names typed in slots

Inside a region, an unqualified identifier typed in an expression slot is
resolved by the same lookup model. If two or more candidates remain where
lookup stops (user or nominated, from any namespaces), it is error `E0216`,
with one quick fix per candidate (`::name`, `geo::name`, `std::name`,
`std::chrono::name`). Explicit `::x`, `std::x` and `geo::x` resolve directly and
are kept. `E0216` is reported only while typed text is being resolved: stored
tokens, and tokens of a slot that the user did not edit, keep their symbol IDs
and are never re-resolved. Re-resolution by name (paste, snippets, Quick
Insert) uses the target's qualified name recorded in the payload (`::count`,
`geo::area`, `std::max`), never its shortened spelling. It binds only to an
entity with that qualified name, of the same kind, that is visible at the new
position (for a module's non-shared name, the target module's own entity);
otherwise the reference is `E0201`, naming the original.

### 6.14.12 Turning Textbook style on and off; export

* Turning it on runs the analyser with the option set before applying it (in
  WASM, per module) and previews the effect: names that must change (`E0215`,
  `E0213`) with suggested names, names that will be written with `::`, and Raw
  C++ blocks whose meaning or compilation depends on the setting (`E0218`,
  `W0525`). *Rename all and turn on* applies the renames, the confirmed Raw C++
  insertions and the option as one undo step.
* Turning it off never creates an error in blocks, unless `W0525` is set to
  `error`. Raw C++ that relies on the directive is listed (`W0525`), with
  *Insert `std::`* (confirmed per match) or *Keep a `use namespace std here`
  block* in each affected function; turning it off without fixing them is
  allowed but shown as breaking the build. Changing the project's C++ standard
  is previewed the same way.
* These previews ignore lint levels ([§6.6](06-compiler-pipeline.md#66-stage--types-flow-checks-and-lints)):
  they list every `E0218` and `W0525` match, even when `W0525` is set to
  `off`.
* Export writes exactly what the build compiles. When any exported `.cpp`
  contains a directive, the README says so, says that headers never contain
  one, and says that names were checked against the standard library of GCC
  11–15 (other standard libraries may declare more names, which can make the
  code fail to compile). The generated `CMakeLists.txt` adds
  `/permissive- /Zc:__cplusplus` for MSVC, whose default mode skips the
  two-phase lookup this code relies on.
* The code view explains on hover why a `std::` was kept (the program also
  declares `count`), why a name has `::`, and why a multi-module file opens
  `namespace { using namespace std; }`. With Textbook style on, block labels in
  C++ mode drop `std::` as the generated code does, while the plain-text
  clipboard rendering ([05 §5.12](05-project-format.md#512-clipboard-format))
  spells names in full so that it compiles on its own.

### 6.14.13 Tests

* Golden files for every example with Textbook style off and on.
* A differential test: every example and every catalog block's default C++,
  generated with the option off and on, and with each nominable standard
  namespace opened by a statement block at the top of each function body,
  compiles with every supported GCC and standard and prints the same output.
  Projects with Raw C++ must also have every raw block whose output differs
  listed by the preview of §6.14.12.
* The compiled cases of the design studies and the two adversarial review
  rounds (about 500 programs: placement, layouts, every kind of name,
  argument-dependent lookup, emission order, Raw C++, library packs,
  multi-module projects) as regression tests on GCC 11–15, with clang and
  libc++ for exported code.
* The build probe of §6.14.9 against headers that declare a module's names,
  that contain `using namespace std;` and that contain `using std::max;`, in
  single-module and multi-module projects.
* A torture test per GCC and standard: every table name that is a valid user
  name (passes `Ident::new` and `check_namespace_scope`) is declared as a user
  global variable, function and enumerator, at global scope and in the
  anonymous namespace of the multi-module layout, and referenced with the
  §6.14.2 spelling from inside and outside the anonymous namespace under each
  directive; everything compiles.
