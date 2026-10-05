/**
 * Block registration from the generated catalog (docs/adr/0002-block-editor-blockly.md: block
 * definitions are generated from the catalog, not hand-written Blockly JSON).
 *
 * Every catalog block becomes a Blockly block type whose type name is the catalog ID. Blocks are
 * built in code (`init`), not from Blockly's JSON format: no catalog text goes through Blockly's
 * message interpolation (`%1`, `%{BKY_…}`), and every label is a plain `FieldLabel`, rendered as SVG
 * text.
 */
import * as Blockly from 'blockly/core';

import { createCatalogField, registerB2cFields } from '../fields/register';
import { BLOCK_DEFS, type BlockDefJson } from '../generated/catalog';
import { registerExpressionShadows } from '../shadows/register';
import { visibleInvisibles } from '../text';
import { blockStyleFor } from '../theme/themes';
import { catalogFieldOptions } from './catalog';
import { registerB2cCss } from './css';
import { blockLayout, type BlockLayout, type LayoutItem } from './layout';
import { MUTATOR_FOR_BLOCK } from './mutators';
import { registerPlaceholderBlock } from './placeholder';
import { applyShape } from './shape';

/** A block could not be built as its definition says (a missing mutator, a bad catalog entry). */
export class BlockRegistrationError extends Error {
  override readonly name = 'BlockRegistrationError';
}

/**
 * Defines every catalog block as a Blockly block type, plus the internal expression shadows and the
 * "missing pack" placeholder, and registers the Blocks2Cpp fields. Idempotent: types that already
 * exist are left as they are.
 *
 * Blocks whose definitions need a mutator (see `MUTATOR_FOR_BLOCK`) can be created only after
 * `registerB2cMutators()` has run; creating one earlier throws {@link BlockRegistrationError}.
 */
export function registerB2cBlocks(): void {
  registerB2cFields();
  registerB2cCss();
  for (const def of BLOCK_DEFS) {
    if (!Object.hasOwn(Blockly.Blocks, def.id)) {
      Blockly.Blocks[def.id] = blockDefinition(def);
    }
  }
  registerExpressionShadows();
  registerPlaceholderBlock();
}

/** The Blockly definition (`init`) of one catalog block. */
export function blockDefinition(def: BlockDefJson): { init: (this: Blockly.Block) => void } {
  const layout = blockLayout(def);
  const mutator = MUTATOR_FOR_BLOCK[def.id];
  const tooltip = visibleInvisibles(def.help);
  return {
    init(this: Blockly.Block): void {
      buildInputs(this, def, layout);
      applyShape(this, def.shape);
      this.setStyle(blockStyleFor(def.category, def.shape));
      this.setTooltip(tooltip);
      this.setInputsInline(true);
      if (mutator !== undefined) {
        if (!Blockly.Extensions.isRegistered(mutator)) {
          throw new BlockRegistrationError(
            `The block ${def.id} needs the mutator ${mutator}; call registerB2cMutators() first.`,
          );
        }
        Blockly.Extensions.apply(mutator, this, true);
      }
    },
  };
}

function buildInputs(block: Blockly.Block, def: BlockDefJson, layout: BlockLayout): void {
  for (const input of layout.inputs) {
    let created: Blockly.Input;
    switch (input.kind) {
      case 'value':
        created = block.appendValueInput(input.name);
        break;
      case 'statement':
        created = block.appendStatementInput(input.name);
        break;
      case 'dummy':
      case 'anchor':
        created = block.appendDummyInput(input.name);
        break;
    }
    for (const item of input.items) {
      appendItem(block, created, def, item);
    }
  }
}

function appendItem(
  block: Blockly.Block,
  input: Blockly.Input,
  def: BlockDefJson,
  item: LayoutItem,
): void {
  if (item.kind === 'label') {
    input.appendField(new Blockly.FieldLabel(item.text));
    return;
  }
  const fieldDef = def.fields.find((field) => field.name === item.name);
  if (fieldDef === undefined) {
    throw new BlockRegistrationError(`The block ${def.id} has no field ${item.name}.`);
  }
  input.appendField(
    createCatalogField(fieldDef, catalogFieldOptions(block.type, fieldDef.name)),
    fieldDef.name,
  );
}
