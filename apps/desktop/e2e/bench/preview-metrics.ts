/**
 * The metrics of the preview benchmark (preview.bench.e2e.ts), taken from what it measured in
 * each edit, round by round: one round of {@link EDITS} edits per sample. Since the preview runs
 * in a Web Worker (ADR-0016), the preview metrics come from the app's User Timing spans of each
 * edit (src/lib/timing.ts), not from the page's long tasks (docs/plans/m3-contracts/worker.md §9):
 *
 * - `webview.preview-1000.p95` (gated; redefined by the worker, `accepted.json`): the span
 *   `b2c:preview`, from the run's start until its answer is applied; each round's p95 is one
 *   sample.
 * - `webview.preview-1000.update-p95` (gated, N4): `update()` alone, the span
 *   `b2c:preview:update`, timed where it runs (the worker; the main thread in the fallback);
 *   each round's p95 is one sample.
 * - `webview.preview-1000.main-p95` (reported): per edit, the sum of the main-thread phases
 *   ({@link MAIN_SPANS}); each round's p95 is one sample.
 * - `webview.edit-1000.p95-all` (gated, decision D19): every edit of the run, C++ panel shown, is
 *   a sample, and the comparison takes the p95 of all of them (results.ts `statistic`), so a few
 *   slow edits do not decide it.
 * - `webview.edit-1000.p95` (reported; gated before D19): each round's p95 of the same edits, its
 *   slowest edit (the p95 of 10 values by the nearest rank is the largest), is one sample.
 */
import type { BenchMetric } from './results';
import { percentile } from './stats';

/** Edits per sample. */
export const EDITS = 10;

/** The span of a preview run, from its start until its answer is applied. */
export const PREVIEW_SPAN = 'b2c:preview';

/** The span of the core's `update()`, placed where it ran (the worker or the main thread). */
export const UPDATE_SPAN = 'b2c:preview:update';

/**
 * The main-thread phases of a preview run, and whether every run has it: `b2c:preview:receive`
 * (reading the worker's scope index) runs only when the worker previews.
 */
export const MAIN_SPANS: readonly { readonly name: string; readonly always: boolean }[] = [
  { name: 'b2c:preview:read', always: true },
  { name: 'b2c:preview:receive', always: false },
  { name: 'b2c:preview:commit', always: true },
  { name: 'b2c:preview:publish', always: true },
];

/** What the edits with the C++ panel shown measure, less the pipeline's debounce. */
const EDIT_WHAT =
  'Edit until the page settles (rendering, preview, C++ panel shown) less the 50 ms debounce, 1,000 blocks';

/** The app's spans of one edit: the time of each span name, in milliseconds. */
export type EditSpans = Readonly<Record<string, number>>;

/** What one edit's preview measured, from its spans (milliseconds). */
export interface PreviewEdit {
  /** The run, from its start until its answer was applied (`b2c:preview`). */
  readonly latencyMs: number;
  /** `update()` alone (`b2c:preview:update`). */
  readonly updateMs: number;
  /** The sum of the main-thread phases ({@link MAIN_SPANS}). */
  readonly mainMs: number;
}

/**
 * What an edit's preview measured, from the app's spans of the edit. Throws when a span that every
 * preview run has is missing: the app did not measure the preview (a build without these spans).
 */
export function previewEdit(spans: EditSpans): PreviewEdit {
  const required = (name: string): number => {
    const ms = spans[name];
    if (ms === undefined) {
      throw new Error(`The edit's preview has no span ${name} (the app does not measure it)`);
    }
    return ms;
  };
  const latencyMs = required(PREVIEW_SPAN);
  const updateMs = required(UPDATE_SPAN);
  let mainMs = 0;
  for (const span of MAIN_SPANS) {
    mainMs += span.always ? required(span.name) : (spans[span.name] ?? 0);
  }
  return { latencyMs, updateMs, mainMs };
}

/** The metrics of one run of the preview benchmark. */
export interface PreviewMetrics {
  /** Gated: the preview's latency, each round's p95. */
  readonly preview: BenchMetric;
  /** Gated: `update()` alone, each round's p95. */
  readonly update: BenchMetric;
  /** Reported: the main thread's work of the preview, each round's p95. */
  readonly main: BenchMetric;
  /** Gated: the p95 of every edit of the run. */
  readonly edit: BenchMetric;
  /** Reported: each round's p95 of its edits. */
  readonly editPerRound: BenchMetric;
}

/** Each round's p95, one sample per round; every round has {@link EDITS} values. */
function roundP95s(rounds: readonly (readonly number[])[], what: string): number[] {
  return rounds.map((values, round) => {
    if (values.length !== EDITS) {
      throw new Error(
        `Round ${String(round)} of the ${what} has ${String(values.length)} edits, not ${String(EDITS)}`,
      );
    }
    return percentile(values, 95);
  });
}

/**
 * The metrics of a run from what each edit measured, one list of {@link EDITS} per round: with
 * the C++ panel folded, the preview's spans (`preview`), and with it shown, the whole edit
 * (`edit`).
 */
export function previewMetrics(
  preview: readonly (readonly PreviewEdit[])[],
  edit: readonly (readonly number[])[],
): PreviewMetrics {
  const per = `p95 of ${String(EDITS)} edits per sample`;
  const pick = (value: (edit: PreviewEdit) => number) => preview.map((edits) => edits.map(value));
  const editPerRound = roundP95s(edit, 'edits');
  return {
    preview: {
      name: 'webview.preview-1000.p95',
      unit: 'ms',
      better: 'lower',
      gate: true,
      description: `Preview run from its start until its answer is applied (span b2c:preview), C++ panel folded, 1,000 blocks; ${per}`,
      samples: roundP95s(
        pick((run) => run.latencyMs),
        'previews',
      ),
    },
    update: {
      name: 'webview.preview-1000.update-p95',
      unit: 'ms',
      better: 'lower',
      gate: true,
      description: `update() alone where it runs (span b2c:preview:update), C++ panel folded, 1,000 blocks; ${per}`,
      samples: roundP95s(
        pick((run) => run.updateMs),
        'previews',
      ),
    },
    main: {
      name: 'webview.preview-1000.main-p95',
      unit: 'ms',
      better: 'lower',
      gate: false,
      description: `Preview's main-thread phases (read, receive, commit, publish), C++ panel folded, 1,000 blocks; ${per}`,
      samples: roundP95s(
        pick((run) => run.mainMs),
        'previews',
      ),
    },
    edit: {
      name: 'webview.edit-1000.p95-all',
      unit: 'ms',
      better: 'lower',
      gate: true,
      statistic: 'p95',
      description: `${EDIT_WHAT}; p95 of every edit of the run`,
      samples: edit.flat(),
    },
    editPerRound: {
      name: 'webview.edit-1000.p95',
      unit: 'ms',
      better: 'lower',
      gate: false,
      description: `${EDIT_WHAT}; ${per}`,
      samples: editPerRound,
    },
  };
}
