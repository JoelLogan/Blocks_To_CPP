# ADR-0013: Additive format keys raise `formatVersion` only when they are used

* Status: Accepted (owner decision D18 of 2026-10-10)
* Date: 2026-10-10
* Amends [ADR-0004](0004-project-format.md): when `formatVersion` changes
  and what a migration is. Notes [ADR-0011](0011-loose-blocks-in-m2.md),
  whose `stack` key stays in format 1.

## Context

A project file names its format version (`formatVersion`, an integer), and
[ADR-0004](0004-project-format.md) gives the format a chain of forward
migrations. The loader is strict: a key the format does not define is the
load error `B2C-E0110` (*unknown key*), so a file cannot carry settings that
one tool would act on and another would silently drop
([ADR-0005](0005-no-compiler-flags-in-projects.md)). A file with a newer
`formatVersion` is refused as a whole with `B2C-E0108` (*made with a newer
version of Blocks2Cpp*).

The specification already plans keys for later milestones
([05 §5.3](../spec/05-project-format.md#53-top-level-structure)):
`options.usingNamespaceStd` (M3) and `project.lints` (M5). Each is written
only when it is used, "so files that do not use it are unchanged", but no
version step was given for them. An M3 file that uses *Textbook style*,
opened in M2, would therefore be told *"unknown key … Remove it or check its
spelling"*: the wrong advice, since the key is valid and removing it changes
the program. The two rules of the specification also conflicted: a format
change needs a new version and a migration, yet the new keys were meant to
leave existing files unchanged.

ADR-0011 added the optional `stack` key to format 1 before any app that could
write it was released. That stays as it is.

## Options considered

1. **One format version per release that adds keys.** Every file an M3 app
   saves would be format 2, so M2 would refuse files that use nothing new,
   and opening an old file in M3 would mark it as changed. This breaks
   "files that do not use it are unchanged".
2. **The lowest version whose keys cover the file (chosen).** Each version
   only adds keys. A file that uses no newer key keeps its version and still
   opens in older apps; a file that does use one is refused by them as made
   with a newer version.
3. **Ignore unknown keys.** A file could then carry settings that an older
   app silently drops when it saves. Rejected for the reason E0110 exists.
4. **Keep the rules as they are.** The conflict stays, and an older app
   advises deleting valid settings.

## Decision

* **`formatVersion` is the lowest version whose keys cover the file.** Each
  version only adds keys and never changes the meaning of earlier ones:
  version 2 adds `options.usingNamespaceStd` (M3), version 3 adds
  `project.lints` (M5), and each later key gets the next number.
* **The writer computes the version from the keys present,** so a file that
  uses no newer key keeps its version and still opens in older apps.
* **Migrations are only for changes of meaning.** Only a change to the
  meaning or shape of an existing key needs a migration v(n) → v(n+1), and
  `migratedFrom` reports only such migrations.
* **The newer-file message.** An unknown key in a file whose `generator.app`
  is a newer SemVer than this app is `B2C-E0108` (*made with a newer version
  of Blocks2Cpp*). The message never names a version taken from the file:
  an older app cannot know which version introduced the key, and text from
  the file is not repeated as a requirement. Otherwise an unknown key is
  `B2C-E0110`.
* The `stack` key of ADR-0011 stays in format 1.

## Consequences

* Files keep opening in the oldest app that understands every key they use,
  and opening an old file in a newer app never marks it as changed.
* Every reader that accepts `lints` enforces the rule that a project file
  cannot lower `W0520`
  ([05 §5.3](../spec/05-project-format.md#53-top-level-structure)).
* `b2c migrate` migrates to the lowest version that covers the file.
* In M2 only the newer-file message needs code (`b2c-model`'s loader, with
  rule tests: an unknown key with `generator.app` 9.0.0 gives `E0108`
  without a version number, an equal or older `generator.app` still gives
  `E0110`). The writer rule needs code with the first new key (M3).
* Block types are versioned separately, by each block's `v` and the
  catalog's version
  ([03 §3.11.3](../spec/03-block-language.md#3113-block-versioning)); this
  ADR does not change how.
