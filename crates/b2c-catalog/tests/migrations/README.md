# Golden files of block upgrades

Every block migration and every replacement of a retired block type has a
golden-file test here
([03 §3.11.3](../../../../docs/spec/03-block-language.md#3113-block-versioning),
[ADR-0015](../../../../docs/adr/0015-block-upgrades-on-load.md)).

* `<case>.b2c` is a project as an older version of Blocks2Cpp saved it.
* `<case>.upgraded.b2c` is what `b2c_catalog::upgrade_document_with` makes of
  it, in canonical form.

`../migrations.rs` upgrades every case with the shipped tables joined with
test-only entries, compares the result with the expected file, and checks
that the result loads, keeps the security hash and summary, does not change
when it is upgraded again, and leaves the catalog nothing old or unknown to
report. Write the expected files with
`B2C_UPDATE_GOLDEN=1 cargo test -p b2c-catalog --test migrations` and review
the diff.

| Case | What it covers |
| ------ | ---------------- |
| `retired_type` | The test-only retired type `test.say` (v0 and v1), replaced by `io.print`: its ID, position, comment, flags, nested blocks and loose stack are kept, and its own migration runs first |
| `var_update` | The shipped replacement of the retired `var.update` v1 by `var.change` v1 (catalog 1.2.0): each of the five operators becomes the same option of *change*'s menu `OP`, `VALUE` becomes `BY` with the expression or block in it, an absent `OP` or `VALUE` stays absent, and the ID, position, comment, flags, nested blocks and loose stack of each block are kept |
| `version_chain` | The test-only `test.counter`, saved at versions 1, 2 and 3 and upgraded through two migrations |

A shipped migration or replacement adds its own case, named after the block
type it upgrades (as `var_update.b2c` for the replacement of `var.update`).
