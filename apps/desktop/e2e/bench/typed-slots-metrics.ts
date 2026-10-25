/**
 * The metrics of the typed-slots benchmarks (typed-slots.bench.e2e.ts, docs/plans/m3-contracts/
 * slots.md §8), both reported only (not gated):
 *
 * - `webview.drag-typed-1000.frame-p95-all`: every frame time recorded while a number block is
 *   dragged slowly down the column of typed `create` blocks and back, in the timed drags, is a
 *   sample, and the comparison takes the p95 of all of them, as for `webview.drag-5000.frame-p95-all`
 *   (decision D20). Reported until it has 5 baselines; then a later change gates it, as D20's
 *   pooled metric was.
 * - `webview.keyboard-list-1000.ms`: the time to list the keyboard mover's places for the number
 *   block (the app's User Timing span `b2c:keyboard-targets`), one listing per sample, against
 *   N12's 100 ms at 1,000 blocks.
 */
import type { BenchMetric } from './results';

/** The first drag warms up (the connection database, the typed-slot caches): not kept. */
export const WARM_UP_DRAGS = 1;

/** The first keyboard listing warms up too. */
export const WARM_UP_LISTINGS = 1;

/** `webview.drag-typed-1000.frame-p95-all` from each drag's frame times, the warm-up first. */
export function typedDragMetric(drags: readonly (readonly number[])[]): BenchMetric {
  if (drags.length <= WARM_UP_DRAGS) {
    throw new Error(
      `The typed-slots benchmark made ${String(drags.length)} drags; the first ${String(WARM_UP_DRAGS)} only warms up`,
    );
  }
  const timed = drags.slice(WARM_UP_DRAGS);
  timed.forEach((times, index) => {
    if (times.length === 0) {
      throw new Error(`Timed drag ${String(index + 1)} has no frame times`);
    }
  });
  return {
    name: 'webview.drag-typed-1000.frame-p95-all',
    unit: 'ms',
    better: 'lower',
    gate: false,
    statistic: 'p95',
    description:
      'p95 frame time dragging a number block down a column of 200 typed create blocks and back in a 1,000-block module; every frame of the timed drags',
    samples: timed.flat(),
  };
}

/** `webview.keyboard-list-1000.ms` from each listing's time, the warm-up first. */
export function keyboardListMetric(listings: readonly number[]): BenchMetric {
  if (listings.length <= WARM_UP_LISTINGS) {
    throw new Error(
      `The typed-slots benchmark made ${String(listings.length)} keyboard listings; the first ${String(WARM_UP_LISTINGS)} only warms up`,
    );
  }
  return {
    name: 'webview.keyboard-list-1000.ms',
    unit: 'ms',
    better: 'lower',
    gate: false,
    description:
      "time to list the keyboard mover's places for a number block in a 1,000-block module with 200 typed create blocks; one listing per sample",
    samples: listings.slice(WARM_UP_LISTINGS),
  };
}
