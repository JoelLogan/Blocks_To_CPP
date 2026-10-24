# ADR-0015: Block upgrades run on load; before 1.0 a retired block type is replaced at once

* Status: Accepted (owner decision D18 of 2026-10-10)
* Date: 2026-10-13
* Amends [ADR-0004](0004-project-format.md): its forward migrations also
  cover blocks, and they run whenever a document is loaded. Builds on
  [ADR-0013](0013-additive-format-keys.md), which versions the format, not
  blocks.

## Context

Every block in a project file names its type and the version of that type's
definition (`v`), and the file names the catalog version it was saved with
(`generator.catalog`). The specification promised a migration, a pure
function from one block to another, for every breaking change of a block
([03 §3.11.3](../spec/03-block-language.md#3113-block-versioning)), applied
like the format's migrations: in memory on load, written on the user's next
save ([05 §5.7](../spec/05-project-format.md#57-versioning-and-migration)).

The code did something else (observation U3 of the owner's M2 trial):

* Block migrations ran only in the compiler's resolve stage. Opening a
  project, restoring a recovery snapshot and pasting did not apply them.
* The editor shows every block whose `v` differs from its definition as a
  placeholder. A block that could have been upgraded could only be looked
  at, and the upgrade was never saved.
* A migration could not change a block's type, so two block types could not
  be merged. Retiring a type needed a deprecation period of one minor
  release, with a badge, that nothing implemented.

The first change that needs more is merging *update* (`var.update`) into
*change* (`var.change`), so that the Variables category shows one block
for both (owner decision D3). It retires a block type.

Three more questions came with it:

* Where do upgrades run relative to the limits of
  [05 §5.6](../spec/05-project-format.md#56-validation-limits) and to
  workspace trust
  ([08 §8.3](../spec/08-security.md#83-workspace-trust-and-restricted-mode))?
  An upgrade that added blocks or text could take a file past the limits
  after they were checked, or change Raw C++ text after trust was decided.
* What does an older app do with a block that a newer catalog extended
  compatibly, for example with a new field that has a default or a new
  dropdown option? It would report the new field as unknown (`B2C-E0605`)
  and advise removing it.
* May block types be retired before 1.0, or are the core block IDs frozen
  from M2 on?

## Options considered

1. **Upgrade in the compiler only (the old behaviour).** The editor keeps
   showing placeholders, and files are never upgraded. Rejected: the user
   cannot edit such blocks, and every later version has to carry the old
   forms forever.
2. **Upgrade whenever a document is loaded (chosen).** Projects, recovery
   snapshots and clipboard payloads are upgraded before the editor or the
   compiler sees them, and the user saves the upgraded form.
3. **Freeze the core block IDs from M2 on.** Retired types stay loadable
   with a deprecation badge. No merge is possible before 1.0, and the editor
   and compiler carry block types that nobody should use.
4. **Treat every new field as a breaking change** (a new version and a
   migration). An older app then refuses a block that only gained an
   option, and every file from an older app counts as changed when it is
   opened. Rejected by D18 in favour of comparing catalog versions.

## Decision

* **Upgrades run on load.** When a project, a recovery snapshot or a
  clipboard payload is loaded, `b2c_catalog::upgrade_document` upgrades in
  memory every block with an older `v` (the type's chain of migrations) and
  every block of a retired type (its replacement), before the editor or the
  compiler sees it. The compiler's resolve stage runs it as well; running it
  twice changes nothing.
* **Replacements.** Merging or retiring a block type ships a *replacement*
  from the old type and its last version to the successor type and a version
  of it, listed in `BLOCK_REPLACEMENTS`. Like a migration, it is a pure
  function. The upgrade keeps the block's ID, position, comment, collapsed
  and disabled flags, its loose stack and every nested block; a replacement
  only renames and rewrites the block's own fields, inputs and `extra`
  settings. The old type is listed in `RETIRED_BLOCKS`, and a test checks
  that no catalog definition uses a retired ID, so a retired ID is never
  used again.
* **Order and limits.** Upgrades run after the document has passed
  05 §5.6 and before trust is evaluated. An upgrade never adds blocks,
  nesting or expression tokens, and never changes a block ID or the IDs of
  the symbols a block declares. It never changes the security summary or the
  security hash
  ([08 §8.3.1](../spec/08-security.md#831-how-trust-is-decided)). The
  upgraded blocks are checked against 05 §5.6 again. A block whose upgrade
  fails or would break one of these rules stays exactly as it was and is
  reported as `B2C-E0603`: the build stops, and the editor shows the block
  as a placeholder and saves it unchanged. The whole upgraded document (or
  clipboard payload) is then checked once more: if its security hash or
  summary changed, or it no longer passes 05 §5.6, no single block is to
  blame, so every upgrade of the document is undone and each upgraded block
  is reported as `B2C-E0603`.
* **Saving.** An upgraded project counts as changed. The upgraded form is
  written only when the user saves, and the save keeps the file as it was as
  `<name>.b2c.bak`. `b2c migrate` applies block upgrades as well
  (`--in-place` keeps the `.b2c.bak`), `b2c fmt` never upgrades, and
  `b2c check`, `generate` and `build` upgrade in memory only. A save, and a
  `b2c migrate` that upgraded blocks, names this app's catalog in
  `generator.catalog` unless the file names a newer one, which is kept
  ([05 §5.3](../spec/05-project-format.md#53-top-level-structure)).
* **Newer catalogs (D18, option b).** A compatible change (a field, `extra`
  key or input with a default; a dropdown option) keeps the block's version
  and raises the catalog's minor version. When a document's
  `generator.catalog`, or a clipboard payload's `catalog`, is newer than
  this app's catalog, a block with a field, input, `extra` key or dropdown
  option this app does not know is reported as made by a newer catalog
  (`B2C-E0602`), not as unknown (`B2C-E0605`, `E0607`, `E0608`, `E0610`,
  `E0611`), and is kept unchanged.
* **Before 1.0** an exact replacement retires a block type at once.
  **After 1.0** a retired type first gets a deprecation period: it still
  loads, shows a deprecation badge and offers its replacement.

## Consequences

* The editor never shows a block that can be upgraded as a placeholder.
  Opening, reloading or restoring such a project says so (`blocksUpgraded`,
  [02 §2.5.2](../spec/02-architecture.md)), and the project starts with
  unsaved changes.
* Every migration and replacement gets a golden-file test under
  `crates/b2c-catalog/tests/migrations/`. `upgrade_document` is part of the
  resolve fuzz target and of the mutation-testing list of
  [09 §9.2](../spec/09-quality-and-delivery.md#92-testing-strategy).
* A replacement that would have to change Raw C++ text, libraries, packs or
  defines cannot be written under this ADR: it needs a new version of the
  security hash (`b2c-trust-v2`), so the project asks for trust again.
* Opened in an older app, a file saved (or migrated) after a replacement
  names a newer catalog, so the successor's new fields or options are
  reported as made by a newer catalog instead of with the advice to remove
  them. Saving the file in the older app keeps that newer catalog, so the
  report does not change after the save.
* M2 ships the mechanism with its first replacement, `var.update` →
  `var.change` (catalog 1.2.0): *change* gained the operator menu `OP`,
  and each *update* block becomes the *change* block with the same
  operator and its value as the amount, so the C++ does not change.
