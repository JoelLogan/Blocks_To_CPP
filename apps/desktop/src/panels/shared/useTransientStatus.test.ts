/** A status message that confirms briefly or stays until the next attempt (04 §4.3, §4.8). */
import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { REANNOUNCE_MS, TRANSIENT_STATUS_MS, useTransientStatus } from './useTransientStatus';

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

/** Lets `ms` milliseconds pass, with the React updates the timers cause. */
function wait(ms: number): void {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
}

describe('useTransientStatus', () => {
  it('shows a success at once and removes it after 3 seconds', () => {
    const { result } = renderHook(() => useTransientStatus());
    expect(result.current.shown).toBeNull();
    act(() => {
      result.current.show('success', 'Copied');
    });
    expect(result.current.shown).toEqual({ kind: 'success', text: 'Copied' });
    wait(TRANSIENT_STATUS_MS - 1);
    expect(result.current.shown?.text).toBe('Copied');
    wait(1);
    expect(result.current.shown).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('keeps a failure until the next attempt', () => {
    const { result } = renderHook(() => useTransientStatus());
    act(() => {
      result.current.show('failure', 'Could not copy');
    });
    wait(10_000);
    expect(result.current.shown).toEqual({ kind: 'failure', text: 'Could not copy' });
    expect(vi.getTimerCount()).toBe(0);

    // The next attempt replaces it, after a moment without text.
    act(() => {
      result.current.show('success', 'Copied');
    });
    expect(result.current.shown).toBeNull();
    wait(REANNOUNCE_MS);
    expect(result.current.shown?.text).toBe('Copied');
  });

  it('empties the status and shows the same text again on a repeat, with a fresh 3 seconds', () => {
    const { result } = renderHook(() => useTransientStatus());
    act(() => {
      result.current.show('success', 'Copied');
    });
    wait(2000);
    act(() => {
      result.current.show('success', 'Copied');
    });
    // Empty for a moment, so a screen reader announces the repeat.
    expect(result.current.shown).toBeNull();
    wait(REANNOUNCE_MS);
    expect(result.current.shown?.text).toBe('Copied');
    // The first copy's timer no longer applies.
    wait(TRANSIENT_STATUS_MS - 1);
    expect(result.current.shown?.text).toBe('Copied');
    wait(1);
    expect(result.current.shown).toBeNull();
  });

  it('clears at once, and a cleared repeat does not come back', () => {
    const { result } = renderHook(() => useTransientStatus());
    act(() => {
      result.current.show('failure', 'Could not copy');
    });
    act(() => {
      result.current.show('success', 'Copied');
    });
    act(() => {
      result.current.clear();
    });
    expect(result.current.shown).toBeNull();
    wait(TRANSIENT_STATUS_MS * 2);
    expect(result.current.shown).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('changes nothing after its component has unmounted', () => {
    const { result, unmount } = renderHook(() => useTransientStatus());
    act(() => {
      result.current.show('success', 'Copied');
    });
    const { show, clear } = result.current;
    unmount();
    expect(vi.getTimerCount()).toBe(0);
    show('success', 'Copied');
    show('failure', 'Could not copy');
    clear();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('keeps its functions across renders', () => {
    const { result, rerender } = renderHook(() => useTransientStatus());
    const { show, clear } = result.current;
    rerender();
    expect(result.current.show).toBe(show);
    expect(result.current.clear).toBe(clear);
  });
});
