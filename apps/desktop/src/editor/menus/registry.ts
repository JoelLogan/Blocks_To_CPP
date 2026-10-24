/**
 * Blockly's own block and canvas menu items, adjusted for Blocks2Cpp
 * (docs/spec/04-user-interface.md §4.1 "Menus"; owner decision D14):
 *
 * - *Inline/External Inputs* (`blockInline`) is removed: a block's input layout is never saved, so
 *   the change was lost on the next load.
 * - *Delete N Blocks* on the canvas (`workspaceDelete`) is replaced, keeping its ID and place: it
 *   counts the blocks it deletes (Blockly also counts every expression slot, which is a shadow
 *   block), always asks first with *Cancel* as the default, and deletes all or nothing in one
 *   event group, so one Undo brings every block back.
 * - The block menu's *Delete* (`blockDelete`) keeps Blockly's action but counts the same way.
 * - *Clean up blocks*, Undo, Redo, Collapse and Expand stay Blockly's own (Undo and Redo wait for
 *   Blockly's events: ../sync/undo.ts).
 *
 * The registry is global, while each editor has its own canvas: the changes are made when the
 * first editor attaches and Blockly's items are put back when the last one detaches. A menu goes
 * to the editor of the canvas it was opened on.
 */
import * as Blockly from 'blockly/core';

import {
  ChainTooLongError,
  disposeTree,
  isStackOverflow,
  longestChainFrom,
} from '../clipboard/chains';
import { descendantsOf } from '../sync/traverse';

/** The IDs of Blockly's items that the editor removes or replaces. */
export const MENU_ITEM_IDS = {
  /** *Inline Inputs* / *External Inputs*: removed. */
  inline: 'blockInline',
  /** The canvas's *Delete N blocks*: replaced. */
  deleteAll: 'workspaceDelete',
  /** The block's *Delete*: relabelled. */
  deleteBlock: 'blockDelete',
} as const;

/** What one editor does for the menu items. */
export interface MenuTarget {
  /** The canvas (the main workspace) the editor works on. */
  readonly workspace: Blockly.WorkspaceSvg;
  /** Asks whether to delete `count` blocks; resolves `true` to delete them. */
  confirmDeleteAll(count: number): Promise<boolean>;
  /**
   * Tells the user that *Delete N blocks* deleted nothing, because a chain of `longestChain`
   * statements is too long for the block editor to delete in one step.
   */
  tellChainTooLong(longestChain: number): void;
}

/** A menu action (an item that is not a separator). */
type ActionItem = Exclude<Blockly.ContextMenuRegistry.RegistryItem, { separator: true }>;

/** The label of an item that deletes `count` blocks. */
export function deleteLabel(count: number): string {
  return count === 1 ? 'Delete 1 block' : `Delete ${String(count)} blocks`;
}

/** How many blocks the user sees in `blocks` and what they hold (shadows and previews left out). */
export function countShown(blocks: readonly Blockly.Block[]): number {
  let count = 0;
  for (const root of blocks) {
    for (const block of descendantsOf(root)) {
      if (!block.isShadow() && !block.isInsertionMarker()) {
        count += 1;
      }
    }
  }
  return count;
}

/**
 * What *Delete N blocks* deletes on `workspace`: every top-level stack that can be deleted. A
 * block that cannot be deleted stays, with everything in it.
 */
export function deletableStacks(workspace: Blockly.WorkspaceSvg): Blockly.BlockSvg[] {
  return workspace
    .getTopBlocks(false)
    .filter((block) => !block.isInsertionMarker() && block.isDeletable());
}

/**
 * Deletes `stacks` (those still on the canvas) as one undo step.
 *
 * Blockly's delete event serialises a stack recursively along its statement chains, and its own
 * `dispose` recurses the same way, so a very long chain overflows the JavaScript stack
 * (../clipboard/chains.ts) and would leave the canvas half deleted. As the clipboard's cut does,
 * every event is therefore made before any block is removed, the blocks are removed with events
 * off and without recursion, and then the events are fired.
 *
 * @throws {@link ChainTooLongError} when an event cannot be made; nothing was deleted then.
 */
export function deleteStacks(stacks: readonly Blockly.BlockSvg[]): void {
  const doomed = stacks.filter((stack) => !stack.isDeadOrDying() && stack.isDeletable());
  Blockly.Events.setGroup(true);
  try {
    const events = Blockly.Events.isEnabled() ? doomed.map(deleteEvent) : [];
    for (const stack of doomed) {
      disposeTree(stack);
    }
    for (const event of events) {
      Blockly.Events.fire(event);
    }
  } finally {
    Blockly.Events.setGroup(false);
  }
}

/** Blockly's delete event for `stack`, which serialises it for undo. */
function deleteEvent(stack: Blockly.BlockSvg): Blockly.Events.BlockDelete {
  try {
    return new Blockly.Events.BlockDelete(stack);
  } catch (error: unknown) {
    throw isStackOverflow(error) ? new ChainTooLongError(longestChainFrom(stack), error) : error;
  }
}

const targets = new Map<Blockly.WorkspaceSvg, MenuTarget>();
let restoreBlockly: (() => void) | null = null;

/** The editor of the canvas a menu was opened on (never a flyout), or `null`. */
function targetOf(workspace: Blockly.WorkspaceSvg | undefined): MenuTarget | null {
  if (workspace === undefined || workspace.isFlyout) {
    return null;
  }
  return targets.get(workspace) ?? null;
}

/** The canvas's *Delete N blocks*. */
function deleteAllItem(original: ActionItem | null): ActionItem {
  return {
    id: MENU_ITEM_IDS.deleteAll,
    weight: original?.weight ?? 6,
    scopeType: Blockly.ContextMenuRegistry.ScopeType.WORKSPACE,
    displayText(scope) {
      return scope.workspace === undefined
        ? ''
        : deleteLabel(countShown(deletableStacks(scope.workspace)));
    },
    preconditionFn(scope) {
      if (targetOf(scope.workspace) === null || scope.workspace === undefined) {
        return 'hidden';
      }
      return countShown(deletableStacks(scope.workspace)) > 0 ? 'enabled' : 'disabled';
    },
    callback(scope) {
      const workspace = scope.workspace;
      const target = targetOf(workspace);
      if (workspace === undefined || target === null) {
        return;
      }
      workspace.cancelCurrentGesture();
      const stacks = deletableStacks(workspace);
      const count = countShown(stacks);
      if (count === 0) {
        return;
      }
      target
        .confirmDeleteAll(count)
        .then((confirmed) => {
          // The editor may have gone, or loaded another project, while the question was open.
          if (confirmed && targets.get(workspace) === target) {
            deleteStacks(stacks);
          }
        })
        .catch((error: unknown) => {
          if (error instanceof ChainTooLongError) {
            target.tellChainTooLong(error.longestChain);
          } else {
            console.error('The blocks could not be deleted', error);
          }
        });
    },
  };
}

/** The block menu's *Delete*, labelled with the number of blocks the user sees. */
function deleteBlockItem(original: ActionItem): ActionItem {
  return {
    ...original,
    displayText(scope) {
      const block = scope.block;
      if (block === undefined) {
        return deleteLabel(1);
      }
      // Blocks stacked below the block stay (as in Blockly's own count).
      const next = block.getNextBlock();
      const stays = next === null ? 0 : countShown([next]);
      return deleteLabel(countShown([block]) - stays);
    },
  };
}

/** The registry's action item `id`, or `null`. */
function actionItem(id: string): ActionItem | null {
  const item = Blockly.ContextMenuRegistry.registry.getItem(id);
  return item === null || item.separator === true ? null : item;
}

/** Makes the editor's changes to Blockly's menu items; returns the function that undoes them. */
function adjustBlocklyItems(): () => void {
  const menu = Blockly.ContextMenuRegistry.registry;
  const inline = menu.getItem(MENU_ITEM_IDS.inline);
  const deleteAll = actionItem(MENU_ITEM_IDS.deleteAll);
  const deleteBlock = actionItem(MENU_ITEM_IDS.deleteBlock);
  if (inline !== null) {
    menu.unregister(MENU_ITEM_IDS.inline);
  }
  if (deleteAll !== null) {
    menu.unregister(MENU_ITEM_IDS.deleteAll);
  }
  menu.register(deleteAllItem(deleteAll));
  if (deleteBlock !== null) {
    menu.unregister(MENU_ITEM_IDS.deleteBlock);
    menu.register(deleteBlockItem(deleteBlock));
  }

  return () => {
    for (const id of [MENU_ITEM_IDS.deleteAll, MENU_ITEM_IDS.deleteBlock]) {
      if (menu.getItem(id) !== null) {
        menu.unregister(id);
      }
    }
    for (const original of [inline, deleteAll, deleteBlock]) {
      if (original !== null && menu.getItem(original.id) === null) {
        menu.register(original);
      }
    }
  };
}

/**
 * Sends the menus of `target.workspace` to `target`, making the editor's changes to Blockly's
 * items on first use. Returns the function that stops it (and, for the last editor, puts Blockly's
 * items back).
 */
export function installMenuItems(target: MenuTarget): () => void {
  targets.set(target.workspace, target);
  restoreBlockly ??= adjustBlocklyItems();
  let installed = true;
  return () => {
    if (!installed) {
      return;
    }
    installed = false;
    if (targets.get(target.workspace) === target) {
      targets.delete(target.workspace);
    }
    if (targets.size === 0 && restoreBlockly !== null) {
      const restore = restoreBlockly;
      restoreBlockly = null;
      restore();
    }
  };
}
