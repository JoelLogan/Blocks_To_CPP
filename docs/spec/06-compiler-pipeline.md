# 6. Translation Pipeline: Blocks → C++

> Status: **Draft v0.1** · Crates: `b2c-ir` (shared types), `b2c-model`, `b2c-catalog`, `b2c-lang`, `b2c-codegen` · Related ADR: [0003](../adr/0003-rust-core-native-and-wasm.md)

## 6.1 Overview

```
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

**Core invariants:**

1. **Only stage ⑦ produces C++ text,** and it accepts only typed CAST nodes.
2. **User-provided text reaches C++ through a validating type:** `Ident`,
   `StrLit`, `CharLit`, `NumLit`, `Comment` or `RawCode`. There is no
   `String`-to-output path ([§6.8.2](#682-typed-text-parse-dont-validate)).
3. **Output is re-printed from ASTs, never spliced from input strings.** The
   only exceptions are Raw C++ blocks and trusted catalog templates, and both
   carry their provenance into the source map so the UI can mark them.
4. **Determinism.** The same document, catalog version and options always
   produce byte-identical output. This is enforced by tests that generate
   twice, and in different block orders, then compare.

## 6.2 Stage ① Load and validate

* `serde_json` with a custom wrapper that **rejects duplicate keys** and
  enforces the depth limit while parsing. The limits from
  [05 §5.6](05-project-format.md#56-validation-limits) are checked before
  allocation-heavy work (the size check happens before parsing).
* Strongly typed structs with `deny_unknown_fields`. Field values are decoded
  into enums and newtypes, never left as untyped JSON.
* Migrations run on the typed model (`formatVersion` chain).
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
* **Lints** (each individually configurable): integer division into a
  floating-point context (`W0510`), `==` on floating-point values (`W0511`),
  signed/unsigned comparison (`W0512`), `forever` without an exit (`I0513`),
  `using namespace` in a shared header (`E0514`), raw `new` without a matching
  owner (`W0515`), capturing locals by reference in a detached context
  (`W0516`), literal overflow (`E0517`).

## 6.7 Ordering and declarations

Users never have to think about declaration order. Per translation unit:

1. **Includes** (sorted: standard headers, then library-pack headers, then
   project headers).
2. **Support helpers** used by this TU (inline section or `#include
   "b2c_support.hpp"`).
3. **Type definitions**, topologically sorted by *completeness dependencies*:
   by-value fields, base classes and `std::array<T, N>` require a complete
   type, while pointers, references and smart pointers need only a forward
   declaration. A by-value cycle produces error `E0420` (*"Player contains a
   Team which contains a Player; make one of them a pointer or
   reference"*). Ties are broken by name.
4. **Forward declarations** of all free functions (so call order never
   matters) and of classes that are only referenced indirectly.
5. **Global variables**, topologically sorted by initialiser dependencies.
   Cycles produce error `E0421`.
6. **Function and method definitions.** Methods are defined inside the class
   body when every type they use is complete at that point. Otherwise they are
   emitted **out of line** after all classes (`void Player::attack(Team& t) {
   … }`). Free functions are sorted by name, and `main` comes last.

Ordering never depends on canvas position, so moving blocks around never
changes the generated code.

**Header split (multi-module projects).** A definition marked *shared* (on its
`⚙`) goes into `module.hpp`:

| Construct | `module.hpp` | `module.cpp` |
|-----------|--------------|--------------|
| Shared struct/class/enum/alias | full definition | out-of-line non-template method bodies |
| Shared function | declaration | definition |
| Shared template function / class | full definition | — |
| Shared global | `extern` declaration (or `inline constexpr` for constants) | definition |
| Non-shared anything | — | inside an anonymous `namespace { }` (internal linkage, preventing cross-module name clashes) |

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
  `E0701`, which names the needed standard or GCC version.

## 6.11 Export

*Export as C++ project* (UI and `b2c generate --export`) writes:

```
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
    pub severity: Severity,         // Error | Warning | Info
    pub message: MessageKey,        // i18n key + typed args; rendered in the UI
    pub primary: Location,          // block + part (+ token range)
    pub related: Vec<(Location, MessageKey)>,
    pub fixes: Vec<QuickFix>,       // structured BDM edits, applied via the editor's undo stack
    pub source: DiagSource,         // Analyser | Compiler | Linker | Runtime
    pub raw: Option<String>,        // original compiler text when source != Analyser
}
```

| Code range | Area |
|------------|------|
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
documentation.

## 6.13 Performance in the editor

* Analysis and codegen run per **module** on a debounced change (50 ms).
  Lowered SAST is cached per top-level block by structural hash, so editing
  one function re-lowers only that function. Name resolution, type checking
  and emission run per module. The target is p95 < 50 ms at 1,000 blocks
  ([N4](01-overview.md#non-functional-goals)).
* The WASM module (~1–2 MB, optimised with `wasm-opt -Oz`) is loaded once.
  Data crosses the JS ↔ WASM boundary as compact JSON. If profiling shows
  boundary costs dominate, it can switch to a binary format (`postcard`).
* Modules over 2,000 blocks run analysis in a Web Worker.
