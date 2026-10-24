/**
 * Inserting blocks for the end-to-end tests (`window.__B2C_E2E__.insertBlocks`): the tests drag
 * some blocks for real and add the rest of a program through this helper, which builds them as
 * the clipboard's paste does (editor/clipboard/insert.ts). The insertion is one undoable step, a
 * refused one leaves no step behind, and the editing session sees the new blocks like any others
 * (fresh-ID check, live preview).
 *
 * The blocks come from the test, so they are checked like any other input: the target must be a
 * project block on the shown canvas, the input one of its own, and every block a catalog block the
 * editor can show exactly (a block that would only fit as a placeholder is refused).
 */
import type { BdmBlock } from '@blocks2cpp/b2c-core-wasm';
import { isProjectId } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';

import { isProjectBlock, type PasteAnchor } from '../editor/clipboard/anchor';
import { type BuiltBlocks, canAttach, insertPasted } from '../editor/clipboard/insert';
import { forEachNode } from '../editor/sync/bdmTree';
import { placeholderNode } from '../editor/sync/placeholders';
import { descendantsOf } from '../editor/sync/traverse';

/** The most blocks one call inserts at the top level. */
export const MAX_INSERTED_ROOTS = 64;

/** The most blocks one call inserts in all, nested ones included. */
export const MAX_INSERTED_BLOCKS = 2000;

/** What an input name looks like (the catalog's names: `BODY`, `COND0`, `ITEM1`). */
const INPUT_NAME = /^[A-Z][A-Z0-9_]{0,63}$/;

/** Why an insertion was refused; nothing was changed. */
export class InsertBlocksError extends Error {
  override readonly name = 'InsertBlocksError';
}

/** Whether `value` is a plain object (not an array, not null). */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * Checks the shape the builder relies on (an object with a project ID, a type and a version, at
 * every level) and the limits. The builder checks everything else against the catalog.
 */
export function checkNodes(blocks: unknown): BdmBlock[] {
  if (!Array.isArray(blocks) || blocks.length === 0 || blocks.length > MAX_INSERTED_ROOTS) {
    throw new InsertBlocksError(
      `blocks must be a list of 1 to ${String(MAX_INSERTED_ROOTS)} blocks`,
    );
  }
  let count = 0;
  const pending: unknown[] = (blocks as unknown[]).slice();
  for (let node = pending.pop(); node !== undefined; node = pending.pop()) {
    count += 1;
    if (count > MAX_INSERTED_BLOCKS) {
      throw new InsertBlocksError(`at most ${String(MAX_INSERTED_BLOCKS)} blocks can be inserted`);
    }
    if (
      !isRecord(node) ||
      !isProjectId(node['id']) ||
      typeof node['type'] !== 'string' ||
      typeof node['v'] !== 'number'
    ) {
      throw new InsertBlocksError('every block needs an id, a type and a version');
    }
    pending.push(...nestedNodes(node));
  }
  return blocks as BdmBlock[];
}

/** The nodes nested in `node`, checking that its inputs, statement lists and stack are shaped right. */
function nestedNodes(node: Record<string, unknown>): unknown[] {
  const nested: unknown[] = [];
  const { inputs, statements, stack } = node;
  if (inputs !== undefined) {
    if (!isRecord(inputs) || !Object.values(inputs).every(isRecord)) {
      throw new InsertBlocksError('inputs must map input names to {expr} or {block}');
    }
    for (const value of Object.values(inputs) as Record<string, unknown>[]) {
      if ('block' in value) {
        nested.push(value['block']);
      }
    }
  }
  if (statements !== undefined) {
    if (!isRecord(statements) || !Object.values(statements).every(Array.isArray)) {
      throw new InsertBlocksError('statements must map input names to lists of blocks');
    }
    for (const list of Object.values(statements) as unknown[][]) {
      nested.push(...list);
    }
  }
  if (stack !== undefined) {
    if (!Array.isArray(stack)) {
      throw new InsertBlocksError('stack must be a list of blocks');
    }
    nested.push(...(stack as unknown[]));
  }
  return nested;
}

/** The last block of the statement list in `input` of `block`, or `null` when it is empty. */
function lastInList(block: Blockly.Block, input: string): Blockly.Block | null {
  let last: Blockly.Block | null = null;
  for (
    let next = block.getInput(input)?.connection?.targetBlock() ?? null;
    next !== null;
    next = next.getNextBlock()
  ) {
    last = next;
  }
  return last;
}

/** Where blocks for `input` of `parent` go: the end of a statement list, or a value input. */
function anchorFor(parent: Blockly.Block, input: string): PasteAnchor {
  const found = parent.getInput(input);
  if (found?.connection === null || found === null) {
    throw new InsertBlocksError(`block ${parent.id} has no input ${input}`);
  }
  if (found.type === Blockly.inputs.inputTypes.STATEMENT) {
    const last = lastInList(parent, input);
    return last === null ? { kind: 'list', block: parent, input } : { kind: 'after', block: last };
  }
  if (found.type === Blockly.inputs.inputTypes.VALUE) {
    return { kind: 'value', block: parent, input };
  }
  throw new InsertBlocksError(`input ${input} of block ${parent.id} takes no blocks`);
}

/**
 * Refuses built blocks before anything is announced (the `check` of `insertPasted`): a block that
 * is only a placeholder, or blocks that are not one tree or chain that connects at `anchor`.
 */
function refuseMisfits(
  workspace: Blockly.Workspace,
  anchor: PasteAnchor,
  built: BuiltBlocks,
  target: string,
): void {
  const placeholder = built.roots
    .flatMap((root) => descendantsOf(root))
    .find((block) => placeholderNode(block) !== null);
  if (placeholder !== undefined) {
    throw new InsertBlocksError(`block ${placeholder.id} is not one the editor can show exactly`);
  }
  const [only, ...others] = built.roots;
  if (
    built.lead === null ||
    only !== built.lead ||
    others.length > 0 ||
    !canAttach(workspace, anchor, built.lead)
  ) {
    throw new InsertBlocksError(`the blocks do not fit into ${target}`);
  }
}

/** Removes inserted blocks again (in the insertion's own event group). */
function removeInserted(roots: readonly Blockly.Block[]): void {
  for (const root of roots) {
    if (!root.isDisposed()) {
      root.dispose(false);
    }
  }
}

/**
 * Inserts `blocks` into `input` of block `parentBlockId` on `workspace` (the shown module's
 * canvas): appended to the end of a statement list, or, for a value input, the one reporter or
 * predicate in place of the input's expression slot.
 *
 * The blocks are checked once they are built and before anything is announced, so a refused
 * insertion fires no event and leaves nothing on the undo stack. Should the connection still fail
 * after that check, the blocks are removed in the insertion's own event group: one undo step that
 * changes nothing.
 *
 * @throws InsertBlocksError when the target or the blocks are not valid, a block is not one the
 *   editor can show exactly, or the blocks do not fit at the target; nothing is changed then.
 *   Errors of the clipboard's builder (a chain too long for Blockly) are passed on.
 */
export function insertBlocks(
  workspace: Blockly.WorkspaceSvg,
  parentBlockId: unknown,
  input: unknown,
  blocks: unknown,
): void {
  if (typeof parentBlockId !== 'string' || !isProjectId(parentBlockId)) {
    throw new InsertBlocksError('parentBlockId must be a block ID');
  }
  if (typeof input !== 'string' || !INPUT_NAME.test(input)) {
    throw new InsertBlocksError('input must be an input name such as BODY or COND0');
  }
  const nodes = checkNodes(blocks);
  const taken = new Set<string>();
  forEachNode(nodes, (node) => {
    if (taken.has(node.id) || workspace.getBlockById(node.id) !== null) {
      throw new InsertBlocksError(`block ID ${node.id} is already used`);
    }
    taken.add(node.id);
  });
  const parent = workspace.getBlockById(parentBlockId);
  if (parent === null || !isProjectBlock(workspace, parent)) {
    throw new InsertBlocksError(`there is no block ${parentBlockId} on the canvas`);
  }
  const anchor = anchorFor(parent, input);
  const origin = parent.getRelativeToSurfaceXY();
  const target = `${input} of block ${parentBlockId}`;
  const outerGroup = Blockly.Events.getGroup();
  if (outerGroup === '') {
    Blockly.Events.setGroup(true);
  }
  try {
    const inserted = insertPasted(
      workspace,
      nodes,
      anchor,
      { x: origin.x, y: origin.y },
      {
        check: (built) => {
          refuseMisfits(workspace, anchor, built, target);
        },
      },
    );
    if (!inserted.attached) {
      removeInserted(inserted.roots);
      throw new InsertBlocksError(`the blocks do not fit into ${target}`);
    }
  } finally {
    if (outerGroup === '') {
      Blockly.Events.setGroup(false);
    }
  }
}
