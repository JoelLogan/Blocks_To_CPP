import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { expectNoAxeViolations } from '../test/axe';
import { App } from './App';
import { commands } from './commands';
import { BOTTOM_DOCK, RIGHT_DOCK } from './layout/EditorLayout';
import { STEP } from './layout/Splitter';
import { screens } from './screens';
import { resetAppStore, useAppStore } from './store';
import {
  diagnosticFixture,
  previewFixture,
  projectFixture,
  settingsFixture,
  toolchainFixture,
} from './testing/fixtures';
import { HintProvider } from './ui/Hint';

/**
 * The block editor is replaced by a stand-in that records its mounts, so these tests check the shell
 * around it. src/editor/EditorWorkspace.test.tsx tests the editor itself against the real Blockly.
 */
const editor = vi.hoisted(() => ({ mounted: 0, unmounted: 0 }));
vi.mock('../editor/EditorWorkspace', async () => {
  const { useEffect } = await import('react');
  return {
    EditorWorkspace: function EditorWorkspace() {
      useEffect(() => {
        editor.mounted += 1;
        return () => {
          editor.unmounted += 1;
        };
      }, []);
      return <div className="blockly-host" />;
    },
  };
});

/** A ResizeObserver whose callback the test runs by hand (happy-dom computes no layout). */
class FakeResizeObserver {
  static instances: FakeResizeObserver[] = [];
  readonly callback: () => void;
  readonly observed: Element[] = [];
  disconnected = false;

  constructor(callback: () => void) {
    this.callback = callback;
    FakeResizeObserver.instances.push(this);
  }

  observe(element: Element): void {
    this.observed.push(element);
  }

  unobserve(): void {
    // Not needed by the tests.
  }

  disconnect(): void {
    this.disconnected = true;
  }
}

/** Registrations made by a test, removed after it. */
const cleanups: (() => void)[] = [];

function renderApp() {
  return render(
    <HintProvider>
      <App />
    </HintProvider>,
  );
}

/** A state in which Run is available. */
function openRunnableProject(): void {
  const { actions } = useAppStore.getState();
  actions.setProject(projectFixture());
  actions.setToolchains({ list: [toolchainFixture()] });
  actions.setSettings({ value: settingsFixture() });
}

function setState(update: () => void): void {
  act(update);
}

beforeEach(() => {
  resetAppStore();
  editor.mounted = 0;
  editor.unmounted = 0;
  FakeResizeObserver.instances = [];
  vi.stubGlobal('ResizeObserver', FakeResizeObserver);
});

afterEach(() => {
  for (const cleanup of cleanups.splice(0)) {
    cleanup();
  }
});

describe('the main window', () => {
  it('shows the regions of the main window, each with a name', () => {
    renderApp();

    expect(screen.getByRole('banner')).toBeTruthy();
    expect(screen.getByRole('main', { name: 'Block workspace' })).toBeTruthy();
    expect(screen.getByRole('complementary', { name: 'Generated C++' })).toBeTruthy();
    expect(screen.getByRole('contentinfo')).toBeTruthy();

    const dock = screen.getByRole('region', { name: 'Console, problems and build output' });
    const tabs = within(dock).getAllByRole('tab');
    expect(tabs.map((tab) => tab.textContent)).toEqual(['Console', 'Problems (0)', 'Build output']);
    expect(within(dock).getByRole('tabpanel').textContent).toBe(
      "Your program's output will appear here.",
    );
  });

  it('has no accessibility problems, with and without a project', async () => {
    const { container } = renderApp();
    await expectNoAxeViolations(container);

    setState(() => {
      openRunnableProject();
      useAppStore.getState().actions.updateProject({
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

  it('shows a page a feature provides in place of the editor, keeping the editor mounted', () => {
    cleanups.push(screens.registerScreen('start', () => <p>Welcome</p>));
    renderApp();

    expect(screen.getByRole('main', { name: 'Start page' }).textContent).toBe('Welcome');
    expect(screen.queryByRole('main', { name: 'Block workspace' })).toBeNull();
    expect(editor.mounted).toBe(1);

    setState(() => {
      useAppStore.getState().actions.setUi({ screen: 'editor' });
    });
    expect(screen.getByRole('main', { name: 'Block workspace' })).toBeTruthy();
    expect(editor.unmounted).toBe(0);
  });

  it('shows the editor for a page no feature provides yet', () => {
    useAppStore.getState().actions.setUi({ screen: 'settings' });
    renderApp();
    expect(screen.getByRole('main', { name: 'Block workspace' })).toBeTruthy();
  });
});

describe('the toolbar', () => {
  it('shows the app name without a project, and the project name with • when unsaved', () => {
    renderApp();
    expect(within(screen.getByRole('banner')).getByText('Blocks2Cpp')).toBeTruthy();

    setState(() => {
      useAppStore.getState().actions.setProject(projectFixture());
    });
    const name = screen.getByTestId('project-name');
    expect(name.textContent).toBe('Guessing Game');

    setState(() => {
      useAppStore.getState().actions.updateProject({ dirty: true });
    });
    expect(name.textContent).toBe('Guessing Game • (unsaved changes)');
    expect(within(name).getByText('•', { exact: false }).getAttribute('aria-hidden')).toBe('true');
  });

  it('disables Run and Build with a tooltip saying why', async () => {
    renderApp();
    const run = screen.getByTestId('toolbar-run');
    const build = screen.getByTestId('toolbar-build');

    expect(run.getAttribute('aria-disabled')).toBe('true');
    expect(run.getAttribute('aria-keyshortcuts')).toBe('F5');
    expect(build.getAttribute('aria-keyshortcuts')).toBe('Control+B');
    expect(run.textContent).toBe('►Run');
    // The reason is the button's description, for screen readers.
    expect(
      screen.getByRole('button', { description: 'Open or create a project first', name: 'Run' }),
    ).toBe(run);

    setState(openRunnableProject);
    setState(() => {
      useAppStore.getState().actions.setAnalysis({
        preview: previewFixture([diagnosticFixture(), diagnosticFixture({ code: 'B2C-E0301' })]),
      });
    });
    expect(run.getAttribute('aria-disabled')).toBe('true');
    act(() => {
      run.focus();
    });
    const tooltip = await screen.findByRole('tooltip');
    expect(tooltip.textContent).toBe('2 errors – click to see the first');
    await expectNoAxeViolations(screen.getByRole('banner'));
  });

  it('enables Run and Build when the gate is open', () => {
    setState(openRunnableProject);
    renderApp();
    const run = screen.getByTestId('toolbar-run');
    expect(run.getAttribute('aria-disabled')).toBe('false');
    expect(
      screen.getByRole('button', { name: 'Run', description: 'Build if needed, then run (F5)' }),
    ).toBe(run);
    expect(
      screen.getByRole('button', { name: 'Build', description: 'Build (Ctrl+B)' }),
    ).toBeTruthy();
  });

  it('runs the commands, or shows the first error, when pressed', () => {
    const ran: string[] = [];
    for (const id of [
      'run.start',
      'build.start',
      'run.stop',
      'problems.focusFirstError',
    ] as const) {
      cleanups.push(commands.registerCommand(id, () => void ran.push(id)));
    }
    setState(openRunnableProject);
    renderApp();

    fireEvent.click(screen.getByTestId('toolbar-run'));
    fireEvent.click(screen.getByTestId('toolbar-build'));
    fireEvent.click(screen.getByTestId('toolbar-stop'));
    expect(ran).toEqual(['run.start', 'build.start']);

    setState(() => {
      useAppStore.getState().actions.setRun({ status: 'running' });
    });
    fireEvent.click(screen.getByTestId('toolbar-stop'));
    setState(() => {
      useAppStore
        .getState()
        .actions.setAnalysis({ preview: previewFixture([diagnosticFixture()]) });
    });
    fireEvent.click(screen.getByTestId('toolbar-run'));
    expect(ran).toEqual(['run.start', 'build.start', 'run.stop', 'problems.focusFirstError']);
  });

  it('describes Stop for a program, a build and nothing', () => {
    renderApp();
    const stop = screen.getByTestId('toolbar-stop');
    expect(stop.getAttribute('aria-disabled')).toBe('true');
    expect(screen.getByRole('button', { name: 'Stop', description: 'Nothing is running' })).toBe(
      stop,
    );

    setState(() => {
      useAppStore.getState().actions.setBuild({ status: 'building' });
    });
    expect(stop.getAttribute('aria-disabled')).toBe('false');
    expect(
      screen.getByRole('button', { name: 'Stop', description: 'Stop the build (Shift+F5)' }),
    ).toBe(stop);

    setState(() => {
      useAppStore.getState().actions.setRun({ status: 'starting' });
    });
    expect(
      screen.getByRole('button', { name: 'Stop', description: 'Stop the program (Shift+F5)' }),
    ).toBe(stop);
  });

  it('keeps the Debug/Release choice for the session', () => {
    renderApp();
    const select = screen.getByRole<HTMLSelectElement>('combobox', { name: 'Build configuration' });
    expect(select.value).toBe('debug');

    fireEvent.change(select, { target: { value: 'release' } });
    expect(useAppStore.getState().build.config).toBe('release');
    expect(screen.getByTestId('status-config').textContent).toBe('Release');
  });

  it('shows Settings once a feature provides the page', () => {
    const open = vi.fn();
    cleanups.push(commands.registerCommand('settings.open', open));
    renderApp();
    expect(screen.queryByRole('button', { name: 'Settings' })).toBeNull();

    act(() => {
      cleanups.push(screens.registerScreen('settings', () => <p>Settings page</p>));
    });
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(open).toHaveBeenCalledTimes(1);
  });
});

describe('the status bar', () => {
  it('shows the toolchain, the standard, the configuration and the save state', () => {
    setState(openRunnableProject);
    renderApp();
    const bar = screen.getByRole('contentinfo');

    expect(screen.getByTestId('status-toolchain').textContent).toBe('√g++ 15.2.0 (MSYS2 UCRT64)');
    expect(within(bar).getByText('C++20')).toBeTruthy();
    expect(screen.getByTestId('status-config').textContent).toBe('Debug');
    expect(screen.getByTestId('status-save').textContent).toMatch(/^Saved \d\d:\d\d$/);
    expect(screen.queryByTestId('status-restricted')).toBeNull();
  });

  it('shows "No g++ found", or that it is still looking', () => {
    renderApp();
    expect(screen.getByTestId('status-toolchain').textContent).toBe('✖No g++ found');

    setState(() => {
      useAppStore.getState().actions.setToolchains({ discovering: true });
    });
    expect(screen.getByTestId('status-toolchain').textContent).toBe('Looking for g++…');
  });

  it('shows unsaved changes and Restricted Mode', () => {
    setState(() => {
      useAppStore.getState().actions.setProject(
        projectFixture({
          dirty: true,
          trust: {
            state: 'restricted',
            source: null,
            restrictedReason: 'changedOutside',
            markOfTheWeb: false,
          },
        }),
      );
    });
    renderApp();

    expect(screen.getByTestId('status-save').textContent).toBe('Unsaved');
    expect(screen.getByTestId('status-restricted').textContent).toBe('Restricted Mode');
  });

  it('links the toolchain to its page once a feature provides it', () => {
    cleanups.push(screens.registerScreen('toolchainSetup', () => <p>Set up g++</p>));
    useAppStore.getState().actions.setUi({ screen: 'editor' });
    renderApp();

    fireEvent.click(screen.getByRole('button', { name: /No g\+\+ found/ }));
    expect(useAppStore.getState().ui.screen).toBe('toolchainSetup');
    expect(screen.getByRole('main', { name: 'Set up a C++ compiler' }).textContent).toBe(
      'Set up g++',
    );
  });
});

describe('the docks', () => {
  it('switch tabs, and count the problems', () => {
    renderApp();
    const tab = screen.getByRole('tab', { name: 'Problems (0)' });
    fireEvent.mouseDown(tab);

    expect(useAppStore.getState().ui.bottomTab).toBe('problems');
    expect(screen.getByRole('tabpanel').textContent).toBe(
      'Problems in your blocks will be listed here.',
    );

    setState(() => {
      useAppStore.getState().actions.setAnalysis({
        preview: previewFixture([diagnosticFixture(), diagnosticFixture({ severity: 'warning' })]),
      });
    });
    expect(screen.getByRole('tab', { name: 'Problems (2)' })).toBe(tab);
  });

  it('collapse and expand, keeping their panels mounted', () => {
    renderApp();
    const hideBottom = screen.getByRole('button', { name: 'Hide the bottom panel' });
    expect(hideBottom.getAttribute('aria-expanded')).toBe('true');

    fireEvent.click(hideBottom);
    expect(useAppStore.getState().ui.bottomCollapsed).toBe(true);
    expect(screen.queryByRole('separator', { name: 'Resize the bottom panel' })).toBeNull();
    const showBottom = screen.getByRole('button', { name: 'Show the bottom panel' });
    expect(showBottom.getAttribute('aria-expanded')).toBe('false');

    // Choosing the shown tab again opens the dock.
    fireEvent.click(screen.getByRole('tab', { name: 'Console' }));
    expect(useAppStore.getState().ui.bottomCollapsed).toBe(false);

    fireEvent.click(screen.getByRole('button', { name: 'Hide the C++ panel' }));
    expect(useAppStore.getState().ui.rightCollapsed).toBe(true);
    expect(
      screen.getByRole('complementary', { name: 'Generated C++' }).querySelector('[hidden]'),
    ).not.toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Show the C++ panel' }));
    expect(useAppStore.getState().ui.rightCollapsed).toBe(false);
  });

  it('resize by keyboard, and collapse with Enter', () => {
    renderApp();
    const right = screen.getByRole('separator', { name: 'Resize the C++ panel' });
    const bottom = screen.getByRole('separator', { name: 'Resize the bottom panel' });
    expect(right.getAttribute('aria-valuenow')).toBe(String(RIGHT_DOCK.initial));
    expect(right.getAttribute('aria-orientation')).toBe('vertical');

    fireEvent.keyDown(right, { key: 'ArrowLeft' });
    expect(right.getAttribute('aria-valuenow')).toBe(String(RIGHT_DOCK.initial + STEP));
    fireEvent.keyDown(right, { key: 'End' });
    expect(right.getAttribute('aria-valuenow')).toBe(String(RIGHT_DOCK.max));
    fireEvent.keyDown(right, { key: 'Home' });
    expect(right.getAttribute('aria-valuenow')).toBe(String(RIGHT_DOCK.min));
    fireEvent.keyDown(right, { key: 'ArrowRight' });
    expect(right.getAttribute('aria-valuenow')).toBe(String(RIGHT_DOCK.min));

    fireEvent.keyDown(bottom, { key: 'ArrowUp', shiftKey: true });
    expect(bottom.getAttribute('aria-valuenow')).toBe(String(BOTTOM_DOCK.initial + 64));
    fireEvent.keyDown(bottom, { key: 'ArrowDown' });
    expect(bottom.getAttribute('aria-valuenow')).toBe(String(BOTTOM_DOCK.initial + 64 - STEP));
    fireEvent.keyDown(bottom, { key: 'x' });
    fireEvent.keyDown(bottom, { key: 'Enter' });
    expect(useAppStore.getState().ui.bottomCollapsed).toBe(true);
    fireEvent.keyDown(right, { key: 'Enter' });
    expect(useAppStore.getState().ui.rightCollapsed).toBe(true);
  });

  it('resize by pointer', () => {
    renderApp();
    const right = screen.getByRole('separator', { name: 'Resize the C++ panel' });
    right.setPointerCapture = vi.fn();
    right.hasPointerCapture = vi.fn(() => true);
    const releasePointerCapture = vi.fn();
    right.releasePointerCapture = releasePointerCapture;

    fireEvent.pointerMove(right, { clientX: 500 });
    expect(right.getAttribute('aria-valuenow')).toBe(String(RIGHT_DOCK.initial));
    fireEvent.pointerDown(right, { button: 2, clientX: 500 });
    fireEvent.pointerMove(right, { clientX: 400 });
    expect(right.getAttribute('aria-valuenow')).toBe(String(RIGHT_DOCK.initial));

    fireEvent.pointerDown(right, { button: 0, clientX: 500, pointerId: 1 });
    fireEvent.pointerMove(right, { clientX: 450, pointerId: 1 });
    expect(right.getAttribute('aria-valuenow')).toBe(String(RIGHT_DOCK.initial + 50));
    fireEvent.pointerUp(right, { pointerId: 1 });
    expect(releasePointerCapture).toHaveBeenCalledWith(1);
    fireEvent.pointerMove(right, { clientX: 300 });
    expect(right.getAttribute('aria-valuenow')).toBe(String(RIGHT_DOCK.initial + 50));

    const bottom = screen.getByRole('separator', { name: 'Resize the bottom panel' });
    bottom.setPointerCapture = vi.fn();
    bottom.hasPointerCapture = vi.fn(() => false);
    fireEvent.pointerDown(bottom, { button: 0, clientY: 600 });
    fireEvent.pointerMove(bottom, { clientY: 10 });
    expect(bottom.getAttribute('aria-valuenow')).toBe(String(BOTTOM_DOCK.max));
    fireEvent.pointerCancel(bottom);
    fireEvent.pointerMove(bottom, { clientY: 600 });
    expect(bottom.getAttribute('aria-valuenow')).toBe(String(BOTTOM_DOCK.max));
    fireEvent.pointerUp(bottom);
  });
});

describe('Block workspace', () => {
  it('is the block editor, in the workspace region, released when the window closes', () => {
    const { unmount } = renderApp();
    const host = screen
      .getByRole('main', { name: 'Block workspace' })
      .querySelector('.blockly-host');
    expect(host).not.toBeNull();
    expect(editor.mounted).toBe(1);

    unmount();
    expect(editor.unmounted).toBe(1);
  });
});
