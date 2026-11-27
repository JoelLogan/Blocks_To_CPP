/**
 * The ⚙ popover (docs/plans/m3-contracts/w4-editor-tools.md §7, 03 §3.7.2): checkboxes the catalog
 * sets aside with `popover` (`constexpr` and `static` on *create*) are not drawn on the block; one
 * ⚙ after their marks opens a popover in Blockly's drop-down with a tick per field, each with the
 * catalog's words. A tick the gate refuses (`static` without *Show advanced blocks*) is left out
 * unless the field already holds it. Each change is one undo step ({@link flipCheckbox}); Escape closes the popover and gives the focus back to the block.
 *
 * Every text is set as text, never as HTML.
 */
import * as Blockly from 'blockly/core';

import type { BlockDefJson, FieldDefJson } from '../generated/catalog';
import { optionOffered } from '../fields/option-filter';

/** The ⚙ button's text. */
export const GEAR_TEXT = '⚙';

/** The CSS class of the popover. */
export const GEAR_POPOVER_CLASS = 'b2c-gear-popover';

/** The fields of `def` set from its ⚙ popover, in catalog order. */
export function popoverFieldDefs(def: Pick<BlockDefJson, 'fields'>): FieldDefJson[] {
  return def.fields.filter((field) => field.popover !== undefined);
}

/** Whether a checkbox value (a boolean, or Blockly's `'TRUE'`/`'FALSE'`) is ticked. */
function ticked(value: unknown): boolean {
  return value === true || value === 'TRUE';
}

/** Flips the checkbox `name` of `block` as one undo step. */
export function flipCheckbox(block: Blockly.Block, name: string): void {
  const field = block.getField(name);
  if (field === null) {
    return;
  }
  Blockly.Events.setGroup(true);
  try {
    field.setValue(!ticked(field.getValue()));
  } finally {
    Blockly.Events.setGroup(false);
  }
}

/**
 * The popover's content for `block`: a labelled group with a checkbox per tick of `ticks` the gate
 * offers (or the field holds). `onClose` is called on Escape.
 */
export function gearPopover(
  block: Blockly.Block,
  ticks: readonly FieldDefJson[],
  onClose: () => void,
  doc: Document = document,
): HTMLElement {
  const group = doc.createElement('div');
  group.className = GEAR_POPOVER_CLASS;
  group.setAttribute('role', 'group');
  group.setAttribute('aria-label', 'Options of this block');
  for (const tick of ticks) {
    const field = block.getField(tick.name);
    if (field === null) {
      continue;
    }
    const on = ticked(field.getValue());
    const next = on ? 'false' : 'true';
    const atDefault = on === (tick.default === true);
    if (atDefault && !optionOffered(block, tick.name, next)) {
      continue;
    }
    const label = doc.createElement('label');
    const box = doc.createElement('input');
    box.type = 'checkbox';
    box.checked = on;
    box.dataset['field'] = tick.name;
    box.addEventListener('change', () => {
      flipCheckbox(block, tick.name);
      box.checked = ticked(field.getValue());
    });
    const words = doc.createElement('span');
    words.textContent = tick.popover ?? tick.name;
    label.append(box, words);
    group.append(label);
  }
  group.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      onClose();
    }
  });
  return group;
}

/** The ⚙ button: a label that opens the popover, not saved with the block. */
export class B2cGearField extends Blockly.FieldLabel {
  override EDITABLE = true;
  override SERIALIZABLE = false;

  /** The open popover, while there is one (tests). */
  popover: HTMLElement | null = null;

  /** The ticks the popover offers. */
  private readonly ticks: readonly FieldDefJson[];

  constructor(ticks: readonly FieldDefJson[]) {
    super(GEAR_TEXT);
    this.ticks = ticks;
  }

  /** Opens the popover in Blockly's drop-down, the focus on its first tick. */
  protected override showEditor_(): void {
    const block = this.getSourceBlock();
    if (block === null) {
      return;
    }
    const doc = block instanceof Blockly.BlockSvg ? block.getSvgRoot().ownerDocument : document;
    const close = (): void => {
      Blockly.DropDownDiv.hideIfOwner(this, true);
    };
    const popover = gearPopover(block, this.ticks, close, doc);
    this.popover = popover;
    Blockly.DropDownDiv.clearContent();
    Blockly.DropDownDiv.getContentDiv().append(popover);
    Blockly.DropDownDiv.setColour(
      'var(--b2c-surface-raised, #ffffff)',
      'var(--b2c-border, #c4c4c4)',
    );
    Blockly.DropDownDiv.showPositionedByField(this, () => {
      this.popover = null;
    });
    popover.querySelector('input')?.focus();
  }
}
