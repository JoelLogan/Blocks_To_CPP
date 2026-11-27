/**
 * The unchecked `[]` index on *item of* and *set item* (./marks.ts,
 * docs/plans/m3-contracts/w4-editor-tools.md §7): the field `CHECKED`, which the app's block menu
 * toggles (*Use unchecked `[]`*, its words the menu's own), hidden on the block and shown as the
 * mark *unchecked* in the C++ label mode only (§7.1).
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { BLOCK_DEFS } from '../generated/catalog';
import { installIdGenerator } from '../ids';
import { registerB2cMutators } from '../mutators/register';
import { flushEvents } from '../mutators/test-fixtures';
import { hasCppLayout } from './labels';
import { setBlockLabelMode } from './labelMode';
import {
  isSetAside,
  MARK_CLASS,
  toggleUncheckedIndex,
  UNCHECKED_FIELD,
  UNCHECKED_MARK,
  uncheckedIndex,
  uncheckedIndexDef,
} from './marks';
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

/** The mark label of `block` (an unnamed label with the mark's words), or `undefined`. */
function markOf(block: Blockly.Block): Blockly.Field | undefined {
  return block.inputList
    .flatMap((input) => input.fieldRow)
    .find((field) => field instanceof Blockly.FieldLabel && field.getText() === UNCHECKED_MARK);
}

/** A rendered block of `type` built in `mode`. */
function built(type: string, mode: 'friendly' | 'cpp'): Blockly.BlockSvg {
  setBlockLabelMode(mode);
  const block = workspace.newBlock(type);
  block.initSvg();
  block.render();
  return block;
}

describe('the unchecked [] index', () => {
  it('is CHECKED on item of and set item, with no menu words or mark in the catalog', () => {
    const fields = BLOCK_DEFS.flatMap((def) => {
      const field = uncheckedIndexDef(def);
      return field === undefined ? [] : [`${def.id}.${field.name}`];
    });
    expect(fields).toEqual(['coll.item.CHECKED', 'coll.set_item.CHECKED']);
    for (const def of BLOCK_DEFS) {
      const field = uncheckedIndexDef(def);
      if (field !== undefined) {
        expect(field.name).toBe(UNCHECKED_FIELD);
        expect(field).not.toHaveProperty('menu');
        expect(field).not.toHaveProperty('mark');
        expect(isSetAside(field)).toBe(true);
      }
    }
    // The field does not keep a block from its C++ layout (`%LIST.at(%INDEX)`).
    const item = BLOCK_DEFS.find((def) => def.id === 'coll.item');
    expect(item !== undefined && hasCppLayout(item)).toBe(true);
  });

  it('is hidden on the block, with no mark in the Friendly label mode', () => {
    const item = built('coll.item', 'friendly');
    expect(item.getField('CHECKED')?.isVisible()).toBe(false);
    expect(markOf(item)).toBeUndefined();
    expect(uncheckedIndex(item)).toEqual({ on: false, next: 'false' });
    toggleUncheckedIndex(item);
    expect(item.getFieldValue('CHECKED')).toBe('FALSE');
    expect(uncheckedIndex(item)).toEqual({ on: true, next: 'true' });
    expect(markOf(item)).toBeUndefined();
    expect(Blockly.serialization.blocks.save(item)?.fields?.['CHECKED']).toBe(false);
  });

  it('shows the mark unchecked in the C++ label mode only while unchecked', () => {
    const item = built('coll.item', 'cpp');
    expect(item.getField('CHECKED')?.isVisible()).toBe(false);
    const mark = markOf(item);
    expect(mark?.isVisible()).toBe(false);
    expect(mark?.getSvgRoot()?.querySelector(`.${MARK_CLASS}`) ?? null).not.toBeNull();
    toggleUncheckedIndex(item);
    expect(mark?.isVisible()).toBe(true);
    toggleUncheckedIndex(item);
    expect(mark?.isVisible()).toBe(false);
  });

  it('is absent from a block without the field', () => {
    expect(uncheckedIndex(built('io.print', 'friendly'))).toBeNull();
  });

  it('loads unchecked, and flips back in one undo step', async () => {
    setBlockLabelMode('cpp');
    const set = Blockly.serialization.blocks.append(
      { type: 'coll.set_item', fields: { CHECKED: false } },
      workspace,
    );
    expect(markOf(set)?.isVisible()).toBe(true);
    await flushEvents();
    workspace.clearUndo();
    toggleUncheckedIndex(set);
    expect(markOf(set)?.isVisible()).toBe(false);
    await flushEvents();
    expect(workspace.getUndoStack()).toHaveLength(1);
    workspace.undo(false);
    expect(set.getFieldValue('CHECKED')).toBe('FALSE');
    expect(markOf(set)?.isVisible()).toBe(true);
  });
});
