/**
 * Connects the gating service (./gate.ts) to the block menus (W4's gating, wired by W2's package C2
 * for its first gated options, docs/plans/m3-contracts/w2.md §1.9): a dropdown lists only the options
 * the editor's mode offers. *change*'s bitwise forms need *Show advanced blocks*. A block that
 * already holds a gated option keeps it and still shows it (blockly-ext's option filter).
 */
import { setOptionFilter } from '@blocks2cpp/blockly-ext';

import { type EditorMode, optionGate } from './gate';

/**
 * Installs the option gate for the editor's mode, read when a menu opens; returns the function that
 * removes it again.
 */
export function installOptionGate(mode: () => EditorMode): () => void {
  setOptionFilter((block, field, value) => optionGate(block.type, field, value, mode()).allowed);
  return () => {
    setOptionFilter(null);
  };
}
