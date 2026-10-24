/**
 * The metrics of the preview benchmark (preview.bench.e2e.ts), taken from what it measured in
 * each edit, round by round: one round of {@link EDITS} edits per sample.
 *
 * - `webview.preview-1000.p95` (gated): each round's p95 of the preview task is one sample.
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

/** What the edits with the C++ panel shown measure, less the pipeline's debounce. */
const EDIT_WHAT =
  'Edit until the page settles (rendering, preview, C++ panel shown) less the 50 ms debounce, 1,000 blocks';

/** The metrics of one run of the preview benchmark. */
export interface PreviewMetrics {
  readonly preview: BenchMetric;
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
 * the C++ panel folded, the preview task (`preview`), and with it shown, the whole edit (`edit`).
 */
export function previewMetrics(
  preview: readonly (readonly number[])[],
  edit: readonly (readonly number[])[],
): PreviewMetrics {
  const per = `p95 of ${String(EDITS)} edits per sample`;
  const editPerRound = roundP95s(edit, 'edits');
  return {
    preview: {
      name: 'webview.preview-1000.p95',
      unit: 'ms',
      better: 'lower',
      gate: true,
      description: `Preview pipeline task (canvas, update, store) at 1,000 blocks; ${per}`,
      samples: roundP95s(preview, 'preview tasks'),
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
