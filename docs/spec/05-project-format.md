# 5. Project Format and Persistence

> Status: **Draft v0.1** · Related ADRs: [0004](../adr/0004-project-format.md), [0005](../adr/0005-no-compiler-flags-in-projects.md), [0007](../adr/0007-backend-crates-and-ipc-contract.md), [0011](../adr/0011-loose-blocks-in-m2.md), [0012](../adr/0012-parked-blocks.md), [0013](../adr/0013-additive-format-keys.md)

## 5.1 Requirements

| # | Requirement |
| --- | ------------- |
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
  Numbers in free-form data are written as `JSON.stringify` writes them
  ([§5.6](#56-validation-limits)).
* Why one file: it is easy to share, email or submit, it has no zip-slip or
  path-traversal surface, and multiple modules still map to multiple
  generated C++ files.

## 5.3 Top-level structure

```json
{
  "format": "blocks2cpp/project",
  "formatVersion": 1,
  "generator": { "app": "0.1.0", "catalog": "1.0.0" },
  "project": {
    "id": "prj_4kq9Xb2LmT7pRz1s",
    "name": "Guessing Game",
    "language": { "standard": "c++20" },
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
    "run": { "args": ["--easy"], "workingDirectory": "project" }
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

`description` is written only when it is not empty, and
`language.gnuExtensions` only when it is `true`.

`generator` names the app and the block catalog that saved the file. A save
writes this app's version and the newer of the two catalogs, the file's and
this app's (SemVer precedence, as for `generator.app` in §5.7): a save never
lowers `generator.catalog`. So the blocks of a file from a newer catalog,
kept unchanged in an older app, are still reported as made by a newer
version (`B2C-E0602`,
[03 §3.11.3](03-block-language.md#3113-block-versioning)) after that app
saved the file, and not as unknown. `b2c migrate` writes
`generator` the same way when it upgraded blocks.

`project.name` is the display name shown in the top bar, the recent list,
recovery offers, the trust dialog and the generated header comment. It is
independent of the file's name; renaming
([04 §4.1](04-user-interface.md#41-main-window-layout)) is an ordinary edit
that changes the content hash.

`options.usingNamespaceStd` (*Textbook style*: `using namespace std;` in
every generated `.cpp`, [06 §6.14](06-compiler-pipeline.md#614-standard-names-and-using-namespace))
comes in M3; a file that sets it has `formatVersion` 2 or higher (§5.7). It
is `false` when absent and is written only when `true`, like
`language.gnuExtensions`, so files that do not use it are unchanged.

`project.lints` (M5; `formatVersion` 3 or higher) sets lint levels for
everyone who opens the project
([06 §6.6](06-compiler-pipeline.md#66-stage--types-flow-checks-and-lints)):
`"lints": { "W0510": "error", "I0513": "off" }`.

* Each key is an analyser warning or info code (`W05` or `I05` followed by
  two digits, without the `B2C-` prefix), and each value is `off`, `info`,
  `warning` or `error`. Any other key or value is a load error.
* The project file can raise `W0520` (hidden characters in Raw C++) but
  cannot lower it, because that warning defends against the file itself
  ([08 §8.4.6](08-security.md#846-raw-c-and-hidden-text-defences)). A lower
  level for `W0520` is a load error. The machine settings can still change
  it.
* A code that this version does not know (from a newer version) is kept on
  save and ignored. `b2c-lang`, which owns the list of codes, reports it as
  info `I0526` on the project (*"This project sets a level for `W0599`, which
  this version of Blocks2Cpp does not know. It is ignored."*). The file still
  opens.
* The machine settings can override each entry, and the machine value wins
  (§5.9).
* The object is written only when it has entries, so files that do not use
  it are unchanged.

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
| ----- | --------- |
| `id` | Unique within the project. `[A-Za-z0-9_]{1,32}`, generated as a random 96-bit base62 string with a type prefix. |
| `type`, `v` | Catalog block ID and version. Unknown type → the block is kept and reported as `B2C-E0601` (naming the missing pack); newer `v` → `E0602`; an older `v` or a retired type → upgraded on load (§5.7). |
| `x`, `y` | Present only on top-level blocks (integers, clamped to ±10⁷). |
| `collapsed`, `disabled` | Written only when `true`. `disabled` means the user disabled the block (Blockly's *manually disabled* reason); Blockly's other reasons for disabling a block are never saved. |
| `stack` | Only on a top-level *statement* block: the statement blocks attached below it on the canvas, in order (see *Loose blocks* below). |
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

**Loose blocks.** The editor saves whatever is on the canvas, including blocks
that are not inside the program or a function
([ADR-0011](../adr/0011-loose-blocks-in-m2.md),
[ADR-0012](../adr/0012-parked-blocks.md)):

* A loose block is an ordinary top-level block with `x` and `y`. A loose
  statement, reporter or predicate, with its stack, is *parked*
  (ADR-0012): it is saved and shown, but not lowered; it declares nothing,
  generates nothing and never stops a build. The analyser reports each
  enabled parked head once as `B2C-W0505`. Parked blocks are still checked
  against the catalog, so an unknown or damaged block is an error wherever
  it is.
* A **loose stack** of statements is saved intact: its first block is the
  top-level block, and `"stack": [ … ]` holds the statement blocks attached
  below it, in order. Stack elements are ordinary block nodes without `x`,
  `y` or a `stack` of their own; blocks nested in their inputs follow the
  normal rules. A stack element's depth is its head's depth, and stacked
  blocks count toward every limit of §5.6.
* `stack` on a nested block or on a stack element is a load error, and so is
  an empty `stack` (it is never written, so the canonical form has one
  spelling). `stack` on a block whose catalog shape is not *statement* is an
  error found when the blocks are checked against the catalog (stage ② of
  [06 §6.3](06-compiler-pipeline.md#63-stage--resolve-catalog)), because the
  loader does not know block shapes.
* A parked stack is reported once, as `W0505` on its enabled head; its
  stacked blocks get no placement diagnostic. `B2C-E0604` remains for
  placements the editor cannot produce.
* `stack` is content: it is part of the canonical serialisation and of the
  project hash (§5.11).

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
| ------- | --------- | ----------- |
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
| Define `int` values | ±(2⁵³ − 1) | The editor holds the document as JavaScript numbers, which keep whole numbers only up to this size exactly (`B2C-E0112`) |
| Numbers in `extra` and `x-ext` | ±(2⁵³ − 1) | As above; every larger float is a whole number too (`B2C-E0112`) |

Further rules:

* **Unknown keys are rejected** (`deny_unknown_fields`), except inside an
  explicit `"x-ext"` object reserved for forward-compatible tooling metadata,
  which is preserved but never interpreted.
* **Numbers in free-form data** (`extra` and `x-ext`) are read as JavaScript
  reads them, because the editor holds the document as JavaScript values: a
  whole number written with a fraction or an exponent (`1.0`, `2e3`) is
  stored as that integer, and the canonical writer writes the other numbers
  as `JSON.stringify` does (`0.5`, `0.0000015`, `1e-7`). With the limits
  above, a document that has been through the editor (`JSON.parse`, then
  `JSON.stringify`) saves to the same text and content hash, and its defines
  keep their values.
* **Duplicate JSON keys are rejected.** Many parsers silently take the last
  value, which allows ambiguity attacks.
* **Strings must be valid UTF-8** with no NUL. Text fields reject C0 controls
  other than `\t` and `\n`, and reject Unicode bidi controls (see
  [08 §8.4](08-security.md#84-code-injection-through-block-content)).
* **Library-pack references** have an ID matching `[a-z][a-z0-9_-]{0,63}`
  (pack IDs name folders, so they can never carry a path) and a version
  requirement made only of SemVer-requirement characters; a pack may be
  listed only once.
* **Module names** match `[a-z][a-z0-9_-]{0,63}`, are unique ignoring case,
  and are never Windows device names (`con`, `nul`, `com1`, … including the
  superscript-digit forms), because they become file names.
* In the frontend, JSON is never parsed into objects that are used as
  prototypes or merged into other objects. The WASM validator returns fresh,
  typed data. `__proto__`, `constructor` and `prototype` keys are rejected
  outright.

## 5.7 Versioning and migration

* `formatVersion` is an integer: the lowest version whose keys cover the
  file ([ADR-0013](../adr/0013-additive-format-keys.md)). Each version only
  adds keys and never changes the meaning of earlier ones: version 2 adds
  `options.usingNamespaceStd` (M3), version 3 adds `project.lints` (M5), and
  each later key gets the next number. The writer computes the version from
  the keys present, so a file that uses no newer key keeps its version and
  still opens in older apps.
* Only a change to the meaning or shape of an existing key needs a migration
  `v(n) → v(n+1)`. `b2c-model` contains the chain of these pure migrations,
  each with golden-file tests, and `migratedFrom` reports only such
  migrations.
* On load: if `formatVersion` is newer than supported, refuse with *"This
  project was made with a newer version of Blocks2Cpp"* (`B2C-E0108`). If it
  is older, migrate in memory, and save in the new format only after the
  user's explicit save (a `.b2c.bak` of the original is kept beside it).
* An unknown key in a file whose `generator.app` is newer than this app (by
  SemVer precedence) is `B2C-E0108` (*made with a newer version of
  Blocks2Cpp*); otherwise it is `B2C-E0110`. Neither message names a version
  taken from the file, which could claim any.
* **Block upgrades**
  ([03 §3.11.3](03-block-language.md#3113-block-versioning),
  [ADR-0015](../adr/0015-block-upgrades-on-load.md)). When a project,
  recovery snapshot or clipboard payload is loaded, `b2c-catalog` upgrades in
  memory every block with an older `v` and every block of a retired type,
  before the editor or compiler sees it. Block upgrades run after the
  document has passed §5.6 and before trust is evaluated. A migration or
  replacement never adds blocks, nesting or tokens and never changes a
  block's ID. It never changes the security summary or the security hash
  ([08 §8.3.1](08-security.md#831-how-trust-is-decided)): the text fields of
  `raw.*` blocks, libraries, packs and defines are carried over unchanged; a
  change that must alter them bumps the hash's version tag (`b2c-trust-v2`)
  and asks for trust again. Each upgraded block is checked against these
  rules and §5.6 again, and a violation leaves that block unchanged with
  `B2C-E0603`. The whole upgraded document (or clipboard payload) is then
  checked once more: its security hash and summary must be unchanged, and it
  must still pass §5.6. If it does not, every upgrade of the document is
  undone and each upgraded block is reported as `B2C-E0603`. The project is
  marked changed; the upgraded form is written only on explicit save,
  keeping the `.b2c.bak`.
* `b2c migrate` applies format migrations and block upgrades and writes the
  result (`--in-place` keeps `.b2c.bak`); when it upgraded blocks, it writes
  `generator` as a save does (§5.3). `b2c fmt` only re-serialises and never
  upgrades blocks. `b2c check`, `generate` and `build` upgrade in memory
  only.

## 5.8 What is deliberately **not** stored in a project

| Not stored | Stored instead | Why |
| ------------ | ---------------- | ----- |
| Compiler path, flags, `-I`/`-L` paths | Machine settings, library profiles | Project files can come from anyone; flags like `-fplugin`, `-B` and `-wrapper` execute code ([08 §8.5](08-security.md#85-compiler-invocation-safety)). |
| Trust decisions | Trust store (machine-local) | A file must not be able to vouch for itself. |
| Absolute paths of any kind | — | Privacy and portability |
| Build outputs | Per-user cache directory | Keeps files small and avoids writing next to untrusted locations |
| User identity, timestamps | — | Privacy and determinism. VCS provides history. |

## 5.9 Machine-local data

| File | Contents | Schema |
| ------ | ---------- | -------- |
| `settings.json` | UI preferences, code style, default standard, selected toolchain ID, lint levels for every project on this computer (same shape as `project.lints`, §5.3; they win over the project's), *advanced extra compiler flags* (still filtered, [08 §8.5](08-security.md#85-compiler-invocation-safety)), environment pass-through list | Versioned JSON, validated on load. Invalid values are reset to defaults with a notice. |
| `toolchains.json` | Discovered and manually added toolchains, with probe results and fingerprints | Re-probed when a fingerprint changes |
| `trust.json` | Trusted project records `{projectId, canonicalPath, rawCodeHashAtGrant, grantedAt}` and trusted folders | Validated on load; a corrupt file means "nothing is trusted". Other processes running as the same user are out of scope ([08 §8.1](08-security.md#81-scope-and-assumptions)). |
| `libraries.json` | Library profiles: `name → {includeDirs[], libDirs[], linkNames[], runtimeDirs[]}` | Paths are chosen via dialog only |
| `recent.json` | Recent project paths (opaque IDs exposed to the UI) | |

In `settings.json`, a lint entry with an invalid key or value is dropped with
a notice, and the other entries are kept. A code in the right form that this
version does not know is kept on save and ignored, as in the project file, so
an older version never deletes levels that a newer one wrote.

Where each file lives is in [02 §2.7](02-architecture.md#27-persistence-locations).
Every file is JSON with a `format` tag and an integer `formatVersion`, is read
with a size limit (at most *limit + 1* bytes, so an oversized file is rejected
without being loaded), is validated strictly, and is written atomically
(§5.10) with owner-only permissions. Only the backend reads or writes them;
nothing in them ever comes from, or goes into, a project file. The shapes
below are the M2 versions (`libraries.json` comes with library profiles in
M4).

**`settings.json`** (read limit 1 MiB):

```json
{
  "format": "blocks2cpp/settings",
  "formatVersion": 1,
  "codeStyle": { "indentWidth": 4 },
  "run": { "onErrors": "disableRun" },
  "console": { "scrollbackLines": 10000 },
  "toolchain": { "selectedId": null },
  "newProject": { "standard": "c++20" },
  "buildCache": { "maxBytes": 2147483648 }
}
```

| Key | Values | Default |
| --- | --- | --- |
| `codeStyle.indentWidth` | `2` or `4` | `4` |
| `run.onErrors` | `disableRun` or `showProblems` ([04 §4.4](04-user-interface.md#44-diagnostics-ux)) | `disableRun` |
| `console.scrollbackLines` | 1,000–100,000 | 10,000 |
| `toolchain.selectedId` | `null` or a toolchain ID (`tc_` + 16 hex digits) | `null` |
| `newProject.standard` | `c++17`, `c++20`, `c++23` or `c++26` | `c++20` |
| `buildCache.maxBytes` | 256 MiB to 1 TiB | 2 GiB |
| `ui.layout.toolboxWidth` | 160–1,200 (CSS pixels) | absent |
| `ui.layout.rightDockWidth` | 200–960 | absent |
| `ui.layout.bottomDockHeight` | 96–720 | absent |
| `ui.layout.rightCollapsed`, `ui.layout.bottomCollapsed` | `true` or `false` | absent |

* A missing file means the defaults. A file that is not valid JSON means the
  defaults with a notice; the file is rewritten at the next change. A single
  invalid value is reset to its default with a notice naming its key, and the
  other values are kept.
* Keys this version does not know are kept verbatim when the file is saved.
  A file with a newer `formatVersion` is read for the keys this version knows,
  with a notice, and its other keys are kept when it is saved.
* `settings_get` returns the full settings with defaults filled in, plus the
  notices (`{ key, reason }`, with `reason` one of `invalidValue`,
  `corruptFile`, `newerVersion`). `settings_update` accepts only `codeStyle`,
  `run`, `console` and `ui`, each partial (`ui` takes a partial `layout`),
  and rejects any other key. Only `toolchain_select` changes
  `toolchain.selectedId`. Updates are serialised and return the full new
  settings.
* `ui.layout` is the editor's layout as the user last left it
  ([04 §4.1](04-user-interface.md#41-main-window-layout)): the width of the
  toolbox's flyout, the docks' sizes and whether each dock is hidden. A key
  is absent until the user first changes it, and while it is absent the
  first-start rule of 04 §4.1 applies. The app writes a change at once when
  it ends what the user did (a dock shown or hidden, the end of a splitter
  drag, `Home` or `End` on a splitter), so closing the window right after it
  keeps it; the arrow-key steps on a splitter are written together half a
  second after the last one.
  Each value is validated on its own like the others; a `ui` or `ui.layout`
  that is not an object is reset as a whole, with a notice naming it, and
  unknown keys inside them are kept.
* The keys of later milestones, `lints` (M5) and the extra flags and
  `envPassthrough` (M4, [10 §10.1](10-roadmap.md#101-milestones)), are kept
  verbatim when present until then.

**`toolchains.json`** (read limit 4 MiB): `{ format: "blocks2cpp/toolchains",
formatVersion: 1, toolchains: [{ source, foundAs, probe }] }`, where:

* `source` is how the toolchain was found, with the values of the IPC
  `Toolchain.source` ([02 §2.5.2](02-architecture.md#252-commands)): `path`
  (a `PATH` entry), `wellKnown` (a well-known or configured install folder)
  or `manual` (added with `toolchain_add_dialog`);
* `foundAs` is the path it was found as, before links were resolved (for
  example `/usr/bin/g++`, or for a manual toolchain the file the user
  picked), which the IPC `Toolchain.displayPath` shows;
* `probe` is a probed toolchain (fingerprint with the canonical path,
  version, target, capabilities and problems,
  [07 §7.3](07-toolchain-build-run.md#73-capability-probing)).

`toolchain_list` reports the cached entries before background discovery
finishes, and `source` and `foundAs` make them show the same source and path
as discovery does. The file is a cache: an unreadable or invalid file, or
the bare array that M1 wrote, counts as empty, and discovery fills it again.

**`trust.json`** (read limit 4 MiB):

```json
{
  "format": "blocks2cpp/trust",
  "formatVersion": 1,
  "projects": [
    { "projectId": "prj_4kq9Xb2LmT7pRz1s", "canonicalPath": "/home/ada/games/guess.b2c",
      "rawCodeHashAtGrant": "<64 lower-case hex digits>", "grantedAt": "2026-10-05T09:30:00Z" }
  ],
  "folders": [ { "canonicalPath": "/home/ada/games", "grantedAt": "2026-10-05T09:31:00Z" } ]
}
```

`rawCodeHashAtGrant` is the security hash of
[08 §8.3](08-security.md#83-workspace-trust-and-restricted-mode), and times
are RFC 3339 in UTC. A missing, oversized or invalid file means nothing is
trusted; the next grant writes a valid file. A file that exists but cannot be
read at all is never overwritten. A path that is not valid Unicode cannot be
recorded, so such a project cannot be trusted.

* A path holds at most one project record. At most 10,000 project and 1,000
  folder records are kept, paths are at most 8 KiB, and the file never grows
  past its read limit: the records granted longest ago are dropped first.
* Every change re-reads the file and replaces it atomically while holding an
  exclusive lock on `trust.json.lock` next to it (waiting at most 5 s), so
  several app instances never undo each other's grants. Every evaluation
  reads the file again. Trust is evaluated when a project is opened,
  reloaded or restored
  ([08 §8.3.1](08-security.md#831-how-trust-is-decided)), so a revocation in
  one instance holds in the others the next time they open the project.
  Build and Run use the open project's evaluated trust.
* A whole drive or the file-system root cannot be trusted as a folder.

**`recent.json`** (read limit 1 MiB): `{ format: "blocks2cpp/recent",
formatVersion: 1, entries: [{ id, path, projectName, lastOpenedAt }] }`,
newest first and at most 10. `id` is the `recentId` the UI uses
(`rc_` + 32 hex digits); an entry keeps its ID when it moves to the front. An
entry's `projectName` follows the project: it is updated whenever the project
is opened, saved or saved as, and the entry moves to the front then. The UI
gets the name, a display form of the path and the time, never a path it
could send back ([02 §2.5](02-architecture.md#25-ipc-surface)).

## 5.10 Saving and recovery

* **Atomic save.** Serialise → write to a temporary file in the **same
  directory** (created exclusively, with a random name) → flush and `fsync` →
  rename over the target (`MoveFileExW(MOVEFILE_REPLACE_EXISTING |
  MOVEFILE_WRITE_THROUGH)` on Windows, `rename(2)` + directory `fsync` on
  Linux). A crash leaves either the old file or the new file, never a mix.
  Projects, `settings.json`, `trust.json`, `recent.json`, `toolchains.json`,
  recovery snapshots and build manifests are all written this way; the
  temporary file is removed when anything fails. A new project file gets
  the permissions any new file of the user gets (the umask applies); a saved
  project keeps the permissions it had. Machine-local files and recovery
  snapshots are always `0600`
  ([08 §8.6](08-security.md#86-filesystem-safety)).
* Before overwriting, the previous version is kept as `<name>.b2c.bak`
  (one generation, configurable). The `.bak` file is written through its own
  temporary file and rename, so a link planted at the `.bak` path is replaced,
  never followed. In M2 the number of generations is fixed at one.
* **Save responses.** `project_save` returns `savedAt`, an RFC 3339 UTC
  timestamp, and `hash`, the lower-case hex SHA-256 of the bytes written. That
  hash becomes the baseline for detecting outside changes.
* **Autosave** writes recovery snapshots to the recovery directory (never next
  to the project) every 30 s while the project has unsaved changes, and when
  the window loses focus. Snapshots are deleted on a clean save, a reload
  (which discards the unsaved changes) or close.
* **External change detection.** If the file changes on disk while open
  (detected by a file watcher and a hash check before save), the user chooses
  *Reload* or *Keep mine (save as…)*.

**Recovery snapshots.** The recovery directory
([02 §2.7](02-architecture.md#27-persistence-locations)) holds one folder per
running app instance:

```text
<recovery>/
├── <instanceId>.lock                 held exclusively by that instance while it runs
└── <instanceId>/                     instanceId: 32 hex digits, new for each start
    ├── <snapshotId>.b2c              the document, as the editor last sent it
    ├── <snapshotId>.json             its metadata
    └── <snapshotId>.prev.b2c         the previous document, only while a new one is written
```

* The metadata is `{ format: "blocks2cpp/recovery", formatVersion: 1,
  projectId, projectName, hasPath, boundPath, savedAt, appVersion,
  trustedAtWrite, securityHash, documentHash }`: `boundPath` is the project's
  file or `null` for a project never saved, `trustedAtWrite` says whether the
  project was trusted when the snapshot was written, `securityHash` is its
  security hash ([08 §8.3](08-security.md#83-workspace-trust-and-restricted-mode)),
  and `documentHash` is the SHA-256 of the document it was written with. It is
  read with a 64 KiB limit and validated strictly (no unknown or duplicate
  keys); the document is read with the project limit of §5.6. A snapshot whose
  metadata is invalid or from a newer `formatVersion` is not offered and is
  left alone.
* `projectName` is cut to 1 KiB. A path that cannot be written to JSON (not
  valid Unicode, or longer than 8 KiB) is recorded as `boundPath: null` with
  `hasPath: true`: such a snapshot restores without a path, in Restricted
  Mode, and is never taken for a project that was never saved.
* Each project has one snapshot per instance. Its `snapshotId`, `sn_` + 32 hex
  digits, stays the same until the snapshot is deleted, which happens when the
  project is saved cleanly, reloaded or closed. An instance keeps at most 64
  snapshots.
* Every file is written atomically (§5.10 above) with mode `0600`. A
  snapshot is replaced as a pair: the current document is first renamed to
  `<snapshotId>.prev.b2c`, then the new document is written, then the new
  metadata, and the previous document is removed. A restore returns only the
  document whose SHA-256 equals `documentHash`, so a crash at any point leaves
  the old snapshot or the new one, and a document is never restored with
  metadata (and trust facts) written for another one.
* Several instances of the app may run at once. Each takes an exclusive lock
  on its own `.lock` file before it creates its folder, and holds it until it
  exits; the lock file is never read. Only snapshots whose instance lock is
  free, that is, of instances that have exited or crashed, are offered for
  restore, newest first and at most 100; reading or discarding one takes that
  lock again. While it holds a stopped instance's lock, the app removes the
  temporary files and documents without metadata that a crash left there,
  then the folder and its lock file once the folder is empty, and lock files
  without a folder. An instance that exits removes its own folder and lock
  file when no snapshot is left in it; the snapshots of projects that still
  had unsaved changes stay for the next start.
* A restored snapshot opens as a project bound to its `boundPath` (or to no
  path) and is trusted only under the rules of
  [08 §8.3](08-security.md#83-workspace-trust-and-restricted-mode), with the
  trust record of `boundPath` itself: when that path now resolves to another
  one (a link was put in place of the file or of a folder above it), it
  restores in Restricted Mode. The app then writes the restored project's
  own snapshot and discards the old one.
* Project content never goes into logs.

**External changes.** For each open project with a file, the backend watches
the file's parent folder. Events are debounced for 300 ms per project, and a
check happens at most 2 s after the first event while events keep coming;
then the file is hashed again (with the size limit), and only a SHA-256 that
differs from the baseline counts as a change. A deleted or renamed file
counts as changed (`deleted: true`), and then *Reload* is not available. The
frontend is told through the `projectChangedOnDisk` app event
([02 §2.5](02-architecture.md#25-ipc-surface)), once per change. The app's own
saves update the baseline first, so they never notify, and a save or reload
that finishes after the project was closed does not watch its file again.
`project_save` checks the hash again before writing and refuses with
`changedOnDisk` when it differs; M2 has no *overwrite anyway*.

## 5.11 Content hash

`projectHash = SHA-256(canonical serialisation with layout-only keys removed)`.
Layout-only keys are `x`, `y`, `viewport`, `frames`, `notes`, `collapsed` and
comment `pinned`. `project.lints` is removed too, because lint levels never
change the generated code. The hash is used for:

* **Build-cache keys**, together with the toolchain fingerprint and resolved
  build options
* **Run staleness checks**: Run uses the binary only if it matches the current
  hash. The build records it in `build-manifest.json`
  ([07 §7.5.1](07-toolchain-build-run.md#751-build-directory)), and the
  `finished` build event reports it.

Trust uses a narrower hash of its own, the **security hash** over Raw C++
text, libraries, packs and defines only, so that ordinary edits never touch
trust and a trusted project whose security-relevant content changes *outside
the app* is re-flagged ([08 §8.3](08-security.md#83-workspace-trust-and-restricted-mode)).

## 5.12 Clipboard format

Copying blocks puts two representations on the clipboard: a text/plain
rendering of the generated C++ (useful for pasting into chat or an editor),
and a custom `application/x-blocks2cpp+json` payload:

```json
{ "format": "blocks2cpp/clipboard", "formatVersion": 1, "catalog": "1.0.0", "blocks": [ … ],
  "refs": { "<symbol id>": { "name": "geo::area", "kind": "function" } } }
```

`refs` records the qualified name and kind of each symbol that the copied
blocks refer to but do not declare. `kind` is `variable`, `parameter`,
`loopVariable` or `function`. A copied stack of statements is one block with
`stack` (§5.4).

Copied blocks have no `x` or `y`, because a paste places blocks at the
target: `clipboard_make` writes none. `load_clipboard` accepts `x`/`y` on a
top-level copied block, checks it like a canvas position (`B2C-E0129` when
out of range) and drops it, so the paste ignores it and the canonical
payload has one spelling. `x`/`y` on a stacked or nested block is
`B2C-E0128`, as in a project file.

Pasting runs the **same validator and limits as file loading**. Pasted blocks
get fresh IDs, and symbol references are re-resolved by qualified name in the
target scope ([06 §6.14.11](06-compiler-pipeline.md#61411-names-typed-in-slots)).
A paste that contains Raw C++ shows an inline notice: *"Pasted content
includes Raw C++ – review before running."*

**Transport and validation:**

* The editor uses the webview's own `copy`, `cut` and `paste` events, writing
  and reading `application/x-blocks2cpp+json` and `text/plain` through the
  event's `DataTransfer`, with an in-memory copy as the fallback inside the
  app. There is no clipboard plugin and no backend command.
* The payload is made and checked by the WASM core: `clipboard_make` builds
  it from the selected blocks (and cuts their C++ from the last preview,
  whole blocks only, for `text/plain`), and `paste_prepare` loads it with
  `b2c_model::load_clipboard`, which uses the same strict JSON parser, limits,
  text rules and block decoder as `load`, with the same `B2C-E01xx` codes. A
  payload whose `format` is not `blocks2cpp/clipboard` is refused
  (`B2C-E0138`), and a newer `formatVersion` gives `B2C-E0108`.
* Fresh block and symbol IDs come from a seeded generator in Rust
  (`SeededIds`): the frontend passes 256 random bits from
  `crypto.getRandomValues`, so the WASM core itself uses no randomness. IDs
  already used in the document are skipped, and every reference to a symbol
  declared inside the pasted blocks is rewritten to its new ID.
* A reference to a symbol declared outside the pasted blocks is bound again by
  qualified name and kind among the symbols in scope at the paste target
  ([06 §6.14.11](06-compiler-pipeline.md#61411-names-typed-in-slots)). When
  no symbol in scope has that name, a reference whose original symbol (the
  same symbol ID) is in scope there with the same kind keeps it, even when it
  was renamed since the copy, so copy, rename and paste within one project
  needs no fix. Symbol IDs are stable but not unique across projects:
  projects made from the same example or template share them, so a block
  pasted from one into another binds such a reference silently to the
  symbol with its ID, whatever that symbol is called there. A reference that
  finds no match (or several) stays a reference and gets `B2C-E0201`,
  naming the original symbol.
* Until namespaces arrive (M3), a qualified name is the plain name for
  variables, parameters and loop variables, and `::name` for functions.
* A paste target is the canvas, the start of a statement list or a value
  input, or the position directly after a block; scope is evaluated there
  ([06 §6.5](06-compiler-pipeline.md#65-stage--names-and-scopes)). A paste
  on the canvas, or into or directly after a parked block, has the parked
  scope (06 §6.5). A disabled statement in a list the analyser reaches has
  the scope of its position, and a block inside a disabled statement has
  that statement's scope.
* `paste_prepare` returns the blocks ready to insert at the target, in order.
  On a canvas a copied stack stays one block with `stack`; in or after a
  block, the stacked blocks follow their head as ordinary blocks and no
  block has `stack`, since a nested block cannot have one (`B2C-E0139`).
