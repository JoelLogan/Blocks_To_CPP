/**
 * User Timing spans around the editor's work per interaction (docs/plans/m2-polish-plan.md W2-1,
 * 09 §9.2): each span becomes one `performance.measure` entry named after it, so a profile of the
 * window (WebKit's Web Inspector, Edge's DevTools) shows the spans on its timeline, and the latest
 * duration of each is kept for the end-to-end test hook (src/e2e), which the benchmarks read.
 * Nothing in the app shows them.
 *
 * - `b2c:preview`: one run of the preview pipeline, from reading the canvas to storing the
 *   preview and relabelling the canvas. Its phases are spans too:
 *   - `b2c:preview:read`: reading the document from the canvas and serialising it;
 *   - `b2c:preview:canonical`: the compiler core's `canonical()`;
 *   - `b2c:preview:commit`: storing the edit, with what the store's subscribers do at once;
 *   - `b2c:preview:analyse`: the compiler core's `preview()`;
 *   - `b2c:preview:publish`: storing the preview and relabelling the canvas, with what the
 *     store's subscribers do at once.
 * - `b2c:toolbox-refresh`: rebuilding the toolbox's open dynamic category.
 * - `b2c:select`: the selection handler, which puts the selected block in the store, with what
 *   the store's subscribers do at once.
 * - `b2c:code-panel`: the code panel taking new code, diagnostics or a highlight.
 *
 * A span that ends with an error or is abandoned (a preview run overtaken by a newer one) is not
 * recorded. Recording is cheap enough to leave on: two clock reads and one entry per span, and
 * the entries of a name are cleared before the next one is added, so the browser's performance
 * buffer holds at most one entry per span however long the window runs (an observer still sees
 * every entry). A browser without User Timing still gets the durations.
 */

/** Every span the app measures, in the order of the list above. */
export const TIMING_SPANS = [
  'b2c:preview',
  'b2c:preview:read',
  'b2c:preview:canonical',
  'b2c:preview:commit',
  'b2c:preview:analyse',
  'b2c:preview:publish',
  'b2c:toolbox-refresh',
  'b2c:select',
  'b2c:code-panel',
] as const;

/** The name of a span. */
export type TimingSpan = (typeof TIMING_SPANS)[number];

/** The latest duration of each span, in milliseconds, or `null` for one that has not run. */
export type Timings = Readonly<Record<TimingSpan, number | null>>;

/** A span that has started: {@link SpanTimer.end} records it. */
export interface SpanTimer {
  /** Ends the span now and records it; later calls do nothing. */
  end(): void;
}

const latest = new Map<TimingSpan, number>();

/** The page's clock, in milliseconds since its time origin. */
function now(): number {
  return performance.now();
}

/** Records a span that ran from `start` to `end` (page clock). Never throws. */
function record(name: TimingSpan, start: number, end: number): void {
  latest.set(name, end - start);
  try {
    performance.clearMeasures(name);
    performance.measure(name, { start, end });
  } catch {
    // No User Timing Level 3 in this webview: the duration above is kept all the same.
  }
}

/** Starts span `name` now. Ending it records it; a span never ended is never recorded. */
export function startSpan(name: TimingSpan): SpanTimer {
  const start = now();
  let ended = false;
  return {
    end() {
      if (!ended) {
        ended = true;
        record(name, start, now());
      }
    },
  };
}

/**
 * Runs `work` as span `name` and returns what it returns. The span is recorded when `work`
 * returns; when it throws, the error passes through and nothing is recorded.
 */
export function timed<T>(name: TimingSpan, work: () => T): T {
  const span = startSpan(name);
  const result = work();
  span.end();
  return result;
}

/** The latest duration of each span (a copy). */
export function latestTimings(): Timings {
  const timings = {} as Record<TimingSpan, number | null>;
  for (const name of TIMING_SPANS) {
    timings[name] = latest.get(name) ?? null;
  }
  return timings;
}

/** Forgets every recorded duration (for tests). */
export function resetTimings(): void {
  latest.clear();
}
