/**
 * Stopping a program (docs/spec/07-toolchain-build-run.md §7.5.4, §7.6.2, 04 §4.5): a program
 * that loops forever is stopped with the console's ■ Stop button, run again with ⟲ Run again, and
 * stopped with Shift+F5. Each time the header says *Stopped* within 5 s and no process of the
 * program is left.
 */
import path from 'node:path';

import { Key } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { NodeIds, print } from '../../support/bdm';
import { waitForConsoleState } from '../../support/console';
import { clickTestId } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { timeToState, waitForTranscript } from './lib/console';
import { type FlowApp, startFlow } from './lib/launch';
import { PROCESS_CHECK_TIMEOUT_MS, processesRunningFrom } from './lib/processes';
import { changeBy, declare, forever, newEmptyProject } from './lib/project';
import { BUILD_TIMEOUT_MS, pressShift, UI_TIMEOUT_MS, waitForToolchain } from './lib/ui';

/** What the program prints before its endless loop. */
const STARTED = 'B2C-LOOP-STARTED';

/** How long Stop may take, at most (04 §4.5). */
const STOP_LIMIT_MS = 5_000;

/** How many times the transcript has {@link STARTED}. */
function starts(text: string): number {
  return text.split(STARTED).length - 1;
}

/** Waits for run number `run` to print its first line and the header to say Running. */
async function waitForRun(app: FlowApp, run: number): Promise<void> {
  await waitForTranscript(
    app,
    (text) => starts(text) >= run,
    BUILD_TIMEOUT_MS,
    `run ${String(run)} of the endless loop`,
  );
  await waitForConsoleState(app.driver, /Running/, UI_TIMEOUT_MS);
}

/** Checks that the program's processes are gone (the whole tree is killed, 07 §7.5.4). */
async function expectNoProgram(cache: string): Promise<void> {
  await waitFor(async () => (await processesRunningFrom(cache)).length === 0, {
    timeout: PROCESS_CHECK_TIMEOUT_MS,
    message: async () =>
      `the stopped program's processes to end (still running: ${(await processesRunningFrom(cache))
        .map((entry) => String(entry.pid))
        .join(', ')})`,
  });
}

describe('Stop', () => {
  it('stops an endless loop with the console button and with Shift+F5', async (context) => {
    const flow = startFlow(context);
    const cache = path.join(flow.folders.profile, 'cache');
    const app = await flow.launch();
    await waitForToolchain(app);
    const mainId = await newEmptyProject(app);
    const ids = new NodeIds('stop');
    await app.hook.insertBlocks(mainId, 'BODY', [
      declare(ids, { sym: 'sym_e2e_n', name: 'n', type: 'int', value: { expr: [{ num: '0' }] } }),
      print(ids, STARTED),
      forever(ids, [changeBy(ids, 'sym_e2e_n', { expr: [{ num: '1' }] })]),
    ]);

    // (1) Run, then the console's ■ Stop.
    await clickTestId(app.driver, 'toolbar-run');
    await waitForRun(app, 1);
    expect((await processesRunningFrom(cache)).length).toBeGreaterThan(0);
    await clickTestId(app.driver, 'console-stop');
    expect(await timeToState(app.driver, /^■?\s*Stopped$/, STOP_LIMIT_MS)).toBeLessThanOrEqual(
      STOP_LIMIT_MS,
    );
    await expectNoProgram(cache);

    // (2) ⟲ Run again, then Shift+F5.
    await clickTestId(app.driver, 'console-run-again');
    await waitForRun(app, 2);
    expect((await processesRunningFrom(cache)).length).toBeGreaterThan(0);
    await pressShift(app.driver, Key.F5);
    expect(await timeToState(app.driver, /^■?\s*Stopped$/, STOP_LIMIT_MS)).toBeLessThanOrEqual(
      STOP_LIMIT_MS,
    );
    await expectNoProgram(cache);
  }, 180_000);
});
