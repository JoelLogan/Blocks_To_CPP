/**
 * Visual diff of the block canvas (docs/spec/09-quality-and-delivery.md §9.2, 10 §10.2 risk 2): the
 * Zelos renderer and the Blocks2Cpp theme as WebKitGTK and WebView2 draw them. The guessing game
 * (visual/fixture.ts) is opened in a 1000×700 window at zoom 1.0 in the light theme, with nothing
 * focused and the text caret hidden; a WebDriver screenshot of the workspace element is compared
 * with this system's baseline (visual/baselines/<system>/canvas-guessing-game.png) by pixelmatch,
 * and more than 0.5% of differing pixels fails. The screenshot is always written as a candidate
 * (visual/baselines.ts), so a missing or outdated baseline can be committed from the CI artifact.
 */
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { By, Origin, type WebDriver } from 'selenium-webdriver';
import { afterAll, describe, expect, it } from 'vitest';

import { type App, launchApp } from '../support/app';
import { foldCodePanel } from '../support/editor';
import { byTestId, clickTestId } from '../support/ui';
import { waitFor } from '../support/wait';
import { QUIET_SCRIPT } from '../bench/page';
import { openProjectFile } from '../bench/project';
import {
  readBaseline,
  recordOutcome,
  type VisualOutcome,
  visualSettings,
  writeCandidate,
} from './baselines';
import { compareScreenshots, MAX_DIFF_RATIO, pngSize, VisualCompareError } from './compare';
import { GUESSING_GAME_MAIN, writeFixture } from './fixture';

/** The window size the baselines are taken at, in CSS pixels. */
export const WINDOW = { width: 1000, height: 700 } as const;

/** The screenshot's name (and its baseline's file name). */
const SHOT = 'canvas-guessing-game';

/**
 * Runs in the webview: takes focus away from whatever has it (no focus ring, no text caret), hides
 * the caret everywhere in case an editor opens, and answers whether the page prefers the dark
 * theme. Inline style properties set from script are allowed by the app's style-src.
 */
const STILL_SCRIPT = `
  const active = document.activeElement;
  if (active instanceof HTMLElement || active instanceof SVGElement) {
    active.blur();
  }
  document.documentElement.style.setProperty('caret-color', 'transparent');
  return window.matchMedia('(prefers-color-scheme: dark)').matches;
`;

/** Runs in the webview (async): calls back once the page's fonts have loaded. */
const FONTS_SCRIPT = `
  const done = arguments[arguments.length - 1];
  document.fonts.ready.then(() => done(true), () => done(false));
`;

const folders: string[] = [];

afterAll(() => {
  for (const folder of folders) {
    rmSync(folder, { recursive: true, force: true });
  }
});

/** Folds the C++ panel and the bottom panel away, so the canvas gets the whole window. */
async function foldDocks(app: App): Promise<void> {
  await foldCodePanel(app);
  await clickTestId(app.driver, 'bottom-dock-toggle');
  await waitFor(
    async () =>
      (await app.driver.findElements(By.css('.bottom-dock[data-collapsed="true"]'))).length > 0,
    { timeout: 5_000, message: 'the bottom panel to fold away' },
  );
}

/** Writes a GitHub Actions warning annotation (plain text elsewhere). */
function warn(message: string): void {
  process.stdout.write(`::warning title=Visual diff::${message.replace(/\r?\n/g, ' ')}\n`);
}

/**
 * Sets the window to {@link WINDOW}. A native driver that cannot resize the app's window (WebDriver
 * window commands are optional for an embedded webview) leaves it at the app's own size, which is
 * as repeatable: that is reported, and the baseline of that system has that size.
 */
async function setWindowSize(driver: WebDriver): Promise<void> {
  try {
    await driver.manage().window().setRect({ width: WINDOW.width, height: WINDOW.height });
    const rect = await driver.manage().window().getRect();
    if (rect.width !== WINDOW.width || rect.height !== WINDOW.height) {
      warn(
        `The window is ${String(rect.width)}×${String(rect.height)}, not ${String(WINDOW.width)}×${String(WINDOW.height)}`,
      );
    }
  } catch (error: unknown) {
    warn(
      `The native driver could not resize the window (${error instanceof Error ? error.message : String(error)}); the app's own size is used`,
    );
  }
}

/** The page's viewport size, in CSS pixels. */
async function viewportSize(driver: WebDriver): Promise<{ width: number; height: number }> {
  const size: unknown = await driver.executeScript(
    'return [window.innerWidth, window.innerHeight];',
  );
  if (
    !Array.isArray(size) ||
    size.length !== 2 ||
    !size.every((side) => typeof side === 'number' && side > 0)
  ) {
    throw new Error('The page did not report its size');
  }
  const [width, height] = size as [number, number];
  return { width, height };
}

/** Waits until nothing moves or repaints on the page any more (animations, the first preview). */
async function waitUntilStill(app: App): Promise<void> {
  const fonts: unknown = await app.driver.executeAsyncScript(FONTS_SCRIPT);
  if (fonts !== true) {
    throw new Error('The page fonts did not load');
  }
  const quiet: unknown = await app.driver.executeAsyncScript(QUIET_SCRIPT, 20, 40, 30_000);
  if (quiet !== 'quiet') {
    throw new Error('The page did not come to rest within 30 s');
  }
}

describe('visual diff: the block canvas', () => {
  it('draws the guessing game as this system’s baseline does', async (context) => {
    const settings = visualSettings();
    const folder = mkdtempSync(path.join(tmpdir(), 'b2c-visual-'));
    folders.push(folder);
    const app = await launchApp(context, { dialogs: { open: [writeFixture(folder)] } });
    await setWindowSize(app.driver);
    await openProjectFile(app, { blockId: GUESSING_GAME_MAIN, code: 'Too low!', timeout: 60_000 });
    await foldDocks(app);
    // The pointer rests in the window's bottom-left corner (the status bar), hovering nothing.
    const viewport = await viewportSize(app.driver);
    await app.driver
      .actions({ async: true })
      .move({ x: 2, y: viewport.height - 4, origin: Origin.VIEWPORT })
      .perform();
    const dark: unknown = await app.driver.executeScript(STILL_SCRIPT);
    if (dark !== false) {
      throw new Error('The visual diff needs the light theme, but the system prefers dark');
    }
    await waitUntilStill(app);

    const workspace = await app.driver.findElement(byTestId('workspace'));
    const screenshot = Buffer.from(await workspace.takeScreenshot(), 'base64');
    const baseline = settings.update ? null : readBaseline(settings, SHOT);

    let outcome: VisualOutcome;
    if (settings.update) {
      outcome = { kind: 'updated', size: pngSize(screenshot) };
    } else if (baseline === null) {
      outcome = { kind: 'noBaseline', size: pngSize(screenshot) };
    } else {
      try {
        outcome = { kind: 'compared', result: compareScreenshots(baseline, screenshot) };
      } catch (error: unknown) {
        if (!(error instanceof VisualCompareError)) {
          throw error;
        }
        outcome = { kind: 'sizeMismatch', message: error.message };
      }
    }
    const candidate = writeCandidate(
      settings,
      SHOT,
      screenshot,
      outcome.kind === 'compared' ? outcome.result.diffPng : null,
    );
    recordOutcome(settings, SHOT, outcome);

    if (outcome.kind === 'noBaseline') {
      // Not a failure: the first run on a system makes the baseline to review and commit.
      warn(`No ${settings.os} baseline for ${SHOT} yet; the candidate is ${candidate}`);
    }
    if (outcome.kind === 'sizeMismatch') {
      throw new Error(`${outcome.message}. The candidate is ${candidate}`);
    }
    if (outcome.kind === 'compared') {
      const { result } = outcome;
      expect(
        result.ratio,
        `${String(result.diffPixels)} pixels differ from the ${settings.os} baseline ` +
          `(${(result.ratio * 100).toFixed(3)}%); see ${candidate} and its .diff.png`,
      ).toBeLessThanOrEqual(MAX_DIFF_RATIO);
    }
  });
});
