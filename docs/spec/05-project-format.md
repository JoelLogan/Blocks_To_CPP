# 5. Project Format and Persistence

> Status: **Draft v0.1** · Related ADR: [0004](../adr/0004-project-format.md), [0005](../adr/0005-no-compiler-flags-in-projects.md)

## 5.1 Requirements

| # | Requirement |
|---|-------------|
| F1 | Human-readable, diff- and merge-friendly (projects live happily in git) |
| F2 | **Editor-independent.** Not Blockly's serialisation, so we can upgrade or replace Blockly without breaking files. |
| F3 | Versioned with forward migrations. Newer-format files are refused with a clear message, never half-loaded. |
| F4 | **Safe to parse from untrusted sources.** Bounded size, depth and counts, with no references to external files, URLs or commands. |
| F5 | Deterministic. Saving an unchanged project produces byte-identical output. |
| F6 | Portable. The same file works on Windows and Linux; nothing machine-specific is stored in it. |

## 5.2 Files

A project is a **single JSON file** with the extension `.b2c`.

* Encoding: UTF-8 without BOM, `\n` line endings, 2-space indentation and a
  trailing newline.
* **Canonical serialisation:** object keys are written in schema order, and
  arrays keep semantic order (statement order, ⊕ slot order). Top-level
  blocks are sorted by `id`; their canvas position is data, not order.
* Why one file: it is easy to share, email or submit, it has no zip-slip or
  path-traversal surface, and multiple modules still map to multiple
  generated C++ files.

## 5.3 Top-level structure

```json
{
  "format": "blocks-to-cpp/project",
  "formatVersion": 1,
  "generator": { "app": "0.1.0", "catalog": "1.0.0" },
  "project": {
    "id": "prj_4kq9Xb2LmT7pRz1s",
    "name": "Guessing Game",
    "description": "",
    "language": { "standard": "c++20", "gnuExtensions": false },
    "options": {
      "showAdvanced": false,
      "manualMemory": false,
      "preferPlainStd": false,
      "formattingStyle": "stream",
      "checkedIndexing": true
    },
    "build": {
      "configurations": {
        "debug":   { "optimization": "none", "debugInfo": true,  "sanitizers": ["address", "undefined"], "warnings": "helpful", "hardening": true },
        "release": { "optimization": "speed", "debugInfo": false, "sanitizers": [],                    "warnings": "helpful", "hardening": true }
      },
      "defines": [ { "name": "GAME_VERSION", "value": { "int": 2 } } ],
      "libraries": [ "sfml-graphics" ],
      "packs": [ { "id": "std", "version": "^1.0" } ]
    },
    "run": { "args": ["--easy"], "workingDirectory": "project", "stdinFile": null }
  },
  "modules": [
    {
      "id": "mod_main",
      "name": "main",
      "workspace": {
        "blocks": [ /* top-level blocks, see §5.4 */ ],
        "frames": [ { "id": "frm_1", "title": "Game loop", "x": 0, "y": 0, "w": 800, "h": 600, "color": "blue", "emitBanner": true } ],
        "notes":  [ { "id": "nte_1", "text": "TODO: add difficulty levels", "x": 900, "y": 40 } ],
        "viewport": { "x": 0, "y": 0, "scale": 1.0 }
      }
    }
  ]
}
```

Every option is a **closed enum or a validated scalar**. There is no field
for free-form compiler flags, paths or commands ([ADR-0005](../adr/0005-no-compiler-flags-in-projects.md)).
`defines` take validated identifiers and typed values (`int`, `bool`, `string`
escaped by the emitter), which become `-D` arguments built by the backend.

## 5.4 Block nodes

```json
{
  "id": "blk_Qm81",
  "type": "control.if",
  "v": 1,
  "x": 120, "y": 80,
  "collapsed": false,
  "disabled": false,
  "comment": { "text": "Check the guess", "pinned": false },
  "extra": { "elseIfCount": 1, "hasElse": true },
  "fields": { },
  "inputs": {
    "COND0": { "expr": [ {"ref": "sym_guess"}, {"op": "<"}, {"ref": "sym_secret"} ] },
    "COND1": { "block": { "id": "blk_Zx01", "type": "logic.compare", "v": 1, "fields": {"OP": ">"},
                          "inputs": { "A": {"expr": [{"ref": "sym_guess"}]}, "B": {"expr": [{"ref": "sym_secret"}]} } } }
  },
  "statements": {
    "DO0":  [ { "id": "blk_P1", "type": "io.print", "v": 1, "...": "..." } ],
    "DO1":  [ ],
    "ELSE": [ ]
  }
}
```

| Key | Meaning |
|-----|---------|
| `id` | Unique within the project. `[A-Za-z0-9_]{1,32}`, generated as a random 96-bit base62 string with a type prefix. |
| `type`, `v` | Catalog block ID and version. Unknown type → load error naming the missing pack. Older `v` → migration. |
| `x`, `y` | Present only on top-level blocks (integers, clamped to ±10⁷). |
| `fields` | Field values: strings, numbers, booleans or symbol declarations (`{"sym": "sym_x", "name": "score"}`). Each value is validated against the catalog field kind. |
| `inputs` | Value inputs: either `{"block": …}` (a nested reporter) or `{"expr": [tokens]}` (an expression slot, [03 §3.4](03-block-language.md#34-expression-slots)). An absent input means the catalog default (shadow). |
| `statements` | Statement inputs as **arrays** of blocks. |
| `extra` | Mutator state (variadic counts, ⊕ parts), validated against the catalog's schema for that block. |

**Statement sequences are arrays, not linked `next` pointers.** Blockly's
native JSON chains statements through nested `next` objects, so a 500-line
function becomes JSON nested 500 levels deep. That breaks recursion-limited
parsers and invites stack-exhaustion attacks. Arrays keep nesting depth equal
to the **logical** nesting of the program (if-in-loop-in-function), which is
small and bounded.

## 5.5 Symbols

A declaration field stores `{"sym": "<id>", "name": "<identifier>"}`. All
references (getter blocks, expression tokens, symbol dropdowns) store only the
symbol ID. The analyser rebuilds the symbol table on load. A reference to a
missing symbol is a diagnostic, not a load failure, so a file damaged by hand
editing still opens.

## 5.6 Validation limits

Enforced in `b2c-model` **before** any other processing, on files, clipboard
pastes and IPC payloads alike:

| Limit | Default | Rationale |
|-------|---------|-----------|
| File / payload size | 32 MiB | Typical projects are < 1 MiB |
| JSON nesting depth | 128 | Logical program nesting rarely exceeds 20 |
| Total blocks | 100,000 | Far above the 5,000-block performance target |
| Modules | 256 | |
| Expression tokens per slot | 512 | |
| Expression parse depth | 64 | |
| String field length | 64 KiB (Raw C++: 256 KiB) | |
| Identifier length | 64 | |
| Variadic parts per block | 64 | |
| Numeric coordinates | ±10⁷ | |

Further rules:

* **Unknown keys are rejected** (`deny_unknown_fields`), except inside an
  explicit `"x-ext"` object reserved for forward-compatible tooling metadata,
  which is preserved but never interpreted.
* **Duplicate JSON keys are rejected.** Many parsers silently take the last
  value, which allows ambiguity attacks.
* **Strings must be valid UTF-8** with no NUL. Text fields reject C0 controls
  other than `\t` and `\n`, and reject Unicode bidi controls (see
  [08 §8.4](08-security.md#84-code-injection-through-block-content)).
* In the frontend, JSON is never parsed into objects that are used as
  prototypes or merged into other objects. The WASM validator returns fresh,
  typed data. `__proto__`, `constructor` and `prototype` keys are rejected
  outright.

## 5.7 Versioning and migration

* `formatVersion` is an integer. `b2c-model` contains a chain of pure
  migrations `v(n) → v(n+1)`, each with golden-file tests.
* On load: if `formatVersion` is newer than supported, refuse with *"This
  project was made with a newer version of Blocks to C++ (needs ≥ X)"*. If it
  is older, migrate in memory, and save in the new format only after the
  user's explicit save (a `.b2c.bak` of the original is kept beside it).
* Block-level migrations (`v` per block) follow the same pattern
  ([03 §3.11.3](03-block-language.md#3113-block-versioning)).

## 5.8 What is deliberately **not** stored in a project

| Not stored | Stored instead | Why |
|------------|----------------|-----|
| Compiler path, flags, `-I`/`-L` paths | Machine settings, library profiles | Project files can come from anyone; flags like `-fplugin`, `-B` and `-wrapper` execute code ([08 §8.5](08-security.md#85-compiler-invocation-safety)). |
| Trust decisions | Trust store (machine-local) | A file must not be able to vouch for itself. |
| Absolute paths of any kind | — | Privacy and portability |
| Build outputs | Per-user cache directory | Keeps files small and avoids writing next to untrusted locations |
| User identity, timestamps | — | Privacy and determinism. VCS provides history. |

## 5.9 Machine-local data

| File | Contents | Schema |
|------|----------|--------|
| `settings.json` | UI preferences, code style, default standard, selected toolchain ID, *advanced extra compiler flags* (still filtered, [08 §8.5](08-security.md#85-compiler-invocation-safety)), environment pass-through list | Versioned JSON, validated on load. Invalid values are reset to defaults with a notice. |
| `toolchains.json` | Discovered and manually added toolchains, with probe results and fingerprints | Re-probed when a fingerprint changes |
| `trust.json` | Trusted project records `{projectId, canonicalPath, rawCodeHashAtGrant, grantedAt}` and trusted folders | Validated on load; a corrupt file means "nothing is trusted". Other processes running as the same user are out of scope ([08 §8.1](08-security.md#81-scope-and-assumptions)). |
| `libraries.json` | Library profiles: `name → {includeDirs[], libDirs[], linkNames[], runtimeDirs[]}` | Paths are chosen via dialog only |
| `recent.json` | Recent project paths (opaque IDs exposed to the UI) | |

## 5.10 Saving and recovery

* **Atomic save.** Serialise → write to a temporary file in the **same
  directory** (created exclusively, with a random name) → flush and `fsync` →
  rename over the target (`MoveFileExW(MOVEFILE_REPLACE_EXISTING |
  MOVEFILE_WRITE_THROUGH)` on Windows, `rename(2)` + directory `fsync` on
  Linux). A crash leaves either the old file or the new file, never a mix.
* Before overwriting, the previous version is kept as `<name>.b2c.bak`
  (one generation, configurable).
* **Autosave** writes recovery snapshots to the recovery directory (never next
  to the project) every 30 s while dirty. Snapshots are deleted on a clean
  save or close.
* **External change detection.** If the file changes on disk while open
  (detected by a file watcher and a hash check before save), the user chooses
  *Reload* or *Keep mine (save as…)*.

## 5.11 Content hash

`projectHash = SHA-256(canonical serialisation with layout-only keys removed)`.
Layout-only keys are `x`, `y`, `viewport`, `frames`, `notes`, `collapsed` and
comment `pinned`. The hash is used for:

* **Build-cache keys**, together with the toolchain fingerprint and resolved
  build options
* **Run staleness checks**: Run uses the binary only if it matches the current
  hash
* **Trust**: a trusted project whose Raw C++ or library requirements change
  *outside the app* is re-flagged ([08 §8.3](08-security.md#83-workspace-trust-and-restricted-mode))

## 5.12 Clipboard format

Copying blocks puts two representations on the clipboard: a text/plain
rendering of the generated C++ (useful for pasting into chat or an editor),
and a custom `application/x-blocks-to-cpp+json` payload:

```json
{ "format": "blocks-to-cpp/clipboard", "formatVersion": 1, "catalog": "1.0.0", "blocks": [ … ] }
```

Pasting runs the **same validator and limits as file loading**. Pasted blocks
get fresh IDs, and symbol references are re-resolved by name in the target
scope. A paste that contains Raw C++ shows an inline notice: *"Pasted content
includes Raw C++ – review before running."*
