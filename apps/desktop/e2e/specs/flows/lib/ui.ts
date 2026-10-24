/**
 * The window's chrome in the flow tests: the app's own dialogs, the main menu, buttons by their
 * visible text, the toolchain in the status bar, the Settings page and the run gate. Elements are
 * found by their stable `data-testid` where they have one, otherwise by role and visible text, as
 * a person finds them.
 */
import { By, Key, type WebDriver, type WebElement } from 'selenium-webdriver';

import type { App } from '../../../support/app';
import { byTestId, clickTestId, testIdText, textContent } from '../../../support/ui';
import { waitFor } from '../../../support/wait';

/** How long background toolchain discovery may take on a fresh profile. */
export const TOOLCHAIN_TIMEOUT_MS = 30_000;

/** How long a build may take (the compile timeout is two minutes). */
export const BUILD_TIMEOUT_MS = 120_000;

/** How long the app may take to answer an action in the window. */
export const UI_TIMEOUT_MS = 10_000;

/** The visible buttons inside `scope` (CSS) whose text is `label`, ignoring extra white space. */
async function buttonsByText(
  driver: WebDriver,
  scope: string,
  label: string | RegExp,
): Promise<WebElement[]> {
  const found: WebElement[] = [];
  for (const button of await driver.findElements(By.css(`${scope} button`))) {
    const text = await textContent(driver, button);
    const matches = typeof label === 'string' ? text === label : label.test(text);
    if (matches && (await button.isDisplayed())) {
      found.push(button);
    }
  }
  return found;
}

/** Waits for a visible button inside `scope` with the text `label`, and returns it. */
export function buttonByText(
  driver: WebDriver,
  scope: string,
  label: string | RegExp,
  timeout = UI_TIMEOUT_MS,
): Promise<WebElement> {
  return waitFor(async () => (await buttonsByText(driver, scope, label))[0] ?? null, {
    timeout,
    message: `a button "${String(label)}" in ${scope}`,
  });
}

/** Clicks the visible button inside `scope` with the text `label`. */
export async function clickButton(
  driver: WebDriver,
  scope: string,
  label: string | RegExp,
  timeout = UI_TIMEOUT_MS,
): Promise<void> {
  await (await buttonByText(driver, scope, label, timeout)).click();
}

/** The selector of the app's open dialog (app/dialogs/DialogHost.tsx). */
export const DIALOG = '[data-testid="app-dialog"]';

/** Waits for the app's dialog and returns its title (and message) text. */
export async function waitForDialog(
  driver: WebDriver,
  expected: RegExp,
  timeout = UI_TIMEOUT_MS,
): Promise<string> {
  return waitFor(
    async () => {
      const dialogs = await driver.findElements(By.css(DIALOG));
      const dialog = dialogs[0];
      if (dialog === undefined) {
        return null;
      }
      const text = await textContent(driver, dialog);
      return expected.test(text) ? text : null;
    },
    {
      timeout,
      message: async () =>
        `a dialog matching ${String(expected)} (shown: "${await dialogText(driver)}")`,
    },
  );
}

/** The open dialog's text ('' when none is open). */
export async function dialogText(driver: WebDriver): Promise<string> {
  const dialog = (await driver.findElements(By.css(DIALOG)))[0];
  return dialog === undefined ? '' : textContent(driver, dialog);
}

/** Answers the open dialog with the button `label`, and waits for it to close. */
export async function answerDialog(driver: WebDriver, label: string): Promise<void> {
  await clickButton(driver, DIALOG, label);
  await waitFor(async () => (await driver.findElements(By.css(DIALOG))).length === 0, {
    timeout: UI_TIMEOUT_MS,
    message: `the dialog to close after "${label}"`,
  });
}

/** Chooses `label` in the toolbar's main menu `≡` (app/layout/MainMenu.tsx). */
export async function chooseMenuItem(driver: WebDriver, label: string): Promise<void> {
  await clickTestId(driver, 'main-menu');
  const item = await waitFor(
    async () => {
      for (const candidate of await driver.findElements(By.css('[role="menuitem"]'))) {
        const text = await textContent(driver, candidate);
        if (text === label || text.startsWith(`${label} `)) {
          return candidate;
        }
      }
      return null;
    },
    { timeout: UI_TIMEOUT_MS, message: `the main menu item "${label}"` },
  );
  await item.click();
}

/** Waits until the status bar names a g++ (discovery found the toolchain), and returns it. */
export function waitForToolchain(app: App, timeout = TOOLCHAIN_TIMEOUT_MS): Promise<string> {
  return waitFor(
    async () => {
      const text = await testIdText(app.driver, 'status-toolchain');
      return /\bg\+\+/.test(text) && !/No g\+\+|Looking for/.test(text) ? text : null;
    },
    {
      timeout,
      message: async () =>
        `the status bar to show g++ (it shows "${await testIdText(app.driver, 'status-toolchain')}")`,
    },
  );
}

/** Whether a button with this test ID is held back (`aria-disabled`, as the run gate decides). */
export async function heldBack(driver: WebDriver, id: string): Promise<boolean> {
  const button = await driver.findElement(byTestId(id));
  return (await button.getAttribute('aria-disabled')) === 'true';
}

/**
 * Runs in the webview: the text of the element that describes the element with test ID
 * `arguments[0]` (`aria-describedby`), or ''.
 */
const DESCRIPTION = `
  const element = document.querySelector('[data-testid="' + arguments[0] + '"]');
  const id = element === null ? null : element.getAttribute('aria-describedby');
  const description = id === null ? null : document.getElementById(id);
  return description === null ? '' : description.textContent;
`;

/**
 * The accessible description of the element with this test ID, such as why a toolbar button is
 * held back (the run gate's hint).
 */
export async function accessibleDescription(driver: WebDriver, id: string): Promise<string> {
  // The ID goes into a selector: byTestId refuses anything that is not a test ID.
  byTestId(id);
  const text: unknown = await driver.executeScript(DESCRIPTION, id);
  return typeof text === 'string' ? text.trim() : '';
}

/** Waits until the element with this test ID exists (`present`) or does not. */
export async function waitForTestId(
  driver: WebDriver,
  id: string,
  present: boolean,
  timeout = UI_TIMEOUT_MS,
): Promise<void> {
  await waitFor(async () => (await driver.findElements(byTestId(id))).length > 0 === present, {
    timeout,
    message: `[data-testid="${id}"] to be ${present ? 'shown' : 'gone'}`,
  });
}

/** Opens the Settings page from the toolbar. */
export async function openSettings(driver: WebDriver): Promise<void> {
  await clickTestId(driver, 'toolbar-settings');
  await waitForTestId(driver, 'settings-page', true);
}

/** Leaves a full-window page (Settings, the toolchain page) for the editor. */
export async function backToEditor(driver: WebDriver): Promise<void> {
  await clickButton(driver, '.feature-page-header', 'Back to the editor');
  await waitForTestId(driver, 'workspace', true);
}

/** Presses `key` with Ctrl held (the window's and the editor's shortcuts). */
export async function pressCtrl(driver: WebDriver, key: string): Promise<void> {
  await driver
    .actions({ async: true })
    .keyDown(Key.CONTROL)
    .sendKeys(key)
    .keyUp(Key.CONTROL)
    .perform();
}

/** Presses `key` with Shift held (`Shift+F5` is Stop). */
export async function pressShift(driver: WebDriver, key: string): Promise<void> {
  await driver.actions({ async: true }).keyDown(Key.SHIFT).sendKeys(key).keyUp(Key.SHIFT).perform();
}
