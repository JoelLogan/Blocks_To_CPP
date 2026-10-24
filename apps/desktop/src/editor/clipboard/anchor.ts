/**
 * Which blocks a copy takes and where a paste goes, from what has the focus on the canvas
 * (Blockly 12: the selection is the focused node).
 *
 * A paste goes to one of the targets of 05 §5.12, which also decides which symbols its references
 * can bind to:
 *
 * - **after** a statement block, in its statement list (or its loose stack);
 * - at the start of a block's statement **list** (a focused statement-input connection, or a
 *   focused hat such as `main`, whose first list it fills);
 * - into a **value** input (a focused value-input connection, or a focused expression slot);
 * - on the **canvas**, near the focused block or at a given point.
 *
 * Whether the pasted blocks actually fit there is decided when they are inserted (./insert.ts):
 * blocks that do not fit stay on the canvas next to the target.
 */
import type { PasteTarget } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';

import { blockDefOf } from '../sync/catalog';
import { placeholderNode } from '../sync/placeholders';

/** A point in workspace coordinates. */
export interface WorkspacePoint {
  readonly x: number;
  readonly y: number;
}

/** Where a paste goes (see the module comment). */
export type PasteAnchor =
  | { readonly kind: 'after'; readonly block: Blockly.Block }
  | { readonly kind: 'list'; readonly block: Blockly.Block; readonly input: string }
  | { readonly kind: 'value'; readonly block: Blockly.Block; readonly input: string }
  | {
      readonly kind: 'canvas';
      /** A block to put the pasted blocks next to, or `null`. */
      readonly near: Blockly.Block | null;
      /** Where to put them (the point a context menu was opened at), or `null`. */
      readonly at: WorkspacePoint | null;
    };

/** The canvas, with no preferred place. */
export const ON_CANVAS: PasteAnchor = { kind: 'canvas', near: null, at: null };

/** A focused node that belongs to a block: a field, an icon or a connection. */
interface BlockPart {
  getSourceBlock(): Blockly.Block | null;
}

function isBlockPart(node: unknown): node is BlockPart {
  return (
    typeof node === 'object' &&
    node !== null &&
    typeof (node as { getSourceBlock?: unknown }).getSourceBlock === 'function'
  );
}

/** Whether a block is alive (not disposed, not being disposed). */
export function isLive(block: Blockly.Block): boolean {
  if (block.isDisposed()) {
    return false;
  }
  return !(block instanceof Blockly.BlockSvg && block.isDeadOrDying());
}

/**
 * Whether `block` is a project block of `workspace` itself: a catalog block or a placeholder, not
 * a shadow (an expression slot), an insertion marker, a flyout block or a disposed one.
 */
export function isProjectBlock(workspace: Blockly.Workspace, block: Blockly.Block): boolean {
  if (block.workspace !== workspace || block.isInFlyout || !isLive(block)) {
    return false;
  }
  if (block.isShadow() || block.isInsertionMarker()) {
    return false;
  }
  return blockDefOf(block.type) !== null || placeholderNode(block) !== null;
}

/** The block a focused node is or belongs to, or `null`. */
function ownerOf(node: unknown): Blockly.Block | null {
  if (node instanceof Blockly.Block) {
    return node;
  }
  return isBlockPart(node) ? node.getSourceBlock() : null;
}

/**
 * The block a copy takes when `node` has the focus: the focused project block, or the block whose
 * field, icon or connection has the focus. `null` for anything else (an expression slot, the
 * canvas itself, a flyout block, nothing).
 */
export function copyableBlock(workspace: Blockly.Workspace, node: unknown): Blockly.Block | null {
  const block = ownerOf(node);
  return block !== null && isProjectBlock(workspace, block) ? block : null;
}

/** The first statement input of a block that has a connection, or `null`. */
function firstStatementInput(block: Blockly.Block): string | null {
  for (const input of block.inputList) {
    if (input.type === Blockly.inputs.inputTypes.STATEMENT && input.connection !== null) {
      return input.name;
    }
  }
  return null;
}

/** Whether a block sits in statement lists: it has a previous and a next connection. */
function isStatementBlock(block: Blockly.Block): boolean {
  return block.previousConnection !== null && block.nextConnection !== null;
}

/** Where a paste goes when `block` (a project block of the canvas) has the focus. */
export function anchorForBlock(block: Blockly.Block): PasteAnchor {
  if (isStatementBlock(block)) {
    return { kind: 'after', block };
  }
  if (block.previousConnection === null && block.outputConnection === null) {
    // A hat or a definition (`main`, a function): its first statement list.
    const input = firstStatementInput(block);
    if (input !== null) {
      return { kind: 'list', block, input };
    }
  }
  return { kind: 'canvas', near: block, at: null };
}

/** Where a paste goes when a connection of a project block of the canvas has the focus. */
function anchorForConnection(
  workspace: Blockly.Workspace,
  connection: Blockly.Connection,
  depth = 0,
): PasteAnchor {
  const block = connection.getSourceBlock();
  if (!isProjectBlock(workspace, block)) {
    return ON_CANVAS;
  }
  const input = connection.getParentInput();
  if (input !== null) {
    switch (input.type) {
      case Blockly.inputs.inputTypes.VALUE:
        return { kind: 'value', block, input: input.name };
      case Blockly.inputs.inputTypes.STATEMENT:
        return { kind: 'list', block, input: input.name };
      default:
        return anchorForBlock(block);
    }
  }
  if (connection === block.nextConnection) {
    return { kind: 'after', block };
  }
  if (connection === block.previousConnection) {
    // Before the block: after the block above it, or at the start of the list it begins.
    const above = connection.targetConnection;
    return above === null || depth > 0
      ? { kind: 'canvas', near: block, at: null }
      : anchorForConnection(workspace, above, depth + 1);
  }
  return anchorForBlock(block);
}

/** The value input an expression slot (a shadow) fills, or `null`. */
function slotOf(workspace: Blockly.Workspace, shadow: Blockly.Block): PasteAnchor | null {
  const holder = shadow.outputConnection?.targetConnection ?? null;
  if (holder === null) {
    return null;
  }
  const input = holder.getParentInput();
  const parent = holder.getSourceBlock();
  return input !== null && isProjectBlock(workspace, parent)
    ? { kind: 'value', block: parent, input: input.name }
    : null;
}

/**
 * Where a paste goes when `node` has the focus on `workspace` (see the module comment). Anything
 * that is not part of a project block of the canvas pastes on the canvas.
 */
export function anchorFor(workspace: Blockly.Workspace, node: unknown): PasteAnchor {
  if (node instanceof Blockly.Connection) {
    return anchorForConnection(workspace, node);
  }
  const block = ownerOf(node);
  if (block?.workspace !== workspace || block.isInFlyout || !isLive(block)) {
    return ON_CANVAS;
  }
  if (block.isShadow()) {
    return slotOf(workspace, block) ?? ON_CANVAS;
  }
  return isProjectBlock(workspace, block) ? anchorForBlock(block) : ON_CANVAS;
}

/** Whether the blocks an anchor names are still alive on `workspace`. */
export function anchorIsLive(workspace: Blockly.Workspace, anchor: PasteAnchor): boolean {
  const block = anchor.kind === 'canvas' ? anchor.near : anchor.block;
  return block === null || (block.workspace === workspace && isLive(block));
}

/** The compiler core's paste target for an anchor in module `moduleId`. */
export function pasteTarget(anchor: PasteAnchor, moduleId: string): PasteTarget {
  switch (anchor.kind) {
    case 'after':
      return { module: moduleId, block: anchor.block.id, input: null };
    case 'list':
    case 'value':
      return { module: moduleId, block: anchor.block.id, input: anchor.input };
    case 'canvas':
      return { module: moduleId, block: null, input: null };
  }
}
