/**
 * Checkboxes set from the block's menu (./marks.ts): *Fast unchecked access* on *item of* and *set
 * item* (docs/plans/m3-contracts/w4-editor-tools.md §7).
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { BLOCK_DEFS } from '../generated/catalog';
import { installIdGenerator } from '../ids';
import { registerB2cMutators } from '../mutators/register';
import { flushEvents } from '../mutators/test-fixtures';
import { hasCppLayout } from './labels';
import { setBlockLabelMode } from './labelMode';
import { MARK_CLASS, menuFieldDefs, menuToggles, toggleMenuField } from './marks';
import { registerB2cBlocks } from './register';

let workspace: Blockly.WorkspaceSvg;

beforeAll(() => {
  installIdGenerator();
  registerB2cMutators();
  registerB2cBlocks();
});

beforeEach(() => {
  const host = document.createElement('div');
  document.body.append(host);
  workspace = Blockly.inject(host, { renderer: 'zelos', sounds: false });
});

afterEach(() => {
  setBlockLabelMode('friendly');
  workspace.dispose();
  document.body.replaceChildren();
});

/** The mark label of `block` (an unnamed label with the mark's class). */
function markOf(block: Blockly.Block): Blockly.Field | undefined {
  return block.inputList
    .flatMap((input) => input.fieldRow)
    .find((field) => field instanceof Blockly.FieldLabel && field.getText() === 'unchecked');
}

describe('menu fields', () => {
  it('are the unchecked access of item of and set item, each with its words and mark', () => {
    const withMenu = BLOCK_DEFS.flatMap((def) =>
      menuFieldDefs(def).map((field) => `${def.id}.${field.name}`),
    );
    expect(withMenu).toEqual(['coll.item.CHECKED', 'coll.set_item.CHECKED']);
    for (const def of BLOCK_DEFS) {
      for (const field of menuFieldDefs(def)) {
        expect(field.mark, def.id).toBe('unchecked');
        expect(field.menu, def.id).toMatch(/unchecked/i);
      }
    }
    // A menu field does not keep a block from its C++ layout (`%LIST.at(%INDEX)`).
    const item = BLOCK_DEFS.find((def) => def.id === 'coll.item');
    expect(item !== undefined && hasCppLayout(item)).toBe(true);
  });

  it('hide the checkbox and show the mark only away from the default', () => {
    for (const mode of ['friendly', 'cpp'] as const) {
      setBlockLabelMode(mode);
      const item = workspace.newBlock('coll.item');
      item.initSvg();
      item.render();
      expect(item.getField('CHECKED')?.isVisible(), mode).toBe(false);
      const mark = markOf(item);
      expect(mark?.isVisible(), mode).toBe(false);
      expect(mark?.getSvgRoot()?.querySelector(`.${MARK_CLASS}`) ?? null, mode).not.toBeNull();
      expect(menuToggles(item)).toEqual([
        {
          field: 'CHECKED',
          text: expect.stringMatching(/unchecked/i) as string,
          on: false,
          next: 'false',
        },
      ]);
      toggleMenuField(item, 'CHECKED');
      expect(item.getFieldValue('CHECKED'), mode).toBe('FALSE');
      expect(mark?.isVisible(), mode).toBe(true);
      expect(menuToggles(item)[0]).toMatchObject({ on: true, next: 'true' });
      expect(Blockly.serialization.blocks.save(item)?.fields?.['CHECKED']).toBe(false);
      item.dispose();
    }
  });

  it('load unchecked with the mark shown, and flip back in one undo step', async () => {
    const set = Blockly.serialization.blocks.append(
      { type: 'coll.set_item', fields: { CHECKED: false } },
      workspace,
    );
    expect(markOf(set)?.isVisible()).toBe(true);
    await flushEvents();
    workspace.clearUndo();
    toggleMenuField(set, 'CHECKED');
    expect(markOf(set)?.isVisible()).toBe(false);
    await flushEvents();
    expect(workspace.getUndoStack()).toHaveLength(1);
    workspace.undo(false);
    expect(set.getFieldValue('CHECKED')).toBe('FALSE');
    expect(markOf(set)?.isVisible()).toBe(true);
  });
});
