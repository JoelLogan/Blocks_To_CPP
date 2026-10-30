/**
 * Webview benchmark: the guessing game, a small program (docs/spec/01-overview.md §1.4 N12: no
 * main-thread task over 50 ms while a program of up to 200 blocks is edited; 09 §9.2: "on the
 * guessing game, the longest task after a selection and after an edit"). The example
 * (examples/guessing_game.b2c, 40 blocks) is opened with the C++ panel shown, as the editor starts,
 * and two report-only metrics are taken, one sample per interaction:
 *
 * - `webview.guessing-game.select.max-task`: blocks in different scopes are clicked in turn with
 *   real pointer input (bench/targets.ts `GUESSING_TARGETS`), each scrolled into view first
 *   through the test hook; the longest task from the press until the page has run no long task
 *   for 300 ms. A first click warms up and is not counted. The clicks start at target
 *   `B2C_BENCH_OFFSET`, as in the 1,000-block selection benchmark.
 * - `webview.guessing-game.edit.max-task`: a print with a unique text is added at the end of
 *   `main` through the test hook; the longest task from the insertion until its C++ is in the
 *   editor's state and the page has run no long task for 300 ms (Blockly's rendering, the
 *   preview, the C++ panel and the toolbox). The print is deleted again with the keyboard, outside
 *   the timing.
 *
 * Both are gated (M3-9, docs/plans/m3-contracts/w5-performance.md §6): a regression of more than
 * the gate's 10% fails the nightly, and so does a median over N12's budget of 50 ms
 * ({@link SMALL_PROGRAM_BUDGET_MS}), checked here.
 */
import { copyFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterAll, describe, expect, it } from 'vitest';

import { launchApp } from '../support/app';
import { NodeIds, print } from '../support/bdm';
import { REPOSITORY_ROOT } from '../support/env';
import { requireHookMethods } from './capabilities';
import {
  deleteBlock,
  SETTLE_MS,
  type SpanTimes,
  spanSummary,
  timedClick,
  timedEdit,
  waitUntilQuiet,
} from './interaction';
import { openProjectFile } from './project';
import { benchOffset, benchSamples, writeMetric } from './results';
import { median, rounded } from './stats';
import { GUESSING_MAIN_ID, GUESSING_TARGETS, targetAt } from './targets';

/** How long opening the example may take. */
const OPEN_TIMEOUT_MS = 60_000;

/** N12: no main-thread task over 50 ms while a program of up to 200 blocks is edited. */
export const SMALL_PROGRAM_BUDGET_MS = 50;

/** How long the whole benchmark may take. */
const TEST_TIMEOUT_MS = 900_000;

const folders: string[] = [];

afterAll(() => {
  for (const folder of folders) {
    rmSync(folder, { recursive: true, force: true });
  }
});

describe('webview benchmark: the guessing game', () => {
  it(
    'times the longest task after selections and after edits',
    async (context) => {
      const samples = benchSamples();
      const offset = benchOffset();
      const folder = mkdtempSync(path.join(tmpdir(), 'b2c-bench-guessing-'));
      folders.push(folder);
      const file = path.join(folder, 'guessing_game.b2c');
      copyFileSync(path.join(REPOSITORY_ROOT, 'examples', 'guessing_game.b2c'), file);
      const app = await launchApp(context, { dialogs: { open: [file] }, recordLongTasks: false });
      await requireHookMethods(context, app, 'guessing');
      await openProjectFile(app, {
        blockId: GUESSING_MAIN_ID,
        code: 'Guess a number',
        timeout: OPEN_TIMEOUT_MS,
      });
      await waitUntilQuiet(app, 'opening the guessing game');

      await timedClick(app, targetAt(GUESSING_TARGETS, offset + GUESSING_TARGETS.length - 1));
      const select: number[] = [];
      const selectSpans: SpanTimes[] = [];
      for (let sample = 0; sample < samples; sample += 1) {
        const timed = await timedClick(app, targetAt(GUESSING_TARGETS, offset + sample));
        select.push(timed.longestMs);
        selectSpans.push(timed.spans);
      }

      const ids = new NodeIds('bench');
      const edit: number[] = [];
      const editSpans: SpanTimes[] = [];
      for (let sample = 0; sample < samples; sample += 1) {
        const marker = `bench edit ${String(sample)}`;
        const timed = await timedEdit(app, print(ids, marker), {
          parentId: GUESSING_MAIN_ID,
          input: 'BODY',
          marker,
          settleMs: SETTLE_MS,
        });
        edit.push(timed.longestMs);
        editSpans.push(timed.spans);
        await deleteBlock(app, timed.id, marker, true);
      }

      const per = `one per sample, C++ panel shown; gated, and its median at most ${String(SMALL_PROGRAM_BUDGET_MS)} ms (N12)`;
      writeMetric({
        name: 'webview.guessing-game.select.max-task',
        unit: 'ms',
        better: 'lower',
        gate: true,
        description: `Guessing game: longest main-thread task after clicking a block in another scope; ${per}`,
        samples: select,
      });
      writeMetric({
        name: 'webview.guessing-game.edit.max-task',
        unit: 'ms',
        better: 'lower',
        gate: true,
        description: `Guessing game: longest main-thread task after adding a print to main; ${per}`,
        samples: edit,
      });
      process.stdout.write(
        `Guessing game: longest task after a selection median ${String(rounded(median(select), 1))} ms, ` +
          `after an edit median ${String(rounded(median(edit), 1))} ms\n` +
          `  spans per selection (median): ${spanSummary(selectSpans)}\n` +
          `  spans per edit (median): ${spanSummary(editSpans)}\n`,
      );
      expect(select).toHaveLength(samples);
      expect(edit).toHaveLength(samples);
      // The small-program budget (N12), checked after the results are written so a run over it
      // still leaves them for the comparison.
      expect(median(select), 'the median longest task after a selection').toBeLessThanOrEqual(
        SMALL_PROGRAM_BUDGET_MS,
      );
      expect(median(edit), 'the median longest task after an edit').toBeLessThanOrEqual(
        SMALL_PROGRAM_BUDGET_MS,
      );
    },
    TEST_TIMEOUT_MS,
  );
});
