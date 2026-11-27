/**
 * Checkboxes not drawn on the block (docs/plans/m3-contracts/w4-editor-tools.md §7):
 *
 * - The unchecked `[]` index, the field `CHECKED` of *item of* and *set item*: the app's block menu
 *   toggles it with its item *Use unchecked `[]`* (the app's `editor/menus/registry.ts`, which owns
 *   the item's words), using {@link uncheckedIndex} and {@link toggleUncheckedIndex}. The block
 *   shows it in the C++ label mode only, as the mark *unchecked* after its parts while unticked
 *   (§7.1); the Friendly label mode shows nothing.
 * - The catalog's `popover` fields (`constexpr` and `static` on *create*), set from the block's ⚙
 *   popover (./gear.ts): the block shows each field's `mark` while it holds something other than
 *   its default, in either label mode, and after the last one the ⚙.
 *
 * The checkbox stays on the block, hidden, so the block's state and the project hold it as any
 * field; each change by the menu, the popover, an undo or a load shows or hides its mark.
 */
import * as Blockly from 'blockly/core';

import type { BlockDefJson, FieldDefJson } from '../generated/catalog';
import { catalogBlock } from './catalog';
import { B2cGearField, flipCheckbox, popoverFieldDefs } from './gear';
import { blockLabelMode } from './labelMode';

/** The CSS class of a mark's text. */
export const MARK_CLASS = 'b2c-mark';

/** The name of the unchecked `[]` index's checkbox (the catalog's `UNCHECKED_FIELD`). */
export const UNCHECKED_FIELD = 'CHECKED';

/** The words the C++ label mode shows after a block that uses the unchecked `[]` index. */
export const UNCHECKED_MARK = 'unchecked';

/** Whether a checkbox value (a boolean, or Blockly's `'TRUE'`/`'FALSE'`) is ticked. */
function ticked(value: unknown): boolean {
  return value === true || value === 'TRUE';
}

/** Whether `value` is the default of the checkbox `def`. */
function atDefault(def: FieldDefJson, value: unknown): boolean {
  return ticked(value) === (def.default === true);
}

/** Whether `field` is the unchecked `[]` index's checkbox. */
function isUncheckedIndexField(field: FieldDefJson): boolean {
  return field.name === UNCHECKED_FIELD && field.kind === 'checkbox';
}

/**
 * Whether `field` is not drawn on the block (the unchecked `[]` index, or a popover field), so no
 * label names it.
 */
export function isSetAside(field: FieldDefJson): boolean {
  return isUncheckedIndexField(field) || field.popover !== undefined;
}

/** The unchecked `[]` index's checkbox of `def`, or `undefined` for a block without it. */
export function uncheckedIndexDef(def: Pick<BlockDefJson, 'fields'>): FieldDefJson | undefined {
  return def.fields.find(isUncheckedIndexField);
}

/**
 * Hides the unchecked `[]` index and each popover field of `block` and puts its mark after it,
 * shown while the field is away from its default, now and whenever it changes (by the menu, the
 * popover, an undo or a load): the unchecked index's in the C++ label mode only. Then puts the ⚙
 * after the last popover field's mark.
 */
export function applyMenuMarks(block: Blockly.Block, def: BlockDefJson): void {
  const popover = popoverFieldDefs(def);
  let gearAfter: Blockly.Field | null = null;
  for (const fieldDef of def.fields) {
    if (!isSetAside(fieldDef)) {
      continue;
    }
    const field = block.getField(fieldDef.name);
    const input = field?.getParentInput();
    if (field === null || input === undefined) {
      continue;
    }
    field.setVisible(false);
    const words = isUncheckedIndexField(fieldDef)
      ? blockLabelMode() === 'cpp'
        ? UNCHECKED_MARK
        : null
      : (fieldDef.mark ?? '');
    if (words === null) {
      continue;
    }
    const mark = new Blockly.FieldLabel(words, MARK_CLASS);
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

/** The unchecked `[]` index of a block, as the block menu's item shows it. */
export interface UncheckedIndex {
  /** Whether the block uses `[]` (the checkbox is unticked): the item shows a tick. */
  readonly on: boolean;
  /** The value toggling sets, as an option value (`'true'` or `'false'`), for the gate. */
  readonly next: 'true' | 'false';
}

/** The unchecked `[]` index of `block`, or `null` for a block without it. */
export function uncheckedIndex(block: Blockly.Block): UncheckedIndex | null {
  const def = catalogBlock(block.type);
  const fieldDef = def === undefined ? undefined : uncheckedIndexDef(def);
  const field = fieldDef === undefined ? null : block.getField(fieldDef.name);
  if (fieldDef === undefined || field === null) {
    return null;
  }
  const value = ticked(field.getValue());
  return { on: !atDefault(fieldDef, value), next: value ? 'false' : 'true' };
}

/** Toggles the unchecked `[]` index of `block`, as one undo step. */
export function toggleUncheckedIndex(block: Blockly.Block): void {
  flipCheckbox(block, UNCHECKED_FIELD);
}
