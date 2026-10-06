/**
 * The accessibility pass over the window's chrome (docs/spec/04-user-interface.md §4.1, §4.7,
 * §4.8): axe-core on the toolbar, docks, status bar and dialogs in each of their states, the Tab
 * order through the window, dialogs that hold and return the focus, the focus coming back when the
 * editor is shown again, and the window shortcuts pressed from anywhere. The block editor is a
 * stand-in here; ./tabOrder.test.tsx goes through the real one.
 */
import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { expectNoAxeViolations } from '../../test/axe';
import { installShellCommands, SHELL } from '../actions';
import { App } from '../App';
import { commands } from '../commands';
import { createDialogQueue, DialogHost } from '../dialogs';
import { screens } from '../screens';
import { installShortcuts } from '../shortcuts';
import { resetAppStore, useAppStore } from '../store';
import {
  diagnosticFixture,
  previewFixture,
  projectFixture,
  settingsFixture,
  toolchainFixture,
} from '../testing/fixtures';
import { HintProvider } from '../ui/Hint';
import { focusIfLost, focusIsLost } from './focus';

vi.mock('../../editor/EditorWorkspace', () => ({
  EditorWorkspace: function EditorWorkspace() {
    return <div className="blockly-host" data-testid="editor-stand-in" />;
  },
}));

/** A ResizeObserver that never calls back (happy-dom lays nothing out). */
class StillResizeObserver {
  observe(): void {
    // Nothing to observe without layout.
  }
  unobserve(): void {
    // Nothing to stop.
  }
  disconnect(): void {
    // Nothing to stop.
  }
}

const cleanups: (() => void)[] = [];

/**
 * The one rule of app.css that decides what Tab can reach: hidden elements are not displayed.
 * happy-dom loads no style sheets, so the tests add it.
 */
function hideHiddenElements(): () => void {
  const style = document.createElement('style');
  style.textContent = '[hidden] { display: none !important; }';
  document.head.append(style);
  return () => {
    style.remove();
  };
}

beforeEach(() => {
  resetAppStore();
  vi.stubGlobal('ResizeObserver', StillResizeObserver);
  cleanups.push(hideHiddenElements());
});

afterEach(() => {
  for (const cleanup of cleanups.splice(0)) {
    cleanup();
  }
});

/** The project commands the main menu lists, and the pages the toolbar and status bar link to. */
function registerFeatures(): void {
  for (const id of [
    'project.new',
    'project.open',
    'project.save',
    'project.saveAs',
    'project.close',
    'run.start',
    'run.stop',
    'build.start',
  ] as const) {
    cleanups.push(commands.registerCommand(id, () => undefined));
  }
  cleanups.push(screens.registerScreen('settings', () => <p>Settings page</p>));
  cleanups.push(screens.registerScreen('toolchainSetup', () => <p>Toolchain page</p>));
}

function openRunnableProject(): void {
  const { actions } = useAppStore.getState();
  actions.setProject(projectFixture());
  actions.setToolchains({ list: [toolchainFixture()] });
  actions.setSettings({ value: settingsFixture() });
  actions.setUi({ screen: 'editor' });
}

function renderApp() {
  return render(
    <HintProvider>
      <App />
    </HintProvider>,
  );
}

/** An element's text as a screen reader reads it: without what is hidden from it. */
function spokenText(element: Element): string {
  const copy = element.cloneNode(true);
  if (!(copy instanceof Element)) {
    return '';
  }
  for (const hidden of copy.querySelectorAll('[aria-hidden="true"]')) {
    hidden.remove();
  }
  return copy.textContent;
}

/** What has the focus, as a person would name it: its role (or tag) and accessible name. */
function focusedName(): string {
  const element = document.activeElement;
  if (element === null || element === document.body) {
    return 'nothing';
  }
  const role = element.getAttribute('role') ?? element.tagName.toLowerCase();
  const labelledBy = element.getAttribute('aria-labelledby');
  const labelElement = labelledBy === null ? null : document.getElementById(labelledBy);
  const label =
    element.getAttribute('aria-label') ??
    (labelElement === null ? null : spokenText(labelElement)) ??
    spokenText(element);
  return `${role}: ${label.replace(/\s+/g, ' ').trim()}`;
}

describe('the window’s chrome has no accessibility problems', () => {
  it('while a program runs, with errors and a restricted project', async () => {
    registerFeatures();
    const { container } = renderApp();
    act(() => {
      openRunnableProject();
      const { actions } = useAppStore.getState();
      actions.setRun({ status: 'running', runId: 'rn_1', startedAt: Date.now() });
      actions.setBuild({ status: 'building' });
      actions.setAnalysis({ preview: previewFixture([diagnosticFixture()]) });
      actions.updateProject({
        dirty: true,
        trust: {
          state: 'restricted',
          source: null,
          restrictedReason: 'noRecord',
          markOfTheWeb: false,
        },
      });
    });
    await expectNoAxeViolations(container);
  });

  it('with both docks collapsed', async () => {
    registerFeatures();
    const { container } = renderApp();
    act(() => {
      openRunnableProject();
      useAppStore.getState().actions.setUi({ rightCollapsed: true, bottomCollapsed: true });
    });
    expect(screen.getByRole('button', { name: 'Show the C++ panel' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Show the bottom panel' })).toBeTruthy();
    await expectNoAxeViolations(container);
  });

  it.each(['console', 'problems', 'buildOutput'] as const)('with the %s tab shown', async (tab) => {
    registerFeatures();
    const { container } = renderApp();
    act(() => {
      openRunnableProject();
      useAppStore.getState().actions.setUi({ bottomTab: tab });
    });
    await expectNoAxeViolations(container);
  });

  it('with the main menu open', async () => {
    registerFeatures();
    const user = userEvent.setup();
    const { container } = renderApp();
    act(openRunnableProject);
    await user.click(screen.getByTestId('main-menu'));
    expect(screen.getByRole('menu')).toBeTruthy();
    await expectNoAxeViolations(container);
  });
});

describe('the Tab order of the window', () => {
  it('goes toolbar, workspace, C++ dock, bottom dock, status bar, and back', async () => {
    registerFeatures();
    const user = userEvent.setup();
    renderApp();
    act(openRunnableProject);

    const reached: string[] = [];
    for (let step = 0; step < 17; step++) {
      await user.tab();
      reached.push(focusedName());
    }
    expect(reached).toEqual([
      'button: Main menu',
      'select: DebugRelease',
      'button: Run',
      'button: Stop',
      'button: Build',
      'button: Settings',
      'separator: Resize the C++ panel',
      'button: Hide the C++ panel',
      'separator: Resize the bottom panel',
      'tab: Console',
      'button: Hide the bottom panel',
      // The shown tab's panel, then what is in it (Stop and Run again wait for a run).
      'tabpanel: Console',
      'button: Clear',
      'textarea: Terminal input',
      'button: g++ 15.2.0 (MSYS2 UCRT64) (open the toolchain page)',
      // Out of the page (to the webview), then round again.
      'nothing',
      'button: Main menu',
    ]);

    // Shift+Tab goes the same way back.
    await user.tab({ shift: true });
    await user.tab({ shift: true });
    expect(focusedName()).toBe('button: g++ 15.2.0 (MSYS2 UCRT64) (open the toolchain page)');
  });

  it('moves between the bottom tabs with the arrow keys, not with Tab', async () => {
    registerFeatures();
    const user = userEvent.setup();
    renderApp();
    act(openRunnableProject);
    const tabs = within(screen.getByRole('tablist', { name: 'Bottom panel' })).getAllByRole('tab');
    tabs[0]?.focus();
    await user.keyboard('{ArrowRight}');
    expect(document.activeElement).toBe(tabs[1]);
    expect(useAppStore.getState().ui.bottomTab).toBe('problems');
    await user.keyboard('{ArrowRight}');
    expect(document.activeElement).toBe(tabs[2]);
    await user.keyboard('{Home}');
    expect(document.activeElement).toBe(tabs[0]);
  });

  it('collapses a dock from its splitter and leaves the focus on its toggle', async () => {
    registerFeatures();
    const user = userEvent.setup();
    renderApp();
    act(openRunnableProject);
    screen.getByRole('separator', { name: 'Resize the bottom panel' }).focus();
    await user.keyboard('{Enter}');
    expect(focusedName()).toBe('button: Show the bottom panel');
    await user.keyboard('{Enter}');
    expect(useAppStore.getState().ui.bottomCollapsed).toBe(false);
  });
});

describe('dialogs', () => {
  it('take the focus, keep it inside, and give it back on Escape', async () => {
    const user = userEvent.setup();
    const queue = createDialogQueue();
    render(
      <>
        <button type="button">Before</button>
        <DialogHost queue={queue} />
      </>,
    );
    const before = screen.getByRole('button', { name: 'Before' });
    before.focus();

    let answer: Promise<boolean> | null = null;
    act(() => {
      answer = queue.confirm({ title: 'Delete it?', message: 'This cannot be undone.' });
    });
    const dialog = await screen.findByRole('dialog', { name: 'Delete it?' });
    expect(dialog.contains(document.activeElement)).toBe(true);
    expect(focusedName()).toBe('button: OK');
    // The dialog itself: Radix's focus guards around the page are hidden on purpose.
    await expectNoAxeViolations(dialog);

    // Tab and Shift+Tab stay inside.
    for (let step = 0; step < 4; step++) {
      await user.tab();
      expect(dialog.contains(document.activeElement)).toBe(true);
    }
    await user.tab({ shift: true });
    expect(dialog.contains(document.activeElement)).toBe(true);

    await user.keyboard('{Escape}');
    await expect(answer).resolves.toBe(false);
    expect(screen.queryByRole('dialog')).toBeNull();
    // Radix gives the focus back once the dialog has gone (after a timer).
    await waitFor(() => {
      expect(document.activeElement).toBe(before);
    });
  });

  it('give a prompt’s field the focus and announce why a value is refused', async () => {
    const user = userEvent.setup();
    const queue = createDialogQueue();
    render(<DialogHost queue={queue} />);
    let answer: Promise<string | null> | null = null;
    act(() => {
      answer = queue.prompt({
        message: 'New variable name:',
        defaultValue: 'x',
        validate: (value) => (value === '1' ? 'A name cannot start with a digit.' : null),
      });
    });
    const field = await screen.findByRole('textbox', { name: 'New variable name:' });
    expect(document.activeElement).toBe(field);
    await user.clear(field);
    await user.type(field, '1');
    expect(screen.getByRole('alert').textContent).toContain('A name cannot start with a digit.');
    expect(field.getAttribute('aria-invalid')).toBe('true');
    await expectNoAxeViolations(screen.getByRole('dialog'));
    await user.clear(field);
    await user.type(field, 'guess{Enter}');
    await expect(answer).resolves.toBe('guess');
  });
});

describe('the focus when the editor is shown again', () => {
  it('goes to the workspace region when what had it went away', () => {
    cleanups.push(screens.registerScreen('start', () => <button type="button">Back</button>));
    renderApp();
    act(() => {
      useAppStore.getState().actions.setUi({ screen: 'start' });
    });
    screen.getByRole('button', { name: 'Back' }).focus();
    act(() => {
      useAppStore.getState().actions.setUi({ screen: 'editor' });
    });
    expect(document.activeElement).toBe(screen.getByRole('main', { name: 'Block workspace' }));
  });

  it('stays where it is when that is still shown', () => {
    cleanups.push(screens.registerScreen('start', () => <p>Start</p>));
    registerFeatures();
    renderApp();
    act(() => {
      useAppStore.getState().actions.setUi({ screen: 'start' });
    });
    const settings = screen.getByRole('button', { name: 'Settings' });
    settings.focus();
    act(() => {
      useAppStore.getState().actions.setUi({ screen: 'editor' });
    });
    expect(document.activeElement).toBe(settings);
  });

  it('is lost only on nothing, the body, or something hidden or gone', () => {
    const shown = document.createElement('button');
    const hidden = document.createElement('div');
    hidden.hidden = true;
    const inside = document.createElement('button');
    hidden.append(inside);
    document.body.append(shown, hidden);
    document.body.focus();
    expect(focusIsLost()).toBe(true);
    shown.focus();
    expect(focusIsLost()).toBe(false);
    expect(focusIfLost(inside)).toBe(false);
    inside.focus();
    expect(focusIsLost()).toBe(true);
    expect(focusIfLost(null)).toBe(false);
    expect(focusIfLost(shown)).toBe(true);
    shown.remove();
    hidden.remove();
  });
});

describe('the window shortcuts from anywhere in the window', () => {
  it('F5, Shift+F5, Ctrl+B and Ctrl+S run their commands from a dock tab and a panel', async () => {
    const ran: string[] = [];
    for (const id of ['run.start', 'run.stop', 'build.start', 'project.save'] as const) {
      cleanups.push(
        commands.registerCommand(id, () => {
          ran.push(id);
        }),
      );
    }
    cleanups.push(installShellCommands(SHELL));
    cleanups.push(installShortcuts(window, SHELL));
    const user = userEvent.setup();
    renderApp();
    act(openRunnableProject);

    within(screen.getByRole('tablist', { name: 'Bottom panel' }))
      .getAllByRole('tab')[0]
      ?.focus();
    await user.keyboard('{F5}');
    await user.keyboard('{Shift>}{F5}{/Shift}');
    screen.getByRole('button', { name: 'Hide the C++ panel' }).focus();
    await user.keyboard('{Control>}b{/Control}');
    await user.keyboard('{Control>}s{/Control}');
    expect(ran).toEqual(['run.start', 'run.stop', 'build.start', 'project.save']);
  });
});
