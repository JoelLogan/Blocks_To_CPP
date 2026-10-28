// The scope index with the real compiler core, built by
// `pnpm --filter @blocks2cpp/b2c-core-wasm build` (docs/plans/m3-contracts/worker.md §8): for every
// example, every security project that loads, and those documents changed by seeded edits (a
// block disabled, a statement parked on the canvas, a stack deleted), `ScopeIndex.symbolsAt`
// answers exactly as the core's own `symbolsInScope` for every block, with no input, each of its
// inputs and a name it lacks; `visibleAt` binds a paste as the core does. Malformed indexes are
// refused. Without a build these tests are skipped, unless B2C_REQUIRE_WASM is set (as in CI).

import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { beforeAll, describe, expect, it } from 'vitest';

import type { PreviewResult } from '../src/api';
import { type CoreHandle } from '../src/core';
import { CoreError } from '../src/errors';
import { instantiate } from '../src/instance';
import { ScopeIndex } from '../src/scopeIndex';

const repo = (path: string) => fileURLToPath(new URL(`../../../${path}`, import.meta.url));
const builtModule = fileURLToPath(new URL('../pkg/b2c_core_wasm_bg.wasm', import.meta.url));
const built = existsSync(builtModule);
const required = (process.env['B2C_REQUIRE_WASM'] ?? '') !== '';

if (!built && !required) {
  console.warn(
    'test/scopeIndex.test.ts skipped: build the module first (pnpm --filter @blocks2cpp/b2c-core-wasm build)',
  );
}

type Json = Record<string, unknown>;

function files(dir: string): { name: string; text: string }[] {
  return readdirSync(repo(dir))
    .filter((name) => name.endsWith('.b2c'))
    .sort()
    .map((name) => ({
      name: `${dir}/${name}`,
      text: readFileSync(repo(`${dir}/${name}`), 'utf8'),
    }));
}

/** Every block of a document with the names of its inputs and statement lists. */
function blocksOf(document: Json): { id: string; inputs: string[] }[] {
  const found: { id: string; inputs: string[] }[] = [];
  const pending: unknown[] = [];
  for (const module of (document['modules'] as Json[] | undefined) ?? []) {
    pending.push(
      ...(((module['workspace'] as Json | undefined)?.['blocks'] as unknown[] | undefined) ?? []),
    );
  }
  while (pending.length > 0) {
    const block = pending.pop() as Json | null;
    if (block === null || typeof block !== 'object' || typeof block['id'] !== 'string') {
      continue;
    }
    const inputs: string[] = [];
    for (const key of ['inputs', 'statements']) {
      const map = block[key];
      if (typeof map === 'object' && map !== null) {
        for (const [name, value] of Object.entries(map as Json)) {
          inputs.push(name);
          if (Array.isArray(value)) {
            pending.push(...(value as unknown[]));
          } else if (typeof value === 'object' && value !== null) {
            pending.push((value as Json)['block'] ?? null);
          }
        }
      }
    }
    pending.push(...((block['stack'] as unknown[] | undefined) ?? []));
    found.push({ id: block['id'], inputs });
  }
  return found;
}

/** A small seeded generator, so a failure names an edit that can be made again. */
function seeded(seed: number): () => number {
  let state = seed;
  return () => {
    state = (state * 1_103_515_245 + 12_345) % 2 ** 31;
    return state / 2 ** 31;
  };
}

/** The statement lists of a document, each with the list that holds it. */
function statementLists(document: Json): unknown[][] {
  const lists: unknown[][] = [];
  const pending: unknown[] = [];
  for (const module of (document['modules'] as Json[] | undefined) ?? []) {
    pending.push(
      ...(((module['workspace'] as Json | undefined)?.['blocks'] as unknown[] | undefined) ?? []),
    );
  }
  while (pending.length > 0) {
    const block = pending.pop() as Json | null;
    if (block === null || typeof block !== 'object') {
      continue;
    }
    const statements = block['statements'];
    if (typeof statements === 'object' && statements !== null) {
      for (const list of Object.values(statements as Json)) {
        if (Array.isArray(list) && list.length > 0) {
          lists.push(list as unknown[]);
          pending.push(...(list as unknown[]));
        }
      }
    }
  }
  return lists;
}

/** The document with seeded edits: statements disabled, parked on the canvas or deleted. */
function edited(text: string, seed: number): string {
  const document = JSON.parse(text) as Json;
  const random = seeded(seed);
  const canvas =
    (((document['modules'] as Json[] | undefined)?.[0]?.['workspace'] as Json | undefined)?.[
      'blocks'
    ] as unknown[] | undefined) ?? [];
  for (let edit = 0; edit < 3; edit += 1) {
    const lists = statementLists(document);
    const list = lists[Math.floor(random() * lists.length)];
    if (list === undefined || list.length === 0) {
      break;
    }
    const at = Math.floor(random() * list.length);
    const choice = random();
    if (choice < 0.4) {
      (list[at] as Json)['disabled'] = true;
    } else if (choice < 0.8) {
      const [moved] = list.splice(at, 1) as Json[];
      if (moved !== undefined) {
        canvas.push({ ...moved, x: 900, y: 900 + 50 * edit });
      }
    } else {
      list.splice(at, 1);
    }
  }
  return JSON.stringify(document);
}

describe.skipIf(!built && !required)('the scope index with the real core', () => {
  let handle: CoreHandle;
  beforeAll(async () => {
    handle = await instantiate(new WebAssembly.Module(readFileSync(builtModule)));
  });

  /** Updates, reads the index, and compares every answer; returns the questions asked. */
  function check(name: string, text: string): number {
    const result = handle.core.update(text, { indentWidth: 4 });
    if (result.contentHash === null) {
      return 0;
    }
    const index = ScopeIndex.parse(handle.raw.scopeIndex(), result);
    let asked = 0;
    const document = JSON.parse(text) as Json;
    for (const { id, inputs } of blocksOf(document)) {
      for (const input of [null, ...inputs, 'NOPE']) {
        expect(index.symbolsAt(id, input), `${name}: ${id} ${String(input)}`).toEqual(
          handle.core.symbolsInScope(id, input),
        );
        asked += 1;
      }
    }
    expect(index.symbolsAt('no-such-block', null)).toBeNull();
    expect(index.parkedScope('no-such-module')).toEqual([]);
    return asked;
  }

  it('answers as the core for every example', () => {
    let asked = 0;
    for (const { name, text } of files('examples')) {
      asked += check(name, text);
    }
    expect(asked).toBeGreaterThan(500);
  });

  it('answers as the core for every security project that loads', () => {
    for (const { name, text } of files('tests/security/projects')) {
      check(name, text);
    }
  });

  it('answers as the core after seeded edits', () => {
    let asked = 0;
    for (const { name, text } of files('examples')) {
      for (const seed of [1, 2, 3]) {
        asked += check(`${name} (edits ${String(seed)})`, edited(text, seed));
      }
    }
    expect(asked).toBeGreaterThan(1_000);
  });

  it('binds a paste against what the core binds against', () => {
    // A copied reference to `secret`, pasted after each block of the guessing game: it binds
    // when `visibleAt` there lists a `secret`, as the core's own binding does.
    const text = readFileSync(repo('examples/guessing_game.b2c'), 'utf8');
    const result = handle.core.update(text, { indentWidth: 4 });
    const index = ScopeIndex.parse(handle.raw.scopeIndex(), result);
    const document = JSON.parse(text) as Json;
    const module = ((document['modules'] as Json[])[0]?.['id'] as string | undefined) ?? '';
    const copied = handle.core.clipboardMake(text, ['b005']);
    expect(copied.ok).toBe(true);
    if (!copied.ok) {
      return;
    }
    for (const { id } of blocksOf(document)) {
      const pasted = handle.core.pastePrepare(
        copied.payload,
        text,
        { module, block: id, input: null },
        '00'.repeat(32),
      );
      if (!pasted.ok) {
        continue;
      }
      const visible = index.visibleAt(module, id, null).map((info) => info.name);
      const bound = pasted.unresolved.length === 0;
      expect(bound, id).toBe(visible.includes('guess'));
    }
    expect(index.visibleAt(module, null, null)).toEqual(index.parkedScope(module));
  });
});

describe('ScopeIndex.parse', () => {
  const preview = {
    contentHash: 'aa',
    symbols: [{ id: 's1', name: 'x', kind: 'variable', isConst: false, type: 'int', module: 'm' }],
  } as unknown as PreviewResult;
  const good = {
    v: 1,
    contentHash: 'aa',
    modules: ['m'],
    names: ['x'],
    trail: [-1, 0, 0],
    functions: [[]],
    parked: [0],
    blocks: [['b1', 0, 0, 'BODY', -1, 0]],
    members: [],
    created: [['b0', 0]],
  };
  const parse = (value: unknown) => ScopeIndex.parse(JSON.stringify(value), preview);

  it('reads a good index and answers from it', () => {
    const index = parse(good);
    expect(index.symbolsAt('b1', null)?.map((info) => info.id)).toEqual(['s1']);
    expect(index.symbolsAt('b1', 'BODY')).toEqual([]);
    expect(index.symbolsAt('b1', 'ELSE')?.length).toBe(1);
    expect(index.parkedScope('m').map((info) => info.id)).toEqual(['s1']);
    expect(index.symbolsAfter('b0')).toBeNull();
  });

  it.each([
    ['not JSON', '{'],
    ['not an object', '[]'],
    ['another version', { ...good, v: 2 }],
    ['another document', { ...good, contentHash: 'bb' }],
    ['a parent not smaller than its node', { ...good, trail: [0, 0, 0] }],
    ['a name out of range', { ...good, trail: [-1, 1, 0] }],
    ['a symbol out of range', { ...good, trail: [-1, 0, 1] }],
    ['a trail cut short', { ...good, trail: [-1, 0] }],
    ['a parked point out of range', { ...good, parked: [1] }],
    ['parked points for another module count', { ...good, parked: [0, 0] }],
    ['a block in a module out of range', { ...good, blocks: [['b1', 0, 1]] }],
    ['a block without positions', { ...good, blocks: [['b1']] }],
    ['an input out of range', { ...good, blocks: [['b1', 0, 0, 'BODY', 1, 0]] }],
    ['a function list out of range', { ...good, functions: [[0, 3]] }],
    ['a member list out of range', { ...good, members: [[0, 9]] }],
    ['a created pair out of range', { ...good, created: [['b0', 9]] }],
    ['a list that is not one', { ...good, members: 'x' }],
  ])('refuses %s', (_what, value) => {
    expect(() =>
      typeof value === 'string' ? ScopeIndex.parse(value, preview) : parse(value),
    ).toThrow(CoreError);
  });

  it('refuses an index for a preview that did not load', () => {
    expect(() =>
      ScopeIndex.parse(JSON.stringify(good), {
        ...preview,
        contentHash: null,
      }),
    ).toThrow(CoreError);
  });
});
