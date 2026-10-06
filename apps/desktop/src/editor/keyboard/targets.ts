/**
 * Where a block can go when it is moved with the keyboard (docs/spec/04-user-interface.md §4.7):
 * every connection on the canvas it can connect to, in reading order, plus "loose on the canvas".
 *
 * A keyboard move takes the block with what is inside it (its inputs and statement lists) but not
 * the statements below it: those close up, as when a block is deleted. So the places listed are:
 *
 * - for a statement: after any statement, and at the top of any statement list;
 * - for a reporter or predicate: any value input that holds no block of its own (an expression
 *   slot or an empty input);
 * - for every block: loose on the canvas (where a hat block such as `when program starts` can only
 *   go).
 *
 * Each place is checked with the workspace's connection checker (the type-aware checker of
 * blockly-ext), and a place where the statements already there could not hang below the block is
 * left out, so a move never knocks other blocks loose. The canvas is walked iteratively with a
 * bound, never recursively.
 */
import * as Blockly from 'blockly/core';

/** The most places offered; a larger canvas offers the first ones in reading order. */
export const MAX_DROP_TARGETS = 5000;

/** The most blocks looked at for places. */
export const MAX_VISITED_BLOCKS = 20_000;

/** A place where a moved block can go. */
export type DropTarget =
  | {
      readonly kind: 'after';
      /** The statement the block goes after. */
      readonly owner: Blockly.BlockSvg;
      readonly connection: Blockly.RenderedConnection;
    }
  | {
      readonly kind: 'statements';
      /** The block whose statement list the block goes at the top of. */
      readonly owner: Blockly.BlockSvg;
      readonly input: string;
      readonly connection: Blockly.RenderedConnection;
    }
  | {
      readonly kind: 'value';
      /** The block whose value input the block goes into. */
      readonly owner: Blockly.BlockSvg;
      readonly input: string;
      readonly connection: Blockly.RenderedConnection;
    }
  | { readonly kind: 'canvas' };

/** Blockly's connection types, as the numbers `Connection.type` holds. */
const INPUT_VALUE: number = Blockly.ConnectionType.INPUT_VALUE;
const NEXT_STATEMENT: number = Blockly.ConnectionType.NEXT_STATEMENT;
const PREVIOUS_STATEMENT: number = Blockly.ConnectionType.PREVIOUS_STATEMENT;

/**
 * A block's connections as they are: Blockly's types for rendered blocks claim that every block
 * has all three, but a hat has none and a statement no output.
 */
function connectionsOf(block: Blockly.Block): {
  readonly previous: Blockly.Connection | null;
  readonly next: Blockly.Connection | null;
  readonly output: Blockly.Connection | null;
} {
  return {
    previous: block.previousConnection,
    next: block.nextConnection,
    output: block.outputConnection,
  };
}

/** The connection a block is moved by: its previous connection, else its output, else none. */
export function movingConnection(block: Blockly.BlockSvg): Blockly.RenderedConnection | null {
  const { previous, output } = connectionsOf(block);
  const connection = previous ?? output;
  return connection instanceof Blockly.RenderedConnection ? connection : null;
}

/**
 * The blocks that move with `block`: it and everything inside it, but not the statements below it
 * (they stay, closing up).
 */
export function movingBlocks(block: Blockly.BlockSvg): Set<Blockly.Block> {
  const moving = new Set<Blockly.Block>(block.getDescendants(false));
  const below = block.getNextBlock();
  if (below !== null) {
    for (const staying of below.getDescendants(false)) {
      moving.delete(staying);
    }
  }
  return moving;
}

/** Whether `from` can connect to `to` (with Blockly's checks and the type-aware checker). */
function canConnect(
  workspace: Blockly.WorkspaceSvg,
  from: Blockly.Connection,
  to: Blockly.Connection,
): boolean {
  try {
    return workspace.connectionChecker.canConnect(from, to, false);
  } catch (error: unknown) {
    console.warn('A connection could not be checked', error);
    return false;
  }
}

/**
 * Whether the statements already at `to` (if any) can hang below `block` after it is connected
 * there, so that nothing is knocked loose.
 */
function keepsWhatIsThere(
  workspace: Blockly.WorkspaceSvg,
  block: Blockly.BlockSvg,
  to: Blockly.Connection,
): boolean {
  const there = to.targetBlock();
  if (there === null || there === block || there.isShadow()) {
    return true;
  }
  // Only the block itself moves, so what is there would hang on its own next connection.
  const previous = connectionsOf(there).previous;
  const below = connectionsOf(block).next;
  return previous !== null && below !== null && canConnect(workspace, below, previous);
}

/** Whether a value input can take the block: it holds nothing, an expression slot, or the block. */
function freeValueInput(block: Blockly.BlockSvg, connection: Blockly.Connection): boolean {
  const there = connection.targetBlock();
  return there === null || there === block || there.isShadow();
}

/** The connection the block hangs on now, or `null` when it is loose on the canvas. */
export function currentConnection(block: Blockly.BlockSvg): Blockly.Connection | null {
  return movingConnection(block)?.targetConnection ?? null;
}

/** Whether a target is where the block already is. */
export function isCurrentPlace(block: Blockly.BlockSvg, target: DropTarget): boolean {
  if (target.kind === 'canvas') {
    return block.getParent() === null;
  }
  return target.connection === currentConnection(block);
}

/** Work for the walk over the canvas: a block to visit, or a place to list. */
type Step =
  | { readonly kind: 'block'; readonly block: Blockly.BlockSvg }
  | { readonly kind: 'place'; readonly target: DropTarget };

/**
 * Every place `block` can be moved to, in reading order (top blocks top to bottom, then each block
 * before what is inside it, a statement list's top before its statements, and "after" a block
 * after what is inside it), ending with the canvas. The block's current place is included, so the
 * keyboard can start there.
 */
export function dropTargets(
  block: Blockly.BlockSvg,
  limit: number = MAX_DROP_TARGETS,
): DropTarget[] {
  const workspace = block.workspace;
  const from = movingConnection(block);
  const targets: DropTarget[] = [];
  if (from !== null) {
    const moving = movingBlocks(block);
    const isStatement = from.type === PREVIOUS_STATEMENT;
    const consider = (target: DropTarget & { connection: Blockly.RenderedConnection }) => {
      if (
        targets.length < limit &&
        canConnect(workspace, from, target.connection) &&
        (!isStatement || keepsWhatIsThere(workspace, block, target.connection))
      ) {
        targets.push(target);
      }
    };

    const steps: Step[] = [...workspace.getTopBlocks(true)]
      .reverse()
      .map((top) => ({ kind: 'block', block: top }));
    let visited = 0;
    for (
      let step = steps.pop();
      step !== undefined && targets.length < limit && visited < MAX_VISITED_BLOCKS;
      step = steps.pop()
    ) {
      if (step.kind === 'place') {
        if (step.target.kind !== 'canvas') {
          consider(step.target);
        }
        continue;
      }
      const owner = step.block;
      visited += 1;
      if (owner.isShadow() || owner.isInFlyout) {
        continue;
      }
      if (moving.has(owner)) {
        // Nothing in or after a moving block, but the statements below the moved block stay.
        const below = owner.getNextBlock();
        if (below !== null && !moving.has(below)) {
          steps.push({ kind: 'block', block: below });
        }
        continue;
      }
      // Pushed in reverse: the block's next statement last, then "after" it, then its inputs.
      const later: Step[] = [];
      for (const input of owner.inputList) {
        const connection = input.connection;
        if (!(connection instanceof Blockly.RenderedConnection) || !input.isVisible()) {
          continue;
        }
        const child = connection.targetBlock();
        if (connection.type === NEXT_STATEMENT) {
          if (isStatement) {
            later.push({
              kind: 'place',
              target: { kind: 'statements', owner, input: input.name, connection },
            });
          }
        } else if (
          connection.type === INPUT_VALUE &&
          !isStatement &&
          freeValueInput(block, connection)
        ) {
          later.push({
            kind: 'place',
            target: { kind: 'value', owner, input: input.name, connection },
          });
        }
        if (child !== null) {
          later.push({ kind: 'block', block: child });
        }
      }
      const next = connectionsOf(owner).next;
      if (next instanceof Blockly.RenderedConnection && isStatement) {
        later.push({ kind: 'place', target: { kind: 'after', owner, connection: next } });
      }
      const below = owner.getNextBlock();
      if (below !== null) {
        later.push({ kind: 'block', block: below });
      }
      for (let index = later.length - 1; index >= 0; index -= 1) {
        const item = later[index];
        if (item !== undefined) {
          steps.push(item);
        }
      }
    }
  }
  targets.push({ kind: 'canvas' });
  return targets;
}

/** Whether `target` is still a place `block` can go (the canvas may have changed since). */
export function isStillValid(block: Blockly.BlockSvg, target: DropTarget): boolean {
  if (target.kind === 'canvas') {
    return true;
  }
  const from = movingConnection(block);
  if (from === null || target.owner.isDeadOrDying() || target.owner.workspace !== block.workspace) {
    return false;
  }
  if (movingBlocks(block).has(target.owner)) {
    return false;
  }
  const workspace = block.workspace;
  if (!canConnect(workspace, from, target.connection)) {
    return false;
  }
  if (target.kind === 'value') {
    return freeValueInput(block, target.connection);
  }
  return keepsWhatIsThere(workspace, block, target.connection);
}
