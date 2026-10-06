/** Turning the page's readiness result into a cold-start time (launch.ts). */
import { describe, expect, it } from 'vitest';

import { COLD_START_TIMEOUT_MS, ColdStartError, coldStartOf, isReadyResult } from './launch';

describe('isReadyResult', () => {
  it('accepts what READY_SCRIPT calls back with', () => {
    expect(isReadyResult({ kind: 'ready', at: 1.5, already: false })).toBe(true);
    expect(isReadyResult({ kind: 'timeout' })).toBe(true);
  });

  it('refuses anything else', () => {
    expect(isReadyResult(null)).toBe(false);
    expect(isReadyResult('ready')).toBe(false);
    expect(isReadyResult({ kind: 'ready', at: '1', already: false })).toBe(false);
    expect(isReadyResult({ kind: 'ready', at: Number.NaN, already: false })).toBe(false);
    expect(isReadyResult({ kind: 'ready', at: 1 })).toBe(false);
    expect(isReadyResult({ kind: 'other' })).toBe(false);
  });
});

describe('coldStartOf', () => {
  const launched = 1_700_000_000_000;

  it('is the time from the launch to the marker', () => {
    expect(
      coldStartOf(launched, { kind: 'ready', at: launched + 1_966.5, already: false }),
    ).toEqual({ ms: 1_966.5, already: false });
    expect(coldStartOf(launched, { kind: 'ready', at: launched + 2_500, already: true })).toEqual({
      ms: 2_500,
      already: true,
    });
  });

  it('fails on a timeout, a malformed result or an implausible time', () => {
    expect(() => coldStartOf(launched, { kind: 'timeout' })).toThrow(ColdStartError);
    expect(() => coldStartOf(launched, { kind: 'timeout' })).toThrow(
      `${String(COLD_START_TIMEOUT_MS / 1000)} s`,
    );
    expect(() => coldStartOf(launched, undefined)).toThrow(/unexpected/);
    // The page's clock before the launch, or ten times the timeout after it.
    expect(() =>
      coldStartOf(launched, { kind: 'ready', at: launched - 5, already: false }),
    ).toThrow(/not plausible/);
    expect(() =>
      coldStartOf(launched, {
        kind: 'ready',
        at: launched + 10 * COLD_START_TIMEOUT_MS,
        already: false,
      }),
    ).toThrow(/not plausible/);
  });
});
