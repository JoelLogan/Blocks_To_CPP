/**
 * Two-way highlighting, code and Problems side (docs/spec/04-user-interface.md §4.3, §4.4): a click
 * in the C++ selects the block that produced it and scrolls it into view; activating a problem
 * selects its block and centres it. A block inside a collapsed block cannot be seen, so the
 * outermost collapsed block around it is selected instead.
 *
 * The dock panels call these, so this module uses Blockly's types only and never loads Blockly
 * itself (see ../diagnostics/catalog.ts): it works through the editor handle's workspace.
 */
import type * as Blockly from 'blockly/core';

import type { EditorHandle } from '../../app/editor-types';

/** The padding around a block scrolled into view, in workspace units. */
const SCROLL_PADDING = 24;

/**
 * The block that shows `block`: the block itself or, when it is inside collapsed blocks, the
 * outermost collapsed one around it.
 */
export function visibleHolder<B extends Blockly.Block>(block: B): B {
  let holder = block;
  // `getSurroundParent` skips the blocks above in the same stack: only nesting hides a block.
  for (
    let parent = block.getSurroundParent();
    parent !== null;
    parent = parent.getSurroundParent()
  ) {
    if (parent.isCollapsed()) {
      holder = parent;
    }
  }
  return holder;
}

/**
 * The block to select for `blockId`: the block itself or, when it is inside collapsed blocks, the
 * outermost collapsed one around it (the one that shows). Null when the workspace does not have it.
 */
export function revealTarget(
  workspace: Blockly.WorkspaceSvg,
  blockId: string,
): Blockly.BlockSvg | null {
  const block = workspace.getBlockById(blockId);
  return block === null ? null : visibleHolder(block);
}

/** Scrolls a block into view (Blockly scrolls only when it is not fully visible already). */
function scrollIntoView(workspace: Blockly.WorkspaceSvg, block: Blockly.BlockSvg): void {
  try {
    workspace.scrollBoundsIntoView(block.getBoundingRectangleWithoutChildren(), SCROLL_PADDING);
  } catch (error: unknown) {
    console.warn('The block could not be scrolled into view', error);
  }
}

/**
 * Selects the block of a click in the code panel and scrolls it into view: the innermost block at
 * the clicked position (the code panel finds it), or the collapsed block around it. A block the
 * canvas does not show (another module, or inside a placeholder) is left to the editor to find.
 */
export function selectBlockFromCode(editor: EditorHandle | null, blockId: string): void {
  if (editor === null) {
    return;
  }
  const target = revealTarget(editor.workspace, blockId);
  if (target === null) {
    editor.selectBlock(blockId, { center: true });
    return;
  }
  editor.selectBlock(target.id);
  scrollIntoView(editor.workspace, target);
}

/** Selects and centres the block of an activated problem (or the collapsed block around it). */
export function selectBlockFromProblem(editor: EditorHandle | null, blockId: string): void {
  if (editor === null) {
    return;
  }
  const target = revealTarget(editor.workspace, blockId);
  editor.selectBlock(target?.id ?? blockId, { center: true });
}
