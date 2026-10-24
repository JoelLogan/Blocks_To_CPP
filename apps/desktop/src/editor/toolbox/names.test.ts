import { NAME_ENTRY_PATTERN } from '@blocks2cpp/blockly-ext';
import { describe, expect, it } from 'vitest';

import { candidateName, firstFreeName, type DefaultNameKind } from './names';

describe('default names (03 §3.6)', () => {
  it('counts value, value2, value3, … for a new variable', () => {
    expect([0, 1, 2, 3].map((index) => candidateName('variable', index))).toEqual([
      'value',
      'value2',
      'value3',
      'value4',
    ]);
  });

  it('counts i, j, k, then i2, j2, k2, … for a counted loop', () => {
    expect([0, 1, 2, 3, 4, 5, 6].map((index) => candidateName('loop', index))).toEqual([
      'i',
      'j',
      'k',
      'i2',
      'j2',
      'k2',
      'i3',
    ]);
  });

  it('counts myFunction, myFunction2, … for a new function', () => {
    expect([0, 1, 2].map((index) => candidateName('function', index))).toEqual([
      'myFunction',
      'myFunction2',
      'myFunction3',
    ]);
  });

  it('takes the first name not in use', () => {
    expect(firstFreeName('variable', new Set())).toBe('value');
    expect(firstFreeName('variable', new Set(['value']))).toBe('value2');
    expect(firstFreeName('variable', new Set(['value2']))).toBe('value');
    expect(firstFreeName('loop', new Set(['i', 'j']))).toBe('k');
    expect(firstFreeName('loop', new Set(['i', 'j', 'k']))).toBe('i2');
    expect(firstFreeName('function', new Set(['myFunction', 'other']))).toBe('myFunction2');
  });

  it('compares names exactly (C++ names are case-sensitive)', () => {
    expect(firstFreeName('variable', new Set(['Value', 'VALUE']))).toBe('value');
  });

  it('always finds a free, valid name, however many are taken', () => {
    for (const kind of ['variable', 'loop', 'function'] as DefaultNameKind[]) {
      const taken = new Set<string>();
      for (let round = 0; round < 300; round += 1) {
        const name = firstFreeName(kind, taken);
        expect(taken.has(name)).toBe(false);
        expect(name).toMatch(NAME_ENTRY_PATTERN);
        expect(name).toMatch(/^[A-Za-z]/);
        taken.add(name);
      }
    }
  });
});
