/**
 * The app's chrome in the tests: elements by their stable `data-testid`, text that may be in a
 * hidden tab, and Blockly's own widgets (toolbox categories, dropdown menus, text editors).
 */
import {
  By,
  error as webdriverError,
  Key,
  Origin,
  type WebDriver,
  type WebElement,
} from 'selenium-webdriver';

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

/** How long to wait between two clicks on an element that was not ready for one. */
const CLICK_RETRY_MS = 200;

/**
 * Whether a click failed because the element was not ready for one yet: covered for a moment, not
 * laid out while the page is busy (a large project just opened), or replaced by a new render.
 */
export function isNotReadyForClick(error: unknown): boolean {
  return (
    error instanceof webdriverError.ElementNotInteractableError ||
    error instanceof webdriverError.ElementClickInterceptedError ||
    error instanceof webdriverError.StaleElementReferenceError
  );
}

/**
 * Clicks the element with this test ID, waiting for it first. A click the element was not ready
 * for ({@link isNotReadyForClick}) is tried again until `timeout`; any other error is thrown.
 */
export async function clickTestId(driver: WebDriver, id: string, timeout = 10_000): Promise<void> {
  const deadline = Date.now() + timeout;
  for (;;) {
    const element = await testId(driver, id, Math.max(1_000, deadline - Date.now()));
    try {
      await element.click();
      return;
    } catch (error: unknown) {
      if (!isNotReadyForClick(error) || Date.now() >= deadline) {
        throw error;
      }
    }
    await sleep(CLICK_RETRY_MS);
  }
}

/** The SVG group the toolbox's flyout scrolls: its `transform` changes while it scrolls. */
export const FLYOUT_CANVAS = '.blocklyToolboxFlyout .blocklyBlockCanvas';

/** How long the flyout's position must stay the same to count as settled (at least 3 frames). */
export const FLYOUT_QUIET_MS = 150;

/** How long the flyout may take to stop scrolling. */
export const FLYOUT_SETTLE_TIMEOUT_MS = 5_000;

/** What {@link SETTLE_SCRIPT} calls back with. */
export type SettleResult = 'settled' | 'moving' | 'missing';

/**
 * Runs in the webview (`executeAsyncScript`): calls back with `'settled'` once the element at
 * selector `arguments[0]` has kept its `transform` for `arguments[1]` milliseconds and at least
 * three animation frames, with `'moving'` when that has not happened after `arguments[2]`
 * milliseconds, and with `'missing'` at once when there is no such element. The continuous toolbox
 * scrolls its flyout a fraction of the way on every animation frame, so the time an animation
 * takes depends on the frame rate: counting frames and time, rather than sleeping, waits for its
 * end on a slow machine too. A plain script (not a function from this file), so nothing the test
 * runner adds to compiled functions can end up in the page.
 */
export const SETTLE_SCRIPT = `
  const selector = arguments[0];
  const quietMs = arguments[1];
  const timeoutMs = arguments[2];
  const done = arguments[arguments.length - 1];
  const read = () => {
    const element = document.querySelector(selector);
    return element === null ? null : element.getAttribute('transform');
  };
  const start = performance.now();
  let last = read();
  let since = start;
  let frames = 0;
  let finished = false;
  const finish = (result) => {
    if (!finished) {
      finished = true;
      done(result);
    }
  };
  if (document.querySelector(selector) === null) {
    finish('missing');
    return;
  }
  const step = (now) => {
    if (finished) {
      return;
    }
    const current = read();
    if (current !== last) {
      last = current;
      since = now;
      frames = 0;
    } else {
      frames += 1;
    }
    if (frames >= 3 && now - since >= quietMs) {
      finish('settled');
    } else if (now - start >= timeoutMs) {
      finish('moving');
    } else {
      requestAnimationFrame(step);
    }
  };
  requestAnimationFrame(step);
  // Without animation frames (a hidden window) nothing would ever call back.
  setTimeout(() => finish('moving'), timeoutMs + 1000);
`;

/**
 * Waits until the toolbox's flyout has stopped scrolling (see {@link SETTLE_SCRIPT}).
 *
 * @throws Error when there is no flyout, or it still moves after {@link FLYOUT_SETTLE_TIMEOUT_MS}.
 */
export async function waitForFlyout(driver: WebDriver): Promise<void> {
  const result: unknown = await driver.executeAsyncScript(
    SETTLE_SCRIPT,
    FLYOUT_CANVAS,
    FLYOUT_QUIET_MS,
    FLYOUT_SETTLE_TIMEOUT_MS,
  );
  if (result === 'missing') {
    throw new Error(`The toolbox's flyout (${FLYOUT_CANVAS}) is not on the page`);
  }
  if (result !== 'settled') {
    throw new Error(
      `The toolbox's flyout was still scrolling after ${String(FLYOUT_SETTLE_TIMEOUT_MS / 1000)} s`,
    );
  }
}

/**
 * Opens a toolbox category by its name and waits until the flyout has scrolled to its blocks, so
 * that the blocks are where the next WebDriver call finds them.
 */
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
  // The flyout scrolls with an animation that lasts a number of frames.
  await waitForFlyout(driver);
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
