/**
 * The whole flow with the real console panel (xterm.js) and a fake backend, under a frozen
 * `Object.prototype` as in the shipped app (Tauri's `freezePrototype: true`, docs/spec/08-security.md
 * §8.8): Run builds, starts the program, writes its output into the terminal, acknowledges each
 * batch once xterm has processed it, sends what the person types, and shows the exit in the header.
 * Vitest runs every test file in its own module graph, so the freeze stays in this file.
 */
import { act, render, screen, within } from '@testing-library/react';
import { Terminal } from '@xterm/xterm';
import { afterEach, describe, expect, it, vi } from 'vitest';

Object.freeze(Object.prototype);

const { DockPanels } = await import('../../app/panels');
const { useAppStore } = await import('../../app/store');
const { createBuildRunFeature } = await import('./feature');
const { consoleBridge } = await import('./consoleBridge');
const testing = await import('./testing');

const cleanups: (() => void)[] = [];

afterEach(() => {
  for (const cleanup of cleanups.splice(0)) {
    cleanup();
  }
});

/** Waits until `check` passes (xterm parses its writes in timers). */
async function eventually(check: () => void): Promise<void> {
  for (let attempt = 0; ; attempt++) {
    try {
      check();
      return;
    } catch (error) {
      if (attempt >= 100) {
        throw error;
      }
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 10));
      });
    }
  }
}

describe('build and run with the real console', () => {
  it('runs the program in the terminal, acknowledges its output and shows the exit', async () => {
    expect(Object.isFrozen(Object.prototype)).toBe(true);
    const open = vi.spyOn(Terminal.prototype, 'open');
    const backend = testing.createFakeBackend();
    testing.openRunnableProject();
    const ctx = testing.featureContext(backend.ipc);
    cleanups.push(createBuildRunFeature()(ctx));

    const panels = DockPanels();
    render(<div>{panels.console}</div>);
    const terminal = open.mock.contexts.at(-1);
    if (!(terminal instanceof Terminal)) {
      throw new Error('the console did not open a terminal');
    }
    expect(consoleBridge.attached).toBe(true);

    let running: Promise<void> = Promise.resolve();
    await act(async () => {
      // The command ends once the program has started, after the build.
      running = ctx.commands.runCommand('run.start');
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(backend.builds).toHaveLength(1);
    await act(async () => {
      backend.builds[0]?.send({
        kind: 'finished',
        outcome: 'built',
        projectHash: 'a'.repeat(64),
        elapsedMs: 800,
      });
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    await eventually(() => {
      expect(backend.runs).toHaveLength(1);
    });
    await act(() => running);
    const program = backend.runs[0];
    if (program === undefined) {
      throw new Error('no run');
    }

    act(() => {
      program.send({
        kind: 'started',
        containment: 'processGroupOnly',
        mode: 'pty',
        ideHelpers: true,
      });
    });
    const header = screen.getByTestId('console-header');
    expect(within(header).getByTestId('console-state').textContent).toBe('▶ Running');
    expect(header.textContent).toContain('Process group only');

    act(() => {
      program.output('Guess a number from 1 to 100!\r\n');
      program.output('Your guess: ');
    });
    await eventually(() => {
      expect(backend.ipc.runAck).toHaveBeenLastCalledWith({ runId: program.runId, seq: 2 });
    });
    expect(screen.getByTestId('console-terminal').textContent).toContain('Your guess:');

    act(() => {
      terminal.input('50\r', true);
    });
    await eventually(() => {
      expect(backend.inputs()).toEqual(['50\r']);
    });

    act(() => {
      program.output('Correct!\r\n');
      program.send({
        kind: 'exit',
        afterSeq: 3,
        elapsedMs: 4200,
        status: { type: 'exited', code: 0 },
        crash: null,
        sanitizer: null,
        message: 'Finished (exit code 0)',
      });
    });
    await eventually(() => {
      expect(within(header).getByTestId('console-state').textContent).toBe(
        '✓ Finished (exit code 0)',
      );
    });
    expect(screen.getByTestId('console-terminal').textContent).toContain('Correct!');
    expect(useAppStore.getState().run.status).toBe('exited');
  });
});
