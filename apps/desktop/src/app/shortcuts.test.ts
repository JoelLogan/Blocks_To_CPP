/**
 * The window's shortcuts and what the shell's buttons do (actions.ts), including Undo and Redo for
 * the block editor wherever the focus is, except in text fields, dialogs, Blockly's own areas and
 * the console while a program runs.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { activateGated, installShellCommands, type ShellActionContext } from './actions';
import { type CommandId, createCommandRegistry } from './commands';
import type { EditorHandle } from './editor-types';
import { createScreenRegistry } from './screens';
import {
  installShortcuts,
  isReloadKey,
  keepsNativeContextMenu,
  matchShortcut,
  registerPendingEdit,
  SHORTCUTS_OFF_ATTRIBUTE,
} from './shortcuts';
import { resetAppStore, useAppStore } from './store';
import {
  diagnosticFixture,
  previewFixture,
  projectFixture,
  settingsFixture,
  toolchainFixture,
} from './testing/fixtures';

/** A shell context with fresh registries and a spy for every command. */
function setUp() {
  const commands = createCommandRegistry();
  const screens = createScreenRegistry();
  const editor = { selectBlock: vi.fn() };
  const ctx: ShellActionContext = {
    store: useAppStore,
    commands,
    screens,
    editor: () => editor as unknown as EditorHandle,
  };
  const ran: CommandId[] = [];
  for (const id of [
    'run.start',
    'run.stop',
    'build.start',
    'project.save',
    'problems.focusFirstError',
    'edit.undo',
    'edit.redo',
  ] as const) {
    commands.registerCommand(id, () => {
      ran.push(id);
    });
  }
  return { ctx, commands, screens, editor, ran };
}

/** A project shown in the block editor. */
function showEditor(): void {
  const { actions } = useAppStore.getState();
  actions.setProject(projectFixture());
  actions.setUi({ screen: 'editor' });
}

/** Appends `element` to `parent` (the page's body by default) and returns it. */
function attach<T extends Element>(element: T, parent: Element = document.body): T {
  parent.append(element);
  return element;
}

/** A state in which Run is available. */
function makeRunnable(): void {
  const { actions } = useAppStore.getState();
  actions.setProject(projectFixture());
  actions.setToolchains({ list: [toolchainFixture()] });
  actions.setSettings({ value: settingsFixture() });
}

function press(init: KeyboardEventInit, target: EventTarget = document.body): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}

let uninstall: () => void = () => undefined;

beforeEach(() => {
  resetAppStore();
});

afterEach(() => {
  uninstall();
  document.body.replaceChildren();
});

describe('matchShortcut', () => {
  const key = (init: KeyboardEventInit) => matchShortcut(new KeyboardEvent('keydown', init));

  it('knows the four M2 shortcuts', () => {
    expect(key({ key: 'F5' })).toBe('run');
    expect(key({ key: 'F5', shiftKey: true })).toBe('stop');
    expect(key({ key: 'b', ctrlKey: true })).toBe('build');
    expect(key({ key: 'S', ctrlKey: true })).toBe('save');
  });

  it('finds the letter on a non-Latin keyboard layout by its position', () => {
    expect(key({ key: 'и', code: 'KeyB', ctrlKey: true })).toBe('build');
    expect(key({ key: 'ы', code: 'KeyS', ctrlKey: true })).toBe('save');
    expect(key({ key: 'ы', code: 'Digit1', ctrlKey: true })).toBeNull();
  });

  it('knows Undo and Redo, also on a non-Latin keyboard layout', () => {
    expect(key({ key: 'z', ctrlKey: true })).toBe('undo');
    expect(key({ key: 'y', ctrlKey: true })).toBe('redo');
    expect(key({ key: 'Z', ctrlKey: true, shiftKey: true })).toBe('redo');
    expect(key({ key: 'я', code: 'KeyZ', ctrlKey: true })).toBe('undo');
    expect(key({ key: 'н', code: 'KeyY', ctrlKey: true })).toBe('redo');
    expect(key({ key: 'Y', ctrlKey: true, shiftKey: true })).toBeNull();
    expect(key({ key: 'z', ctrlKey: true, altKey: true })).toBeNull();
    expect(key({ key: 'z', metaKey: true })).toBeNull();
    expect(key({ key: 'z' })).toBeNull();
  });

  it('ignores other combinations, including AltGr (Ctrl+Alt)', () => {
    expect(key({ key: 'F5', ctrlKey: true })).toBeNull();
    expect(key({ key: 'F5', altKey: true })).toBeNull();
    expect(key({ key: 'b', ctrlKey: true, altKey: true })).toBeNull();
    expect(key({ key: 'b', ctrlKey: true, shiftKey: true })).toBeNull();
    expect(key({ key: 's', metaKey: true })).toBeNull();
    expect(key({ key: 's' })).toBeNull();
    expect(key({ key: 'c', ctrlKey: true })).toBeNull();
  });
});

describe('installShortcuts', () => {
  it('runs the command of each shortcut and keeps the webview from acting on the key', () => {
    const { ctx, ran } = setUp();
    makeRunnable();
    uninstall = installShortcuts(window, ctx);

    const f5 = press({ key: 'F5' });
    press({ key: 'F5', shiftKey: true });
    press({ key: 'b', ctrlKey: true });
    press({ key: 's', ctrlKey: true });

    expect(ran).toEqual(['run.start', 'run.stop', 'build.start', 'project.save']);
    expect(f5.defaultPrevented).toBe(true);
  });

  it('acts before the focused element sees the key (the console would send it on)', () => {
    const { ctx, ran } = setUp();
    makeRunnable();
    uninstall = installShortcuts(window, ctx);
    const terminal = document.createElement('textarea');
    document.body.append(terminal);
    const seenByTerminal = vi.fn();
    terminal.addEventListener('keydown', seenByTerminal);

    press({ key: 's', ctrlKey: true }, terminal);
    expect(ran).toEqual(['project.save']);
    expect(seenByTerminal).not.toHaveBeenCalled();
  });

  it('leaves the keys to an element that asks for them', () => {
    const { ctx, ran } = setUp();
    uninstall = installShortcuts(window, ctx);
    const scope = document.createElement('div');
    scope.setAttribute(SHORTCUTS_OFF_ATTRIBUTE, 'off');
    const inner = document.createElement('textarea');
    scope.append(inner);
    document.body.append(scope);

    const event = press({ key: 's', ctrlKey: true }, inner);
    expect(ran).toEqual([]);
    expect(event.defaultPrevented).toBe(false);
  });

  it('does nothing while a dialog is open, but still stops a reload', () => {
    const { ctx, ran } = setUp();
    makeRunnable();
    uninstall = installShortcuts(window, ctx);
    const dialog = document.createElement('div');
    dialog.setAttribute('role', 'dialog');
    dialog.setAttribute('data-state', 'open');
    const input = document.createElement('input');
    dialog.append(input);
    document.body.append(dialog);

    expect(press({ key: 'F5' }, input).defaultPrevented).toBe(true);
    expect(press({ key: 's', ctrlKey: true }).defaultPrevented).toBe(true);
    expect(ran).toEqual([]);
  });

  it('ignores held keys and keys pressed while composing text', () => {
    const { ctx, ran } = setUp();
    makeRunnable();
    uninstall = installShortcuts(window, ctx);

    expect(press({ key: 'F5', repeat: true }).defaultPrevented).toBe(true);
    press({ key: 'F5', isComposing: true });
    press({ key: 'x' });
    expect(ran).toEqual([]);
  });

  it('finishes an edit in progress before Save, Build and Run, and does nothing when it cannot', () => {
    const { ctx, ran } = setUp();
    makeRunnable();
    uninstall = installShortcuts(window, ctx);
    let finishes = true;
    const finish = vi.fn(() => {
      ran.push('project.rename');
      return finishes;
    });
    const remove = registerPendingEdit(finish);
    try {
      press({ key: 's', ctrlKey: true });
      press({ key: 'b', ctrlKey: true });
      press({ key: 'F5' });
      expect(ran).toEqual([
        'project.rename',
        'project.save',
        'project.rename',
        'build.start',
        'project.rename',
        'run.start',
      ]);

      // It cannot be finished (a name that breaks a rule): the key does nothing else.
      ran.length = 0;
      finishes = false;
      expect(press({ key: 's', ctrlKey: true }).defaultPrevented).toBe(true);
      press({ key: 'F5' });
      expect(ran).toEqual(['project.rename', 'project.rename']);
      // Stop never waits for it.
      ran.length = 0;
      press({ key: 'F5', shiftKey: true });
      expect(ran).toEqual(['run.stop']);
    } finally {
      remove();
    }
    ran.length = 0;
    press({ key: 's', ctrlKey: true });
    expect(ran).toEqual(['project.save']);
  });

  it('stops listening when uninstalled', () => {
    const { ctx, ran } = setUp();
    installShortcuts(window, ctx)();

    press({ key: 's', ctrlKey: true });
    expect(ran).toEqual([]);
  });
});

describe('Undo and Redo for the block editor', () => {
  const undoKeys: [KeyboardEventInit, CommandId][] = [
    [{ key: 'z', ctrlKey: true }, 'edit.undo'],
    [{ key: 'y', ctrlKey: true }, 'edit.redo'],
    [{ key: 'Z', ctrlKey: true, shiftKey: true }, 'edit.redo'],
  ];

  it('undo and redo with the focus on the Problems panel, the toolbar or nothing', () => {
    const { ctx, ran } = setUp();
    showEditor();
    uninstall = installShortcuts(window, ctx);
    const grid = attach(document.createElement('div'));
    grid.setAttribute('role', 'grid');
    const cell = attach(document.createElement('div'), grid);
    cell.setAttribute('role', 'gridcell');
    cell.tabIndex = 0;
    const button = attach(document.createElement('button'));
    // The read-only code panel (CodeMirror marks its text read-only).
    const code = attach(document.createElement('div'));
    code.contentEditable = 'true';
    code.setAttribute('aria-readonly', 'true');

    for (const target of [cell, button, code, document.body]) {
      for (const [init, command] of undoKeys) {
        ran.length = 0;
        const event = press(init, target);
        expect(ran, `${command} at ${target.tagName}`).toEqual([command]);
        expect(event.defaultPrevented).toBe(true);
      }
    }
  });

  it('undo step by step while the key is held', () => {
    const { ctx, ran } = setUp();
    showEditor();
    uninstall = installShortcuts(window, ctx);
    press({ key: 'z', ctrlKey: true });
    press({ key: 'z', ctrlKey: true, repeat: true });
    expect(ran).toEqual(['edit.undo', 'edit.undo']);
  });

  it('leave the keys to text fields, which undo the typing', () => {
    const { ctx, ran } = setUp();
    showEditor();
    uninstall = installShortcuts(window, ctx);
    const input = attach(document.createElement('input'));
    const area = attach(document.createElement('textarea'));
    const editable = attach(document.createElement('div'));
    editable.contentEditable = 'true';
    const inside = attach(document.createElement('span'), editable);

    for (const target of [input, area, inside]) {
      for (const [init] of undoKeys) {
        expect(press(init, target).defaultPrevented).toBe(false);
      }
    }
    expect(ran).toEqual([]);
  });

  it('leave the keys to Blockly on the canvas, in the toolbox and in its pop-ups', () => {
    const { ctx, ran } = setUp();
    showEditor();
    uninstall = installShortcuts(window, ctx);
    const injection = attach(document.createElement('div'));
    injection.className = 'injectionDiv';
    const canvas = attach(document.createElementNS('http://www.w3.org/2000/svg', 'svg'), injection);
    const widget = attach(document.createElement('div'));
    widget.className = 'blocklyWidgetDiv';
    const menu = attach(document.createElement('div'));
    menu.className = 'blocklyDropDownDiv';
    const option = attach(document.createElement('div'), menu);
    const seen = vi.fn();
    canvas.addEventListener('keydown', seen);

    for (const target of [canvas, widget, option]) {
      expect(press({ key: 'z', ctrlKey: true }, target).defaultPrevented).toBe(false);
    }
    expect(ran).toEqual([]);
    expect(seen).toHaveBeenCalledOnce();
  });

  // The ≡ menu reads whether there is anything to undo as it opens: an undo behind it made its
  // Undo and Redo items wrong.
  it('do nothing in an open menu', () => {
    const { ctx, ran } = setUp();
    showEditor();
    uninstall = installShortcuts(window, ctx);
    const menu = attach(document.createElement('div'));
    menu.setAttribute('role', 'menu');
    const item = attach(document.createElement('div'), menu);
    item.setAttribute('role', 'menuitem');
    item.tabIndex = -1;
    for (const [init] of undoKeys) {
      expect(press(init, item).defaultPrevented).toBe(false);
    }
    expect(ran).toEqual([]);
    // The other shortcuts still work there.
    press({ key: 's', ctrlKey: true }, item);
    expect(ran).toEqual(['project.save']);
  });

  it('undo from the console only while no program runs; a running program gets the key', () => {
    const { ctx, ran } = setUp();
    showEditor();
    uninstall = installShortcuts(window, ctx);
    const terminal = attach(document.createElement('div'));
    terminal.className = 'xterm';
    const input = attach(document.createElement('textarea'), terminal);
    const seenByTerminal = vi.fn();
    input.addEventListener('keydown', seenByTerminal);

    expect(press({ key: 'z', ctrlKey: true }, input).defaultPrevented).toBe(true);
    expect(ran).toEqual(['edit.undo']);
    expect(seenByTerminal).not.toHaveBeenCalled();

    for (const status of ['starting', 'running'] as const) {
      useAppStore.getState().actions.setRun({ status });
      expect(press({ key: 'z', ctrlKey: true }, input).defaultPrevented).toBe(false);
    }
    expect(ran).toEqual(['edit.undo']);
    expect(seenByTerminal).toHaveBeenCalledTimes(2);

    useAppStore.getState().actions.setRun({ status: 'exited' });
    press({ key: 'y', ctrlKey: true }, input);
    expect(ran).toEqual(['edit.undo', 'edit.redo']);
  });

  it('do nothing in a dialog, an opted-out element, another page or without a project', () => {
    const { ctx, ran } = setUp();
    uninstall = installShortcuts(window, ctx);
    const button = attach(document.createElement('button'));

    // No project.
    expect(press({ key: 'z', ctrlKey: true }, button).defaultPrevented).toBe(false);
    // Another page than the block editor.
    useAppStore.getState().actions.setProject(projectFixture());
    useAppStore.getState().actions.setUi({ screen: 'settings' });
    expect(press({ key: 'z', ctrlKey: true }, button).defaultPrevented).toBe(false);

    showEditor();
    const scope = attach(document.createElement('div'));
    scope.setAttribute(SHORTCUTS_OFF_ATTRIBUTE, 'off');
    const inner = attach(document.createElement('button'), scope);
    expect(press({ key: 'z', ctrlKey: true }, inner).defaultPrevented).toBe(false);

    const dialog = attach(document.createElement('div'));
    dialog.setAttribute('role', 'dialog');
    dialog.setAttribute('data-state', 'open');
    const inDialog = attach(document.createElement('button'), dialog);
    for (const target of [inDialog, button]) {
      expect(press({ key: 'z', ctrlKey: true }, target).defaultPrevented).toBe(false);
    }
    expect(ran).toEqual([]);
  });
});

describe('keeping the page from reloading', () => {
  const reloadKeys: KeyboardEventInit[] = [
    { key: 'r', ctrlKey: true },
    { key: 'R', ctrlKey: true, shiftKey: true },
    { key: 'к', code: 'KeyR', ctrlKey: true },
    { key: 'F5', ctrlKey: true },
    { key: 'F5', ctrlKey: true, shiftKey: true },
  ];

  /** A right click on `target`. */
  function rightClick(target: EventTarget): MouseEvent {
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 });
    target.dispatchEvent(event);
    return event;
  }

  it('knows the reload keys, but not AltGr+R or other letters', () => {
    const key = (init: KeyboardEventInit) => isReloadKey(new KeyboardEvent('keydown', init));
    for (const init of reloadKeys) {
      expect(key(init)).toBe(true);
    }
    expect(key({ key: 'F5' })).toBe(true);
    expect(key({ key: 'F5', shiftKey: true })).toBe(true);
    expect(key({ key: 'r', ctrlKey: true, altKey: true })).toBe(false);
    expect(key({ key: 'r', metaKey: true })).toBe(false);
    expect(key({ key: 'r' })).toBe(false);
    expect(key({ key: 't', ctrlKey: true })).toBe(false);
  });

  it('cancels the reload keys everywhere, but still lets the focused element have them', () => {
    const { ctx, ran } = setUp();
    makeRunnable();
    uninstall = installShortcuts(window, ctx);
    const scope = document.createElement('div');
    scope.setAttribute(SHORTCUTS_OFF_ATTRIBUTE, 'off');
    const terminal = document.createElement('textarea');
    scope.append(terminal);
    const dialog = document.createElement('div');
    dialog.setAttribute('role', 'dialog');
    dialog.setAttribute('data-state', 'open');
    const field = document.createElement('input');
    dialog.append(field);
    document.body.append(scope, dialog);
    const seen: string[] = [];
    terminal.addEventListener('keydown', (event) => seen.push(event.key));

    for (const target of [document.body, terminal, field]) {
      for (const init of reloadKeys) {
        expect(press(init, target).defaultPrevented).toBe(true);
      }
    }
    // In an element that takes the keys itself, F5 reaches it but does not reload either.
    expect(press({ key: 'F5' }, terminal).defaultPrevented).toBe(true);
    expect(seen).toEqual(['r', 'R', 'к', 'F5', 'F5', 'F5']);
    expect(press({ key: 'r', ctrlKey: true, altKey: true }).defaultPrevented).toBe(false);
    expect(ran).toEqual([]);
  });

  it("cancels the webview's context menu, which offers Reload, but not app menus", () => {
    const { ctx } = setUp();
    uninstall = installShortcuts(window, ctx);
    const canvas = document.createElement('div');
    document.body.append(canvas);
    const appMenu = vi.fn();
    canvas.addEventListener('contextmenu', appMenu);

    expect(rightClick(canvas).defaultPrevented).toBe(true);
    expect(rightClick(document.body).defaultPrevented).toBe(true);
    // Blockly's own menu still sees the event.
    expect(appMenu).toHaveBeenCalledOnce();
  });

  it('keeps the context menu over editable text, the console and selected text', () => {
    const { ctx } = setUp();
    uninstall = installShortcuts(window, ctx);
    const input = document.createElement('input');
    const area = document.createElement('textarea');
    const editable = document.createElement('div');
    editable.contentEditable = 'true';
    const readOnly = document.createElement('div');
    readOnly.setAttribute('contenteditable', 'false');
    const terminal = document.createElement('div');
    terminal.className = 'xterm';
    const screen = document.createElement('div');
    terminal.append(screen);
    const code = document.createElement('p');
    code.textContent = 'int main() {}';
    document.body.append(input, area, editable, readOnly, terminal, code);

    for (const target of [input, area, editable, screen]) {
      expect(rightClick(target).defaultPrevented).toBe(false);
    }
    expect(rightClick(readOnly).defaultPrevented).toBe(true);
    expect(rightClick(code).defaultPrevented).toBe(true);

    // With the code's text selected, the menu offers Copy.
    const range = document.createRange();
    range.selectNodeContents(code);
    document.getSelection()?.removeAllRanges();
    document.getSelection()?.addRange(range);
    expect(keepsNativeContextMenu(new MouseEvent('contextmenu'))).toBe(false);
    expect(rightClick(code).defaultPrevented).toBe(false);
    // Elsewhere, even around the selection, the page's menu would open: cancelled.
    expect(rightClick(readOnly).defaultPrevented).toBe(true);
    expect(rightClick(document.body).defaultPrevented).toBe(true);
    document.getSelection()?.removeAllRanges();
  });

  it('stops cancelling once uninstalled', () => {
    const { ctx } = setUp();
    installShortcuts(window, ctx)();
    expect(press({ key: 'r', ctrlKey: true }).defaultPrevented).toBe(false);
    expect(rightClick(document.body).defaultPrevented).toBe(false);
  });
});

describe('activateGated', () => {
  it('runs the command when the gate is open', () => {
    const { ctx, ran } = setUp();
    makeRunnable();
    activateGated('run', ctx);
    activateGated('build', ctx);
    expect(ran).toEqual(['run.start', 'build.start']);
  });

  it('shows the first error instead, in both "Run on errors" modes', () => {
    const { ctx, ran } = setUp();
    makeRunnable();
    const { actions } = useAppStore.getState();
    actions.setAnalysis({ preview: previewFixture([diagnosticFixture()]) });

    activateGated('run', ctx);
    actions.setSettings({ value: settingsFixture({ run: { onErrors: 'showProblems' } }) });
    activateGated('run', ctx);
    activateGated('build', ctx);
    expect(ran).toEqual([
      'problems.focusFirstError',
      'problems.focusFirstError',
      'problems.focusFirstError',
    ]);
  });

  it('opens the toolchain setup page without a compiler, once a feature provides it', () => {
    const { ctx, screens, ran } = setUp();
    useAppStore.getState().actions.setProject(projectFixture());

    activateGated('run', ctx);
    expect(useAppStore.getState().ui.screen).toBe('start');

    screens.registerScreen('toolchainSetup', () => null);
    activateGated('run', ctx);
    expect(useAppStore.getState().ui.screen).toBe('toolchainSetup');
    expect(ran).toEqual([]);
  });

  it('does nothing in Restricted Mode or without a project', () => {
    const { ctx, ran } = setUp();
    activateGated('run', ctx);
    makeRunnable();
    useAppStore.getState().actions.updateProject({
      trust: {
        state: 'restricted',
        source: null,
        restrictedReason: 'noRecord',
        markOfTheWeb: true,
      },
    });
    activateGated('build', ctx);
    expect(ran).toEqual([]);
  });
});

describe("the shell's default commands", () => {
  it('problems.focusFirstError opens Problems, selects the block and focuses the panel', async () => {
    const commands = createCommandRegistry();
    const screens = createScreenRegistry();
    const editor = { selectBlock: vi.fn() };
    uninstall = installShellCommands({
      store: useAppStore,
      commands,
      screens,
      editor: () => editor as unknown as EditorHandle,
    });
    const panel = document.createElement('div');
    panel.dataset['dockPanel'] = 'problems';
    panel.tabIndex = 0;
    document.body.append(panel);
    const { actions } = useAppStore.getState();
    actions.setUi({ bottomCollapsed: true, bottomTab: 'console', screen: 'settings' });
    actions.setAnalysis({
      preview: previewFixture([
        diagnosticFixture({ primary: { block: 'b042', part: { kind: 'whole' } } }),
      ]),
    });

    await commands.runCommand('problems.focusFirstError');

    expect(useAppStore.getState().ui).toMatchObject({
      screen: 'editor',
      bottomTab: 'problems',
      bottomCollapsed: false,
      selection: 'b042',
    });
    expect(editor.selectBlock).toHaveBeenCalledWith('b042', { center: true });
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(panel);
    });
  });

  it('problems.focusFirstError without an error only opens Problems', async () => {
    const commands = createCommandRegistry();
    uninstall = installShellCommands({
      store: useAppStore,
      commands,
      screens: createScreenRegistry(),
      editor: () => null,
    });
    useAppStore.getState().actions.setUi({ selection: 'b001' });

    await commands.runCommand('problems.focusFirstError');
    expect(useAppStore.getState().ui).toMatchObject({ bottomTab: 'problems', selection: 'b001' });
  });

  it('settings.open shows the Settings page once a feature provides it', async () => {
    const commands = createCommandRegistry();
    const screens = createScreenRegistry();
    uninstall = installShellCommands({ store: useAppStore, commands, screens, editor: () => null });

    await commands.runCommand('settings.open');
    expect(useAppStore.getState().ui.screen).toBe('start');

    screens.registerScreen('settings', () => null);
    await commands.runCommand('settings.open');
    expect(useAppStore.getState().ui.screen).toBe('settings');
  });

  it('edit.undo and edit.redo undo and redo the block editor’s last change', async () => {
    const commands = createCommandRegistry();
    const editor = { undo: vi.fn(), redo: vi.fn() };
    let shown: EditorHandle | null = editor as unknown as EditorHandle;
    uninstall = installShellCommands({
      store: useAppStore,
      commands,
      screens: createScreenRegistry(),
      editor: () => shown,
    });

    await commands.runCommand('edit.undo');
    await commands.runCommand('edit.redo');
    await commands.runCommand('edit.undo');
    expect(editor.undo).toHaveBeenCalledTimes(2);
    expect(editor.redo).toHaveBeenCalledTimes(1);

    // Without an editor there is nothing to undo.
    shown = null;
    await commands.runCommand('edit.undo');
    expect(editor.undo).toHaveBeenCalledTimes(2);
  });

  it('can be overridden by a feature and removed again', async () => {
    const commands = createCommandRegistry();
    const remove = installShellCommands({
      store: useAppStore,
      commands,
      screens: createScreenRegistry(),
      editor: () => null,
    });
    const feature = vi.fn();
    commands.registerCommand('problems.focusFirstError', feature);
    await commands.runCommand('problems.focusFirstError');
    expect(feature).toHaveBeenCalledTimes(1);

    remove();
    expect(commands.hasCommand('settings.open')).toBe(false);
  });
});
