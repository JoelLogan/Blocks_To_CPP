/**
 * Block state that maps to the project format (docs/spec/05-project-format.md §5.4, M2 decision
 * "Mapping Blockly 12 disabled state"): a project's `disabled: true` is exactly Blockly's
 * *manually disabled* reason. Blockly's other reasons (for example a block disabled because it is
 * orphaned, or by a plugin) are editor state and are never saved.
 */
import * as Blockly from 'blockly/core';

/** Whether the user disabled the block (the only disabled state a project stores). */
export function isManuallyDisabled(block: Blockly.Block): boolean {
  return block.hasDisabledReason(Blockly.constants.MANUALLY_DISABLED);
}

/** Sets or clears the user's disabled state, leaving every other disabled reason as it is. */
export function setManuallyDisabled(block: Blockly.Block, disabled: boolean): void {
  block.setDisabledReason(disabled, Blockly.constants.MANUALLY_DISABLED);
}
