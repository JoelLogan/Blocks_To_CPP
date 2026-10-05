# Malicious-clipboard regression suite

Crafted clipboard payloads (`application/x-blocks2cpp+json`,
[spec §5.12](../../../docs/spec/05-project-format.md#512-clipboard-format)).
Clipboard data can come from any app or web page, so a paste runs the same
validator and limits as opening a project file. Each file is the valid
payload `valid.json` with one attack in it, so the expected outcome is
exact: the payload is rejected with exactly the listed problem codes, or it
is accepted.

The table below **is** the test expectation.
`crates/b2c-model/tests/clipboard.rs` reads it, checks that it lists every
file in this folder (and no others), and asserts that
`b2c_model::load_clipboard` fails with exactly the listed codes (every
diagnostic an error from the loader), or succeeds (`accepted`). Accepted
payloads must also survive a canonical save and reload unchanged, and no
diagnostic message may contain a control, bidirectional or invisible
character. The same test generates the one case too large to commit: a
payload one byte over the 32 MiB limit (`B2C-E0101`).

Characters that are dangerous to show (bidi controls, NUL) are written as
JSON `\uXXXX` escapes, so the files are safe to open in an editor and on
GitHub. The files also seed the `clipboard` fuzz target
([fuzz/README.md](../../../fuzz/README.md)).

Threat IDs refer to the table in
[spec 08 §8.12](../../../docs/spec/08-security.md#812-threat-summary).

## Adding a case

Add the file here and a row to the table, with the codes you expect. Keep
the rest of the payload valid, so that the file tests one thing.

## Cases

| File | Threat | Attack | Loader |
|------|--------|--------|--------|
| `bad-id.json` | T7 | Markup in a block ID (`blk_x" onmouseover="alert(1)`) | `B2C-E0113` |
| `bad-ref-id.json` | T7 | A `refs` key that is not a symbol ID | `B2C-E0113` |
| `bidi-in-ref-name.json` | T4 | A right-to-left override (U+202E) in a qualified name in `refs` | `B2C-E0126` |
| `bidi-text.json` | T4 | A right-to-left override (U+202E) in a string literal of a stacked block | `B2C-E0126` |
| `depth-128.json` | T5 | Lists nested exactly 128 levels deep (the limit) | accepted |
| `depth-129.json` | T5 | Lists nested 129 levels deep, one over the limit | `B2C-E0104` |
| `duplicate-block-ids.json` | T1 | Two copied blocks with the same ID | `B2C-E0114` |
| `duplicate-key.json` | T5 | A block's `id` appears twice; parsers disagree about which one counts | `B2C-E0105` |
| `duplicate-symbols.json` | T1 | Two declarations of the same symbol ID | `B2C-E0115` |
| `huge-coordinate.json` | T5 | A canvas position beyond 32 bits | `B2C-E0129` |
| `huge-variadic-count.json` | T5 | A ⊕ count of 4,294,967,295 parts | `B2C-E0131` |
| `linked-next-chain.json` | T5 | A Blockly-style linked `next` chain instead of a stack array | `B2C-E0110` |
| `newer-version.json` | T1 | `formatVersion` 2 with keys this version does not know | `B2C-E0108` |
| `not-an-object.json` | T1 | A JSON list instead of a payload object | `B2C-E0138` |
| `nul-in-ref-name.json` | T3 | A NUL character in a qualified name in `refs` | `B2C-E0124` |
| `project-file.json` | T1 | A payload tagged as a whole project (`blocks2cpp/project`) | `B2C-E0138` |
| `proto-key-in-refs.json` | T6 | A `__proto__` key in `refs` | `B2C-E0127` |
| `proto-key.json` | T6 | A `__proto__` key in a block's fields | `B2C-E0127` |
| `stack-in-stack.json` | T5 | A `stack` inside a stacked block | `B2C-E0139` |
| `unknown-ref-kind.json` | T1 | A `refs` entry of an unknown kind (`macro`) | `B2C-E0112` |
| `valid.json` | — | None: a loose stack, a declaration and two outside references | accepted |
| `wrong-tag.json` | T1 | Another program's clipboard format (`blockly/clipboard`) | `B2C-E0138` |
| `x-ext.json` | T1 | Tool data in `x-ext`, which only project files may carry | `B2C-E0110` |
