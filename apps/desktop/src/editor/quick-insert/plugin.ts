/**
 * The Quick Insert editor plugin (docs/spec/03-block-language.md §3.14,
 * docs/plans/m3-contracts/w4-editor-tools.md §2.4): `Ctrl+Space` on the canvas, or *Insert a
 * block…* in the canvas's and a block's menu, opens Quick Insert for the place the keyboard cursor
 * or the selection is at; `Ctrl+F` on the canvas, or *Search blocks…* in its menu, opens toolbox
 * search. It reads the toolbox's blocks and symbols
 * (../toolbox/plugin.ts, `toolboxContentsContext`), so it offers what the toolbox offers.
 *
 * Blockly's shortcut and menu registries are global: the items are registered while at least one
 * editor is attached, and an item acts on the canvas it was used on.
 */
import * as Blockly from 'blockly/core';

import type { EditorContext, EditorPlugin } from '../../app/editor-types';
import { searchEntries } from '../search';
import { toolboxContentsContext } from '../toolbox/plugin';
import { currentTarget } from './insert';
import { lineNames } from './parseLine';
import { closeQuickInsert, openQuickInsert, quickInsertSession } from './store';
import { openToolboxSearch } from './toolboxSearchStore';

/** The names of the shortcuts and menu items. */
export const QUICK_INSERT_IDS = {
  shortcut: 'b2c_quick_insert',
  searchShortcut: 'b2c_toolbox_search',
  canvasMenu: 'b2c_quick_insert_canvas',
  blockMenu: 'b2c_quick_insert_block',
  searchMenu: 'b2c_toolbox_search_canvas',
} as const;

/** The text of the menu items. */
export const QUICK_INSERT_MENU_TEXT = 'Insert a block…';
/** The text of the canvas menu's item that opens toolbox search. */
export const TOOLBOX_SEARCH_MENU_TEXT = 'Search blocks…';

/** The canvases with the plugin attached. */
const attached = new Set<Blockly.WorkspaceSvg>();

/** Whether Quick Insert may open on `workspace` now: no drag, no field editor or menu open. */
function mayOpen(workspace: Blockly.WorkspaceSvg): boolean {
  return (
    attached.has(workspace) &&
    !workspace.isReadOnly() &&
    !workspace.isDragging() &&
    !Blockly.getFocusManager().ephemeralFocusTaken()
  );
}

/** The canvas a key was pressed on (its own, or its flyout's target). */
function mainOf(workspace: Blockly.WorkspaceSvg | undefined): Blockly.WorkspaceSvg | null {
  if (workspace === undefined) {
    return null;
  }
  const main = workspace.isFlyout ? workspace.targetWorkspace : workspace;
  return main !== null && attached.has(main) ? main : null;
}

/** Where the popup opens: below the selected block, when it is on screen, in canvas pixels. */
function anchorOf(
  workspace: Blockly.WorkspaceSvg,
  block: Blockly.BlockSvg | null,
): { x: number; y: number } | null {
  if (block === null) {
    return null;
  }
  try {
    const host = workspace.getInjectionDiv().getBoundingClientRect();
    const box = block.getSvgRoot().getBoundingClientRect();
    return { x: Math.max(0, box.left - host.left), y: Math.max(0, box.bottom - host.top + 4) };
  } catch {
    return null;
  }
}

/** Opens Quick Insert on `workspace` for its cursor or selection; whether it opened. */
export function openQuickInsertOn(workspace: Blockly.WorkspaceSvg): boolean {
  const context = toolboxContentsContext(workspace);
  if (context === null || !mayOpen(workspace)) {
    return false;
  }
  const target = currentTarget(workspace);
  const at = target.selected ?? target.connection?.getSourceBlock() ?? null;
  const visible = at === null ? null : context.symbols.symbolsAt(at.id, null);
  const active = context.activeModuleId();
  const symbols =
    visible ?? context.symbols.allSymbols().filter((symbol) => symbol.module === active);
  openQuickInsert({
    workspace,
    target,
    entries: searchEntries(context),
    names: lineNames(symbols),
    symbols: context.symbols,
    anchor: anchorOf(workspace, at instanceof Blockly.BlockSvg ? at : null),
  });
  return true;
}

function register(): void {
  const { KeyCodes } = Blockly.utils;
  const registry = Blockly.ShortcutRegistry.registry;
  const ctrl = (code: number) => registry.createSerializedKey(code, [KeyCodes.CTRL]);
  const shortcuts: Blockly.ShortcutRegistry.KeyboardShortcut[] = [
    {
      name: QUICK_INSERT_IDS.shortcut,
      keyCodes: [ctrl(KeyCodes.SPACE)],
      preconditionFn: (workspace) => {
        const main = mainOf(workspace);
        return main !== null && mayOpen(main);
      },
      callback: (workspace, event) => {
        const main = mainOf(workspace);
        if (main === null || !openQuickInsertOn(main)) {
          return false;
        }
        event.preventDefault();
        return true;
      },
    },
    {
      name: QUICK_INSERT_IDS.searchShortcut,
      keyCodes: [ctrl(KeyCodes.F)],
      preconditionFn: (workspace) => {
        const main = mainOf(workspace);
        return main !== null && mayOpen(main);
      },
      callback: (workspace, event) => {
        const main = mainOf(workspace);
        if (main === null) {
          return false;
        }
        openToolboxSearch(main);
        event.preventDefault();
        return true;
      },
    },
  ];
  for (const shortcut of shortcuts) {
    if (registry.getRegistry()[shortcut.name] !== undefined) {
      registry.unregister(shortcut.name);
    }
    registry.register(shortcut);
  }
  const menus = Blockly.ContextMenuRegistry.registry;
  const items: Blockly.ContextMenuRegistry.RegistryItem[] = [
    {
      id: QUICK_INSERT_IDS.canvasMenu,
      scopeType: Blockly.ContextMenuRegistry.ScopeType.WORKSPACE,
      displayText: QUICK_INSERT_MENU_TEXT,
      weight: -1,
      preconditionFn: (scope) =>
        scope.workspace !== undefined && mayOpen(scope.workspace) ? 'enabled' : 'hidden',
      callback: (scope) => {
        if (scope.workspace !== undefined) {
          openQuickInsertOn(scope.workspace);
        }
      },
    },
    {
      id: QUICK_INSERT_IDS.searchMenu,
      scopeType: Blockly.ContextMenuRegistry.ScopeType.WORKSPACE,
      displayText: TOOLBOX_SEARCH_MENU_TEXT,
      weight: -1,
      preconditionFn: (scope) =>
        scope.workspace !== undefined && mayOpen(scope.workspace) ? 'enabled' : 'hidden',
      callback: (scope) => {
        if (scope.workspace !== undefined) {
          openToolboxSearch(scope.workspace);
        }
      },
    },
    {
      id: QUICK_INSERT_IDS.blockMenu,
      scopeType: Blockly.ContextMenuRegistry.ScopeType.BLOCK,
      displayText: QUICK_INSERT_MENU_TEXT,
      weight: -1,
      preconditionFn: (scope) => {
        const block = scope.block;
        return block instanceof Blockly.BlockSvg &&
          !block.isInFlyout &&
          !block.isShadow() &&
          mayOpen(block.workspace)
          ? 'enabled'
          : 'hidden';
      },
      callback: (scope) => {
        const block = scope.block;
        if (block instanceof Blockly.BlockSvg) {
          Blockly.common.setSelected(block);
          openQuickInsertOn(block.workspace);
        }
      },
    },
  ];
  for (const item of items) {
    if (menus.getItem(item.id) !== null) {
      menus.unregister(item.id);
    }
    menus.register(item);
  }
}

function unregister(): void {
  const registry = Blockly.ShortcutRegistry.registry;
  for (const name of [QUICK_INSERT_IDS.shortcut, QUICK_INSERT_IDS.searchShortcut]) {
    if (registry.getRegistry()[name] !== undefined) {
      registry.unregister(name);
    }
  }
  const menus = Blockly.ContextMenuRegistry.registry;
  for (const id of [
    QUICK_INSERT_IDS.canvasMenu,
    QUICK_INSERT_IDS.blockMenu,
    QUICK_INSERT_IDS.searchMenu,
  ]) {
    if (menus.getItem(id) !== null) {
      menus.unregister(id);
    }
  }
}

/** The Quick Insert plugin (attach it after the toolbox, whose blocks it offers). */
export const quickInsertPlugin: EditorPlugin = {
  name: 'quick-insert',
  attach(context: EditorContext): () => void {
    const { workspace } = context;
    if (attached.size === 0) {
      register();
    }
    attached.add(workspace);
    return () => {
      attached.delete(workspace);
      if (quickInsertSession()?.workspace === workspace) {
        closeQuickInsert();
      }
      if (attached.size === 0) {
        unregister();
      }
    };
  },
};
