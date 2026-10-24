/**
 * Creates the toolbox's blocks in a flyout (flyout item kind `b2c_block`, see presets.ts).
 *
 * It is Blockly's block inflater with four differences:
 *
 * - each declaration the item names gets a new symbol ID, so no two blocks created from one item
 *   declare the same symbol;
 * - a call block from *Your functions* gets its argument labels (`n:`) right away. On the canvas the
 *   call mutator labels arguments from the analysis, which does not know flyout blocks;
 * - blocks are never recycled between showings, so each showing has exactly the presets it was
 *   built with (presets change between showings);
 * - each block remembers the item it was built from, so a block dragged out of it can be settled
 *   where it lands (./landing.ts).
 *
 * Its items have their own type, so the flyout gives them back to this inflater to dispose of.
 */
import * as Blockly from 'blockly/core';

import { rememberToolboxItem } from './landing';
import { B2C_BLOCK_KIND, blockState, type B2cBlockInfo } from './presets';

/** The field name of the flyout-only label before argument `index`. */
export function argLabelField(index: number): string {
  return `B2C_TOOLBOX_LABEL_ARG${String(index)}`;
}

/** Whether `state` is one of our block items. */
function isB2cBlockInfo(state: object): state is B2cBlockInfo {
  return (state as { kind?: unknown }).kind === B2C_BLOCK_KIND;
}

/**
 * Blockly's block item for one of ours: the same block, with a new symbol ID for each declaration
 * and without our own keys.
 */
export function plainBlockInfo(info: B2cBlockInfo): Blockly.utils.toolbox.BlockInfo {
  return { kind: 'block', ...blockState(info) };
}

/** Shows `labels[i]` before the call argument `ARG{i}` (empty labels are skipped). */
export function applyArgLabels(block: Blockly.Block, labels: readonly string[]): void {
  labels.forEach((text, index) => {
    const input = block.getInput(`ARG${String(index)}`);
    if (text === '' || input === null || block.getField(argLabelField(index)) !== null) {
      return;
    }
    input.appendField(new Blockly.FieldLabel(text), argLabelField(index));
  });
}

/** The flyout inflater of the toolbox's blocks. */
export class B2cBlockInflater extends Blockly.BlockFlyoutInflater {
  override load(state: object, flyout: Blockly.IFlyout): Blockly.FlyoutItem {
    const info = isB2cBlockInfo(state) ? state : null;
    const block = super.load(info === null ? state : plainBlockInfo(info), flyout).getElement();
    if (info !== null && block instanceof Blockly.BlockSvg) {
      rememberToolboxItem(block, info);
      if (info.argLabels !== undefined) {
        applyArgLabels(block, info.argLabels);
      }
    }
    return new Blockly.FlyoutItem(block, B2C_BLOCK_KIND);
  }

  override getType(): string {
    return B2C_BLOCK_KIND;
  }
}
