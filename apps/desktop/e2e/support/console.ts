/**
 * The console in the tests (docs/spec/04-user-interface.md §4.5): the header's state, what the
 * terminal shows, and typing into it. xterm.js uses its DOM renderer, so the rows on screen are
 * `.xterm-rows`; input goes through its `.xterm-helper-textarea`, which has the focus once the
 * terminal is clicked. The whole run (beyond the rows on screen) comes from the hook's transcript.
 */
import { By, type WebDriver } from 'selenium-webdriver';

import { byTestId, clickTestId, testIdText } from './ui';
import { waitFor } from './wait';

/** The header's state line: `Running`, `Finished (exit code 0)`, `Stopped`, … */
export function consoleState(driver: WebDriver): Promise<string> {
  return testIdText(driver, 'console-state');
}

/** Waits until the header's state line matches `expected`, and returns it. */
export function waitForConsoleState(
  driver: WebDriver,
  expected: RegExp,
  timeout: number,
): Promise<string> {
  return waitFor(
    async () => {
      const state = await consoleState(driver);
      return expected.test(state) ? state : null;
    },
    {
      timeout,
      message: async () =>
        `the console state ${String(expected)} (it shows "${await consoleState(driver)}")`,
    },
  );
}

/** The terminal's rows on screen, as text (non-breaking spaces as spaces, trailing blanks cut). */
export async function visibleConsoleText(driver: WebDriver): Promise<string> {
  const text: unknown = await driver.executeScript(`
    const rows = document.querySelector('[data-testid="console-terminal"] .xterm-rows');
    if (rows === null) {
      return '';
    }
    return Array.from(rows.children, (row) => row.textContent || '').join('\\n');
  `);
  if (typeof text !== 'string') {
    return '';
  }
  return text
    .replace(/\u00a0/g, ' ')
    .split('\n')
    .map((line) => line.trimEnd())
    .join('\n');
}

/**
 * Turns the mouse wheel over the console's terminal by `lines` lines (negative: up), in one event
 * measured in lines (`WheelEvent.DOM_DELTA_LINE`). The event is dispatched on the terminal's
 * screen, where the pointer's would land, and reaches the console's own wheel handler
 * (src/panels/console/wheel.ts), which scrolls the scrollback by whole lines.
 */
export async function scrollConsoleLines(driver: WebDriver, lines: number): Promise<void> {
  await driver.executeScript(
    `const screen = document.querySelector('[data-testid="console-terminal"] .xterm-screen');
     const box = screen.getBoundingClientRect();
     screen.dispatchEvent(new WheelEvent('wheel', {
       deltaY: arguments[0],
       deltaMode: WheelEvent.DOM_DELTA_LINE,
       bubbles: true,
       cancelable: true,
       clientX: box.left + box.width / 2,
       clientY: box.top + box.height / 2,
     }));`,
    lines,
  );
}

/**
 * What screen readers find in the terminal: the text of its accessibility tree (the rows on
 * screen, non-breaking spaces as spaces), or `null` while it has none (the console pauses it
 * during an output flood).
 */
export async function accessibleConsoleText(driver: WebDriver): Promise<string | null> {
  const text: unknown = await driver.executeScript(`
    const tree = document.querySelector(
      '[data-testid="console-terminal"] .xterm-accessibility-tree',
    );
    return tree === null ? null : tree.textContent;
  `);
  return typeof text === 'string' ? text.replace(/\u00a0/g, ' ') : null;
}

/** Shows the Console tab of the bottom dock. */
export async function showConsole(driver: WebDriver): Promise<void> {
  await clickTestId(driver, 'dock-tab-console');
  await waitFor(
    async () =>
      (await driver.findElements(By.css('[data-testid="console-terminal"] .xterm'))).length > 0,
    { timeout: 10_000, message: 'the terminal in the Console tab' },
  );
}

/** Types `text` into the console as the person would (click the terminal, then the keys). */
export async function typeIntoConsole(driver: WebDriver, text: string): Promise<void> {
  const terminal = await driver.findElement(byTestId('console-terminal'));
  await terminal.click();
  await waitFor(
    async () => {
      const focused: unknown = await driver.executeScript(
        "return document.activeElement !== null && document.activeElement.classList.contains('xterm-helper-textarea');",
      );
      return focused === true;
    },
    { timeout: 5_000, message: 'the terminal to have the keyboard focus' },
  );
  await driver.actions({ async: true }).sendKeys(text).perform();
}
