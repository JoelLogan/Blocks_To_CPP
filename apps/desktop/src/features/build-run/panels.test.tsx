/**
 * The Console and Build output tabs as src/app/panels.tsx connects them: the header follows the
 * run slice (state, exit, elapsed time, notices), its buttons run the commands, the terminal is
 * attached to the console bridge, and the Build output shows the build's lines. Also axe-core on
 * the console and its header.
 */
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { Terminal } from '@xterm/xterm';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { registerCommand } from '../../app/commands';
import { DockPanels } from '../../app/panels';
import { resetAppStore, type RunExitEvent, useAppStore } from '../../app/store';
import { projectFixture, settingsFixture } from '../../app/testing/fixtures';
import { expectNoAxeViolations } from '../../test/axe';
import { consoleBridge } from './consoleBridge';

const STOPPED_WITH_3: RunExitEvent = {
  kind: 'exit',
  afterSeq: 2,
  elapsedMs: 3_400,
  status: { type: 'exited', code: 3 },
  crash: null,
  sanitizer: null,
  message: 'Finished with exit code 3',
};

const cleanups: (() => void)[] = [];

function renderPanels() {
  const panels = DockPanels();
  return render(
    <div>
      <div data-testid="console-slot">{panels.console}</div>
      <div data-testid="build-output-slot">{panels.buildOutput}</div>
    </div>,
  );
}

function setState(update: () => void): void {
  act(update);
}

function header(): HTMLElement {
  return screen.getByTestId('console-header');
}

beforeEach(() => {
  resetAppStore();
});

afterEach(() => {
  for (const cleanup of cleanups.splice(0)) {
    cleanup();
  }
  vi.useRealTimers();
});

describe('the connected console', () => {
  it('shows notes without a project', () => {
    renderPanels();
    expect(screen.getByTestId('console-slot').textContent).toBe(
      "Your program's output will appear here.",
    );
    expect(screen.getByTestId('build-output-slot').textContent).toBe(
      "The compiler's messages will appear here.",
    );
    expect(consoleBridge.attached).toBe(false);
  });

  it('attaches the terminal to the console bridge while a project is open', async () => {
    useAppStore.getState().actions.setProject(projectFixture());
    const { unmount } = renderPanels();
    expect(consoleBridge.attached).toBe(true);
    expect(within(header()).getByTestId('console-state').textContent).toBe('Not running');

    await act(() => consoleBridge.write(new TextEncoder().encode('Hello from the program')));
    expect(screen.getByTestId('console-terminal').textContent).toContain('Hello from the program');
    expect(consoleBridge.used).toBe(true);

    fireEvent.click(within(header()).getByRole('button', { name: 'Clear' }));
    expect(consoleBridge.used).toBe(false);

    unmount();
    expect(consoleBridge.attached).toBe(false);
  });

  it('shows the running state, the elapsed time and the notices', () => {
    vi.useFakeTimers({ toFake: ['Date', 'setInterval', 'clearInterval'] });
    vi.setSystemTime(100_000);
    useAppStore.getState().actions.setProject(projectFixture());
    useAppStore.getState().actions.setRun({
      status: 'running',
      startedAt: 100_000 - 65_000,
      containment: 'processGroupOnly',
      ideHelpers: true,
    });
    renderPanels();

    expect(within(header()).getByTestId('console-state').textContent).toBe('▶ Running');
    expect(within(header()).getByTestId('console-elapsed').textContent).toBe('Elapsed 1:05');
    expect(header().textContent).toContain('Running with IDE helpers');
    expect(header().textContent).toContain('Process group only');

    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(within(header()).getByTestId('console-elapsed').textContent).toBe('Elapsed 1:07');
  });

  it('shows the exit, highlighted when the exit code is not 0', () => {
    useAppStore.getState().actions.setProject(projectFixture());
    renderPanels();
    setState(() => {
      useAppStore.getState().actions.setRun({ status: 'exited', exit: STOPPED_WITH_3 });
    });
    const state = within(header()).getByTestId('console-state');
    expect(state.textContent).toBe('⚠ Finished with exit code 3');
    expect(state.className).toContain('b2c-console-state-warning');
    expect(within(header()).getByTestId('console-elapsed').textContent).toBe('Elapsed 0:03');
  });

  it('runs ■ Stop and ⟲ Run again', () => {
    const stop = vi.fn();
    const again = vi.fn();
    cleanups.push(registerCommand('run.stop', stop), registerCommand('run.again', again));
    useAppStore.getState().actions.setProject(projectFixture());
    useAppStore.getState().actions.setRun({ status: 'running', startedAt: Date.now() });
    renderPanels();

    fireEvent.click(within(header()).getByRole('button', { name: 'Stop' }));
    fireEvent.click(within(header()).getByRole('button', { name: 'Run again' }));
    expect(stop).toHaveBeenCalledTimes(1);
    expect(again).toHaveBeenCalledTimes(1);
  });

  it('follows the run mode and the scrollback setting', () => {
    const open = vi.spyOn(Terminal.prototype, 'open');
    useAppStore.getState().actions.setProject(projectFixture());
    useAppStore.getState().actions.setSettings({
      value: settingsFixture({ console: { scrollbackLines: 2000 } }),
    });
    renderPanels();
    const terminal = open.mock.contexts.at(-1);
    if (!(terminal instanceof Terminal)) {
      throw new Error('the console did not open a terminal');
    }
    expect(terminal.options.scrollback).toBe(2000);
    expect(terminal.options.convertEol).toBe(false);
    setState(() => {
      consoleBridge.setMode('pipes');
    });
    expect(terminal.options.convertEol).toBe(true);
    setState(() => {
      consoleBridge.setMode('pty');
    });
    expect(terminal.options.convertEol).toBe(false);
  });

  it('has no accessibility problems, idle and running with notices', async () => {
    useAppStore.getState().actions.setProject(projectFixture());
    const { container } = renderPanels();
    await expectNoAxeViolations(container);
    setState(() => {
      useAppStore.getState().actions.setRun({
        status: 'running',
        startedAt: Date.now(),
        containment: 'processGroupOnly',
        ideHelpers: true,
      });
    });
    await expectNoAxeViolations(header());
    await expectNoAxeViolations(container);
    setState(() => {
      useAppStore.getState().actions.setRun({ status: 'exited', exit: STOPPED_WITH_3 });
    });
    await expectNoAxeViolations(container);
  });
});

describe('the connected Build output', () => {
  it('shows the build lines of the open project', () => {
    useAppStore.getState().actions.setProject(projectFixture());
    renderPanels();
    expect(screen.getByTestId('build-output-panel').textContent).toContain('No build output yet');
    setState(() => {
      useAppStore.getState().actions.appendBuildOutput([
        { kind: 'progress', text: 'Building Guessing Game (Debug)…' },
        { kind: 'note', text: 'B2C-T1011: Sanitizers were left out.' },
        { kind: 'raw', text: 'main.cpp:3:1: warning: unused' },
      ]);
    });
    expect(screen.getAllByTestId('build-output-line').map((line) => line.textContent)).toEqual([
      'Building Guessing Game (Debug)…',
      'ℹ B2C-T1011: Sanitizers were left out.',
      'main.cpp:3:1: warning: unused',
    ]);
  });

  it('has no accessibility problems', async () => {
    useAppStore.getState().actions.setProject(projectFixture());
    useAppStore
      .getState()
      .actions.appendBuildOutput([{ kind: 'progress', text: 'Built in 1.2 s.' }]);
    renderPanels();
    await expectNoAxeViolations(screen.getByTestId('build-output-slot'));
  });
});
