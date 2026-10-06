/**
 * Inserting prepared blocks (the result of the core's `pastePrepare`) into the workspace, as one
 * undoable step.
 *
 * The blocks are built with the document sync's builder (../sync/bdmToWorkspace.ts), so pasted
 * blocks are exactly what loading them from a file would give (placeholders included). They are
 * first built on the canvas with Blockly's events off; then, with events on and in one event
 * group, a `BlockCreate` is fired for each and the leading statements (or the one reporter) are
 * connected at the anchor. Undo therefore removes the pasted blocks and puts back anything the
 * insertion moved (the statements that followed the anchor), and the editing session sees the new
 * blocks like any other (fresh-ID check, preview).
 *
 * Blocks that do not fit at the anchor (a reporter after a statement, a statement in a value input,
 * a hat anywhere but the canvas, a refused type) stay on the canvas next to it, statements that
 * followed each other still chained.
 */
import type { BdmBlock } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';

import { buildBlockTree, withoutEvents } from '../sync/bdmToWorkspace';
import { blockDefOf } from '../sync/catalog';
import { clampCoordinate } from '../sync/limits';
import type { PasteAnchor, WorkspacePoint } from './anchor';

/** How far each further block placed on the canvas is moved from the one before (both axes). */
export const CANVAS_STEP = 32;

/** The most steps taken to find a canvas position no other top-level block has. */
const MAX_FREE_SPOT_STEPS = 100;

/** Where the pasted blocks ended up. */
export interface InsertedBlocks {
  /** Every block built at the top of a tree or chain, in the pasted order. */
  readonly roots: readonly Blockly.Block[];
  /** The first pasted block, or `null` when nothing was pasted. */
  readonly first: Blockly.Block | null;
  /** Whether the leading blocks were connected at the anchor (always false on the canvas). */
  readonly attached: boolean;
}

/** Whether a node is a catalog block of `shape` at the catalog's version. */
function hasShape(node: BdmBlock, shapes: readonly string[]): boolean {
  const def = blockDefOf(node.type);
  return def !== null && def.version === node.v && shapes.includes(def.shape);
}

/**
 * How many of the leading nodes go to the anchor: the leading statements for a statement list
 * (none of them with a loose stack, which only a canvas block may have), or the single reporter or
 * predicate for a value input. The rest stay on the canvas.
 */
export function leadingCount(nodes: readonly BdmBlock[], anchor: PasteAnchor): number {
  switch (anchor.kind) {
    case 'canvas':
      return 0;
    case 'value': {
      const only = nodes.length === 1 ? nodes[0] : undefined;
      return only !== undefined && hasShape(only, ['reporter', 'predicate']) ? 1 : 0;
    }
    case 'after':
    case 'list':
      return chainableRun(nodes, 0);
  }
}

/**
 * Whether a node can go into a statement list (or be chained below another statement): a
 * statement without a loose stack, which only a canvas block may have.
 */
function isChainable(node: BdmBlock | undefined): node is BdmBlock {
  return node !== undefined && node.stack === undefined && hasShape(node, ['statement']);
}

/** How many nodes from `start` on can be chained, one below the other. */
function chainableRun(nodes: readonly BdmBlock[], start: number): number {
  let end = start;
  while (end < nodes.length && isChainable(nodes[end])) {
    end += 1;
  }
  return end - start;
}

/**
 * How many nodes from the start of `rest` (blocks that stay on the canvas) form one chain there.
 * For a target in or after a block, the core has flattened each copied stack into the list (its
 * head, then the stacked blocks), so a run of statements is chained again rather than scattered.
 * For a canvas target every node is a block of its own: a copied stack still carries `stack`.
 */
function canvasRun(rest: readonly BdmBlock[], anchor: PasteAnchor): number {
  if (rest.length === 0) {
    return 0;
  }
  return anchor.kind === 'canvas' ? 1 : Math.max(1, chainableRun(rest, 0));
}

/** The connection at the anchor that the leading blocks connect to, or `null`. */
function anchorConnection(anchor: PasteAnchor): Blockly.Connection | null {
  switch (anchor.kind) {
    case 'after':
      return anchor.block.nextConnection;
    case 'list':
    case 'value':
      return anchor.block.getInput(anchor.input)?.connection ?? null;
    case 'canvas':
      return null;
  }
}

/** The top-level blocks' positions, to keep pasted blocks from landing exactly on another one. */
function takenSpots(workspace: Blockly.Workspace): Set<string> {
  const spots = new Set<string>();
  for (const block of workspace.getTopBlocks(false)) {
    const xy = block.getRelativeToSurfaceXY();
    spots.add(`${String(Math.round(xy.x))},${String(Math.round(xy.y))}`);
  }
  return spots;
}

/** Hands out canvas positions from `origin` on, stepping past occupied ones. */
class SpotFinder {
  private next: WorkspacePoint;
  private readonly taken: Set<string>;

  constructor(workspace: Blockly.Workspace, origin: WorkspacePoint) {
    this.taken = takenSpots(workspace);
    this.next = { x: clampCoordinate(origin.x), y: clampCoordinate(origin.y) };
  }

  take(): WorkspacePoint {
    let spot = this.next;
    for (let step = 0; step < MAX_FREE_SPOT_STEPS; step += 1) {
      if (!this.taken.has(`${String(spot.x)},${String(spot.y)}`)) {
        break;
      }
      spot = this.after(spot);
    }
    this.taken.add(`${String(spot.x)},${String(spot.y)}`);
    this.next = this.after(spot);
    return spot;
  }

  private after(spot: WorkspacePoint): WorkspacePoint {
    return {
      x: clampCoordinate(spot.x + CANVAS_STEP),
      y: clampCoordinate(spot.y + CANVAS_STEP),
    };
  }
}

/**
 * Builds `nodes` as one chain on the canvas at `spot` (the first on top, each further one
 * connected below the one before) and returns its first block. Stops chaining when a block has no
 * `next` connection; the remaining nodes are returned.
 */
function buildChain(
  workspace: Blockly.Workspace,
  nodes: readonly BdmBlock[],
  spot: WorkspacePoint,
): { head: Blockly.Block | null; rest: readonly BdmBlock[] } {
  const [first, ...others] = nodes;
  if (first === undefined) {
    return { head: null, rest: [] };
  }
  const head = buildBlockTree(workspace, first, { kind: 'top', x: spot.x, y: spot.y });
  let last = head;
  for (let index = 0; index < others.length; index += 1) {
    const node = others[index];
    const connection = last.nextConnection;
    if (node === undefined || connection === null) {
      return { head, rest: others.slice(index) };
    }
    last = buildBlockTree(workspace, node, { kind: 'statement', connection });
  }
  return { head, rest: [] };
}

/**
 * Removes, with events off, every top-level block that is not in `before`: what a failed build left
 * behind. Nothing was announced or connected yet, so nothing else changed.
 */
function discardBuilt(workspace: Blockly.Workspace, before: ReadonlySet<Blockly.Block>): void {
  withoutEvents(() => {
    for (const block of workspace.getTopBlocks(false)) {
      if (!before.has(block) && !block.isDisposed()) {
        block.dispose(false);
      }
    }
  });
}

/** Tells the workspace (and its undo stack) about a block built with events off. */
function announce(block: Blockly.Block): void {
  if (Blockly.Events.isEnabled()) {
    Blockly.Events.fire(new Blockly.Events.BlockCreate(block));
  }
}

/**
 * Connects `head` (the first leading block) at the anchor's connection, if the connection checker
 * allows it there. A value input that already holds a block (not an expression slot) is left as
 * it is.
 */
function attach(workspace: Blockly.Workspace, anchor: PasteAnchor, head: Blockly.Block): boolean {
  const target = anchorConnection(anchor);
  const own = anchor.kind === 'value' ? head.outputConnection : head.previousConnection;
  if (target === null || own === null) {
    return false;
  }
  const occupant = target.targetBlock();
  if (anchor.kind === 'value' && occupant !== null && !occupant.isShadow()) {
    return false;
  }
  try {
    if (!workspace.connectionChecker.canConnect(own, target, false)) {
      return false;
    }
    target.connect(own);
  } catch (error: unknown) {
    console.warn('A pasted block could not be connected; it stays on the canvas', error);
    return false;
  }
  return own.targetConnection === target;
}

/** The blocks built on the canvas, before they are announced and connected. */
interface Built {
  /** Every block built at the top of a tree or chain, in the pasted order. */
  readonly roots: readonly Blockly.Block[];
  /** The head of the leading chain that goes to the anchor, or `null`. */
  readonly lead: Blockly.Block | null;
}

/** Builds `nodes` on the canvas with events off: the leading chain first, then the rest. */
function buildAll(
  workspace: Blockly.Workspace,
  nodes: readonly BdmBlock[],
  anchor: PasteAnchor,
  origin: WorkspacePoint,
): Built {
  const spots = new SpotFinder(workspace, origin);
  const roots: Blockly.Block[] = [];
  const leading = leadingCount(nodes, anchor);
  let rest: readonly BdmBlock[] = nodes;
  let lead: Blockly.Block | null = null;
  if (leading > 0) {
    const chain = buildChain(workspace, nodes.slice(0, leading), spots.take());
    lead = chain.head;
    if (lead !== null) {
      roots.push(lead);
    }
    rest = [...chain.rest, ...nodes.slice(leading)];
  }
  while (rest.length > 0) {
    const run = canvasRun(rest, anchor);
    const chain = buildChain(workspace, rest.slice(0, run), spots.take());
    if (chain.head !== null) {
      roots.push(chain.head);
    }
    rest = [...chain.rest, ...rest.slice(run)];
  }
  return { roots, lead };
}

/**
 * Inserts prepared blocks at `anchor` as one undoable step (see the module comment). `origin` is
 * where blocks that stay on the canvas start; each further one is moved a step down and right.
 *
 * @throws whatever the builder throws for a node it cannot build; the blocks built until then are
 *   removed again, so the canvas is as it was.
 */
export function insertPasted(
  workspace: Blockly.Workspace,
  nodes: readonly BdmBlock[],
  anchor: PasteAnchor,
  origin: WorkspacePoint,
): InsertedBlocks {
  if (nodes.length === 0) {
    return { roots: [], first: null, attached: false };
  }
  const outerGroup = Blockly.Events.getGroup();
  if (outerGroup === '') {
    Blockly.Events.setGroup(true);
  }
  const svg = workspace instanceof Blockly.WorkspaceSvg ? workspace : null;
  svg?.setResizesEnabled(false);
  try {
    const before = new Set(workspace.getTopBlocks(false));
    let built: Built;
    try {
      built = buildAll(workspace, nodes, anchor, origin);
    } catch (error: unknown) {
      discardBuilt(workspace, before);
      throw error;
    }
    for (const root of built.roots) {
      announce(root);
    }
    const attached = built.lead !== null && attach(workspace, anchor, built.lead);
    return { roots: built.roots, first: built.roots[0] ?? null, attached };
  } finally {
    svg?.setResizesEnabled(true);
    if (outerGroup === '') {
      Blockly.Events.setGroup(false);
    }
  }
}
