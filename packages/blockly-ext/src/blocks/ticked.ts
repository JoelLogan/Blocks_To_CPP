/**
 * Fields a block shows only while one of its checkboxes is ticked (docs/plans/m3-contracts/w2.md
 * §1.9): `when program starts` names its arguments (`ARGV`) only *with arguments* (`ARGS`). The
 * field keeps its value while it is hidden, so unticking and ticking again brings the same name
 * back, and the project saves it either way.
 */
import * as Blockly from 'blockly/core';

/** For each block type, the fields shown only while a checkbox is ticked: field → checkbox. */
export const SHOWN_WHILE_TICKED: Readonly<Record<string, Readonly<Record<string, string>>>> = {
  'program.main': { ARGV: 'ARGS' },
};

/** Whether a checkbox value is ticked (Blockly stores `'TRUE'` and `'FALSE'`). */
function ticked(value: unknown): boolean {
  return value === true || value === 'TRUE';
}

/**
 * Shows each of `block`'s {@link SHOWN_WHILE_TICKED} fields as its checkbox says, now and whenever
 * the checkbox changes (by the user, an undo or a load).
 */
export function applyShownWhileTicked(block: Blockly.Block): void {
  const rules = SHOWN_WHILE_TICKED[block.type];
  if (rules === undefined) {
    return;
  }
  for (const [name, tick] of Object.entries(rules)) {
    const field = block.getField(name);
    const box = block.getField(tick);
    if (field === null || box === null) {
      continue;
    }
    // While the block is being built it is drawn afterwards; later changes redraw it.
    field.setVisible(ticked(box.getValue()));
    box.setValidator((value: unknown) => {
      if (field.isVisible() !== ticked(value)) {
        field.setVisible(ticked(value));
        if (block instanceof Blockly.BlockSvg) {
          void block.queueRender();
        }
      }
      return undefined;
    });
  }
}
