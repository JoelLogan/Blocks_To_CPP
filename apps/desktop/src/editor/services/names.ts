/**
 * The names of symbols, for reference fields and read-only expressions (docs/spec/03-block-language.md
 * §3.6, M2 decision "References whose declaration is gone").
 *
 * The current names come from the document itself (every declaration field and parameter row of
 * every module, including disabled and loose blocks the analyser leaves out), so a rename shows on
 * every reference after the next sync. Names of declarations that were deleted are kept for the
 * session (*last-known names*); loading a document forgets them, so after a reload a reference
 * whose declaration is gone shows `missing (sym_…)`.
 */
import type { BdmDocument } from '@blocks2cpp/b2c-core-wasm';

import { forEachNode, nodeDecls } from '../sync/bdmTree';

/** The most last-known names kept; the memory is emptied when it is full. */
export const MAX_LAST_KNOWN_NAMES = 200_000;

/** Whether two name maps hold the same names for the same symbols. */
function sameNames(a: ReadonlyMap<string, string>, b: ReadonlyMap<string, string>): boolean {
  if (a.size !== b.size) {
    return false;
  }
  for (const [sym, name] of a) {
    if (b.get(sym) !== name) {
      return false;
    }
  }
  return true;
}

/** Current and last-known symbol names. */
export class SymbolNames {
  private current = new Map<string, string>();
  private readonly lastKnown = new Map<string, string>();

  /** Takes the names declared in `doc` as the current ones; true when any name changed. */
  update(doc: BdmDocument): boolean {
    const next = new Map<string, string>();
    for (const module of doc.modules) {
      forEachNode(module.workspace.blocks, (node) => {
        for (const decl of nodeDecls(node)) {
          if (!next.has(decl.sym)) {
            next.set(decl.sym, decl.name);
          }
        }
      });
    }
    const changed = !sameNames(this.current, next);
    this.current = next;
    if (this.lastKnown.size + next.size > MAX_LAST_KNOWN_NAMES) {
      this.lastKnown.clear();
    }
    for (const [sym, name] of next) {
      this.lastKnown.set(sym, name);
    }
    return changed;
  }

  /** Gives renamed symbols (old → new) the name of the old one until the next update. */
  alias(syms: ReadonlyMap<string, string>): void {
    for (const [old, renamed] of syms) {
      const name = this.nameOf(old);
      if (name !== null) {
        this.current.set(renamed, name);
      }
    }
  }

  /** Forgets every name (a document is loaded). */
  reset(): void {
    this.current = new Map();
    this.lastKnown.clear();
  }

  /** The current name of a symbol, its last-known name, or `null`. */
  nameOf(sym: string): string | null {
    return this.current.get(sym) ?? this.lastKnown.get(sym) ?? null;
  }
}
