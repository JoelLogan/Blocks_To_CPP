/**
 * The menus editor plugin (docs/spec/04-user-interface.md §4.1 "Menus"): Blockly's block and
 * canvas menu items as Blocks2Cpp offers them (./registry.ts), with the question *Delete N blocks*
 * asks in the app's dialogs.
 */
import type { EditorContext, EditorPlugin } from '../../app/editor-types';
import { lendBlocklyFocus } from '../../app/dialogs/blockly';
import { dialogs as appDialogs } from '../../app/dialogs/instance';
import type { AlertOptions, ConfirmOptions, DialogService } from '../../app/dialogs/service';
import { deleteLabel, installMenuItems } from './registry';

/** Options of {@link createMenusPlugin}. */
export interface MenusPluginOptions {
  /** Where questions are asked and notices shown; the app's dialogs by default. */
  readonly dialogs?: Pick<DialogService, 'confirm' | 'alert'>;
}

/** The question *Delete N blocks* asks before it deletes `count` blocks. */
export function deleteAllQuestion(count: number): ConfirmOptions {
  return {
    title: count === 1 ? 'Delete the block on the canvas?' : `Delete all ${String(count)} blocks?`,
    message: `Undo (Ctrl+Z) brings ${count === 1 ? 'it' : 'them'} back.`,
    confirmLabel: deleteLabel(count),
    cancelLabel: 'Cancel',
    destructive: true,
    onOpen: lendBlocklyFocus,
  };
}

/** The notice shown when *Delete N blocks* deleted nothing because of a chain of `length`. */
export function chainTooLongNotice(length: number): AlertOptions {
  return {
    title: 'The blocks were not deleted',
    message: `A chain of ${String(length)} statements is longer than the block editor can delete in one step, because it nests each statement inside the one before it. Nothing was deleted: delete part of the chain first.`,
    onOpen: lendBlocklyFocus,
  };
}

/** Creates the menus plugin with `options`. */
export function createMenusPlugin(options: MenusPluginOptions = {}): EditorPlugin {
  const dialogs = options.dialogs ?? appDialogs;
  return {
    name: 'menus',
    attach(ctx: EditorContext): () => void {
      return installMenuItems({
        workspace: ctx.workspace,
        confirmDeleteAll: (count) => dialogs.confirm(deleteAllQuestion(count)),
        tellChainTooLong: (length) => {
          dialogs.alert(chainTooLongNotice(length)).catch((error: unknown) => {
            console.error('A notice could not be shown', error);
          });
        },
      });
    },
  };
}

/** The menus plugin with the app's dialogs. */
export const menusPlugin: EditorPlugin = createMenusPlugin();
