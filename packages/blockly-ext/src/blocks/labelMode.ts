/**
 * The label mode blocks are built in (docs/spec/03-block-language.md §3.5.2): the editor sets it
 * from its settings, and a block reads it when it is built (`init`, ./register.ts). Changing it
 * changes no block already built; the editor builds the shown blocks again
 * (docs/plans/m3-contracts/w4-editor-tools.md §2.1).
 */
import type { LabelMode } from './labels';

let mode: LabelMode = 'friendly';

/** Sets the label mode of the blocks built from now on. */
export function setBlockLabelMode(next: LabelMode): void {
  mode = next === 'cpp' ? 'cpp' : 'friendly';
}

/** The label mode blocks are built in: `friendly` until the editor sets another. */
export function blockLabelMode(): LabelMode {
  return mode;
}
