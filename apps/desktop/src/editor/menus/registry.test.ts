/**
 * Blockly's block and canvas menu items as the editor offers them (04 §4.1 "Menus", D14): no
 * *Inline Inputs*, a *Delete N blocks* that counts the blocks the user sees, asks first (Cancel
 * by default) and is undone with one Ctrl+Z, and Blockly's items put back when the last editor
 * detaches. The guessing game has 11 blocks and 11 expression slots (shadow blocks).
 */
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { AlertOptions, ConfirmOptions } from '../../app/dialogs/service';
import { type ClipboardKeyTarget, installClipboardRegistries } from '../clipboard/registry';
import { editorInjectOptions } from '../EditorWorkspace';
import {
  disposeWorkspaces,
  EXAMPLE_PROJECTS,
  loadText,
  present,
  renderedWorkspace,
  startSession,
  testCore,
  type TestSession,
} from '../sync/testing';
import { allBlocks } from '../sync/traverse';
import { chainTooLongNotice, createMenusPlugin, deleteAllQuestion } from './plugin';
import {
  countShown,
  deletableStacks,
  deleteLabel,
  installMenuItems,
  MENU_ITEM_IDS,
  type MenuTarget,
} from './registry';

const core = await testCore();
const menu = Blockly.ContextMenuRegistry.registry;

let running: TestSession | null = null;
const uninstalls: (() => void)[] = [];

afterEach(() => {
  for (const uninstall of uninstalls.splice(0)) {
    uninstall();
  }
  running?.dispose();
  running = null;
  disposeWorkspaces();
});

/** A menu action (an item that is not a separator). */
type ActionItem = Exclude<Blockly.ContextMenuRegistry.RegistryItem, { separator: true }>;

function actionItem(id: string): ActionItem {
  const item = menu.getItem(id);
  if (item === null || item.separator === true) {
    throw new Error(`no menu action ${id}`);
  }
  return item;
}

/** A menu text, which Blockly allows to be an element. */
function textOf(text: string | HTMLElement): string {
  return typeof text === 'string' ? text : text.textContent;
}

function label(id: string, scope: Blockly.ContextMenuRegistry.Scope): string {
  const text = actionItem(id).displayText;
  return textOf(typeof text === 'function' ? text(scope) : text);
}

function precondition(id: string, scope: Blockly.ContextMenuRegistry.Scope): string {
  return actionItem(id).preconditionFn(scope, new Event('contextmenu'));
}

function choose(id: string, scope: Blockly.ContextMenuRegistry.Scope): void {
  actionItem(id).callback(
    scope,
    new Event('contextmenu'),
    new Event('click'),
    new Blockly.utils.Coordinate(0, 0),
  );
}

/** The labels of the menu Blockly would show for `scope`, in order. */
function offered(scope: Blockly.ContextMenuRegistry.Scope): string[] {
  return menu
    .getContextMenuOptions(scope, new Event('contextmenu'))
    .flatMap((option) => (option.separator === true ? [] : [textOf(option.text)]));
}

/** Attaches the menus to `workspace`, answering *Delete N blocks* with `answer`. */
function attach(workspace: Blockly.WorkspaceSvg, answer = true) {
  const confirm = vi.fn<(options: ConfirmOptions) => Promise<boolean>>(() =>
    Promise.resolve(answer),
  );
  const alert = vi.fn<(options: AlertOptions) => Promise<void>>(() => Promise.resolve());
  const plugin = createMenusPlugin({ dialogs: { confirm, alert } });
  const detach = plugin.attach({
    workspace,
    store: undefined as never,
    core: () => null,
    selectBlock: () => undefined,
    activeModuleId: () => 'mod_main',
  });
  uninstalls.push(detach);
  return { confirm, alert, detach };
}

/** A clipboard target that allows everything, so the clipboard's menu items show. */
function clipboardTarget(workspace: Blockly.WorkspaceSvg): ClipboardKeyTarget {
  const yes = () => true;
  const nothing = () => undefined;
  return {
    workspace,
    canCopy: yes,
    canCut: yes,
    canPaste: yes,
    hasCopy: yes,
    canDuplicate: yes,
    copyKey: yes,
    cutKey: yes,
    pasteKey: yes,
    duplicateKey: yes,
    copyBlock: nothing,
    cutBlock: nothing,
    pasteAt: nothing,
    anchorFor: () => ({ kind: 'canvas', near: null, at: null }),
    duplicateBlock: nothing,
    canCutBlock: yes,
    canCopyBlock: yes,
    canDuplicateBlock: yes,
  };
}

/** The guessing game on a canvas injected as the editor injects it. */
async function guessingGame(): Promise<Blockly.WorkspaceSvg> {
  const wasm = present(core, 'core');
  const workspace = renderedWorkspace(editorInjectOptions(Blockly.Themes.Zelos));
  const doc = loadText(wasm, present(EXAMPLE_PROJECTS['guessing_game.b2c'], 'guessing game'));
  running = startSession(wasm, doc, { workspace });
  await running.session.flush();
  await Blockly.renderManagement.finishQueuedRenders();
  return workspace;
}

/** Waits until Blockly has delivered (and recorded for undo) the events fired so far. */
function eventsDelivered(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => {
      setTimeout(() => {
        setTimeout(resolve, 0);
      }, 0);
    });
  });
}

/** The non-shadow blocks on `workspace`. */
function shownBlocks(workspace: Blockly.Workspace): number {
  return allBlocks(workspace).filter((block) => !block.isShadow()).length;
}

describe('installing the menu items', () => {
  it('removes Inline Inputs and replaces Delete N blocks, then puts Blockly’s back', () => {
    const inline = menu.getItem(MENU_ITEM_IDS.inline);
    const deleteAll = menu.getItem(MENU_ITEM_IDS.deleteAll);
    const deleteBlock = menu.getItem(MENU_ITEM_IDS.deleteBlock);
    const clean = menu.getItem('cleanWorkspace');
    expect([inline, deleteAll, deleteBlock, clean]).not.toContain(null);

    const workspace = renderedWorkspace();
    const { detach } = attach(workspace);
    expect(menu.getItem(MENU_ITEM_IDS.inline)).toBeNull();
    expect(menu.getItem(MENU_ITEM_IDS.deleteAll)).not.toBe(deleteAll);
    expect(menu.getItem(MENU_ITEM_IDS.deleteAll)?.weight).toBe(deleteAll?.weight);
    expect(menu.getItem(MENU_ITEM_IDS.deleteBlock)).not.toBe(deleteBlock);
    // Clean up blocks stays Blockly's own (D14).
    expect(menu.getItem('cleanWorkspace')).toBe(clean);

    detach();
    expect(menu.getItem(MENU_ITEM_IDS.inline)).toBe(inline);
    expect(menu.getItem(MENU_ITEM_IDS.deleteAll)).toBe(deleteAll);
    expect(menu.getItem(MENU_ITEM_IDS.deleteBlock)).toBe(deleteBlock);
    detach();
    expect(menu.getItem(MENU_ITEM_IDS.inline)).toBe(inline);
  });

  it('keeps the changes until the last editor detaches', () => {
    const inline = menu.getItem(MENU_ITEM_IDS.inline);
    const first = attach(renderedWorkspace());
    const second = attach(renderedWorkspace());
    first.detach();
    expect(menu.getItem(MENU_ITEM_IDS.inline)).toBeNull();
    second.detach();
    expect(menu.getItem(MENU_ITEM_IDS.inline)).toBe(inline);
  });

  it('hides Delete N blocks on a canvas without an editor and in flyouts', () => {
    const workspace = renderedWorkspace({
      toolbox: { kind: 'flyoutToolbox', contents: [{ kind: 'block', type: 'io.print' }] },
    });
    const other = renderedWorkspace();
    const target: MenuTarget = {
      workspace,
      confirmDeleteAll: () => Promise.resolve(false),
      tellChainTooLong: () => undefined,
    };
    uninstalls.push(installMenuItems(target));
    Blockly.serialization.blocks.append({ type: 'control.forever' }, other);
    const flyout = present(workspace.getFlyout()?.getWorkspace(), 'flyout');

    expect(precondition(MENU_ITEM_IDS.deleteAll, { workspace: other })).toBe('hidden');
    expect(precondition(MENU_ITEM_IDS.deleteAll, { workspace: flyout })).toBe('hidden');
    expect(precondition(MENU_ITEM_IDS.deleteAll, {})).toBe('hidden');
    // An empty canvas: nothing to delete.
    expect(precondition(MENU_ITEM_IDS.deleteAll, { workspace })).toBe('disabled');
  });
});

describe('the labels', () => {
  it('count blocks in the singular and the plural', () => {
    expect(deleteLabel(1)).toBe('Delete 1 block');
    expect(deleteLabel(11)).toBe('Delete 11 blocks');
  });

  it('ask before deleting, destructively, with Cancel as the default', () => {
    expect(deleteAllQuestion(11)).toMatchObject({
      title: 'Delete all 11 blocks?',
      message: 'Undo (Ctrl+Z) brings them back.',
      confirmLabel: 'Delete 11 blocks',
      cancelLabel: 'Cancel',
      destructive: true,
    });
    expect(deleteAllQuestion(1)).toMatchObject({
      title: 'Delete the block on the canvas?',
      message: 'Undo (Ctrl+Z) brings it back.',
      confirmLabel: 'Delete 1 block',
    });
  });

  it('say why nothing was deleted when a chain is too long', () => {
    expect(chainTooLongNotice(12_000)).toMatchObject({
      title: 'The blocks were not deleted',
      message: expect.stringContaining('A chain of 12000 statements') as unknown,
    });
  });
});

describe.skipIf(core === null)('in the guessing game', () => {
  it('Delete N blocks counts the blocks the user sees, not the expression slots', async () => {
    const workspace = await guessingGame();
    attach(workspace);
    expect(allBlocks(workspace)).toHaveLength(22);

    expect(label(MENU_ITEM_IDS.deleteAll, { workspace })).toBe('Delete 11 blocks');
    expect(precondition(MENU_ITEM_IDS.deleteAll, { workspace })).toBe('enabled');
    expect(countShown(deletableStacks(workspace))).toBe(11);
  });

  it('a block’s Delete counts what it holds, but not the blocks below it', async () => {
    const workspace = await guessingGame();
    attach(workspace);
    const block = (id: string) => present(workspace.getBlockById(id), id);

    // print with its text slot; the while loop below it stays.
    expect(label(MENU_ITEM_IDS.deleteBlock, { block: block('b004') })).toBe('Delete 1 block');
    // The while loop with the five blocks inside it.
    expect(label(MENU_ITEM_IDS.deleteBlock, { block: block('b010') })).toBe('Delete 6 blocks');
    expect(label(MENU_ITEM_IDS.deleteBlock, { block: block('b011') })).toBe('Delete 11 blocks');
  });

  it('deletes every block after asking, and one Ctrl+Z brings them all back', async () => {
    const workspace = await guessingGame();
    // Two loose stacks besides the program: three stacks to delete.
    for (const [index, id] of ['loose1', 'loose2'].entries()) {
      Blockly.serialization.blocks.append(
        { type: 'control.forever', id, x: 600, y: 100 + 200 * index },
        workspace,
      );
    }
    const { confirm } = attach(workspace, true);
    await eventsDelivered();

    choose(MENU_ITEM_IDS.deleteAll, { workspace });
    expect(confirm).toHaveBeenCalledTimes(1);
    expect(confirm.mock.calls[0]?.[0]).toMatchObject({
      confirmLabel: 'Delete 13 blocks',
      cancelLabel: 'Cancel',
      destructive: true,
    });
    await vi.waitFor(() => {
      expect(allBlocks(workspace)).toHaveLength(0);
    });
    await eventsDelivered();

    workspace.undo(false);
    expect(shownBlocks(workspace)).toBe(13);
    expect(allBlocks(workspace)).toHaveLength(24);
    expect(
      workspace
        .getTopBlocks(false)
        .map((block) => block.id)
        .sort(),
    ).toEqual(['b011', 'loose1', 'loose2']);
  });

  it('deletes nothing, and says so, when a chain is too long to delete in one step', async () => {
    const workspace = await guessingGame();
    const { alert } = attach(workspace, true);
    // What V8 and JavaScriptCore throw when Blockly's recursive serialisation runs out of stack.
    vi.spyOn(Blockly.Events, 'BlockDelete').mockImplementation(function overflow() {
      throw new RangeError('Maximum call stack size exceeded');
    });

    choose(MENU_ITEM_IDS.deleteAll, { workspace });
    await vi.waitFor(() => {
      expect(alert).toHaveBeenCalledTimes(1);
    });
    // main's body is the longest chain: four statements.
    expect(alert.mock.calls[0]?.[0]).toMatchObject(chainTooLongNotice(4));
    expect(shownBlocks(workspace)).toBe(11);
  });

  it('deletes nothing when the question is cancelled', async () => {
    const workspace = await guessingGame();
    const { confirm } = attach(workspace, false);

    choose(MENU_ITEM_IDS.deleteAll, { workspace });
    await vi.waitFor(() => {
      expect(confirm).toHaveBeenCalledTimes(1);
    });
    await Promise.resolve();
    expect(shownBlocks(workspace)).toBe(11);
  });

  it('deletes nothing when the editor detached while the question was open', async () => {
    const workspace = await guessingGame();
    let answer: (value: boolean) => void = () => undefined;
    const target: MenuTarget = {
      workspace,
      confirmDeleteAll: () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
      tellChainTooLong: () => undefined,
    };
    const detach = installMenuItems(target);
    uninstalls.push(detach);

    choose(MENU_ITEM_IDS.deleteAll, { workspace });
    detach();
    answer(true);
    await Promise.resolve();
    await Promise.resolve();
    expect(shownBlocks(workspace)).toBe(11);
  });

  it('offers the menus of 04 §4.1 on a block and on the canvas', async () => {
    const workspace = await guessingGame();
    uninstalls.push(installClipboardRegistries(clipboardTarget(workspace)));
    attach(workspace);
    const print = present(workspace.getBlockById('b004'), 'b004');

    expect(offered({ block: print, focusedNode: print })).toEqual([
      'Cut',
      'Copy',
      'Paste',
      'Duplicate',
      'Add Comment',
      'Collapse Block',
      'Disable Block',
      'Delete 1 block',
    ]);
    expect(offered({ workspace, focusedNode: workspace })).toEqual([
      'Paste',
      'Undo',
      'Redo',
      'Clean up Blocks',
      'Collapse Blocks',
      'Expand Blocks',
      'Delete 11 blocks',
    ]);
  });
});
