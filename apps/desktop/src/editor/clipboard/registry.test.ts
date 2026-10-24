/**
 * The clipboard's keys and menu items in Blockly's global registries: Blockly's copy, cut, paste
 * and duplicate are replaced while an editor is attached and put back afterwards, `Ctrl+D`
 * duplicates, Blockly's own `Ctrl+Y` still redoes (04 §4.7), and every key or menu item goes to
 * the editor of its canvas.
 */
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { registerEditorBlocks } from '../services/install';
import type { PasteAnchor } from './anchor';
import {
  type ClipboardKeyTarget,
  installClipboardRegistries,
  MENU_ITEM_IDS,
  MENU_LABELS,
  SHORTCUT_NAMES,
} from './registry';

const shortcuts = Blockly.ShortcutRegistry.registry;
const menu = Blockly.ContextMenuRegistry.registry;
const { KeyCodes } = Blockly.utils;
const REDO = Blockly.ShortcutItems.names.REDO;

const workspaces: Blockly.WorkspaceSvg[] = [];
const uninstalls: (() => void)[] = [];

afterEach(() => {
  for (const uninstall of uninstalls.splice(0)) {
    uninstall();
  }
  for (const workspace of workspaces.splice(0)) {
    workspace.dispose();
  }
  document.body.replaceChildren();
});

/** A rendered workspace with a flyout holding one block. */
function workspaceWithFlyout(): Blockly.WorkspaceSvg {
  registerEditorBlocks();
  const host = document.createElement('div');
  document.body.append(host);
  const workspace = Blockly.inject(host, {
    renderer: 'zelos',
    sounds: false,
    toolbox: { kind: 'flyoutToolbox', contents: [{ kind: 'block', type: 'io.print' }] },
  });
  workspaces.push(workspace);
  return workspace;
}

/** The flyout's own workspace. */
function flyoutOf(workspace: Blockly.WorkspaceSvg): Blockly.WorkspaceSvg {
  const flyout = workspace.getFlyout()?.getWorkspace();
  if (flyout === undefined) {
    throw new Error('the workspace has no flyout');
  }
  return flyout;
}

type Target = ClipboardKeyTarget;

/** A key target whose every answer is "yes" unless changed, recording what it is asked to do. */
function fakeTarget(workspace: Blockly.WorkspaceSvg) {
  const anchor: PasteAnchor = { kind: 'canvas', near: null, at: null };
  const target = {
    workspace,
    canCopy: vi.fn<Target['canCopy']>(() => true),
    canCut: vi.fn<Target['canCut']>(() => true),
    canPaste: vi.fn<Target['canPaste']>(() => true),
    hasCopy: vi.fn<Target['hasCopy']>(() => true),
    canDuplicate: vi.fn<Target['canDuplicate']>(() => true),
    copyKey: vi.fn<Target['copyKey']>(() => true),
    cutKey: vi.fn<Target['cutKey']>(() => true),
    pasteKey: vi.fn<Target['pasteKey']>(() => true),
    duplicateKey: vi.fn<Target['duplicateKey']>(() => true),
    copyBlock: vi.fn<Target['copyBlock']>(),
    cutBlock: vi.fn<Target['cutBlock']>(),
    pasteAt: vi.fn<Target['pasteAt']>(),
    anchorFor: vi.fn<Target['anchorFor']>(() => anchor),
    duplicateBlock: vi.fn<Target['duplicateBlock']>(),
    canCutBlock: vi.fn<Target['canCutBlock']>(() => true),
    canCopyBlock: vi.fn<Target['canCopyBlock']>(() => true),
    canDuplicateBlock: vi.fn<Target['canDuplicateBlock']>(() => true),
  } satisfies ClipboardKeyTarget;
  return target;
}

function install(target: ClipboardKeyTarget): () => void {
  const uninstall = installClipboardRegistries(target);
  uninstalls.push(uninstall);
  return uninstall;
}

/** A Ctrl+letter key press. */
function ctrlKey(keyCode: number): KeyboardEvent {
  return new KeyboardEvent('keydown', { keyCode, ctrlKey: true, bubbles: true, cancelable: true });
}

function serialized(keyCode: number): string {
  return shortcuts.createSerializedKey(keyCode, [KeyCodes.CTRL_CMD]);
}

/** A menu action (an item that is not a separator). */
type ActionItem = Exclude<Blockly.ContextMenuRegistry.RegistryItem, { separator: true }>;

/** A menu item of the registry, which must be an action (not a separator). */
function actionItem(id: string): ActionItem {
  const item = menu.getItem(id);
  if (item === null || item.separator === true) {
    throw new Error(`no menu action ${id}`);
  }
  return item;
}

function precondition(id: string, scope: Blockly.ContextMenuRegistry.Scope): string {
  return actionItem(id).preconditionFn(scope, new Event('contextmenu'));
}

function choose(
  id: string,
  scope: Blockly.ContextMenuRegistry.Scope,
  location = new Blockly.utils.Coordinate(0, 0),
): void {
  actionItem(id).callback(scope, new Event('contextmenu'), new Event('click'), location);
}

describe('installing the registries', () => {
  it('replaces Blockly’s copy, cut and paste keys and its Duplicate item, then puts them back', () => {
    const before = shortcuts.getRegistry();
    const originals = [SHORTCUT_NAMES.copy, SHORTCUT_NAMES.cut, SHORTCUT_NAMES.paste].map(
      (name) => ({
        name,
        callback: before[name]?.callback,
        keys: shortcuts.getKeyCodesByShortcutName(name),
      }),
    );
    expect(originals.every((original) => original.callback !== undefined)).toBe(true);
    const duplicateItem = menu.getItem(MENU_ITEM_IDS.duplicate);
    expect(duplicateItem).not.toBeNull();

    const uninstall = install(fakeTarget(workspaceWithFlyout()));
    const during = shortcuts.getRegistry();
    for (const original of originals) {
      expect(during[original.name]?.callback).not.toBe(original.callback);
    }
    expect(shortcuts.getKeyCodesByShortcutName(SHORTCUT_NAMES.copy)).toEqual([
      serialized(KeyCodes.C),
    ]);
    expect(shortcuts.getShortcutNamesByKeyCode(serialized(KeyCodes.D))).toContain(
      SHORTCUT_NAMES.duplicate,
    );
    expect(menu.getItem(MENU_ITEM_IDS.duplicate)).not.toBe(duplicateItem);
    for (const id of [MENU_ITEM_IDS.cut, MENU_ITEM_IDS.copy, MENU_ITEM_IDS.pasteOnCanvas]) {
      expect(menu.getItem(id)).not.toBeNull();
    }

    uninstall();
    const after = shortcuts.getRegistry();
    for (const original of originals) {
      expect(after[original.name]?.callback).toBe(original.callback);
      expect(shortcuts.getKeyCodesByShortcutName(original.name)).toEqual(original.keys);
    }
    expect(after[SHORTCUT_NAMES.duplicate]).toBeUndefined();
    expect(menu.getItem(MENU_ITEM_IDS.duplicate)).toBe(duplicateItem);
    for (const id of [MENU_ITEM_IDS.cut, MENU_ITEM_IDS.copy, MENU_ITEM_IDS.pasteOnCanvas]) {
      expect(menu.getItem(id)).toBeNull();
    }
  });

  it('keeps the replacements until the last editor detaches', () => {
    const original = shortcuts.getRegistry()[SHORTCUT_NAMES.copy]?.callback;
    const first = install(fakeTarget(workspaceWithFlyout()));
    const second = install(fakeTarget(workspaceWithFlyout()));
    first();
    first();
    expect(shortcuts.getRegistry()[SHORTCUT_NAMES.copy]?.callback).not.toBe(original);
    second();
    expect(shortcuts.getRegistry()[SHORTCUT_NAMES.copy]?.callback).toBe(original);
  });
});

describe('keys', () => {
  it('go to the editor of the canvas they are pressed on', () => {
    const one = fakeTarget(workspaceWithFlyout());
    const two = fakeTarget(workspaceWithFlyout());
    install(one);
    install(two);
    const copy = ctrlKey(KeyCodes.C);
    expect(shortcuts.onKeyDown(two.workspace, copy)).toBe(true);
    expect(two.copyKey).toHaveBeenCalledOnce();
    expect(two.copyKey.mock.calls[0]?.[1]).toBe(copy);
    expect(one.copyKey).not.toHaveBeenCalled();

    expect(shortcuts.onKeyDown(one.workspace, ctrlKey(KeyCodes.X))).toBe(true);
    expect(one.cutKey).toHaveBeenCalledOnce();
    expect(shortcuts.onKeyDown(one.workspace, ctrlKey(KeyCodes.V))).toBe(true);
    expect(one.pasteKey).toHaveBeenCalledOnce();
    expect(shortcuts.onKeyDown(one.workspace, ctrlKey(KeyCodes.D))).toBe(true);
    expect(one.duplicateKey).toHaveBeenCalledOnce();
  });

  it('pressed in a flyout go to the editor of its canvas', () => {
    const target = fakeTarget(workspaceWithFlyout());
    install(target);
    expect(shortcuts.onKeyDown(flyoutOf(target.workspace), ctrlKey(KeyCodes.C))).toBe(true);
    expect(target.copyKey).toHaveBeenCalledOnce();
  });

  it('do nothing when the editor says no, during a drag, or on a canvas without an editor', () => {
    const target = fakeTarget(workspaceWithFlyout());
    install(target);
    target.canCopy.mockReturnValue(false);
    target.canPaste.mockReturnValue(false);
    expect(shortcuts.onKeyDown(target.workspace, ctrlKey(KeyCodes.C))).toBe(false);
    expect(shortcuts.onKeyDown(target.workspace, ctrlKey(KeyCodes.V))).toBe(false);
    expect(target.copyKey).not.toHaveBeenCalled();
    expect(target.pasteKey).not.toHaveBeenCalled();

    vi.spyOn(target.workspace, 'isDragging').mockReturnValue(true);
    expect(shortcuts.onKeyDown(target.workspace, ctrlKey(KeyCodes.D))).toBe(false);
    expect(target.duplicateKey).not.toHaveBeenCalled();

    const other = workspaceWithFlyout();
    expect(shortcuts.onKeyDown(other, ctrlKey(KeyCodes.X))).toBe(false);
    expect(target.cutKey).not.toHaveBeenCalled();
  });

  it('leave Blockly’s own Ctrl+Y redo in place', () => {
    const target = fakeTarget(workspaceWithFlyout());
    install(target);
    expect(shortcuts.getShortcutNamesByKeyCode(serialized(KeyCodes.Y))).toContain(REDO);
    const undo = vi.spyOn(target.workspace, 'undo');
    expect(shortcuts.onKeyDown(target.workspace, ctrlKey(KeyCodes.Y))).toBe(true);
    expect(undo).toHaveBeenCalledExactlyOnceWith(true);
  });
});

describe('menu items', () => {
  it('on a block offer Cut, Copy, Paste and Duplicate, sent to the block’s editor', () => {
    const target = fakeTarget(workspaceWithFlyout());
    install(target);
    const block = target.workspace.newBlock('io.print');
    const scope = { block };
    for (const id of [
      MENU_ITEM_IDS.cut,
      MENU_ITEM_IDS.copy,
      MENU_ITEM_IDS.pasteOnBlock,
      MENU_ITEM_IDS.duplicate,
    ]) {
      expect(precondition(id, scope)).toBe('enabled');
    }
    expect(actionItem(MENU_ITEM_IDS.cut).displayText).toBe(MENU_LABELS.cut);
    expect(actionItem(MENU_ITEM_IDS.duplicate).displayText).toBe(MENU_LABELS.duplicate);

    choose(MENU_ITEM_IDS.cut, scope);
    expect(target.cutBlock).toHaveBeenCalledExactlyOnceWith(block);
    choose(MENU_ITEM_IDS.copy, scope);
    expect(target.copyBlock).toHaveBeenCalledExactlyOnceWith(block);
    choose(MENU_ITEM_IDS.pasteOnBlock, scope);
    expect(target.anchorFor).toHaveBeenCalledExactlyOnceWith(block);
    expect(target.pasteAt).toHaveBeenCalledExactlyOnceWith({
      kind: 'canvas',
      near: null,
      at: null,
    });
    choose(MENU_ITEM_IDS.duplicate, scope);
    expect(target.duplicateBlock).toHaveBeenCalledExactlyOnceWith(block);
  });

  it('are disabled when the editor says no', () => {
    const target = fakeTarget(workspaceWithFlyout());
    install(target);
    const scope = { block: target.workspace.newBlock('io.print') };
    target.canCutBlock.mockReturnValue(false);
    target.canCopyBlock.mockReturnValue(false);
    target.canDuplicateBlock.mockReturnValue(false);
    target.hasCopy.mockReturnValue(false);
    for (const id of [
      MENU_ITEM_IDS.cut,
      MENU_ITEM_IDS.copy,
      MENU_ITEM_IDS.pasteOnBlock,
      MENU_ITEM_IDS.duplicate,
    ]) {
      expect(precondition(id, scope)).toBe('disabled');
    }
    expect(precondition(MENU_ITEM_IDS.pasteOnCanvas, { workspace: target.workspace })).toBe(
      'disabled',
    );
  });

  it('are hidden in a flyout and on a canvas without an editor', () => {
    const target = fakeTarget(workspaceWithFlyout());
    install(target);
    const flyout = flyoutOf(target.workspace);
    const flyoutBlock = flyout.getTopBlocks(false)[0];
    if (flyoutBlock === undefined) {
      throw new Error('the flyout shows no block');
    }
    const other = workspaceWithFlyout();
    const strayBlock = other.newBlock('io.print');
    for (const block of [flyoutBlock, strayBlock]) {
      expect(precondition(MENU_ITEM_IDS.copy, { block })).toBe('hidden');
      expect(precondition(MENU_ITEM_IDS.duplicate, { block })).toBe('hidden');
    }
    expect(precondition(MENU_ITEM_IDS.pasteOnCanvas, { workspace: flyout })).toBe('hidden');
    expect(precondition(MENU_ITEM_IDS.pasteOnCanvas, { workspace: other })).toBe('hidden');
    choose(MENU_ITEM_IDS.copy, { block: strayBlock });
    expect(target.copyBlock).not.toHaveBeenCalled();
  });

  it('on the canvas pastes the in-app copy where the menu was opened', () => {
    const target = fakeTarget(workspaceWithFlyout());
    install(target);
    expect(precondition(MENU_ITEM_IDS.pasteOnCanvas, { workspace: target.workspace })).toBe(
      'enabled',
    );
    choose(
      MENU_ITEM_IDS.pasteOnCanvas,
      { workspace: target.workspace },
      new Blockly.utils.Coordinate(120, 80),
    );
    expect(target.pasteAt).toHaveBeenCalledOnce();
    const anchor = target.pasteAt.mock.calls[0]?.[0];
    expect(anchor).toMatchObject({ kind: 'canvas', near: null });
    const at = anchor?.kind === 'canvas' ? anchor.at : undefined;
    // Without layout the point may not be measurable; when it is, it is a whole, finite point.
    expect(at === null || (Number.isInteger(at?.x) && Number.isInteger(at?.y))).toBe(true);
  });
});
