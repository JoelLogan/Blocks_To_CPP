/** The chrome scenes (scenes.ts): names, themes, areas, masks and the toolbox categories. */
import { readFileSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

import { REPOSITORY_ROOT } from '../support/env';
import { shotName } from './baselines';
import {
  CHROME_SCENES,
  CHROME_WINDOW,
  chromeScene,
  chromeShotName,
  chromeShotNames,
  CODE_HEADER_MASK,
  consoleScrollProblem,
  CONSOLE_SCROLL_LINES,
  ELAPSED_MASK,
  GTK_THEMES,
  PROGRAM_LINES,
  sceneRunsOn,
  themesOf,
  TOOLBOX_CATEGORIES,
  TOOLCHAIN_MASK,
  toolboxSceneId,
  WINDOW_AREA,
} from './scenes';

/** The categories of catalog/toolbox.toml, in order: each `[[category]]`'s `id` and `name`. */
function catalogCategories(): { id: string; name: string }[] {
  const text = readFileSync(path.join(REPOSITORY_ROOT, 'catalog', 'toolbox.toml'), 'utf8');
  return text
    .split(/^\[\[category\]\]\s*$/m)
    .slice(1)
    .map((part) => {
      const head = part.split(/^\[\[/m)[0] ?? '';
      const id = /^id = "([^"]+)"$/m.exec(head)?.[1];
      const name = /^name = "([^"]+)"$/m.exec(head)?.[1];
      if (id === undefined || name === undefined) {
        throw new Error(`A toolbox category without an id or a name: ${head}`);
      }
      return { id, name };
    });
}

describe('the scenes', () => {
  it('are the ones the plan lists, in the order they are taken', () => {
    expect(CHROME_SCENES.map((scene) => scene.id)).toEqual([
      'window-smallest',
      'toolbar-select',
      'toolbar-select-hover',
      'menu-open',
      ...TOOLBOX_CATEGORIES.map(toolboxSceneId),
      'console-scrolled',
      'console-two-runs',
    ]);
  });

  it('take a toolbox scene for each category of the catalog, in toolbox order', () => {
    expect(TOOLBOX_CATEGORIES).toEqual(catalogCategories());
    expect(TOOLBOX_CATEGORIES.map(toolboxSceneId)).toContain('toolbox-io');
  });

  it('each have an area and say what they show', () => {
    for (const scene of CHROME_SCENES) {
      expect(scene.shows.length, scene.id).toBeGreaterThan(10);
      if (scene.area !== WINDOW_AREA) {
        expect(scene.area.length, scene.id).toBeGreaterThan(0);
      }
    }
    expect(chromeScene('window-smallest').area).toBe(WINDOW_AREA);
    expect(chromeScene('menu-open').area).toBe(WINDOW_AREA);
    expect(chromeScene('toolbox-math').area).toEqual([
      '.blocklyToolbox',
      'svg.blocklyToolboxFlyout',
    ]);
    expect(chromeScene('console-two-runs').area).toEqual(['section.bottom-dock']);
  });

  it('hover only the Debug/Release drop-down', () => {
    expect(
      CHROME_SCENES.filter((scene) => scene.hover !== undefined).map((scene) => scene.id),
    ).toEqual(['toolbar-select-hover']);
    expect(chromeScene('toolbar-select-hover').hover).toBe('[data-testid="toolbar-config"]');
  });

  it('mask what changes between machines and runs wherever it shows', () => {
    for (const id of ['window-smallest', 'menu-open']) {
      expect(chromeScene(id).masks, id).toEqual([TOOLCHAIN_MASK, CODE_HEADER_MASK]);
    }
    for (const id of ['console-scrolled', 'console-two-runs']) {
      expect(chromeScene(id).masks, id).toEqual([ELAPSED_MASK]);
    }
    // The toolbar and the toolbox show nothing that changes.
    expect(chromeScene('toolbar-select').masks).toEqual([]);
    expect(chromeScene('toolbox-loops').masks).toEqual([]);
    // A mask whose text can change length keeps a fixed width, so nothing after it moves.
    expect(TOOLCHAIN_MASK.width).toBeGreaterThan(0);
    expect(ELAPSED_MASK.width).toBeGreaterThan(0);
    expect(TOOLCHAIN_MASK.selector).toBe('[data-testid="status-toolchain"]');
    expect(ELAPSED_MASK.selector).toBe('[data-testid="console-elapsed"]');
  });

  it('are found by ID', () => {
    expect(chromeScene('toolbar-select').id).toBe('toolbar-select');
    expect(() => chromeScene('nothing')).toThrow(/no chrome scene nothing/);
  });
});

describe('themes and names', () => {
  it('take light on both systems and dark on Linux only', () => {
    expect(themesOf('linux')).toEqual(['light', 'dark']);
    expect(themesOf('windows')).toEqual(['light']);
    expect(GTK_THEMES).toEqual({ light: 'Adwaita', dark: 'Adwaita:dark' });
  });

  it('name each scene per theme, as a screenshot name', () => {
    const scene = chromeScene('menu-open');
    expect(chromeShotName(scene, 'light')).toBe('chrome-menu-open');
    expect(chromeShotName(scene, 'dark')).toBe('chrome-menu-open-dark');
    for (const os of ['linux', 'windows'] as const) {
      const names = chromeShotNames(os);
      expect(new Set(names).size, os).toBe(names.length);
      for (const name of names) {
        expect(shotName(name)).toBe(name);
        expect(name).not.toBe('canvas-guessing-game');
      }
    }
    expect(chromeShotNames('linux')).toHaveLength(CHROME_SCENES.length * 2);
    expect(chromeShotNames('windows')).toHaveLength(CHROME_SCENES.length);
    expect(chromeShotNames('windows').some((name) => name.endsWith('-dark'))).toBe(false);
    expect(chromeShotNames('linux')).toContain('chrome-toolbox-functions-dark');
  });

  it('runs a scene on the systems it names, or on both', () => {
    const scene = chromeScene('menu-open');
    expect(sceneRunsOn(scene, 'linux')).toBe(true);
    expect(sceneRunsOn(scene, 'windows')).toBe(true);
    expect(sceneRunsOn({ ...scene, systems: ['linux'] }, 'windows')).toBe(false);
    expect(sceneRunsOn({ ...scene, systems: ['linux'] }, 'linux')).toBe(true);
  });
});

describe('the window and the program', () => {
  it('use the smallest window the app allows', () => {
    const config = JSON.parse(
      readFileSync(path.join(REPOSITORY_ROOT, 'apps/desktop/src-tauri/tauri.conf.json'), 'utf8'),
    ) as { app: { windows: { minWidth: number; minHeight: number }[] } };
    const window = config.app.windows[0];
    expect(CHROME_WINDOW).toEqual({ width: window?.minWidth, height: window?.minHeight });
  });

  it('print plain text that is the same every time', () => {
    for (const line of PROGRAM_LINES) {
      expect(line).toMatch(/^[A-Za-z ]+$/);
    }
    expect(CONSOLE_SCROLL_LINES).toBeGreaterThan(0);
  });

  it('scroll the console only from the end, with enough rows above the view', () => {
    // At the end, 4 rows above a view of 5 (two runs: 9 rows of output).
    expect(consoleScrollProblem({ viewportY: 4, baseY: 4, rows: 5 }, CONSOLE_SCROLL_LINES)).toBe(
      null,
    );
    // The panel shows 6 rows: only 3 are above it.
    expect(consoleScrollProblem({ viewportY: 3, baseY: 3, rows: 6 }, CONSOLE_SCROLL_LINES)).toBe(
      'only 3 rows are above its view (6 rows shown)',
    );
    // Scrolled away from the end already.
    expect(consoleScrollProblem({ viewportY: 1, baseY: 4, rows: 5 }, CONSOLE_SCROLL_LINES)).toBe(
      'its view is at row 1, not at the end (row 4; 5 rows shown)',
    );
  });
});
