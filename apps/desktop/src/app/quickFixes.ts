/**
 * The seam between the Problems panel and the editor's quick fixes
 * (docs/plans/m3-contracts/w4-editor-tools.md §2.5), as `editor-types.ts` is for the editor: the
 * quick-fix plugin publishes what finds a diagnostic's fixes while it is attached, so the dock
 * panels never load Blockly themselves.
 */
import type { Diagnostic } from '@blocks2cpp/ipc-types';

/** A quick fix as the panel offers it. */
export interface PanelQuickFix {
  /** What it does: `Create variable “score”`. */
  readonly title: string;
  /** Makes the change (one undo step); whether it did. */
  apply(): boolean;
}

/** Finds the fixes of a diagnostic on the canvas shown. */
export type QuickFixProvider = (diagnostic: Diagnostic) => readonly PanelQuickFix[];

let provider: QuickFixProvider | null = null;

/** Publishes the provider (or, with `null`, withdraws it when the editor detaches). */
export function setQuickFixProvider(next: QuickFixProvider | null): void {
  provider = next;
}

/** The fixes of `diagnostic` that apply now; none without an editor. */
export function quickFixesOf(diagnostic: Diagnostic): readonly PanelQuickFix[] {
  try {
    return provider?.(diagnostic) ?? [];
  } catch (error: unknown) {
    console.error('The quick fixes could not be found', error);
    return [];
  }
}
