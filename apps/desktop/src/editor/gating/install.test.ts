/**
 * The option gate on the block menus (./install.ts): *change*'s bitwise forms are listed only with
 * *Show advanced blocks* (docs/plans/m3-contracts/w2.md §1.9).
 */
import { B2cDropdownField } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { registerEditorBlocks } from '../services/install';
import type { EditorMode } from './gate';
import { installOptionGate } from './install';

let workspace: Blockly.Workspace | null = null;
let uninstall: (() => void) | null = null;

beforeAll(() => {
  registerEditorBlocks();
});

afterEach(() => {
  uninstall?.();
  uninstall = null;
  workspace?.dispose();
  workspace = null;
});

/** *change*'s operator values as its menu lists them in `mode`. */
function operators(showAdvanced: boolean): string[] {
  const mode: EditorMode = { labels: 'friendly', showAdvanced, standard: 'c++20', toolchain: null };
  uninstall = installOptionGate(() => mode);
  workspace = new Blockly.Workspace();
  const field = workspace.newBlock('var.change').getField('OP');
  if (!(field instanceof B2cDropdownField)) {
    throw new Error('OP is not a Blocks2Cpp dropdown');
  }
  return field.getOptions(false).map((option) => option[1]);
}

describe('the option gate', () => {
  it('hides change’s bitwise forms until Show advanced blocks is on', () => {
    expect(operators(false)).toEqual(['add', 'sub', 'mul', 'div', 'mod', 'append']);
  });

  it('lists them with Show advanced blocks', () => {
    expect(operators(true)).toEqual([
      'add',
      'sub',
      'mul',
      'div',
      'mod',
      'append',
      'and',
      'or',
      'xor',
      'shl',
      'shr',
    ]);
  });

  it('is removed again, and then every option is listed', () => {
    operators(false);
    uninstall?.();
    uninstall = null;
    const field = workspace?.newBlock('var.change').getField('OP');
    expect(field instanceof B2cDropdownField ? field.getOptions(false) : []).toHaveLength(11);
  });
});
