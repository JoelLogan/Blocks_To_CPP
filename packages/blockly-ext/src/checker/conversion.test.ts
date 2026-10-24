/**
 * The editor's copy of the analyser's conversion rule (crates/b2c-lang/src/typing.rs) and how the
 * connection checker applies it to catalog type classes.
 */
import { describe, expect, it } from 'vitest';

import type { TypeClass } from '../generated/catalog';
import { conversionAllowed, staticConversion } from './conversion';
import { type Compatibility, type StaticConversion, STATIC_TYPES, type StaticType } from './types';

const CLASSES: readonly TypeClass[] = ['any', 'number', 'integer', 'bool', 'text'];

describe('staticConversion', () => {
  it('agrees with the cases of b2c-lang’s own conversion test', () => {
    const cases: readonly [StaticType, StaticType, StaticConversion][] = [
      ['int', 'int', 'same'],
      ['int', 'double', 'widening'],
      ['char', 'double', 'widening'],
      ['char', 'int', 'widening'],
      ['double', 'int', 'narrowing'],
      ['double', 'char', 'narrowing'],
      ['int', 'char', 'narrowing'],
      ['bool', 'int', 'boolNumber'],
      ['double', 'bool', 'boolNumber'],
      ['string', 'int', 'invalid'],
      ['int', 'string', 'invalid'],
      ['char', 'string', 'invalid'],
      ['bool', 'string', 'invalid'],
      ['void', 'int', 'invalid'],
      ['error', 'string', 'same'],
      ['string', 'error', 'same'],
    ];
    for (const [from, to, expected] of cases) {
      expect(staticConversion(from, to), `${from} -> ${to}`).toBe(expected);
    }
    for (const type of STATIC_TYPES) {
      expect(staticConversion(type, type)).toBe('same');
      expect(staticConversion(type, 'error')).toBe('same');
    }
  });

  it('gives the whole table of b2c_lang::conversion', () => {
    // Rows: from; columns: to, both in the order void, bool, char, int, double, string, error.
    const table: Record<StaticType, readonly StaticConversion[]> = {
      void: ['same', 'invalid', 'invalid', 'invalid', 'invalid', 'invalid', 'same'],
      bool: ['invalid', 'same', 'boolNumber', 'boolNumber', 'boolNumber', 'invalid', 'same'],
      char: ['invalid', 'boolNumber', 'same', 'widening', 'widening', 'invalid', 'same'],
      int: ['invalid', 'boolNumber', 'narrowing', 'same', 'widening', 'invalid', 'same'],
      double: ['invalid', 'boolNumber', 'narrowing', 'narrowing', 'same', 'invalid', 'same'],
      string: ['invalid', 'invalid', 'invalid', 'invalid', 'invalid', 'same', 'same'],
      error: ['same', 'same', 'same', 'same', 'same', 'same', 'same'],
    };
    for (const from of STATIC_TYPES) {
      STATIC_TYPES.forEach((to, column) => {
        expect(staticConversion(from, to), `${from} -> ${to}`).toBe(table[from][column]);
      });
    }
  });
});

describe('conversionAllowed', () => {
  // Columns: any, number, integer, bool, text.
  const expected: readonly [StaticType | null, readonly Compatibility[]][] = [
    [null, ['ok', 'ok', 'ok', 'ok', 'ok']],
    ['error', ['ok', 'ok', 'ok', 'ok', 'ok']],
    ['void', ['invalid', 'invalid', 'invalid', 'invalid', 'invalid']],
    ['bool', ['ok', 'warning', 'warning', 'ok', 'invalid']],
    ['char', ['ok', 'ok', 'ok', 'warning', 'ok']],
    ['int', ['ok', 'ok', 'ok', 'warning', 'invalid']],
    ['double', ['ok', 'ok', 'warning', 'warning', 'invalid']],
    ['string', ['ok', 'invalid', 'invalid', 'invalid', 'ok']],
  ];

  it('refuses only what the analyser reports as an error, for every type and class', () => {
    for (const [from, row] of expected) {
      CLASSES.forEach((to, column) => {
        expect(conversionAllowed(from, to), `${String(from)} into ${to}`).toBe(row[column]);
      });
    }
  });

  it('covers every static type', () => {
    const covered = expected.map(([type]) => type).filter((type) => type !== null);
    expect([...covered].sort()).toEqual([...STATIC_TYPES].sort());
  });

  it('lets types and classes it does not know connect', () => {
    const futureType = 'std::vector<int>' as StaticType;
    for (const to of CLASSES) {
      expect(conversionAllowed(futureType, to)).toBe('ok');
    }
    expect(conversionAllowed('string', 'list' as TypeClass)).toBe('ok');
    expect(conversionAllowed('void', 'list' as TypeClass)).toBe('invalid');
  });
});
