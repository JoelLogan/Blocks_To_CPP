/**
 * Block drags, as the editing session needs to hear of them. While a block is dragged the canvas is
 * in flux: Blockly has detached the block from where it was, and an insertion marker (the drag
 * preview) may stand in a statement list or head a loose stack. The session therefore reads the
 * canvas once just before a drag starts, and saving during the drag writes that (see
 * `EditorSession.currentDocument`).
 *
 * Blockly announces a drag only through events, which it delivers after the block has moved, so
 * the editor gives Blockly its own block dragger ({@link B2cBlockDragger}, the `blockDragger`
 * plugin at inject time): it reports a drag before Blockly's dragger starts it, and its end.
 */
import * as Blockly from 'blockly/core';

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

/** Calls a listener, so that a failing one never breaks Blockly's drag. */
function tell(run: () => void): void {
  try {
    run();
  } catch (error: unknown) {
    console.error('A block drag listener failed', error);
  }
}

/**
 * Blockly's dragger, reporting block drags to the workspace's {@link BlockDragListener}. Blockly
 * creates one per drag, in the workspace the block is dragged on (also for a block taken from the
 * toolbox, which Blockly has just created there).
 */
export class B2cBlockDragger extends Blockly.dragging.Dragger {
  override onDragStart(e: PointerEvent): void {
    const listener = LISTENERS.get(this.workspace);
    let block = this.draggable instanceof Blockly.BlockSvg ? this.draggable : null;
    // Dragging a shadow drags the block it belongs to.
    while (block?.isShadow() === true) {
      block = block.getParent();
    }
    if (listener !== undefined && block !== null) {
      const dragged = block;
      tell(() => {
        listener.onDragStart(dragged);
      });
    }
    super.onDragStart(e);
  }

  override onDragEnd(e: PointerEvent): void {
    try {
      super.onDragEnd(e);
    } finally {
      const listener = LISTENERS.get(this.workspace);
      if (listener !== undefined) {
        tell(() => {
          listener.onDragEnd();
        });
      }
    }
  }
}
