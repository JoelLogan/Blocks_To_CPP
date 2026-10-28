/**
 * Which options a dropdown's menu lists (./option-filter.ts): the editor's mode (the app's gating:
 * *Show advanced blocks* hides the bitwise forms of *change*) and the type of the variable chosen on
 * the block (docs/plans/m3-contracts/w2.md §1.9).
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { registerB2cBlocks } from '../blocks/register';
import { installIdGenerator } from '../ids';
import { registerB2cMutators } from '../mutators/register';
import { resetEditorServices, setEditorServices, type SymbolInfo } from '../services';
import { setOptionFilter } from './option-filter';
import { B2cSymbolRefField } from './symbol-ref';

let workspace: Blockly.WorkspaceSvg;

function variable(id: string, type: string): SymbolInfo {
  return { id, name: id, kind: 'variable', type, module: 'mod_main', declBlock: 'blk' };
}

const SYMBOLS = new Map(
  [variable('v_text', 'std::string'), variable('v_int', 'int'), variable('v_real', 'double')].map(
    (symbol) => [symbol.id, symbol],
  ),
);

beforeAll(() => {
  installIdGenerator();
  registerB2cMutators();
  registerB2cBlocks();
});

beforeEach(() => {
  const host = document.createElement('div');
  document.body.append(host);
  workspace = Blockly.inject(host, { renderer: 'zelos', sounds: false });
  setEditorServices({
    symbols: {
      symbolsAt: () => [...SYMBOLS.values()],
      nameOf: (id) => SYMBOLS.get(id)?.name ?? null,
    },
    types: { outputTypeOf: () => null, symbol: (id) => SYMBOLS.get(id) ?? null },
  });
});

afterEach(() => {
  setOptionFilter(null);
  resetEditorServices();
  workspace.dispose();
  document.body.replaceChildren();
});

/** The values *change*'s operator menu lists with `variable` chosen and `op` the value. */
function operators(variableId: string | null, op = 'add'): string[] {
  const change = workspace.newBlock('var.change');
  const reference = change.getField('VAR');
  if (variableId !== null && reference instanceof B2cSymbolRefField) {
    reference.setValue({ ref: variableId });
  }
  change.setFieldValue(op, 'OP');
  const menu = change.getField('OP');
  if (!(menu instanceof Blockly.FieldDropdown)) {
    throw new Error('OP is not a dropdown');
  }
  return menu.getOptions(false).map((option) => option[1]);
}

describe('change’s operator menu', () => {
  it('lists what the chosen variable’s type takes', () => {
    expect(operators('v_int')).toEqual([
      'add',
      'sub',
      'mul',
      'div',
      'mod',
      'and',
      'or',
      'xor',
      'shl',
      'shr',
    ]);
    expect(operators('v_real')).toEqual(['add', 'sub', 'mul', 'div']);
    // A text variable: add to the end, and the current choice, which is never taken away.
    expect(operators('v_text')).toEqual(['add', 'append']);
    expect(operators('v_text', 'append')).toEqual(['append']);
  });

  it('lists everything while no variable, or no type, is known', () => {
    expect(operators(null)).toHaveLength(11);
    expect(operators('v_missing')).toHaveLength(11);
  });

  it('leaves out what the editor’s mode does not offer', () => {
    setOptionFilter(
      (block, field, value) =>
        !(
          block.type === 'var.change' &&
          field === 'OP' &&
          ['and', 'or', 'xor', 'shl', 'shr'].includes(value)
        ),
    );
    expect(operators('v_int')).toEqual(['add', 'sub', 'mul', 'div', 'mod']);
    // A block that already holds a hidden option still shows it.
    expect(operators('v_int', 'xor')).toEqual(['add', 'sub', 'mul', 'div', 'mod', 'xor']);
  });
});
