/**
 * Block drags: what the editing session needs to hear of them, and what a drop on a delete area
 * does (docs/spec/03-block-language.md §3.3 "Placing blocks").
 *
 * While a block is dragged the canvas is in flux: Blockly has detached the block from where it
 * was, and an insertion marker (the drag preview) may stand in a statement list or head a loose
 * stack. The session therefore reads the canvas once just before a drag starts, and saving during
 * the drag writes that (see `EditorSession.currentDocument`). Blockly announces a drag only through
 * events, which it delivers after the block has moved, so the editor gives Blockly its own block
 * dragger ({@link B2cBlockDragger}, the `blockDragger` plugin at inject time): it reports a drag
 * before Blockly's dragger starts it, and its end.
 *
 * A drop with the pointer over a delete area (anywhere over the toolbox, its category column and
 * its always-open flyout alike, or over the trash can) deletes the dragged blocks and never
 * connects them; it is the pointer that counts, not how much of the blocks lies over the area.
 * The toolbox's area has no top or bottom (Blockly's `VerticalFlyout.getClientRect`, which the
 * continuous toolbox's `getClientRect` returns, reaches up and down without end), so a drop with
 * the pointer left of the flyout's right edge deletes at any height, also over the app's toolbar
 * above the editor or the bottom dock below it.
 *
 * Blockly 12 decides "delete" from the pointer-up event but applies the connection its last
 * pointer move previewed, then deletes the dragged stack's first block together with everything
 * that connection put below it. Blockly's gesture also ends a drag without a last move (a fast
 * flick, a touch, an automated drop), so the preview can be one the pointer has already left. The
 * editor's {@link B2cBlockDragStrategy} looks for no connection while the pointer is over a delete
 * area, and {@link B2cBlockDragger} makes one last move to the drop point before a deleting drop,
 * so nothing is connected when the dragged blocks are deleted. Both use only public and protected
 * members of Blockly's documented classes (ADR-0014), checked against Blockly 12.5.1. They go once
 * Blockly fixes the defect. It is still to be reported upstream: the owner files the report
 * drafted in `docs/plans/upstream/` and records its link here.
 *
 * Blockly checks the pointer against the places of the delete areas that the canvas recorded
 * last, and Blockly 12.5.1 records the toolbox's flyout while it is not displayed each time it
 * shows new contents. So the toolbox's flyout marks that record stale after each of its layouts
 * and whenever it moves (`toolbox/continuous.ts`), and a stale record is made again in the next
 * animation frame or, by {@link B2cBlockDragger}, at the start of a drag that comes before that
 * frame (`./dragTargets.ts`). A drag that starts once the record is current reads no place on the
 * screen for it, so its start does not make the browser lay the page out. The record at the drag
 * start goes once Blockly records after it displays the flyout (also still to be reported: the
 * report drafted in `docs/plans/upstream/`).
 *
 * A block dropped on the canvas is settled by the workspace's {@link BlockDropHandler}
 * ({@link handleBlockDrops}) inside the drag's Blockly event group: the toolbox gives a block
 * taken from it a free name, or a variable the drop point can see (04 §4.2, `toolbox/landing.ts`),
 * and one Undo takes back the drop and all of that. A block the keyboard adds from the toolbox is
 * settled the same way when it is put down ({@link settleBlockDrop}, `keyboard/mover.ts`).
 */
import * as Blockly from 'blockly/core';

import './drag.css';
import { recordDragTargetsIfStale } from './dragTargets';

/** Hears the block drags of one workspace. */
export interface BlockDragListener {
  /** A drag of `block` is about to start: the canvas is still as it was. */
  readonly onDragStart: (block: Blockly.BlockSvg) => void;
  /** The drag has ended: dropped, put back, deleted or cancelled. */
  readonly onDragEnd: () => void;
}

const LISTENERS = new WeakMap<Blockly.WorkspaceSvg, BlockDragListener>();

/**
 * Makes `listener` hear the block drags of `workspace` (one listener per workspace; a new one
 * replaces the old). Returns the function that stops it.
 */
export function listenToBlockDrags(
  workspace: Blockly.WorkspaceSvg,
  listener: BlockDragListener,
): () => void {
  LISTENERS.set(workspace, listener);
  return () => {
    if (LISTENERS.get(workspace) === listener) {
      LISTENERS.delete(workspace);
    }
  };
}

/**
 * Settles a block just dropped on the canvas (not deleted), inside the drop's Blockly event group:
 * the toolbox gives a block taken from it a free name or another variable there (04 §4.2,
 * `toolbox/landing.ts`). Every event it fires joins the drop, so one Undo takes all of it back.
 */
export type BlockDropHandler = (block: Blockly.BlockSvg) => void;

const DROP_HANDLERS = new WeakMap<Blockly.WorkspaceSvg, BlockDropHandler>();

/**
 * Makes `handler` settle the blocks dropped on `workspace` (one handler per workspace; a new one
 * replaces the old). Returns the function that stops it.
 */
export function handleBlockDrops(
  workspace: Blockly.WorkspaceSvg,
  handler: BlockDropHandler,
): () => void {
  DROP_HANDLERS.set(workspace, handler);
  return () => {
    if (DROP_HANDLERS.get(workspace) === handler) {
      DROP_HANDLERS.delete(workspace);
    }
  };
}

/** Calls a listener, so that a failing one never breaks Blockly's drag. */
function tell(run: () => void): void {
  try {
    run();
  } catch (error: unknown) {
    console.error('A block drag listener failed', error);
  }
}

/**
 * Has the {@link BlockDropHandler} of `workspace` settle `block`, just put down on it, in Blockly
 * event group `group` (the group of the gesture or keyboard add that put it there; a group of its
 * own when `group` is empty), which it opens again and closes. Nothing happens for a block that is
 * gone. A pointer drag calls it when its drop ends ({@link B2cBlockDragger}); the keyboard calls
 * it when it puts down a block added from the toolbox (`keyboard/mover.ts`), so a block gets its
 * free name, or a variable visible there, however it was added.
 */
export function settleBlockDrop(
  workspace: Blockly.WorkspaceSvg,
  block: Blockly.BlockSvg,
  group: string,
): void {
  const handler = DROP_HANDLERS.get(workspace);
  if (handler === undefined || block.isDeadOrDying()) {
    return;
  }
  Blockly.Events.setGroup(group === '' ? true : group);
  try {
    tell(() => {
      handler(block);
    });
  } finally {
    Blockly.Events.setGroup(false);
  }
}

/**
 * Whether the pointer of `e` is over one of `workspace`'s delete areas (the toolbox, whose
 * always-open flyout is its area, or the trash can).
 */
export function isOverDeleteArea(workspace: Blockly.WorkspaceSvg, e: PointerEvent): boolean {
  const target = workspace.getDragTarget(e);
  return (
    target !== null &&
    workspace
      .getComponentManager()
      .hasCapability(target.id, Blockly.ComponentManager.Capability.DELETE_AREA)
  );
}

/**
 * Blockly's block drag strategy, except that it looks for no place to connect (and so previews
 * none) while the pointer is over a delete area: a drop there deletes the dragged blocks and
 * never connects them. {@link B2cBlockDragger} gives it to every block it drags.
 */
export class B2cBlockDragStrategy extends Blockly.dragging.BlockDragStrategy {
  /** The canvas the block is dragged on, whose delete areas count. */
  private readonly canvas: Blockly.WorkspaceSvg;

  /** Whether the pointer of the latest move was over a delete area. */
  private overDeleteArea = false;

  constructor(block: Blockly.BlockSvg) {
    super(block);
    this.canvas = block.workspace;
  }

  /**
   * Moves the block to `newLoc` and updates the preview. `e` is the pointer event of the move;
   * a move without one (a programmatic drag) is never over a delete area.
   */
  override drag(newLoc: Blockly.utils.Coordinate, e?: PointerEvent): void {
    this.overDeleteArea = e !== undefined && isOverDeleteArea(this.canvas, e);
    super.drag(newLoc);
  }

  /**
   * How far away a connection may be to be previewed: Blockly's radius, or none at all over a
   * delete area. The radius is -1 rather than 0, because a search with radius 0 still finds a
   * connection exactly where the dragged one is.
   */
  protected override getSearchRadius(): number {
    return this.overDeleteArea ? -1 : super.getSearchRadius();
  }
}

/** The block a drag of `draggable` moves: for a shadow, the first block above it that is not. */
function dragRoot(draggable: Blockly.IDraggable): Blockly.IDraggable {
  let block = draggable instanceof Blockly.BlockSvg ? draggable : null;
  while (block?.isShadow() === true) {
    block = block.getParent();
  }
  return block ?? draggable;
}

/**
 * Blockly's dragger, reporting block drags to the workspace's {@link BlockDragListener} and
 * dragging every block with a {@link B2cBlockDragStrategy}. Blockly creates one per drag, in the
 * workspace the block is dragged on (also for a block taken from the toolbox, which Blockly has
 * just created there).
 */
export class B2cBlockDragger extends Blockly.dragging.Dragger {
  /**
   * A drag of a shadow (an expression slot) moves the block it belongs to, so that block is what
   * this dragger moves: Blockly's own shadow strategy would pass each move on without its pointer
   * event, and the strategy needs the event to see a delete area.
   */
  constructor(draggable: Blockly.IDraggable, workspace: Blockly.WorkspaceSvg) {
    super(dragRoot(draggable), workspace);
  }

  /**
   * Starts the drag: gives a block the editor's drag strategy, tells the listener, and, when the
   * canvas's record of where its drag targets (the toolbox, its flyout, the trash can) are was
   * marked stale and its animation frame has not come yet, has the canvas record them now
   * (`./dragTargets.ts`). Blockly checks every move and the drop against the places recorded
   * last. Blockly itself records them on a resize, a zoom and the first gesture after the page
   * scrolled, but not after every change of the toolbox's flyout (Blockly 12.5.1's `Flyout.show`
   * records them while the flyout is not displayed, when it ends at the window's left edge), and
   * a stale record made a drop on the toolbox delete nothing, with no faded blocks and with
   * connections still previewed over it (03 §3.3). The editor's flyout marks the record stale
   * after each of its layouts and whenever it moves; a drag that starts with a current record
   * reads nothing here, so it does not make the browser lay the page out
   * (`getBoundingClientRect`) when the page has changed since its last layout.
   */
  override onDragStart(e: PointerEvent): void {
    if (this.draggable instanceof Blockly.BlockSvg) {
      const block = this.draggable;
      block.setDragStrategy(new B2cBlockDragStrategy(block));
      const listener = LISTENERS.get(this.workspace);
      if (listener !== undefined) {
        tell(() => {
          listener.onDragStart(block);
        });
      }
    }
    recordDragTargetsIfStale(this.workspace);
    super.onDragStart(e);
  }

  /**
   * Ends the drag. Blockly's gesture passes the pointer's total movement; when the drop deletes
   * the dragged blocks, the block is first moved to the drop point, so the strategy drops any
   * preview the last move left behind and nothing is connected before the delete.
   *
   * A block dropped on the canvas is then settled by the workspace's {@link BlockDropHandler}
   * (`handleBlockDrops`) in the drag's Blockly event group, which Blockly's dragger closes: the
   * group is taken before Blockly ends the drag, opened again for the handler, and closed after
   * it. A block taken from the toolbox was created in the same group (Blockly's gesture starts it
   * first), so one Undo removes the block, its move and what the handler changed.
   */
  override onDragEnd(e: PointerEvent, totalDelta?: Blockly.utils.Coordinate): void {
    const group = Blockly.Events.getGroup();
    try {
      const root =
        this.draggable instanceof Blockly.BlockSvg ? this.draggable.getRootBlock() : null;
      if (totalDelta !== undefined && root !== null && this.wouldDeleteDraggable(e, root)) {
        this.onDrag(e, totalDelta);
      }
      super.onDragEnd(e);
      this.settleDrop(group);
    } finally {
      const listener = LISTENERS.get(this.workspace);
      if (listener !== undefined) {
        tell(() => {
          listener.onDragEnd();
        });
      }
    }
  }

  /**
   * Has the workspace's {@link BlockDropHandler} settle the dropped block, in event group `group`
   * (the drag's), unless the drop deleted it.
   */
  private settleDrop(group: string): void {
    if (this.draggable instanceof Blockly.BlockSvg) {
      settleBlockDrop(this.workspace, this.draggable, group);
    }
  }
}
