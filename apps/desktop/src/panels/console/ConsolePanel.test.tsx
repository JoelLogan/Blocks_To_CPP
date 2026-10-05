import { Terminal } from '@xterm/xterm';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { createRef } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { expectNoAxeViolations } from '../../test/axe';
import type { RunExit } from '../types';
import {
  ConsolePanel,
  type ConsoleHandle,
  type ConsoleHeader,
  type ConsolePanelProps,
  clampScrollback,
} from './ConsolePanel';

const IDLE: ConsoleHeader = { state: 'idle', exit: null, elapsedMs: 0, notices: [] };
const RUNNING: ConsoleHeader = {
  state: 'running',
  exit: null,
  elapsedMs: 4_200,
  notices: ['ideHelpers', 'processGroupOnly'],
};

function exitHeader(status: RunExit['status'], message: string): ConsoleHeader {
  return {
    state: 'exited',
    exit: {
      kind: 'exit',
      afterSeq: 1,
      elapsedMs: 900,
      status,
      crash: null,
      sanitizer: null,
      message,
    },
    elapsedMs: 900,
    notices: [],
  };
}

/** Watches `Terminal.prototype.open` (calling through), to find the terminal a panel opened. */
let openSpy: ReturnType<typeof spyOnOpen>;

function spyOnOpen() {
  return vi.spyOn(Terminal.prototype, 'open');
}

beforeEach(() => {
  openSpy = spyOnOpen();
});

afterEach(() => {
  vi.useRealTimers();
});

function renderConsole(overrides: Partial<ConsolePanelProps> = {}) {
  const handleRef = createRef<ConsoleHandle>();
  const props: ConsolePanelProps = {
    header: RUNNING,
    scrollbackLines: 10_000,
    onStop: vi.fn(),
    onRunAgain: vi.fn(),
    onClear: vi.fn(),
    ...overrides,
  };
  const result = render(<ConsolePanel {...props} ref={handleRef} />);
  const handle = handleRef.current;
  const terminal = openSpy.mock.contexts.at(-1);
  if (handle === null || !(terminal instanceof Terminal)) {
    throw new Error('the console did not mount');
  }
  return { ...result, props, handle, terminal };
}

/** The terminal's buffer as text, one string per line, without the cells nothing was written to. */
function lines(terminal: Terminal): string[] {
  const buffer = terminal.buffer.active;
  const result: string[] = [];
  for (let y = 0; y < buffer.length; y++) {
    result.push(buffer.getLine(y)?.translateToString(true) ?? '');
  }
  return result;
}

const encode = (text: string) => new TextEncoder().encode(text);

describe('ConsolePanel', () => {
  it('writes program output and resolves once xterm has processed it', async () => {
    const { handle, terminal } = renderConsole();
    await act(() => handle.write(encode('Guess a number from 1 to 100!\r\nYour guess: ')));
    expect(lines(terminal).slice(0, 2)).toEqual(['Guess a number from 1 to 100!', 'Your guess: ']);
  });

  it('writes to the terminal at most 60 times a second however fast output arrives', async () => {
    vi.useFakeTimers();
    const writeSpy = vi.spyOn(Terminal.prototype, 'write');
    const { handle, terminal } = renderConsole();

    // A byte source that delivers a batch every millisecond for one second.
    const acknowledged: Promise<void>[] = [];
    for (let ms = 0; ms < 1000; ms++) {
      acknowledged.push(handle.write(encode('.')));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1);
      });
    }
    const inFirstSecond = writeSpy.mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(200);
    });
    await Promise.all(acknowledged);

    expect(inFirstSecond).toBeGreaterThan(10);
    expect(inFirstSecond).toBeLessThanOrEqual(60);
    expect(lines(terminal).join('')).toBe('.'.repeat(1000));
  });

  it('keeps at most the configured scrollback', async () => {
    const { handle, terminal, rerender, props } = renderConsole({ scrollbackLines: 1000 });
    const text = Array.from({ length: 3000 }, (_, i) => `line ${String(i)}`).join('\r\n');
    await act(() => handle.write(encode(text)));
    expect(terminal.options.scrollback).toBe(1000);
    expect(terminal.buffer.active.length).toBeLessThanOrEqual(1000 + terminal.rows);
    expect(lines(terminal).at(-1)).toBe('line 2999');

    rerender(<ConsolePanel {...props} scrollbackLines={250_000} />);
    expect(terminal.options.scrollback).toBe(100_000);
  });

  it('clamps the scrollback setting', () => {
    expect(clampScrollback(10)).toBe(1000);
    expect(clampScrollback(5000.4)).toBe(5000);
    expect(clampScrollback(1e9)).toBe(100_000);
    expect(clampScrollback(Number.NaN)).toBe(10_000);
  });

  it('writes the lines-skipped marker in order with the output', async () => {
    const { handle, terminal } = renderConsole();
    void handle.write(encode('before'));
    handle.writeSkipped(1_204_331);
    handle.writeSkipped(0);
    await act(() => handle.write(encode('after')));
    expect(lines(terminal).slice(0, 3)).toEqual(['before', ' … 1,204,331 lines skipped ', 'after']);
  });

  it('ignores clipboard writes by escape sequence (OSC 52)', async () => {
    const writeText = vi.spyOn(navigator.clipboard, 'writeText');
    const clipboardWrite = vi.spyOn(navigator.clipboard, 'write');
    const { handle, terminal } = renderConsole();
    // "secret" in base64, asked to go to the clipboard, then ordinary text.
    await act(() => handle.write(encode('\u001b]52;c;c2VjcmV0\u0007visible')));
    expect(writeText).not.toHaveBeenCalled();
    expect(clipboardWrite).not.toHaveBeenCalled();
    expect(lines(terminal)[0]).toBe('visible');
  });

  it('shows an http link only after activation, in full, and opens nothing', async () => {
    const open = vi.spyOn(window, 'open').mockReturnValue(null);
    const writeText = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue();
    const { handle, terminal } = renderConsole();
    await act(() =>
      handle.write(encode('\u001b]8;;https://example.com/docs?x=1\u0007the docs\u001b]8;;\u0007')),
    );
    expect(lines(terminal)[0]).toBe('the docs');
    expect(screen.queryByRole('dialog')).toBeNull();

    const linkHandler = terminal.options.linkHandler;
    expect(linkHandler).toBeDefined();
    act(() => {
      linkHandler?.activate(new MouseEvent('click'), 'https://example.com/docs?x=1', {
        start: { x: 1, y: 1 },
        end: { x: 8, y: 1 },
      });
    });
    const dialog = screen.getByRole('dialog', { name: 'Link from your program' });
    expect(screen.getByTestId('link-dialog-url').textContent).toBe('https://example.com/docs?x=1');
    expect(open).not.toHaveBeenCalled();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Copy link' }));
      await Promise.resolve();
    });
    expect(writeText).toHaveBeenCalledWith('https://example.com/docs?x=1');
    expect(screen.getByText('Link copied')).toBeDefined();
    await expectNoAxeViolations(dialog);

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(open).not.toHaveBeenCalled();
  });

  it('refuses links that are not http or https', () => {
    const { terminal } = renderConsole();
    expect(terminal.options.linkHandler?.allowNonHttpProtocols).toBe(false);
    act(() => {
      const script = ['java', 'script:alert(1)'].join('');
      terminal.options.linkHandler?.activate(new MouseEvent('click'), script, {
        start: { x: 1, y: 1 },
        end: { x: 2, y: 1 },
      });
    });
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('passes typing and size changes to its subscribers and reports its size', () => {
    const { handle, terminal } = renderConsole();
    const typed = vi.fn();
    const resized = vi.fn();
    const stopTyping = handle.onData(typed);
    handle.onResize(resized);

    terminal.input('42\r', true);
    expect(typed).toHaveBeenCalledWith('42\r');
    stopTyping();
    terminal.input('7', true);
    expect(typed).toHaveBeenCalledTimes(1);

    terminal.resize(100, 30);
    expect(resized).toHaveBeenCalledWith({ cols: 100, rows: 30 });
    expect(handle.size()).toEqual({ cols: 100, rows: 30 });
  });

  it('accepts typing only while the program runs', () => {
    const { terminal, rerender, props } = renderConsole({ header: IDLE });
    expect(terminal.options.disableStdin).toBe(true);
    rerender(<ConsolePanel {...props} header={RUNNING} />);
    expect(terminal.options.disableStdin).toBe(false);
  });

  it('converts bare line feeds only in pipe mode', () => {
    const { terminal, rerender, props } = renderConsole();
    expect(terminal.options.convertEol).toBe(false);
    rerender(<ConsolePanel {...props} mode="pipes" />);
    expect(terminal.options.convertEol).toBe(true);
  });

  it('shows the running state, elapsed time and notices, with Stop enabled', () => {
    const { props } = renderConsole();
    expect(screen.getByTestId('console-state').textContent).toBe('▶ Running');
    expect(screen.getByTestId('console-elapsed').textContent).toBe('Elapsed 0:04');
    expect(screen.getByText('Running with IDE helpers')).toBeDefined();
    expect(screen.getByText('Process group only')).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }));
    expect(props.onStop).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole('button', { name: 'Run again' }));
    expect(props.onRunAgain).toHaveBeenCalledTimes(1);
  });

  it('shows the header text for each kind of exit, highlighting a non-zero exit', () => {
    const { rerender, props } = renderConsole();
    const cases: [ConsoleHeader, string, string][] = [
      [
        exitHeader({ type: 'exited', code: 0 }, 'Finished (exit code 0)'),
        '✓ Finished (exit code 0)',
        'ok',
      ],
      [
        exitHeader({ type: 'exited', code: 3 }, 'Finished with exit code 3'),
        '⚠ Finished with exit code 3',
        'warning',
      ],
      [exitHeader({ type: 'stopped' }, 'Stopped'), '■ Stopped', 'neutral'],
      [
        exitHeader({ type: 'signaled', signal: 8 }, 'Crashed: integer division by zero (SIGFPE)'),
        '✖ Crashed: integer division by zero (SIGFPE)',
        'error',
      ],
      [
        exitHeader(
          { type: 'exception', ntstatus: 0xc00000fd },
          'Crashed: stack overflow, probably infinite recursion (exception 0xC00000FD)',
        ),
        '✖ Crashed: stack overflow, probably infinite recursion (exception 0xC00000FD)',
        'error',
      ],
      [IDLE, 'Not running', 'neutral'],
    ];
    for (const [header, text, tone] of cases) {
      rerender(<ConsolePanel {...props} header={header} />);
      const state = screen.getByTestId('console-state');
      expect(state.textContent).toBe(text);
      expect(state.className).toContain(`b2c-console-state-${tone}`);
    }
    // Idle: nothing to stop or run again, and no elapsed time.
    expect(screen.getByRole('button', { name: 'Stop' })).toHaveProperty('disabled', true);
    expect(screen.getByRole('button', { name: 'Run again' })).toHaveProperty('disabled', true);
    expect(screen.queryByTestId('console-elapsed')).toBeNull();
  });

  it('clears the terminal and tells its owner', async () => {
    const { handle, terminal, props } = renderConsole();
    await act(() => handle.write(encode('one\r\ntwo\r\nthree')));
    fireEvent.click(screen.getByRole('button', { name: 'Clear' }));
    expect(props.onClear).toHaveBeenCalledTimes(1);
    // xterm keeps the cursor's line and drops everything above it.
    expect(lines(terminal).filter((line) => line !== '')).toEqual(['three']);
  });

  it('resolves pending writes when it unmounts', async () => {
    vi.useFakeTimers();
    const { handle, unmount } = renderConsole();
    const pending = handle.write(encode('never shown'));
    unmount();
    await expect(pending).resolves.toBeUndefined();
  });

  it('has no accessibility violations', async () => {
    const { container } = renderConsole();
    await expectNoAxeViolations(container);
  });
});
