/**
 * Checkboxes set from a block's context menu or its ⚙ popover instead of drawn on the block (the
 * catalog's `menu` and `popover` keys: *Fast unchecked access* on *item of* and *set item*,
 * `constexpr` and `static` on *create*; docs/plans/m3-contracts/w4-editor-tools.md §7). The checkbox
 * stays on the block, hidden, so the block's state and the project hold it as any field; next to it
 * the block shows the field's `mark` (`unchecked`, `static`) while it holds something other than its
 * default, in either label mode, and after the last popover field a ⚙ opens the popover (./gear.ts).
 * The app's block menu lists one item per menu field ({@link menuToggles}) and flips it with
 * {@link toggleMenuField}, one undo step.
 */
import * as Blockly from 'blockly/core';

import type { BlockDefJson, FieldDefJson } from '../generated/catalog';
import { catalogBlock } from './catalog';
import { B2cGearField, flipCheckbox, popoverFieldDefs } from './gear';

/** The CSS class of a mark's text. */
export const MARK_CLASS = 'b2c-mark';

/** Whether a checkbox value (a boolean, or Blockly's `'TRUE'`/`'FALSE'`) is ticked. */
function ticked(value: unknown): boolean {
  return value === true || value === 'TRUE';
}

/** Whether `value` is the default of the menu field `def`. */
function atDefault(def: FieldDefJson, value: unknown): boolean {
  return ticked(value) === (def.default === true);
}

/** The fields of `def` set from the block's menu, in catalog order. */
export function menuFieldDefs(def: Pick<BlockDefJson, 'fields'>): FieldDefJson[] {
  return def.fields.filter((field) => field.menu !== undefined);
}

/**
 * Hides each menu and popover field of `block` and puts its mark after it, shown while the field is
 * away from its default, now and whenever it changes (by the menu, the popover, an undo or a load);
 * then puts the ⚙ after the last popover field's mark.
 */
export function applyMenuMarks(block: Blockly.Block, def: BlockDefJson): void {
  const popover = popoverFieldDefs(def);
  let gearAfter: Blockly.Field | null = null;
  for (const fieldDef of def.fields) {
    if (fieldDef.menu === undefined && fieldDef.popover === undefined) {
      continue;
    }
    const field = block.getField(fieldDef.name);
    const input = field?.getParentInput();
    if (field === null || input === undefined) {
      continue;
    }
    field.setVisible(false);
    const mark = new Blockly.FieldLabel(fieldDef.mark ?? '', MARK_CLASS);
    input.insertFieldAt(input.fieldRow.indexOf(field) + 1, mark);
    if (fieldDef.popover !== undefined) {
      gearAfter = mark;
    }
    // While the block is being built it is drawn afterwards; later changes redraw it.
    mark.setVisible(!atDefault(fieldDef, field.getValue()));
    field.setValidator((value: unknown) => {
      const shown = !atDefault(fieldDef, value);
      if (mark.isVisible() !== shown) {
        mark.setVisible(shown);
        if (block instanceof Blockly.BlockSvg) {
          void block.queueRender();
        }
      }
      return undefined;
    });
  }
  const input = gearAfter?.getParentInput();
  if (gearAfter !== null && input !== undefined) {
    input.insertFieldAt(input.fieldRow.indexOf(gearAfter) + 1, new B2cGearField(popover));
  }
}

/** One menu field of a block, as its menu item shows it. */
export interface MenuToggle {
  /** The field's name (`CHECKED`). */
  readonly field: string;
  /** The menu item's words. */
  readonly text: string;
  /** Whether the field is away from its default: the item shows a tick. */
  readonly on: boolean;
  /** The value the item sets, as an option value (`'true'` or `'false'`), for the gate. */
  readonly next: 'true' | 'false';
}

/** The menu fields of `block`, in catalog order. */
export function menuToggles(block: Blockly.Block): MenuToggle[] {
  const def = catalogBlock(block.type);
  if (def === undefined) {
    return [];
  }
  return menuFieldDefs(def).flatMap((fieldDef): MenuToggle[] => {
    const field = block.getField(fieldDef.name);
    if (field === null) {
      return [];
    }
    const value = ticked(field.getValue());
    return [
      {
        field: fieldDef.name,
        text: fieldDef.menu ?? fieldDef.name,
        on: !atDefault(fieldDef, value),
        next: value ? 'false' : 'true',
      },
    ];
  });
}

/** Flips the menu field `name` of `block`, as one undo step. */
export function toggleMenuField(block: Blockly.Block, name: string): void {
  flipCheckbox(block, name);
}
