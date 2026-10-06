/**
 * The window's keyboard shortcuts in M2 (docs/spec/04-user-interface.md §4.7): `F5` Run,
 * `Shift+F5` Stop, `Ctrl+B` Build and `Ctrl+S` Save. The workspace's own shortcuts (copy, paste,
 * delete, undo) belong to the editor.
 *
 * The listener runs in the capture phase, before the focused element sees the key, so the
 * shortcuts work wherever the focus is, including the console, whose terminal would otherwise send
 * `F5` and `Ctrl+S` to the program. Two scopes are excluded:
 *
 * - an open dialog owns the keyboard: the keys do nothing (but `F5` still cannot reload the page);
 * - an element marked `data-b2c-shortcuts="off"` (and everything inside it) gets the keys itself.
 *
 * **The page never reloads.** A reload would start the frontend empty while the backend keeps the
 * open project, its unsaved changes and its running program, out of the window's reach. So the
 * webview's reload keys (`Ctrl+R`, `Ctrl+Shift+R` and `F5` with `Ctrl` or `Shift`) are cancelled
 * everywhere, also in a dialog and in an opted-out element, which still receive the key (the
 * console's program gets `Ctrl+R`). The webview's own context menu, which offers *Reload*, is
 * cancelled too, except over editable text (fields, the console's terminal) and over selected text,
 * where it offers only editing and *Copy*. Menus the app draws itself, such as Blockly's, are not
 * affected: the event still reaches them.
 */
import { activateGated, type ShellActionContext } from './actions';
import { triggerCommand } from './commands';

/** What a shortcut does. */
export type ShortcutAction = 'run' | 'stop' | 'build' | 'save';

/** How each shortcut is written for people (`label`) and for `aria-keyshortcuts` (`aria`). */
export const SHORTCUTS: Readonly<Record<ShortcutAction, { label: string; aria: string }>> = {
  run: { label: 'F5', aria: 'F5' },
  stop: { label: 'Shift+F5', aria: 'Shift+F5' },
  build: { label: 'Ctrl+B', aria: 'Control+B' },
  save: { label: 'Ctrl+S', aria: 'Control+S' },
};

/** The attribute that hands the shortcut keys to an element and everything inside it. */
export const SHORTCUTS_OFF_ATTRIBUTE = 'data-b2c-shortcuts';

/** Elements inside which the shortcuts do nothing. */
const BLOCKED_SCOPE = `[role="dialog"], [role="alertdialog"]`;

/** An open dialog anywhere in the window. */
const OPEN_DIALOG = `[role="dialog"][data-state="open"], [role="alertdialog"][data-state="open"]`;

/** The letter key pressed, by its character, or by its position on non-Latin layouts. */
function letterOf(event: KeyboardEvent): string | null {
  if (/^[a-z]$/i.test(event.key)) {
    return event.key.toLowerCase();
  }
  const match = /^Key([A-Z])$/.exec(event.code);
  return match?.[1]?.toLowerCase() ?? null;
}

/** The shortcut `event` presses, if any. */
export function matchShortcut(event: KeyboardEvent): ShortcutAction | null {
  if (event.altKey || event.metaKey) {
    return null;
  }
  if (event.key === 'F5' && !event.ctrlKey) {
    return event.shiftKey ? 'stop' : 'run';
  }
  if (event.ctrlKey && !event.shiftKey) {
    switch (letterOf(event)) {
      case 'b':
        return 'build';
      case 's':
        return 'save';
      default:
        return null;
    }
  }
  return null;
}

/**
 * Whether `event` is one of the webview's reload keys: `F5` (also with `Ctrl` or `Shift`; plain
 * `F5` and `Shift+F5` are also Run and Stop), `Ctrl+R` and `Ctrl+Shift+R`. With `Alt` (AltGr) or
 * the Windows/Command key it is not, as AltGr+R types a character on some keyboard layouts.
 */
export function isReloadKey(event: KeyboardEvent): boolean {
  if (event.altKey || event.metaKey) {
    return false;
  }
  return event.key === 'F5' || (event.ctrlKey && letterOf(event) === 'r');
}

/**
 * Where the webview's context menu stays: over editable text (an input, a text area, editable
 * content, or the console's terminal, which moves its own text area under the pointer to offer
 * *Copy* and *Paste*), and over selected text. There it offers editing commands and *Copy*, never
 * *Reload*.
 */
const EDITABLE = 'input, textarea, [contenteditable]:not([contenteditable="false"]), .xterm';

/**
 * Whether `element`'s own text is selected, so the pointer is on selected text: the webview then
 * offers *Copy* instead of the page's menu. (An element that only contains the selection, such as
 * the page around it, does not count.)
 */
function isOnSelectedText(element: Element): boolean {
  const selection = element.ownerDocument.getSelection();
  if (selection === null || selection.isCollapsed) {
    return false;
  }
  for (const child of element.childNodes) {
    if (child.nodeType !== Node.TEXT_NODE || (child.textContent ?? '').trim() === '') {
      continue;
    }
    for (let index = 0; index < selection.rangeCount; index++) {
      const range = selection.getRangeAt(index);
      if (!range.collapsed && range.intersectsNode(child)) {
        return true;
      }
    }
  }
  return false;
}

/** Whether the webview's own context menu may open for `event`; see {@link EDITABLE}. */
export function keepsNativeContextMenu(event: Event): boolean {
  const target = event.target;
  const element =
    target instanceof Element ? target : target instanceof Node ? target.parentElement : null;
  if (element === null) {
    return false;
  }
  return element.closest(EDITABLE) !== null || isOnSelectedText(element);
}

/** The element the key press is aimed at. */
function targetElement(event: KeyboardEvent): Element | null {
  return event.target instanceof Element ? event.target : document.activeElement;
}

/** Whether the shortcuts are handed to the focused element. */
function isOptedOut(element: Element | null): boolean {
  return element?.closest(`[${SHORTCUTS_OFF_ATTRIBUTE}="off"]`) != null;
}

/** Whether a dialog owns the keyboard. */
function isInDialogScope(element: Element | null): boolean {
  return element?.closest(BLOCKED_SCOPE) != null || document.querySelector(OPEN_DIALOG) !== null;
}

/** Runs what a shortcut does. */
function perform(action: ShortcutAction, ctx: ShellActionContext): void {
  switch (action) {
    case 'run':
      activateGated('run', ctx);
      break;
    case 'build':
      activateGated('build', ctx);
      break;
    case 'stop':
      triggerCommand('run.stop', ctx.commands);
      break;
    case 'save':
      triggerCommand('project.save', ctx.commands);
      break;
  }
}

/**
 * Listens for the shortcuts on `target` (the window), and keeps the webview from reloading the page
 * (see the module comment). Returns the function that stops listening.
 */
export function installShortcuts(target: Window, ctx: ShellActionContext): () => void {
  const onKeyDown = (event: KeyboardEvent) => {
    if (isReloadKey(event)) {
      // Wherever the focus is. Not stopped here: an element that gets the key itself (the
      // console's program, for Ctrl+R) still does.
      event.preventDefault();
    }
    if (event.isComposing) {
      return;
    }
    const action = matchShortcut(event);
    if (action === null) {
      return;
    }
    const element = targetElement(event);
    if (isOptedOut(element)) {
      return;
    }
    // The webview's own meaning of these keys (save the page, bookmarks) never applies.
    event.preventDefault();
    event.stopPropagation();
    if (event.repeat || isInDialogScope(element)) {
      return;
    }
    perform(action, ctx);
  };
  const onContextMenu = (event: MouseEvent) => {
    if (!keepsNativeContextMenu(event)) {
      // Not stopped: Blockly and the other menus the app draws still see the event.
      event.preventDefault();
    }
  };
  target.addEventListener('keydown', onKeyDown, { capture: true });
  target.addEventListener('contextmenu', onContextMenu, { capture: true });
  return () => {
    target.removeEventListener('keydown', onKeyDown, { capture: true });
    target.removeEventListener('contextmenu', onContextMenu, { capture: true });
  };
}
