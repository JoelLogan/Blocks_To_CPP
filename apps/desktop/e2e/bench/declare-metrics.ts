/**
 * The metrics of the declaration benchmark (declare.bench.e2e.ts), taken from what it measured in
 * each declaration, round by round: one round of {@link DECLARATIONS} declarations per sample.
 *
 * - `webview.declare-variable.toolbox-refresh` (reported): on the guessing game, each round's
 *   median time of the toolbox's refresh (the User Timing span `b2c:toolbox-refresh`) after a new
 *   variable is declared in `main`, is one sample;
 * - `webview.declare-variable-1000.toolbox-refresh` (reported): the same on the 1,000-block
 *   document;
 * - `webview.declare-variable.max-task` (reported): on the guessing game, each round's median of
 *   the longest task after a declaration, until the page has finished what it started.
 *
 * All three are report-only until they have five nightly baselines (README.md "Gating").
 */
import type { BenchMetric } from './results';
import { median } from './stats';

/** Declarations per sample. */
export const DECLARATIONS = 10;

/** The span of the toolbox's refresh (src/lib/timing.ts). */
export const TOOLBOX_REFRESH_SPAN = 'b2c:toolbox-refresh';

/** What one declaration measured (milliseconds). */
export interface Declaration {
  /** The toolbox's refresh after it (the span's time; 0 when the toolbox did not refresh). */
  readonly refreshMs: number;
  /** The longest task from the declaration until the page had finished what it started. */
  readonly longestMs: number;
}

/** The metrics of one run of the declaration benchmark. */
export interface DeclareMetrics {
  readonly refresh: BenchMetric;
  readonly refresh1000: BenchMetric;
  readonly maxTask: BenchMetric;
}

/**
 * Each round's median of `pick`, one sample per round; every round has {@link DECLARATIONS}
 * declarations, and each must have refreshed the toolbox (a sample never lacks that work).
 */
function roundMedians(
  rounds: readonly (readonly Declaration[])[],
  pick: (declaration: Declaration) => number,
  what: string,
): number[] {
  return rounds.map((declarations, round) => {
    if (declarations.length !== DECLARATIONS) {
      throw new Error(
        `Round ${String(round)} on ${what} has ${String(declarations.length)} declarations, not ${String(DECLARATIONS)}`,
      );
    }
    const missing = declarations.findIndex((declaration) => declaration.refreshMs <= 0);
    if (missing >= 0) {
      throw new Error(
        `Declaration ${String(missing)} of round ${String(round)} on ${what} did not refresh the toolbox`,
      );
    }
    return median(declarations.map(pick));
  });
}

/**
 * The metrics of a run from what each declaration measured, one list of {@link DECLARATIONS} per
 * round: on the guessing game (`guessing`) and on the 1,000-block document (`large`).
 */
export function declareMetrics(
  guessing: readonly (readonly Declaration[])[],
  large: readonly (readonly Declaration[])[],
): DeclareMetrics {
  const per = `median of ${String(DECLARATIONS)} declarations per sample; report-only`;
  const what = 'Toolbox refresh after a new int variable is declared at the end of main';
  return {
    refresh: {
      name: 'webview.declare-variable.toolbox-refresh',
      unit: 'ms',
      better: 'lower',
      gate: false,
      description: `${what}, guessing game (40 blocks); ${per}`,
      samples: roundMedians(guessing, (declaration) => declaration.refreshMs, 'the guessing game'),
    },
    refresh1000: {
      name: 'webview.declare-variable-1000.toolbox-refresh',
      unit: 'ms',
      better: 'lower',
      gate: false,
      description: `${what}, 1,000 blocks; ${per}`,
      samples: roundMedians(large, (declaration) => declaration.refreshMs, '1,000 blocks'),
    },
    maxTask: {
      name: 'webview.declare-variable.max-task',
      unit: 'ms',
      better: 'lower',
      gate: false,
      description: `Longest main-thread task after a new int variable is declared at the end of main, until the page settles, guessing game; ${per}`,
      samples: roundMedians(guessing, (declaration) => declaration.longestMs, 'the guessing game'),
    },
  };
}
