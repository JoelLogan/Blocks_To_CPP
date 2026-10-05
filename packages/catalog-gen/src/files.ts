// Reading the inputs and writing or checking the outputs, all relative to a repository root.

import { mkdir, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';

import { format, resolveConfig, type Options } from 'prettier';

import { MAX_CATALOG_JSON_BYTES } from './catalog-json.ts';
import { REFERENCE_DIR } from './emit-markdown.ts';
import {
  CATALOG_JSON_PATH,
  CATALOG_TYPES_PATH,
  generate,
  isReferenceFile,
  type Output,
} from './generate.ts';

/** The repository's Prettier style, used when the target package has no configuration. */
const FALLBACK_PRETTIER: Options = { singleQuote: true, printWidth: 100 };

/** The largest source file (catalog-types.ts) read, in bytes. */
const MAX_SOURCE_BYTES = 1024 * 1024;

/** Generates every output from the inputs under `root`. */
export async function generateFrom(root: string): Promise<Output[]> {
  return generate({
    catalogJson: await readBounded(path.join(root, CATALOG_JSON_PATH), MAX_CATALOG_JSON_BYTES),
    catalogTypes: await readBounded(path.join(root, CATALOG_TYPES_PATH), MAX_SOURCE_BYTES),
    formatTypeScript: (source, relative) => formatTypeScript(source, path.join(root, relative)),
  });
}

/**
 * Formats TypeScript with the Prettier configuration that applies to the target file, so the
 * output passes the target package's `format:check`.
 */
export async function formatTypeScript(source: string, filepath: string): Promise<string> {
  const config = (await resolveConfig(filepath)) ?? FALLBACK_PRETTIER;
  return format(source, { ...config, parser: 'typescript', filepath });
}

/** A file's text, refused when it is larger than `maxBytes`. */
async function readBounded(file: string, maxBytes: number): Promise<string> {
  const { size } = await stat(file);
  if (size > maxBytes) {
    throw new Error(
      `${file} has ${String(size)} bytes, more than the limit of ${String(maxBytes)}`,
    );
  }
  return readFile(file, 'utf8');
}

/** A file's text, or null when there is no such file. */
async function readIfExists(file: string): Promise<string | null> {
  try {
    return await readFile(file, 'utf8');
  } catch (error) {
    if (isNotFound(error)) {
      return null;
    }
    throw error;
  }
}

function isNotFound(error: unknown): boolean {
  return error instanceof Error && 'code' in error && error.code === 'ENOENT';
}

/**
 * Writes the outputs that differ from the files under `root`, and removes reference pages that
 * are no longer generated. Returns the paths it changed.
 */
export async function writeOutputs(root: string, outputs: readonly Output[]): Promise<string[]> {
  const changed: string[] = [];
  for (const output of outputs) {
    const file = path.join(root, output.path);
    if ((await readIfExists(file)) !== output.content) {
      await mkdir(path.dirname(file), { recursive: true });
      await writeFile(file, output.content, 'utf8');
      changed.push(output.path);
    }
  }
  for (const name of await staleReferenceFiles(root, outputs)) {
    await rm(path.join(root, REFERENCE_DIR, name));
    changed.push(`${REFERENCE_DIR}/${name}`);
  }
  return changed;
}

/** Every difference between the outputs and the files under `root`, as messages. */
export async function checkOutputs(root: string, outputs: readonly Output[]): Promise<string[]> {
  const problems: string[] = [];
  for (const output of outputs) {
    const current = await readIfExists(path.join(root, output.path));
    if (current === null) {
      problems.push(`${output.path} is missing`);
    } else if (current !== output.content) {
      problems.push(`${output.path} is stale`);
    }
  }
  for (const name of await staleReferenceFiles(root, outputs)) {
    problems.push(`${REFERENCE_DIR}/${name} is no longer generated`);
  }
  return problems;
}

/**
 * Reference pages in the reference directory that the generator would not write now (for
 * example the page of a category that was removed). The directory holds generated files only.
 */
async function staleReferenceFiles(root: string, outputs: readonly Output[]): Promise<string[]> {
  const prefix = `${REFERENCE_DIR}/`;
  const expected = new Set(
    outputs.filter((o) => o.path.startsWith(prefix)).map((o) => o.path.slice(prefix.length)),
  );
  let entries;
  try {
    entries = await readdir(path.join(root, REFERENCE_DIR), { withFileTypes: true });
  } catch (error) {
    if (isNotFound(error)) {
      return [];
    }
    throw error;
  }
  return entries
    .filter((entry) => entry.isFile() && isReferenceFile(entry.name) && !expected.has(entry.name))
    .map((entry) => entry.name)
    .sort();
}
