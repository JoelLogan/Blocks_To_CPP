/**
 * Webview benchmark: a program's output in the console (docs/spec/04-user-interface.md §4.5, 09
 * §9.2: "a program's output in the console"). A new project from the start page's Empty template
 * gets a program that prints a first line, {@link LINES} lines in a *repeat* loop and a last line
 * (blocks inserted through the test hook), and is run once to build it (not counted). Then each
 * run clears the console, outside the timing, and clicks ► Run with real pointer input; the page
 * times from the press until the console's state says *Finished*, and records its tasks until it
 * has run no long task for 300 ms (interaction.ts `timedRun`). The project is built already, so a
 * run is the program writing its lines and the console showing them: the backend's output events,
 * the terminal's parsing and rendering, and its accessibility tree once the console's
 * screen-reader output is on (W3-11 of the M2 polish plan). The console's work keeps the page
 * busy most of the run, which varies by about 10% from run to run, so each sample is the median of
 * {@link RUNS_PER_SAMPLE} runs. Two metrics:
 *
 * - `webview.console-200000.run` (gated): from the press until the console says *Finished*;
 * - `webview.console-200000.max-task` (report-only): the longest main-thread task of a run.
 *
 * After each run, outside the timing, the console must end with the last line, without a "… N
 * lines skipped" marker: the output (about 3 MB) is less than the backend lets go unacknowledged
 * (4 MiB, 07 §7.6.5), so nothing is dropped and every run shows the same output.
 */
import { describe, expect, it } from 'vitest';

import { E2E_HOOK_NAME } from '../../src/e2e/contract';
import { launchApp } from '../support/app';
import { NodeIds, print } from '../support/bdm';
import { consoleState, showConsole, waitForConsoleState } from '../support/console';
import { clickTestId } from '../support/ui';
import { waitFor } from '../support/wait';
import { newEmptyProject, repeat } from '../specs/flows/lib/project';
import { BUILD_TIMEOUT_MS, waitForToolchain } from '../specs/flows/lib/ui';
import { type SpanTimes, spanSummary, type TimedRun, timedRun } from './interaction';
import { RUN_OUTPUT_SCRIPT, type RunOutput } from './page';
import { benchSamples, writeMetric } from './results';
import { median, rounded } from './stats';

/** How many lines the program prints in its loop. */
const LINES = 200_000;

/** The loop's line, and the program's first and last lines around it. */
const LINE = 'B2C bench line';
const FIRST = 'B2C-CONSOLE-FIRST';
const LAST = 'B2C-CONSOLE-LAST';

/** How many runs one sample takes the median of. */
const RUNS_PER_SAMPLE = 3;

/** How long the whole benchmark may take (the first run compiles the program). */
const TEST_TIMEOUT_MS = 900_000;

/** The console's transcript, counted in the page. */
async function runOutput(app: Awaited<ReturnType<typeof launchApp>>): Promise<RunOutput> {
  const output: unknown = await app.driver.executeScript(RUN_OUTPUT_SCRIPT, E2E_HOOK_NAME, LINE);
  if (
    typeof output !== 'object' ||
    output === null ||
    typeof (output as RunOutput).lines !== 'number' ||
    typeof (output as RunOutput).skipped !== 'boolean' ||
    typeof (output as RunOutput).last !== 'string'
  ) {
    throw new Error('The console transcript could not be read');
  }
  return output as RunOutput;
}

/**
 * Fails unless the console shows the whole output of one run: it ends with the last line, and no
 * output was dropped (the flood protection would have left a "… N lines skipped" marker). The
 * transcript keeps only the console's last megabyte, so the lines are not counted.
 */
async function checkOutput(app: Awaited<ReturnType<typeof launchApp>>): Promise<void> {
  const output = await runOutput(app);
  if (output.lines === 0 || output.skipped || output.last !== LAST) {
    throw new Error(
      `The console holds ${String(output.lines)} of the lines` +
        `${output.skipped ? ' and a skipped-lines marker' : ''}, and ends with "${output.last}"` +
        ` (after the state "${await consoleState(app.driver)}")`,
    );
  }
}

/** Clears the console, outside the timing, so that every run starts with an empty terminal. */
async function clearConsole(app: Awaited<ReturnType<typeof launchApp>>): Promise<void> {
  await clickTestId(app.driver, 'console-clear');
  await waitFor(async () => (await runOutput(app)).lines === 0, {
    timeout: 10_000,
    message: 'the console to be cleared',
  });
}

describe('webview benchmark: a program’s output in the console', () => {
  it(
    `times runs that print ${String(LINES)} lines`,
    async (context) => {
      const samples = benchSamples();
      const app = await launchApp(context, { recordLongTasks: false });
      await waitForToolchain(app);
      const mainId = await newEmptyProject(app);
      const ids = new NodeIds('console');
      await app.hook.insertBlocks(mainId, 'BODY', [
        print(ids, FIRST),
        repeat(ids, { expr: [{ num: String(LINES) }] }, [print(ids, LINE)]),
        print(ids, LAST),
      ]);
      await showConsole(app.driver);

      // The first run builds the program: not counted.
      await clickTestId(app.driver, 'toolbar-run');
      await waitForConsoleState(app.driver, /Finished \(exit code 0\)/, BUILD_TIMEOUT_MS);
      await waitFor(async () => (await runOutput(app)).last === LAST, {
        timeout: 30_000,
        message: 'the last line of the first run in the console',
      });

      const run: number[] = [];
      const longest: number[] = [];
      const spans: SpanTimes[] = [];
      for (let sample = 0; sample < samples; sample += 1) {
        const runs: TimedRun[] = [];
        for (let index = 0; index < RUNS_PER_SAMPLE; index += 1) {
          await clearConsole(app);
          const timed = await timedRun(app);
          expect(await consoleState(app.driver)).toMatch(/Finished \(exit code 0\)/);
          await checkOutput(app);
          runs.push(timed);
          spans.push(timed.spans);
        }
        run.push(median(runs.map((timed) => timed.runMs)));
        longest.push(median(runs.map((timed) => timed.longestMs)));
      }
      const what = `a built program printing ${String(LINES)} lines; median of ${String(RUNS_PER_SAMPLE)} runs per sample`;
      writeMetric({
        name: 'webview.console-200000.run',
        unit: 'ms',
        better: 'lower',
        gate: true,
        description: `From the click on Run until the console says Finished: ${what}`,
        samples: run,
      });
      writeMetric({
        name: 'webview.console-200000.max-task',
        unit: 'ms',
        better: 'lower',
        gate: false,
        description: `Longest main-thread task of a run: ${what}`,
        samples: longest,
      });
      process.stdout.write(
        `Console output (${String(LINES)} lines): run median ${String(rounded(median(run), 1))} ms, ` +
          `longest task median ${String(rounded(median(longest), 1))} ms ` +
          `(${String(samples)} samples of ${String(RUNS_PER_SAMPLE)} runs)\n` +
          `  spans per run (median): ${spanSummary(spans)}\n`,
      );
      expect(run).toHaveLength(samples);
    },
    TEST_TIMEOUT_MS,
  );
});
