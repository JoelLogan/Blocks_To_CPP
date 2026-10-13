/**
 * The console as the person sees it (docs/spec/04-user-interface.md §4.5), read from the
 * terminal's buffer through the test hook's `consoleScreen()` rather than from the bytes it was
 * given:
 *
 * - a second run writes below the first, after a *New run* line, and never over it;
 * - at three heights of the bottom dock the terminal is fitted to whole rows inside its padding,
 *   and neither it nor the dock panel around it scrolls. Each height is chosen so that the host's
 *   height is two pixels more than a whole number of rows, which a fit that ignores the padding
 *   gets wrong;
 * - with a notice open in the smallest bottom dock, the header scrolls by itself to Stop, Run
 *   again and Clear;
 * - a new run brings its output into view after the person scrolled up, and Clear empties the
 *   console once the program has ended; screen readers find the output in the terminal's
 *   accessibility tree;
 * - the wheel scrolls the console back while a program prints, and the view stays there.
 */
import { By, Key, type WebDriver } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import type { E2eConsoleScreen } from '../../../src/e2e/contract';
import { NodeIds, print } from '../../support/bdm';
import { accessibleConsoleText, showConsole, waitForConsoleState } from '../../support/console';
import { drag } from '../../support/drag';
import { clickTestId, middleOf } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { waitForConsoleText, waitForTranscript } from './lib/console';
import { type FlowApp, startFlow } from './lib/launch';
import {
  changeBy,
  declare,
  forever,
  newEmptyProject,
  printValue,
  ref,
  repeat,
} from './lib/project';
import { BUILD_TIMEOUT_MS, UI_TIMEOUT_MS, waitForToolchain } from './lib/ui';

/** How many lines the program prints: more than the console shows, so it scrolls. */
const LINES = 30;

/** The console's line between two runs. */
const SEPARATOR = '── New run ──';

/** The lines one run of the program prints. */
const RUN_OUTPUT = Array.from({ length: LINES }, (_, i) => String(i + 1));

/** Starts the app with a new project whose program prints 1 to {@link LINES}, one per line. */
async function counterProject(context: Parameters<typeof startFlow>[0]): Promise<FlowApp> {
  const app = await startFlow(context).launch();
  await waitForToolchain(app);
  const mainId = await newEmptyProject(app);
  const ids = new NodeIds('console');
  await app.hook.insertBlocks(mainId, 'BODY', [
    declare(ids, { sym: 'sym_e2e_n', name: 'n', type: 'int', value: { expr: [{ num: '0' }] } }),
    repeat(ids, { expr: [{ num: String(LINES) }] }, [
      changeBy(ids, 'sym_e2e_n', { expr: [{ num: '1' }] }),
      printValue(ids, ref('sym_e2e_n')),
    ]),
  ]);
  return app;
}

/** How many runs of the program the transcript has (its last line, alone on a line). */
function finishedRuns(transcript: string): number {
  return transcript.split('\n').filter((line) => line.trim() === String(LINES)).length;
}

/** Waits until run number `run` has printed everything and the header says it finished. */
async function waitForRun(app: FlowApp, run: number): Promise<void> {
  await waitForTranscript(
    app,
    (text) => finishedRuns(text) >= run,
    BUILD_TIMEOUT_MS,
    `the end of run ${String(run)}`,
  );
  await waitForConsoleState(app.driver, /Finished \(exit code 0\)$/, UI_TIMEOUT_MS);
}

/** Where the terminal is and how it is fitted, read in the webview. */
interface Geometry {
  /** The terminal's host (`console-terminal`): its bottom and its height. */
  readonly hostBottom: number;
  readonly hostHeight: number;
  /** The bottom of xterm's screen (the rows). */
  readonly screenBottom: number;
  /** The rows shown and the height of one. */
  readonly rows: number;
  readonly cellHeight: number;
  /** The console's dock panel: how far its content reaches, and how much of it shows. */
  readonly panelScrollHeight: number;
  readonly panelClientHeight: number;
}

const GEOMETRY = `
  const host = document.querySelector('[data-testid="console-terminal"]');
  const screen = host && host.querySelector('.xterm-screen');
  const rows = host && host.querySelector('.xterm-rows');
  const panel = host && host.closest('[data-dock-panel]');
  if (!host || !screen || !rows || !panel) {
    return null;
  }
  const h = host.getBoundingClientRect();
  const s = screen.getBoundingClientRect();
  return {
    hostBottom: h.bottom,
    hostHeight: h.height,
    screenBottom: s.bottom,
    rows: rows.children.length,
    cellHeight: rows.children.length > 0 ? s.height / rows.children.length : 0,
    panelScrollHeight: panel.scrollHeight,
    panelClientHeight: panel.clientHeight,
  };
`;

async function geometry(driver: WebDriver): Promise<Geometry> {
  const found: Geometry | null = await driver.executeScript(GEOMETRY);
  if (found === null || found.rows === 0) {
    throw new Error('The console shows no terminal');
  }
  return found;
}

/** Waits until the host is `height` pixels tall and the terminal has been fitted to it. */
async function waitForFit(driver: WebDriver, height: number): Promise<Geometry> {
  let last: Geometry | null = null;
  return waitFor(
    async () => {
      const now = await geometry(driver);
      const settled =
        last !== null &&
        Math.abs(now.hostHeight - height) < 0.5 &&
        now.rows === last.rows &&
        now.hostHeight === last.hostHeight;
      last = now;
      return settled ? now : null;
    },
    {
      timeout: UI_TIMEOUT_MS,
      interval: 200,
      message: () => `the console host to be ${String(height)} px tall (${JSON.stringify(last)})`,
    },
  );
}

/** The console header and its buttons, read in the webview. */
interface Header {
  /** The header's own scrolling: how far its content reaches, and how much of it shows. */
  readonly scrollHeight: number;
  readonly clientHeight: number;
  /** The console's dock panel: the same. */
  readonly panelScrollHeight: number;
  readonly panelClientHeight: number;
  /** For each button, what is under the middle of it (its test ID, or `null`). */
  readonly hits: Readonly<Record<string, string | null>>;
}

const BUTTONS = ['console-stop', 'console-run-again', 'console-clear'];

const HEADER = `
  const header = document.querySelector('[data-testid="console-header"]');
  const panel = header && header.closest('[data-dock-panel]');
  if (!header || !panel) {
    return null;
  }
  const hits = {};
  for (const id of arguments[0]) {
    const box = document.querySelector('[data-testid="' + id + '"]').getBoundingClientRect();
    const found = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
    const owner = found && found.closest('[data-testid]');
    hits[id] = owner ? owner.getAttribute('data-testid') : null;
  }
  return {
    scrollHeight: header.scrollHeight,
    clientHeight: header.clientHeight,
    panelScrollHeight: panel.scrollHeight,
    panelClientHeight: panel.clientHeight,
    hits,
  };
`;

async function header(driver: WebDriver): Promise<Header> {
  const found: Header | null = await driver.executeScript(HEADER, BUTTONS);
  if (found === null) {
    throw new Error('The console shows no header');
  }
  return found;
}

/** Drags the bottom dock's splitter so that the dock grows by `delta` pixels (or shrinks). */
async function resizeBottomDock(driver: WebDriver, delta: number): Promise<void> {
  const splitter = await driver.findElement(
    By.css('[role="separator"][aria-label="Resize the bottom panel"]'),
  );
  const from = await middleOf(driver, splitter);
  await drag(driver, from, { x: from.x, y: from.y - delta });
}

/** What the console's terminal holds now. */
function consoleScreen(app: FlowApp): Promise<E2eConsoleScreen> {
  return app.hook.call('consoleScreen');
}

/** Waits until the console's view is scrolled up from the end of the output; returns the screen. */
function waitForScrolledUp(app: FlowApp): Promise<E2eConsoleScreen> {
  return waitFor(
    async () => {
      const screen = await consoleScreen(app);
      return screen.viewportY < screen.baseY ? screen : null;
    },
    { timeout: UI_TIMEOUT_MS, message: 'the console to be scrolled up from the end' },
  );
}

/**
 * Turns the mouse wheel over the console's terminal `notches` times, by `deltaY` pixels each
 * (negative: up). The wheel events are dispatched on the terminal's screen, where the pointer's
 * would land, and reach the console's own wheel handler like the pointer's (this version of
 * selenium-webdriver's types has no wheel actions).
 */
async function turnWheel(driver: WebDriver, deltaY: number, notches: number): Promise<void> {
  await driver.executeScript(
    `const screen = document.querySelector('[data-testid="console-terminal"] .xterm-screen');
     const box = screen.getBoundingClientRect();
     for (let notch = 0; notch < arguments[1]; notch++) {
       screen.dispatchEvent(new WheelEvent('wheel', {
         deltaY: arguments[0],
         deltaMode: 0,
         bubbles: true,
         cancelable: true,
         clientX: box.left + box.width / 2,
         clientY: box.top + box.height / 2,
       }));
     }`,
    deltaY,
    notches,
  );
}

describe('Console', () => {
  it('shows a second run below the first, after a New run line', async (context) => {
    const app = await counterProject(context);

    await clickTestId(app.driver, 'toolbar-run');
    await waitForRun(app, 1);
    await clickTestId(app.driver, 'console-run-again');
    await waitForRun(app, 2);

    const screen = await app.hook.call('consoleScreen');
    expect(screen.buffer).toBe('normal');
    const shown = screen.lines.map((line) => line.trimEnd()).filter((line) => line !== '');
    if (process.platform === 'win32') {
      // Windows' pseudoconsole clears the screen and paints each new program from the top-left
      // corner (with cursor moves), so the console first scrolls what it shows into the
      // scrollback. How the pseudoconsole scrolls a program's own lines is its affair, so there
      // the end of the first run, the New run line after it and the end of the second run are
      // checked on screen, and the order of what the console was given.
      const separator = shown.indexOf(SEPARATOR);
      expect(separator, 'the New run line is gone').toBeGreaterThan(0);
      expect(shown[separator - 1], 'the end of the first run is gone').toBe(String(LINES));
      expect(shown.at(-1)).toBe(String(LINES));
      const transcript = await app.hook.consoleText();
      const given = transcript.indexOf(SEPARATOR);
      expect(given).toBeGreaterThan(transcript.indexOf(`\n${String(LINES)}`));
      expect(finishedRuns(transcript.slice(given))).toBe(1);
    } else {
      // Everything the first run printed is still there, the scrollback included.
      expect(shown).toEqual([...RUN_OUTPUT, SEPARATOR, ...RUN_OUTPUT]);
    }
  }, 240_000);

  it('fits the terminal inside its box at three dock heights, and nothing around it scrolls', async (context) => {
    const app = await counterProject(context);
    await clickTestId(app.driver, 'toolbar-run');
    await waitForRun(app, 1);
    await showConsole(app.driver);

    for (const rows of [3, 7, 15]) {
      const before = await geometry(app.driver);
      // Two pixels more than whole rows: a fit that ignored the 4 px padding would take one row
      // too many, and the screen would stick out of the host by two pixels.
      const target = Math.ceil(rows * before.cellHeight) + 2;
      await resizeBottomDock(app.driver, target - before.hostHeight);
      const fitted = await waitForFit(app.driver, target);

      const where = `with the console host ${String(fitted.hostHeight)} px tall`;
      expect(fitted.panelScrollHeight, `the dock panel scrolls ${where}`).toBe(
        fitted.panelClientHeight,
      );
      expect(fitted.screenBottom, `the rows stick out of the host ${where}`).toBeLessThanOrEqual(
        fitted.hostBottom + 0.5,
      );
      // Whole rows fill the host: less than one row's height is left below the last.
      expect(fitted.hostBottom - fitted.screenBottom, where).toBeLessThan(fitted.cellHeight);
    }
  }, 240_000);

  it('scrolls a header taller than the smallest dock by itself, to its buttons', async (context) => {
    const app = await counterProject(context);
    await clickTestId(app.driver, 'toolbar-run');
    await waitForRun(app, 1);
    await showConsole(app.driver);

    // Every run from the app has IDE helpers, so the header has at least that notice. Open it,
    // then make the bottom dock as small as it goes.
    await app.driver.findElement(By.css('[data-testid="console-notice"] summary')).click();
    const splitter = await app.driver.findElement(
      By.css('[role="separator"][aria-label="Resize the bottom panel"]'),
    );
    await splitter.sendKeys(Key.HOME);
    await waitFor(
      async () =>
        (await splitter.getAttribute('aria-valuenow')) ===
        (await splitter.getAttribute('aria-valuemin')),
      { timeout: UI_TIMEOUT_MS, message: 'the bottom dock at its smallest' },
    );
    const before = await waitFor(
      async () => {
        const now = await header(app.driver);
        return now.scrollHeight > now.clientHeight ? now : null;
      },
      {
        timeout: UI_TIMEOUT_MS,
        message: 'the header to be taller than the dock (so this test checks something)',
      },
    );
    expect(before.panelScrollHeight, 'the dock panel scrolls').toBe(before.panelClientHeight);

    // The header scrolls by itself (as the wheel or its scroll bar would), and then every button
    // is there to be clicked; the dock panel around it still does not scroll.
    await app.driver.executeScript(
      `const header = document.querySelector('[data-testid="console-header"]');
       header.scrollTop = header.scrollHeight;`,
    );
    const after = await header(app.driver);
    expect(after.hits).toEqual(Object.fromEntries(BUTTONS.map((id) => [id, id])));
    expect(after.panelScrollHeight, 'the dock panel scrolls').toBe(after.panelClientHeight);
  }, 240_000);

  it('brings a new run into view after scrolling up, and Clear empties it once the program has ended', async (context) => {
    const app = await counterProject(context);
    await clickTestId(app.driver, 'toolbar-run');
    await waitForRun(app, 1);
    await showConsole(app.driver);

    // Screen readers find the output among the rows on screen (updated at most once a second).
    await waitFor(async () => (await accessibleConsoleText(app.driver))?.includes(String(LINES)), {
      timeout: UI_TIMEOUT_MS,
      message: async () =>
        `the last line in the accessibility tree (it holds ${String(await accessibleConsoleText(app.driver))})`,
    });

    // Three notches up to read the first lines, then Run again.
    await turnWheel(app.driver, -53, 3);
    await waitForScrolledUp(app);
    await clickTestId(app.driver, 'console-run-again');
    await waitForRun(app, 2);
    const second = await consoleScreen(app);
    expect(second.viewportY, 'the new run is out of view').toBe(second.baseY);

    // Nothing runs any more: Clear leaves nothing, not even the line with the cursor.
    await clickTestId(app.driver, 'console-clear');
    const cleared = await waitFor(
      async () => {
        const screen = await consoleScreen(app);
        return screen.lines.every((line) => line.trim() === '') ? screen : null;
      },
      {
        timeout: UI_TIMEOUT_MS,
        message: async () =>
          `an empty console (it shows ${JSON.stringify((await consoleScreen(app)).lines.filter((line) => line.trim() !== ''))})`,
      },
    );
    expect(cleared.baseY).toBe(0);
    expect(cleared.cursor).toEqual({ row: 0, column: 0 });
  }, 240_000);

  it('scrolls back with the wheel while a program prints, and stays there', async (context) => {
    const app = await startFlow(context).launch();
    await waitForToolchain(app);
    const mainId = await newEmptyProject(app);
    const ids = new NodeIds('wheel');
    await app.hook.insertBlocks(mainId, 'BODY', [forever(ids, [print(ids, 'B2C wheel line')])]);
    await clickTestId(app.driver, 'toolbar-run');
    await waitForConsoleText(app.driver, ['B2C wheel line'], BUILD_TIMEOUT_MS);
    await waitForConsoleState(app.driver, /Running/, UI_TIMEOUT_MS);

    await turnWheel(app.driver, -53, 3);
    await waitForScrolledUp(app);
    // The program goes on printing; the view stays scrolled up (it keeps its text while the
    // scrollback is not full, and moves up with it once it is).
    for (let check = 0; check < 5; check++) {
      await new Promise((resolve) => setTimeout(resolve, 200));
      const screen = await consoleScreen(app);
      expect(screen.viewportY, 'the view went back to the end').toBeLessThan(screen.baseY);
    }

    // A turn of the wheel down past the end (more than the whole scrollback) follows the output
    // again.
    await turnWheel(app.driver, 10_000_000, 1);
    await waitFor(
      async () => {
        const screen = await consoleScreen(app);
        return screen.viewportY === screen.baseY;
      },
      { timeout: UI_TIMEOUT_MS, message: 'the view back at the end of the output' },
    );
    await clickTestId(app.driver, 'console-stop');
    await waitForConsoleState(app.driver, /^■?\s*Stopped$/, UI_TIMEOUT_MS);
  }, 240_000);
});
