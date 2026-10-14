/**
 * Wrapping statements in a C-block (docs/spec/03-block-language.md §3.3 "Placing blocks";
 * decision D13 (a) of the M2 polish plan): an empty statement C-block dragged with the pointer
 * onto a statement of a list takes that statement's place, and the statement and everything below
 * it in the list go into the C-block's first mouth, as in Scratch. An empty C-block is never
 * inserted between two statements.
 *
 * The C-block's top picks the statement: the pair Blockly compares is the dragged block's previous
 * connection and the connection above the statement (the next connection of the statement before
 * it, or the statement input whose first statement it is), the pair an insertion would use. The
 * C-block's mouth does not pick it: a preview puts the wrapped statements into the mouth, about
 * 50 canvas units from where they were, so a pair with the wrapped statement's own connection
 * would be measured against where the preview moved it, and the next move would pick another
 * place or none (the W5-2 spike in the plan measured this). The connection above stays where it
 * is while the preview is shown.
 *
 * Three parts apply this one rule ({@link wrapPlace}):
 *
 * - the connection checker allows the pair while the C-block is dragged with the pointer
 *   (`B2cConnectionChecker.doDragChecks`);
 * - the connection previewer shows the C-block wrapping the statements
 *   (`B2cConnectionPreviewer`, ./previewer.ts);
 * - when the drop is done, the editor's block dragger moves the statements Blockly hung below the
 *   C-block into its mouth ({@link completeWrap}), in the drop's event group, so one Undo takes the
 *   whole drop back.
 *
 * Wrapping is a pointer gesture: a block moved with the keyboard (it is not being dragged) is
 * inserted as before. The top of a loose stack has no connection above it; there Blockly's own
 * pair, the C-block's mouth with the stack's first block, puts the stack into the mouth.
 */
import * as Blockly from 'blockly/core';

import { blockDef } from './catalog';

/** Connection kinds as plain numbers (`Connection.type` is a number). */
const NEXT_STATEMENT: number = Blockly.ConnectionType.NEXT_STATEMENT;
const PREVIOUS_STATEMENT: number = Blockly.ConnectionType.PREVIOUS_STATEMENT;

/** A drop that wraps statements: what goes where. */
export interface WrapPlace {
  /** The empty statement C-block that is dragged. */
  readonly block: Blockly.Block;
  /** Its first mouth, where the wrapped statements go. */
  readonly mouth: Blockly.Connection;
  /**
   * The connection the C-block takes: the next connection of the statement before the wrapped
   * one, or the statement input whose first statement it is.
   */
  readonly place: Blockly.Connection;
  /** The first statement wrapped; the statements below it in its list go with it. */
  readonly wrapped: Blockly.Block;
}

/** The first statement input's connection of `block` (its first mouth), or `null`. */
export function firstMouth(block: Blockly.Block): Blockly.Connection | null {
  for (const input of block.inputList) {
    if (input.type === Blockly.inputs.inputTypes.STATEMENT && input.connection !== null) {
      return input.connection;
    }
  }
  return null;
}

/**
 * Whether `block` is an empty statement C-block: a statement of the catalog (not a hat or a
 * definition) with a previous connection and a first mouth that holds nothing.
 */
export function isEmptyStatementCBlock(block: Blockly.Block): boolean {
  if (blockDef(block.type)?.shape !== 'statement' || block.previousConnection === null) {
    return false;
  }
  const mouth = firstMouth(block);
  return mouth !== null && mouth.targetConnection === null;
}

/**
 * The real statement held at `place`, looking through the C-block's own wrap preview: while the
 * preview is shown, `place` holds an insertion marker whose first mouth holds the statement.
 */
function statementAt(place: Blockly.Connection): Blockly.Block | null {
  const held = place.targetBlock();
  if (held?.isInsertionMarker() !== true) {
    return held;
  }
  const mouth = firstMouth(held);
  return mouth === null ? null : mouth.targetBlock();
}

/**
 * The wrap that connecting `dragged` to `target` stands for, or `null` when it is not one:
 * `dragged` is the previous connection of an empty statement C-block that is dragged with the
 * pointer (Blockly lists the connections of the dragged blocks in `common.draggingConnections`;
 * a keyboard move does not), and `target` is a next connection or statement input, on a block
 * that is neither dragged nor a drag preview, that holds a statement which can move.
 */
export function wrapPlace(
  dragged: Blockly.Connection,
  target: Blockly.Connection,
): WrapPlace | null {
  const block = dragged.getSourceBlock();
  if (
    dragged.type !== PREVIOUS_STATEMENT ||
    target.type !== NEXT_STATEMENT ||
    block.previousConnection !== dragged ||
    !Blockly.common.draggingConnections.includes(dragged) ||
    Blockly.common.draggingConnections.includes(target) ||
    target.getSourceBlock().isInsertionMarker() ||
    !isEmptyStatementCBlock(block)
  ) {
    return null;
  }
  const mouth = firstMouth(block);
  const wrapped = statementAt(target);
  if (
    mouth === null ||
    wrapped === null ||
    wrapped.isInsertionMarker() ||
    wrapped.isShadow() ||
    !wrapped.isMovable() ||
    wrapped.previousConnection === null
  ) {
    return null;
  }
  return { block, mouth, place: target, wrapped };
}

/**
 * Finishes a wrap after the drop: Blockly has connected the C-block at `wrap.place` and hung the
 * statements that were there below the C-block's stack (or left them loose, when nothing below
 * the stack takes them); this moves them into its first mouth. Nothing happens unless the drop did
 * exactly that (the C-block deleted, put elsewhere, or no longer empty; the statements gone or
 * somewhere else). Returns whether the statements were moved. Call it inside the drop's Blockly
 * event group.
 */
export function completeWrap(wrap: WrapPlace): boolean {
  const { block, mouth, place, wrapped } = wrap;
  const previous = wrapped.previousConnection;
  if (
    block.isDeadOrDying() ||
    wrapped.isDeadOrDying() ||
    previous === null ||
    block.previousConnection?.targetConnection !== place ||
    mouth.targetConnection !== null ||
    !(wrapped.getParent() === null || isBelow(wrapped, block))
  ) {
    return false;
  }
  return mouth.connect(previous);
}

/** Whether `statement` is in the stack below `block` (on its chain of next blocks). */
function isBelow(statement: Blockly.Block, block: Blockly.Block): boolean {
  for (let next = block.getNextBlock(); next !== null; next = next.getNextBlock()) {
    if (next === statement) {
      return true;
    }
  }
  return false;
}
