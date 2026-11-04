/**
 * The project's name on the project settings page (docs/plans/m3-contracts/w3-modules.md §1.5,
 * §5.2 E): ≡ *Project settings…* shows the name in its *Project* section; a new name saved there is
 * an unsaved change that the top bar (with `•`) and the C++ header comment show. An empty name
 * shows its problem under the field, and *Save changes* then saves nothing and stays on the page.
 */
import { Key, type WebDriver } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { clickTestId, testId, testIdText } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { startFlow } from './lib/launch';
import { newEmptyProject } from './lib/project';
import { chooseMenuItem, UI_TIMEOUT_MS, waitForTestId } from './lib/ui';

/** The name the Empty template gives a project. */
const TEMPLATE_NAME = 'My Project';

/** The name given on the page. */
const NEW_NAME = 'Number Quest';

/** Opens the project settings page from ≡. */
async function openProjectSettings(driver: WebDriver): Promise<void> {
  await chooseMenuItem(driver, 'Project settings…');
  await waitForTestId(driver, 'project-settings', true);
}

/**
 * Replaces all the text of the page's Name field with `text`, as a person types: the field is
 * clicked into, then the keys go to the focused element.
 */
async function typeName(driver: WebDriver, text: string): Promise<void> {
  await clickTestId(driver, 'project-settings-name');
  await driver
    .actions({ async: true })
    .keyDown(Key.CONTROL)
    .sendKeys('a')
    .keyUp(Key.CONTROL)
    .sendKeys(Key.BACK_SPACE)
    .perform();
  if (text !== '') {
    await driver.actions({ async: true }).sendKeys(text).perform();
  }
  await waitFor(
    async () =>
      (await (await testId(driver, 'project-settings-name')).getAttribute('value')) === text,
    { timeout: UI_TIMEOUT_MS, message: `the Name field to hold "${text}"` },
  );
}

describe('The project name on the project settings page', () => {
  it('saves a new name as an unsaved change, and refuses an empty one', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const { driver } = app;
    await newEmptyProject(app);
    expect(await testIdText(driver, 'project-name-text')).toBe(TEMPLATE_NAME);
    expect(await testIdText(driver, 'project-name')).not.toContain('•');

    // (1) The page shows the name; a new one is saved with Save changes.
    await openProjectSettings(driver);
    const field = await testId(driver, 'project-settings-name');
    expect(await field.getAttribute('value')).toBe(TEMPLATE_NAME);
    await typeName(driver, NEW_NAME);
    await clickTestId(driver, 'project-settings-save');
    await waitForTestId(driver, 'project-settings', false);

    // The top bar names it, unsaved, and the C++ header comment follows.
    await waitFor(async () => (await testIdText(driver, 'project-name-text')) === NEW_NAME, {
      timeout: UI_TIMEOUT_MS,
      message: `the top bar to name the project "${NEW_NAME}"`,
    });
    await waitFor(async () => (await testIdText(driver, 'project-name')).includes('•'), {
      timeout: UI_TIMEOUT_MS,
      message: 'the unsaved-changes marker',
    });
    await waitFor(async () => (await app.hook.code()).includes(`from project "${NEW_NAME}"`), {
      timeout: UI_TIMEOUT_MS,
      message: 'the header comment to name the new project',
    });

    // (2) An empty name: its problem under the field, and nothing is saved.
    await openProjectSettings(driver);
    await typeName(driver, '');
    const problem = await waitFor(
      async () => {
        const id = await (
          await testId(driver, 'project-settings-name')
        ).getAttribute('aria-describedby');
        if (id === null || id === '') {
          return null;
        }
        const text: unknown = await driver.executeScript(
          'const element = document.getElementById(arguments[0]); return element === null ? null : element.textContent;',
          id,
        );
        return typeof text === 'string' ? text : null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'the Name field’s problem' },
    );
    expect(problem).toBe('Type a name for the project.');
    expect(await (await testId(driver, 'project-settings-name')).getAttribute('aria-invalid')).toBe(
      'true',
    );
    await clickTestId(driver, 'project-settings-save');
    // Still on the page, with the focus on the field; the project keeps its name.
    await waitForTestId(driver, 'project-settings', true);
    const focused: unknown = await driver.executeScript(
      'return document.activeElement === null ? null : document.activeElement.getAttribute("data-testid");',
    );
    expect(focused).toBe('project-settings-name');
    await clickTestId(driver, 'project-settings-cancel');
    await waitForTestId(driver, 'project-settings', false);
    expect(await testIdText(driver, 'project-name-text')).toBe(NEW_NAME);
  }, 180_000);
});
