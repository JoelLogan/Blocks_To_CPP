/**
 * The block editor's navigation keys in Blockly's global shortcut registry (docs/spec/04-user-
 * interface.md §4.7): the arrow keys, Enter and Space, `M`, `T`, and Escape (which still reaches
 * Blockly's own Escape when the editor has nothing to do with it). They are plain keys: with Ctrl,
 * Alt, Shift or the Windows key Blockly's registry does not match them, so they never take a key
 * from the clipboard (`Ctrl+C`…), undo (`Ctrl+Z`) or the window's shortcuts (`F5`, `Ctrl+S`).
 *
 * The registry is global while each editor has its own canvas: the keys are registered when the
 * first editor attaches and removed when the last one detaches; a key goes to the editor of the
 * canvas (or toolbox, or flyout) it was pressed in. A held block (./mover.ts) gets its keys before
 * Blockly sees them, so none of these run during a move.
 */
import * as Blockly from 'blockly/core';

import type { CursorStep, EditorKeyboard } from './navigation';

/** The names of the shortcuts, all prefixed so they cannot clash with Blockly's own. */
export const KEYBOARD_SHORTCUT_NAMES = {
  down: 'b2c_kb_down',
  up: 'b2c_kb_up',
  right: 'b2c_kb_right',
  left: 'b2c_kb_left',
  activate: 'b2c_kb_activate',
  move: 'b2c_kb_move',
  toolbox: 'b2c_kb_toolbox',
  escape: 'b2c_kb_escape',
} as const;

const editors = new Map<Blockly.WorkspaceSvg, EditorKeyboard>();
let registered: string[] | null = null;

/** The editor of the canvas a key was pressed on (its own canvas or its flyout), or `null`. */
function editorOf(workspace: Blockly.WorkspaceSvg | undefined): EditorKeyboard | null {
  if (workspace === undefined) {
    return null;
  }
  const main = workspace.isFlyout ? workspace.targetWorkspace : workspace;
  return main === null ? null : (editors.get(main) ?? null);
}

/** Whether the editor may take a key now: no drag, no field editor or drop-down open, no move. */
function keysAllowed(editor: EditorKeyboard): boolean {
  return (
    !editor.workspace.isDragging() &&
    !Blockly.getFocusManager().ephemeralFocusTaken() &&
    !editor.mover.active
  );
}

/** One shortcut: `run` returns whether it handled the key (else Blockly's own, if any, runs). */
function shortcut(
  name: string,
  keyCodes: number[],
  run: (editor: EditorKeyboard, event: Event) => boolean,
  allowCollision = false,
): Blockly.ShortcutRegistry.KeyboardShortcut {
  return {
    name,
    keyCodes,
    allowCollision,
    preconditionFn(workspace) {
      const editor = editorOf(workspace);
      return editor !== null && keysAllowed(editor);
    },
    callback(workspace, event) {
      const editor = editorOf(workspace);
      if (editor === null || event.defaultPrevented) {
        // Already handled where it was pressed (Blockly's toolbox handles its own arrow keys).
        return false;
      }
      const handled = run(editor, event);
      if (handled) {
        // No scrolling of the page, no button press, no typing.
        event.preventDefault();
      }
      return handled;
    },
  };
}

/** The cursor step of an arrow key, mirrored for right-to-left canvases. */
function arrow(step: CursorStep, mirrored: CursorStep) {
  return (editor: EditorKeyboard) => editor.step(editor.workspace.RTL ? mirrored : step);
}

function keyboardShortcuts(): Blockly.ShortcutRegistry.KeyboardShortcut[] {
  const { KeyCodes } = Blockly.utils;
  const names = KEYBOARD_SHORTCUT_NAMES;
  return [
    shortcut(names.down, [KeyCodes.DOWN], (editor) => editor.step('next')),
    shortcut(names.up, [KeyCodes.UP], (editor) => editor.step('previous')),
    shortcut(names.right, [KeyCodes.RIGHT], (editor) =>
      editor.area() === 'toolbox' ? editor.enterFlyout() : arrow('in', 'out')(editor),
    ),
    shortcut(names.left, [KeyCodes.LEFT], arrow('out', 'in')),
    shortcut(names.activate, [KeyCodes.ENTER, KeyCodes.SPACE], (editor) => editor.activate()),
    shortcut(names.move, [KeyCodes.M], (editor) => editor.pickUp()),
    shortcut(names.toolbox, [KeyCodes.T], (editor) => editor.openToolbox()),
    shortcut(names.escape, [KeyCodes.ESC], (editor) => editor.escape(), true),
  ];
}

/** Registers the shortcuts; returns their names. */
function register(): string[] {
  const registry = Blockly.ShortcutRegistry.registry;
  const known = registry.getRegistry();
  const added: string[] = [];
  for (const item of keyboardShortcuts()) {
    if (known[item.name] !== undefined) {
      // Left over from an editor that was not detached (a test): replace it.
      registry.unregister(item.name);
    }
    registry.register(item);
    added.push(item.name);
  }
  return added;
}

/** Removes the shortcuts registered by {@link register}. */
function unregister(names: readonly string[]): void {
  const registry = Blockly.ShortcutRegistry.registry;
  const known = registry.getRegistry();
  for (const name of names) {
    if (known[name] !== undefined) {
      registry.unregister(name);
    }
  }
}

/**
 * Sends the navigation keys of `editor.workspace` (and of its toolbox and flyout) to `editor`,
 * registering the shortcuts on first use. Returns the function that stops it (and, for the last
 * editor, removes the shortcuts again).
 */
export function installKeyboardShortcuts(editor: EditorKeyboard): () => void {
  editors.set(editor.workspace, editor);
  registered ??= register();
  let installed = true;
  return () => {
    if (!installed) {
      return;
    }
    installed = false;
    if (editors.get(editor.workspace) === editor) {
      editors.delete(editor.workspace);
    }
    if (editors.size === 0 && registered !== null) {
      const names = registered;
      registered = null;
      unregister(names);
    }
  };
}
