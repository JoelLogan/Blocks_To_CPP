/**
 * The quick-fix editor plugin (docs/plans/m3-contracts/w4-editor-tools.md §2.5): a block's menu
 * offers the fix of each of its diagnostics that has one (*Create variable “score”*), and the
 * Problems panel asks {@link quickFixesFor} for a row's. A fix acts on the canvas shown, as one undo
 * step (./fixes.ts).
 *
 * Blockly's menu registry is global: the item is registered while at least one editor is attached,
 * and acts on the canvas its menu was opened on.
 */
import type { Diagnostic } from '@blocks2cpp/ipc-types';
import * as Blockly from 'blockly/core';

import type { EditorContext, EditorPlugin } from '../../app/editor-types';
import { toolboxContentsContext } from '../toolbox/plugin';
import { fixesFor, type FixContext, type QuickFix } from './fixes';

/** The ID of the block menu's quick-fix items (one per possible fix of a block). */
export const QUICK_FIX_MENU_ID = 'b2c_quick_fix';

/** How many fixes one block's menu offers at most. */
const MAX_MENU_FIXES = 3;

/** What an attached editor gives the fixes: its context, and its live diagnostics. */
interface AttachedEditor {
  readonly context: () => FixContext | null;
  readonly diagnostics: () => readonly Diagnostic[];
}

const editors = new Map<Blockly.WorkspaceSvg, AttachedEditor>();

/** The editor shown last attached (the app has one). */
function current(): AttachedEditor | null {
  return [...editors.values()].at(-1) ?? null;
}

/** The fixes of `diagnostic` that apply now on the canvas shown (none without an editor). */
export function quickFixesFor(diagnostic: Diagnostic): QuickFix[] {
  const editor = current();
  const context = editor?.context() ?? null;
  return context === null ? [] : fixesFor(diagnostic, context);
}

/** The fixes of the live diagnostics of `block`, in their order, at most a few. */
function blockFixes(block: Blockly.BlockSvg): QuickFix[] {
  const editor = editors.get(block.workspace);
  const context = editor?.context() ?? null;
  if (editor === undefined || context === null) {
    return [];
  }
  const seen = new Set<string>();
  const fixes: QuickFix[] = [];
  for (const diagnostic of editor.diagnostics()) {
    if (diagnostic.primary.block !== block.id) {
      continue;
    }
    for (const fix of fixesFor(diagnostic, context)) {
      if (!seen.has(fix.id) && fixes.length < MAX_MENU_FIXES) {
        seen.add(fix.id);
        fixes.push(fix);
      }
    }
  }
  return fixes;
}

/** The menu item for the `index`-th fix of a block. */
function menuItem(index: number): Blockly.ContextMenuRegistry.RegistryItem {
  const fixAt = (scope: Blockly.ContextMenuRegistry.Scope): QuickFix | null => {
    const block = scope.block;
    return block instanceof Blockly.BlockSvg && !block.isInFlyout
      ? (blockFixes(block)[index] ?? null)
      : null;
  };
  return {
    id: `${QUICK_FIX_MENU_ID}_${String(index)}`,
    scopeType: Blockly.ContextMenuRegistry.ScopeType.BLOCK,
    weight: -2 + index / 10,
    displayText: (scope) => `Fix: ${fixAt(scope)?.title ?? ''}`,
    preconditionFn: (scope) => (fixAt(scope) === null ? 'hidden' : 'enabled'),
    callback: (scope) => {
      fixAt(scope)?.apply();
    },
  };
}

function register(): void {
  const menus = Blockly.ContextMenuRegistry.registry;
  for (let index = 0; index < MAX_MENU_FIXES; index += 1) {
    const item = menuItem(index);
    if (menus.getItem(item.id) !== null) {
      menus.unregister(item.id);
    }
    menus.register(item);
  }
}

function unregister(): void {
  const menus = Blockly.ContextMenuRegistry.registry;
  for (let index = 0; index < MAX_MENU_FIXES; index += 1) {
    const id = `${QUICK_FIX_MENU_ID}_${String(index)}`;
    if (menus.getItem(id) !== null) {
      menus.unregister(id);
    }
  }
}

/** The quick-fix plugin (attach it after the toolbox, whose symbols it reads). */
export const quickFixPlugin: EditorPlugin = {
  name: 'quick-fixes',
  attach(context: EditorContext): () => void {
    const { workspace, store } = context;
    if (editors.size === 0) {
      register();
    }
    editors.set(workspace, {
      context: () => {
        const contents = toolboxContentsContext(workspace);
        return contents === null
          ? null
          : { workspace, symbols: contents.symbols, moduleId: context.activeModuleId() };
      },
      diagnostics: () => store.getState().analysis.preview?.diagnostics ?? [],
    });
    return () => {
      editors.delete(workspace);
      if (editors.size === 0) {
        unregister();
      }
    };
  },
};
