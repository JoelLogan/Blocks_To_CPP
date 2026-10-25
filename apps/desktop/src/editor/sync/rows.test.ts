/**
 * The editor's mirrors of W2's catalog schema (docs/plans/m3-contracts/w2.md §2.1): the row extras
 * `fields` and `values` declare symbols like `params` (declarations, renaming, duplicates), a
 * repeated field's parts are fields of the project file, and a repeat counted by a row extra has
 * one part per row.
 */
import type { BdmBlock } from '@blocks2cpp/b2c-core-wasm';
import {
  B2cSymbolDeclField,
  type BlockDefJson,
  createCatalogField,
  type ExtraDefJson,
  type FieldDefJson,
} from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it } from 'vitest';

import { valueInputs } from '../toolbox/presets';
import { nodeDecls, renameInTrees, ROW_EXTRA_KEYS, treeDecls } from './bdmTree';
import { readFields } from './blockState';
import { catalogFieldsOf, extraDefault, fieldDef } from './catalog';
import { declaredSyms, renameInBlock } from './duplicates';
import { disposeWorkspaces, headlessWorkspace } from './testing';

afterEach(() => {
  disposeWorkspaces();
});

/** A W2-shaped *try* (w2.md §1.7): `ERROR` and `NAME` repeated by `catchCount`. */
const TRY_DEF = {
  id: 'test.try',
  version: 1,
  category: 'errors',
  shape: 'statement',
  label: { friendly: 'try %TRY catch %ERROR as %NAME %CATCH catch …', cpp: 'try' },
  labelParts: [],
  statementsNotInLabel: [],
  help: '',
  output: null,
  fields: [
    {
      name: 'ERROR',
      kind: 'type',
      options: [],
      types: ['std::exception', 'std::runtime_error'],
      default: 'std::exception',
      type: null,
      repeat: { count: 'catchCount', plus: 0 },
    },
    {
      name: 'NAME',
      kind: 'symbol_decl',
      options: [],
      types: [],
      default: null,
      type: { kind: 'field', field: 'ERROR' },
      repeat: { count: 'catchCount', plus: 0 },
    },
  ],
  inputs: [
    {
      name: 'DEFAULT',
      check: 'any',
      optional: true,
      repeat: { count: 'fields', plus: 0 },
      default: [{ num: '0' }],
      type: null,
      assign: false,
    },
  ],
  statements: [],
  extra: [
    { name: 'catchCount', kind: 'count', min: 1, max: 8, default: 1, types: [], typeClass: null },
    {
      name: 'fields',
      kind: 'fields',
      min: null,
      max: 64,
      default: null,
      types: [],
      typeClass: null,
    },
  ],
  typeVars: {},
  order: [],
  placeVar: null,
  members: {},
  headers: [],
  optionHeaders: {},
} satisfies BlockDefJson;

describe('row extras in project trees', () => {
  it('declare a symbol for each row of params, fields and values', () => {
    expect(ROW_EXTRA_KEYS).toEqual(['params', 'fields', 'values']);
    const struct = {
      id: 'b1',
      type: 'type.struct',
      v: 1,
      fields: { NAME: { sym: 's_student', name: 'Student' } },
      extra: {
        fields: [
          { sym: 's_name', name: 'name', type: 'std::string' },
          { sym: 's_mark', name: 'mark', type: 'int' },
        ],
      },
    } as unknown as BdmBlock;
    const enumeration = {
      id: 'b2',
      type: 'type.enum',
      v: 1,
      fields: { NAME: { sym: 's_action', name: 'Action' } },
      extra: { values: [{ sym: 's_deposit', name: 'Deposit' }] },
    } as unknown as BdmBlock;
    expect(nodeDecls(struct).map((decl) => decl.sym)).toEqual(['s_student', 's_name', 's_mark']);
    expect(treeDecls([struct, enumeration]).map((decl) => decl.name)).toEqual([
      'Student',
      'name',
      'mark',
      'Action',
      'Deposit',
    ]);
  });

  it('rename the rows’ symbols and the user types their types name', () => {
    const struct = {
      id: 'b1',
      type: 'type.struct',
      v: 1,
      fields: { NAME: { sym: 's_student', name: 'Student' } },
      extra: {
        fields: [
          { sym: 's_name', name: 'name', type: 'std::string' },
          { sym: 's_next', name: 'next', type: { 'std::vector': { ref: 's_student' } } },
        ],
      },
    } as unknown as BdmBlock;
    const enumeration = {
      id: 'b2',
      type: 'type.enum',
      v: 1,
      extra: { values: [{ sym: 's_deposit', name: 'Deposit' }] },
    } as unknown as BdmBlock;
    renameInTrees(
      [struct, enumeration],
      new Map(),
      new Map([
        ['s_student', 's_student2'],
        ['s_name', 's_name2'],
        ['s_deposit', 's_deposit2'],
      ]),
    );
    expect(struct.extra?.['fields']).toEqual([
      { sym: 's_name2', name: 'name', type: 'std::string' },
      { sym: 's_next', name: 'next', type: { 'std::vector': { ref: 's_student2' } } },
    ]);
    expect(enumeration.extra?.['values']).toEqual([{ sym: 's_deposit2', name: 'Deposit' }]);
  });
});

describe('the sync’s catalog lookups', () => {
  it('find a repeated field by its parts and give row extras no rows by default', () => {
    expect(fieldDef(TRY_DEF, 'ERROR1')?.name).toBe('ERROR');
    expect(fieldDef(TRY_DEF, 'NAME0')?.name).toBe('NAME');
    expect(fieldDef(TRY_DEF, 'ERROR')).toBeNull();
    expect(fieldDef(TRY_DEF, 'ERROR01')).toBeNull();
    const [count, rows]: readonly ExtraDefJson[] = TRY_DEF.extra;
    expect(count === undefined ? null : extraDefault(count)).toBe(1);
    expect(rows === undefined ? null : extraDefault(rows)).toEqual([]);
  });

  it('read every part of a repeated field as a field of the project file', () => {
    const workspace = headlessWorkspace();
    Blockly.Blocks['test.try'] = {
      init(this: Blockly.Block): void {
        const [error, name]: readonly FieldDefJson[] = TRY_DEF.fields;
        if (error === undefined || name === undefined) {
          return;
        }
        for (const copy of [0, 1]) {
          const input = this.appendStatementInput(`CATCH${String(copy)}`);
          input.appendField(createCatalogField(error), `ERROR${String(copy)}`);
          const decl = createCatalogField(name);
          if (decl instanceof B2cSymbolDeclField) {
            decl.setDecl({ sym: `s_e${String(copy)}`, name: 'e' });
          }
          input.appendField(decl, `NAME${String(copy)}`);
        }
      },
    };
    try {
      const block = workspace.newBlock('test.try');
      block.setFieldValue('std::runtime_error', 'ERROR1');
      expect(catalogFieldsOf(block, TRY_DEF).map((found) => found.name)).toEqual([
        'ERROR0',
        'NAME0',
        'ERROR1',
        'NAME1',
      ]);
      expect(readFields(block, TRY_DEF, null)).toEqual({
        ERROR0: 'std::exception',
        NAME0: { sym: 's_e0', name: 'e' },
        ERROR1: 'std::runtime_error',
        NAME1: { sym: 's_e1', name: 'e' },
      });
    } finally {
      Reflect.deleteProperty(Blockly.Blocks, 'test.try');
    }
  });

  it('give a repeat counted by a row extra one part per row', () => {
    const rows = [
      { sym: 's_a', name: 'a', type: 'int' },
      { sym: 's_b', name: 'b', type: 'int' },
    ];
    expect(valueInputs(TRY_DEF, { fields: rows }).map((input) => input.name)).toEqual([
      'DEFAULT0',
      'DEFAULT1',
    ]);
    expect(valueInputs(TRY_DEF, { fields: [] })).toEqual([]);
  });
});

describe('row extras on the canvas (duplicates)', () => {
  it('declare and rename the symbols of fields and values rows', () => {
    const workspace = headlessWorkspace();
    let extra: Record<string, unknown> = {
      fields: [{ sym: 's_name', name: 'name', type: { ref: 's_student' } }],
      values: [{ sym: 's_deposit', name: 'Deposit' }],
    };
    Blockly.Blocks['test.rows'] = {
      init(this: Blockly.Block): void {
        this.appendDummyInput();
      },
      b2cGetExtra: (): Record<string, unknown> => extra,
      b2cSetExtra: (next: Record<string, unknown>): void => {
        extra = next;
      },
    };
    try {
      const block = workspace.newBlock('test.rows');
      expect(declaredSyms(block)).toEqual(['s_name', 's_deposit']);
      renameInBlock(
        block,
        new Map([
          ['s_name', 's_name2'],
          ['s_student', 's_student2'],
          ['s_deposit', 's_deposit2'],
        ]),
        () => new Map(),
      );
      expect(extra).toEqual({
        fields: [{ sym: 's_name2', name: 'name', type: { ref: 's_student2' } }],
        values: [{ sym: 's_deposit2', name: 'Deposit' }],
      });
    } finally {
      Reflect.deleteProperty(Blockly.Blocks, 'test.rows');
    }
  });
});
