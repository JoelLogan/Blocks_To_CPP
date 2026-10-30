/**
 * Blocks in the C++ label mode (docs/plans/m3-contracts/w4-editor-tools.md §2.1): the layout from
 * the C++ template, the menus' C++ option texts, type names in C++, and the same state as in the
 * Friendly mode.
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import {
  BLOCK_DEFS,
  B2cTypeField,
  blockLabelMode,
  catalogBlock,
  displayTypeName,
  hasCppLayout,
  setBlockLabelMode,
} from '../src';
import { headlessWorkspace, setUpBlocks } from './helpers';

beforeEach(() => {
  setUpBlocks();
});

afterEach(() => {
  setBlockLabelMode('friendly');
});

/** The block's visible texts and the names of its inputs and fields, in order. */
function shown(block: Blockly.Block): string[] {
  return block.inputList.flatMap((input) => [
    ...input.fieldRow
      .filter((field) => field.isVisible())
      .map((field) =>
        field instanceof Blockly.FieldLabel ? `"${field.getText()}"` : (field.name ?? ''),
      ),
    ...(input.connection === null ? [] : [`<${input.name}>`]),
  ]);
}

describe('the label mode', () => {
  it('is Friendly until set, and takes only the two modes', () => {
    expect(blockLabelMode()).toBe('friendly');
    setBlockLabelMode('cpp');
    expect(blockLabelMode()).toBe('cpp');
    setBlockLabelMode('other' as 'cpp');
    expect(blockLabelMode()).toBe('friendly');
  });

  it('lays a block out from its C++ template in the C++ mode', () => {
    const workspace = headlessWorkspace();
    expect(shown(workspace.newBlock('coll.add'))).toEqual(['"add"', '<VALUE>', '"to"', 'LIST']);
    setBlockLabelMode('cpp');
    expect(shown(workspace.newBlock('coll.add'))).toEqual([
      'LIST',
      '".push_back("',
      '<VALUE>',
      '");"',
    ]);
  });

  it('keeps the Friendly layout of a block with a mutator, a part left out or named twice', () => {
    for (const id of ['control.if', 'math.fn', 'program.exit', 'io.print']) {
      const def = catalogBlock(id);
      expect(def !== undefined && hasCppLayout(def), id).toBe(false);
    }
    const set = catalogBlock('var.set');
    expect(set !== undefined && hasCppLayout(set)).toBe(true);
  });

  it('shows the C++ text of menu options in the C++ mode, storing the same values', () => {
    const workspace = headlessWorkspace();
    setBlockLabelMode('cpp');
    const block = workspace.newBlock('math.compare');
    const field = block.getField('OP') as Blockly.FieldDropdown;
    expect(field.getOptions(false).map(([label, value]) => [label, value])).toContainEqual([
      '<=',
      'le',
    ]);
    const fn = workspace.newBlock('math.fn').getField('FN') as Blockly.FieldDropdown;
    expect(fn.getText()).toMatch(/^std::/u);
  });

  it('names types in C++ in the C++ mode', () => {
    expect(displayTypeName('std::string')).toBe('string');
    expect(displayTypeName('{"std::vector":"int"}')).toBe('list of int');
    setBlockLabelMode('cpp');
    expect(displayTypeName('std::string')).toBe('std::string');
    expect(displayTypeName('{"std::vector":"int"}')).toBe('std::vector<int>');
    const workspace = headlessWorkspace();
    const field = workspace.newBlock('var.declare').getField('TYPE');
    expect(field).toBeInstanceOf(B2cTypeField);
    field?.setValue('std::string');
    expect(field?.getText()).toBe('std::string');
  });

  it('gives every catalog block the same fields, inputs and state in both modes', () => {
    const workspace = headlessWorkspace();
    for (const def of BLOCK_DEFS) {
      setBlockLabelMode('friendly');
      const friendly = workspace.newBlock(def.id, `f${String(BLOCK_DEFS.indexOf(def))}`);
      setBlockLabelMode('cpp');
      const cpp = workspace.newBlock(def.id, `c${String(BLOCK_DEFS.indexOf(def))}`);
      const names = (block: Blockly.Block) =>
        block.inputList
          .flatMap((input) => [input.name, ...input.fieldRow.map((field) => field.name ?? '')])
          .filter((name) => name !== '' && !name.startsWith('b2c_row'))
          .sort();
      expect(names(cpp), def.id).toEqual(names(friendly));
      const state = (block: Blockly.Block) => {
        const saved = Blockly.serialization.blocks.save(block, { saveIds: false });
        return { ...saved, id: undefined };
      };
      expect(state(cpp), def.id).toEqual(state(friendly));
    }
  });
});
