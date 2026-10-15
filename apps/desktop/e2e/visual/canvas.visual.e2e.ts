/**
 * Visual diff of the block canvas (docs/spec/09-quality-and-delivery.md §9.2, 10 §10.2 risk 2): the
 * Zelos renderer and the Blocks2Cpp theme as WebKitGTK and WebView2 draw them. The guessing game
 * (visual/fixture.ts) is opened in a 1000×700 window (the app's window-size seam) at zoom 1.0 in
 * the light theme, with the Program category chosen in the toolbox, nothing focused and the text
 * caret hidden; a WebDriver screenshot of the workspace element is compared with this system's
 * baseline (visual/baselines/<system>/canvas-guessing-game.png) by pixelmatch, and more than 0.5%
 * of differing pixels fails. The screenshot is always written as a candidate (visual/baselines.ts
 * `checkShot`), so a missing or outdated baseline can be committed from the CI artifact; a test
 * that stops before the screenshot records it as not taken. The chrome's scenes are
 * chrome.visual.e2e.ts.
 */
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { By } from 'selenium-webdriver';
import { afterAll, describe, expect, it } from 'vitest';

import { type App, launchApp } from '../support/app';
import { foldCodePanel } from '../support/editor';
import { byTestId, clickTestId } from '../support/ui';
import { waitFor } from '../support/wait';
import { openProjectFile } from '../bench/project';
import { takeAndCheckShot, visualSettings } from './baselines';
import { GUESSING_GAME_MAIN, writeFixture } from './fixture';
import {
  blurAll,
  hideOverlayScrollbars,
  parkPointer,
  selectCategory,
  SHOWN_CATEGORY,
  viewportSize,
  waitUntilStill,
  warn,
} from './settle';

/** The window size the baselines are taken at, in CSS pixels. */
export const WINDOW = { width: 1000, height: 700 } as const;

/** The screenshot's name (and its baseline's file name). */
const SHOT = 'canvas-guessing-game';

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

describe('visual diff: the block canvas', () => {
  it('draws the guessing game as this system’s baseline does', async (context) => {
    const settings = visualSettings();
    const check = await takeAndCheckShot(settings, SHOT, async () => {
      const folder = mkdtempSync(path.join(tmpdir(), 'b2c-visual-'));
      folders.push(folder);
      // The window-size seam: WebDriver's setRect may not resize WebView2's web view.
      const app = await launchApp(context, {
        dialogs: { open: [writeFixture(folder)] },
        windowSize: WINDOW,
      });
      const viewport = await viewportSize(app.driver);
      if (viewport.width !== WINDOW.width || viewport.height !== WINDOW.height) {
        warn(
          `The window is ${String(viewport.width)}×${String(viewport.height)}, not ${String(WINDOW.width)}×${String(WINDOW.height)} (a smaller screen)`,
        );
      }
      await openProjectFile(app, {
        blockId: GUESSING_GAME_MAIN,
        code: 'Too low!',
        timeout: 60_000,
      });
      await foldDocks(app);
      // The toolbox's selected category is part of the screenshot.
      await selectCategory(app.driver, SHOWN_CATEGORY);
      // The pointer rests in the window's bottom-left corner (the status bar), hovering nothing.
      await parkPointer(app.driver);
      await hideOverlayScrollbars(app.driver);
      if (await blurAll(app.driver)) {
        throw new Error('The visual diff needs the light theme, but the system prefers dark');
      }
      await waitUntilStill(app.driver);
      const workspace = await app.driver.findElement(byTestId('workspace'));
      return Buffer.from(await workspace.takeScreenshot(), 'base64');
    });
    if (check.warning !== null) {
      warn(check.warning);
    }
    expect(check.failure).toBeNull();
  });
});
