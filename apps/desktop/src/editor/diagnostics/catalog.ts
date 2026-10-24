/**
 * The block catalog that names blocks in Problems' block paths (./blockPath.ts).
 *
 * The catalog's data lives in `@blocks2cpp/blockly-ext`, which extends Blockly's classes when it
 * loads. The shell (the dock panels, app/panels.tsx) must not load Blockly at start-up, just as it
 * never loads the compiler core itself (app/core.ts), so the editor hands the catalog over: the
 * diagnostics plugin provides it when it attaches to the workspace, before any project is shown.
 * Until then block paths name blocks by their type.
 *
 * Only data and pure functions cross this seam; the catalog is the same for every workspace, so
 * it stays provided once given.
 */
import type { BlockDefJson } from '@blocks2cpp/blockly-ext';

/** What block paths need from the block catalog. */
export interface PathCatalog {
  /** The catalog definition of a block type, or `undefined` for a type the catalog lacks. */
  readonly block: (type: string) => BlockDefJson | undefined;
  /** How a `type` field shows a C++ type (`std::string` reads `string`). */
  readonly typeName: (type: string) => string;
}

let current: PathCatalog | null = null;
const listeners = new Set<() => void>();

/** Provides the catalog (or, with `null`, withdraws it) and tells the subscribers. */
export function providePathCatalog(catalog: PathCatalog | null): void {
  if (catalog === current) {
    return;
  }
  current = catalog;
  for (const listener of [...listeners]) {
    try {
      listener();
    } catch (error: unknown) {
      console.warn('A block catalog subscriber failed', error);
    }
  }
}

/** The provided catalog, or `null` before the editor provided one. */
export function pathCatalog(): PathCatalog | null {
  return current;
}

/**
 * Calls `listener` whenever the catalog is provided or withdrawn (for `useSyncExternalStore`).
 * Returns the function that unsubscribes.
 */
export function subscribePathCatalog(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
