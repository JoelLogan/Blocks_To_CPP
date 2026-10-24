/**
 * Placeholders: blocks the editor shows but cannot edit, which keep their project data verbatim
 * (blockly-ext's `b2c.placeholder`). The sync uses them for
 *
 * - a block type the catalog does not have (a missing library pack, B2C-E0601), and
 * - a catalog block the editor cannot show faithfully: a field or `extra` value its fields refuse,
 *   an input its mutator state does not have, a block in a place its shape does not fit, or a
 *   newer block version. The analyser reports what is wrong (B2C-E06xx); the block is saved back
 *   unchanged until the user deletes it.
 *
 * A placeholder holds its whole subtree (nested blocks included), so saving never loses data.
 */
import type { BdmBlock } from '@blocks2cpp/b2c-core-wasm';
import {
  type PlaceholderData,
  type PlaceholderShape,
  readPlaceholder,
  truncateForDisplay,
  visibleInvisibles,
} from '@blocks2cpp/blockly-ext';
import type * as Blockly from 'blockly/core';

import { blockDefOf } from './catalog';

/** The longest block type shown on a placeholder for a known type. */
const MAX_SHOWN_TYPE_CHARS = 40;

/** What Blockly's `loadExtraState` member of a block looks like. */
interface WithExtraState {
  loadExtraState?: (state: unknown) => void;
}

/** blockly-ext keeps a placeholder's data in this member (see `readPlaceholder`). */
interface PlaceholderMember {
  b2cPlaceholder: PlaceholderData | null;
}

/**
 * Turns a new `b2c.placeholder` block into the placeholder of `node`. For a catalog type it says the
 * block cannot be shown here (instead of naming a missing pack).
 */
export function initPlaceholder(
  block: Blockly.Block,
  node: BdmBlock,
  shape: PlaceholderShape,
): void {
  const data: PlaceholderData = { node, shape };
  (block as unknown as WithExtraState).loadExtraState?.(data);
  if (blockDefOf(node.type) !== null) {
    const shown = truncateForDisplay(
      visibleInvisibles(node.type, { lineBreaks: true }),
      MAX_SHOWN_TYPE_CHARS,
    );
    block.setFieldValue('Cannot show', 'PACK');
    block.setFieldValue(shown, 'TYPE');
    block.setTooltip(
      'This block has values this version of Blocks2Cpp cannot show here. It is kept unchanged; the Problems panel says what is wrong. Delete it, or fix the file.',
    );
  }
}

/** The project data a placeholder keeps (a copy), or `null` for any other block. */
export function placeholderNode(block: Blockly.Block): BdmBlock | null {
  return readPlaceholder(block)?.node ?? null;
}

/**
 * Replaces the data of an existing placeholder (after its copy got fresh IDs). Returns false when
 * the block is not a loaded placeholder.
 */
export function replacePlaceholderNode(block: Blockly.Block, node: BdmBlock): boolean {
  const data = readPlaceholder(block);
  if (data === null) {
    return false;
  }
  // blockly-ext loads placeholder data only once, so a copy's data is replaced in place.
  (block as unknown as PlaceholderMember).b2cPlaceholder = { node, shape: data.shape };
  return true;
}
