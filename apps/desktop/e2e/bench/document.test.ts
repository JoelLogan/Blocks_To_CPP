/**
 * The generated benchmark document (document.ts): its size, its shape, and its parity with the Rust
 * generator of the native benchmark (crates/b2c-core-wasm/benches/pipeline).
 */
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { REPOSITORY_ROOT } from '../support/env';
import {
  countBlocks,
  generateDocument,
  HANDLE_BLOCKS,
  handleId,
  type Json,
  MAIN_ID,
  MAX_BLOCKS,
  minBlocks,
  parityDigest,
  ShapeError,
  sortedJson,
  UNIT_BLOCKS,
  writeBenchProject,
} from './document';
import { PREVIEW_DEBOUNCE_MS } from './page';

/**
 * The digests the Rust generator gives (crates/b2c-core-wasm/benches/pipeline/tests.rs
 * `DIGEST_1000` and `DIGEST_5000_HANDLE`). Change both copies together with both generators.
 */
const DIGEST_1000 = 'a1816065efc298a5ffb8647caeb57f9e32b9b40bfe724938c82572b18ffae64f';
const DIGEST_5000_HANDLE = '05900f8a729be4e3c46e1d16ec6d3ad7359b430a9e9df019241fe96afa7fb70d';

const folders: string[] = [];

afterEach(() => {
  for (const folder of folders.splice(0)) {
    rmSync(folder, { recursive: true, force: true });
  }
});

/** A property of a JSON object, or `undefined`. */
function at(value: Json | undefined, ...keys: (string | number)[]): Json | undefined {
  let current: Json | undefined = value;
  for (const key of keys) {
    if (typeof current !== 'object' || current === null) {
      return undefined;
    }
    current = (current as Record<string | number, Json | undefined>)[key];
  }
  return current;
}

describe('generateDocument', () => {
  it('makes exactly the requested number of blocks', () => {
    for (const blocks of [1, 2, 8, 9, 10, 16, 17, 100, 1_000, 1_001, 5_000]) {
      expect(countBlocks(generateDocument({ blocks, dragHandle: false })), String(blocks)).toBe(
        blocks,
      );
    }
    for (const blocks of [3, 4, 11, 1_000, 5_000]) {
      expect(countBlocks(generateDocument({ blocks, dragHandle: true })), String(blocks)).toBe(
        blocks,
      );
    }
  });

  it('gives the same documents as the Rust generator', () => {
    expect(parityDigest(generateDocument({ blocks: 1_000, dragHandle: false }))).toBe(DIGEST_1000);
    expect(parityDigest(generateDocument({ blocks: 5_000, dragHandle: true }))).toBe(
      DIGEST_5000_HANDLE,
    );
  });

  it('fills main with units of eight blocks, then fillers', () => {
    const doc = generateDocument({ blocks: 1 + 2 * UNIT_BLOCKS + 3, dragHandle: false });
    const body = at(doc, 'modules', 0, 'workspace', 'blocks', 0, 'statements', 'BODY');
    expect(Array.isArray(body)).toBe(true);
    const types = (body as Json[]).map((block) => at(block, 'type'));
    expect(types).toEqual([
      'var.declare',
      'control.for_range',
      'io.print',
      'var.declare',
      'control.for_range',
      'io.print',
      'io.print',
      'io.print',
      'io.print',
    ]);
    expect(at(body, 8, 'inputs', 'ITEM0', 'expr', 0, 'str')).toBe('filler 2');
    expect(at(body, 4, 'fields', 'VAR', 'name')).toBe('i_1');
  });

  it('numbers the blocks in document order', () => {
    const doc = generateDocument({ blocks: 1 + UNIT_BLOCKS, dragHandle: false });
    const main = at(doc, 'modules', 0, 'workspace', 'blocks', 0);
    expect(at(main, 'id')).toBe(MAIN_ID);
    expect(at(main, 'statements', 'BODY', 0, 'id')).toBe('b1');
    expect(at(main, 'statements', 'BODY', 1, 'id')).toBe('b2');
    const branch = at(main, 'statements', 'BODY', 1, 'statements', 'BODY', 0);
    expect(at(branch, 'id')).toBe('b3');
    expect(at(branch, 'statements', 'DO0', 0, 'id')).toBe('b4');
    expect(at(branch, 'statements', 'ELSE', 0, 'id')).toBe('b5');
    expect(at(main, 'statements', 'BODY', 2, 'id')).toBe('b6');
    expect(at(main, 'statements', 'BODY', 2, 'inputs', 'ITEM1', 'block', 'id')).toBe('b7');
    expect(
      at(main, 'statements', 'BODY', 2, 'inputs', 'ITEM1', 'block', 'inputs', 'A', 'block', 'id'),
    ).toBe('b8');
  });

  it('puts the drag handle beside main', () => {
    const doc = generateDocument({ blocks: 1_000, dragHandle: true });
    const handle = at(doc, 'modules', 0, 'workspace', 'blocks', 1);
    expect(at(handle, 'type')).toBe('func.define');
    expect(at(handle, 'id')).toBe(handleId(1_000));
    expect(handleId(1_000)).toBe(`b${String(1_000 - HANDLE_BLOCKS)}`);
    expect(at(handle, 'x')).toBe(800);
    expect(at(handle, 'statements', 'BODY', 0, 'id')).toBe('b999');
  });

  it('refuses counts outside the limits', () => {
    expect(minBlocks({ dragHandle: false })).toBe(1);
    expect(minBlocks({ dragHandle: true })).toBe(3);
    expect(() => generateDocument({ blocks: 0, dragHandle: false })).toThrow(ShapeError);
    expect(() => generateDocument({ blocks: 2, dragHandle: true })).toThrow(/at least 3/);
    expect(() => generateDocument({ blocks: 1.5, dragHandle: false })).toThrow(ShapeError);
    expect(() => generateDocument({ blocks: MAX_BLOCKS + 1, dragHandle: false })).toThrow(
      /at most 100000/,
    );
  });
});

describe('sortedJson and parityDigest', () => {
  it('sort keys at every level and keep array order', () => {
    expect(sortedJson({ b: [1, { y: true, x: 's' }], a: null })).toBe(
      '{"a":null,"b":[1,{"x":"s","y":true}]}',
    );
    // `printf '%s' '{"a":null,"b":[1,{"x":"s","y":true}]}' | sha256sum`, as in the Rust test.
    expect(parityDigest({ b: [1, { y: true, x: 's' }], a: null })).toBe(
      '704112698874bfb52b267e9cec3b2285641e983134d36cdf5f86e283eb7c737e',
    );
    expect(parityDigest({ a: [2, 1] })).not.toBe(parityDigest({ a: [1, 2] }));
  });
});

describe('countBlocks', () => {
  it('counts stacks and blocks in inputs, and nothing else', () => {
    const doc: Json = {
      modules: [
        {
          workspace: {
            blocks: [
              { id: 'a', stack: [{ id: 'b' }, { id: 'c', inputs: { X: { block: { id: 'd' } } } }] },
              { id: 'e', inputs: { Y: { expr: [{ num: '1' }] } } },
            ],
          },
        },
      ],
    };
    expect(countBlocks(doc)).toBe(5);
    expect(countBlocks(null)).toBe(0);
    expect(countBlocks({ modules: 'none' })).toBe(0);
  });
});

describe('writeBenchProject', () => {
  it('writes the document as a project file', () => {
    const folder = mkdtempSync(path.join(tmpdir(), 'b2c-bench-doc-'));
    folders.push(folder);
    const file = writeBenchProject(folder, { blocks: 100, dragHandle: true });
    expect(path.basename(file)).toBe('bench-100-drag.b2c');
    const text = readFileSync(file, 'utf8');
    expect(text.endsWith('}\n')).toBe(true);
    expect(JSON.parse(text)).toEqual(generateDocument({ blocks: 100, dragHandle: true }));
  });
});

describe('the preview benchmark', () => {
  it('subtracts the debounce the editor really uses', () => {
    const pipeline = readFileSync(
      path.join(REPOSITORY_ROOT, 'apps/desktop/src/editor/preview/pipeline.ts'),
      'utf8',
    );
    const match = /export const PREVIEW_DEBOUNCE_MS = (\d+);/.exec(pipeline);
    expect(match?.[1]).toBe(String(PREVIEW_DEBOUNCE_MS));
  });
});
