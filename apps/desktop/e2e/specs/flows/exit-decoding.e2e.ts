/**
 * How a program ended, in the console header (docs/spec/07-toolchain-build-run.md §7.6.4): the
 * exit code of `examples/exit_code.b2c` (`stop program with exit code 3`), and the friendly text of
 * a crash, an integer division by zero in a Release build (no sanitizers, so the processor's own
 * fault: `SIGFPE` on Linux, exception `0xC0000094` on Windows).
 */
import { By } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { NodeIds, print, randomInt } from '../../support/bdm';
import { consoleState, waitForConsoleState } from '../../support/console';
import { byTestId, clickTestId, testIdText } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { startFlow } from './lib/launch';
import {
  arithmetic,
  block,
  copyExample,
  declare,
  newEmptyProject,
  openFromStartPage,
  printValue,
  ref,
} from './lib/project';
import {
  BUILD_TIMEOUT_MS,
  heldBack,
  UI_TIMEOUT_MS,
  waitForTestId,
  waitForToolchain,
} from './lib/ui';

/**
 * Exit code 3 (07 §7.6.4): an ordinary exit code on Linux; on Windows also what `abort()` exits
 * with, so it is described as the program stopping itself.
 */
const EXIT_CODE_3 =
  process.platform === 'win32'
    ? /Stopped itself: an uncaught error or failed check \(exit code 3\)/
    : /Finished with exit code 3$/;

/** An integer division by zero, with its technical name in brackets. */
const DIVISION_BY_ZERO =
  process.platform === 'win32'
    ? /Crashed: integer division by zero \(exception 0xC0000094\)/
    : /Crashed: integer division by zero \(SIGFPE\)/;

describe('Exit decoding', () => {
  it('shows the exit code of exit_code.b2c', async (context) => {
    const flow = startFlow(context);
    const example = copyExample('exit_code', flow.folders.projects);
    const app = await flow.launch({ dialogs: { open: [example], trust: ['trustProject'] } });
    await waitForToolchain(app);
    await openFromStartPage(app);

    // A copied example is restricted until it is trusted (08 §8.3).
    await waitForTestId(app.driver, 'restricted-banner', true);
    await clickTestId(app.driver, 'restricted-banner-trust');
    await waitForTestId(app.driver, 'restricted-banner', false);
    await waitFor(async () => !(await heldBack(app.driver, 'toolbar-run')), {
      timeout: UI_TIMEOUT_MS,
      message: 'Run to be enabled',
    });

    await clickTestId(app.driver, 'toolbar-run');
    const state = await waitForConsoleState(app.driver, EXIT_CODE_3, BUILD_TIMEOUT_MS);
    expect(state).toMatch(EXIT_CODE_3);
    const transcript = await app.hook.consoleText();
    expect(transcript).toContain('Checking...');
    expect(transcript).toContain('Stopping early');
    expect(transcript).not.toContain('This line is never printed');
  }, 180_000);

  it('shows the 07 §7.6.4 text for a Release program that divides by zero', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    await waitForToolchain(app);
    const mainId = await newEmptyProject(app);
    const ids = new NodeIds('crash');
    // `zero` comes from the random generator, so the compiler cannot see the division coming.
    await app.hook.insertBlocks(mainId, 'BODY', [
      declare(ids, {
        sym: 'sym_e2e_zero',
        name: 'zero',
        type: 'int',
        value: block(randomInt(ids, 0, 0)),
      }),
      print(ids, 'B2C-BEFORE-THE-CRASH'),
      printValue(
        ids,
        block(arithmetic(ids, 'div', { expr: [{ num: '100' }] }, ref('sym_e2e_zero'))),
      ),
    ]);

    // Release: optimised, without the sanitizers of Debug (05 §5.3 build configurations).
    const config = await app.driver.findElement(byTestId('toolbar-config'));
    await config.findElement(By.css('option[value="release"]')).click();
    await waitFor(async () => (await config.getAttribute('value')) === 'release', {
      timeout: UI_TIMEOUT_MS,
      message: 'the Release configuration to be chosen',
    });
    expect(await testIdText(app.driver, 'status-config')).toContain('Release');

    await clickTestId(app.driver, 'toolbar-run');
    await waitForConsoleState(app.driver, DIVISION_BY_ZERO, BUILD_TIMEOUT_MS);
    expect(await consoleState(app.driver)).toMatch(/^✖?\s*Crashed: integer division by zero/);
    expect(await app.hook.consoleText()).toContain('B2C-BEFORE-THE-CRASH');
  }, 180_000);
});
