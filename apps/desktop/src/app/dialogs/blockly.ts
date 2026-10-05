/**
 * Routes Blockly's own alerts, confirmations and prompts (for example when renaming) through the
 * app's accessible dialogs instead of the browser's `window.alert`, `confirm` and `prompt`
 * (docs/spec/04-user-interface.md §4.8).
 */
import * as Blockly from 'blockly/core';

import { dialogs } from './instance';
import type { DialogService, DialogTextOptions } from './service';

/**
 * Lends Blockly's keyboard focus to the open dialog, as Blockly requires of dialogs that replace
 * its prompts, and returns it when the dialog closes. Nothing happens when something else (such as
 * a field's drop-down) already holds it.
 */
function lendBlocklyFocus(content: HTMLElement): (() => void) | undefined {
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
