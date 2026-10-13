import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { FLOOD_QUIET_MS, FLOOD_WRITE_BYTES, ScreenReaderSupport } from './screenReader';

/** The modes the support switched the terminal to, in order. */
function recorder() {
  const modes: boolean[] = [];
  const support = new ScreenReaderSupport((on) => {
    modes.push(on);
  });
  return { modes, support };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('ScreenReaderSupport', () => {
  it('keeps the mode on for output a person can follow', () => {
    const { modes, support } = recorder();
    for (let i = 0; i < 1000; i++) {
      support.beforeWrite(FLOOD_WRITE_BYTES);
    }
    expect(modes).toEqual([]);
    expect(support.paused).toBe(false);
  });

  it('pauses during a flood and comes back once no flood-sized write has come for a second', () => {
    const { modes, support } = recorder();
    support.beforeWrite(FLOOD_WRITE_BYTES + 1);
    expect(modes).toEqual([false]);
    expect(support.paused).toBe(true);

    // The flood goes on: every big write moves the end of the pause; small ones do not.
    for (let i = 0; i < 10; i++) {
      vi.advanceTimersByTime(FLOOD_QUIET_MS / 2);
      support.beforeWrite(4 * 1024 * 1024);
      support.beforeWrite(10);
    }
    expect(modes).toEqual([false]);
    vi.advanceTimersByTime(FLOOD_QUIET_MS - 1);
    expect(modes).toEqual([false]);
    vi.advanceTimersByTime(1);
    expect(modes).toEqual([false, true]);
    expect(support.paused).toBe(false);
  });

  it('builds the accessibility tree afresh after a clear or a reset, but not during a pause', () => {
    const { modes, support } = recorder();
    support.restart();
    expect(modes).toEqual([false, true]);

    support.beforeWrite(FLOOD_WRITE_BYTES * 2);
    support.restart();
    expect(modes).toEqual([false, true, false]);
    vi.advanceTimersByTime(FLOOD_QUIET_MS);
    expect(modes).toEqual([false, true, false, true]);
  });

  it('cancels the end of a pause when the console goes away', () => {
    const { modes, support } = recorder();
    support.beforeWrite(FLOOD_WRITE_BYTES * 2);
    support.dispose();
    vi.advanceTimersByTime(FLOOD_QUIET_MS * 2);
    expect(modes).toEqual([false]);
    expect(vi.getTimerCount()).toBe(0);
  });
});
