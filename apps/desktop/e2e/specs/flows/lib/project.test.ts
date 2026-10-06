/**
 * The flow tests' projects: their block builders and the fixture files are checked against the
 * catalog the app is built from (packages/catalog-gen/catalog.json), and the file helpers against
 * real files.
 */
import { mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { allNodes, type BlockNode, NodeIds, parseDocument, print } from '../../../support/bdm';
import { REPOSITORY_ROOT } from '../../../support/env';
import {
  arithmetic,
  block,
  changeBy,
  declare,
  FIXTURES,
  fileStamp,
  forever,
  moduleBlocks,
  printValue,
  ref,
  repeat,
  waitForWrite,
} from './project';

/** A block definition of the catalog (the parts checked here). */
interface CatalogBlock {
  readonly id: string;
  readonly version: number;
  readonly field: readonly { readonly name: string }[];
  readonly input: readonly { readonly name: string; readonly repeat: unknown }[];
  readonly statement: readonly { readonly name: string; readonly repeat: unknown }[];
  readonly extra: readonly { readonly name: string }[];
}

const catalog = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'packages', 'catalog-gen', 'catalog.json'), 'utf8'),
) as { blocks: CatalogBlock[] };

/** Whether `name` is part `def` of the catalog (`ITEM0` for a repeated `ITEM`). */
function hasPart(defs: readonly { name: string; repeat: unknown }[], name: string): boolean {
  return defs.some((def) =>
    def.repeat === null ? def.name === name : new RegExp(`^${def.name}[0-9]+$`).test(name),
  );
}

/** Checks one block node (not its children) against the catalog. */
function checkAgainstCatalog(node: BlockNode): void {
  const def = catalog.blocks.find((entry) => entry.id === node.type);
  expect(def, node.type).toBeDefined();
  if (def === undefined) {
    return;
  }
  expect(node.v, node.type).toBe(def.version);
  for (const field of Object.keys(node.fields ?? {})) {
    expect(
      def.field.map((entry) => entry.name),
      `${node.type} field`,
    ).toContain(field);
  }
  for (const input of Object.keys(node.inputs ?? {})) {
    expect(hasPart(def.input, input), `${node.type} input ${input}`).toBe(true);
  }
  for (const list of Object.keys(node.statements ?? {})) {
    expect(hasPart(def.statement, list), `${node.type} statement ${list}`).toBe(true);
  }
  for (const extra of Object.keys(node.extra ?? {})) {
    expect(
      def.extra.map((entry) => entry.name),
      `${node.type} extra`,
    ).toContain(extra);
  }
  expect(node.id).toMatch(/^[A-Za-z0-9_]{1,32}$/);
}

const folders: string[] = [];

afterEach(() => {
  for (const folder of folders.splice(0)) {
    rmSync(folder, { recursive: true, force: true });
  }
});

describe('block builders', () => {
  it('build blocks as the catalog defines them, with unique IDs', () => {
    const ids = new NodeIds('flows');
    const roots = [
      declare(ids, { sym: 'sym_n', name: 'n', type: 'int', value: { expr: [{ num: '0' }] } }),
      declare(ids, {
        sym: 'sym_c',
        name: 'c',
        type: 'int',
        value: { expr: [{ num: '1' }] },
        isConst: true,
      }),
      forever(ids, [changeBy(ids, 'sym_n', { expr: [{ num: '1' }] })]),
      repeat(ids, { expr: [{ num: '10' }] }, [print(ids, 'x')]),
      printValue(ids, block(arithmetic(ids, 'div', { expr: [{ num: '100' }] }, ref('sym_n')))),
    ];
    const nodes = allNodes(roots);
    expect(nodes).toHaveLength(8);
    for (const node of nodes) {
      checkAgainstCatalog(node);
    }
    expect(new Set(nodes.map((node) => node.id)).size).toBe(nodes.length);
    expect(roots[1]?.fields?.['CONST']).toBe(true);
    expect(roots[0]?.fields?.['CONST']).toBe(false);
  });
});

describe('fixtures', () => {
  it('are project files whose blocks the catalog defines', () => {
    const files = readdirSync(FIXTURES).filter((name) => name.endsWith('.b2c'));
    expect(files).toContain('scopes.b2c');
    for (const name of files) {
      const doc = parseDocument(readFileSync(path.join(FIXTURES, name), 'utf8'));
      const nodes = doc.modules.flatMap((module) => allNodes(module.workspace.blocks));
      expect(nodes.length, name).toBeGreaterThan(0);
      for (const node of nodes) {
        checkAgainstCatalog(node);
      }
      expect(new Set(nodes.map((node) => node.id)).size, name).toBe(nodes.length);
    }
  });

  it('scopes.b2c has the blocks the variable menu test names', () => {
    const doc = parseDocument(readFileSync(path.join(FIXTURES, 'scopes.b2c'), 'utf8'));
    const ids = allNodes(doc.modules[0]?.workspace.blocks ?? []).map((node) => node.id);
    expect(ids).toEqual(
      expect.arrayContaining(['b_ask', 'b_get', 'b_limit', 'b_later', 'b_inner']),
    );
  });
});

describe('documents and files', () => {
  it('compare blocks without the viewport', () => {
    const blocks: BlockNode[] = [{ id: 'm', type: 'program.main', v: 1, x: 1, y: 2 }];
    const a = { modules: [{ id: 'mod', name: 'main', workspace: { blocks } }] };
    const b = {
      modules: [{ id: 'mod', name: 'main', workspace: { blocks, viewport: { x: 5, y: 6 } } }],
    };
    expect(moduleBlocks(a)).toEqual(moduleBlocks(b));
  });

  it('tell when a file was written again', async () => {
    const folder = mkdtempSync(path.join(tmpdir(), 'b2c-e2e-flows-'));
    folders.push(folder);
    const file = path.join(folder, 'project.b2c');
    setTimeout(() => {
      writeFileSync(file, 'first');
    }, 50);
    expect((await waitForWrite(file, null, 5_000)).toString('utf8')).toBe('first');
    const stamp = fileStamp(file);
    // An atomic save replaces the file: a new one takes its name.
    setTimeout(() => {
      writeFileSync(`${file}.tmp`, 'second!');
      rmSync(file);
      writeFileSync(file, readFileSync(`${file}.tmp`));
    }, 50);
    expect((await waitForWrite(file, stamp, 5_000)).toString('utf8')).toBe('second!');
    await expect(waitForWrite(path.join(folder, 'missing.b2c'), null, 200)).rejects.toThrow(
      'to be written',
    );
  });
});
