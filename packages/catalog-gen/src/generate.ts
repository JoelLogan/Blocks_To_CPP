// The whole generation, without touching the file system: catalog.json text in, the generated
// files out. The command line (cli.ts) reads and writes; the tests call this directly.

import { readCatalogJson } from './catalog-json.ts';
import { CATALOG_TS_PATH, catalogTs } from './emit-ts.ts';
import { REFERENCE_DIR, blockReference } from './emit-markdown.ts';
import { normalize } from './normalize.ts';

/** catalog.json, relative to the repository root. */
export const CATALOG_JSON_PATH = 'packages/catalog-gen/catalog.json';

/** catalog-types.ts, relative to the repository root. */
export const CATALOG_TYPES_PATH = 'packages/catalog-gen/src/catalog-types.ts';

/** A generated file: its path relative to the repository root (with `/`) and its text. */
export interface Output {
  readonly path: string;
  readonly content: string;
}

/** What the generation reads. */
export interface Inputs {
  /** The text of catalog.json. */
  readonly catalogJson: string;
  /** The text of catalog-types.ts. */
  readonly catalogTypes: string;
  /** Formats TypeScript the way the target package's Prettier configuration does. */
  readonly formatTypeScript: (source: string, path: string) => Promise<string>;
}

/** The generated files' names in the reference directory: `README.md` and `<category>.md`. */
const REFERENCE_FILE = /^(?:README|[a-z][a-z_]*)\.md$/u;

/**
 * Generates every output: the editor's module and the block reference pages.
 *
 * @throws {import('./catalog-json.ts').CatalogJsonError} when catalog.json does not have the
 *   shape of the export.
 */
export async function generate(inputs: Inputs): Promise<Output[]> {
  const model = normalize(readCatalogJson(inputs.catalogJson));
  const outputs: Output[] = [
    {
      path: CATALOG_TS_PATH,
      content: await inputs.formatTypeScript(
        catalogTs(model, inputs.catalogTypes),
        CATALOG_TS_PATH,
      ),
    },
  ];
  for (const [name, content] of blockReference(model)) {
    if (!REFERENCE_FILE.test(name)) {
      throw new Error(
        `refusing to write the unexpected reference file name ${JSON.stringify(name)}`,
      );
    }
    outputs.push({ path: `${REFERENCE_DIR}/${name}`, content });
  }
  return outputs;
}

/** Whether a file in the reference directory is one the generator owns. */
export function isReferenceFile(name: string): boolean {
  return REFERENCE_FILE.test(name);
}
