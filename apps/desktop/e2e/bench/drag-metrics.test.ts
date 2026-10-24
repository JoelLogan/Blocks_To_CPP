/**
 * The drag benchmark's metrics (drag-metrics.ts): the gated metric pools every frame time of the
 * timed pairs of drags (decision D20), and the reported one takes each pair's p95.
 */
import { describe, expect, it } from 'vitest';

import { dragMetrics, WARM_UP_PAIRS } from './drag-metrics';
import { checkMetric, metricValue } from './results';

/** The frame time of a page at 60 Hz, and of a frame that missed one. */
const FRAME = 16.7;
const SLOW = 33.4;

/** The warm-up pair and `pairs` timed pairs of `frames` frame times each, as `time` gives them. */
function measured(
  pairs: number,
  frames: number,
  time: (pair: number, frame: number) => number,
): number[][] {
  const warmUp = Array.from({ length: frames }, () => 500);
  return [
    warmUp,
    ...Array.from({ length: pairs }, (_, pair) =>
      Array.from({ length: frames }, (_, frame) => time(pair, frame)),
    ),
  ];
}

describe('dragMetrics', () => {
  it('pools every frame time of the timed pairs into the gated metric, valued at their p95', () => {
    // 10 timed pairs of 120 frames, 1,000.0 to 1,119.9 ms apart: 1,200 frame times.
    const pairs = measured(10, 120, (pair, frame) => 1_000 + frame + pair / 10);
    const metrics = dragMetrics(pairs);
    expect(WARM_UP_PAIRS).toBe(1);
    expect(metrics.frames.name).toBe('webview.drag-5000.frame-p95-all');
    expect(metrics.frames.gate).toBe(true);
    expect(metrics.frames.statistic).toBe('p95');
    expect(metrics.frames.samples).toEqual(pairs.slice(1).flat());
    expect(metrics.frames.samples).toHaveLength(1_200);
    // The p95 of 1,200 is the 1,140th smallest: every pair's frame 113 (1,113.0 to 1,113.9) is
    // below it, and the 6 slowest frames of each pair (60) are above.
    expect(metricValue(metrics.frames)).toBe(1_113.9);
    for (const metric of [metrics.frames, metrics.perPair]) {
      expect(() => {
        checkMetric(metric);
      }).not.toThrow();
    }
  });

  it('leaves out the warm-up pair', () => {
    const metrics = dragMetrics(measured(3, 40, () => FRAME));
    expect(metrics.frames.samples).toHaveLength(120);
    expect(metrics.frames.samples.every((time) => time === FRAME)).toBe(true);
    expect(metrics.perPair.samples).toEqual([FRAME, FRAME, FRAME]);
  });

  it('reports each timed pair’s p95, ungated', () => {
    const metrics = dragMetrics(measured(3, 120, (pair, frame) => 10 * pair + frame / 10));
    expect(metrics.perPair.name).toBe('webview.drag-5000.frame-p95');
    expect(metrics.perPair.gate).toBe(false);
    expect(metrics.perPair.statistic ?? 'median').toBe('median');
    // The p95 of 120 frames is the 114th smallest (frame 113).
    expect(metrics.perPair.samples).toEqual([11.3, 21.3, 31.3]);
  });

  it('counts the slow frames of the whole run, not how many pairs have more than 6', () => {
    // 6 of 10 pairs start with 10 slow frames, the others with none: 60 slow frames of 1,200, 5%.
    const pairs = measured(10, 120, (pair, frame) => (pair < 6 && frame < 10 ? SLOW : FRAME));
    const metrics = dragMetrics(pairs);
    expect(metricValue(metrics.frames)).toBe(FRAME);
    expect(metricValue(metrics.perPair)).toBe(SLOW);
  });

  it('refuses a run without a timed pair, and a pair without frame times', () => {
    expect(() => dragMetrics([])).toThrow(/made 0 pairs/);
    expect(() => dragMetrics(measured(0, 40, () => FRAME))).toThrow(/made 1 pairs/);
    const empty = measured(2, 40, () => FRAME);
    empty[2] = [];
    expect(() => dragMetrics(empty)).toThrow(/Timed pair 2 of the drags has no frame times/);
  });
});
