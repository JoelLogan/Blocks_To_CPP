/** The window's shortcuts and what the shell's buttons do (actions.ts). */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { activateGated, installShellCommands, type ShellActionContext } from './actions';
import { type CommandId, createCommandRegistry } from './commands';
import type { EditorHandle } from './editor-types';
import { createScreenRegistry } from './screens';
import { installShortcuts, matchShortcut, SHORTCUTS_OFF_ATTRIBUTE } from './shortcuts';
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
  ] as const) {
    commands.registerCommand(id, () => {
      ran.push(id);
    });
  }
  return { ctx, commands, screens, editor, ran };
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

  it('stops listening when uninstalled', () => {
    const { ctx, ran } = setUp();
    installShortcuts(window, ctx)();

    press({ key: 's', ctrlKey: true });
    expect(ran).toEqual([]);
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
