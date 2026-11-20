/**
 * Block shapes under the Zelos renderer (docs/spec/03-block-language.md §3.3): which connections a
 * block has, and the outline of its output, set explicitly rather than derived from Blockly check
 * strings.
 */
import type * as Blockly from 'blockly/core';

import type { Shape } from '../generated/catalog';

/**
 * Zelos output shapes (`Blockly.zelos.ConstantProvider.SHAPES`): hexagonal for predicates, round
 * for reporters. test/blocks.test.ts checks them against Blockly's constants.
 */
export const ZELOS_OUTPUT_SHAPE = Object.freeze({ hexagonal: 1, round: 2 } as const);

/**
 * Gives a block the connections of its shape:
 * - hat and definition blocks: none above, below or as a value (they stand alone on the canvas;
 *   a project cannot chain anything after them);
 * - a definition that is a namespace member (`namespaceMember`, w3-modules.md §2.12): a previous
 *   and a next connection, which only a namespace's mouth and the definitions in one use
 *   (./namespace.ts); at the top level it still draws as a definition;
 * - statements: a previous and a next connection;
 * - reporters: a round output; predicates: a hexagonal output.
 *
 * No connection has Blockly type checks: the Blocks2Cpp connection checker decides what fits.
 */
export function applyShape(block: Blockly.Block, shape: Shape, namespaceMember = false): void {
  switch (shape) {
    case 'hat':
      return;
    case 'definition':
      if (namespaceMember) {
        block.setPreviousStatement(true, null);
        block.setNextStatement(true, null);
      }
      return;
    case 'statement':
      block.setPreviousStatement(true, null);
      block.setNextStatement(true, null);
      return;
    case 'reporter':
      block.setOutput(true, null);
      block.setOutputShape(ZELOS_OUTPUT_SHAPE.round);
      return;
    case 'predicate':
      block.setOutput(true, null);
      block.setOutputShape(ZELOS_OUTPUT_SHAPE.hexagonal);
      return;
  }
}
