/** The User Timing spans: what is recorded, what is not, and what stays in the browser's buffer. */
import { afterEach, describe, expect, it, vi } from 'vitest';

import { latestTimings, resetTimings, startSpan, timed, TIMING_SPANS } from './timing';

afterEach(() => {
  vi.restoreAllMocks();
  resetTimings();
  performance.clearMeasures();
});

describe('User Timing spans', () => {
  it('record the duration of what they wrap, and pass its result through', () => {
    const clock = vi.spyOn(performance, 'now');
    clock.mockReturnValueOnce(100).mockReturnValueOnce(112.5);
    expect(timed('b2c:toolbox-refresh', () => 'rebuilt')).toBe('rebuilt');
    expect(latestTimings()['b2c:toolbox-refresh']).toBe(12.5);
    // Every other span has not run.
    const others = TIMING_SPANS.filter((name) => name !== 'b2c:toolbox-refresh');
    expect(others.map((name) => latestTimings()[name])).toEqual(others.map(() => null));
  });

  it('record nothing for work that throws, or a span that never ends', () => {
    expect(() =>
      timed('b2c:select', () => {
        throw new Error('broken');
      }),
    ).toThrow('broken');
    startSpan('b2c:preview');
    expect(latestTimings()['b2c:select']).toBeNull();
    expect(latestTimings()['b2c:preview']).toBeNull();
  });

  it('end once: a second end changes nothing', () => {
    const clock = vi.spyOn(performance, 'now');
    clock.mockReturnValueOnce(0).mockReturnValueOnce(5).mockReturnValueOnce(50);
    const span = startSpan('b2c:code-panel');
    span.end();
    span.end();
    expect(latestTimings()['b2c:code-panel']).toBe(5);
  });

  it('are measure entries, at most one per name in the buffer however often they run', () => {
    for (let run = 0; run < 50; run += 1) {
      timed('b2c:preview:read', () => undefined);
    }
    expect(performance.getEntriesByName('b2c:preview:read', 'measure')).toHaveLength(1);
  });

  it('keep the durations where User Timing is missing or refuses', () => {
    vi.spyOn(performance, 'measure').mockImplementation(() => {
      throw new TypeError('no User Timing Level 3');
    });
    timed('b2c:preview:update', () => undefined);
    expect(latestTimings()['b2c:preview:update']).toBeGreaterThanOrEqual(0);
  });
});
