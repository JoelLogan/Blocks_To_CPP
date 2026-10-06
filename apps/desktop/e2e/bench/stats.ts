/**
 * The statistics of the webview benchmarks: medians, percentiles and frame times. The comparison
 * with the baseline (tools/bench-compare.py) takes the median of a metric's samples; a sample of a
 * percentile metric is itself the percentile of one round of measurements.
 */

/** The values are not usable: empty, or not finite numbers. */
export class StatsError extends Error {
  override readonly name = 'StatsError';
}

function checked(values: readonly number[], what: string): number[] {
  if (values.length === 0) {
    throw new StatsError(`No ${what} to summarise`);
  }
  if (!values.every((value) => Number.isFinite(value))) {
    throw new StatsError(`The ${what} are not all finite numbers`);
  }
  return [...values].sort((a, b) => a - b);
}

/** The value at `index`, which the caller knows is in range. */
function valueAt(values: readonly number[], index: number): number {
  const value = values[index];
  if (value === undefined) {
    throw new StatsError(`No value at ${String(index)} of ${String(values.length)}`);
  }
  return value;
}

/** The median (the mean of the middle two for an even count), as Python's statistics.median. */
export function median(values: readonly number[]): number {
  const sorted = checked(values, 'values');
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 1
    ? valueAt(sorted, middle)
    : (valueAt(sorted, middle - 1) + valueAt(sorted, middle)) / 2;
}

/**
 * The `p`th percentile by the nearest-rank method: the smallest value with at least `p`% of the
 * values at or below it (p95 of 20 values is the 19th smallest).
 */
export function percentile(values: readonly number[], p: number): number {
  if (!(p > 0 && p <= 100)) {
    throw new StatsError(`A percentile is above 0 and at most 100, not ${String(p)}`);
  }
  const sorted = checked(values, 'values');
  return valueAt(sorted, Math.ceil((p / 100) * sorted.length) - 1);
}

/** The time between consecutive timestamps (frame times from animation-frame timestamps). */
export function deltas(times: readonly number[]): number[] {
  const result: number[] = [];
  for (let index = 1; index < times.length; index += 1) {
    result.push(valueAt(times, index) - valueAt(times, index - 1));
  }
  return result;
}

/** A number rounded to `digits` decimals, for results files that read well. */
export function rounded(value: number, digits = 3): number {
  const scale = 10 ** digits;
  return Math.round(value * scale) / scale;
}
