/**
 * Machine settings (docs/spec/05-project-format.md §5.9, 04 §4.12): changing the code style's
 * indent width to 2 on the Settings page updates the C++ panel at once, is kept in the machine's
 * settings.json (never in the project), and still applies after the app is restarted.
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';

import { By, type WebDriver } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { NodeIds, print } from '../../support/bdm';
import { waitFor } from '../../support/wait';
import { type FlowApp, startFlow } from './lib/launch';
import { newEmptyProject } from './lib/project';
import { backToEditor, openSettings, UI_TIMEOUT_MS } from './lib/ui';

/** The text the test's print shows. */
const TEXT = 'B2C-INDENT-CHECK';

/** The radio button of an indent width on the Settings page. */
function indentOption(driver: WebDriver, width: 2 | 4) {
  return driver.findElement(
    By.css(`[data-testid="settings-indent"] input[type="radio"][value="${String(width)}"]`),
  );
}

/** The C++ line of the test's print, as the code panel shows it. */
async function printLine(app: FlowApp): Promise<string> {
  return waitFor(
    async () => (await app.hook.code()).split('\n').find((line) => line.includes(TEXT)) ?? null,
    { timeout: UI_TIMEOUT_MS, message: 'the print in the C++ panel' },
  );
}

/** Waits until the print's line is indented by `width` spaces. */
async function waitForIndent(app: FlowApp, width: number): Promise<void> {
  const expected = `${' '.repeat(width)}std::cout`;
  await waitFor(async () => (await printLine(app)).startsWith(expected), {
    timeout: UI_TIMEOUT_MS,
    message: async () =>
      `the print to be indented by ${String(width)} spaces (it is "${await printLine(app)}")`,
  });
}

/** A new project with one print in main. */
async function projectWithPrint(app: FlowApp): Promise<void> {
  const mainId = await newEmptyProject(app);
  await app.hook.insertBlocks(mainId, 'BODY', [print(new NodeIds('indent'), TEXT)]);
}

describe('Settings', () => {
  it('applies indent width 2 to the C++ panel at once and keeps it after a restart', async (context) => {
    const flow = startFlow(context);
    const settingsFile = path.join(flow.folders.profile, 'config', 'settings.json');

    // (1) The default is 4 spaces.
    const first = await flow.launch();
    await projectWithPrint(first);
    await waitForIndent(first, 4);

    // (2) 2 spaces on the Settings page: saved in settings.json, and the C++ panel follows.
    await openSettings(first.driver);
    expect(await (await indentOption(first.driver, 4)).isSelected()).toBe(true);
    await (await indentOption(first.driver, 2)).click();
    await waitFor(
      () => {
        try {
          const saved = JSON.parse(readFileSync(settingsFile, 'utf8')) as {
            codeStyle?: { indentWidth?: unknown };
          };
          return saved.codeStyle?.indentWidth === 2;
        } catch {
          return false;
        }
      },
      { timeout: UI_TIMEOUT_MS, message: 'settings.json to have indent width 2' },
    );
    await backToEditor(first.driver);
    await waitForIndent(first, 2);
    // A machine setting: the project's own file format has no code style.
    expect(await first.hook.documentText()).not.toContain('indentWidth');

    // (3) After a restart, a new project's C++ has 2 spaces too, and the page shows the choice.
    await first.quit();
    const second = await flow.launch();
    await projectWithPrint(second);
    await waitForIndent(second, 2);
    await openSettings(second.driver);
    expect(await (await indentOption(second.driver, 2)).isSelected()).toBe(true);
  }, 180_000);
});
