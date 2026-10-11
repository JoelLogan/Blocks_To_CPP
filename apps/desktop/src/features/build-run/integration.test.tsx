/**
 * The whole flow with the real console panel (xterm.js) and a fake backend, under a frozen
 * `Object.prototype` as in the shipped app (Tauri's `freezePrototype: true`, docs/spec/08-security.md
 * §8.8): Run builds, starts the program, writes its output into the terminal, acknowledges each
 * batch once xterm has processed it, sends what the person types, and shows the exit in the header.
 * Between runs the terminal keeps the earlier output and resets what a program left behind
 * (04 §4.5). Vitest runs every test file in its own module graph, so the freeze stays in this file.
 */
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { Terminal } from '@xterm/xterm';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { RecordedRun } from './testing';

Object.freeze(Object.prototype);

const { DockPanels } = await import('../../app/panels');
const { resetAppStore, useAppStore } = await import('../../app/store');
const { createBuildRunFeature } = await import('./feature');
const { consoleBridge } = await import('./consoleBridge');
const testing = await import('./testing');

const cleanups: (() => void)[] = [];

afterEach(() => {
  for (const cleanup of cleanups.splice(0)) {
    cleanup();
  }
  resetAppStore();
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

/** The console's separator line between two runs, as the terminal shows it. */
const SEPARATOR_LINE = '── New run ──';

/** The feature over a fake backend, with the real console panel and its terminal. */
interface Console {
  readonly backend: ReturnType<typeof testing.createFakeBackend>;
  readonly ctx: ReturnType<typeof testing.featureContext>;
  readonly terminal: Terminal;
}

/** Opens a project that can run and shows the Console tab with a fresh terminal. */
function showConsole(): Console {
  const open = vi.spyOn(Terminal.prototype, 'open');
  const backend = testing.createFakeBackend();
  testing.openRunnableProject();
  const ctx = testing.featureContext(backend.ipc);
  cleanups.push(createBuildRunFeature()(ctx));
  render(<div>{DockPanels().console}</div>);
  const terminal = open.mock.contexts.at(-1);
  if (!(terminal instanceof Terminal)) {
    throw new Error('the console did not open a terminal');
  }
  // The bridge is the app's own, shared by the tests in this file: nothing shown yet counts.
  consoleBridge.cleared();
  return { backend, ctx, terminal };
}

/** Lets promise callbacks and the timers that are due run. */
async function tick(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

/**
 * Presses ▶ Run (or ⟲ Run again): the first time the project is built, then the program starts.
 * Returns the program, which has started.
 */
async function startProgram(
  { backend, ctx }: Console,
  command: 'run.start' | 'run.again' = 'run.start',
): Promise<RecordedRun> {
  const builds = backend.builds.length;
  const runs = backend.runs.length;
  let running: Promise<void> = Promise.resolve();
  await act(async () => {
    running = ctx.commands.runCommand(command);
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  if (backend.builds.length > builds) {
    await act(async () => {
      backend.builds.at(-1)?.send({
        kind: 'finished',
        outcome: 'built',
        projectHash: 'a'.repeat(64),
        elapsedMs: 800,
      });
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
  }
  await eventually(() => {
    expect(backend.runs).toHaveLength(runs + 1);
  });
  await act(() => running);
  const program = backend.runs.at(-1);
  if (program === undefined) {
    throw new Error('no run');
  }
  act(() => {
    program.send({ kind: 'started', containment: 'cgroup', mode: 'pty', ideHelpers: false });
  });
  await eventually(() => {
    expect(useAppStore.getState().run.status).toBe('running');
  });
  return program;
}

/** The program writes `output` and ends; returns once the console shows the end. */
async function finishProgram(program: RecordedRun, output: readonly string[]): Promise<void> {
  act(() => {
    for (const text of output) {
      program.output(text);
    }
    program.send({
      kind: 'exit',
      afterSeq: output.length,
      elapsedMs: 10,
      status: { type: 'exited', code: 0 },
      crash: null,
      sanitizer: null,
      message: 'Finished (exit code 0)',
    });
  });
  // The exit is shown once the terminal has processed all of the output.
  await eventually(() => {
    expect(useAppStore.getState().run.status).toBe('exited');
  });
}

/** The console's lines with text, scrollback first, as the person sees them. */
function shownLines(): string[] {
  return consoleBridge
    .console()
    .screen()
    .lines.filter((line) => line !== '');
}

describe('the console between runs', () => {
  it('keeps the earlier run above the next one, scrolled or not, with or without a last newline', async () => {
    const env = showConsole();
    // More lines than the screen has rows, so part of the first run is in the scrollback.
    const first = Array.from({ length: env.terminal.rows + 6 }, (_, i) => `first ${String(i + 1)}`);
    await finishProgram(await startProgram(env), [
      `${first.join('\r\n')}\r\n`,
      'no newline at the end',
    ]);
    await finishProgram(await startProgram(env, 'run.again'), ['second\r\n']);
    await finishProgram(await startProgram(env), ['third\r\n']);

    expect(shownLines()).toEqual([
      ...first,
      'no newline at the end',
      SEPARATOR_LINE,
      'second',
      SEPARATOR_LINE,
      'third',
    ]);
    const shown = consoleBridge.console().screen();
    expect(shown.buffer).toBe('normal');
    expect(shown.lines[shown.cursor.row - 1]).toBe('third');
    expect(shown.cursor.column).toBe(0);
  });

  it('starts at the top without a separator after Clear', async () => {
    const env = showConsole();
    await finishProgram(await startProgram(env), ['old 1\r\nold 2\r\n']);
    fireEvent.click(screen.getByTestId('console-clear'));
    await tick();
    await finishProgram(await startProgram(env), ['new\r\n']);

    expect(consoleBridge.console().screen().lines[0]).toBe('new');
    expect(shownLines()).toEqual(['new']);
  });

  it('resets what a program left behind: the alternate screen, a hidden cursor, a scroll region and origin mode', async () => {
    const env = showConsole();
    await finishProgram(await startProgram(env), [
      'before\r\n',
      // A full-screen program that ends without restoring the terminal.
      '\u001b[?1049h\u001b[?25l\u001b[3;8r\u001b[?6h\u001b[Hdrawn on the alternate screen',
    ]);
    expect(consoleBridge.console().screen().buffer).toBe('alternate');

    // The next program asks the terminal for its modes (DECRQM) and its margins (DECRQSS); the
    // terminal answers as typed input.
    await finishProgram(await startProgram(env), [
      '\u001b[?1049$p\u001b[?25$p\u001b[?6$p\u001bP$qr\u001b\\',
      'after\r\n',
    ]);
    await eventually(() => {
      expect(env.backend.inputs().join('')).toBe(
        [
          '\u001b[?1049;2$y', // the normal screen is shown
          '\u001b[?25;1$y', // the cursor is shown
          '\u001b[?6;2$y', // origin mode is off
          `\u001bP1$r1;${String(env.terminal.rows)}r\u001b\\`, // the margins are the whole screen
        ].join(''),
      );
    });
    expect(env.terminal.modes.originMode).toBe(false);
    expect(consoleBridge.console().screen().buffer).toBe('normal');
    expect(shownLines()).toEqual(['before', SEPARATOR_LINE, 'after']);
  });
});
