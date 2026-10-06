/**
 * Short names for what the keyboard reaches on the canvas, for the screen-reader announcements
 * (./announcer.ts): a block by the label Problems uses for it (`repeat until`, `main`, a function's
 * name; see ../diagnostics/blockPath.ts), a field by its text, a value by what it shows.
 *
 * Block text is project data: it only ever becomes plain text in a live region, with hidden
 * characters made visible and long text shortened by the block-path labels.
 */
import type { BdmBlock, FieldValue } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';

import { visibleInvisibles } from '../../panels/shared/invisibles';
import { blockLabel, MAX_LABEL_CHARS, shorten } from '../diagnostics/blockPath';
import { readFields } from '../sync/blockState';
import { blockDefOf } from '../sync/catalog';
import type { DropTarget } from './targets';

/** The names of the symbols a document declares, by symbol ID (for references such as `guess`). */
export type SymbolNames = ReadonlyMap<string, string>;

const NO_NAMES: SymbolNames = new Map();

/** Text as shown in an announcement: hidden characters visible, on one line, shortened. */
function shownText(text: string): string {
  return shorten(visibleInvisibles(text).replace(/\s+/g, ' ').trim(), MAX_LABEL_CHARS);
}

/** The name a block's fields declare (a variable's or a loop counter's), or `null`. */
function declaredName(fields: Readonly<Record<string, FieldValue>> | undefined): string | null {
  for (const value of Object.values(fields ?? {})) {
    if (
      typeof value === 'object' &&
      !Array.isArray(value) &&
      typeof (value as { sym?: unknown }).sym === 'string' &&
      typeof (value as { name?: unknown }).name === 'string'
    ) {
      return (value as { name: string }).name;
    }
  }
  return null;
}

/**
 * The block's short label: the block-path label for a catalog block, with the name it declares
 * (`create int variable guess`), the shown value for an expression (a number or text slot), or the
 * block's type. Never throws.
 */
export function describeBlock(block: Blockly.Block, names: SymbolNames = NO_NAMES): string {
  try {
    const def = blockDefOf(block.type);
    if (def !== null) {
      const fields = readFields(block, def, null);
      const node: BdmBlock = {
        id: block.id,
        type: block.type,
        v: def.version,
        ...(fields === undefined ? {} : { fields }),
      };
      const label = blockLabel(node, names);
      const declared = declaredName(fields);
      if (declared === null) {
        return label;
      }
      const name = shownText(declared);
      return label.endsWith(name) ? label : shorten(`${label} ${name}`, MAX_LABEL_CHARS);
    }
    const text = shownText(block.toString(MAX_LABEL_CHARS * 2));
    return text === '' ? block.type : text;
  } catch (error: unknown) {
    console.warn('A block could not be named for the screen reader', error);
    return block.type;
  }
}

/** “label” in quotation marks, as announcements name blocks. */
function quoted(text: string): string {
  return `“${text}”`;
}

/** The block a value or a shadow belongs to: its nearest parent that is not an expression. */
function ownerOf(block: Blockly.Block): Blockly.Block | null {
  let parent = block.getParent();
  while (parent !== null && blockDefOf(parent.type) === null) {
    parent = parent.getParent();
  }
  return parent;
}

/** The words shown on an input's row (`else`, `then`), or `''`. */
function inputCaption(input: Blockly.Input): string {
  const words = input.fieldRow
    .filter((field) => !field.EDITABLE)
    .map((field) => field.getText().trim())
    .filter((text) => text !== '');
  return shownText(words.join(' '));
}

/** Which of a block's inputs of one kind `name` is, when it has more than one: ` (“else” part)`. */
function whichInput(owner: Blockly.Block, name: string, kind: Blockly.ConnectionType): string {
  const inputs = owner.inputList.filter((input) => input.connection?.type === kind);
  if (inputs.length < 2) {
    return '';
  }
  const index = inputs.findIndex((input) => input.name === name);
  const input = inputs[index];
  if (input === undefined) {
    return '';
  }
  const caption = inputCaption(input);
  return caption === '' ? ` (part ${String(index + 1)})` : ` (${quoted(caption)} part)`;
}

/**
 * Where a moved block would go, as one short phrase: `after “print”`, `at the top of “repeat
 * until”`, `into “create guess”`, `loose on the canvas`. Never throws.
 */
export function describeTarget(target: DropTarget, names: SymbolNames = NO_NAMES): string {
  try {
    switch (target.kind) {
      case 'canvas':
        return 'loose on the canvas';
      case 'after':
        return `after ${quoted(describeBlock(target.owner, names))}`;
      case 'statements':
        return `at the top of ${quoted(describeBlock(target.owner, names))}${whichInput(
          target.owner,
          target.input,
          Blockly.ConnectionType.NEXT_STATEMENT,
        )}`;
      case 'value':
        return `into ${quoted(describeBlock(target.owner, names))}${whichInput(
          target.owner,
          target.input,
          Blockly.ConnectionType.INPUT_VALUE,
        )}`;
    }
  } catch (error: unknown) {
    console.warn('A place could not be named for the screen reader', error);
    return 'a place';
  }
}

/**
 * What the keyboard cursor is on, as one short phrase: `block “print”`, `field “int” of “create
 * guess”`, `value “0” in “create guess”`, `empty input of “print”`, `the canvas`. Never throws.
 */
export function describeNode(node: unknown, names: SymbolNames = NO_NAMES): string {
  try {
    if (node instanceof Blockly.Field) {
      const block = node.getSourceBlock();
      const text = shownText(node.getText());
      const what = text === '' ? 'empty field' : `field ${quoted(text)}`;
      if (block === null) {
        return what;
      }
      if (blockDefOf(block.type) === null) {
        const owner = ownerOf(block);
        return owner === null ? what : `${what} in ${quoted(describeBlock(owner, names))}`;
      }
      return `${what} of ${quoted(describeBlock(block, names))}`;
    }
    if (node instanceof Blockly.BlockSvg) {
      if (blockDefOf(node.type) === null) {
        const owner = ownerOf(node);
        const value = `value ${quoted(describeBlock(node, names))}`;
        return owner === null ? value : `${value} in ${quoted(describeBlock(owner, names))}`;
      }
      const disabled = node.isEnabled() ? '' : ', disabled';
      return `block ${quoted(describeBlock(node, names))}${disabled}`;
    }
    if (node instanceof Blockly.RenderedConnection) {
      return `empty input of ${quoted(describeBlock(node.getSourceBlock(), names))}`;
    }
    if (node instanceof Blockly.FlyoutButton) {
      const text = shownText(node.getButtonText());
      return node.isLabel() ? `category ${quoted(text)}` : `button ${quoted(text)}`;
    }
    if (node instanceof Blockly.WorkspaceSvg) {
      return node.isFlyout ? 'the toolbox’s blocks' : 'the canvas';
    }
  } catch (error: unknown) {
    console.warn('The keyboard cursor could not be named for the screen reader', error);
  }
  return '';
}
