/**
 * Whether Quick Insert is open, and for what (docs/plans/m3-contracts/w4-editor-tools.md §2.4): a
 * small store outside React, so that the editor's keys and menus open it and the popup
 * (./QuickInsert.tsx) shows it.
 */
import type * as Blockly from 'blockly/core';

import type { SearchEntry } from '../search';
import type { SymbolSource } from '../toolbox/scope';
import type { InsertTarget } from './insert';
import type { LineNames } from './parseLine';

/** An open Quick Insert. */
export interface QuickInsertSession {
  /** The canvas it inserts into. */
  readonly workspace: Blockly.WorkspaceSvg;
  /** Where it inserts (taken when it opened). */
  readonly target: InsertTarget;
  /** The blocks it searches. */
  readonly entries: readonly SearchEntry[];
  /** The variables a typed line may name. */
  readonly names: LineNames;
  /** The symbols of the latest analysis, to settle the inserted block. */
  readonly symbols: SymbolSource;
  /** Where it opens, in pixels from the canvas's top left, or `null` for the default place. */
  readonly anchor: { readonly x: number; readonly y: number } | null;
}

let session: QuickInsertSession | null = null;
const listeners = new Set<() => void>();

function changed(): void {
  for (const listener of [...listeners]) {
    listener();
  }
}

/** Opens Quick Insert (or opens it again for another place). */
export function openQuickInsert(next: QuickInsertSession): void {
  session = next;
  changed();
}

/** Closes Quick Insert. */
export function closeQuickInsert(): void {
  if (session !== null) {
    session = null;
    changed();
  }
}

/** The open Quick Insert, or `null`. */
export function quickInsertSession(): QuickInsertSession | null {
  return session;
}

/** Calls `listener` whenever Quick Insert opens or closes; returns the function that stops. */
export function subscribeQuickInsert(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
