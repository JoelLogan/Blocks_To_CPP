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
 * Listens for the shortcuts on `target` (the window). Returns the function that stops listening.
 */
export function installShortcuts(target: Window, ctx: ShellActionContext): () => void {
  const onKeyDown = (event: KeyboardEvent) => {
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
    // The webview's own meaning of these keys (reload, save the page, bookmarks) never applies.
    event.preventDefault();
    event.stopPropagation();
    if (event.repeat || isInDialogScope(element)) {
      return;
    }
    perform(action, ctx);
  };
  target.addEventListener('keydown', onKeyDown, { capture: true });
  return () => {
    target.removeEventListener('keydown', onKeyDown, { capture: true });
  };
}
