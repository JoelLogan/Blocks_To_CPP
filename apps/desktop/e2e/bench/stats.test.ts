/** The benchmarks' statistics (stats.ts). */
import { describe, expect, it } from 'vitest';

import { deltas, median, percentile, rounded, StatsError } from './stats';

describe('median', () => {
  it('is the middle value, or the mean of the middle two', () => {
    expect(median([3, 1, 2])).toBe(2);
    expect(median([4, 1, 3, 2])).toBe(2.5);
    expect(median([7])).toBe(7);
  });

  it('refuses no values and values that are not finite', () => {
    expect(() => median([])).toThrow(StatsError);
    expect(() => median([1, Number.NaN])).toThrow(/finite/);
    expect(() => median([1, Number.POSITIVE_INFINITY])).toThrow(StatsError);
  });

  it('does not change its argument', () => {
    const values = [3, 1, 2];
    median(values);
    expect(values).toEqual([3, 1, 2]);
  });
});

describe('percentile', () => {
  const twenty = Array.from({ length: 20 }, (_, index) => 20 - index);

  it('takes the nearest rank', () => {
    expect(percentile(twenty, 95)).toBe(19);
    expect(percentile(twenty, 100)).toBe(20);
    expect(percentile(twenty, 50)).toBe(10);
    expect(percentile(twenty, 1)).toBe(1);
    expect(percentile([5, 1, 9, 3, 7, 2, 8, 4, 6, 10], 95)).toBe(10);
    expect(percentile([42], 95)).toBe(42);
  });

  it('refuses percentiles outside (0, 100] and bad values', () => {
    expect(() => percentile(twenty, 0)).toThrow(StatsError);
    expect(() => percentile(twenty, 101)).toThrow(StatsError);
    expect(() => percentile(twenty, Number.NaN)).toThrow(StatsError);
    expect(() => percentile([], 95)).toThrow(StatsError);
  });
});

describe('deltas', () => {
  it('gives the time between consecutive timestamps', () => {
    expect(deltas([0, 16, 33, 50])).toEqual([16, 17, 17]);
    expect(deltas([5])).toEqual([]);
    expect(deltas([])).toEqual([]);
  });
});

describe('rounded', () => {
  it('rounds to the given number of decimals', () => {
    expect(rounded(1.23456)).toBe(1.235);
    expect(rounded(1.23456, 1)).toBe(1.2);
    expect(rounded(1999.96, 0)).toBe(2000);
  });
});
