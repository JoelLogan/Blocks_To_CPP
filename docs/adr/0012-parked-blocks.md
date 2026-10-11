# ADR-0012: Loose blocks are parked: a warning, not an error

* Status: Accepted (owner decision D1 of 2026-10-10)
* Date: 2026-10-10
* Partly supersedes [ADR-0011](0011-loose-blocks-in-m2.md): its placement
  decision. ADR-0011's `stack` key and clipboard shape stay in force.

## Context

[ADR-0011](0011-loose-blocks-in-m2.md) kept every loose block an error. A
statement, reporter or predicate left on the canvas outside *when program
starts* or a function is reported as `B2C-E0604` (*block in the wrong
place*), which disables ▶ Run and makes `b2c check` exit with 1. Catalog
resolution reports it even when the user has disabled the block.

The owner's first trial of M2 (observation U5) showed that this works against
the way people build programs in a Scratch-like editor. Scratch lets blocks
lie anywhere: a stack is put together on the side, tried, and attached when it
is ready, and a block pulled out for a moment is not a mistake. In
Blocks2Cpp the same habit disables Run until every loose block is attached or
deleted, even one the user disabled on purpose.

ADR-0011 rejected a warning (its option 5) for two reasons, which still need
an answer:

* the app and the CLI would accept files with code that silently does not
  run;
* an editor bug that detaches a stack would change the program without an
  error.

ADR-0011's other decision is not in question: the `stack` key
([05 §5.4](../spec/05-project-format.md#54-block-nodes)) and the clipboard
shape keep loose stacks intact whatever their severity.

## Options considered

1. **Parked blocks with a warning (chosen).** A loose statement or value
   block, with its stack, is saved and shown, gets one warning `B2C-W0505`,
   is never generated and never blocks Run. Definitions are never parked,
   and damaged or unknown blocks still stop the build.
2. **The same with severity *info*.** Quieter, but it answers ADR-0011's
   concern about code that silently does not run with only a faint note.
3. **Keep `B2C-E0604` an error, but let Run go ahead.** The app would build
   programs that `b2c check` and `b2c build` reject, so the CLI and the app
   would disagree about the same file.
4. **Keep ADR-0011 as it is.** Rejected by the owner after the trial.

Three further questions were decided with option 1:

* **Should parked blocks also look dimmed or dashed?** No. The ⚠ badge of
  the warning is enough, and Scratch shows nothing at all.
* **Should a second *when program starts* be parked instead of being
  `B2C-E0406`?** No. It would be unclear which of the two runs, so it stays
  an error.
* **Should parked stacks be lowered now, so that their own declarations
  work?** Not in M2. It is about 1–2 weeks of work on the critical path, and
  the Scratch workflow (build a stack loose, then attach it) works as soon as
  the stack is attached. The limitation is accepted until M3 and documented
  (below).

## Decision

1. **What is parked.** A top-level block whose catalog shape is statement,
   reporter or predicate is parked, together with its `stack`. It is saved
   and shown, but never lowered, generated or run, and being loose never
   stops a build. Definitions (*when program starts*, *define*, and later
   struct, class, enum, global and Raw C++ declarations) are never parked.
2. **One warning per parked stack.** The analyser reports each enabled
   parked head once as the lint `B2C-W0505` (default: warning; a project can
   raise it to an error once lint levels exist, M5). A parked head the user
   disabled is not reported. Parked blocks are not dimmed or drawn
   differently: the warning's badge marks them.
3. **`B2C-E0604` stays for what the editor cannot produce:** a hat or
   definition inside a block, a statement in a value input, a reporter as a
   step, and blocks stacked below a block that is not a statement.
4. **Catalog checks still apply.** Parked blocks are checked against the
   catalog like every other block. An unknown or newer block type, or an
   invalid field or `extra` (`B2C-E0601`–`B2C-E0605`), stays an error
   wherever the block is, so a damaged or unsupported block (including a Raw
   C++ block before M4,
   [08 §8.3.1](../spec/08-security.md#831-how-trust-is-decided)) still stops
   the build.
5. **Parked code is not analysed until it is attached.** The scope query
   answers parked blocks with the parked scope
   ([06 §6.5](../spec/06-compiler-pipeline.md#65-stage--names-and-scopes)):
   what is visible at the end of the module's `main` body, or the module's
   functions alone when it has no `main`.
6. **`B2C-E0406` stays** for a second *when program starts*, whether it is
   loose or not.

## Consequences

* ▶ Run works with parked code, and `b2c check` exits with 0 and a warning.
  The generated C++ is identical with or without parked blocks.
* Until parked code is lowered for the scope query (M3), a variable created
  inside a parked stack is not offered by the menus of the blocks below it,
  and is not listed in Variables. A parked reporter also has no known type,
  so the connection checker accepts any value in its slots. Both resolve when
  the stack is attached. Once parked blocks are implemented,
  [03 §3.3](../spec/03-block-language.md#33-block-shapes) and the
  getting-started guide state this limitation.
* Editing a parked block changes the project's content hash, so the next Run
  recompiles although the C++ is unchanged. This lasts until the build's
  up-to-date check compares the generated files
  ([07 §7.5.1](../spec/07-toolchain-build-run.md#751-build-directory), M5).
* ADR-0011's objections are answered by the visible warning, by lint levels
  (M5), which let a classroom make `W0505` an error, and by the round-trip
  tests that the editor never detaches a stack by itself.
* The work is done in the M2 polish wave (W4): catalog resolution stops
  reporting `E0604` for loose heads, the analyser reports `W0505`, the scope
  query answers for parked blocks, and the spec chapters 03–06, the
  diagnostics reference and the getting-started guide describe parked blocks.
  Until then the code behaves as ADR-0011 describes.
* The app and the CLI treat parked blocks the same way, so parking does not
  split them as option 3 would.
