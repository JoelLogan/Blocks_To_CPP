# ADR-0004: Project format: editor-independent, versioned JSON with array statement lists

* Status: Accepted
* Date: 2026-10-02

## Context

Project files are shared between users (including untrusted sources), kept in
git, and must survive editor upgrades. Blockly's own serialisation stores UI
details, uses a global variable model, and chains statements through nested
`next` objects. That makes a long function deeply nested JSON, which stresses
recursion-limited parsers and enables stack-exhaustion inputs.

## Options considered

1. **Blockly JSON as-is.** Zero mapping work, but it ties us to Blockly's
   format, carries the deep nesting problem, and has no typed symbols.
2. **Zip container with multiple files.** Allows assets, but brings zip-slip
   and path-traversal risks and is not diff-friendly.
3. **Our own Block Document Model (BDM): single-file, versioned JSON.**
   Statement lists are arrays, symbols are referenced by ID, expression slots
   are stored as flat token lists, and the serialisation is canonical.

## Decision

**Option 3.** A single `.b2c` JSON file with `formatVersion`, strict schema,
hard limits, canonical serialisation and forward migrations
([05](../spec/05-project-format.md)).

## Consequences

* A mapping layer (BDM ⇄ Blockly workspace) is needed, with round-trip tests
  for every block type.
* Nesting depth equals logical program nesting, so a depth limit of 128 is
  ample and safe.
* Renames are stable, because references use symbol IDs.
* Files diff and merge reasonably in git, thanks to canonical ordering and
  layout kept separate from semantics.
