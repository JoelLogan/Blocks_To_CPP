/**
 * Placeholders for blocks whose type the catalog does not have, usually because a library pack is
 * missing (B2C-E0601, docs/spec/03-block-language.md §3.11). The placeholder is a grey block with a
 * "Missing pack: X" badge. It keeps the block's project data verbatim (its fields, inputs, nested
 * blocks and stack), so saving the project writes the block back unchanged.
 *
 * All placeholders share one internal block type, {@link PLACEHOLDER_TYPE}, and carry the original
 * block in their extra state. The original type is never used as a Blockly type name: it comes from
 * the file and may be any text, and Blockly turns type names into CSS class names.
 */
import type { BdmBlock } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';

import { truncateForDisplay, visibleInvisibles } from '../text';
import { PLACEHOLDER_BLOCK_STYLE } from '../theme/themes';
import { B2C_CSS_CLASS } from './css';
import { applyShape } from './shape';

/** The Blockly type of every placeholder. */
export const PLACEHOLDER_TYPE = 'b2c.placeholder';

/**
 * Where the original block sits, which decides the placeholder's connections: in a statement list
 * (`statement`), in a value input (`value`), or at the top level of the canvas (`top`, no
 * connections: nothing is known about what may attach to it).
 */
export type PlaceholderShape = 'statement' | 'value' | 'top';

/** What a placeholder keeps. */
export interface PlaceholderData {
  /** The original block, exactly as the project file has it. */
  readonly node: BdmBlock;
  readonly shape: PlaceholderShape;
}

/** The longest pack or type name shown on the badge. */
const MAX_SHOWN_CHARS = 40;

const SHAPES: readonly PlaceholderShape[] = ['statement', 'value', 'top'];

/**
 * The pack a block type comes from: the part before the first `.` (`sfml.window.open` → `sfml`), or
 * the whole type when it has none.
 */
export function missingPackOf(type: string): string {
  const dot = type.indexOf('.');
  return dot > 0 ? type.slice(0, dot) : type;
}

/** The text shown for untrusted names: invisible characters as placeholders, cut to a short length. */
function shown(text: string): string {
  return truncateForDisplay(visibleInvisibles(text, { lineBreaks: true }), MAX_SHOWN_CHARS);
}

/** Whether `value` is placeholder extra state. Only the parts the placeholder reads are checked. */
function isPlaceholderData(value: unknown): value is PlaceholderData {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const { node, shape } = value as { node?: unknown; shape?: unknown };
  return (
    typeof node === 'object' &&
    node !== null &&
    !Array.isArray(node) &&
    typeof (node as { type?: unknown }).type === 'string' &&
    typeof shape === 'string' &&
    SHAPES.includes(shape as PlaceholderShape)
  );
}

/**
 * The Blockly state of a placeholder for `node`, to append with
 * `Blockly.serialization.blocks.append`. The node is copied.
 */
export function placeholderState(
  node: BdmBlock,
  shape: PlaceholderShape,
): Blockly.serialization.blocks.State {
  return {
    type: PLACEHOLDER_TYPE,
    extraState: { node: structuredClone(node), shape } satisfies PlaceholderData,
  };
}

/** Whether a block is a placeholder. */
export function isPlaceholder(block: Blockly.Block): boolean {
  return block.type === PLACEHOLDER_TYPE;
}

/** What a placeholder keeps (a copy), or null when the block is not a loaded placeholder. */
export function readPlaceholder(block: Blockly.Block): PlaceholderData | null {
  if (!isPlaceholder(block)) {
    return null;
  }
  const data = (block as PlaceholderBlock).b2cPlaceholder;
  return data === null ? null : structuredClone(data);
}

interface PlaceholderBlock extends Blockly.Block {
  b2cPlaceholder: PlaceholderData | null;
}

/** Registers {@link PLACEHOLDER_TYPE}. Idempotent. */
export function registerPlaceholderBlock(): void {
  if (Object.hasOwn(Blockly.Blocks, PLACEHOLDER_TYPE)) {
    return;
  }
  Blockly.Blocks[PLACEHOLDER_TYPE] = {
    init(this: PlaceholderBlock): void {
      this.b2cPlaceholder = null;
      this.setStyle(PLACEHOLDER_BLOCK_STYLE);
      this.setInputsInline(true);
      this.appendDummyInput('b2c_row0')
        .appendField(new Blockly.FieldLabel('Missing pack', B2C_CSS_CLASS.missingPack), 'PACK')
        .appendField(new Blockly.FieldLabel(''), 'TYPE');
      this.setTooltip(
        'This block comes from a library pack that is not loaded. It is kept unchanged; load the pack to edit it.',
      );
    },

    saveExtraState(this: PlaceholderBlock): PlaceholderData | null {
      return this.b2cPlaceholder === null ? null : structuredClone(this.b2cPlaceholder);
    },

    loadExtraState(this: PlaceholderBlock, state: unknown): void {
      if (!isPlaceholderData(state) || this.b2cPlaceholder !== null) {
        return;
      }
      this.b2cPlaceholder = structuredClone(state);
      if (state.shape === 'statement') {
        applyShape(this, 'statement');
      } else if (state.shape === 'value') {
        applyShape(this, 'reporter');
      }
      const type = state.node.type;
      this.setFieldValue(`Missing pack: ${shown(missingPackOf(type))}`, 'PACK');
      this.setFieldValue(shown(type), 'TYPE');
    },
  };
}
