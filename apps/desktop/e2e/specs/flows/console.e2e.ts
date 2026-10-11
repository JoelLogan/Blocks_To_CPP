/**
 * The console as the person sees it (docs/spec/04-user-interface.md §4.5), read from the
 * terminal's buffer through the test hook's `consoleScreen()` rather than from the bytes it was
 * given:
 *
 * - a second run writes below the first, after a *New run* line, and never over it;
 * - at three heights of the bottom dock the terminal is fitted to whole rows inside its padding,
 *   and neither it nor the dock panel around it scrolls. Each height is chosen so that the host's
 *   height is two pixels more than a whole number of rows, which a fit that ignores the padding
 *   gets wrong.
 */
import { By, type WebDriver } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { NodeIds } from '../../support/bdm';
import { showConsole, waitForConsoleState } from '../../support/console';
import { drag } from '../../support/drag';
import { clickTestId, middleOf } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { waitForTranscript } from './lib/console';
import { type FlowApp, startFlow } from './lib/launch';
import { changeBy, declare, newEmptyProject, printValue, ref, repeat } from './lib/project';
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

/** Drags the bottom dock's splitter so that the dock grows by `delta` pixels (or shrinks). */
async function resizeBottomDock(driver: WebDriver, delta: number): Promise<void> {
  const splitter = await driver.findElement(
    By.css('[role="separator"][aria-label="Resize the bottom panel"]'),
  );
  const from = await middleOf(driver, splitter);
  await drag(driver, from, { x: from.x, y: from.y - delta });
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
      // Windows' pseudoconsole paints each new program's screen from its own top-left corner
      // (with cursor moves), so there only the end of the second run on screen and the order of
      // what the console was given are checked.
      expect(shown.at(-1)).toBe(String(LINES));
      const transcript = await app.hook.consoleText();
      const separator = transcript.indexOf(SEPARATOR);
      expect(separator).toBeGreaterThan(transcript.indexOf(`\n${String(LINES)}`));
      expect(finishedRuns(transcript.slice(separator))).toBe(1);
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
});
