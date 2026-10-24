import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { CatalogJsonError } from '../src/catalog-json.ts';
import { CATALOG_TS_PATH } from '../src/emit-ts.ts';
import { checkOutputs, formatTypeScript, generateFrom, writeOutputs } from '../src/files.ts';
import {
  CATALOG_JSON_PATH,
  CATALOG_TYPES_PATH,
  generate,
  isReferenceFile,
} from '../src/generate.ts';
import { fixtureText, repoPath } from './fixture.ts';

let root = '';

/** A scratch repository holding the fixture catalog and the real catalog-types.ts. */
beforeEach(async () => {
  root = await mkdtemp(path.join(tmpdir(), 'catalog-gen-'));
  await mkdir(path.join(root, path.dirname(CATALOG_JSON_PATH)), { recursive: true });
  await mkdir(path.join(root, path.dirname(CATALOG_TYPES_PATH)), { recursive: true });
  await writeFile(path.join(root, CATALOG_JSON_PATH), fixtureText());
  await writeFile(
    path.join(root, CATALOG_TYPES_PATH),
    await readFile(repoPath(CATALOG_TYPES_PATH), 'utf8'),
  );
});

afterEach(async () => {
  await rm(root, { recursive: true, force: true });
});

const reference = (name: string): string => path.join(root, 'docs/reference/blocks', name);

describe('writing and checking the outputs', () => {
  it('writes every output once, then finds them current', async () => {
    const outputs = await generateFrom(root);
    expect(outputs.map((o) => o.path)).toEqual([
      CATALOG_TS_PATH,
      'docs/reference/blocks/README.md',
      'docs/reference/blocks/text.md',
      'docs/reference/blocks/control.md',
      'docs/reference/blocks/loops.md',
      'docs/reference/blocks/functions.md',
    ]);
    expect(await checkOutputs(root, outputs)).toEqual(outputs.map((o) => `${o.path} is missing`));
    expect(await writeOutputs(root, outputs)).toEqual(outputs.map((o) => o.path));
    expect(await checkOutputs(root, outputs)).toEqual([]);
    expect(await writeOutputs(root, outputs)).toEqual([]);
    const written = await readFile(path.join(root, CATALOG_TS_PATH), 'utf8');
    expect(written).toContain("export const CATALOG_VERSION = '9.8.7';");
  });

  it('finds stale outputs and reference pages that are no longer generated', async () => {
    const outputs = await generateFrom(root);
    await writeOutputs(root, outputs);
    await writeFile(reference('text.md'), '# Changed\n');
    await writeFile(reference('lists.md'), '# Lists\n');
    await writeFile(reference('notes.txt'), 'not a reference page');
    await mkdir(reference('images.md'));
    expect(await checkOutputs(root, outputs)).toEqual([
      'docs/reference/blocks/text.md is stale',
      'docs/reference/blocks/lists.md is no longer generated',
    ]);
    expect(await writeOutputs(root, outputs)).toEqual([
      'docs/reference/blocks/text.md',
      'docs/reference/blocks/lists.md',
    ]);
    expect((await readdir(reference(''))).sort()).toEqual([
      'README.md',
      'control.md',
      'functions.md',
      'images.md',
      'loops.md',
      'notes.txt',
      'text.md',
    ]);
    expect(await checkOutputs(root, outputs)).toEqual([]);
  });

  it('checks a repository without a reference directory', async () => {
    const outputs = await generateFrom(root);
    const problems = await checkOutputs(root, outputs);
    expect(problems).toHaveLength(outputs.length);
  });

  it('refuses a catalog.json that is too large or malformed', async () => {
    await writeFile(path.join(root, CATALOG_JSON_PATH), '{"catalogVersion": 1}');
    await expect(generateFrom(root)).rejects.toBeInstanceOf(CatalogJsonError);
    await writeFile(path.join(root, CATALOG_JSON_PATH), ' '.repeat(4 * 1024 * 1024 + 1));
    await expect(generateFrom(root)).rejects.toThrow(/more than the limit/u);
  });

  it('formats TypeScript with the repository style when no configuration applies', async () => {
    const formatted = await formatTypeScript(
      'export const A = {"b": "c"}',
      path.join(root, 'x/y.ts'),
    );
    expect(formatted).toBe("export const A = { b: 'c' };\n");
  });

  it('refuses reference file names it does not own', async () => {
    expect(isReferenceFile('README.md')).toBe(true);
    expect(isReferenceFile('io.md')).toBe(true);
    expect(isReferenceFile('../io.md')).toBe(false);
    expect(isReferenceFile('IO.md')).toBe(false);
    expect(isReferenceFile('io.txt')).toBe(false);
    const outputs = await generate({
      catalogJson: fixtureText(),
      catalogTypes: '',
      formatTypeScript: (source) => Promise.resolve(source),
    });
    expect(outputs).toHaveLength(6);
  });
});
