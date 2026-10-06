/**
 * Keeping the keyboard focus on a live part of the block editor (docs/spec/04-user-interface.md
 * §4.8, WCAG 2.4.3) when what had it goes away.
 *
 * Blockly moves the focus off a focused block it removes, but its choice can be a block nested in
 * the removed one (a top-level block's own value slot), which is removed a moment later; and the
 * toolbox's flyout disposes every item in it each time it is shown again. Either way Blockly's
 * focus would be left on a node that is gone and the DOM focus on the document's body, where no
 * key of the editor works (Blockly listens for keys on the editor) until the user tabs back in.
 */
import * as Blockly from 'blockly/core';

/** The block a node is or belongs to (a field's or a connection's block), or `null`. */
export function blockOfNode(
  node: Blockly.IFocusableNode | null | undefined,
): Blockly.BlockSvg | null {
  if (node instanceof Blockly.BlockSvg) {
    return node;
  }
  if (node instanceof Blockly.Field || node instanceof Blockly.RenderedConnection) {
    const block = node.getSourceBlock();
    return block instanceof Blockly.BlockSvg ? block : null;
  }
  return null;
}

/** Whether `node` is gone: its block was removed, or its element is no longer in the document. */
export function isGone(node: Blockly.IFocusableNode): boolean {
  try {
    return blockOfNode(node)?.isDeadOrDying() === true || !node.getFocusableElement().isConnected;
  } catch {
    // A removed field has no block to answer for it any more.
    return true;
  }
}

/** The tree `node` belongs to, or `null` when it cannot say (it is gone). */
function treeOf(node: Blockly.IFocusableNode): Blockly.IFocusableTree | null {
  try {
    return node.getFocusableTree();
  } catch {
    return null;
  }
}

/**
 * Whether the DOM focus is free to be moved into `workspace`'s editor: it is on nothing, on the
 * body, on an element that is gone, or already inside the editor.
 */
function domFocusIsFree(workspace: Blockly.WorkspaceSvg): boolean {
  const doc = workspace.getInjectionDiv().ownerDocument;
  const active = doc.activeElement;
  return (
    active === null ||
    active === doc.body ||
    active === doc.documentElement ||
    !active.isConnected ||
    workspace.getInjectionDiv().contains(active)
  );
}

/**
 * Where on the canvas a deleted block was (its top-left corner, in workspace units), from its
 * delete event, or `null` when the event does not say.
 */
function deletedAt(
  event: Blockly.Events.BlockDelete,
  workspace: Blockly.WorkspaceSvg,
): Blockly.utils.Coordinate | null {
  const x = event.oldJson?.x;
  const y = event.oldJson?.y;
  if (typeof x !== 'number' || typeof y !== 'number') {
    return null;
  }
  // Blockly saves the x of a right-to-left canvas from its right edge.
  return new Blockly.utils.Coordinate(workspace.RTL ? workspace.getWidth() - x : x, y);
}

/** The block of `workspace` nearest `at` (not a value slot, and not one being removed), or `null`. */
function nearestBlock(
  workspace: Blockly.WorkspaceSvg,
  at: Blockly.utils.Coordinate,
): Blockly.BlockSvg | null {
  let nearest: Blockly.BlockSvg | null = null;
  let distance = Number.POSITIVE_INFINITY;
  for (const block of workspace.getAllBlocks(false)) {
    if (block.isShadow() || block.isDeadOrDying() || !block.canBeFocused()) {
      continue;
    }
    const d = Blockly.utils.Coordinate.distance(block.getRelativeToSurfaceXY(), at);
    if (d < distance) {
      nearest = block;
      distance = d;
    }
  }
  return nearest;
}

/**
 * After a block of `workspace` is deleted (by Delete, `Ctrl+X` or any other way): if Blockly left
 * the keyboard focus on a part of the canvas that is gone, and the DOM focus is not on something
 * else that is alive, puts it on the block nearest where the deleted block was, or else on the
 * canvas (which focuses its first block). A focus that is anywhere else is left alone.
 */
function refocusAfterDelete(
  workspace: Blockly.WorkspaceSvg,
  event: Blockly.Events.BlockDelete,
): void {
  const manager = Blockly.getFocusManager();
  const node = manager.getFocusedNode();
  if (node === null || !isGone(node) || !domFocusIsFree(workspace)) {
    return;
  }
  const owner = treeOf(node) ?? blockOfNode(node)?.workspace ?? null;
  if (owner !== workspace) {
    return;
  }
  const at = deletedAt(event, workspace);
  const nearest = at === null ? null : nearestBlock(workspace, at);
  if (nearest !== null) {
    manager.focusNode(nearest);
  } else {
    manager.focusTree(workspace);
  }
}

/**
 * Keeps the keyboard focus on the canvas of `workspace` when a deleted block took it away (see
 * {@link refocusAfterDelete}). Returns the function that stops it.
 */
export function keepCanvasFocus(workspace: Blockly.WorkspaceSvg): () => void {
  const onChange = (event: Blockly.Events.Abstract) => {
    if (!(event instanceof Blockly.Events.BlockDelete) || !workspace.rendered) {
      return;
    }
    try {
      refocusAfterDelete(workspace, event);
    } catch (error: unknown) {
      console.warn('The keyboard focus could not be put back on the canvas', error);
    }
  };
  workspace.addChangeListener(onChange);
  return () => {
    workspace.removeChangeListener(onChange);
  };
}

/** What identifies a flyout item across showings (its IDs are new each time), or `null`. */
function itemKey(element: Blockly.IFocusableNode): string | null {
  if (element instanceof Blockly.BlockSvg) {
    return `block:${element.type}:${element.toString()}`;
  }
  if (element instanceof Blockly.FlyoutButton) {
    return `${element.isLabel() ? 'label' : 'button'}:${element.getButtonText()}`;
  }
  return null;
}

/** The index in `items` of the flyout item `node` is or is part of, or `-1`. */
function itemIndexOf(
  items: readonly Blockly.FlyoutItem[],
  node: Blockly.IFocusableNode | null,
): number {
  if (node === null) {
    return -1;
  }
  const element = blockOfNode(node)?.getRootBlock() ?? node;
  return items.findIndex((item) => item.getElement() === element);
}

/**
 * The item of the new flyout contents that stands for the one at `index` before (with `key`):
 * the item with the same key nearest that position, else the focusable item at that position (or
 * the one nearest it), else `null`.
 */
function replacementFor(
  items: readonly Blockly.FlyoutItem[],
  index: number,
  key: string | null,
): Blockly.IFocusableNode | null {
  const focusable = items.map((item) => {
    const element = item.getElement();
    return element.canBeFocused() ? element : null;
  });
  let best: Blockly.IFocusableNode | null = null;
  let bestDistance = Number.POSITIVE_INFINITY;
  if (key !== null) {
    for (const [at, element] of focusable.entries()) {
      const distance = Math.abs(at - index);
      if (element !== null && distance < bestDistance && itemKey(element) === key) {
        best = element;
        bestDistance = distance;
      }
    }
  }
  if (best !== null) {
    return best;
  }
  // The same position (or the last one), else the nearest focusable item before it.
  const start = Math.min(index, focusable.length - 1);
  const after = focusable.slice(start).find((element) => element !== null);
  return (
    after ?? focusable.slice(0, Math.max(start, 0)).findLast((element) => element !== null) ?? null
  );
}

/**
 * Runs `rebuild`, which shows `flyout` again (disposing every item in it), and keeps the keyboard
 * where it was: if an item of the flyout had the focus, the focus goes to the same item in the new
 * contents (the one of the same kind and text nearest its old position), else to the item now at
 * that position, else to the flyout itself (its first item). Returns what `rebuild` returns.
 */
export function keepFlyoutFocus<T>(flyout: Blockly.IFlyout, rebuild: () => T): T {
  const manager = Blockly.getFocusManager();
  const flyoutWorkspace = flyout.getWorkspace();
  let index = -1;
  let key: string | null = null;
  try {
    if (manager.getFocusedTree() === flyoutWorkspace) {
      const items = flyout.getContents();
      index = itemIndexOf(items, manager.getFocusedNode());
      const element = items[index]?.getElement();
      key = element === undefined ? null : itemKey(element);
    }
  } catch (error: unknown) {
    console.warn('The keyboard focus in the toolbox could not be read', error);
  }
  const result = rebuild();
  if (index < 0) {
    return result;
  }
  try {
    const replacement = replacementFor(flyout.getContents(), index, key);
    if (replacement !== null) {
      manager.focusNode(replacement);
    } else {
      manager.focusTree(flyoutWorkspace);
    }
  } catch (error: unknown) {
    console.warn('The keyboard focus could not be put back in the toolbox', error);
  }
  return result;
}
