/**
 * Test support for the keyboard plugin's tests (not used by the app): an injected editor with the
 * app's toolbox and the keyboard plugin attached, and key presses as Blockly reads them (it matches
 * shortcuts by `keyCode`).
 */
import type { BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import { b2cLightTheme } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';

import type { EditorContext, EditorPlugin } from '../../app/editor-types';
import { resetAppStore, useAppStore } from '../../app/store';
import { projectFixture } from '../../app/testing/fixtures';
import { providePathCatalog } from '../diagnostics/catalog';
import { BLOCKLY_EXT_PATH_CATALOG } from '../diagnostics/plugin';
import { editorInjectOptions } from '../EditorWorkspace';
import { registerEditorBlocks } from '../services';
import { withoutEvents } from '../sync/bdmToWorkspace';
import { clearWorkspace } from '../sync/traverse';
import { registerToolboxComponents } from '../toolbox/register';
import { guessDocument, loadModule } from '../toolbox/testing';
import type { EditorKeyboard } from './navigation';
import { createKeyboardPlugin, type KeyboardPluginOptions } from './plugin';

/** The `keyCode` of each key the tests press. */
export const KEY_CODES: Readonly<Record<string, number>> = {
  ArrowDown: 40,
  ArrowUp: 38,
  ArrowRight: 39,
  ArrowLeft: 37,
  Enter: 13,
  ' ': 32,
  Escape: 27,
  Tab: 9,
  Delete: 46,
  Backspace: 8,
  Home: 36,
  End: 35,
  m: 77,
  t: 84,
  x: 88,
  c: 67,
  v: 86,
  z: 90,
  y: 89,
  Shift: 16,
};

/** Modifiers of a key press. */
export interface KeyModifiers {
  readonly ctrlKey?: boolean;
  readonly shiftKey?: boolean;
  readonly altKey?: boolean;
  readonly metaKey?: boolean;
}

/**
 * Presses `key` (a `KeyboardEvent.key`) on the focused element, as a browser would: a cancellable
 * `keydown` that bubbles. Returns the event (to check `defaultPrevented`).
 */
export function press(key: string, modifiers: KeyModifiers = {}): KeyboardEvent {
  const keyCode = KEY_CODES[key] ?? key.toUpperCase().charCodeAt(0);
  const event = new KeyboardEvent('keydown', {
    key,
    keyCode,
    bubbles: true,
    cancelable: true,
    ...modifiers,
  });
  const target = document.activeElement ?? document.body;
  target.dispatchEvent(event);
  return event;
}

/** An editor for the keyboard tests. */
export interface KeyboardEditor {
  readonly workspace: Blockly.WorkspaceSvg;
  readonly keyboard: EditorKeyboard;
  /** The block with this ID (it must exist). */
  readonly block: (id: string) => Blockly.BlockSvg;
  /** Detaches the plugin and disposes the workspace. */
  readonly dispose: () => void;
}

/** What {@link keyboardEditor} sets up. */
export interface KeyboardEditorOptions extends KeyboardPluginOptions {
  /** The document on the canvas (by default {@link guessDocument}). */
  readonly document?: BdmDocument;
  /** More plugins, attached before the keyboard plugin (as `EDITOR_PLUGINS` orders them). */
  readonly plugins?: readonly EditorPlugin[];
}

/** Waits until Blockly has delivered (and recorded for undo) the events fired so far. */
export function eventsDelivered(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => {
      setTimeout(() => {
        setTimeout(resolve, 0);
      }, 0);
    });
  });
}

/** The focused node, as Blockly has it. */
export function focused(): Blockly.IFocusableNode | null {
  return Blockly.getFocusManager().getFocusedNode();
}

/**
 * An injected editor with the app's inject options (continuous toolbox, Zelos, the type-aware
 * checker), the given document loaded and the keyboard plugin attached. The document is also the
 * open project, so blocks are named with its symbols.
 */
export function keyboardEditor(options: KeyboardEditorOptions = {}): KeyboardEditor {
  registerEditorBlocks();
  registerToolboxComponents();
  providePathCatalog(BLOCKLY_EXT_PATH_CATALOG);
  resetAppStore();
  const doc = options.document ?? guessDocument();
  useAppStore.getState().actions.setProject(projectFixture({ document: doc }));

  const host = document.createElement('div');
  document.body.append(host);
  const workspace = Blockly.inject(host, editorInjectOptions(b2cLightTheme));
  loadModule(workspace, doc);
  const attachedTo: { keyboard: EditorKeyboard | null } = { keyboard: null };
  const context: EditorContext = {
    workspace,
    store: useAppStore,
    core: () => null,
    selectBlock: () => undefined,
    activeModuleId: () => doc.modules[0]?.id ?? '',
  };
  const detachers: (() => void)[] = [];
  for (const plugin of options.plugins ?? []) {
    detachers.push(plugin.attach(context));
  }
  const plugin = createKeyboardPlugin({
    matchMedia: options.matchMedia ?? null,
    onAttached: (attached) => {
      attachedTo.keyboard = attached;
      options.onAttached?.(attached);
    },
  });
  detachers.push(plugin.attach(context));
  const attached = attachedTo.keyboard;
  if (attached === null) {
    throw new Error('The keyboard plugin did not attach');
  }
  let disposed = false;
  return {
    workspace,
    keyboard: attached,
    block: (id) => {
      const block = workspace.getBlockById(id);
      if (!(block instanceof Blockly.BlockSvg)) {
        throw new Error(`There is no block ${id}`);
      }
      return block;
    },
    dispose: () => {
      if (disposed) {
        return;
      }
      disposed = true;
      for (const detach of detachers.reverse()) {
        detach();
      }
      clearWorkspace(workspace);
      withoutEvents(() => {
        workspace.dispose();
      });
      Blockly.WidgetDiv.hide();
      Blockly.DropDownDiv.hideWithoutAnimation();
      host.remove();
    },
  };
}
