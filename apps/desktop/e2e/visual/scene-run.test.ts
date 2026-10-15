/**
 * The bookkeeping of a launch's chrome scenes (scene-run.ts), against a temporary output folder:
 * the summary lists every scene of the system once per theme, whatever stopped the test.
 */
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { PNG } from 'pngjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { BenchOs } from '../bench/results';
import { VISUAL_SUMMARY, type VisualSettings } from './baselines';
import { UnstableSceneError } from './capture';
import { changedArea } from './image';
import { SceneRun, type ShootScene } from './scene-run';
import {
  CHROME_SCENES,
  type ChromeScene,
  chromeScene,
  chromeShotName,
  chromeShotNames,
  type Theme,
  themesOf,
} from './scenes';
import { PageNotStillError } from './settle';

const folders: string[] = [];

beforeEach(() => {
  // checkShot's warnings (no baseline yet) go to stdout as annotations.
  vi.spyOn(process.stdout, 'write').mockImplementation(() => true);
});

afterEach(() => {
  vi.restoreAllMocks();
  for (const folder of folders.splice(0)) {
    rmSync(folder, { recursive: true, force: true });
  }
});

/** Settings for `os` with empty baselines and an output folder of their own. */
function settingsFor(os: BenchOs): VisualSettings {
  const folder = mkdtempSync(path.join(tmpdir(), 'b2c-scene-run-'));
  folders.push(folder);
  return {
    os,
    baselines: path.join(folder, 'baselines', os),
    out: path.join(folder, 'out'),
    update: false,
  };
}

/** A `width`×`height` PNG of one colour. */
function png(width: number, height: number, grey = 240): Buffer {
  const image = new PNG({ width, height });
  image.data.fill(grey);
  for (let index = 3; index < image.data.length; index += 4) {
    image.data[index] = 255;
  }
  return PNG.sync.write(image);
}

/** The summary's rows: each screenshot name and its result cell, in order. */
function rows(settings: VisualSettings): { name: string; result: string }[] {
  const file = path.join(settings.out, VISUAL_SUMMARY);
  if (!existsSync(file)) {
    return [];
  }
  return readFileSync(file, 'utf8')
    .split('\n')
    .filter((line) => line.startsWith('| chrome-'))
    .map((line) => {
      const cells = line.split('|').map((cell) => cell.trim());
      return { name: cells[1] ?? '', result: cells[5] ?? '' };
    });
}

/** The names of this system's scenes in `theme`, in order. */
function namesOf(os: BenchOs, theme: Theme): string[] {
  return CHROME_SCENES.map((scene) => chromeShotName(scene, theme)).filter((name) =>
    chromeShotNames(os).includes(name),
  );
}

/** A shooter that gives a small image for every scene, except those `fail` names. */
function shooter(fail: Record<string, Error> = {}): ShootScene & { shot: string[] } {
  const shot: string[] = [];
  const shoot = (scene: ChromeScene): Promise<Buffer> => {
    shot.push(scene.id);
    const error = fail[scene.id];
    if (error !== undefined) {
      return Promise.reject(error);
    }
    return Promise.resolve(png(4, 3));
  };
  return Object.assign(shoot, { shot });
}

describe('a launch that takes every scene', () => {
  it('lists each scene of each theme once, in order, with nothing missing', async () => {
    for (const os of ['linux', 'windows'] as const) {
      const settings = settingsFor(os);
      for (const theme of themesOf(os)) {
        const run = new SceneRun(settings, theme, shooter());
        for (const scene of CHROME_SCENES) {
          await run.take(scene.id);
        }
        expect(run.missing(), `${os} ${theme}`).toEqual([]);
        // No baselines yet: every scene passes with a warning.
        expect(run.failures).toEqual([]);
      }
      expect(rows(settings).map((row) => row.name)).toEqual(chromeShotNames(os));
      expect(rows(settings).every((row) => row.result.startsWith('no baseline yet'))).toBe(true);
    }
  });
});

describe('a scene that fails', () => {
  it('is recorded as not taken, and the next scenes are still taken', async () => {
    const settings = settingsFor('linux');
    const shoot = shooter({ 'toolbar-select': new Error('the drop-down is missing') });
    const run = new SceneRun(settings, 'light', shoot);
    for (const scene of CHROME_SCENES) {
      await run.take(scene.id);
    }
    expect(shoot.shot).toEqual(CHROME_SCENES.map((scene) => scene.id));
    expect(run.failures).toEqual(['chrome-toolbar-select was not taken: the drop-down is missing']);
    expect(run.missing()).toEqual([]);
    const toolbar = rows(settings).find((row) => row.name === 'chrome-toolbar-select');
    expect(toolbar?.result).toBe('**fail**: not taken: the drop-down is missing');
    expect(rows(settings)).toHaveLength(CHROME_SCENES.length);
  });

  it('that kept changing leaves its last image and what moved as the candidate', async () => {
    const settings = settingsFor('linux');
    const previous = png(4, 3, 240);
    const last = png(4, 3, 10);
    const unstable = new UnstableSceneError(previous, last, changedArea(previous, last));
    const run = new SceneRun(settings, 'dark', shooter({ 'menu-open': unstable }));
    await run.take('menu-open');
    const dir = path.join(settings.out, 'linux');
    expect(readFileSync(path.join(dir, 'chrome-menu-open-dark.png'))).toEqual(last);
    expect(existsSync(path.join(dir, 'chrome-menu-open-dark.diff.png'))).toBe(true);
    expect(rows(settings)).toEqual([
      {
        name: 'chrome-menu-open-dark',
        result: expect.stringMatching(
          /^\*\*fail\*\*: not taken: The scene still changed/,
        ) as string,
      },
    ]);
  });
});

describe('a test that stops early', () => {
  it('lists the scenes it never reached as not taken, once each, in the theme’s names', async () => {
    const settings = settingsFor('linux');
    const run = new SceneRun(settings, 'dark', shooter());
    const taken = CHROME_SCENES.slice(0, 3);
    for (const scene of taken) {
      await run.take(scene.id);
    }
    run.notReached(new Error('the build failed'));
    // A second call (a failure while recording) adds nothing.
    run.notReached(new Error('again'));
    expect(run.missing()).toEqual([]);
    expect(rows(settings).map((row) => row.name)).toEqual(namesOf('linux', 'dark'));
    for (const row of rows(settings).slice(taken.length)) {
      expect(row.result, row.name).toBe(
        '**fail**: not taken: the test stopped before it: the build failed',
      );
    }
    expect(run.failures).toHaveLength(CHROME_SCENES.length - taken.length);
  });

  it('stops at a page that does not come to rest, which no later scene would either', async () => {
    const settings = settingsFor('linux');
    const stuck = new PageNotStillError('Animations still run after 20 s: spin on div.busy');
    const shoot = shooter({ 'toolbar-select': stuck });
    const run = new SceneRun(settings, 'light', shoot);
    let stopped: unknown = null;
    try {
      for (const scene of CHROME_SCENES) {
        await run.take(scene.id);
      }
    } catch (error: unknown) {
      stopped = error;
      run.notReached(error);
    }
    expect(stopped).toBeInstanceOf(Error);
    expect((stopped as Error).message).toBe(
      'chrome-toolbar-select was not taken, nor the scenes after it: Animations still run after 20 s: spin on div.busy',
    );
    // One wait for the page, not one per scene.
    expect(shoot.shot).toEqual(['window-smallest', 'toolbar-select']);
    expect(run.missing()).toEqual([]);
    expect(rows(settings).map((row) => row.name)).toEqual(namesOf('linux', 'light'));
    expect(rows(settings)[1]?.result).toBe(
      '**fail**: not taken: Animations still run after 20 s: spin on div.busy',
    );
    expect(rows(settings)[2]?.result).toMatch(
      /^\*\*fail\*\*: not taken: the test stopped before it: chrome-toolbar-select was not taken/,
    );
  });
});

describe('scenes of the other system', () => {
  it('are neither taken nor listed nor missing', async () => {
    const scenes: ChromeScene[] = [
      chromeScene('window-smallest'),
      { ...chromeScene('console-scrolled'), systems: ['linux'] },
      chromeScene('console-two-runs'),
    ];
    const settings = settingsFor('windows');
    const shoot = shooter();
    const run = new SceneRun(settings, 'light', shoot, scenes);
    await run.take('window-smallest');
    await run.take('console-scrolled');
    expect(shoot.shot).toEqual(['window-smallest']);
    expect(run.missing()).toEqual(['chrome-console-two-runs']);
    run.notReached(new Error('stopped'));
    expect(rows(settings).map((row) => row.name)).toEqual([
      'chrome-window-smallest',
      'chrome-console-two-runs',
    ]);
    expect(run.missing()).toEqual([]);
  });

  it('refuses a scene that does not exist', async () => {
    const run = new SceneRun(settingsFor('linux'), 'light', shooter());
    await expect(run.take('nothing')).rejects.toThrow(/no chrome scene nothing/);
  });
});
