/**
 * The editor's copy of the analyser's conversion rule (crates/b2c-lang/src/typing.rs) and how the
 * connection checker applies it to catalog type classes.
 */
import { describe, expect, it } from 'vitest';

import type { TypeClass } from '../generated/catalog';
import { parseType } from '../types/static-type';
import { conversionAllowed, staticConversion } from './conversion';
import type { Compatibility, StaticConversion, StaticType } from './types';

/** M2's seven types by the names its tables use (`string` is `std::string`). */
type M2Name = 'void' | 'bool' | 'char' | 'int' | 'double' | 'string' | 'error';

const STATIC_TYPES: readonly M2Name[] = [
  'void',
  'bool',
  'char',
  'int',
  'double',
  'string',
  'error',
];

/** The type object of an M2 name, as the analysis now sends it (types.md §3.1). */
function t(name: M2Name): StaticType {
  const type = parseType(name === 'string' ? 'std::string' : name);
  if (type === null) {
    throw new Error(name);
  }
  return type;
}

const CLASSES: readonly TypeClass[] = ['any', 'number', 'integer', 'bool', 'text'];

describe('staticConversion', () => {
  it('agrees with the cases of b2c-lang’s own conversion test', () => {
    const cases: readonly [M2Name, M2Name, StaticConversion][] = [
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
      expect(staticConversion(t(from), t(to)), `${from} -> ${to}`).toBe(expected);
    }
    for (const type of STATIC_TYPES) {
      expect(staticConversion(t(type), t(type))).toBe('same');
      expect(staticConversion(t(type), t('error'))).toBe('same');
    }
  });

  it('gives the whole table of b2c_lang::conversion', () => {
    // Rows: from; columns: to, both in the order void, bool, char, int, double, string, error.
    const table: Record<M2Name, readonly StaticConversion[]> = {
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
        expect(staticConversion(t(from), t(to)), `${from} -> ${to}`).toBe(table[from][column]);
      });
    }
  });
});

describe('conversionAllowed', () => {
  // Columns: any, number, integer, bool, text.
  const expected: readonly [M2Name | null, readonly Compatibility[]][] = [
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
        expect(
          conversionAllowed(from === null ? null : t(from), to),
          `${String(from)} into ${to}`,
        ).toBe(row[column]);
      });
    }
  });

  it('covers every static type', () => {
    const covered = expected.map(([type]) => type).filter((type) => type !== null);
    expect([...covered].sort()).toEqual([...STATIC_TYPES].sort());
  });

  it('lets values that are no type and classes it does not know connect', () => {
    const notAType = 'std::vector<int>' as unknown as StaticType;
    for (const to of CLASSES) {
      expect(conversionAllowed(notAType, to)).toBe('ok');
    }
    expect(conversionAllowed(t('string'), 'list' as TypeClass)).toBe('ok');
    expect(conversionAllowed(t('void'), 'list' as TypeClass)).toBe('invalid');
  });

  it('refuses a library type where M2 needs a number, a condition or text, as the analyser does', () => {
    const list = parseType({ 'std::vector': 'int' });
    expect(conversionAllowed(list, 'any')).toBe('ok');
    for (const to of ['number', 'integer', 'bool', 'text'] as const) {
      expect(conversionAllowed(list, to), to).toBe('invalid');
    }
    expect(conversionAllowed(list, 'collection')).toBe('ok');
    expect(conversionAllowed(t('int'), 'collection')).toBe('invalid');
    // A user type the checker knows nothing about connects, except where only numbers convert.
    const user = parseType({ ref: 'sym_Point' });
    expect(conversionAllowed(user, 'text')).toBe('ok');
    expect(conversionAllowed(user, 'struct')).toBe('ok');
    expect(conversionAllowed(user, 'number')).toBe('invalid');
  });
});
