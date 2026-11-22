/**
 * The preview benchmark's metrics (preview-metrics.ts): the preview's metrics come from the app's
 * spans of each edit (worker.md §9), the gated edit metric pools every edit of the run (decision
 * D19), and the per-sample metrics take each round's p95.
 */
import { describe, expect, it } from 'vitest';

import { EDITS, type PreviewEdit, previewEdit, previewMetrics } from './preview-metrics';
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

/** Preview runs whose latency is `values`' value; update() takes half of it, the main thread a tenth. */
function previews(values: readonly (readonly number[])[]): PreviewEdit[][] {
  return values.map((round) =>
    round.map((latencyMs) => ({ latencyMs, updateMs: latencyMs / 2, mainMs: latencyMs / 10 })),
  );
}

/** The spans of a preview run in the worker, as the page gives them. */
const WORKER_SPANS = {
  'b2c:preview': 60,
  'b2c:preview:read': 4,
  'b2c:preview:update': 30,
  'b2c:preview:index': 3,
  'b2c:preview:wait': 45,
  'b2c:preview:receive': 1.5,
  'b2c:preview:commit': 0.5,
  'b2c:preview:publish': 2,
  'b2c:code-panel': 7,
};

describe('previewEdit', () => {
  it('takes the run, update() where it ran, and the sum of the main-thread phases', () => {
    expect(previewEdit(WORKER_SPANS)).toEqual({ latencyMs: 60, updateMs: 30, mainMs: 8 });
  });

  it('counts no receipt when update() ran on the main thread', () => {
    const onMain = Object.fromEntries(
      Object.entries(WORKER_SPANS).filter(([name]) => name !== 'b2c:preview:receive'),
    );
    expect(previewEdit(onMain)).toEqual({ latencyMs: 60, updateMs: 30, mainMs: 6.5 });
  });

  it('refuses an edit whose preview lacks a span every run has', () => {
    for (const name of [
      'b2c:preview',
      'b2c:preview:update',
      'b2c:preview:read',
      'b2c:preview:commit',
      'b2c:preview:publish',
    ]) {
      const spans = Object.fromEntries(
        Object.entries(WORKER_SPANS).filter(([span]) => span !== name),
      );
      expect(() => previewEdit(spans)).toThrow(`no span ${name}`);
    }
    expect(() => previewEdit({})).toThrow(/no span b2c:preview /);
  });
});

describe('previewMetrics', () => {
  it('pools every edit of every round into the gated edit metric, valued at their p95', () => {
    // 10 rounds of edits from 1,000 to 1,099 ms: the p95 of all 100 is the 95th smallest.
    const edit = measured(10, 1_000);
    const metrics = previewMetrics(previews(measured(10, 50)), edit);
    expect(metrics.edit.name).toBe('webview.edit-1000.p95-all');
    expect(metrics.edit.gate).toBe(true);
    expect(metrics.edit.statistic).toBe('p95');
    expect(metrics.edit.samples).toEqual(edit.flat());
    expect(metrics.edit.samples).toHaveLength(100);
    expect(metricValue(metrics.edit)).toBe(1_094);
    for (const metric of [
      metrics.preview,
      metrics.update,
      metrics.main,
      metrics.edit,
      metrics.editPerRound,
    ]) {
      expect(() => {
        checkMetric(metric);
      }).not.toThrow();
    }
  });

  it('reports each round’s p95 of the edits, its slowest edit, ungated', () => {
    const metrics = previewMetrics(previews(measured(3, 50)), measured(3, 1_000));
    expect(metrics.editPerRound.name).toBe('webview.edit-1000.p95');
    expect(metrics.editPerRound.gate).toBe(false);
    expect(metrics.editPerRound.statistic ?? 'median').toBe('median');
    expect(metrics.editPerRound.samples).toEqual([1_009, 1_019, 1_029]);
    expect(metrics.edit.samples).toHaveLength(30);
  });

  it('gates the preview’s latency and its update() on each round’s p95', () => {
    const metrics = previewMetrics(previews(measured(3, 50)), measured(3, 1_000));
    expect(metrics.preview.name).toBe('webview.preview-1000.p95');
    expect(metrics.preview.gate).toBe(true);
    expect(metrics.preview.statistic ?? 'median').toBe('median');
    expect(metrics.preview.samples).toEqual([59, 69, 79]);
    expect(metrics.update.name).toBe('webview.preview-1000.update-p95');
    expect(metrics.update.gate).toBe(true);
    expect(metrics.update.statistic ?? 'median').toBe('median');
    expect(metrics.update.samples).toEqual([29.5, 34.5, 39.5]);
  });

  it('reports the main thread’s work of the preview, ungated, on each round’s p95', () => {
    const metrics = previewMetrics(previews(measured(3, 50)), measured(3, 1_000));
    expect(metrics.main.name).toBe('webview.preview-1000.main-p95');
    expect(metrics.main.gate).toBe(false);
    expect(metrics.main.statistic ?? 'median').toBe('median');
    expect(metrics.main.samples).toEqual([5.9, 6.9, 7.9]);
  });

  it('is not decided by a few slow edits, as the per-round p95 is', () => {
    // One slow edit (3,000 ms) in each of five rounds: 5 of the run's 100 edits.
    const edit = measured(10, 1_000, (round, index) =>
      round % 2 === 0 && index === 3 ? 3_000 : undefined,
    );
    const metrics = previewMetrics(previews(measured(10, 50)), edit);
    expect(metricValue(metrics.edit)).toBeLessThan(1_100);
    expect(metricValue(metrics.editPerRound)).toBeGreaterThan(2_000);
  });

  it('refuses a round without exactly EDITS edits', () => {
    const short = measured(2, 1_000);
    short[1]?.pop();
    expect(() => previewMetrics(previews(measured(2, 50)), short)).toThrow(
      /Round 1 of the edits has 9/,
    );
    expect(() => previewMetrics(previews(short), measured(2, 1_000))).toThrow(
      /Round 1 of the previews has 9/,
    );
  });
});
