/**
 * The declaration benchmark's metrics (declare-metrics.ts): each round's median, the refreshes gated, and
 * a round in which the toolbox did not refresh fails instead of giving a sample without that work.
 */
import { describe, expect, it } from 'vitest';

import { DECLARATIONS, type Declaration, declareMetrics } from './declare-metrics';
import { checkMetric } from './results';

/** `rounds` rounds of {@link DECLARATIONS} declarations, the refresh from `base` up. */
function measured(rounds: number, base: number): Declaration[][] {
  return Array.from({ length: rounds }, (_, round) =>
    Array.from({ length: DECLARATIONS }, (_, index) => ({
      refreshMs: base + round * 100 + index,
      longestMs: base / 2 + index,
    })),
  );
}

describe('declareMetrics', () => {
  it('takes each round’s median of the refreshes, gated, and of the longest tasks, ungated', () => {
    // Results files need 10 samples (results.ts MIN_SAMPLES).
    const metrics = declareMetrics(measured(10, 80), measured(10, 300));
    const rounds = Array.from({ length: 10 }, (_, round) => round * 100 + 4.5);
    expect(metrics.refresh.name).toBe('webview.declare-variable.toolbox-refresh');
    expect(metrics.refresh.samples).toEqual(rounds.map((value) => 80 + value));
    expect(metrics.refresh1000.name).toBe('webview.declare-variable-1000.toolbox-refresh');
    expect(metrics.refresh1000.samples).toEqual(rounds.map((value) => 300 + value));
    expect(metrics.maxTask.name).toBe('webview.declare-variable.max-task');
    expect(metrics.maxTask.samples).toEqual(Array.from({ length: 10 }, () => 44.5));
    expect(metrics.refresh.gate).toBe(true);
    expect(metrics.refresh1000.gate).toBe(true);
    expect(metrics.maxTask.gate).toBe(false);
    for (const metric of [metrics.refresh, metrics.refresh1000, metrics.maxTask]) {
      expect(() => {
        checkMetric(metric);
      }).not.toThrow();
    }
  });

  it('refuses a round with another number of declarations', () => {
    const short = measured(1, 80).map((round) => round.slice(1));
    expect(() => declareMetrics(short, measured(1, 300))).toThrow(/has 9 declarations, not 10/);
  });

  it('refuses a declaration that did not refresh the toolbox', () => {
    const rounds = measured(2, 80);
    const second = rounds[1];
    if (second?.[3] !== undefined) {
      second[3] = { refreshMs: 0, longestMs: 12 };
    }
    expect(() => declareMetrics(measured(1, 80), rounds)).toThrow(
      /Declaration 3 of round 1 on 1,000 blocks did not refresh the toolbox/,
    );
  });
});
