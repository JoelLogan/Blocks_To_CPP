import { BLOCK_DEFS, NAME_ENTRY_PATTERN } from '@blocks2cpp/blockly-ext';
import type * as Blockly from 'blockly/core';
import { describe, expect, it } from 'vitest';

import { defaultDeclName } from '../services/install';
import {
  candidateName,
  DEFAULT_NAME_KINDS,
  defaultNameKind,
  firstFreeName,
  NAME_SCOPES,
  ROW_NAME_KINDS,
  type DefaultNameKind,
} from './names';

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
    for (const kind of Object.keys(NAME_SCOPES) as DefaultNameKind[]) {
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

describe('the default names of declarations (w2.md §2.13)', () => {
  it('give every catalog block with a declaration field a default name', () => {
    for (const def of BLOCK_DEFS) {
      for (const field of def.fields) {
        if (field.kind === 'symbol_decl') {
          expect(defaultNameKind(def.id, field.name), `${def.id}.${field.name}`).toBeDefined();
        }
      }
    }
  });

  it('name only declaration fields the catalog has (w2.md §2.13, §5.3)', () => {
    for (const [id, fields] of Object.entries(DEFAULT_NAME_KINDS)) {
      const def = BLOCK_DEFS.find((block) => block.id === id);
      expect(def, id).toBeDefined();
      for (const name of Object.keys(fields)) {
        const field = def?.fields.find((candidate) => candidate.name === name);
        expect(field?.kind, `${id}.${name}`).toBe('symbol_decl');
      }
    }
  });

  it('name W2’s declarations', () => {
    const first = (block: string, field: string): string | null => {
      const kind = defaultNameKind(block, field);
      return kind === undefined ? null : candidateName(kind, 0);
    };
    expect(first('coll.for_each', 'ITEM')).toBe('item');
    expect(first('coll.for_each_pair', 'KEY')).toBe('key');
    expect(first('coll.for_each_pair', 'VALUE')).toBe('value');
    expect(first('file.for_each_line', 'LINE')).toBe('line');
    expect(first('file.open', 'FILE')).toBe('file');
    expect(first('error.try', 'NAME')).toBe('e');
    expect(first('type.struct', 'NAME')).toBe('MyStruct');
    expect(first('type.enum', 'NAME')).toBe('MyEnum');
    expect(first('var.global', 'NAME')).toBe('value');
    expect(first('program.main', 'ARGV')).toBe('args');
    expect(first('var.declare', 'TYPE')).toBeNull();
    expect(candidateName(ROW_NAME_KINDS['fields'] ?? 'variable', 1)).toBe('field2');
    expect(candidateName(ROW_NAME_KINDS['values'] ?? 'variable', 0)).toBe('Value');
  });

  it('keep the names and scopes of milestone M2', () => {
    expect(DEFAULT_NAME_KINDS['var.declare']).toEqual({ NAME: 'variable' });
    expect(DEFAULT_NAME_KINDS['control.for_range']).toEqual({ VAR: 'loop' });
    expect(DEFAULT_NAME_KINDS['func.define']).toEqual({ NAME: 'function' });
    expect([NAME_SCOPES.variable, NAME_SCOPES.loop, NAME_SCOPES.function]).toEqual([
      'list',
      'body',
      'function',
    ]);
  });

  it('name the new parts of a repeated declaration through the mutators’ hook', () => {
    // The hook reads only the block's type.
    const block = { type: 'error.try' } as Blockly.Block;
    expect(defaultDeclName(block, 'NAME')).toBe('e');
    expect(defaultDeclName(block, 'OTHER')).toBeNull();
  });
});
