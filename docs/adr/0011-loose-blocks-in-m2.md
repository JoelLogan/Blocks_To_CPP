# ADR-0011: Loose blocks stay errors, and loose stacks are saved intact

* Status: Proposed (M2 is built on it; the owner confirms it)
* Date: 2026-10-05

## Context

Blockly lets users leave blocks loose on the canvas: a single reporter, a
single statement, or a whole stack of statements dragged out of a function
and parked while they rework it. The project format had no place for such a
stack ([05 §5.4](../spec/05-project-format.md#54-block-nodes)). Its top level
is a list of single blocks with a canvas position, and statement sequences
exist only inside statement inputs, as arrays. Catalog resolution reports
every top-level statement or value block as `B2C-E0604` (*block in the wrong
place*), so any loose block disables building, and the spec said nothing more
about loose blocks.

The editor must save whatever is on the canvas without losing work, and the
CLI must give the same result as the app for the same file.

## Options considered

1. **Drop loose blocks on save.** Silently loses work. Rejected.
2. **Save each block of a loose stack as its own top-level block** with an
   absolute position. Nothing is lost except the connections, but the parked
   code falls apart into single blocks on every reload, and the user has to
   reassemble it.
3. **A workspace-level `stacks` array** (`[{x, y, blocks: [...]}]`) next to
   `blocks`. Keeps stacks intact, but adds a second top-level list with its
   own positions, ordering and ID rules, and a block could be in either list.
4. **An optional `stack` key on a top-level statement block (chosen).** The
   head of a loose stack is an ordinary top-level block, and `stack` holds the
   statement blocks attached below it, in order. It follows the format's rule
   that sequences are arrays, not `next` chains.
5. **Make loose blocks a warning** and ignore them when building. The app and
   the CLI would then accept files that have code which silently does not run,
   and an editor bug that detaches a stack would change the program without an
   error. Rejected.

## Decision

* **`B2C-E0604` stays an error**, for parity with the CLI. Its message tells
  the user to move the block into a function or delete it. Run is disabled
  while it exists, as for any analyser error
  ([04 §4.4](../spec/04-user-interface.md#44-diagnostics-ux)), and the live
  preview still shows the rest of the program.
* **Loose stacks use the `stack` key** ([05 §5.4](../spec/05-project-format.md#54-block-nodes)):
  * A top-level block (an element of `modules[].workspace.blocks`) whose
    catalog shape is *statement* may carry `"stack": [ <block>, … ]`, the
    statement blocks attached below it on the canvas, in order.
  * Stack elements are ordinary block nodes without `x`, `y` and their own
    `stack`. Blocks nested inside their inputs and statement inputs follow the
    normal rules.
  * `stack` on a nested block or on a stack element is a load error. An
    empty `stack` array is not written, and is a load error when present, so
    the canonical form has one spelling.
  * `stack` on a block whose catalog shape is not *statement* (a hat,
    definition, reporter or predicate) is an error too. The loader does not
    know block shapes, so this one is reported when the blocks are checked
    against the catalog.
  * Stacked blocks count toward every limit of
    [05 §5.6](../spec/05-project-format.md#56-validation-limits) exactly like
    other blocks. A stack element's depth is its head's depth.
  * The key is content: it is part of the canonical serialisation and of the
    project hash ([05 §5.11](../spec/05-project-format.md#511-content-hash)).
* **Catalog resolution** validates every stacked block like any other block,
  and the head's shape as above. The loose stack is reported once, as
  `B2C-E0604` on the head block; the stacked blocks get no further placement
  errors.
* **The analyser** treats a loose block and its whole stack as outside the
  program: no lowering, no symbols, no generated code.
* **A loose reporter or predicate** is saved as a top-level block with its
  position, which the format already allows. It has no `stack`.
* **Clipboard payloads** use the same shape: copying a stack produces one
  top-level block with `stack` ([05 §5.12](../spec/05-project-format.md#512-clipboard-format)).

## Consequences

* Parked code survives saving and reloading exactly as the user left it.
* The format gains one optional key in `formatVersion` 1, before 1.0. Files
  that use it are refused by builds from before M2 with *unknown key*
  (`B2C-E0110`) rather than *made with a newer version*. No app that could
  write such a file was released before M2, so this is acceptable.
* Implementation is shared: `b2c-model` parses, limits, serialises and hashes
  the key; `b2c-catalog` resolves stacked blocks, checks the head's shape and
  reports `E0604` once;
  `b2c-lang` skips loose blocks and stacks without panicking; the editor's
  BDM ⇄ Blockly sync maps a Blockly top-level statement chain to a head block
  with `stack` and back.
* Moving a statement out of a function changes the project hash, so a
  previous build no longer matches. That is correct: the program changed.
