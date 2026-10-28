/**
 * The Appearance setting (docs/spec/04-user-interface.md §4.8, F-1 and U14d of the M2 polish plan)
 * and the expert-mode switch (03 §3.5.2): choosing Dark, then Light, on the Settings page changes
 * the whole window at once (the page's theme and scheme, the block canvas and the console); the
 * choice is kept in settings.json and applies again after a restart; *Follow the system* gives the
 * system's theme back. The expert-mode switch saves both of its settings.
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';

import { By, type WebDriver } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { showConsole } from '../../support/console';
import { waitFor } from '../../support/wait';
import { startFlow } from './lib/launch';
import { newEmptyProject } from './lib/project';
import { backToEditor, openSettings, UI_TIMEOUT_MS } from './lib/ui';

/** The colours each theme gives the canvas and the console (app.css, panels.css), as CSS gives them. */
const EXPECTED = {
  light: { canvas: 'rgb(247, 248, 251)', console: 'rgb(251, 252, 254)', scheme: 'light' },
  dark: { canvas: 'rgb(25, 29, 38)', console: 'rgb(22, 26, 34)', scheme: 'dark' },
} as const;

/** What the page shows, from {@link FACTS_SCRIPT}. */
interface ThemeFacts {
  readonly theme: string | null;
  readonly systemDark: boolean;
  readonly scheme: string;
  readonly canvas: string | null;
  readonly console: string | null;
}

/**
 * Runs in the webview: the page's theme and colour scheme, whether the system prefers dark, the
 * canvas's background (the Blockly theme's workspace colour, which Blockly sets on its SVG; the
 * background rectangle shows the grid) and the console terminal's.
 */
const FACTS_SCRIPT = `
  const root = document.documentElement;
  const canvas = document.querySelector('[data-testid="workspace"] svg.blocklySvg');
  const viewport = document.querySelector('[data-testid="console-terminal"] .xterm-viewport');
  return {
    theme: root.getAttribute('data-theme'),
    systemDark: window.matchMedia('(prefers-color-scheme: dark)').matches,
    scheme: getComputedStyle(root).colorScheme,
    canvas: canvas === null ? null : getComputedStyle(canvas).backgroundColor,
    console: viewport === null ? null : getComputedStyle(viewport).backgroundColor,
  };
`;

async function facts(driver: WebDriver): Promise<ThemeFacts> {
  return driver.executeScript<ThemeFacts>(FACTS_SCRIPT);
}

/** Waits until the page shows `theme` everywhere: the attribute, the scheme, canvas and console. */
async function waitForTheme(driver: WebDriver, theme: 'light' | 'dark'): Promise<void> {
  const expected = EXPECTED[theme];
  await waitFor(
    async () => {
      const shown = await facts(driver);
      return (
        shown.theme === theme &&
        shown.scheme === expected.scheme &&
        shown.canvas === expected.canvas &&
        shown.console === expected.console
      );
    },
    {
      timeout: UI_TIMEOUT_MS,
      message: async () =>
        `the ${theme} theme everywhere (the page shows ${JSON.stringify(await facts(driver))})`,
    },
  );
}

/** Chooses a radio button by its value inside the Settings field `testId`. */
async function choose(driver: WebDriver, testId: string, value: string): Promise<void> {
  await driver
    .findElement(By.css(`[data-testid="${testId}"] input[type="radio"][value="${value}"]`))
    .click();
}

/** The part of settings.json the test reads. */
interface SavedSettings {
  readonly appearance?: { readonly theme?: unknown };
  readonly editor?: { readonly blockLabels?: unknown; readonly showAdvanced?: unknown };
}

/** Waits until settings.json holds what `check` looks for. */
async function waitForSaved(
  file: string,
  check: (saved: SavedSettings) => boolean,
  message: string,
): Promise<void> {
  await waitFor(
    () => {
      try {
        return check(JSON.parse(readFileSync(file, 'utf8')) as SavedSettings);
      } catch {
        return false;
      }
    },
    { timeout: UI_TIMEOUT_MS, message },
  );
}

describe('Appearance', () => {
  it('switches the whole window between light and dark, keeps the choice and follows the system again', async (context) => {
    const flow = startFlow(context);
    const settingsFile = path.join(flow.folders.profile, 'config', 'settings.json');
    // On Linux the system's scheme is GTK's; a light one makes Follow the system deterministic.
    // WebView2 follows Windows' own setting, which the test reads from the page instead.
    const launchOptions = process.platform === 'linux' ? { env: { GTK_THEME: 'Adwaita' } } : {};

    // (1) Follow the system is the default.
    const first = await flow.launch(launchOptions);
    await newEmptyProject(first);
    await showConsole(first.driver);
    const system = (await facts(first.driver)).systemDark ? 'dark' : 'light';
    await waitForTheme(first.driver, system);

    // (2) Dark: saved, and the page, the canvas and the console follow at once.
    await openSettings(first.driver);
    await choose(first.driver, 'settings-theme', 'dark');
    await waitForSaved(
      settingsFile,
      (saved) => saved.appearance?.theme === 'dark',
      'settings.json to have the dark theme',
    );
    await backToEditor(first.driver);
    await waitForTheme(first.driver, 'dark');

    // (3) Light.
    await openSettings(first.driver);
    await choose(first.driver, 'settings-theme', 'light');
    await waitForSaved(
      settingsFile,
      (saved) => saved.appearance?.theme === 'light',
      'settings.json to have the light theme',
    );
    await backToEditor(first.driver);
    await waitForTheme(first.driver, 'light');

    // (4) The expert-mode switch saves both of its settings in one change.
    await openSettings(first.driver);
    await first.driver.findElement(By.css('[data-testid="settings-expert-mode"]')).click();
    await waitForSaved(
      settingsFile,
      (saved) => saved.editor?.blockLabels === 'cpp' && saved.editor.showAdvanced === true,
      'settings.json to have expert mode on',
    );

    // (5) Dark again, then a restart: the window starts dark.
    await choose(first.driver, 'settings-theme', 'dark');
    await waitForSaved(
      settingsFile,
      (saved) => saved.appearance?.theme === 'dark',
      'settings.json to have the dark theme again',
    );
    await first.quit();
    const second = await flow.launch(launchOptions);
    await newEmptyProject(second);
    await showConsole(second.driver);
    await waitForTheme(second.driver, 'dark');

    // (6) Follow the system: the system's theme again.
    await openSettings(second.driver);
    expect(
      await second.driver.findElement(By.css('[data-testid="settings-expert-mode"]')).isSelected(),
    ).toBe(true);
    await choose(second.driver, 'settings-theme', 'system');
    await waitForSaved(
      settingsFile,
      (saved) => saved.appearance?.theme === 'system',
      'settings.json to follow the system',
    );
    await backToEditor(second.driver);
    await waitForTheme(second.driver, system);
  }, 240_000);
});
