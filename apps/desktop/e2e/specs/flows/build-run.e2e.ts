/**
 * Build, and run with input (docs/spec/04-user-interface.md §4.4–4.5, 07 §7.5–7.6): Ctrl+B builds
 * without running and the Build output tab says so; a second Ctrl+B finds nothing to do; F5 runs
 * the program, which reads a number typed into the console and answers.
 */
import { Key } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { NodeIds } from '../../support/bdm';
import {
  consoleState,
  showConsole,
  typeIntoConsole,
  waitForConsoleState,
} from '../../support/console';
import { pressKey, testIdText } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { waitForTranscript } from './lib/console';
import { type FlowApp, startFlow } from './lib/launch';
import { arithmetic, block, declare, newEmptyProject, printValue, ref } from './lib/project';
import { BUILD_TIMEOUT_MS, pressCtrl, UI_TIMEOUT_MS, waitForToolchain } from './lib/ui';

/** The prompt of the test's program. */
const PROMPT = 'Number to double: ';

/** The Build output tab's text, waiting until it matches `expected`. */
function waitForBuildOutput(app: FlowApp, expected: RegExp, timeout: number): Promise<string> {
  return waitFor(
    async () => {
      const text = await testIdText(app.driver, 'build-output-panel');
      return expected.test(text) ? text : null;
    },
    {
      timeout,
      message: async () =>
        `the build output to match ${String(expected)} (it has "${await testIdText(app.driver, 'build-output-panel')}")`,
    },
  );
}

describe('Build and run', () => {
  it('builds with Ctrl+B, finds nothing to rebuild, and runs a program that reads input', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    await waitForToolchain(app);
    const mainId = await newEmptyProject(app);
    const ids = new NodeIds('build');
    await app.hook.insertBlocks(mainId, 'BODY', [
      declare(ids, { sym: 'sym_e2e_n', name: 'n', type: 'int', value: { expr: [{ num: '0' }] } }),
      {
        id: ids.next(),
        type: 'io.ask',
        v: 1,
        fields: { MODE: 'keep_asking', VAR: { ref: 'sym_e2e_n' } },
        inputs: { PROMPT: { expr: [{ str: PROMPT }] } },
      },
      printValue(ids, block(arithmetic(ids, 'mul', ref('sym_e2e_n'), { expr: [{ num: '2' }] }))),
    ]);

    // (1) Ctrl+B builds, and only builds.
    await pressCtrl(app.driver, 'b');
    const built = await waitForBuildOutput(app, /Built in [\d.]+ s\./, BUILD_TIMEOUT_MS);
    expect(built).toMatch(/Building My Project \(Debug\)…/);
    expect(built).toMatch(/Compiling \(\d+\/\d+\)/);
    expect(await consoleState(app.driver)).toBe('Not running');

    // (2) Nothing changed: nothing to build.
    await pressCtrl(app.driver, 'b');
    await waitForBuildOutput(
      app,
      /Up to date: nothing changed since the last build\./,
      UI_TIMEOUT_MS,
    );

    // (3) F5 runs it; the number typed into the console comes back doubled.
    await pressKey(app.driver, Key.F5);
    await waitForTranscript(app, (text) => text.includes(PROMPT), BUILD_TIMEOUT_MS, 'the prompt');
    await showConsole(app.driver);
    await typeIntoConsole(app.driver, `21${Key.ENTER}`);
    await waitForConsoleState(app.driver, /Finished \(exit code 0\)$/, UI_TIMEOUT_MS);
    const transcript = await app.hook.consoleText();
    expect(transcript.slice(transcript.indexOf(PROMPT))).toMatch(/42/);
  }, 180_000);
});
