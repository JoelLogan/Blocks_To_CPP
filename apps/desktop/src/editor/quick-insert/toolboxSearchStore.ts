/**
 * Whether toolbox search is open, and on which canvas (docs/plans/m3-contracts/w4-editor-tools.md
 * §2.4): `Ctrl+F` and the canvas menu's *Search blocks…* open it; its field
 * (./ToolboxSearch.tsx) shows it.
 */
import type * as Blockly from 'blockly/core';

let open: Blockly.WorkspaceSvg | null = null;
/** How many times it was asked to open: each asks the field to take the focus. */
let requests = 0;
const listeners = new Set<() => void>();

function changed(): void {
  for (const listener of [...listeners]) {
    listener();
  }
}

/** Opens toolbox search on `workspace` (or moves the focus back into its field). */
export function openToolboxSearch(workspace: Blockly.WorkspaceSvg): void {
  open = workspace;
  requests += 1;
  changed();
}

/** Closes toolbox search. */
export function closeToolboxSearch(): void {
  if (open !== null) {
    open = null;
    changed();
  }
}

/** The canvas whose toolbox search is open, or `null`. */
export function toolboxSearchOpen(): Blockly.WorkspaceSvg | null {
  return open;
}

/** Calls `listener` whenever toolbox search opens or closes; returns the function that stops. */
export function subscribeToolboxSearch(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** How many times toolbox search was asked to open (the field takes the focus at each). */
export function toolboxSearchRequests(): number {
  return requests;
}
