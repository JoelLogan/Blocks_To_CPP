/**
 * Which canvas block the toolbox treats as selected (docs/spec/03-block-language.md §3.6,
 * 04 §4.2): the Variables category lists the symbols at it, the counted loop and *Make a variable*
 * name and place their declarations by it.
 *
 * In Blockly 12 the selection is the focused node, so it ends as soon as the focus moves into the
 * toolbox or its flyout: clicking a category, scrolling to a block or pressing *Make a variable*
 * would forget the block the user picked on the canvas. The {@link SelectionTracker} therefore
 * remembers the last block that had the focus on the canvas, and keeps it while the focus is
 * anywhere but the canvas itself (the toolbox, the flyout, a dialog, another panel). Focusing the
 * canvas without a block (an empty spot, a workspace comment) forgets it, and so does deleting the
 * block.
 */
import * as Blockly from 'blockly/core';

/** A focused node that belongs to a block: a field, an icon or a connection. */
interface BlockPart {
  getSourceBlock(): Blockly.Block | null;
}

/** Whether a focused node is part of a block (it names its block). */
function isBlockPart(node: unknown): node is BlockPart {
  return (
    typeof node === 'object' &&
    node !== null &&
    typeof (node as { getSourceBlock?: unknown }).getSourceBlock === 'function'
  );
}

/** A live block of the canvas itself (not of a flyout), or `null`. */
function liveCanvasBlock(
  workspace: Blockly.Workspace,
  block: Blockly.Block | null,
): Blockly.Block | null {
  if (block?.workspace !== workspace || block.isInFlyout) {
    return null;
  }
  if (block instanceof Blockly.BlockSvg && block.isDeadOrDying()) {
    return null;
  }
  return block.isDisposed() ? null : block;
}

/** Where the focus is, seen from one canvas. */
export type CanvasFocus =
  /** On a block of the canvas, or on a field, icon or connection of one. */
  | { readonly kind: 'block'; readonly block: Blockly.Block }
  /** On the canvas, but not on a block (its background, a workspace comment). */
  | { readonly kind: 'canvas' }
  /** Anywhere else: its toolbox or flyout, a dialog, another panel, nowhere. */
  | { readonly kind: 'elsewhere' };

/** Where the focus is, seen from `workspace`. Never throws. */
export function canvasFocus(workspace: Blockly.WorkspaceSvg): CanvasFocus {
  let tree: Blockly.IFocusableTree | null;
  let node: Blockly.IFocusableNode | null;
  try {
    const manager = Blockly.getFocusManager();
    tree = manager.getFocusedTree();
    node = manager.getFocusedNode();
  } catch (error: unknown) {
    console.warn('The focus could not be read', error);
    return { kind: 'elsewhere' };
  }
  if (tree !== workspace) {
    return { kind: 'elsewhere' };
  }
  const owner =
    node instanceof Blockly.Block ? node : isBlockPart(node) ? node.getSourceBlock() : null;
  const block = liveCanvasBlock(workspace, owner);
  return block === null ? { kind: 'canvas' } : { kind: 'block', block };
}

/**
 * The block that has the focus on `workspace` right now, or `null` (the focus is elsewhere, or on
 * the canvas but not on a block). Unlike {@link SelectionTracker.current}, it remembers nothing.
 */
export function selectedBlock(workspace: Blockly.WorkspaceSvg): Blockly.Block | null {
  const focus = canvasFocus(workspace);
  return focus.kind === 'block' ? focus.block : null;
}

/** Remembers the canvas selection while the focus is in the toolbox (see the module comment). */
export class SelectionTracker {
  /** The canvas whose selection is tracked. */
  private readonly workspace: Blockly.WorkspaceSvg;
  /** The ID of the block that last had the focus on the canvas, or `null`. */
  private rememberedId: string | null = null;

  constructor(workspace: Blockly.WorkspaceSvg) {
    this.workspace = workspace;
  }

  /** The selected block: focused now, or focused last before the focus left the canvas. */
  current(): Blockly.Block | null {
    const focus = canvasFocus(this.workspace);
    switch (focus.kind) {
      case 'block':
        this.rememberedId = focus.block.id;
        return focus.block;
      case 'canvas':
        this.rememberedId = null;
        return null;
      case 'elsewhere':
        return this.remembered();
    }
  }

  /**
   * Takes note of a selection event of the canvas. A block that gets selected is remembered at
   * once, so it is known even if the focus moves on before {@link current} is next asked.
   */
  noteSelected(event: Blockly.Events.Selected): void {
    const id = event.newElementId ?? null;
    if (id === null) {
      return;
    }
    if (liveCanvasBlock(this.workspace, this.workspace.getBlockById(id)) !== null) {
      this.rememberedId = id;
    }
  }

  /** Forgets the remembered block (for example when another document is loaded). */
  forget(): void {
    this.rememberedId = null;
  }

  /** The remembered block, if it is still on the canvas. */
  private remembered(): Blockly.Block | null {
    if (this.rememberedId === null) {
      return null;
    }
    const block = liveCanvasBlock(this.workspace, this.workspace.getBlockById(this.rememberedId));
    if (block === null) {
      this.rememberedId = null;
    }
    return block;
  }
}
