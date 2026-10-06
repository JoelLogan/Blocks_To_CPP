/**
 * The toolchain setup page (docs/spec/04-user-interface.md §4.6, 07 §7.1–7.3): with no g++ where
 * the app looks, it opens the setup page by itself, which explains what a compiler is and shows the
 * platform's install command (apt on Ubuntu). Once g++ is back, *I installed it → Rescan* finds it,
 * the status bar names it, and a program builds and runs.
 */
import { describe, expect, it } from 'vitest';

import { NodeIds, print } from '../../support/bdm';
import { waitForConsoleState } from '../../support/console';
import { harnessSettings } from '../../support/env';
import { clickTestId, testIdText } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { startFlow } from './lib/launch';
import { newEmptyProject } from './lib/project';
import {
  expectedInstallCommands,
  hideToolchains,
  INSTALL_COMMANDS,
  parseOsRelease,
  readOsRelease,
} from './lib/toolchain';
import {
  BUILD_TIMEOUT_MS,
  clickButton,
  TOOLCHAIN_TIMEOUT_MS,
  UI_TIMEOUT_MS,
  waitForTestId,
  waitForToolchain,
} from './lib/ui';

/** The page's heading while nothing can build, and once a compiler is found. */
const SETUP_TITLE = 'Set up a C++ compiler';
const LIST_TITLE = 'C++ compiler';

describe('Toolchain setup page', () => {
  it('shows the platform install command without g++, and Rescan finds g++ once it is back', async (context) => {
    const flow = startFlow(context);
    const hidden = hideToolchains(flow.folders.root, harnessSettings().toolchainDirs);
    const app = await flow.launch({ env: { B2C_E2E_TOOLCHAIN_DIRS: hidden.value } });
    const { driver } = app;

    // (1) Discovery finds nothing, so the setup page opens by itself.
    await waitForTestId(driver, 'toolchain-page', true, TOOLCHAIN_TIMEOUT_MS);
    await waitFor(
      async () => (await testIdText(driver, 'toolchain-page')).startsWith(SETUP_TITLE),
      {
        timeout: TOOLCHAIN_TIMEOUT_MS,
        message: 'the setup page heading',
      },
    );
    expect(await testIdText(driver, 'toolchain-status')).toContain('No usable g++ was found');
    expect(await testIdText(driver, 'setup-what')).toContain('What is a compiler?');

    // This platform's steps, with the command for this Linux distribution (04 §4.6).
    const section = process.platform === 'win32' ? 'setup-windows' : 'setup-linux';
    const steps = await testIdText(driver, section);
    const expected = expectedInstallCommands(
      process.platform,
      process.platform === 'win32' ? null : parseOsRelease(readOsRelease() ?? ''),
    );
    for (const command of expected) {
      expect(steps).toContain(command);
    }
    if (process.platform !== 'win32' && expected.length === 1) {
      // One distribution, one command: the others are not offered.
      for (const command of [INSTALL_COMMANDS.apt, INSTALL_COMMANDS.dnf, INSTALL_COMMANDS.pacman]) {
        if (!expected.includes(command)) {
          expect(steps).not.toContain(command);
        }
      }
    }
    expect(steps).toContain('I installed it → Rescan');

    // (2) g++ is back where the app looks: Rescan finds it, and the status bar names it.
    hidden.restore();
    await clickTestId(driver, 'toolchain-rescan');
    await waitForToolchain(app, TOOLCHAIN_TIMEOUT_MS);
    await waitFor(
      async () =>
        (await testIdText(driver, 'toolchain-status')).includes('builds your projects with'),
      { timeout: UI_TIMEOUT_MS, message: 'the page to name the compiler it builds with' },
    );
    const page = await testIdText(driver, 'toolchain-page');
    expect(page.startsWith(LIST_TITLE)).toBe(true);
    expect(page).not.toContain(SETUP_TITLE);

    // (3) And it builds: a new project runs.
    await clickButton(driver, '.feature-page-header', 'Back to the start page');
    const mainId = await newEmptyProject(app);
    await app.hook.insertBlocks(mainId, 'BODY', [print(new NodeIds('setup'), 'B2C-FOUND-GXX')]);
    await clickTestId(driver, 'toolbar-run');
    await waitForConsoleState(driver, /Finished \(exit code 0\)$/, BUILD_TIMEOUT_MS);
    expect(await app.hook.consoleText()).toContain('B2C-FOUND-GXX');
  }, 180_000);
});
