import { FitAddon } from '@xterm/addon-fit';
import { Terminal } from '@xterm/xterm';
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { createRef } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { expectNoAxeViolations } from '../../test/axe';
import { REANNOUNCE_MS, TRANSIENT_STATUS_MS } from '../shared/useTransientStatus';
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
import { LinkDialog } from './LinkDialog';
import { COLUMN_SETTLE_MS } from './refit';
import { FLOOD_QUIET_MS, FLOOD_WRITE_BYTES } from './screenReader';

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

  it('reports what the terminal holds: the active buffer, the viewport and the cursor', async () => {
    const { handle, terminal } = renderConsole();
    await act(() => handle.write(encode('one\r\ntwo  ')));
    const normal = handle.screen();
    expect(normal).toMatchObject({
      buffer: 'normal',
      baseY: 0,
      viewportY: 0,
      cursor: { row: 1, column: 5 },
      cols: terminal.cols,
      rows: terminal.rows,
    });
    expect(normal.lines).toHaveLength(terminal.rows);
    // Cells nothing was written to are cut; the spaces written are kept.
    expect(normal.lines.slice(0, 3)).toEqual(['one', 'two  ', '']);

    // Scrolled: the scrollback comes first, and the screen starts at `baseY`.
    const many = Array.from({ length: terminal.rows + 5 }, (_, i) => `line ${String(i)}`);
    await act(() => handle.write(encode(`\r\n${many.join('\r\n')}`)));
    const scrolled = handle.screen();
    expect(scrolled.baseY).toBe(scrolled.lines.length - terminal.rows);
    expect(scrolled.viewportY).toBe(scrolled.baseY);
    expect(scrolled.lines.slice(0, 3)).toEqual(['one', 'two  ', 'line 0']);
    expect(scrolled.lines[scrolled.cursor.row]).toBe(`line ${String(terminal.rows + 4)}`);

    // A program's alternate screen holds only what that program drew.
    await act(() => handle.write(encode('\u001b[?1049h\u001b[Hfull screen')));
    const alternate = handle.screen();
    expect(alternate.buffer).toBe('alternate');
    expect(alternate.lines[0]).toBe('full screen');
    expect(alternate.lines).toHaveLength(terminal.rows);
  });

  it('reads its colours from the console tokens again when the theme changes (U14d)', () => {
    const { terminal, rerender, props } = renderConsole({ theme: 'light' });
    const host = terminal.element?.parentElement;
    if (host === undefined || host === null) {
      throw new Error('the terminal has no host');
    }
    // Stand-ins for the dark tokens, which the page's theme selects in the app.
    host.style.setProperty('--b2c-console-bg', '#161a22');
    host.style.setProperty('--b2c-console-fg', '#e6e9f0');
    host.style.setProperty('--b2c-console-cursor', '#ffc266');
    host.style.setProperty('--b2c-console-selection', 'rgb(138 180 255 / 35%)');
    // The same theme: nothing is read again.
    rerender(<ConsolePanel {...props} theme="light" />);
    expect(terminal.options.theme?.background).not.toBe('#161a22');
    rerender(<ConsolePanel {...props} theme="dark" />);
    expect(terminal.options.theme).toEqual({
      background: '#161a22',
      foreground: '#e6e9f0',
      cursor: '#ffc266',
      selectionBackground: 'rgb(138 180 255 / 35%)',
    });
  });

  it('reads its colours again when it is shown, as the theme may have changed while hidden', () => {
    let resized: () => void = () => undefined;
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(callback: () => void) {
          resized = callback;
        }
        observe(): void {
          // The test calls `resized` itself.
        }
        disconnect(): void {
          // Nothing to stop.
        }
      },
    );
    const { terminal } = renderConsole({ theme: 'light' });
    const host = terminal.element?.parentElement;
    if (host === undefined || host === null) {
      throw new Error('the terminal has no host');
    }
    const before = terminal.options.theme;
    // Unchanged tokens: the theme object is kept, so xterm.js redraws nothing.
    resized();
    expect(terminal.options.theme).toBe(before);
    host.style.setProperty('--b2c-console-bg', '#161a22');
    resized();
    expect(terminal.options.theme?.background).toBe('#161a22');
  });

  it('gives xterm.js a selection colour it can read, in the form the tokens use', () => {
    const { terminal, rerender, props } = renderConsole({ theme: 'light' });
    const host = terminal.element?.parentElement;
    if (host === undefined || host === null) {
      throw new Error('the terminal has no host');
    }
    /** The colours xterm.js resolved (its theme service; not part of its public API). */
    const resolved = () =>
      (
        terminal as unknown as {
          _core: { _themeService: { colors: { selectionBackgroundTransparent: { css: string } } } };
        }
      )._core._themeService.colors.selectionBackgroundTransparent.css;
    // The tokens of panels.css: xterm.js reads the comma form; with a space-separated rgb() and
    // an alpha it throws and uses its own white at 30 %.
    host.style.setProperty('--b2c-console-selection', 'rgba(138, 180, 255, 0.35)');
    rerender(<ConsolePanel {...props} theme="dark" />);
    expect(resolved()).not.toBe('rgba(255, 255, 255, 0.3)');
    expect(resolved()).toMatch(/^(rgba\(138, 180, 255|#8ab4ff)/);
  });

  it('keeps program colours readable on either console background (WCAG 1.4.3)', () => {
    const { terminal } = renderConsole();
    expect(terminal.options.minimumContrastRatio).toBeGreaterThanOrEqual(4.5);
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
    expect(within(dialog).getByRole('status').textContent).toBe('Copied');
    await expectNoAxeViolations(dialog);

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(open).not.toHaveBeenCalled();
  });

  describe("the link dialog's copy confirmation", () => {
    const URL = 'https://example.com/a';

    beforeEach(() => {
      vi.useFakeTimers();
    });

    /** Clicks *Copy link* and lets the copy finish. */
    async function copyLink(): Promise<void> {
      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'Copy link' }));
        await vi.advanceTimersByTimeAsync(0);
      });
    }

    async function wait(ms: number): Promise<void> {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(ms);
      });
    }

    const status = () => within(screen.getByRole('dialog')).getByRole('status').textContent;

    it('says Copied for 3 seconds, and again after every copy', async () => {
      vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue();
      render(<LinkDialog url={URL} onClose={vi.fn()} />);
      await copyLink();
      expect(status()).toBe('Copied');
      await wait(1000);
      await copyLink();
      expect(status()).toBe('');
      await wait(REANNOUNCE_MS);
      expect(status()).toBe('Copied');
      await wait(TRANSIENT_STATUS_MS);
      expect(status()).toBe('');
    });

    it('keeps "Could not copy" until the next copy, and forgets it when it closes', async () => {
      vi.spyOn(navigator.clipboard, 'writeText').mockRejectedValue(new Error('denied'));
      const onClose = vi.fn();
      const { rerender } = render(<LinkDialog url={URL} onClose={onClose} />);
      await copyLink();
      await wait(10_000);
      expect(status()).toBe('Could not copy');

      fireEvent.click(screen.getByRole('button', { name: 'Close' }));
      expect(onClose).toHaveBeenCalledTimes(1);
      rerender(<LinkDialog url={null} onClose={onClose} />);
      rerender(<LinkDialog url={URL} onClose={onClose} />);
      expect(status()).toBe('');
    });

    it('changes nothing once it is gone while a copy is still pending', async () => {
      let finish: () => void = () => undefined;
      vi.spyOn(navigator.clipboard, 'writeText').mockReturnValue(
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
      );
      const { unmount } = render(<LinkDialog url={URL} onClose={vi.fn()} />);
      fireEvent.click(screen.getByRole('button', { name: 'Copy link' }));
      unmount();
      const timers = vi.getTimerCount();
      finish();
      await vi.advanceTimersByTimeAsync(0);
      // The status started no timer: it showed nothing.
      expect(vi.getTimerCount()).toBeLessThanOrEqual(timers);
    });
  });

  it('says a link dialog is about to open before it shows, so the editor closes its pop-ups', () => {
    const shownBefore: boolean[] = [];
    const onBeforeLinkDialog = vi.fn(() => {
      shownBefore.push(screen.queryByRole('dialog') !== null);
    });
    const { terminal } = renderConsole({ onBeforeLinkDialog });
    act(() => {
      terminal.options.linkHandler?.activate(new MouseEvent('click'), 'https://example.com/', {
        start: { x: 1, y: 1 },
        end: { x: 2, y: 1 },
      });
    });
    expect(onBeforeLinkDialog).toHaveBeenCalledTimes(1);
    expect(shownBefore).toEqual([false]);
    expect(screen.getByRole('dialog', { name: 'Link from your program' })).toBeTruthy();
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

  it('clears the terminal and tells its owner; a running program keeps its prompt line', async () => {
    const { handle, terminal, props } = renderConsole();
    await act(() => handle.write(encode('one\r\ntwo\r\nYour guess: ')));
    fireEvent.click(screen.getByRole('button', { name: 'Clear' }));
    expect(props.onClear).toHaveBeenCalledTimes(1);
    // The line with the cursor stays, and the cursor stays at its end.
    await act(() => handle.write(encode('')));
    expect(lines(terminal).filter((line) => line !== '')).toEqual(['Your guess: ']);
    expect(terminal.buffer.active.cursorX).toBe('Your guess: '.length);
  });

  it('empties the terminal completely when no program runs', async () => {
    const { handle, terminal } = renderConsole({ header: IDLE });
    const many = Array.from({ length: terminal.rows + 5 }, (_, i) => `line ${String(i)}`);
    await act(() => handle.write(encode(`${many.join('\r\n')}\r\nFinished without a newline`)));
    fireEvent.click(screen.getByRole('button', { name: 'Clear' }));
    // Output written after Clear starts at the top-left corner of an empty terminal.
    await act(() => handle.write(encode('next')));
    expect(lines(terminal).filter((line) => line !== '')).toEqual(['next']);
    expect(lines(terminal)[0]).toBe('next');
    expect(terminal.buffer.active.baseY).toBe(0);
  });

  // xterm.js's `clear()` counts a screen that never scrolled, with the cursor on its first row, as
  // already clear: Clear left every row below the first one.
  it('empties a screen that never scrolled with the cursor on its first row', async () => {
    for (const home of ['\u001b[H', '\u001b[2A', '\u001b[1;1H']) {
      const { handle, terminal, unmount } = renderConsole({ header: IDLE });
      await act(() => handle.write(encode(`line A\r\nline B\r\nline C${home}`)));
      expect(terminal.buffer.active.baseY).toBe(0);
      expect(terminal.buffer.active.cursorY).toBe(0);
      fireEvent.click(screen.getByRole('button', { name: 'Clear' }));
      await act(() => handle.write(encode('')));
      expect(
        lines(terminal).filter((line) => line !== ''),
        JSON.stringify(home),
      ).toEqual([]);
      unmount();
    }
  });

  it("keeps only the cursor's row when a running program left the cursor on the first row", async () => {
    const { handle, terminal } = renderConsole();
    await act(() => handle.write(encode('board 1\r\nboard 2\r\nboard 3\u001b[1;4H')));
    expect(terminal.buffer.active.cursorY).toBe(0);
    fireEvent.click(screen.getByRole('button', { name: 'Clear' }));
    await act(() => handle.write(encode('')));
    expect(lines(terminal).filter((line) => line !== '')).toEqual(['board 1']);
    // The cursor stays where the program put it.
    expect(terminal.buffer.active.cursorY).toBe(0);
    expect(terminal.buffer.active.cursorX).toBe(3);
  });

  it('empties both screens when a program that ended left its full screen on', async () => {
    const { handle, terminal } = renderConsole({ header: IDLE });
    await act(() => handle.write(encode('kept below\r\n\u001b[?1049h\u001b[5;5Hfull screen')));
    expect(terminal.buffer.active.type).toBe('alternate');
    act(() => {
      handle.clear();
    });
    expect(terminal.buffer.active.type).toBe('normal');
    // The next run leaves the alternate screen: nothing of the earlier output comes back.
    await act(() => handle.write(encode('\u001b7\u001b[?1049l\u001b[!pnew run')));
    expect(lines(terminal).filter((line) => line !== '')).toEqual(['new run']);
  });

  it('scrolls to the end of the output when asked, in order with the output', async () => {
    const { handle, terminal } = renderConsole();
    const many = Array.from({ length: terminal.rows * 3 }, (_, i) => `line ${String(i)}`);
    await act(() => handle.write(encode(many.join('\r\n'))));
    act(() => {
      terminal.scrollLines(-10);
    });
    const scrolledUp = handle.screen();
    expect(scrolledUp.viewportY).toBe(scrolledUp.baseY - 10);

    // Output alone does not move a view the person scrolled up.
    await act(() => handle.write(encode('\r\nmore\r\n')));
    expect(handle.screen().viewportY).toBeLessThan(handle.screen().baseY);

    // A new run: its separator, then the view goes to the end and follows its output.
    void handle.write(encode('\r\n── New run ──\r\n'));
    handle.scrollToBottom();
    await act(() => handle.write(encode('first line of the run\r\n'.repeat(5))));
    const shown = handle.screen();
    expect(shown.viewportY).toBe(shown.baseY);
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

describe('ConsolePanel wheel, fitting and screen readers', () => {
  /** Rows of 16 px: happy-dom lays nothing out, so the screen's height is given. */
  function layOutRows(terminal: Terminal): void {
    const screenElement = terminal.element?.querySelector('.xterm-screen');
    if (!(screenElement instanceof HTMLElement)) {
      throw new Error('the terminal has no screen');
    }
    vi.spyOn(screenElement, 'getBoundingClientRect').mockImplementation(
      () => new DOMRect(0, 0, 640, terminal.rows * 16),
    );
  }

  /**
   * A wheel event on the terminal, as the pointer would send it. happy-dom's `WheelEvent` has no
   * modifier keys (a browser's is a `MouseEvent`), so they are set on the event itself.
   */
  function wheelOn(terminal: Terminal, init: WheelEventInit): WheelEvent {
    const event = new WheelEvent('wheel', { bubbles: true, cancelable: true, ...init });
    for (const key of ['ctrlKey', 'altKey', 'shiftKey', 'metaKey'] as const) {
      Object.defineProperty(event, key, { value: init[key] === true });
    }
    terminal.element?.dispatchEvent(event);
    return event;
  }

  it('scrolls the scrollback by whole lines with the wheel, while output streams', async () => {
    const { handle, terminal } = renderConsole();
    layOutRows(terminal);
    const many = Array.from({ length: 100 }, (_, i) => `line ${String(i)}`);
    await act(() => handle.write(encode(many.join('\r\n'))));
    const end = handle.screen().baseY;

    // Two notches of 53 px up: 6.6 rows, so 6 now and the rest kept.
    expect(wheelOn(terminal, { deltaY: -53 }).defaultPrevented).toBe(true);
    wheelOn(terminal, { deltaY: -53 });
    expect(handle.screen().viewportY).toBe(end - 6);

    // The program goes on printing: the view stays where the person put it.
    await act(() => handle.write(encode('\r\nmore'.repeat(20))));
    expect(handle.screen().viewportY).toBe(end - 6);
    // Ctrl + wheel is not scrolling (it is left to the browser and xterm.js).
    expect(wheelOn(terminal, { deltaY: 53, ctrlKey: true }).defaultPrevented).toBe(false);
    expect(handle.screen().viewportY).toBe(end - 6);
  });

  it("leaves the wheel to a program's alternate screen, which gets arrow keys", async () => {
    const { handle, terminal } = renderConsole();
    layOutRows(terminal);
    const typed = vi.fn();
    handle.onData(typed);
    await act(() => handle.write(encode('\u001b[?1049hfull screen')));
    // Line mode, as xterm.js's own handler converts pixels with a row height it has not got here.
    const event = wheelOn(terminal, { deltaY: 2, deltaMode: 1 });
    expect(event.defaultPrevented).toBe(true);
    expect(typed).toHaveBeenCalledWith('\u001b[B\u001b[B');
  });

  it('fits the terminal once per frame, with new columns once the width has settled', async () => {
    vi.useFakeTimers();
    let resized: () => void = () => undefined;
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(callback: () => void) {
          resized = callback;
        }
        observe(): void {
          // The test calls `resized` itself.
        }
        disconnect(): void {
          // Nothing to stop.
        }
      },
    );
    let fitting = { cols: 80, rows: 24 };
    vi.spyOn(FitAddon.prototype, 'proposeDimensions').mockImplementation(() => fitting);
    const { handle } = renderConsole();
    const sizes = vi.fn();
    handle.onResize(sizes);

    // A splitter dragged: the box changes several times within a frame.
    for (const rows of [26, 28, 30]) {
      fitting = { cols: 100, rows };
      resized();
    }
    await act(async () => {
      await vi.advanceTimersByTimeAsync(16);
    });
    expect(sizes.mock.calls).toEqual([[{ cols: 80, rows: 30 }]]);
    expect(handle.size()).toEqual({ cols: 80, rows: 30 });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(COLUMN_SETTLE_MS);
    });
    expect(handle.size()).toEqual({ cols: 100, rows: 30 });
    expect(sizes).toHaveBeenCalledTimes(2);
  });

  it('gives screen readers the rows on screen and announces new output', async () => {
    const { handle, container, terminal } = renderConsole();
    expect(terminal.options.screenReaderMode).toBe(true);
    await act(() => handle.write(encode('Guess a number from 1 to 100!\r\nYour guess: ')));
    await waitForAccessibility(container, (tree) => tree.textContent.includes('Your guess:'));
    const tree = container.querySelector('.xterm-accessibility');
    expect(tree?.querySelector('[role="list"]')?.textContent).toContain('Guess a number');
    expect(tree?.querySelector('.live-region')?.textContent).toContain('Your guess:');
    await expectNoAxeViolations(container);
  });

  it('pauses the screen reader output during a flood, then offers the rows on screen again', async () => {
    vi.useFakeTimers();
    const { handle, container } = renderConsole();
    const flood = `${'flood line\r\n'.repeat(Math.ceil(FLOOD_WRITE_BYTES / 12))}last line`;
    expect(flood.length).toBeGreaterThan(FLOOD_WRITE_BYTES);
    const written = handle.write(encode(flood));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(50);
    });
    await written;
    expect(container.querySelector('.xterm-accessibility')).toBeNull();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(FLOOD_QUIET_MS);
    });
    const tree = container.querySelector('.xterm-accessibility');
    expect(tree?.querySelector('[role="list"]')?.textContent).toContain('last line');
  });

  // xterm.js announces at most 20 lines and then nothing until a key or a blur; a run started with
  // F5 from the console is neither, so the next run was never announced.
  it('announces each run afresh, however much the run before printed', async () => {
    const { handle, container } = renderConsole();
    terminalInput(container).focus();
    const many = Array.from({ length: 30 }, (_, i) => String(i + 1));
    await act(() => handle.write(encode(`${many.join('\r\n')}\r\n`)));
    await waitForAccessibility(container, (tree) =>
      (tree.querySelector('.live-region')?.textContent ?? '').includes('21'),
    );

    // The next run: its separator, then the view goes to the end (once per run).
    void handle.write(encode('\r\n── New run ──\r\n'));
    handle.scrollToBottom();
    await act(() => handle.write(encode('Guess a number from 1 to 100!\r\nYour guess: ')));
    await waitForAccessibility(container, (tree) =>
      (tree.querySelector('.live-region')?.textContent ?? '').includes('Your guess:'),
    );
    expect(document.activeElement).toBe(terminalInput(container));
  });

  it('builds the accessibility tree afresh when the terminal is cleared or reset', async () => {
    const { handle, container } = renderConsole({ header: IDLE });
    await act(() => handle.write(encode('old output\r\n')));
    await waitForAccessibility(container, (tree) => tree.textContent.includes('old output'));

    fireEvent.click(screen.getByRole('button', { name: 'Clear' }));
    await waitForAccessibility(container, (tree) => !tree.textContent.includes('old output'));
    await act(() => handle.write(encode('new output')));
    await waitForAccessibility(container, (tree) => tree.textContent.includes('new output'));

    act(() => {
      handle.reset();
    });
    await waitForAccessibility(container, (tree) => !tree.textContent.includes('new output'));
  });
});

/**
 * Waits until the terminal's accessibility tree (its rows and live region) passes `check`:
 * xterm.js updates it in timers.
 */
async function waitForAccessibility(
  container: HTMLElement,
  check: (tree: Element) => boolean,
): Promise<void> {
  for (let attempt = 0; attempt < 150; attempt++) {
    const tree = container.querySelector('.xterm-accessibility');
    if (tree !== null && check(tree)) {
      return;
    }
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
  }
  const tree = container.querySelector('.xterm-accessibility');
  throw new Error(`The accessibility tree holds: ${tree?.textContent ?? '(no tree)'}`);
}

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
    const { container, handle, terminal } = renderConsole({ header: RUNNING });
    // In screen reader mode xterm.js leaves Shift+Tab uncancelled; the console cancels it.
    expect(terminal.options.screenReaderMode).toBe(true);
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
