/**
 * The app's dialogs and Blockly: Blockly's own alerts, confirmations and prompts (for example when
 * renaming) go through the app's accessible dialogs instead of the browser's `window.alert`,
 * `confirm` and `prompt` (docs/spec/04-user-interface.md §4.8), and the block editor's pop-ups
 * close before any of the app's dialogs opens (§4.1).
 */
import * as Blockly from 'blockly/core';

import type { EditorPlugin } from '../editor-types';
import { dialogs } from './instance';
import type { DialogQueue, DialogService, DialogTextOptions } from './service';

/**
 * Lends Blockly's keyboard focus to the open dialog, as Blockly requires of dialogs that replace
 * its prompts, and returns it when the dialog closes. Nothing happens when something else (such as
 * a field's drop-down) already holds it. The editor's own replacements of Blockly's dialogs (such
 * as the canvas menu's *Delete N blocks*) pass it as their `onOpen`.
 */
export function lendBlocklyFocus(content: HTMLElement): (() => void) | undefined {
  const manager = Blockly.getFocusManager();
  if (manager.ephemeralFocusTaken()) {
    return undefined;
  }
  try {
    return manager.takeEphemeralFocus(content);
  } catch (error: unknown) {
    // The dialog still works; only Blockly's focus bookkeeping is left as it was.
    console.warn('Blockly did not lend its focus to the dialog', error);
    return undefined;
  }
}

const BLOCKLY_DIALOG: Pick<DialogTextOptions, 'onOpen'> = { onOpen: lendBlocklyFocus };

/**
 * Makes Blockly use `service` (by default the app's dialogs) for its alerts, confirmations and
 * prompts. The override is global to Blockly, so it is installed once at start-up, before any
 * workspace exists. Returns the function that restores Blockly's defaults.
 */
export function installBlocklyDialogs(service: DialogService = dialogs): () => void {
  Blockly.dialog.setAlert((message, callback) => {
    void service.alert({ message, ...BLOCKLY_DIALOG }).then(() => {
      callback?.();
    });
  });
  Blockly.dialog.setConfirm((message, callback) => {
    void service.confirm({ message, ...BLOCKLY_DIALOG }).then(callback);
  });
  Blockly.dialog.setPrompt((message, defaultValue, callback) => {
    void service.prompt({ message, defaultValue, ...BLOCKLY_DIALOG }).then(callback);
  });
  return () => {
    Blockly.dialog.setAlert();
    Blockly.dialog.setConfirm();
    Blockly.dialog.setPrompt();
  };
}

/**
 * The editor plugin that closes the block editor's pop-ups before one of the app's dialogs opens
 * (docs/spec/04-user-interface.md §4.1): a field's drop-down or text editor, a context menu, a
 * tooltip and the trash can's flyout. Blockly attaches them to the page's body above everything
 * else (the drop-down at z-index 1000, the others near 100000), so one left open would be drawn
 * over the modal dialog and its overlay. Closing a field's text editor keeps what was typed, as
 * clicking elsewhere does.
 *
 * It is `Blockly.hideChaff()` for the plugin's own workspace rather than the one Blockly last saw
 * focused, and it is registered only while that workspace exists.
 */
export function blocklyPopupsPlugin(
  queue: Pick<DialogQueue, 'onBeforeShow'> = dialogs,
): EditorPlugin {
  return {
    name: 'dialog-popups',
    attach: ({ workspace }) =>
      queue.onBeforeShow(() => {
        workspace.hideChaff();
      }),
  };
}
