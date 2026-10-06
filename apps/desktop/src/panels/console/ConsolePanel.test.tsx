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
  LEAVE_CONSOLE_KEYS,
  clampScrollback,
  consoleKeyAction,
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
    void handle.write(encode('middle'));
    // Part of one very long line was dropped: no line break, but output is still missing.
    handle.writeSkipped(0);
    handle.writeSkipped(-1);
    await act(() => handle.write(encode('after')));
    expect(lines(terminal).slice(0, 5)).toEqual([
      'before',
      ' … 1,204,331 lines skipped ',
      'middle',
      ' … output skipped ',
      'after',
    ]);
  });

  it('starts afresh on a reset: queued output is dropped and the modes a program set are gone', async () => {
    const { handle, terminal } = renderConsole();
    // The old program switched to the alternate screen and bracketed paste, and printed.
    await act(() => handle.write(encode('old\r\n\u001b[?1049h\u001b[?2004hfull screen')));
    expect(terminal.buffer.active.type).toBe('alternate');
    expect(terminal.modes.bracketedPasteMode).toBe(true);
    const dropped = handle.write(encode('still queued'));

    handle.reset();
    await expect(dropped).resolves.toBeUndefined();
    await act(() => handle.write(encode('new')));
    expect(terminal.buffer.active.type).toBe('normal');
    expect(terminal.modes.bracketedPasteMode).toBe(false);
    expect(lines(terminal).filter((line) => line !== '')).toEqual(['new']);
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
    // Each notice opens its explanation from its summary, by keyboard as by pointer.
    const notices = screen.getAllByTestId('console-notice');
    expect(notices.map((notice) => notice.tagName)).toEqual(['DETAILS', 'DETAILS']);
    expect(notices[0]?.querySelector('summary')?.textContent).toBe('Running with IDE helpers');
    expect(notices[0]?.textContent).toContain('the C++ you see is unchanged');
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

/** The terminal's input element, which has the keyboard focus while the console has it. */
function terminalInput(container: HTMLElement): HTMLTextAreaElement {
  const input = container.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea');
  if (input === null) {
    throw new Error('the terminal has no input element');
  }
  return input;
}

/** Dispatches a Tab key event (keydown unless `type` says otherwise) and returns it. */
function pressTab(
  target: HTMLElement,
  init: { type?: string; shiftKey?: boolean; ctrlKey?: boolean } = {},
): KeyboardEvent {
  const { type = 'keydown', ...modifiers } = init;
  const event = new KeyboardEvent(type, {
    key: 'Tab',
    code: 'Tab',
    keyCode: 9,
    bubbles: true,
    cancelable: true,
    ...modifiers,
  });
  target.dispatchEvent(event);
  return event;
}

describe('ConsolePanel keyboard (never a keyboard trap, WCAG 2.1.2)', () => {
  it('lets Tab and Shift+Tab move the focus while no program runs', () => {
    const exited = exitHeader({ type: 'exited', code: 0 }, 'Finished (exit code 0)');
    for (const header of [IDLE, exited]) {
      const { container, handle, unmount } = renderConsole({ header });
      const typed = vi.fn();
      handle.onData(typed);
      const input = terminalInput(container);
      input.focus();
      // Not cancelled: the browser moves the focus as for any other control.
      expect(pressTab(input).defaultPrevented).toBe(false);
      expect(pressTab(input, { shiftKey: true }).defaultPrevented).toBe(false);
      expect(typed).not.toHaveBeenCalled();
      unmount();
    }
  });

  it('sends Tab to a running program, and Ctrl+Tab moves the focus to the header', () => {
    const { container, handle } = renderConsole({ header: RUNNING });
    const typed = vi.fn();
    handle.onData(typed);
    const input = terminalInput(container);
    input.focus();

    expect(pressTab(input).defaultPrevented).toBe(true);
    expect(pressTab(input, { shiftKey: true }).defaultPrevented).toBe(true);
    expect(typed.mock.calls).toEqual([['\t'], ['\u001b[Z']]);
    expect(document.activeElement).toBe(input);

    // Only the keydown moves the focus; the keyup that follows it changes nothing.
    pressTab(input, { type: 'keyup', ctrlKey: true });
    expect(document.activeElement).toBe(input);
    const leave = pressTab(input, { ctrlKey: true });
    expect(leave.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Stop' }));
    expect(typed).toHaveBeenCalledTimes(2);
  });

  it('leaves with Ctrl+Tab to the first enabled header button in every state', () => {
    const { container } = renderConsole({ header: IDLE });
    const input = terminalInput(container);
    input.focus();
    pressTab(input, { ctrlKey: true, shiftKey: true });
    // Idle: Stop and Run again are disabled.
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Clear' }));
  });

  it('tells screen reader users how to leave the terminal', () => {
    const { container } = renderConsole();
    const describedBy = terminalInput(container).getAttribute('aria-describedby');
    expect(describedBy).not.toBeNull();
    expect(document.getElementById(describedBy ?? '')?.textContent).toBe(
      `${LEAVE_CONSOLE_KEYS} leaves the console.`,
    );
  });

  it('decides what each Tab key event does', () => {
    const key = (
      type: string,
      modifiers: Partial<Record<'ctrlKey' | 'altKey' | 'metaKey', boolean>> = {},
      keyName = 'Tab',
    ) => ({ type, key: keyName, ctrlKey: false, altKey: false, metaKey: false, ...modifiers });
    expect(consoleKeyAction(key('keydown', {}, 'a'), false)).toBe('terminal');
    expect(consoleKeyAction(key('keydown'), true)).toBe('terminal');
    expect(consoleKeyAction(key('keydown'), false)).toBe('browser');
    expect(consoleKeyAction(key('keyup'), false)).toBe('browser');
    expect(consoleKeyAction(key('keydown', { ctrlKey: true }), true)).toBe('leave');
    expect(consoleKeyAction(key('keydown', { ctrlKey: true }), false)).toBe('leave');
    expect(consoleKeyAction(key('keypress', { ctrlKey: true }), true)).toBe('ignore');
    expect(consoleKeyAction(key('keyup', { ctrlKey: true }), true)).toBe('ignore');
    expect(consoleKeyAction(key('keydown', { ctrlKey: true, altKey: true }), true)).toBe(
      'terminal',
    );
    expect(consoleKeyAction(key('keydown', { ctrlKey: true, metaKey: true }), false)).toBe(
      'browser',
    );
  });
});
