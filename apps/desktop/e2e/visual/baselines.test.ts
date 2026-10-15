/** The visual diff's settings, candidates and summary (baselines.ts). */
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { PNG } from 'pngjs';
import { afterEach, describe, expect, it } from 'vitest';

import {
  BASELINES_DIR,
  checkShot,
  readBaseline,
  recordOutcome,
  shotName,
  summaryLine,
  takeAndCheckShot,
  VISUAL_SUMMARY,
  visualSettings,
  writeCandidate,
} from './baselines';

const folders: string[] = [];

afterEach(() => {
  for (const folder of folders.splice(0)) {
    rmSync(folder, { recursive: true, force: true });
  }
});

function tempFolder(): string {
  const folder = mkdtempSync(path.join(tmpdir(), 'b2c-visual-test-'));
  folders.push(folder);
  return folder;
}

const PNG_BYTES = Buffer.from([0x89, 0x50, 0x4e, 0x47]);

describe('visualSettings', () => {
  it('picks the system’s baselines and the artifacts folder', () => {
    const artifacts = tempFolder();
    const env = { B2C_E2E_ARTIFACTS: artifacts, B2C_E2E_TOOLCHAIN_DIRS: artifacts };
    expect(visualSettings(env, 'linux')).toEqual({
      os: 'linux',
      baselines: path.join(BASELINES_DIR, 'linux'),
      out: path.join(artifacts, 'visual'),
      update: false,
    });
    expect(visualSettings({ ...env, B2C_VISUAL_UPDATE: '1' }, 'win32')).toMatchObject({
      os: 'windows',
      baselines: path.join(BASELINES_DIR, 'windows'),
      update: true,
    });
    expect(visualSettings({ ...env, B2C_VISUAL_UPDATE: 'yes' }, 'linux').update).toBe(false);
  });

  it('keeps the baselines next to this file', () => {
    expect(BASELINES_DIR).toBe(path.join(import.meta.dirname, 'baselines'));
  });
});

describe('shotName', () => {
  it('accepts names that make safe file names only', () => {
    expect(shotName('canvas-guessing-game')).toBe('canvas-guessing-game');
    for (const bad of ['', 'Canvas', 'a--b', '-a', 'a/b', '../a', 'a.png', 'x'.repeat(65)]) {
      expect(() => shotName(bad), bad).toThrow(/not a screenshot name/);
    }
  });
});

describe('candidates and baselines', () => {
  it('reads a missing baseline as none', () => {
    const settings = {
      os: 'linux',
      baselines: tempFolder(),
      out: tempFolder(),
      update: false,
    } as const;
    expect(readBaseline(settings, 'canvas-guessing-game')).toBeNull();
  });

  it('writes the screenshot named like its baseline, and the diff beside it', () => {
    const settings = {
      os: 'windows',
      baselines: tempFolder(),
      out: tempFolder(),
      update: false,
    } as const;
    const file = writeCandidate(settings, 'canvas-guessing-game', PNG_BYTES, Buffer.from('diff'));
    expect(file).toBe(path.join(settings.out, 'windows', 'canvas-guessing-game.png'));
    expect(readFileSync(file)).toEqual(PNG_BYTES);
    expect(
      readFileSync(path.join(settings.out, 'windows', 'canvas-guessing-game.diff.png'), 'utf8'),
    ).toBe('diff');
    writeCandidate(settings, 'other', PNG_BYTES, null);
    expect(existsSync(path.join(settings.out, 'windows', 'other.diff.png'))).toBe(false);
  });
});

describe('the summary', () => {
  const result = {
    width: 963,
    height: 510,
    diffPixels: 12,
    ratio: 12 / (963 * 510),
    passed: true,
    diffPng: Buffer.alloc(0),
  };

  it('has one line per comparison', () => {
    expect(summaryLine('linux', 'shot', { kind: 'compared', result })).toBe(
      '| shot | linux | 963×510 | 12 (0.002%) | pass |',
    );
    expect(
      summaryLine('linux', 'shot', { kind: 'compared', result: { ...result, passed: false } }),
    ).toContain('**fail**');
    expect(summaryLine('windows', 'shot', { kind: 'noBaseline', size: '963×510' })).toContain(
      'no baseline yet',
    );
    expect(summaryLine('linux', 'shot', { kind: 'sizeMismatch', message: 'a | b' })).toBe(
      '| shot | linux | — | — | **fail**: a / b |',
    );
    expect(summaryLine('linux', 'shot', { kind: 'updated', size: '1×1' })).toContain(
      'candidate written',
    );
    expect(
      summaryLine('linux', 'shot', { kind: 'notTaken', reason: 'the menu |\n did not open' }),
    ).toBe('| shot | linux | — | — | **fail**: not taken: the menu / did not open |');
    expect(
      summaryLine('linux', 'shot', { kind: 'notTaken', reason: 'x'.repeat(400) }).length,
    ).toBeLessThan(360);
  });

  it('writes its head once, then appends', () => {
    const settings = {
      os: 'linux',
      baselines: tempFolder(),
      out: tempFolder(),
      update: false,
    } as const;
    recordOutcome(settings, 'one', { kind: 'noBaseline', size: '1×1' });
    recordOutcome(settings, 'two', { kind: 'compared', result });
    const text = readFileSync(path.join(settings.out, VISUAL_SUMMARY), 'utf8');
    expect(text.match(/### Visual diff/g)).toHaveLength(1);
    expect(text).toContain('| one | linux | 1×1 |');
    expect(text.trimEnd().endsWith('| two | linux | 963×510 | 12 (0.002%) | pass |')).toBe(true);
  });
});

/** A `width`×`height` PNG of one grey, with the first `marked` pixels red. */
function png(width: number, height: number, marked = 0): Buffer {
  const image = new PNG({ width, height });
  for (let index = 0; index < width * height; index += 1) {
    const [r, g, b] = index < marked ? [200, 30, 30] : [240, 240, 240];
    image.data[index * 4] = r;
    image.data[index * 4 + 1] = g;
    image.data[index * 4 + 2] = b;
    image.data[index * 4 + 3] = 255;
  }
  return PNG.sync.write(image);
}

describe('checkShot', () => {
  function settingsWith(baseline: Buffer | null, update = false) {
    const settings = { os: 'linux', baselines: tempFolder(), out: tempFolder(), update } as const;
    if (baseline !== null) {
      mkdirSync(settings.baselines, { recursive: true });
      writeFileSync(path.join(settings.baselines, 'chrome-menu-open.png'), baseline);
    }
    return settings;
  }

  function summary(settings: { out: string }): string {
    return readFileSync(path.join(settings.out, VISUAL_SUMMARY), 'utf8');
  }

  it('passes without a baseline, with a warning, and writes the candidate', () => {
    const settings = settingsWith(null);
    const check = checkShot(settings, 'chrome-menu-open', png(10, 10));
    expect(check.outcome).toEqual({ kind: 'noBaseline', size: '10×10' });
    expect(check.failure).toBeNull();
    expect(check.warning).toContain('No linux baseline for chrome-menu-open yet');
    expect(check.candidate).toBe(path.join(settings.out, 'linux', 'chrome-menu-open.png'));
    expect(existsSync(check.candidate)).toBe(true);
    expect(existsSync(path.join(settings.out, 'linux', 'chrome-menu-open.diff.png'))).toBe(false);
    expect(summary(settings)).toContain('| chrome-menu-open | linux | 10×10 | — | no baseline yet');
  });

  it('only writes the candidate in an update run, even with a baseline', () => {
    const settings = settingsWith(png(10, 10), true);
    const check = checkShot(settings, 'chrome-menu-open', png(12, 10, 50));
    expect(check.outcome.kind).toBe('updated');
    expect(check.failure).toBeNull();
    expect(check.warning).toBeNull();
    expect(existsSync(check.candidate)).toBe(true);
    expect(summary(settings)).toContain('candidate written (update run)');
  });

  it('passes a screenshot like its baseline, and writes the diff image', () => {
    const settings = settingsWith(png(100, 100));
    const check = checkShot(settings, 'chrome-menu-open', png(100, 100, 50));
    expect(check.outcome.kind).toBe('compared');
    expect(check.failure).toBeNull();
    expect(check.warning).toBeNull();
    expect(existsSync(path.join(settings.out, 'linux', 'chrome-menu-open.diff.png'))).toBe(true);
    expect(summary(settings)).toContain(
      '| chrome-menu-open | linux | 100×100 | 50 (0.500%) | pass |',
    );
  });

  it('fails a screenshot that differs from its baseline, naming the candidate', () => {
    const settings = settingsWith(png(100, 100));
    const check = checkShot(settings, 'chrome-menu-open', png(100, 100, 51));
    expect(check.failure).toMatch(/^51 pixels of chrome-menu-open differ from the linux baseline/);
    expect(check.failure).toContain(check.candidate);
    expect(summary(settings)).toContain('**fail**');
  });

  it('fails a screenshot of another size than its baseline', () => {
    const settings = settingsWith(png(100, 100));
    const check = checkShot(settings, 'chrome-menu-open', png(100, 99));
    expect(check.outcome.kind).toBe('sizeMismatch');
    expect(check.failure).toMatch(/100×99 pixels but the baseline is 100×100/);
    expect(summary(settings)).toContain(
      '| chrome-menu-open | linux | — | — | **fail**: The screenshot',
    );
  });
});

describe('takeAndCheckShot', () => {
  function settings() {
    return { os: 'linux', baselines: tempFolder(), out: tempFolder(), update: false } as const;
  }

  function summary(folder: { out: string }): string {
    return readFileSync(path.join(folder.out, VISUAL_SUMMARY), 'utf8');
  }

  it('checks the screenshot taken', async () => {
    const folder = settings();
    const check = await takeAndCheckShot(folder, 'canvas-guessing-game', () =>
      Promise.resolve(png(10, 10)),
    );
    expect(check.outcome).toEqual({ kind: 'noBaseline', size: '10×10' });
    expect(existsSync(check.candidate)).toBe(true);
    expect(summary(folder)).toContain('| canvas-guessing-game | linux | 10×10 |');
  });

  it('records a screenshot the test stopped before as not taken, and throws on', async () => {
    const folder = settings();
    const stop = new Error('The page fonts did not load within 20 s');
    await expect(
      takeAndCheckShot(folder, 'canvas-guessing-game', () => Promise.reject(stop)),
    ).rejects.toBe(stop);
    const lines = summary(folder)
      .split('\n')
      .filter((line) => line.startsWith('| canvas-guessing-game '));
    expect(lines).toEqual([
      '| canvas-guessing-game | linux | — | — | **fail**: not taken: The page fonts did not load within 20 s |',
    ]);
    expect(existsSync(path.join(folder.out, 'linux', 'canvas-guessing-game.png'))).toBe(false);
  });
});
