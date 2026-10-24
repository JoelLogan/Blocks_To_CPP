/**
 * Closing the app kills the program tree (docs/spec/07-toolchain-build-run.md §7.6.2, 02 §2.5
 * "Window close with unsaved changes"): a program waits for input in the console, the window is
 * closed with its close button, and once the app has exited no process runs from the build cache
 * any more. With no unsaved changes the window closes at once; with unsaved changes the app asks
 * first, and *Don't save* quits.
 */
import path from 'node:path';

import { describe, expect, it } from 'vitest';

import { NodeIds, print } from '../../support/bdm';
import { waitForConsoleState } from '../../support/console';
import { clickTestId, testIdText } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { waitForTranscript } from './lib/console';
import { type FlowApp, startFlow } from './lib/launch';
import { PROCESS_CHECK_TIMEOUT_MS, processesRunningFrom } from './lib/processes';
import { declare, newEmptyProject, waitForWrite } from './lib/project';
import {
  answerDialog,
  BUILD_TIMEOUT_MS,
  pressCtrl,
  UI_TIMEOUT_MS,
  waitForDialog,
  waitForToolchain,
} from './lib/ui';

/** What the program prints before it waits for input. */
const WAITING = 'B2C-WAITING-FOR-INPUT';

/** A program that prints {@link WAITING} and then waits for a number in the console. */
function waitingProgram(ids: NodeIds) {
  return [
    declare(ids, {
      sym: 'sym_e2e_answer',
      name: 'answer',
      type: 'int',
      value: { expr: [{ num: '0' }] },
    }),
    print(ids, WAITING),
    {
      id: ids.next(),
      type: 'io.ask',
      v: 1,
      fields: { MODE: 'keep_asking', VAR: { ref: 'sym_e2e_answer' } },
      inputs: { PROMPT: { expr: [{ str: 'Number: ' }] } },
    },
  ];
}

/** Runs the open project and waits until its program waits for input; returns its processes. */
async function startWaitingProgram(app: FlowApp, cache: string) {
  await clickTestId(app.driver, 'toolbar-run');
  await waitForTranscript(
    app,
    (text) => text.includes(WAITING),
    BUILD_TIMEOUT_MS,
    'the program to start',
  );
  await waitForConsoleState(app.driver, /Running/, UI_TIMEOUT_MS);
  return waitFor(
    async () => {
      const found = await processesRunningFrom(cache);
      return found.length > 0 ? found : null;
    },
    { timeout: PROCESS_CHECK_TIMEOUT_MS, message: `a process running from ${cache}` },
  );
}

/** Waits until no process runs from the build cache; fails naming those that still do. */
async function expectNoProgramLeft(cache: string): Promise<void> {
  const left = await waitFor(
    async () => ((await processesRunningFrom(cache)).length === 0 ? [] : null),
    {
      timeout: PROCESS_CHECK_TIMEOUT_MS,
      interval: 250,
      message: async () =>
        `no process running from ${cache}; still running: ${(await processesRunningFrom(cache))
          .map((entry) => `${String(entry.pid)} ${entry.exe ?? '?'}`)
          .join(', ')}`,
    },
  );
  expect(left).toEqual([]);
}

describe('Closing the app', () => {
  it('kills the running program when the window of a saved project is closed', async (context) => {
    const flow = startFlow(context);
    const cache = path.join(flow.folders.profile, 'cache');
    const file = path.join(flow.folders.projects, 'Waiting.b2c');
    const app = await flow.launch({ dialogs: { saveAs: [file] } });
    await waitForToolchain(app);
    const mainId = await newEmptyProject(app);
    await app.hook.insertBlocks(mainId, 'BODY', waitingProgram(new NodeIds('close')));

    // Saved, so closing the window asks nothing.
    await pressCtrl(app.driver, 's');
    await waitForWrite(file, null);
    await waitFor(async () => !(await testIdText(app.driver, 'project-name')).includes('•'), {
      timeout: UI_TIMEOUT_MS,
      message: 'the project to have no unsaved changes',
    });

    const programs = await startWaitingProgram(app, cache);
    expect(programs.length).toBeGreaterThan(0);

    await app.closeWindow();
    await app.waitForExit();
    await expectNoProgramLeft(cache);
  }, 180_000);

  it("asks about unsaved changes: Cancel keeps it running, Don't save quits and kills the program", async (context) => {
    const flow = startFlow(context);
    const cache = path.join(flow.folders.profile, 'cache');
    const app = await flow.launch();
    await waitForToolchain(app);
    const mainId = await newEmptyProject(app);
    await app.hook.insertBlocks(mainId, 'BODY', waitingProgram(new NodeIds('dirty')));
    await startWaitingProgram(app, cache);

    // Cancel keeps the window, the project and the program.
    await app.closeWindow();
    await waitForDialog(app.driver, /Save changes to/);
    await answerDialog(app.driver, 'Cancel');
    await waitForConsoleState(app.driver, /Running/, UI_TIMEOUT_MS);
    expect((await processesRunningFrom(cache)).length).toBeGreaterThan(0);

    // Don't save quits.
    await app.closeWindow();
    await waitForDialog(app.driver, /Save changes to/);
    await answerDialog(app.driver, "Don't save");
    await app.waitForExit();
    await expectNoProgramLeft(cache);
  }, 180_000);
});
