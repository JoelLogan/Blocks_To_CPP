/** The parameter rows of func.define on a headless workspace. */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { MutatorButton } from './buttons';
import { buttonKey } from './controller';
import { MutatorStateError } from './errors';
import { configureMutators, randomSymbolId } from './hooks';
import { isB2cMutatorBlock, registerB2cMutators } from './register';
import { defineTestBlocks, flushEvents, inputNames, labelTexts } from './test-fixtures';
import type { B2cMutatorBlock, ParamRow } from './types';

let workspace: Blockly.Workspace;

beforeAll(() => {
  registerB2cMutators();
  defineTestBlocks();
});

beforeEach(() => {
  workspace = new Blockly.Workspace();
});

afterEach(() => {
  configureMutators({ symbols: null, inputShadow: null, newSymbolId: null });
  workspace.dispose();
});

function define(): B2cMutatorBlock {
  const block = workspace.newBlock('func.define');
  if (!isB2cMutatorBlock(block)) {
    throw new Error('func.define has no b2c mutator');
  }
  return block;
}

function rows(block: B2cMutatorBlock): ParamRow[] {
  const extra = block.b2cGetExtra();
  return extra['params'] as ParamRow[];
}

function press(block: Blockly.Block, key: string): void {
  for (const input of block.inputList) {
    for (const field of input.fieldRow) {
      if (field instanceof MutatorButton && buttonKey(field) === key) {
        field.showEditor();
        return;
      }
    }
  }
  throw new Error(`no button ${key}`);
}

const greet: ParamRow[] = [
  { sym: 'sym_name', name: 'name', type: 'std::string', mode: 'read_only' },
  { sym: 'sym_times', name: 'times', type: 'int', mode: 'copy' },
];

describe('func.define rows', () => {
  it('start with no parameters, the words of the group, and ⊕', () => {
    const block = define();
    expect(block.b2cGetExtra()).toEqual({ params: [] });
    expect(inputNames(block)).toEqual([
      'b2c_row0',
      'B2C_BUTTONS',
      'b2c_repeat',
      'b2c_row1',
      'BODY',
    ]);
    expect(labelTexts(block, 'B2C_BUTTONS')).toEqual(['with']);
  });

  it('show each row as type, name and mode, and give the rows back exactly', () => {
    const block = define();
    block.b2cSetExtra({ params: greet });
    expect(rows(block)).toEqual(greet);
    expect(inputNames(block)).toEqual([
      'b2c_row0',
      'B2C_PARAM0',
      'B2C_PARAM1',
      'B2C_BUTTONS',
      'b2c_repeat',
      'b2c_row1',
      'BODY',
    ]);
    expect(labelTexts(block, 'B2C_PARAM0')).toEqual(['with']);
    expect(block.getFieldValue('B2C_PARAM0_TYPE')).toBe('std::string');
    expect(block.getField('B2C_PARAM0_TYPE')?.getText()).toBe('string');
    expect(block.getFieldValue('B2C_PARAM0_NAME')).toBe('name');
    expect(block.getFieldValue('B2C_PARAM0_MODE')).toBe('read_only');
    expect(block.getField('B2C_PARAM0_MODE')?.getText()).toBe('read-only');
    expect(block.saveExtraState?.()).toEqual({ params: greet });
  });

  it('take edits from the row fields', () => {
    const block = define();
    block.b2cSetExtra({ params: greet });
    block.setFieldValue('who', 'B2C_PARAM0_NAME');
    block.setFieldValue('double', 'B2C_PARAM1_TYPE');
    block.setFieldValue('editable', 'B2C_PARAM1_MODE');
    expect(rows(block)).toEqual([
      { ...greet[0], name: 'who' },
      { sym: 'sym_times', name: 'times', type: 'double', mode: 'editable' },
    ]);
  });

  it('limit what can be typed as a name, but keep any showable name from a file', () => {
    const block = define();
    block.b2cSetExtra({ params: [{ ...greet[1], name: 'größe' }] });
    expect(rows(block)[0]?.name).toBe('größe');
    block.setFieldValue('two words', 'B2C_PARAM0_NAME');
    expect(rows(block)[0]?.name).toBe('größe');
    block.setFieldValue('count_2', 'B2C_PARAM0_NAME');
    expect(rows(block)[0]?.name).toBe('count_2');
  });

  it('add rows with fresh names and symbol IDs', () => {
    const block = define();
    press(block, 'param:add');
    press(block, 'param:add');
    const [first, second] = rows(block);
    expect(first).toMatchObject({ name: 'param', type: 'int', mode: 'copy' });
    expect(second).toMatchObject({ name: 'param2', type: 'int', mode: 'copy' });
    expect(first?.sym).toMatch(/^sym_[A-Za-z0-9]{17}$/);
    expect(second?.sym).not.toBe(first?.sym);
    expect(labelTexts(block, 'B2C_BUTTONS')).toEqual([]);
  });

  it('use the editor’s symbol IDs, falling back when they are unusable', () => {
    const block = define();
    let next = 0;
    configureMutators({ newSymbolId: () => `sym_test${String((next += 1))}` });
    press(block, 'param:add');
    expect(rows(block)[0]?.sym).toBe('sym_test1');
    configureMutators({ newSymbolId: () => 'sym_test1' });
    press(block, 'param:add');
    expect(rows(block)[1]?.sym).toMatch(/^sym_[A-Za-z0-9]{17}$/);
    configureMutators({ newSymbolId: () => 'not valid!' });
    press(block, 'param:add');
    expect(rows(block)[2]?.sym).toMatch(/^sym_[A-Za-z0-9]{17}$/);
    configureMutators({
      newSymbolId: () => {
        throw new Error('no generator');
      },
    });
    press(block, 'param:add');
    expect(rows(block)[3]?.sym).toMatch(/^sym_[A-Za-z0-9]{17}$/);
  });

  it('remove the row whose ⊖ is pressed, as one undo step', async () => {
    const block = define();
    block.b2cSetExtra({ params: greet });
    block.setFieldValue('count', 'B2C_PARAM1_NAME');
    await flushEvents();
    workspace.clearUndo();
    press(block, 'param:0:remove');
    expect(rows(block)).toEqual([{ ...greet[1], name: 'count' }]);
    await flushEvents();
    expect(workspace.getUndoStack()).toHaveLength(1);
    workspace.undo(false);
    expect(rows(block)).toEqual([greet[0], { ...greet[1], name: 'count' }]);
  });

  it('stop at sixteen rows', () => {
    const block = define();
    const many = Array.from({ length: 16 }, (_, index) => ({
      sym: `sym_p${String(index)}`,
      name: `p${String(index)}`,
      type: 'int',
      mode: 'copy',
    }));
    block.b2cSetExtra({ params: many });
    expect(() => {
      press(block, 'param:add');
    }).toThrow(/no button/);
    expect(rows(block)).toHaveLength(16);
  });

  it('refuse rows they cannot show, leaving the block as it was', () => {
    const block = define();
    block.b2cSetExtra({ params: greet });
    expect(() => {
      block.b2cSetExtra({ params: [{ ...greet[0], type: 'auto' }] });
    }).toThrow(MutatorStateError);
    expect(rows(block)).toEqual(greet);
  });

  it('do nothing for a stale ⊖', () => {
    const block = define();
    block.b2cSetExtra({ params: greet });
    const stale = block
      .getInput('B2C_PARAM1')
      ?.fieldRow.find((field) => field instanceof MutatorButton);
    block.b2cSetExtra({ params: [greet[0]] });
    stale?.showEditor();
    expect(rows(block)).toEqual([greet[0]]);
  });
});

describe('randomSymbolId', () => {
  it('makes distinct IDs of the project-file form', () => {
    const ids = new Set(Array.from({ length: 2000 }, randomSymbolId));
    expect(ids.size).toBe(2000);
    for (const id of ids) {
      expect(id).toMatch(/^sym_[A-Za-z0-9]{17}$/);
    }
  });
});
