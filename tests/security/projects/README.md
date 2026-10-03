# Malicious-project regression suite

Crafted `.b2c` files, one for every attack a project file can express
([spec 08 §8.12](../../../docs/spec/08-security.md#812-threat-summary)). Each
file is a small, otherwise valid project with one attack in it, so the
expected outcome is exact: the file is rejected with exactly the listed
problem codes, or it is accepted.

The table below **is** the test expectation. Two tests read it, check that it
lists every file in this folder (and no others), and assert each outcome:

* `crates/b2c-model/tests/security.rs` runs the **Loader** column:
  `b2c_model::load` must fail with exactly the listed codes (every diagnostic
  an error from the loader), or succeed (`accepted`). Accepted files must also
  survive a canonical save and reload unchanged.
* `crates/b2c-catalog/tests/security.rs` runs the **Resolve** column for
  accepted files: `b2c_catalog::resolve` against the core catalog must report
  exactly the listed codes, or nothing (`clean`). `—` means the loader already
  rejected the file.

Both tests also check that no diagnostic message contains a control,
bidirectional or invisible character: hostile text is always quoted safely.

**Later stages** says what stops an accepted attack further down the
pipeline (typed encoders, the analyser, the backend). Those outcomes are
asserted by the tests of the stages named there.

Characters that are dangerous to show (bidi controls, NUL, escapes) are
written as JSON `\uXXXX` escapes, so the files are safe to open in an editor
and on GitHub; the loader decodes escapes and raw characters identically.

Threat IDs refer to the table in spec 08 §8.12.

## Adding a case

Add the file here and a row to the table, with the codes you expect. Keep the
rest of the project valid, so that the file tests one thing.

## Cases

| File | Threat | Attack | Loader | Resolve | Later stages |
|------|--------|--------|--------|---------|--------------|
| `absolute-path.b2c` | T9 | An absolute path in `run` (paths are never stored in projects) | `B2C-E0110` | — |  |
| `bidi-in-comment.b2c` | T4 | Directional isolates (U+2066, U+2069) in a comment | `B2C-E0126` | — |  |
| `bidi-in-description.b2c` | T4 | An Arabic letter mark (U+061C) in the project description | `B2C-E0126` | — |  |
| `bidi-in-name.b2c` | T4 | A right-to-left mark (U+200F) in a variable name | `B2C-E0126` | — |  |
| `bidi-in-string-literal.b2c` | T4 | A right-to-left override (U+202E) in a string literal | `B2C-E0126` | — |  |
| `carriage-return-in-comment.b2c` | T4 | A carriage return that hides text in some viewers | `B2C-E0125` | — |  |
| `compiler-flags.b2c` | T2 | Compiler flags and include paths in `build` (ADR-0005: not part of the format) | `B2C-E0110` | — |  |
| `constructor-key.b2c` | T6 | A `constructor.prototype` chain in a block's fields | `B2C-E0127` | — |  |
| `control-char-in-key.b2c` | T4 | A control character in a field name | `B2C-E0125` | — |  |
| `deep-block-nesting.b2c` | T5 | 60 loops nested inside each other (more than 128 JSON levels) | `B2C-E0104` | — |  |
| `deep-expression.b2c` | T5 | 255 nested parentheses in one slot (within the token limit) | accepted | clean | The expression parser stops at depth 64 |
| `deep-nesting.b2c` | T5 | 5,000 nested lists (stack exhaustion) | `B2C-E0104` | — |  |
| `define-name-injection.b2c` | T2 | A compiler flag disguised as a `-D` define name | `B2C-E0132` | — |  |
| `define-value-injection.b2c` | T3 | A define value that tries to end its string | accepted | clean | The backend passes it as one escaped string argument |
| `diagnostic-flood.b2c` | T5 | 1,500 unknown keys, to flood the problem list | `B2C-E0110`, `B2C-E0199` | — | At most 1,000 problems are listed, then one summary |
| `duplicate-block-ids.b2c` | T1 | Two blocks with the same ID (diagnostics and edits would hit the wrong one) | `B2C-E0114` | — |  |
| `duplicate-key-escaped.b2c` | T5 | `format` repeated with an escaped spelling (`form\u0061t`) | `B2C-E0105` | — |  |
| `duplicate-key.b2c` | T5 | A block's `type` appears twice; parsers disagree about which one counts | `B2C-E0105` | — |  |
| `duplicate-module-ids.b2c` | T1 | Two modules with the same ID | `B2C-E0116` | — |  |
| `duplicate-symbol-ids.b2c` | T1 | Two variables and a parameter declaring the same symbol ID | `B2C-E0115` | — |  |
| `hat-inside-statement.b2c` | T1 | A second `when program starts` inside the first | accepted | `B2C-E0604` |  |
| `homoglyph-identifier.b2c` | T3 | A name with a Cyrillic `о` that looks like `score` | accepted | clean | The analyser accepts only ASCII names (08 §8.4.1) |
| `huge-coordinates.b2c` | T5 | Canvas coordinates beyond ±10⁷ (and beyond 32 bits) | `B2C-E0129` | — |  |
| `huge-format-version.b2c` | T5 | A `formatVersion` beyond 32 bits | `B2C-E0109` | — |  |
| `huge-number.b2c` | T5 | A number (`1e400`) too large for any number type | `B2C-E0103` | — |  |
| `huge-params-list.b2c` | T5 | A function with 65 parameter rows | `B2C-E0131` | — |  |
| `huge-variadic-count.b2c` | T5 | A ⊕ count of 4,294,967,295 parts (the editor would build that many inputs) | `B2C-E0131` | — |  |
| `huge-zoom.b2c` | T5 | A viewport zoom of 10³⁰⁸ | `B2C-E0130` | — |  |
| `id-injection.b2c` | T7 | Markup in a block ID | `B2C-E0113` | — |  |
| `injection-block-type.b2c` | T3 | A block type containing code | accepted | `B2C-E0601` |  |
| `injection-comment-splice.b2c` | T3/T4 | Comments ending in `\` or `??/` that would splice the next line | accepted | clean | `Comment` adds the ` //` sentinel (08 §8.4.3) |
| `injection-dropdown.b2c` | T3 | A dropdown value containing code | accepted | `B2C-E0607` |  |
| `injection-identifier.b2c` | T3 | A variable name containing code | accepted | clean | The analyser rejects the name (`Ident`, 08 §8.4.1) |
| `injection-number-field.b2c` | T3 | A number field containing code | accepted | clean | `NumLit` accepts only the literal grammar (08 §8.4.4) |
| `injection-operator-token.b2c` | T3 | An operator token containing code | accepted | clean | The expression parser rejects unknown operators (08 §8.4.5) |
| `injection-string-literal.b2c` | T3 | A string literal that tries to close itself and add a call | accepted | clean | `StrLit` escapes it (08 §8.4.2) |
| `injection-text-field.b2c` | T3 | The same payload in a text block's field | accepted | clean | `StrLit` escapes it (08 §8.4.2) |
| `injection-type-field.b2c` | T3 | A type field containing code | accepted | `B2C-E0607` |  |
| `input-beyond-count.b2c` | T4 | An input hidden beyond the ⊕ count, invisible in the editor | accepted | `B2C-E0608` |  |
| `invalid-utf8.b2c` | T5 | A byte that is not UTF-8 | `B2C-E0102` | — |  |
| `json-comments.b2c` | T5 | A comment before the JSON (not JSON) | `B2C-E0103` | — |  |
| `library-flag-injection.b2c` | T2 | Linker flags disguised as library names | `B2C-E0133` | — |  |
| `line-separator-in-comment.b2c` | T4 | Unicode line and paragraph separators and NEL (U+0085) in a comment | accepted | clean | `Comment` splits on every line terminator (08 §8.4.3) |
| `linked-next-chain.b2c` | T5 | Blockly-style `next` chains instead of statement lists | `B2C-E0110` | — |  |
| `lone-surrogate.b2c` | T5 | Half of a UTF-16 surrogate pair in an escape | `B2C-E0103` | — |  |
| `markup-in-text.b2c` | T7 | HTML and script in the project name and a note | accepted | clean | The UI renders project text as text only (08 §8.8) |
| `missing-pack-block.b2c` | T13 | A block from a library pack that is not installed | accepted | `B2C-E0601` |  |
| `module-name-absolute.b2c` | T9 | A Windows absolute path as a module name | `B2C-E0117` | — |  |
| `module-name-case-clash.b2c` | T9 | Two modules whose files collide on Windows (`util`, `Util`) | `B2C-E0117`, `B2C-E0119` | — |  |
| `module-name-device-superscript.b2c` | T9 | `COM¹`, which Windows treats as a device too | `B2C-E0118` | — |  |
| `module-name-device-upper.b2c` | T9 | The Windows device name `NUL` in capitals | `B2C-E0118` | — |  |
| `module-name-device.b2c` | T9 | The Windows device name `con` as a module name | `B2C-E0118` | — |  |
| `module-name-traversal.b2c` | T9 | A module name that walks out of the output folder | `B2C-E0117` | — |  |
| `nan-number.b2c` | T5 | `NaN` (not JSON) | `B2C-E0103` | — |  |
| `nested-block-position.b2c` | T1 | A canvas position on a block inside another block | `B2C-E0128` | — |  |
| `newer-block-version.b2c` | T1 | A block from a newer catalog version | accepted | `B2C-E0602` |  |
| `newer-format-version.b2c` | T1 | A project from a newer Blocks2Cpp (never half-loaded) | `B2C-E0108` | — |  |
| `nul-in-name.b2c` | T4 | A NUL character in a variable name (C strings end there) | `B2C-E0124` | — |  |
| `nul-in-string-literal.b2c` | T4 | A NUL character in a string literal | `B2C-E0124` | — |  |
| `older-format-version.b2c` | T1 | A format version that never existed | `B2C-E0109` | — |  |
| `overlong-utf8.b2c` | T5 | An overlong UTF-8 encoding of `/` (a classic filter bypass) | `B2C-E0102` | — |  |
| `oversized-name.b2c` | T5 | A variable name of 65 characters | `B2C-E0123` | — |  |
| `oversized-number-literal.b2c` | T3 | A number literal far beyond every number type | accepted | clean | Range-checked by `NumLit` (`B2C-E0517`) |
| `oversized-string.b2c` | T5 | A string literal of 64 KiB + 1 byte | `B2C-E0123` | — |  |
| `pack-path-traversal.b2c` | T9/T13 | A library pack ID that walks out of the packs folder | `B2C-E0136` | — |  |
| `proto-key-in-token.b2c` | T6 | A `__proto__` key as an expression token | `B2C-E0127` | — |  |
| `proto-key-in-x-ext.b2c` | T6 | `__proto__` and `constructor` inside `x-ext` | accepted | clean | Preserved as data, never interpreted (05 §5.6) |
| `proto-key.b2c` | T6 | A top-level `__proto__` key | `B2C-E0127` | — |  |
| `prototype-key-in-extra.b2c` | T6 | A `prototype` key in a block's `extra` | `B2C-E0127` | — |  |
| `raw-cpp-block.b2c` | T2 | A Raw C++ block (not available in this version) | accepted | `B2C-E0601` |  |
| `run-args-shell.b2c` | T2 | Shell syntax in program arguments | accepted | clean | Arguments are an argv list, never a shell command (07 §7.6) |
| `statement-in-value-input.b2c` | T1 | A statement block plugged into a value input | accepted | `B2C-E0604` |  |
| `terminal-escape-in-name.b2c` | T4/T7 | Terminal escape sequences in the project name | `B2C-E0125` | — |  |
| `too-many-modules.b2c` | T5 | 257 modules (each becomes a file) | `B2C-E0120` | — |  |
| `too-many-parameters.b2c` | T5 | 17 parameters where the catalog allows 16 | accepted | `B2C-E0613` |  |
| `too-many-tokens.b2c` | T5 | An expression slot with 513 tokens | `B2C-E0122` | — |  |
| `trailing-data.b2c` | T5 | A second project appended after the first | `B2C-E0103` | — |  |
| `utf16.b2c` | T5 | The project saved as UTF-16 | `B2C-E0102` | — |  |
| `utf8-bom.b2c` | T5 | A UTF-8 byte order mark | `B2C-E0102` | — |  |
| `working-directory-path.b2c` | T9 | A path instead of a working-directory choice | `B2C-E0112` | — |  |
| `wrong-format-tag.b2c` | T1 | Some other JSON file | `B2C-E0107` | — |  |
| `zero-width-in-string.b2c` | T4 | A zero-width space (U+200B) in a string literal | accepted | clean | `StrLit` writes it as `\u200B` (08 §8.4.2) |

## Limits tested without a file

Some limits need inputs too large to keep in the repository. They are
generated by the tests instead:

| Limit | Code | Test |
|-------|------|------|
| File larger than 32 MiB (checked before parsing) | `B2C-E0101` | `e0101_file_too_large` in `crates/b2c-model/tests/rules.rs` |
| More than 4,194,304 JSON values | `B2C-E0106` | `e0106_too_many_values` in `crates/b2c-model/tests/rules.rs` |
| More than 100,000 blocks, nested ones included | `B2C-E0121` | `e0121_too_many_blocks_counts_nested_blocks` in `crates/b2c-model/tests/rules.rs` |
| More than 500,000 free-form values in `extra` and `x-ext` | `B2C-E0135` | `e0135_free_form_budget` in `crates/b2c-model/tests/rules.rs` |

Attacks on library packs themselves (template abuse, raw flags in packs) need
pack files, which milestone M1 does not load yet; they join this suite with
pack loading.
