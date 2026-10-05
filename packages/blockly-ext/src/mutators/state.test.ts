/** Reading `extra` into mutator state: defaults, limits and typed errors. */
import { describe, expect, it } from 'vitest';

import { blockDef } from '../checker/catalog';
import type { BlockDefJson } from '../generated/catalog';
import { MutatorStateError, type MutatorStateProblem } from './errors';
import { paramsSpec, variadicSpec } from './spec';
import {
  isShowableName,
  MAX_VARIADIC_PARTS,
  readParamsExtra,
  readVariadicExtra,
  writeVariadicExtra,
} from './state';

function def(id: string): BlockDefJson {
  const found = blockDef(id);
  if (found === undefined) {
    throw new Error(`no catalog block ${id}`);
  }
  return found;
}

const ifSpec = variadicSpec(def('control.if'));
const printSpec = variadicSpec(def('io.print'));
const defineSpec = paramsSpec(def('func.define'));

function problemOf(read: () => unknown): [MutatorStateProblem, string | null] {
  try {
    read();
  } catch (error) {
    if (error instanceof MutatorStateError) {
      return [error.problem, error.key];
    }
    throw error;
  }
  throw new Error('no error');
}

const row = { sym: 'sym_a', name: 'a', type: 'int', mode: 'copy' };

describe('readVariadicExtra', () => {
  it('fills absent keys with catalog defaults', () => {
    const state = readVariadicExtra(ifSpec, {});
    expect(state.count).toBe(0);
    expect(state.flags.get('hasElse')).toBe(false);
    expect(writeVariadicExtra(ifSpec, state)).toEqual({ elseIfCount: 0, hasElse: false });
  });

  it('keeps counts outside the catalog range but within the project limit', () => {
    expect(readVariadicExtra(printSpec, { itemCount: 0 }).count).toBe(0);
    expect(readVariadicExtra(printSpec, { itemCount: MAX_VARIADIC_PARTS }).count).toBe(64);
  });

  it('writes the count first, then the flags', () => {
    const state = readVariadicExtra(ifSpec, { hasElse: true, elseIfCount: 3 });
    expect(Object.keys(writeVariadicExtra(ifSpec, state))).toEqual(['elseIfCount', 'hasElse']);
  });

  it('raises typed errors for values a block cannot show', () => {
    expect(problemOf(() => readVariadicExtra(printSpec, null))).toEqual(['notAnObject', null]);
    expect(problemOf(() => readVariadicExtra(printSpec, [1]))).toEqual(['notAnObject', null]);
    expect(problemOf(() => readVariadicExtra(printSpec, 'x'))).toEqual(['notAnObject', null]);
    expect(problemOf(() => readVariadicExtra(printSpec, new Map()))).toEqual(['notAnObject', null]);
    expect(problemOf(() => readVariadicExtra(printSpec, { itemcount: 1 }))).toEqual([
      'unknownKey',
      'itemcount',
    ]);
    expect(problemOf(() => readVariadicExtra(printSpec, JSON.parse('{"__proto__": 1}')))).toEqual([
      'unknownKey',
      '__proto__',
    ]);
    // An inherited value is never read: the object is not a plain JSON object.
    const inherited: unknown = Object.create({ itemCount: 3 });
    expect(problemOf(() => readVariadicExtra(printSpec, inherited))).toEqual(['notAnObject', null]);
    for (const bad of [-1, 1.5, 65, '2', null, true, Number.NaN, Number.POSITIVE_INFINITY]) {
      expect(
        problemOf(() => readVariadicExtra(printSpec, { itemCount: bad })),
        String(bad),
      ).toEqual(['badCount', 'itemCount']);
    }
    for (const bad of [0, 'true', null]) {
      expect(problemOf(() => readVariadicExtra(ifSpec, { hasElse: bad }))).toEqual([
        'badFlag',
        'hasElse',
      ]);
    }
  });

  it('accepts objects without a prototype', () => {
    const extra = Object.create(null) as Record<string, unknown>;
    extra['itemCount'] = 3;
    expect(readVariadicExtra(printSpec, extra).count).toBe(3);
  });
});

describe('readParamsExtra', () => {
  it('reads rows, and no rows when params is absent', () => {
    expect(readParamsExtra(defineSpec, {})).toEqual([]);
    expect(
      readParamsExtra(defineSpec, { params: [row, { ...row, sym: 'sym_b', mode: 'read_only' }] }),
    ).toEqual([row, { ...row, sym: 'sym_b', mode: 'read_only' }]);
  });

  it('keeps names it can show even when they are not valid identifiers', () => {
    expect(readParamsExtra(defineSpec, { params: [{ ...row, name: '' }] })[0]?.name).toBe('');
    expect(readParamsExtra(defineSpec, { params: [{ ...row, name: 'größe' }] })[0]?.name).toBe(
      'größe',
    );
  });

  it('raises typed errors for rows it cannot show', () => {
    const bad = (params: unknown): [MutatorStateProblem, string | null] =>
      problemOf(() => readParamsExtra(defineSpec, { params }));
    expect(bad('x')).toEqual(['badParams', 'params']);
    expect(bad(Array.from({ length: 65 }, () => row))).toEqual(['badParams', 'params']);
    expect(bad([null])).toEqual(['badParamRow', 'params']);
    expect(bad([{ ...row, extra: 1 }])).toEqual(['badParamRow', 'params']);
    expect(bad([{ ...row, sym: 'bad id' }])).toEqual(['badParamRow', 'params']);
    expect(bad([{ ...row, sym: 's'.repeat(33) }])).toEqual(['badParamRow', 'params']);
    expect(bad([{ ...row, name: 7 }])).toEqual(['badParamRow', 'params']);
    expect(bad([{ ...row, name: 'a‮b' }])).toEqual(['badParamRow', 'params']);
    expect(bad([{ ...row, type: 'auto' }])).toEqual(['badParamRow', 'params']);
    expect(bad([{ ...row, mode: 'moved' }])).toEqual(['badParamRow', 'params']);
    expect(bad([{ sym: row.sym, name: row.name, type: row.type }])).toEqual([
      'badParamRow',
      'params',
    ]);
    expect(problemOf(() => readParamsExtra(defineSpec, { params: [], other: 1 }))).toEqual([
      'unknownKey',
      'other',
    ]);
  });
});

describe('isShowableName', () => {
  it('allows any text within the limits and rejects controls, bidi and broken text', () => {
    expect(isShowableName('score')).toBe(true);
    expect(isShowableName('a'.repeat(64))).toBe(true);
    expect(isShowableName('😀'.repeat(64))).toBe(true);
    expect(isShowableName('a'.repeat(65))).toBe(false);
    for (const bad of [
      'a\u0000',
      'a\nb',
      'a\u007f',
      'a\u0085',
      'a؜',
      'a‏',
      'a⁦',
      '\ud800',
      'x\udc00',
    ]) {
      expect(isShowableName(bad), JSON.stringify(bad)).toBe(false);
    }
  });
});
