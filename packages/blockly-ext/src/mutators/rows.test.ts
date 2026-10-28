/**
 * The rows mutator on a struct's fields and an enum's values (docs/plans/m3-contracts/w2.md §1.5,
 * §2.1), with the blockly-ext fields registered: add, remove, rename, retype and undo.
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { registerB2cFields } from '../fields/register';
import { B2cTypeField } from '../fields/type';
import { MutatorButton } from './buttons';
import { buttonKey } from './controller';
import { configureMutators } from './hooks';
import { isB2cMutatorBlock, registerB2cMutators } from './register';
import { defineTestBlocks, flushEvents, inputNames, labelTexts } from './test-fixtures';
import type { B2cMutatorBlock } from './types';

let workspace: Blockly.Workspace;

beforeAll(() => {
  registerB2cFields();
  registerB2cMutators();
  defineTestBlocks();
});

beforeEach(() => {
  workspace = new Blockly.Workspace();
  let next = 0;
  configureMutators({ newSymbolId: () => `sym_row${String((next += 1))}` });
});

afterEach(() => {
  configureMutators({ symbols: null, inputShadow: null, newSymbolId: null });
  workspace.dispose();
});

function create(type: string): B2cMutatorBlock {
  const block = workspace.newBlock(type);
  if (!isB2cMutatorBlock(block)) {
    throw new Error(`${type} has no b2c mutator`);
  }
  return block;
}

function buttons(block: Blockly.Block): MutatorButton[] {
  return block.inputList
    .flatMap((input) => input.fieldRow)
    .filter((field): field is MutatorButton => field instanceof MutatorButton);
}

function press(block: Blockly.Block, key: string): void {
  const button = buttons(block).find((field) => buttonKey(field) === key);
  if (button === undefined) {
    throw new Error(`no button ${key}: ${buttons(block).map(buttonKey).join(', ')}`);
  }
  button.showEditor();
}

const student = [
  { sym: 's_name', name: 'name', type: 'std::string' },
  { sym: 's_mark', name: 'mark', type: 'int' },
];

describe('a struct’s fields rows', () => {
  it('start empty, and ⊕ adds int field, field2, … with fresh symbols', () => {
    const block = create('type.struct');
    expect(block.b2cGetExtra()).toEqual({ fields: [] });
    expect(buttons(block).map(buttonKey)).toEqual(['field:add']);
    press(block, 'field:add');
    press(block, 'field:add');
    expect(block.b2cGetExtra()).toEqual({
      fields: [
        { sym: 'sym_row1', name: 'field', type: 'int' },
        { sym: 'sym_row2', name: 'field2', type: 'int' },
      ],
    });
    expect(inputNames(block).filter((name) => name.startsWith('B2C_FIELD'))).toEqual([
      'B2C_FIELD0',
      'B2C_FIELD1',
    ]);
    // A field has a type and a name, and no pass mode.
    expect(block.getField('B2C_FIELD0_TYPE')).toBeInstanceOf(B2cTypeField);
    expect(block.getField('B2C_FIELD0_MODE')).toBeNull();
  });

  it('show each row as type and name, and give the rows back exactly', () => {
    const block = create('type.struct');
    block.b2cSetExtra({ fields: student });
    expect(block.b2cGetExtra()).toEqual({ fields: student });
    expect(block.getField('B2C_FIELD0_TYPE')?.getText()).toBe('string');
    // Each row is its default value's input (holding the row's fields, then `=`, W2's package C2)
    // and ⊖.
    expect(inputNames(block).filter((name) => /^(DEFAULT|B2C_FIELD)\d/.test(name))).toEqual([
      'DEFAULT0',
      'B2C_FIELD0',
      'DEFAULT1',
      'B2C_FIELD1',
    ]);
    expect(labelTexts(block, 'DEFAULT0')).toEqual(['has', '=']);
    expect(labelTexts(block, 'DEFAULT1')).toEqual(['=']);
    expect(buttons(block).map(buttonKey)).toEqual([
      'field:0:remove',
      'field:1:remove',
      'field:add',
    ]);
    expect(buttons(block).map((button) => button.label)).toEqual([
      'Remove this field',
      'Remove this field',
      'Add a field',
    ]);
  });

  it('take renames and new types from the row fields, a user type and a list of one too', () => {
    const block = create('type.struct');
    block.b2cSetExtra({ fields: student });
    block.setFieldValue('fullName', 'B2C_FIELD0_NAME');
    block.setFieldValue('double', 'B2C_FIELD1_TYPE');
    expect(block.b2cGetExtra()).toEqual({
      fields: [
        { sym: 's_name', name: 'fullName', type: 'std::string' },
        { sym: 's_mark', name: 'mark', type: 'double' },
      ],
    });
    block.setFieldValue('{"ref":"s_Team"}', 'B2C_FIELD1_TYPE');
    block.setFieldValue('{"std::vector":{"ref":"s_Team"}}', 'B2C_FIELD0_TYPE');
    expect(block.b2cGetExtra()).toEqual({
      fields: [
        { sym: 's_name', name: 'fullName', type: { 'std::vector': { ref: 's_Team' } } },
        { sym: 's_mark', name: 'mark', type: { ref: 's_Team' } },
      ],
    });
  });

  it('remove the row whose ⊖ is pressed as one undo step, and undo brings it back', async () => {
    const block = create('type.struct');
    block.b2cSetExtra({ fields: student });
    await flushEvents();
    workspace.clearUndo();
    press(block, 'field:0:remove');
    expect(block.b2cGetExtra()).toEqual({ fields: [student[1]] });
    await flushEvents();
    expect(workspace.getUndoStack()).toHaveLength(1);
    workspace.undo(false);
    expect(block.b2cGetExtra()).toEqual({ fields: student });
    workspace.undo(true);
    expect(block.b2cGetExtra()).toEqual({ fields: [student[1]] });
  });

  it('undo a retype on its own', async () => {
    const block = create('type.struct');
    block.b2cSetExtra({ fields: student });
    await flushEvents();
    workspace.clearUndo();
    block.setFieldValue('char', 'B2C_FIELD1_TYPE');
    await flushEvents();
    workspace.undo(false);
    expect(block.b2cGetExtra()).toEqual({ fields: student });
  });

  it('refuse a row with a pass mode, leaving the block as it was', () => {
    const block = create('type.struct');
    block.b2cSetExtra({ fields: student });
    expect(() => {
      block.b2cSetExtra({ fields: [{ ...student[0], mode: 'copy' }] });
    }).toThrow(/unknown key "mode"/);
    expect(block.b2cGetExtra()).toEqual({ fields: student });
  });
});

describe('an enum’s values rows', () => {
  it('start empty, and ⊕ adds Value, Value2, … with names only', () => {
    const block = create('type.enum');
    expect(block.b2cGetExtra()).toEqual({ values: [] });
    press(block, 'value:add');
    press(block, 'value:add');
    expect(block.b2cGetExtra()).toEqual({
      values: [
        { sym: 'sym_row1', name: 'Value' },
        { sym: 'sym_row2', name: 'Value2' },
      ],
    });
    expect(block.getField('B2C_VALUE0_TYPE')).toBeNull();
    expect(buttons(block).map((button) => button.label)).toEqual([
      'Remove this value',
      'Remove this value',
      'Add a value',
    ]);
    expect(block.getField('B2C_VALUE0_NAME')).not.toBeNull();
  });

  it('rename a value, and remove one as an undoable step', async () => {
    const block = create('type.enum');
    const values = [
      { sym: 'v_deposit', name: 'Deposit' },
      { sym: 'v_withdraw', name: 'Withdraw' },
    ];
    block.b2cSetExtra({ values });
    block.setFieldValue('PayIn', 'B2C_VALUE0_NAME');
    expect(block.b2cGetExtra()).toEqual({ values: [{ ...values[0], name: 'PayIn' }, values[1]] });
    await flushEvents();
    workspace.clearUndo();
    press(block, 'value:1:remove');
    expect(block.b2cGetExtra()).toEqual({ values: [{ ...values[0], name: 'PayIn' }] });
    await flushEvents();
    workspace.undo(false);
    expect(block.b2cGetExtra()).toEqual({ values: [{ ...values[0], name: 'PayIn' }, values[1]] });
  });

  it('skip a name a row already has when adding', () => {
    const block = create('type.enum');
    block.b2cSetExtra({ values: [{ sym: 'v_a', name: 'Value' }] });
    press(block, 'value:add');
    expect(block.b2cGetExtra()).toEqual({
      values: [
        { sym: 'v_a', name: 'Value' },
        { sym: 'sym_row1', name: 'Value2' },
      ],
    });
  });
});
