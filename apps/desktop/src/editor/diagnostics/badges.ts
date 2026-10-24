/**
 * Diagnostics on the blocks of the shown module (docs/spec/04-user-interface.md §4.4): which block
 * shows which diagnostics, and keeping the badges in step with the app's state.
 *
 * - A diagnostic shows on its block (the block's ID is the document's block ID).
 * - A block inside a collapsed block does not show; its diagnostics show on the outermost
 *   collapsed block around it instead, marked as coming from a block inside.
 * - A block kept as data inside a placeholder (an unknown block type) shows on the placeholder.
 * - Diagnostics without a block, of another module, or of a block that is not on the canvas
 *   (a build diagnostic whose block was deleted) show only in Problems.
 */
import type { Diagnostic } from '@blocks2cpp/ipc-types';
import {
  type BlockDiagnosticItem,
  isPlaceholder,
  isSeverity,
  readPlaceholder,
  setBlockDiagnostics,
} from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';

import type { EditorContext } from '../../app/editor-types';
import type { AppState } from '../../app/store';
import { visibleHolder } from '../highlight/reveal';
import { type DiagnosticInputs, diagnosticInputs } from './inputs';

/**
 * The most diagnostics put on blocks in one update; Problems still lists the rest. The pipeline
 * bounds its own diagnostics far below this.
 */
export const MAX_BADGED_DIAGNOSTICS = 10_000;

/** The blocks to badge and what each one shows. */
export type BadgePlan = Map<Blockly.Block, BlockDiagnosticItem[]>;

/** Finds the placeholder that keeps a block as data, building the lookup only when needed. */
class PlaceholderLookup {
  readonly #workspace: Blockly.Workspace;
  #holders: Map<string, Blockly.Block> | null = null;

  constructor(workspace: Blockly.Workspace) {
    this.#workspace = workspace;
  }

  find(blockId: string): Blockly.Block | null {
    this.#holders ??= this.#build();
    return this.#holders.get(blockId) ?? null;
  }

  #build(): Map<string, Blockly.Block> {
    const holders = new Map<string, Blockly.Block>();
    for (const block of this.#workspace.getAllBlocks(false)) {
      const data = isPlaceholder(block) ? readPlaceholder(block) : null;
      if (data === null) {
        continue;
      }
      // The kept node is document data: walk it with a work list, never by recursion.
      const pending: unknown[] = [data.node];
      const pushAll = (values: Iterable<unknown>): void => {
        for (const value of values) {
          pending.push(value);
        }
      };
      while (pending.length > 0) {
        const node = pending.pop();
        if (typeof node !== 'object' || node === null) {
          continue;
        }
        if (Array.isArray(node)) {
          pushAll(node as unknown[]);
          continue;
        }
        const record = node as Record<string, unknown>;
        const id = record['id'];
        if (typeof id === 'string' && id !== block.id && !holders.has(id)) {
          holders.set(id, block);
        }
        for (const key of ['inputs', 'statements'] as const) {
          const children = record[key];
          if (typeof children === 'object' && children !== null) {
            pushAll(Object.values(children as Record<string, unknown>));
          }
        }
        // The nested block of an input, and a loose stack's blocks.
        pending.push(record['block'], record['stack']);
      }
    }
    return holders;
  }
}

/**
 * Which blocks of `workspace` (showing module `moduleId`) show which diagnostics. The most
 * serious severity of each block wins on its badge (see `setBlockDiagnostics`).
 */
export function planBadges(
  inputs: DiagnosticInputs,
  workspace: Blockly.Workspace,
  moduleId: string,
): BadgePlan {
  const plan: BadgePlan = new Map();
  const placeholders = new PlaceholderLookup(workspace);
  let budget = MAX_BADGED_DIAGNOSTICS;

  const place = (diagnostic: Diagnostic, stale: boolean): void => {
    const { block: blockId, module, part } = diagnostic.primary;
    if (blockId === undefined || !isSeverity(diagnostic.severity)) {
      return;
    }
    if (module !== undefined && module !== moduleId) {
      return;
    }
    let block = workspace.getBlockById(blockId);
    let nested = false;
    if (block === null || block.isShadow()) {
      block = placeholders.find(blockId);
      nested = true;
    }
    if (block === null) {
      return;
    }
    const holder = visibleHolder(block);
    nested ||= holder !== block;
    const items = plan.get(holder) ?? [];
    items.push({
      code: diagnostic.code,
      severity: diagnostic.severity,
      message: diagnostic.message,
      primary: { part },
      stale,
      nested,
    });
    plan.set(holder, items);
  };

  for (const diagnostic of inputs.live) {
    if (budget-- <= 0) {
      break;
    }
    place(diagnostic, false);
  }
  for (const diagnostic of inputs.build) {
    if (budget-- <= 0) {
      break;
    }
    place(diagnostic, inputs.stale);
  }
  return plan;
}

/** Keeps the badges of a workspace matching a plan, touching only blocks that change. */
export class BadgeApplier {
  #badged = new Set<Blockly.Block>();

  /** Shows `plan`: blocks no longer in it lose their badges, the others get theirs. */
  apply(plan: BadgePlan): void {
    for (const block of this.#badged) {
      if (!plan.has(block)) {
        this.#set(block, []);
      }
    }
    this.#badged = new Set();
    for (const [block, items] of plan) {
      this.#set(block, items);
      this.#badged.add(block);
    }
  }

  /** Removes every badge this applier put on. */
  clear(): void {
    this.apply(new Map());
  }

  #set(block: Blockly.Block, items: readonly BlockDiagnosticItem[]): void {
    if (block.isDeadOrDying()) {
      return;
    }
    try {
      setBlockDiagnostics(block, items);
    } catch (error: unknown) {
      // A badge is display only: a block that cannot show one must not break the editor.
      console.warn('The diagnostics of a block could not be shown', error);
    }
  }
}

/** Whether a store change can change the badges. */
function badgesChanged(state: AppState, previous: AppState): boolean {
  return (
    state.analysis.preview !== previous.analysis.preview ||
    state.build.diagnostics !== previous.build.diagnostics ||
    state.build.diagnosticsHash !== previous.build.diagnosticsHash ||
    state.project?.contentHash !== previous.project?.contentHash ||
    state.project?.activeModuleId !== previous.project?.activeModuleId
  );
}

/**
 * Whether blocks may need their badges again after a workspace event: blocks were created (a
 * paste, an undo, a module loaded) or collapsed and expanded (the badges move to or from the
 * collapsed block). Other edits change the document, and the preview that follows updates the
 * badges.
 */
function needsReapply(event: Blockly.Events.Abstract): boolean {
  if (
    event instanceof Blockly.Events.BlockCreate ||
    event instanceof Blockly.Events.FinishedLoading
  ) {
    return true;
  }
  return event instanceof Blockly.Events.BlockChange && event.element === 'collapsed';
}

/**
 * Shows the app's diagnostics on the workspace's blocks and keeps them up to date: after each
 * preview, build and module switch, and when blocks are created, collapsed or expanded. Updates
 * are batched into one per task. Returns the function that removes every badge again.
 */
export function attachBlockDiagnostics(ctx: EditorContext): () => void {
  const applier = new BadgeApplier();
  let scheduled = false;
  let attached = true;

  const update = (): void => {
    const inputs = diagnosticInputs(ctx.store.getState());
    applier.apply(planBadges(inputs, ctx.workspace, ctx.activeModuleId()));
  };
  const schedule = (): void => {
    if (scheduled) {
      return;
    }
    scheduled = true;
    queueMicrotask(() => {
      scheduled = false;
      if (attached) {
        update();
      }
    });
  };

  const unsubscribe = ctx.store.subscribe((state, previous) => {
    if (badgesChanged(state, previous)) {
      schedule();
    }
  });
  const onEvent = (event: Blockly.Events.Abstract): void => {
    if (needsReapply(event)) {
      schedule();
    }
  };
  ctx.workspace.addChangeListener(onEvent);
  update();

  return () => {
    attached = false;
    unsubscribe();
    ctx.workspace.removeChangeListener(onEvent);
    applier.clear();
  };
}
