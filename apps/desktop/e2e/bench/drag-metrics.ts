/**
 * The metrics of the drag benchmark (drag.bench.e2e.ts), taken from the frame times recorded while
 * the pointer dragged: one list per pair of drags (away and back, about 120 frames), the
 * {@link WARM_UP_PAIRS} warm-up pair first.
 *
 * - `webview.drag-5000.frame-p95-all` (gated, decision D20): every frame time of the timed pairs is
 *   a sample (about 1,200 in a nightly run), and the comparison takes the p95 of all of them
 *   (results.ts `statistic`), so the run's frames decide it rather than where each pair's p95
 *   falls.
 * - `webview.drag-5000.frame-p95` (reported; gated before D20): each timed pair's p95, by the
 *   nearest rank about its 7th-longest frame, is one sample.
 */
import type { BenchMetric } from './results';
import { percentile } from './stats';

/** The workspace size. */
export const BLOCKS = 5_000;

/** The first pairs of drags warm up (Blockly's drag surface, the connection database): not kept. */
export const WARM_UP_PAIRS = 1;

/** What the frame times measure. */
const DRAG_WHAT = `frame time dragging a block away and back in a ${String(BLOCKS)}-block workspace`;

/** The metrics of one run of the drag benchmark. */
export interface DragMetrics {
  /** Gated: the p95 of every frame time of the timed pairs. */
  readonly frames: BenchMetric;
  /** Reported: each timed pair's p95. */
  readonly perPair: BenchMetric;
}

/**
 * The metrics of a run from the frame times of each pair of drags, in the order they were made:
 * the {@link WARM_UP_PAIRS} warm-up pairs first, which are left out, then the timed pairs.
 */
export function dragMetrics(pairs: readonly (readonly number[])[]): DragMetrics {
  if (pairs.length <= WARM_UP_PAIRS) {
    throw new Error(
      `The drag benchmark made ${String(pairs.length)} pairs of drags; the first ${String(WARM_UP_PAIRS)} only warms up`,
    );
  }
  const timed = pairs.slice(WARM_UP_PAIRS);
  timed.forEach((times, index) => {
    if (times.length === 0) {
      throw new Error(`Timed pair ${String(index + 1)} of the drags has no frame times`);
    }
  });
  return {
    frames: {
      name: 'webview.drag-5000.frame-p95-all',
      unit: 'ms',
      better: 'lower',
      gate: true,
      statistic: 'p95',
      description: `p95 ${DRAG_WHAT}; every frame of the timed pairs`,
      samples: timed.flat(),
    },
    perPair: {
      name: 'webview.drag-5000.frame-p95',
      unit: 'ms',
      better: 'lower',
      gate: false,
      description: `p95 ${DRAG_WHAT}; one pair per sample`,
      samples: timed.map((times) => percentile(times, 95)),
    },
  };
}
