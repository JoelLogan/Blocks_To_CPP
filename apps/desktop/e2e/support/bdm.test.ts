import { readFileSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

import {
  allNodes,
  ask,
  type BlockNode,
  compare,
  declaredSymbol,
  declareInt,
  ifElseIfElse,
  NodeIds,
  onlyTopBlock,
  parseDocument,
  print,
  randomInt,
  statementsOf,
  varGet,
} from './bdm';
import { REPOSITORY_ROOT } from './env';

/** The validated catalog the app is built from (packages/catalog-gen/catalog.json). */
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

/** Whether `name` is input or statement `def` of the catalog (`ITEM0` for a repeated `ITEM`). */
function hasPart(defs: readonly { name: string; repeat: unknown }[], name: string): boolean {
  return defs.some((def) =>
    def.repeat === null ? def.name === name : new RegExp(`^${def.name}[0-9]+$`).test(name),
  );
}

/** Checks one block node (not its children) against the catalog. */
function checkAgainstCatalog(node: BlockNode): void {
  const def = catalog.blocks.find((block) => block.id === node.type);
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

describe('block builders', () => {
  it('build the guessing game blocks as the catalog defines them', () => {
    const ids = new NodeIds('game');
    const roots = [
      declareInt(ids, 'sym_a', 'guess', 0),
      randomInt(ids, 1, 100),
      ask(ids, 'Your guess: ', 'sym_a'),
      ask(ids, 'Your guess: ', null),
      ifElseIfElse(ids, {
        cond0: compare(ids, 'lt', varGet(ids, 'sym_a'), varGet(ids, 'sym_b')),
        do0: [print(ids, 'Too low!')],
        cond1: compare(ids, 'gt', varGet(ids, 'sym_a'), varGet(ids, 'sym_b')),
        do1: [print(ids, 'Too high!')],
        otherwise: [print(ids, 'Correct!')],
      }),
    ];
    const nodes = allNodes(roots);
    expect(nodes).toHaveLength(14);
    for (const node of nodes) {
      checkAgainstCatalog(node);
    }
    expect(new Set(nodes.map((node) => node.id)).size).toBe(nodes.length);
    expect(roots[3]?.fields).toEqual({ MODE: 'keep_asking' });
  });

  it('refuses ID prefixes that would make invalid IDs', () => {
    expect(() => new NodeIds('Bad-Prefix')).toThrow();
    expect(() => new NodeIds('')).toThrow();
    expect(new NodeIds('ok1').next()).toBe('e2e_ok1_1');
  });
});

describe('reading documents', () => {
  const ids = new NodeIds('doc');
  const decl = declareInt(ids, 'sym_x', 'x', 1);
  const main: BlockNode = {
    id: 'main1',
    type: 'program.main',
    v: 1,
    x: 0,
    y: 0,
    statements: { BODY: [decl] },
  };
  const text = JSON.stringify({
    modules: [{ id: 'm', name: 'main', workspace: { blocks: [main] } }],
  });

  it('finds the top block, its statements and declared symbols', () => {
    const doc = parseDocument(text);
    expect(onlyTopBlock(doc, 'program.main').id).toBe('main1');
    expect(statementsOf(main, 'BODY')).toEqual([decl]);
    expect(statementsOf(main, 'OTHER')).toEqual([]);
    expect(declaredSymbol(decl, 'NAME')).toEqual({ sym: 'sym_x', name: 'x' });
    expect(declaredSymbol(decl, 'TYPE')).toBeNull();
    expect(() => onlyTopBlock(doc, 'io.print')).toThrow('found 0');
  });

  it('refuses text that is not a document', () => {
    expect(() => parseDocument('{}')).toThrow('no modules');
    expect(() => parseDocument('[]')).toThrow('no modules');
  });

  it('walks nested blocks, statements and stacks in document order', () => {
    const tree: BlockNode = {
      id: 'a',
      type: 't',
      v: 1,
      inputs: { X: { block: { id: 'b', type: 't', v: 1 } }, Y: { expr: [] } },
      statements: { S: [{ id: 'c', type: 't', v: 1 }] },
      stack: [{ id: 'd', type: 't', v: 1 }],
    };
    expect(allNodes([tree]).map((node) => node.id)).toEqual(['a', 'b', 'c', 'd']);
  });
});
