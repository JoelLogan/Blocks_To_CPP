/**
 * The undo step of a ⊕/⊖ click.
 *
 * Everything the click does happens in one Blockly event group, so undo and redo take it as one
 * step: first the moves of the blocks bumped out of removed parts, then one `mutation` change event
 * holding the block's `extra` before and after (as JSON text, as Blockly's own mutators record it).
 * Undo runs them backwards: the mutation is undone first, which brings the parts back, and then the
 * bumped blocks are connected to them again.
 */
import * as Blockly from 'blockly/core';

/** Whether a user action may change this block's parts now. */
export function canEdit(block: Blockly.Block): boolean {
  return (
    !block.isDeadOrDying() &&
    !block.isInFlyout &&
    !block.isInsertionMarker() &&
    block.isEditable() &&
    !block.workspace.isReadOnly()
  );
}

/**
 * Runs `change` as one undo step of `block`'s mutation. `extra` reads the block's state; the
 * mutation event is fired only when the state changed.
 */
export function asUndoStep(
  block: Blockly.Block,
  extra: () => Record<string, unknown>,
  change: () => void,
): void {
  const ownGroup = Blockly.Events.getGroup() === '';
  if (ownGroup) {
    Blockly.Events.setGroup(true);
  }
  try {
    const before = JSON.stringify(extra());
    change();
    const after = JSON.stringify(extra());
    if (before !== after && Blockly.Events.isEnabled()) {
      const BlockChange = Blockly.Events.get(Blockly.Events.BLOCK_CHANGE);
      Blockly.Events.fire(new BlockChange(block, 'mutation', null, before, after));
    }
  } finally {
    if (ownGroup) {
      Blockly.Events.setGroup(false);
    }
  }
}
