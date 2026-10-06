/**
 * The clipboard's keys and menu items in Blockly's global registries (docs/spec/04-user-interface.md
 * §4.7): Blockly's own `copy`, `cut` and `paste` shortcuts and its *Duplicate* block menu item are
 * replaced, so that every copy and paste goes through the validated format; `Ctrl+D` duplicates;
 * the block menu gets *Cut*, *Copy* and *Paste*, and the canvas menu *Paste*. The workspace's
 * other keys of 04 §4.7 are Blockly's own and stay as they are: `Delete`, `Ctrl+Z` and `Ctrl+Y`
 * (Blockly 12 maps `Ctrl+Y` and `Ctrl+Shift+Z` to redo).
 *
 * The registries are global, while each editor has its own canvas: the replacements are installed
 * when the first editor attaches and Blockly's originals (with their key mappings at that time)
 * are put back when the last one detaches. A key press or menu goes to the editor of the canvas it
 * happened on (for a flyout, the canvas the flyout belongs to).
 */
import * as Blockly from 'blockly/core';

import type { PasteAnchor, WorkspacePoint } from './anchor';

/** What one editor does for the clipboard keys and menu items. */
export interface ClipboardKeyTarget {
  /** The canvas (the main workspace) the editor works on. */
  readonly workspace: Blockly.WorkspaceSvg;
  /** Whether `Ctrl+C` copies something with `focused` having the focus. */
  canCopy(focused: unknown): boolean;
  /** Whether `Ctrl+X` cuts something with `focused` having the focus. */
  canCut(focused: unknown): boolean;
  /** Whether a paste can go anywhere (a project is shown and the canvas is editable). */
  canPaste(): boolean;
  /** Whether the in-app copy holds blocks (the menus' *Paste* can only use it). */
  hasCopy(): boolean;
  /** Whether `Ctrl+D` duplicates something with `focused` having the focus. */
  canDuplicate(focused: unknown): boolean;
  /** `Ctrl+C`: returns whether the key was handled. */
  copyKey(focused: unknown, event: Event): boolean;
  /** `Ctrl+X`: returns whether the key was handled. */
  cutKey(focused: unknown, event: Event): boolean;
  /** `Ctrl+V`: returns whether the key was handled. */
  pasteKey(focused: unknown, event: Event): boolean;
  /** `Ctrl+D`: returns whether the key was handled. */
  duplicateKey(focused: unknown, event: Event): boolean;
  /** The block menu's *Copy*. */
  copyBlock(block: Blockly.BlockSvg): void;
  /** The block menu's *Cut*. */
  cutBlock(block: Blockly.BlockSvg): void;
  /** The menus' *Paste* (the in-app copy) at `anchor`. */
  pasteAt(anchor: PasteAnchor): void;
  /** Where the block menu's *Paste* goes for `block`. */
  anchorFor(block: Blockly.BlockSvg): PasteAnchor;
  /** The block menu's *Duplicate*. */
  duplicateBlock(block: Blockly.BlockSvg): void;
  /** Whether the block menu offers *Cut* for `block`. */
  canCutBlock(block: Blockly.BlockSvg): boolean;
  /** Whether the block menu offers *Copy* for `block`. */
  canCopyBlock(block: Blockly.BlockSvg): boolean;
  /** Whether the block menu offers *Duplicate* for `block`. */
  canDuplicateBlock(block: Blockly.BlockSvg): boolean;
}

/** The names of the replaced shortcuts (Blockly's own names) and of the new one. */
export const SHORTCUT_NAMES = {
  copy: Blockly.ShortcutItems.names.COPY,
  cut: Blockly.ShortcutItems.names.CUT,
  paste: Blockly.ShortcutItems.names.PASTE,
  duplicate: 'b2c_duplicate',
} as const;

/** The IDs of the menu items (the duplicate item keeps Blockly's ID and place). */
export const MENU_ITEM_IDS = {
  cut: 'b2c_clipboard_cut',
  copy: 'b2c_clipboard_copy',
  pasteOnBlock: 'b2c_clipboard_paste_block',
  duplicate: 'blockDuplicate',
  pasteOnCanvas: 'b2c_clipboard_paste',
} as const;

/** The menu items' labels. */
export const MENU_LABELS = {
  cut: 'Cut',
  copy: 'Copy',
  paste: 'Paste',
  duplicate: 'Duplicate',
} as const;

type MenuState = 'enabled' | 'disabled' | 'hidden';

const targets = new Map<Blockly.WorkspaceSvg, ClipboardKeyTarget>();
let restoreBlockly: (() => void) | null = null;

/** The editor of the canvas a key press or menu happened on, or `null`. */
function targetOf(workspace: Blockly.WorkspaceSvg | undefined): ClipboardKeyTarget | null {
  if (workspace === undefined) {
    return null;
  }
  const main = workspace.isFlyout ? workspace.targetWorkspace : workspace;
  return main === null ? null : (targets.get(main) ?? null);
}

/** Whether the canvas is free for a key: no drag, and no field editor or dropdown open. */
function keysAllowed(workspace: Blockly.WorkspaceSvg): boolean {
  const main = workspace.isFlyout ? workspace.targetWorkspace : workspace;
  return main !== null && !main.isDragging() && !Blockly.getFocusManager().ephemeralFocusTaken();
}

function key(code: number, ...modifiers: Blockly.utils.KeyCodes[]): string {
  return Blockly.ShortcutRegistry.registry.createSerializedKey(code, modifiers);
}

type KeyAction = 'copyKey' | 'cutKey' | 'pasteKey' | 'duplicateKey';

/** One of the editor's shortcuts. */
function shortcut(
  name: string,
  keyCode: number,
  action: KeyAction,
  allowed: (target: ClipboardKeyTarget, focused: unknown) => boolean,
): Blockly.ShortcutRegistry.KeyboardShortcut {
  return {
    name,
    keyCodes: [key(keyCode, Blockly.utils.KeyCodes.CTRL_CMD)],
    preconditionFn(workspace, scope) {
      const target = targetOf(workspace);
      return target !== null && keysAllowed(workspace) && allowed(target, scope.focusedNode);
    },
    callback(workspace, event, _shortcut, scope) {
      return targetOf(workspace)?.[action](scope.focusedNode, event) ?? false;
    },
  };
}

function editorShortcuts(): Blockly.ShortcutRegistry.KeyboardShortcut[] {
  const { KeyCodes } = Blockly.utils;
  return [
    shortcut(SHORTCUT_NAMES.copy, KeyCodes.C, 'copyKey', (t, f) => t.canCopy(f)),
    shortcut(SHORTCUT_NAMES.cut, KeyCodes.X, 'cutKey', (t, f) => t.canCut(f)),
    shortcut(SHORTCUT_NAMES.paste, KeyCodes.V, 'pasteKey', (t) => t.canPaste()),
    shortcut(SHORTCUT_NAMES.duplicate, KeyCodes.D, 'duplicateKey', (t, f) => t.canDuplicate(f)),
  ];
}

/** The block of a menu scope, if it is a canvas block with an editor. */
function menuBlock(scope: Blockly.ContextMenuRegistry.Scope): {
  block: Blockly.BlockSvg;
  target: ClipboardKeyTarget;
} | null {
  const block = scope.block;
  if (block === undefined || block.isInFlyout) {
    return null;
  }
  const target = targetOf(block.workspace);
  return target === null ? null : { block, target };
}

/** A block menu item. */
function blockItem(
  id: string,
  weight: number,
  label: string,
  state: (target: ClipboardKeyTarget, block: Blockly.BlockSvg) => MenuState,
  run: (target: ClipboardKeyTarget, block: Blockly.BlockSvg) => void,
): Blockly.ContextMenuRegistry.RegistryItem {
  return {
    id,
    weight,
    scopeType: Blockly.ContextMenuRegistry.ScopeType.BLOCK,
    displayText: label,
    preconditionFn(scope) {
      const found = menuBlock(scope);
      return found === null ? 'hidden' : state(found.target, found.block);
    },
    callback(scope) {
      const found = menuBlock(scope);
      if (found !== null) {
        run(found.target, found.block);
      }
    },
  };
}

/** The point a menu was opened at, in workspace coordinates, or `null`. */
function menuPoint(
  workspace: Blockly.WorkspaceSvg,
  location: Blockly.utils.Coordinate,
): WorkspacePoint | null {
  try {
    const point = Blockly.utils.svgMath.screenToWsCoordinates(workspace, location);
    return Number.isFinite(point.x) && Number.isFinite(point.y)
      ? { x: Math.round(point.x), y: Math.round(point.y) }
      : null;
  } catch {
    return null;
  }
}

function enabledIf(condition: boolean): MenuState {
  return condition ? 'enabled' : 'disabled';
}

function editorMenuItems(): Blockly.ContextMenuRegistry.RegistryItem[] {
  return [
    blockItem(
      MENU_ITEM_IDS.cut,
      -3,
      MENU_LABELS.cut,
      (target, block) => enabledIf(target.canCutBlock(block)),
      (target, block) => {
        target.cutBlock(block);
      },
    ),
    blockItem(
      MENU_ITEM_IDS.copy,
      -2,
      MENU_LABELS.copy,
      (target, block) => enabledIf(target.canCopyBlock(block)),
      (target, block) => {
        target.copyBlock(block);
      },
    ),
    blockItem(
      MENU_ITEM_IDS.pasteOnBlock,
      -1,
      MENU_LABELS.paste,
      (target) => enabledIf(target.canPaste() && target.hasCopy()),
      (target, block) => {
        target.pasteAt(target.anchorFor(block));
      },
    ),
    blockItem(
      MENU_ITEM_IDS.duplicate,
      1,
      MENU_LABELS.duplicate,
      (target, block) => (target.canDuplicateBlock(block) ? 'enabled' : 'disabled'),
      (target, block) => {
        target.duplicateBlock(block);
      },
    ),
    {
      id: MENU_ITEM_IDS.pasteOnCanvas,
      weight: -1,
      scopeType: Blockly.ContextMenuRegistry.ScopeType.WORKSPACE,
      displayText: MENU_LABELS.paste,
      preconditionFn(scope) {
        const target = targetOf(scope.workspace);
        if (target === null || scope.workspace?.isFlyout === true) {
          return 'hidden';
        }
        return enabledIf(target.canPaste() && target.hasCopy());
      },
      callback(scope, _open, _select, location) {
        const workspace = scope.workspace;
        const target = targetOf(workspace);
        if (workspace === undefined || target === null) {
          return;
        }
        const at = menuPoint(workspace, location);
        target.pasteAt({ kind: 'canvas', near: null, at });
      },
    },
  ];
}

/** Replaces Blockly's items with the editor's; returns the function that puts them back. */
function replaceBlocklyItems(): () => void {
  const shortcuts = Blockly.ShortcutRegistry.registry;
  const known = shortcuts.getRegistry();
  const saved: Blockly.ShortcutRegistry.KeyboardShortcut[] = [];
  const ours = editorShortcuts();
  for (const item of ours) {
    const original = known[item.name];
    if (original !== undefined) {
      saved.push({ ...original, keyCodes: shortcuts.getKeyCodesByShortcutName(item.name) });
      shortcuts.unregister(item.name);
    }
    shortcuts.register(item);
  }

  const menu = Blockly.ContextMenuRegistry.registry;
  const items = editorMenuItems();
  const replacedMenu: Blockly.ContextMenuRegistry.RegistryItem[] = [];
  for (const item of items) {
    const original = menu.getItem(item.id);
    if (original !== null) {
      replacedMenu.push(original);
      menu.unregister(item.id);
    }
    menu.register(item);
  }

  return () => {
    const registered = shortcuts.getRegistry();
    for (const item of ours) {
      if (registered[item.name] !== undefined) {
        shortcuts.unregister(item.name);
      }
    }
    for (const original of saved) {
      shortcuts.register(original, true);
    }
    for (const item of items) {
      if (menu.getItem(item.id) !== null) {
        menu.unregister(item.id);
      }
    }
    for (const original of replacedMenu) {
      menu.register(original);
    }
  };
}

/**
 * Sends the clipboard keys and menu items of `target.workspace` to `target`, installing the
 * editor's replacements on first use. Returns the function that stops it (and, for the last
 * editor, puts Blockly's own items back).
 */
export function installClipboardRegistries(target: ClipboardKeyTarget): () => void {
  targets.set(target.workspace, target);
  restoreBlockly ??= replaceBlocklyItems();
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
