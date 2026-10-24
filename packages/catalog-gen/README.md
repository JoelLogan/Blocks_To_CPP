# @blocks2cpp/catalog-gen

The build-time generator for everything the editor and the docs need from the
block catalog ([spec §3.11.1](../../docs/spec/03-block-language.md#3111-catalog-format)).
It is a development tool: no package depends on it, and nothing of it ships
in the app.

## What it reads and writes

```text
catalog/core/*.toml, catalog/toolbox.toml
        │  crates/b2c-catalog parses and validates them;
        │  crates/b2c-catalog/tests/export.rs exports the result
        ▼
packages/catalog-gen/catalog.json            (committed)
        │  this package
        ▼
packages/blockly-ext/src/generated/catalog.ts   block definitions and toolbox
docs/reference/blocks/*.md                      the block reference
```

- `catalog.json` holds the catalog version, every block definition (sorted by
  ID) and the toolbox, in the Rust (serde) shapes of `b2c-catalog`.
- `catalog.ts` exports `CATALOG_VERSION`, `BLOCK_DEFS` and `TOOLBOX`, with the
  types of [`src/catalog-types.ts`](src/catalog-types.ts) copied in, so the
  module has no imports. The generator renames the Rust keys (`field` becomes
  `fields`), turns count defaults into numbers and parses each friendly label
  into text, argument (`%NAME`) and repeat (`…`) parts.
- `docs/reference/blocks/` has an index and one page per toolbox category. The
  folder holds generated files only: the generator removes pages it no longer
  writes.

All validation stays in Rust. The generator never reads TOML; it only checks
that `catalog.json` has the shape it expects, so a change of the Rust schema
fails loudly instead of producing wrong definitions.

## Regenerating

After changing anything in `catalog/`, from the repository root:

```sh
B2C_UPDATE_CATALOG_JSON=1 cargo test -p b2c-catalog --test export
pnpm --filter @blocks2cpp/catalog-gen run generate
```

Commit `catalog.json` and the generated files with the change. CI fails when
any of them is stale: `cargo test -p b2c-catalog --test export` compares
`catalog.json`, and the `check` script (or regenerating and running
`git diff --exit-code`) compares the rest.

## Scripts

| Script                              | What it does                                                      |
| ----------------------------------- | ----------------------------------------------------------------- |
| `generate`                          | Writes the outputs that changed.                                  |
| `check`                             | Regenerates in memory and fails if an output is missing or stale. |
| `typecheck`, `lint`, `format:check` | The package's own checks.                                         |
| `test`, `test:coverage`             | Vitest, with the coverage gate of spec §9.2.                      |

The scripts run the TypeScript sources directly with Node.js type stripping
(`--experimental-strip-types`, needed on Node.js 22.13 to 22.17 and accepted
by later versions). Generated TypeScript is formatted with the Prettier
configuration that applies to it, so it passes the target package's
`format:check`.
