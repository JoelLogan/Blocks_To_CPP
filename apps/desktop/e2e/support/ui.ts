/**
 * The app's chrome in the tests: elements by their stable `data-testid`, text that may be in a
 * hidden tab, and Blockly's own widgets (toolbox categories, dropdown menus, text editors).
 */
import { By, Key, Origin, type WebDriver, type WebElement } from 'selenium-webdriver';

import type { E2ePoint } from '../../src/e2e/contract';
import { sleep, waitFor } from './wait';

/** The locator of `[data-testid="id"]`. */
export function byTestId(id: string): By {
  if (!/^[a-z0-9-]+$/.test(id)) {
    throw new Error(`Not a test ID: ${id}`);
  }
  return By.css(`[data-testid="${id}"]`);
}

/** The element with this test ID, waiting up to `timeout` for it to exist. */
export function testId(driver: WebDriver, id: string, timeout = 10_000): Promise<WebElement> {
  return waitFor(
    async () => {
      const found = await driver.findElements(byTestId(id));
      return found[0] ?? null;
    },
    { timeout, message: `[data-testid="${id}"]` },
  );
}

/**
 * An element's text content, also when it is in a hidden tab (WebDriver's `getText` only returns
 * rendered text), with runs of white space collapsed.
 */
export async function textContent(driver: WebDriver, element: WebElement): Promise<string> {
  const text: unknown = await driver.executeScript('return arguments[0].textContent;', element);
  return typeof text === 'string' ? text.replace(/\s+/g, ' ').trim() : '';
}

/** The text of the element with this test ID ('' when it does not exist). */
export async function testIdText(driver: WebDriver, id: string): Promise<string> {
  const found = await driver.findElements(byTestId(id));
  return found[0] === undefined ? '' : textContent(driver, found[0]);
}

/** Clicks the element with this test ID, waiting for it first. */
export async function clickTestId(driver: WebDriver, id: string, timeout = 10_000): Promise<void> {
  const element = await testId(driver, id, timeout);
  await element.click();
}

/** Opens a toolbox category by its name; the flyout scrolls to its blocks. */
export async function openCategory(driver: WebDriver, name: string): Promise<void> {
  const category = await waitFor(
    async () => {
      const rows = await driver.findElements(By.css('.blocklyToolboxCategory'));
      for (const row of rows) {
        if ((await textContent(driver, row)).endsWith(name)) {
          return row;
        }
      }
      return null;
    },
    { timeout: 10_000, message: `the toolbox category "${name}"` },
  );
  await category.click();
  // The flyout scrolls with an animation.
  await sleep(600);
}

/** The middle of an element's box on screen (viewport coordinates, whole pixels). */
export async function middleOf(driver: WebDriver, element: WebElement): Promise<E2ePoint> {
  const box: unknown = await driver.executeScript(
    'const r = arguments[0].getBoundingClientRect(); return [r.left, r.top, r.width, r.height];',
    element,
  );
  if (!Array.isArray(box) || box.length !== 4 || !box.every((n) => typeof n === 'number')) {
    throw new Error('The element has no position on screen');
  }
  const [left, top, width, height] = box as [number, number, number, number];
  if (width <= 0 || height <= 0) {
    throw new Error('The element is not shown');
  }
  return { x: Math.round(left + width / 2), y: Math.round(top + height / 2) };
}

/**
 * Clicks the middle of an element with the pointer. Blockly's fields are SVG groups, which
 * WebDriver's element click does not count as interactable in every webview; a pointer click at
 * the element's middle is what a person does anyway.
 */
export async function clickMiddle(driver: WebDriver, element: WebElement): Promise<void> {
  const { x, y } = await middleOf(driver, element);
  await driver.actions({ async: true }).move({ x, y, origin: Origin.VIEWPORT }).click().perform();
}

/** Clicks a dropdown field and then its option `label` in Blockly's menu. */
export async function chooseOption(
  driver: WebDriver,
  field: WebElement,
  label: string,
): Promise<void> {
  await clickMiddle(driver, field);
  const option = await waitFor(
    async () => {
      const items = await driver.findElements(By.css('.blocklyDropDownDiv .blocklyMenuItem'));
      for (const item of items) {
        if ((await item.isDisplayed()) && (await textContent(driver, item)) === label) {
          return item;
        }
      }
      return null;
    },
    { timeout: 10_000, message: `the menu option "${label}"` },
  );
  await option.click();
}

/** Clicks a text field, replaces its text with `text` by typing, and confirms with Enter. */
export async function typeIntoField(
  driver: WebDriver,
  field: WebElement,
  text: string,
): Promise<void> {
  await clickMiddle(driver, field);
  await waitFor(
    async () => {
      const editors = await driver.findElements(By.css('.blocklyWidgetDiv .blocklyHtmlInput'));
      return editors.length > 0;
    },
    { timeout: 10_000, message: 'the field editor' },
  );
  // Blockly selects the field's text when the editor opens; select all again to be sure.
  await driver
    .actions({ async: true })
    .keyDown(Key.CONTROL)
    .sendKeys('a')
    .keyUp(Key.CONTROL)
    .sendKeys(text)
    .sendKeys(Key.ENTER)
    .perform();
  await waitFor(
    async () => {
      const editors = await driver.findElements(By.css('.blocklyWidgetDiv .blocklyHtmlInput'));
      return editors.length === 0;
    },
    { timeout: 10_000, message: 'the field editor to close' },
  );
}

/** Presses a key (with no element in particular: it goes to the focused one, or the window). */
export async function pressKey(driver: WebDriver, key: string): Promise<void> {
  await driver.actions({ async: true }).sendKeys(key).perform();
}

/** The number of errors the Problems panel's summary names (0 for "No problems"). */
export function errorCount(summary: string): number {
  const match = /([\d,]+) errors?\b/.exec(summary);
  return match?.[1] === undefined ? 0 : Number(match[1].replace(/,/g, ''));
}
