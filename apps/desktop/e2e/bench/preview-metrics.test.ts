/**
 * The preview benchmark's metrics (preview-metrics.ts): the gated edit metric pools every edit of
 * the run (decision D19), and the per-sample metrics take each round's p95.
 */
import { describe, expect, it } from 'vitest';

import { EDITS, previewMetrics } from './preview-metrics';
import { checkMetric, metricValue } from './results';

/** `rounds` rounds of {@link EDITS} values; `slow` gives the value of an edit, or undefined. */
function measured(
  rounds: number,
  base: number,
  slow: (round: number, edit: number) => number | undefined = () => undefined,
): number[][] {
  return Array.from({ length: rounds }, (_, round) =>
    Array.from({ length: EDITS }, (_, edit) => slow(round, edit) ?? base + round * EDITS + edit),
  );
}

describe('previewMetrics', () => {
  it('pools every edit of every round into the gated edit metric, valued at their p95', () => {
    // 10 rounds of edits from 1,000 to 1,099 ms: the p95 of all 100 is the 95th smallest.
    const edit = measured(10, 1_000);
    const metrics = previewMetrics(measured(10, 50), edit);
    expect(metrics.edit.name).toBe('webview.edit-1000.p95-all');
    expect(metrics.edit.gate).toBe(true);
    expect(metrics.edit.statistic).toBe('p95');
    expect(metrics.edit.samples).toEqual(edit.flat());
    expect(metrics.edit.samples).toHaveLength(100);
    expect(metricValue(metrics.edit)).toBe(1_094);
    for (const metric of [metrics.preview, metrics.edit, metrics.editPerRound]) {
      expect(() => {
        checkMetric(metric);
      }).not.toThrow();
    }
  });

  it('reports each round’s p95 of the edits, its slowest edit, ungated', () => {
    const metrics = previewMetrics(measured(3, 50), measured(3, 1_000));
    expect(metrics.editPerRound.name).toBe('webview.edit-1000.p95');
    expect(metrics.editPerRound.gate).toBe(false);
    expect(metrics.editPerRound.statistic ?? 'median').toBe('median');
    expect(metrics.editPerRound.samples).toEqual([1_009, 1_019, 1_029]);
    expect(metrics.edit.samples).toHaveLength(30);
  });

  it('keeps the preview task gated on each round’s p95', () => {
    const metrics = previewMetrics(measured(3, 50), measured(3, 1_000));
    expect(metrics.preview.name).toBe('webview.preview-1000.p95');
    expect(metrics.preview.gate).toBe(true);
    expect(metrics.preview.statistic ?? 'median').toBe('median');
    expect(metrics.preview.samples).toEqual([59, 69, 79]);
  });

  it('is not decided by a few slow edits, as the per-round p95 is', () => {
    // One slow edit (3,000 ms) in each of five rounds: 5 of the run's 100 edits.
    const edit = measured(10, 1_000, (round, index) =>
      round % 2 === 0 && index === 3 ? 3_000 : undefined,
    );
    const metrics = previewMetrics(measured(10, 50), edit);
    expect(metricValue(metrics.edit)).toBeLessThan(1_100);
    expect(metricValue(metrics.editPerRound)).toBeGreaterThan(2_000);
  });

  it('refuses a round without exactly EDITS edits', () => {
    const short = measured(2, 1_000);
    short[1]?.pop();
    expect(() => previewMetrics(measured(2, 50), short)).toThrow(/Round 1 of the edits has 9/);
    expect(() => previewMetrics(short, measured(2, 1_000))).toThrow(/preview tasks/);
  });
});
