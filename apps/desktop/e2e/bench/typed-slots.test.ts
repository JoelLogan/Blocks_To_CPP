/**
 * The typed-slots benchmarks' own logic, without the app: their document (typed-document.ts) and
 * their metrics (typed-slots-metrics.ts).
 */
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { countBlocks, type Json } from './document';
import { metricValue } from './results';
import {
  columnPath,
  createId,
  generateTypedDocument,
  NUMBER_ID,
  TYPED_BLOCKS,
  TYPED_CREATES,
  TYPED_MAIN_ID,
  writeTypedProject,
} from './typed-document';
import {
  keyboardListMetric,
  typedDragMetric,
  WARM_UP_DRAGS,
  WARM_UP_LISTINGS,
} from './typed-slots-metrics';

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

/** Every block ID of a document, at any depth. */
function blockIds(value: Json | undefined, out: string[] = []): string[] {
  if (Array.isArray(value)) {
    for (const item of value as readonly Json[]) {
      blockIds(item, out);
    }
  } else if (typeof value === 'object' && value !== null) {
    const record = value as Readonly<Record<string, Json>>;
    if (typeof record['id'] === 'string' && typeof record['type'] === 'string') {
      out.push(record['id']);
    }
    for (const child of Object.values(record)) {
      blockIds(child, out);
    }
  }
  return out;
}

describe('the typed-slots document', () => {
  it('has 1,000 blocks with unique IDs, the column first and the number loose', () => {
    const doc = generateTypedDocument();
    expect(countBlocks(doc)).toBe(TYPED_BLOCKS);
    const ids = blockIds(doc);
    expect(new Set(ids).size).toBe(ids.length);
    const top = at(doc, 'modules', 0, 'workspace', 'blocks');
    expect(at(top, 0, 'id')).toBe(TYPED_MAIN_ID);
    expect(at(top, 1, 'id')).toBe(NUMBER_ID);
    expect(at(top, 1, 'type')).toBe('math.number');
    const body = at(top, 0, 'statements', 'BODY');
    expect(at(body, 0, 'id')).toBe(createId(0));
    expect(at(body, TYPED_CREATES - 1, 'id')).toBe(createId(TYPED_CREATES - 1));
    expect(at(body, TYPED_CREATES, 'type')).toBe('var.set');
    expect(at(body, TYPED_CREATES + 1, 'type')).toBe('io.print');
  });

  it('sets and prints the variables after the column, by turns', () => {
    const body = at(generateTypedDocument(), 'modules', 0, 'workspace', 'blocks', 0, 'statements');
    for (const f of [0, 1, 2, 3, 201, TYPED_BLOCKS - 3 - TYPED_CREATES]) {
      const filler = at(body, 'BODY', TYPED_CREATES + f);
      const k = f % TYPED_CREATES;
      const ref = { ref: `s_t_${String(k)}` };
      if (f % 2 === 0) {
        expect(at(filler, 'type')).toBe('var.set');
        expect(at(filler, 'fields', 'VAR')).toEqual(ref);
        expect(at(filler, 'inputs', 'VALUE', 'expr', 0)).toEqual(
          k % 2 === 0 ? { num: '0' } : { str: '' },
        );
      } else {
        expect(at(filler, 'type')).toBe('io.print');
        expect(at(filler, 'inputs', 'ITEM0', 'expr', 0)).toEqual(ref);
      }
    }
  });

  it('alternates whole numbers and text, each with its start value', () => {
    const body = at(generateTypedDocument(), 'modules', 0, 'workspace', 'blocks', 0, 'statements');
    for (const k of [0, 1, 2, 3, TYPED_CREATES - 2, TYPED_CREATES - 1]) {
      const create = at(body, 'BODY', k);
      expect(at(create, 'type')).toBe('var.declare');
      expect(at(create, 'fields', 'TYPE')).toBe(k % 2 === 0 ? 'int' : 'std::string');
      expect(at(create, 'inputs', 'VALUE', 'expr', 0)).toEqual(
        k % 2 === 0 ? { num: '0' } : { str: '' },
      );
    }
  });

  it('is written as a project file', () => {
    const folder = mkdtempSync(path.join(tmpdir(), 'b2c-typed-doc-'));
    folders.push(folder);
    const file = writeTypedProject(folder);
    expect(path.basename(file)).toBe('bench-typed-1000.b2c');
    expect(JSON.parse(readFileSync(file, 'utf8'))).toEqual(generateTypedDocument());
  });
});

describe('the typed-slots metrics', () => {
  it('pool the timed drags and keep the timed listings, both reported only', () => {
    const frames = typedDragMetric([
      [100, 100],
      [16, 17],
      [18, 50],
    ]);
    expect(frames.name).toBe('webview.drag-typed-1000.frame-p95-all');
    expect(frames.gate).toBe(false);
    expect(frames.statistic).toBe('p95');
    expect(frames.samples).toEqual([16, 17, 18, 50]);
    expect(metricValue(frames)).toBe(50);
    const listing = keyboardListMetric([900, 12, 14, 13]);
    expect(listing.name).toBe('webview.keyboard-list-1000.ms');
    expect(listing.gate).toBe(false);
    expect(listing.samples).toEqual([12, 14, 13]);
    expect(metricValue(listing)).toBe(13);
  });

  it('refuse a run of warm-ups only, or a timed drag without frames', () => {
    expect(() => typedDragMetric(Array.from({ length: WARM_UP_DRAGS }, () => [16]))).toThrow(
      /only warms up/,
    );
    expect(() => keyboardListMetric(Array.from({ length: WARM_UP_LISTINGS }, () => 10))).toThrow(
      /only warms up/,
    );
    expect(() => typedDragMetric([[16], []])).toThrow(/no frame times/);
  });
});

describe('columnPath', () => {
  it('goes through each value input top to bottom and back to where it pressed', () => {
    const grab = { x: 500, y: 100 };
    const own = { x: 480, y: 110 };
    const targets = [
      { x: 300, y: 250 },
      { x: 300, y: 150 },
      { x: 300, y: 200 },
    ];
    const path = columnPath(grab, own, targets, 60);
    expect(path.at(-1)).toEqual(grab);
    // The block's output connection passes over each input, in order from the top.
    const over = (target: { x: number; y: number }) =>
      path.findIndex((point) => point.x - 20 === target.x && point.y + 10 === target.y);
    const order = [150, 200, 250].map((y) => over({ x: 300, y }));
    expect(order.every((index) => index >= 0)).toBe(true);
    expect([...order].sort((a, b) => a - b)).toEqual(order);
    expect(path.length).toBeGreaterThanOrEqual(58);
    expect(path.length).toBeLessThanOrEqual(64);
  });

  it('makes one move per segment at least', () => {
    const grab = { x: 0, y: 0 };
    expect(columnPath(grab, grab, [{ x: 0, y: 0 }], 10)).toEqual([grab, grab]);
  });
});
