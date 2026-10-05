// The command line of catalog-gen (package.json scripts `generate` and `check`):
//
//   node --experimental-strip-types src/cli.ts write   regenerate the outputs
//   node --experimental-strip-types src/cli.ts check   fail when an output is stale
//
// Inputs: packages/catalog-gen/catalog.json (from crates/b2c-catalog/tests/export.rs) and
// src/catalog-types.ts. Outputs: packages/blockly-ext/src/generated/catalog.ts and
// docs/reference/blocks/. Paths are relative to the repository root, wherever this runs from.

import { fileURLToPath } from 'node:url';

import { checkOutputs, generateFrom, writeOutputs } from './files.ts';

const ROOT = fileURLToPath(new URL('../../../', import.meta.url));

const USAGE = 'usage: cli.ts write|check';

const REGENERATE =
  'Regenerate with `B2C_UPDATE_CATALOG_JSON=1 cargo test -p b2c-catalog --test export` and `pnpm --filter @blocks2cpp/catalog-gen run generate`, then commit the results.';

async function main(args: readonly string[]): Promise<number> {
  const [command, ...rest] = args;
  if ((command !== 'write' && command !== 'check') || rest.length > 0) {
    console.error(USAGE);
    return 2;
  }
  const outputs = await generateFrom(ROOT);
  if (command === 'write') {
    const changed = await writeOutputs(ROOT, outputs);
    for (const file of changed) {
      console.log(`updated ${file}`);
    }
    console.log(`${String(outputs.length)} generated files, ${String(changed.length)} updated.`);
    return 0;
  }
  const problems = await checkOutputs(ROOT, outputs);
  if (problems.length > 0) {
    for (const problem of problems) {
      console.error(problem);
    }
    console.error(REGENERATE);
    return 1;
  }
  console.log(`The ${String(outputs.length)} generated files are current.`);
  return 0;
}

try {
  process.exitCode = await main(process.argv.slice(2));
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
}
