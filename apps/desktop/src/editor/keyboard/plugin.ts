/**
 * The keyboard editor plugin (docs/spec/04-user-interface.md §4.7, §4.8): Blockly 12's keyboard
 * navigation for the block editor, built on Blockly's own focus manager, cursor and navigation
 * rules (the maintained `@blockly/keyboard-navigation` plugin is not used: it replaces the
 * clipboard's validated copy and paste, drags blocks with its own strategy past the editing
 * session, enables disabled blocks when they are dragged, and writes HTML with `innerHTML`).
 *
 * Attached to a workspace it:
 *
 * - binds the arrow keys, Enter and Space, `M` (move), `T` (toolbox) and Escape (./shortcuts.ts,
 *   ./navigation.ts); copy, cut, paste and duplicate stay the clipboard plugin's, and Delete, undo,
 *   redo and the block menu (`Ctrl+Enter`) stay Blockly's own, all acting on the keyboard focus;
 * - gives a held block the keyboard until it is put down or the move is cancelled (./mover.ts);
 * - puts the toolbox's blocks between the toolbox and the canvas in the Tab order, and names the
 *   canvas and the toolbox's blocks, with their keys as description (./help.ts);
 * - announces what the keyboard reaches in a polite live region (./announcer.ts);
 * - honours the system's reduced-motion setting for Blockly's own effects (./motion.ts and
 *   ./keyboard.css).
 */
import type { BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';

import type { EditorContext, EditorPlugin } from '../../app/editor-types';
import { indexDocument } from '../diagnostics/blockPath';
import { Announcer } from './announcer';
import { CANVAS_DESCRIPTION, CANVAS_LABEL, FLYOUT_DESCRIPTION, FLYOUT_LABEL } from './help';
import type { SymbolNames } from './labels';
import { attachReducedMotion, type MatchMedia, windowMatchMedia } from './motion';
import { EditorKeyboard } from './navigation';
import { installKeyboardShortcuts } from './shortcuts';
import './keyboard.css';

/** Options of {@link createKeyboardPlugin}. */
export interface KeyboardPluginOptions {
  /** Looks media queries up (`window.matchMedia` by default; tests pass a stand-in). */
  readonly matchMedia?: MatchMedia | null;
  /** Receives each editor's keyboard once it is attached (tests and tools). */
  readonly onAttached?: (keyboard: EditorKeyboard) => void;
}

/** The message for keys that do nothing while a block is held. */
export const MOVE_KEYS_HINT =
  'Moving a block: arrow keys choose where it goes, Enter puts it there, Escape cancels.';

/** Keys that only modify others: they never end or disturb a move. */
const MODIFIER_KEYS = new Set(['Shift', 'Control', 'Alt', 'AltGraph', 'Meta', 'CapsLock']);

/** The symbol names of the open document, recomputed only when the document changes. */
function symbolNames(store: EditorContext['store']): () => SymbolNames {
  let indexed: BdmDocument | null = null;
  let names: SymbolNames = new Map();
  return () => {
    const document = store.getState().project?.document ?? null;
    if (document !== indexed) {
      indexed = document;
      names = document === null ? new Map() : indexDocument(document).names;
    }
    return names;
  };
}

/** Sets an attribute on an element Blockly made, remembering the old value to put back. */
function setAttribute(element: Element, name: string, value: string): () => void {
  const old = element.getAttribute(name);
  element.setAttribute(name, value);
  return () => {
    if (old === null) {
      element.removeAttribute(name);
    } else {
      element.setAttribute(name, old);
    }
  };
}

/**
 * Names the canvas and the toolbox's blocks, describes their keys, and moves the toolbox's blocks
 * before the canvas in the document, so Tab goes toolbox, its blocks, canvas (Blockly puts the
 * flyout after the canvas). Returns the function that undoes it.
 */
function prepareTrees(keyboard: EditorKeyboard): () => void {
  const workspace = keyboard.workspace;
  const undo: (() => void)[] = [];
  const canvas = workspace.getSvgGroup();
  undo.push(
    setAttribute(canvas, 'aria-label', CANVAS_LABEL),
    setAttribute(canvas, 'aria-describedby', keyboard.announcer.descriptionId),
  );
  const flyout = keyboard.flyout();
  if (flyout !== null) {
    const flyoutWorkspace = flyout.getWorkspace();
    const group = flyoutWorkspace.getSvgGroup();
    undo.push(
      setAttribute(group, 'aria-label', FLYOUT_LABEL),
      setAttribute(group, 'aria-describedby', keyboard.announcer.flyoutDescriptionId),
    );
    const flyoutSvg = flyoutWorkspace.getParentSvg();
    const canvasSvg = workspace.getParentSvg();
    const parent = canvasSvg.parentNode;
    if (flyoutSvg !== canvasSvg && parent !== null && flyoutSvg.parentNode === parent) {
      const after = flyoutSvg.nextSibling;
      parent.insertBefore(flyoutSvg, canvasSvg);
      undo.push(() => {
        if (flyoutSvg.parentNode === parent && (after === null || after.parentNode === parent)) {
          parent.insertBefore(flyoutSvg, after);
        }
      });
    }
  }
  return () => {
    for (const step of undo.reverse()) {
      step();
    }
  };
}

/**
 * Listens for keyboard use around the canvas: a held block gets every key first (before Blockly),
 * and focus arriving by Tab turns on Blockly's keyboard-navigation look and says where it landed.
 * Returns the function that stops listening.
 */
function listen(keyboard: EditorKeyboard): () => void {
  const injection = keyboard.workspace.getInjectionDiv();
  const document = injection.ownerDocument;
  let tabbed = false;

  const onKeyDownCapture = (event: KeyboardEvent) => {
    const mover = keyboard.mover;
    if (!mover.active || MODIFIER_KEYS.has(event.key)) {
      return;
    }
    if (event.key === 'Tab') {
      // Tab leaves the canvas as usual; the move ends with it.
      mover.cancel();
      return;
    }
    if (!mover.handleKey(event)) {
      keyboard.announcer.announce(MOVE_KEYS_HINT);
    }
    event.preventDefault();
    event.stopPropagation();
  };
  const onDocumentKeyDown = (event: KeyboardEvent) => {
    tabbed = event.key === 'Tab';
  };
  const onDocumentPointerDown = () => {
    tabbed = false;
  };
  const onFocusIn = () => {
    if (!tabbed) {
      return;
    }
    tabbed = false;
    Blockly.keyboardNavigationController.setIsActive(true);
    keyboard.announceFocus();
  };

  injection.addEventListener('keydown', onKeyDownCapture, true);
  injection.addEventListener('focusin', onFocusIn);
  document.addEventListener('keydown', onDocumentKeyDown, true);
  document.addEventListener('pointerdown', onDocumentPointerDown, true);
  return () => {
    injection.removeEventListener('keydown', onKeyDownCapture, true);
    injection.removeEventListener('focusin', onFocusIn);
    document.removeEventListener('keydown', onDocumentKeyDown, true);
    document.removeEventListener('pointerdown', onDocumentPointerDown, true);
  };
}

/**
 * Moves the keyboard focus off a connection of `workspace` (onto its block): Blockly 12.5 fails to
 * dispose of a workspace while one of its connections has the focus.
 */
function releaseConnectionFocus(workspace: Blockly.WorkspaceSvg): void {
  try {
    const node = Blockly.getFocusManager().getFocusedNode();
    if (node instanceof Blockly.RenderedConnection) {
      const block = node.getSourceBlock();
      if (block.workspace === workspace && !block.isDeadOrDying()) {
        Blockly.getFocusManager().focusNode(block);
      }
    }
  } catch (error: unknown) {
    console.warn('The keyboard focus could not be moved off a connection', error);
  }
}

/** The keyboard plugin with its dependencies replaced (see {@link KeyboardPluginOptions}). */
export function createKeyboardPlugin(options: KeyboardPluginOptions = {}): EditorPlugin {
  return {
    name: 'keyboard',
    attach(ctx: EditorContext): () => void {
      const workspace = ctx.workspace;
      const announcer = new Announcer(workspace.getInjectionDiv(), {
        canvas: CANVAS_DESCRIPTION,
        flyout: FLYOUT_DESCRIPTION,
      });
      const keyboard = new EditorKeyboard({ workspace, announcer, names: symbolNames(ctx.store) });
      const undoTrees = prepareTrees(keyboard);
      const stopListening = listen(keyboard);
      const uninstallShortcuts = installKeyboardShortcuts(keyboard);
      const matchMedia = options.matchMedia === undefined ? windowMatchMedia() : options.matchMedia;
      const detachMotion = attachReducedMotion(workspace, matchMedia);
      // Another project (or a reload) replaces the canvas: a held block is gone with it.
      const unsubscribe = ctx.store.subscribe((state, previous) => {
        if (state.project?.handle !== previous.project?.handle) {
          keyboard.mover.cancel();
        }
      });
      options.onAttached?.(keyboard);
      return () => {
        unsubscribe();
        keyboard.dispose();
        releaseConnectionFocus(workspace);
        detachMotion();
        uninstallShortcuts();
        stopListening();
        undoTrees();
        announcer.dispose();
      };
    },
  };
}

/** The keyboard editor plugin (append it to `EDITOR_PLUGINS`). */
export const keyboardPlugin: EditorPlugin = createKeyboardPlugin();
