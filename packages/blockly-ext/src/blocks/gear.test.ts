/**
 * The ⚙ popover (./gear.ts): `constexpr` and `static` on *create*
 * (docs/plans/m3-contracts/w4-editor-tools.md §7).
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { setOptionFilter } from '../fields/option-filter';
import { BLOCK_DEFS } from '../generated/catalog';
import { installIdGenerator } from '../ids';
import { registerB2cMutators } from '../mutators/register';
import { flushEvents } from '../mutators/test-fixtures';
import { B2cGearField, GEAR_TEXT, gearPopover, popoverFieldDefs } from './gear';
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
  setOptionFilter(null);
  Blockly.DropDownDiv.hideWithoutAnimation();
  workspace.dispose();
  document.body.replaceChildren();
});

function fieldsOf(block: Blockly.Block): Blockly.Field[] {
  return block.inputList.flatMap((input) => input.fieldRow);
}

function gearOf(block: Blockly.Block): B2cGearField {
  const gear = fieldsOf(block).find((field) => field instanceof B2cGearField);
  if (!(gear instanceof B2cGearField)) {
    throw new Error(`${block.type} has no ⚙`);
  }
  return gear;
}

function marks(block: Blockly.Block): string[] {
  return fieldsOf(block)
    .filter((field) => field instanceof Blockly.FieldLabel && field.isVisible())
    .map((field) => field.getText())
    .filter((text) => text === 'constexpr' || text === 'static');
}

function ticks(popover: HTMLElement): Record<string, boolean> {
  return Object.fromEntries(
    [...popover.querySelectorAll('input')].map((box): [string, boolean] => [
      box.dataset['field'] ?? '',
      box.checked,
    ]),
  );
}

describe('the ⚙ popover', () => {
  it('holds constexpr and static of create, and nothing else', () => {
    const withPopover = BLOCK_DEFS.flatMap((def) =>
      popoverFieldDefs(def).map((field) => `${def.id}.${field.name}`),
    );
    expect(withPopover).toEqual(['var.declare.CONSTEXPR', 'var.declare.STATIC']);
  });

  it('is a ⚙ after the hidden ticks, not saved with the block, showing the ticked marks', () => {
    const create = workspace.newBlock('var.declare');
    create.initSvg();
    create.render();
    expect(create.getField('CONSTEXPR')?.isVisible()).toBe(false);
    expect(create.getField('STATIC')?.isVisible()).toBe(false);
    const gear = gearOf(create);
    expect(gear.getText()).toBe(GEAR_TEXT);
    expect(gear.isClickable()).toBe(true);
    expect(fieldsOf(create).at(-1)).toBe(gear);
    expect(marks(create)).toEqual([]);
    // The ⚙ has no name, so the block's state holds the catalog's fields only.
    const fieldNames = BLOCK_DEFS.find((def) => def.id === 'var.declare')?.fields.map(
      (f) => f.name,
    );
    expect(Object.keys(Blockly.serialization.blocks.save(create)?.fields ?? {}).sort()).toEqual(
      [...(fieldNames ?? [])].sort(),
    );

    const loaded = Blockly.serialization.blocks.append(
      { type: 'var.declare', fields: { CONSTEXPR: true, STATIC: true } },
      workspace,
    );
    expect(marks(loaded)).toEqual(['constexpr', 'static']);
    const saved = Blockly.serialization.blocks.save(loaded)?.fields ?? {};
    expect([saved['CONSTEXPR'], saved['STATIC']]).toEqual([true, true]);
  });

  it('offers static only where the gate allows it, or where it is ticked', () => {
    setOptionFilter((_block, field, value) => !(field === 'STATIC' && value === 'true'));
    const ticksDef = popoverFieldDefs(
      BLOCK_DEFS.find((def) => def.id === 'var.declare') ?? { fields: [] },
    );
    const create = Blockly.serialization.blocks.append({ type: 'var.declare' }, workspace);
    expect(ticks(gearPopover(create, ticksDef, () => undefined))).toEqual({ CONSTEXPR: false });
    const kept = Blockly.serialization.blocks.append(
      { type: 'var.declare', fields: { STATIC: true } },
      workspace,
    );
    expect(ticks(gearPopover(kept, ticksDef, () => undefined))).toEqual({
      CONSTEXPR: false,
      STATIC: true,
    });
  });

  it('opens in the drop-down, ticks in one undo step each, and closes on Escape', async () => {
    const create = Blockly.serialization.blocks.append(
      { type: 'var.declare' },
      workspace,
    ) as Blockly.BlockSvg;
    await flushEvents();
    workspace.clearUndo();
    const gear = gearOf(create);
    gear.showEditor();
    const popover = gear.popover;
    expect(popover?.getAttribute('role')).toBe('group');
    expect(popover?.textContent).toContain('constexpr: ');
    const box = popover?.querySelector<HTMLInputElement>('input[data-field="CONSTEXPR"]');
    expect(document.activeElement).toBe(popover?.querySelector('input'));
    box?.click();
    expect(create.getFieldValue('CONSTEXPR')).toBe('TRUE');
    expect(box?.checked).toBe(true);
    expect(marks(create)).toEqual(['constexpr']);
    await flushEvents();
    expect(workspace.getUndoStack()).toHaveLength(1);

    const close = vi.fn();
    const other = gearPopover(
      create,
      popoverFieldDefs(BLOCK_DEFS.find((def) => def.id === 'var.declare') ?? { fields: [] }),
      close,
    );
    other.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(close).toHaveBeenCalledOnce();
  });
});
