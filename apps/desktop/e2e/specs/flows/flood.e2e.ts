/**
 * Output flood protection (docs/spec/07-toolchain-build-run.md §7.6.5, 04 §4.5): a program that
 * prints 10,000,000 lines does not freeze the window. The console drops what it cannot show and
 * says so with a "… N lines skipped" marker, the editor answers while the output pours in, the end
 * of the output arrives, and Stop ends a flooding program within 5 s.
 */
import path from 'node:path';

import { describe, expect, it } from 'vitest';

import { NodeIds, print } from '../../support/bdm';
import { waitForConsoleState } from '../../support/console';
import { centerOn } from '../../support/editor';
import { clickTestId, openCategory } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { consoleSummary, timeToState, waitForConsoleText } from './lib/console';
import { type FlowApp, startFlow } from './lib/launch';
import { PROCESS_CHECK_TIMEOUT_MS, processesRunningFrom } from './lib/processes';
import { declare, forever, newEmptyProject, repeat } from './lib/project';
import { BUILD_TIMEOUT_MS, UI_TIMEOUT_MS, waitForToolchain } from './lib/ui';

/** The flood's line, and the program's first and last lines around it. */
const LINE = 'B2C flood line';
const START = 'B2C-FLOOD-START';
const END = 'B2C-FLOOD-END';

/** How many lines the finite flood prints (07 §7.6.5's example of a flood). */
const FLOOD_LINES = 10_000_000;

/** How long Stop may take (04 §4.5). */
const STOP_LIMIT_MS = 5_000;

/** How long the editor may take to answer while the console floods. */
const RESPONSIVE_LIMIT_MS = 5_000;

/**
 * How long the 10,000,000 lines may take to go through: about 20 s on a Linux machine with four
 * cores; ConPTY is slower on Windows.
 */
const FLOOD_TIMEOUT_MS = 120_000;

/**
 * Runs the open project and waits until its program has started printing. Its first line can
 * already have been dropped by the flood protection, so any of its lines will do.
 */
async function runFlood(app: FlowApp): Promise<void> {
  await clickTestId(app.driver, 'toolbar-run');
  await waitForConsoleText(app.driver, [START, LINE, END], BUILD_TIMEOUT_MS);
}

/** Waits until the console shows a "… N lines skipped" marker; returns its counts. */
async function waitForSkippedMarker(app: FlowApp, timeout: number): Promise<readonly number[]> {
  return waitFor(
    async () => {
      const { counts } = await consoleSummary(app.driver);
      return counts.length > 0 ? counts : null;
    },
    {
      timeout,
      interval: 100,
      message: async () =>
        `a "… N lines skipped" marker; the console ends with: ${(await consoleSummary(app.driver)).tail}`,
    },
  );
}

/**
 * Uses the editor while the console floods, and returns how long it took: a toolbox category
 * opens and its flyout settles, and a block is selected and centred.
 */
async function useTheEditor(app: FlowApp, blockId: string): Promise<number> {
  const started = Date.now();
  await openCategory(app.driver, 'Loops');
  await waitFor(() => app.hook.flyoutBlockId('control.repeat'), {
    timeout: RESPONSIVE_LIMIT_MS,
    message: 'the Loops category to show its blocks while the console floods',
  });
  await centerOn(app, blockId);
  expect(await app.hook.blockElement(blockId)).not.toBeNull();
  return Date.now() - started;
}

describe('Output flood protection', () => {
  it('shows the skipped marker for 10,000,000 lines and keeps the editor responsive', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    await waitForToolchain(app);
    const mainId = await newEmptyProject(app);
    const ids = new NodeIds('flood');
    await app.hook.insertBlocks(mainId, 'BODY', [
      declare(ids, {
        sym: 'sym_e2e_wait',
        name: 'wait',
        type: 'int',
        value: { expr: [{ num: '0' }] },
      }),
      print(ids, START),
      repeat(ids, { expr: [{ num: String(FLOOD_LINES) }] }, [print(ids, LINE)]),
      print(ids, END),
      // Then the program waits, so the test decides when it ends.
      {
        id: ids.next(),
        type: 'io.ask',
        v: 1,
        fields: { MODE: 'keep_asking', VAR: { ref: 'sym_e2e_wait' } },
        inputs: { PROMPT: { expr: [{ str: 'Done: ' }] } },
      },
    ]);

    await runFlood(app);
    // A marker shows once the console has fallen behind (and, on a machine fast enough to finish
    // the flood before the console catches up, before the last lines).
    const counts = await waitForSkippedMarker(app, FLOOD_TIMEOUT_MS);
    expect(counts.every((count) => Number.isSafeInteger(count) && count >= 0)).toBe(true);
    expect(Math.max(...counts)).toBeGreaterThan(0);

    // The editor answers while the program runs.
    expect(await useTheEditor(app, mainId)).toBeLessThanOrEqual(RESPONSIVE_LIMIT_MS);

    // The end of the output always arrives (the console is sent the last lines it can show), and
    // nothing of the flood comes after it.
    await waitForConsoleText(app.driver, [END], FLOOD_TIMEOUT_MS);
    const end = await consoleSummary(app.driver, [END]);
    expect(end.tail).toContain(END);
    expect(end.tail.slice(end.tail.lastIndexOf(END))).not.toContain(LINE);
    await waitForConsoleState(app.driver, /Running/, UI_TIMEOUT_MS);

    await clickTestId(app.driver, 'console-stop');
    expect(await timeToState(app.driver, /^■?\s*Stopped$/, STOP_LIMIT_MS)).toBeLessThanOrEqual(
      STOP_LIMIT_MS,
    );
  }, 180_000);

  it('keeps the editor responsive and stops a program that floods the console within 5 s', async (context) => {
    const flow = startFlow(context);
    const cache = path.join(flow.folders.profile, 'cache');
    const app = await flow.launch();
    await waitForToolchain(app);
    const mainId = await newEmptyProject(app);
    const ids = new NodeIds('endless');
    await app.hook.insertBlocks(mainId, 'BODY', [
      print(ids, START),
      forever(ids, [print(ids, LINE)]),
    ]);

    await runFlood(app);
    await waitForSkippedMarker(app, FLOOD_TIMEOUT_MS);
    // The program never stops printing, so this is while the console floods.
    expect(await useTheEditor(app, mainId)).toBeLessThanOrEqual(RESPONSIVE_LIMIT_MS);
    await waitForConsoleState(app.driver, /Running/, UI_TIMEOUT_MS);

    await clickTestId(app.driver, 'console-stop');
    expect(await timeToState(app.driver, /^■?\s*Stopped$/, STOP_LIMIT_MS)).toBeLessThanOrEqual(
      STOP_LIMIT_MS,
    );
    await waitFor(async () => (await processesRunningFrom(cache)).length === 0, {
      timeout: PROCESS_CHECK_TIMEOUT_MS,
      message: 'the flooding program to end',
    });
  }, 180_000);
});
