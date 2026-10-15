/**
 * Visual diff of the window's chrome (docs/spec/09-quality-and-delivery.md §9.2, the M2 polish
 * plan's W7-1): the scenes of scenes.ts, in the smallest window (800×560), each compared with this
 * system's baseline like the block canvas (baselines.ts `checkShot`). One launch of the app per
 * theme: light on both systems, and dark on Linux (`GTK_THEME=Adwaita:dark`).
 *
 * In each launch: a new project from the Empty template with a program that prints three lines;
 * the whole window; the toolbar with its Debug/Release drop-down, then with the pointer on it; the
 * ≡ main menu open over the editor; the toolbox after a click on each category; two runs of the
 * program, the console scrolled up a fixed number of lines, and the console with the bottom panel
 * at its tallest. Before each screenshot the pointer is parked (or rests on what the scene
 * hovers), nothing has the focus, no caret shows, the page has its fonts and has come to rest
 * (settle.ts), and two screenshots in a row are the same (capture.ts). The status bar's toolchain,
 * the console's elapsed time and the C++ panel's first line (the app's version) are masked.
 *
 * A scene that fails does not stop the others: every scene is listed in the summary, the ones the
 * test never reached as not taken, and the test fails at the end with all the failures.
 */
import { By, Key, type WebDriver } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { codeHas } from '../bench/project';
import { NodeIds, print } from '../support/bdm';
import { waitForConsoleState } from '../support/console';
import { clickTestId, openCategory } from '../support/ui';
import { waitFor } from '../support/wait';
import { waitForTranscript } from '../specs/flows/lib/console';
import { type FlowApp, startFlow } from '../specs/flows/lib/launch';
import { newEmptyProject } from '../specs/flows/lib/project';
import { BUILD_TIMEOUT_MS, UI_TIMEOUT_MS, waitForToolchain } from '../specs/flows/lib/ui';
import {
  checkShot,
  recordOutcome,
  visualSettings,
  type VisualSettings,
  writeCandidate,
} from './baselines';
import { captureScene, fixMaskWidths, UnstableSceneError } from './capture';
import { compareScreenshots } from './compare';
import {
  CHROME_SCENES,
  CHROME_WINDOW,
  chromeScene,
  chromeShotName,
  CONSOLE_SCROLL_LINES,
  GTK_THEMES,
  PROGRAM_LINES,
  sceneRunsOn,
  type Theme,
  themesOf,
  TOOLBOX_CATEGORIES,
  toolboxSceneId,
} from './scenes';
import { blurAll, hideOverlayScrollbars, parkPointer, waitUntilStill, warn } from './settle';

/** How long one theme's launch and scenes may take (two builds and runs among them). */
const THEME_TIMEOUT_MS = 420_000;

/** The bottom panel's splitter. */
const BOTTOM_SPLITTER = '[role="separator"][aria-label="Resize the bottom panel"]';

/** The scenes of one launch: takes them, and records what each came to. */
class SceneRun {
  readonly #settings: VisualSettings;
  readonly #theme: Theme;
  readonly #driver: () => WebDriver;
  /** The screenshot names recorded in the summary so far. */
  readonly #recorded = new Set<string>();
  /** Why scenes failed, one line each. */
  readonly failures: string[] = [];

  constructor(settings: VisualSettings, theme: Theme, driver: () => WebDriver) {
    this.#settings = settings;
    this.#theme = theme;
    this.#driver = driver;
  }

  /**
   * Takes scene `id` and compares it. A failure is noted (and recorded in the summary) and does
   * not stop the test.
   */
  async take(id: string): Promise<void> {
    const scene = chromeScene(id);
    if (!sceneRunsOn(scene, this.#settings.os)) {
      return;
    }
    const name = chromeShotName(scene, this.#theme);
    const driver = this.#driver();
    let image: Buffer;
    try {
      if (scene.hover === undefined) {
        await parkPointer(driver);
      } else {
        await driver
          .actions({ async: true })
          .move({ origin: await driver.findElement(By.css(scene.hover)) })
          .perform();
      }
      await fixMaskWidths(driver, scene.masks);
      await hideOverlayScrollbars(driver);
      const dark = await blurAll(driver);
      if (dark !== (this.#theme === 'dark')) {
        throw new Error(
          `The ${this.#theme} scenes need the page to prefer ${this.#theme === 'dark' ? 'the dark' : 'the light'} theme, but it does not` +
            (this.#settings.os === 'linux' ? ` (GTK_THEME=${GTK_THEMES[this.#theme]})` : ''),
        );
      }
      await waitUntilStill(driver);
      image = await captureScene(driver, scene.area, scene.masks);
    } catch (error: unknown) {
      if (error instanceof UnstableSceneError && error.changed !== null) {
        // What moved: the last image, and the pixels that differ from the one before in red.
        writeCandidate(
          this.#settings,
          name,
          error.last,
          compareScreenshots(error.previous, error.last).diffPng,
        );
      }
      this.notTaken(name, error);
      return;
    }
    const check = checkShot(this.#settings, name, image);
    this.#recorded.add(name);
    if (check.warning !== null) {
      warn(check.warning);
    }
    if (check.failure !== null) {
      this.failures.push(check.failure);
    }
  }

  /** Records `name` as not taken, because of `error`. */
  notTaken(name: string, error: unknown): void {
    const reason = error instanceof Error ? error.message : String(error);
    recordOutcome(this.#settings, name, { kind: 'notTaken', reason });
    this.#recorded.add(name);
    this.failures.push(`${name} was not taken: ${reason}`);
  }

  /** Records every scene of this system not recorded yet as not taken, because of `error`. */
  notReached(error: unknown): void {
    const reason = `the test stopped before it: ${error instanceof Error ? error.message : String(error)}`;
    for (const scene of CHROME_SCENES) {
      const name = chromeShotName(scene, this.#theme);
      if (sceneRunsOn(scene, this.#settings.os) && !this.#recorded.has(name)) {
        this.notTaken(name, new Error(reason));
      }
    }
  }

  /** The scenes of this system that were never recorded (the test's own check). */
  missing(): string[] {
    return CHROME_SCENES.filter((scene) => sceneRunsOn(scene, this.#settings.os))
      .map((scene) => chromeShotName(scene, this.#theme))
      .filter((name) => !this.#recorded.has(name));
  }
}

/** The program: one `io.print` per line of {@link PROGRAM_LINES}. */
function program(): ReturnType<typeof print>[] {
  const ids = new NodeIds('visual');
  return PROGRAM_LINES.map((line) => print(ids, line));
}

/** How many runs of the program the console transcript holds. */
function finishedRuns(transcript: string): number {
  const last = PROGRAM_LINES.at(-1) ?? '';
  return transcript.split('\n').filter((line) => line.trim() === last).length;
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

/** Opens the ≡ main menu with a click and waits for its items. */
async function openMainMenu(driver: WebDriver): Promise<void> {
  await clickTestId(driver, 'main-menu');
  await waitFor(async () => (await driver.findElements(By.css('[role="menuitem"]'))).length > 0, {
    timeout: UI_TIMEOUT_MS,
    message: 'the main menu’s items',
  });
}

/** Closes the main menu with a second click on ≡. */
async function closeMainMenu(driver: WebDriver): Promise<void> {
  await clickTestId(driver, 'main-menu');
  await waitFor(async () => (await driver.findElements(By.css('[role="menuitem"]'))).length === 0, {
    timeout: UI_TIMEOUT_MS,
    message: 'the main menu to close',
  });
}

/**
 * Scrolls the console up `lines` lines with one wheel event in lines (`deltaMode` 1), which the
 * console's own wheel handler turns into exactly that many (src/panels/console/wheel.ts), and
 * checks that the view moved that far from the end.
 */
async function scrollConsoleUp(app: FlowApp, lines: number): Promise<void> {
  const before = await app.hook.call('consoleScreen');
  if (before.viewportY !== before.baseY || before.baseY < lines) {
    throw new Error(
      `The console cannot scroll up ${String(lines)} lines from its end: its view is at row ` +
        `${String(before.viewportY)} of ${String(before.baseY)} (${String(before.rows)} rows shown)`,
    );
  }
  await app.driver.executeScript(
    `const screen = document.querySelector('[data-testid="console-terminal"] .xterm-screen');
     const box = screen.getBoundingClientRect();
     screen.dispatchEvent(new WheelEvent('wheel', {
       deltaY: -arguments[0],
       deltaMode: 1,
       bubbles: true,
       cancelable: true,
       clientX: box.left + box.width / 2,
       clientY: box.top + box.height / 2,
     }));`,
    lines,
  );
  await waitFor(
    async () => {
      const now = await app.hook.call('consoleScreen');
      return now.viewportY === now.baseY - lines;
    },
    {
      timeout: UI_TIMEOUT_MS,
      message: async () => {
        const now = await app.hook.call('consoleScreen');
        return `the console's view ${String(lines)} rows above its end (it is at row ${String(now.viewportY)} of ${String(now.baseY)})`;
      },
    },
  );
}

/**
 * Makes the bottom panel as tall as it goes (its splitter's End key) and waits until the console
 * has been fitted to it: the same rows twice in a row.
 */
async function tallestBottomDock(app: FlowApp): Promise<void> {
  const splitter = await app.driver.findElement(By.css(BOTTOM_SPLITTER));
  await splitter.sendKeys(Key.END);
  await waitFor(
    async () =>
      (await splitter.getAttribute('aria-valuenow')) ===
      (await splitter.getAttribute('aria-valuemax')),
    { timeout: UI_TIMEOUT_MS, message: 'the bottom panel at its tallest' },
  );
  let last = -1;
  await waitFor(
    async () => {
      const rows = (await app.hook.call('consoleScreen')).rows;
      const settled = rows === last;
      last = rows;
      return settled;
    },
    { timeout: UI_TIMEOUT_MS, interval: 250, message: 'the console to fit the taller panel' },
  );
}

/** Launches the app for `theme`, in the scenes' window, and checks the page's theme. */
async function launch(
  context: Parameters<typeof startFlow>[0],
  settings: VisualSettings,
  theme: Theme,
): Promise<FlowApp> {
  return startFlow(context).launch({
    windowSize: CHROME_WINDOW,
    // WebKitGTK follows GTK's theme for prefers-color-scheme; WebView2 follows Windows'.
    ...(settings.os === 'linux' ? { env: { GTK_THEME: GTK_THEMES[theme] } } : {}),
  });
}

describe('visual diff: the chrome', () => {
  const settings = visualSettings();
  for (const theme of themesOf(settings.os)) {
    it(
      `draws the chrome in the ${theme} theme as this system’s baselines do`,
      async (context) => {
        let app: FlowApp | null = null;
        const scenes = new SceneRun(settings, theme, () => {
          if (app === null) {
            throw new Error('The app has not started');
          }
          return app.driver;
        });
        try {
          const started = await launch(context, settings, theme);
          app = started;
          const { driver } = started;
          await waitForToolchain(started);
          const mainId = await newEmptyProject(started);
          await started.hook.insertBlocks(mainId, 'BODY', program());
          const lastLine = PROGRAM_LINES.at(-1) ?? '';
          await waitFor(() => codeHas(started, lastLine), {
            timeout: UI_TIMEOUT_MS,
            message: 'the program in the C++ panel',
          });

          await scenes.take('window-smallest');
          await scenes.take('toolbar-select');
          await scenes.take('toolbar-select-hover');

          await openMainMenu(driver);
          await scenes.take('menu-open');
          await closeMainMenu(driver);

          for (const category of TOOLBOX_CATEGORIES) {
            await openCategory(driver, category.name);
            await scenes.take(toolboxSceneId(category));
          }

          await clickTestId(driver, 'toolbar-run');
          await parkPointer(driver);
          await waitForRun(started, 1);
          await clickTestId(driver, 'console-run-again');
          await parkPointer(driver);
          await waitForRun(started, 2);
          await scrollConsoleUp(started, CONSOLE_SCROLL_LINES);
          await scenes.take('console-scrolled');
          await tallestBottomDock(started);
          await scenes.take('console-two-runs');
        } catch (error: unknown) {
          scenes.notReached(error);
          throw error;
        }
        expect(scenes.missing(), 'scenes the test never took').toEqual([]);
        expect(scenes.failures).toEqual([]);
      },
      THEME_TIMEOUT_MS,
    );
  }
});
