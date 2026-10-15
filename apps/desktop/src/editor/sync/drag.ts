/**
 * Block drags: what the editing session needs to hear of them, and what a drop on a delete area
 * does (docs/spec/03-block-language.md §3.3 "Placing blocks").
 *
 * While a block is dragged the canvas is in flux: Blockly has detached the block from where it
 * was, and an insertion marker (the drag preview) may stand in a statement list or head a loose
 * stack. The session therefore keeps the canvas as it was just before a drag starts (the project's
 * document when the preview pipeline has nothing left to read, else a read of the canvas), and
 * saving during the drag writes that (see `EditorSession.currentDocument`). Blockly announces a
 * drag only through events, which it delivers after the block has moved, so the editor gives
 * Blockly its own block dragger ({@link B2cBlockDragger}, the `blockDragger` plugin at inject
 * time): it reports a drag before Blockly's dragger starts it, and its end.
 *
 * A dragged block snaps to a place within about 44 screen pixels of its connection at every zoom
 * (03 §3.3), where Blockly snaps from 28 canvas units at any zoom: {@link B2cBlockDragStrategy}
 * sets the radius ({@link snapRadius}), keeps a place previewed for a statement until another is
 * clearly nearer, and gives a value the nearest slot.
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
import { completeWrap, previewedWrap, type WrapPlace } from '@blocks2cpp/blockly-ext';
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
 * Finishes a drop that wraps statements in an empty C-block (03 §3.3, `completeWrap`) in Blockly
 * event group `group` (the drag's; a group of its own when `group` is empty), which it opens
 * again and closes. A failure is logged and leaves Blockly's drop as it is: the C-block in the
 * statements' place with the statements below it.
 */
function finishWrap(wrap: WrapPlace, group: string): void {
  Blockly.Events.setGroup(group === '' ? true : group);
  try {
    completeWrap(wrap);
  } catch (error: unknown) {
    console.error('A wrap could not be finished', error);
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
 * How near a dragged block's connection must come to a place, in screen pixels, for the block to
 * snap there (03 §3.3): the same reach at every zoom, within {@link MIN_SNAP_RADIUS} and
 * {@link MAX_SNAP_RADIUS} canvas units.
 */
export const SNAP_SCREEN_PIXELS = 44;

/**
 * The smallest snap radius, in canvas units: zoomed in, where 44 pixels are fewer canvas units, a
 * place stays about as easy to reach on the canvas as at the start zoom (49 units at 90 %), and
 * never less than with Blockly's own 28.
 */
export const MIN_SNAP_RADIUS = 48;

/**
 * The largest snap radius, in canvas units: zoomed far out, a block does not jump to a place that
 * is far away on the canvas.
 */
export const MAX_SNAP_RADIUS = 96;

/**
 * How much nearer, in canvas units, another place must be than the place previewed for the preview
 * to move there while a statement is dragged (Blockly's `currentConnectionPreference`, 8 by
 * default). With the larger snap radius more places of a statement list are in reach, and the grey
 * preview (an insertion marker, which makes the list lay itself out again) would otherwise jump
 * between two places almost as near at every small move. The places of a list lie at least a
 * block's height (48 units) apart.
 */
export const PREVIEW_PREFERENCE = 20;

/**
 * The same preference while a value (a block with an output) is dragged: none, the nearest slot
 * wins. A block's first slot lies only 8 units right of the slot that holds the block (Zelos), so
 * with Blockly's 8, and all the more with 20, a value that came within reach of the outer slot
 * first (dragged in from the left, from the toolbox) stayed on it and replaced the block there
 * instead of going into its first slot. Zelos previews a slot by highlighting it, without an
 * insertion marker, so a change of the slot previewed costs nothing.
 */
export const VALUE_PREVIEW_PREFERENCE = 0;

/**
 * The snap radius, in canvas units, on a canvas at zoom `scale`: {@link SNAP_SCREEN_PIXELS} on
 * screen, kept between {@link MIN_SNAP_RADIUS} and {@link MAX_SNAP_RADIUS}. Blockly's 28 units
 * are about 25 pixels at the editor's start zoom of 90 % and 14 at 50 %, too small a target for a
 * block dropped by eye.
 */
export function snapRadius(scale: number): number {
  return Math.min(
    MAX_SNAP_RADIUS,
    Math.max(MIN_SNAP_RADIUS, Math.round(SNAP_SCREEN_PIXELS / scale)),
  );
}

/**
 * Blockly's block drag strategy, except that:
 *
 * - it looks for no place to connect (and so previews none) while the pointer is over a delete
 *   area: a drop there deletes the dragged blocks and never connects them;
 * - it snaps from {@link SNAP_SCREEN_PIXELS} on screen at every zoom ({@link snapRadius}), where
 *   Blockly snaps from 28 canvas units whatever the zoom;
 * - while a statement is dragged, a previewed place keeps the preview until another is
 *   {@link PREVIEW_PREFERENCE} units nearer; for a dragged value the nearest slot wins
 *   ({@link VALUE_PREVIEW_PREFERENCE}).
 *
 * Bumping, which moves a loose block away from a connection it is not connected to, keeps
 * Blockly's 28 units (`Blockly.config.snapRadius` is not changed): a larger bump would push loose
 * blocks further away. {@link B2cBlockDragger} gives the strategy to every block it drags.
 */
export class B2cBlockDragStrategy extends Blockly.dragging.BlockDragStrategy {
  /** The canvas the block is dragged on, whose delete areas and zoom count. */
  private readonly canvas: Blockly.WorkspaceSvg;

  /** Whether the pointer of the latest move was over a delete area. */
  private overDeleteArea = false;

  /**
   * How much nearer another place must be for the preview to move there: a dragged value (a block
   * with an output) has its own.
   */
  private readonly preference: number;

  constructor(block: Blockly.BlockSvg) {
    super(block);
    this.canvas = block.workspace;
    // `BlockSvg` types its output as always there; a statement has none (`Block`'s type says so).
    const dragged: Blockly.Block = block;
    this.preference =
      dragged.outputConnection === null ? PREVIEW_PREFERENCE : VALUE_PREVIEW_PREFERENCE;
  }

  /**
   * Moves the block to `newLoc` and updates the preview. `e` is the pointer event of the move;
   * a move without one (a programmatic drag) is never over a delete area.
   *
   * Blockly reads the preference for the place already previewed from its global configuration,
   * in a private method that only this move calls (`BlockDragStrategy.currCandidateIsBetter`,
   * Blockly 12.5.1). So while the move runs, the configuration holds this drag's preference
   * ({@link PREVIEW_PREFERENCE} or {@link VALUE_PREVIEW_PREFERENCE}), and it gets its own value
   * back in a `finally`, however the move ends: nothing outside a move of this strategy (another
   * drag, the keyboard's moves, an Escape that cancels the gesture, its disposal) ever sees the
   * changed value.
   */
  override drag(newLoc: Blockly.utils.Coordinate, e?: PointerEvent): void {
    this.overDeleteArea = e !== undefined && isOverDeleteArea(this.canvas, e);
    const config = Blockly.config;
    const preference = config.currentConnectionPreference;
    config.currentConnectionPreference = this.preference;
    try {
      super.drag(newLoc);
    } finally {
      config.currentConnectionPreference = preference;
    }
  }

  /**
   * How far away a connection may be to be previewed: {@link snapRadius} at the canvas's zoom, or
   * none at all over a delete area. The radius is -1 rather than 0, because a search with radius 0
   * still finds a connection exactly where the dragged one is. Blockly asks with a place previewed
   * or not alike (its `connectingSnapRadius` and `snapRadius` are both 28).
   */
  protected override getSearchRadius(): number {
    return this.overDeleteArea ? -1 : snapRadius(this.canvas.scale);
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
   * A drop that wraps statements in an empty C-block (03 §3.3, the wrap its preview shows:
   * `previewedWrap`, read before Blockly ends the drag and hides the preview) is finished after
   * Blockly's drop: Blockly connects the C-block in the statements' place and hangs them below it,
   * and `completeWrap` moves them into its first mouth. A block dropped on the canvas is then
   * settled by the workspace's {@link BlockDropHandler} (`handleBlockDrops`). Both happen in the
   * drag's Blockly event group, which Blockly's dragger closes: the group is taken before Blockly
   * ends the drag, opened again for them, and closed after. A block taken from the toolbox was
   * created in the same group (Blockly's gesture starts it first), so one Undo removes the block,
   * its move, the wrap and what the handler changed.
   */
  override onDragEnd(e: PointerEvent, totalDelta?: Blockly.utils.Coordinate): void {
    const group = Blockly.Events.getGroup();
    try {
      const root =
        this.draggable instanceof Blockly.BlockSvg ? this.draggable.getRootBlock() : null;
      if (totalDelta !== undefined && root !== null && this.wouldDeleteDraggable(e, root)) {
        this.onDrag(e, totalDelta);
      }
      const wrap = root !== null && !this.shouldReturnToStart(e, root) ? previewedWrap(root) : null;
      super.onDragEnd(e);
      if (wrap !== null) {
        finishWrap(wrap, group);
      }
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
