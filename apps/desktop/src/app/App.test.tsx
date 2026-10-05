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
 * Blockly is replaced by a stand-in that records what the workspace component asks of it, so the
 * tests can check the wiring (options, theme, resizing, clean-up). src/test/blockly-environment
 * checks the same component against the real Blockly.
 */
const blockly = vi.hoisted(() => {
  const workspace = { setTheme: vi.fn(), dispose: vi.fn() };
  return {
    workspace,
    inject: vi.fn(() => workspace),
    svgResize: vi.fn(),
    setLocale: vi.fn(),
    Theme: { defineTheme: vi.fn((name: string) => ({ name })) },
    Themes: { Zelos: { name: 'zelos' } },
  };
});
vi.mock('blockly/core', () => blockly);
vi.mock('blockly/msg/en', () => ({}));

/** The colour-scheme media query, which the workspace module reads when it is first imported. */
const colourScheme = vi.hoisted(() => {
  const listeners = new Set<() => void>();
  const query = {
    matches: false,
    addEventListener: (_type: string, listener: () => void) => listeners.add(listener),
    removeEventListener: (_type: string, listener: () => void) => listeners.delete(listener),
  };
  window.matchMedia = () => query as unknown as MediaQueryList;
  return {
    listeners,
    /** Switches the system colour scheme, as the user would in the OS settings. */
    set(dark: boolean) {
      query.matches = dark;
      for (const listener of listeners) {
        listener();
      }
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
  FakeResizeObserver.instances = [];
  vi.stubGlobal('ResizeObserver', FakeResizeObserver);
});

afterEach(() => {
  for (const cleanup of cleanups.splice(0)) {
    cleanup();
  }
  colourScheme.set(false);
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
    expect(blockly.inject).toHaveBeenCalledTimes(1);

    setState(() => {
      useAppStore.getState().actions.setUi({ screen: 'editor' });
    });
    expect(screen.getByRole('main', { name: 'Block workspace' })).toBeTruthy();
    expect(blockly.workspace.dispose).not.toHaveBeenCalled();
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
  it('injects Blockly with the Zelos renderer, bundled media and no sounds', () => {
    renderApp();

    expect(blockly.inject).toHaveBeenCalledTimes(1);
    const [host, options] = blockly.inject.mock.calls[0] as unknown as [
      HTMLElement,
      Record<string, unknown>,
    ];
    expect(host.className).toBe('blockly-host');
    expect(screen.getByRole('main').contains(host)).toBe(true);
    expect(options).toMatchObject({
      renderer: 'zelos',
      sounds: false,
      theme: { name: 'b2c-light' },
      // Tests run like the dev server, which serves Blockly's media from the package.
      media: '/node_modules/blockly/media/',
    });
  });

  it('starts in the dark theme when the system uses a dark colour scheme', () => {
    colourScheme.set(true);
    renderApp();

    expect(blockly.inject.mock.calls[0]).toMatchObject([
      expect.anything(),
      { theme: { name: 'b2c-dark' } },
    ]);
  });

  it('follows changes of the system colour scheme', () => {
    renderApp();

    colourScheme.set(true);
    expect(blockly.workspace.setTheme).toHaveBeenLastCalledWith({ name: 'b2c-dark' });
    colourScheme.set(false);
    expect(blockly.workspace.setTheme).toHaveBeenLastCalledWith({ name: 'b2c-light' });
  });

  it('resizes the workspace when its panel changes size', () => {
    renderApp();

    const host = screen.getByRole('main').firstElementChild;
    const observer = FakeResizeObserver.instances.find(
      (instance) => host !== null && instance.observed.includes(host),
    );
    observer?.callback();
    expect(blockly.svgResize).toHaveBeenCalledWith(blockly.workspace);
  });

  it('releases the workspace and its listeners when the window closes', () => {
    const { unmount } = renderApp();
    expect(colourScheme.listeners.size).toBe(1);
    const host = screen.getByRole('main').firstElementChild;

    unmount();

    expect(blockly.workspace.dispose).toHaveBeenCalledTimes(1);
    const observer = FakeResizeObserver.instances.find(
      (instance) => host !== null && instance.observed.includes(host),
    );
    expect(observer?.disconnected).toBe(true);
    expect(colourScheme.listeners.size).toBe(0);
  });
});
