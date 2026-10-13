import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { TerminalSize } from './ConsolePanel';
import { COLUMN_SETTLE_MS, type FitTarget, Refitter, type RefitTimers } from './refit';

/** About one animation frame. */
const FRAME_MS = 16;

/** The frames and timers of Vitest's fake clock. */
const fakeTimers: RefitTimers = {
  requestAnimationFrame: (callback) => setTimeout(callback, FRAME_MS),
  cancelAnimationFrame: (handle) => {
    clearTimeout(handle as ReturnType<typeof setTimeout>);
  },
  setTimeout: (callback, ms) => setTimeout(callback, ms),
  clearTimeout: (handle) => {
    clearTimeout(handle as ReturnType<typeof setTimeout>);
  },
};

/** A terminal in a box whose fitting size the test sets, recording each resize. */
function fakeTerminal(start: TerminalSize = { cols: 80, rows: 24 }) {
  let size = start;
  let fitting: TerminalSize | null = start;
  const resizes: TerminalSize[] = [];
  const target: FitTarget = {
    propose: () => fitting,
    current: () => size,
    resize: (next) => {
      size = next;
      resizes.push(next);
    },
  };
  return {
    target,
    resizes,
    /** The box changes so that `next` fits (`null`: the box is hidden). */
    box(next: TerminalSize | null) {
      fitting = next;
    },
    size: () => size,
  };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('Refitter', () => {
  it('fits at most once per frame however often the box changes', () => {
    const terminal = fakeTerminal();
    const refitter = new Refitter(terminal.target, fakeTimers);
    // A splitter dragged: the dock's height changes eight times within one frame.
    for (let rows = 25; rows <= 32; rows++) {
      terminal.box({ cols: 80, rows });
      refitter.request();
    }
    expect(terminal.resizes).toEqual([]);
    vi.advanceTimersByTime(FRAME_MS);
    expect(terminal.resizes).toEqual([{ cols: 80, rows: 32 }]);
  });

  it('applies new rows at once and new columns once the width has settled for 100 ms', () => {
    const terminal = fakeTerminal();
    const refitter = new Refitter(terminal.target, fakeTimers);
    // The window is dragged wider and taller for a while, one size per frame.
    for (let step = 1; step <= 10; step++) {
      terminal.box({ cols: 80 + step, rows: 24 + step });
      refitter.request();
      vi.advanceTimersByTime(FRAME_MS);
    }
    // The rows followed every frame; the columns (a reflow of the scrollback) have not changed.
    expect(terminal.resizes).toHaveLength(10);
    expect(terminal.resizes.every((size) => size.cols === 80)).toBe(true);
    expect(terminal.size()).toEqual({ cols: 80, rows: 34 });

    vi.advanceTimersByTime(COLUMN_SETTLE_MS - FRAME_MS - 1);
    expect(terminal.size().cols).toBe(80);
    vi.advanceTimersByTime(FRAME_MS + 1);
    expect(terminal.size()).toEqual({ cols: 90, rows: 34 });
    // One reflow for the whole drag.
    expect(terminal.resizes.filter((size) => size.cols !== 80)).toEqual([{ cols: 90, rows: 34 }]);
  });

  it('forgets the new columns when the width comes back before it settles', () => {
    const terminal = fakeTerminal();
    const refitter = new Refitter(terminal.target, fakeTimers);
    terminal.box({ cols: 60, rows: 24 });
    refitter.request();
    vi.advanceTimersByTime(FRAME_MS);
    terminal.box({ cols: 80, rows: 24 });
    refitter.request();
    vi.advanceTimersByTime(COLUMN_SETTLE_MS * 2);
    expect(terminal.resizes).toEqual([]);
  });

  it('fits rows and columns at once when asked, and leaves a hidden box alone', () => {
    const terminal = fakeTerminal();
    const refitter = new Refitter(terminal.target, fakeTimers);
    terminal.box({ cols: 100, rows: 30 });
    refitter.fitNow();
    expect(terminal.resizes).toEqual([{ cols: 100, rows: 30 }]);

    terminal.box(null);
    refitter.request();
    refitter.fitNow();
    vi.advanceTimersByTime(COLUMN_SETTLE_MS * 2);
    expect(terminal.resizes).toHaveLength(1);
  });

  it('does nothing more once disposed', () => {
    const terminal = fakeTerminal();
    const refitter = new Refitter(terminal.target, fakeTimers);
    terminal.box({ cols: 120, rows: 40 });
    refitter.request();
    vi.advanceTimersByTime(FRAME_MS);
    expect(terminal.size()).toEqual({ cols: 80, rows: 40 });
    terminal.box({ cols: 120, rows: 20 });
    refitter.request();
    refitter.dispose();
    vi.advanceTimersByTime(COLUMN_SETTLE_MS * 2);
    expect(terminal.size()).toEqual({ cols: 80, rows: 40 });
    expect(vi.getTimerCount()).toBe(0);
  });
});
