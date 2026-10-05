/**
 * Walking and clearing a Blockly workspace without deep recursion. Blockly's own
 * `getDescendants` and `dispose` recurse along `next` chains (a block's next block is its child),
 * so a statement list of several thousand blocks can overflow the JavaScript stack; these helpers
 * use explicit work lists instead.
 */
import * as Blockly from 'blockly/core';

/** `root` and every block nested in it or chained after it, parents before children. */
export function descendantsOf(root: Blockly.Block): Blockly.Block[] {
  const found: Blockly.Block[] = [];
  const pending: Blockly.Block[] = [root];
  for (let block = pending.pop(); block !== undefined; block = pending.pop()) {
    found.push(block);
    const children = block.getChildren(false);
    for (let index = children.length - 1; index >= 0; index -= 1) {
      const child = children[index];
      if (child !== undefined) {
        pending.push(child);
      }
    }
  }
  return found;
}

/** Every block of the workspace (shadows included), parents before children. */
export function allBlocks(workspace: Blockly.Workspace): Blockly.Block[] {
  const found: Blockly.Block[] = [];
  for (const top of workspace.getTopBlocks(false)) {
    found.push(...descendantsOf(top));
  }
  return found;
}

/**
 * Removes every block from the workspace with Blockly's events off. Blocks are disposed children
 * first, so Blockly's recursive `dispose` never goes deeper than a block's shadows.
 */
export function clearWorkspace(workspace: Blockly.Workspace): void {
  Blockly.Events.disable();
  try {
    const blocks = allBlocks(workspace).filter((block) => !block.isShadow());
    for (let index = blocks.length - 1; index >= 0; index -= 1) {
      const block = blocks[index];
      if (block !== undefined && !block.isDeadOrDying()) {
        block.dispose(false);
      }
    }
    // Anything else on the canvas (workspace comments, variables).
    workspace.clear();
  } finally {
    Blockly.Events.enable();
  }
}
