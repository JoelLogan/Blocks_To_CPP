/** The visual diff's settings, candidates and summary (baselines.ts). */
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import {
  BASELINES_DIR,
  readBaseline,
  recordOutcome,
  shotName,
  summaryLine,
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
