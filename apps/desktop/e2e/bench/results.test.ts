/**
 * The webview benchmarks' results files (results.ts), including a round trip through the
 * comparison script that reads them (tools/bench-compare.py), when Python is installed.
 */
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { REPOSITORY_ROOT } from '../support/env';
import {
  type BenchMetric,
  benchOs,
  benchOutDir,
  BenchResultsError,
  checkMetric,
  MIN_SAMPLES,
  RESULTS_FORMAT,
  resultsJson,
  writeMetric,
} from './results';

const folders: string[] = [];

afterEach(() => {
  for (const folder of folders.splice(0)) {
    rmSync(folder, { recursive: true, force: true });
  }
});

function tempFolder(): string {
  const folder = mkdtempSync(path.join(tmpdir(), 'b2c-bench-results-'));
  folders.push(folder);
  return folder;
}

const metric: BenchMetric = {
  name: 'webview.cold-start',
  unit: 'ms',
  better: 'lower',
  gate: true,
  description: 'A test metric',
  samples: [1000.1234, 1001, 1002, 1003, 1004, 1005, 1006, 1007, 1008, 1009],
};

/** Whether `python3` can be run here. */
const python = spawnSync('python3', ['--version'], { encoding: 'utf8' }).status === 0;

describe('benchOs', () => {
  it('names the systems as the comparison does', () => {
    expect(benchOs('linux')).toBe('linux');
    expect(benchOs('win32')).toBe('windows');
    expect(benchOs('darwin')).toBe('macos');
    expect(() => benchOs('aix')).toThrow(BenchResultsError);
  });
});

describe('benchOutDir', () => {
  it('is B2C_BENCH_OUT when set (absolute only), else bench/ in the artifacts', () => {
    const out = path.resolve(tmpdir(), 'bench-out');
    expect(benchOutDir({ B2C_BENCH_OUT: out })).toBe(out);
    expect(() => benchOutDir({ B2C_BENCH_OUT: 'relative' })).toThrow(/absolute/);
    const artifacts = path.resolve(tmpdir(), 'artifacts');
    expect(benchOutDir({ B2C_E2E_ARTIFACTS: artifacts, B2C_E2E_TOOLCHAIN_DIRS: artifacts })).toBe(
      path.join(artifacts, 'bench'),
    );
  });
});

describe('checkMetric', () => {
  it('accepts a good metric', () => {
    expect(() => {
      checkMetric(metric);
    }).not.toThrow();
  });

  it('refuses bad names, descriptions and samples', () => {
    const bad: Partial<BenchMetric>[] = [
      { name: 'Webview.Cold' },
      { name: 'webview..cold' },
      { name: `w${'x'.repeat(64)}` },
      { description: 'two\nlines' },
      { description: 'x'.repeat(201) },
      { samples: metric.samples.slice(0, MIN_SAMPLES - 1) },
      { samples: [...metric.samples.slice(1), Number.NaN] },
      { samples: [...metric.samples.slice(1), -1] },
    ];
    for (const change of bad) {
      expect(() => {
        checkMetric({ ...metric, ...change });
      }, JSON.stringify(change)).toThrow(BenchResultsError);
    }
  });
});

describe('resultsJson and writeMetric', () => {
  it('write the comparison script’s format, rounding the samples', () => {
    const value = JSON.parse(resultsJson(metric, 'linux')) as Record<string, unknown>;
    expect(Object.keys(value).sort()).toEqual(['format', 'formatVersion', 'metrics', 'os']);
    expect(value['format']).toBe(RESULTS_FORMAT);
    expect(value['formatVersion']).toBe(1);
    expect(value['os']).toBe('linux');
    const [written] = value['metrics'] as Record<string, unknown>[];
    expect(Object.keys(written ?? {}).sort()).toEqual([
      'better',
      'description',
      'gate',
      'name',
      'samples',
      'unit',
    ]);
    expect((written?.['samples'] as number[])[0]).toBe(1000.123);
  });

  it('writes <name>.json into the folder', () => {
    const folder = tempFolder();
    const file = writeMetric(metric, folder, 'windows');
    expect(file).toBe(path.join(folder, 'webview.cold-start.json'));
    expect(JSON.parse(readFileSync(file, 'utf8'))).toMatchObject({ os: 'windows' });
  });

  it.skipIf(!python)('produces files that tools/bench-compare.py accepts', () => {
    const folder = tempFolder();
    const file = writeMetric(metric, folder, 'linux');
    const history = path.join(folder, 'history.json');
    const run = spawnSync(
      'python3',
      [
        path.join(REPOSITORY_ROOT, 'tools', 'bench-compare.py'),
        'compare',
        '--os',
        'linux',
        '--results',
        file,
        '--history',
        history,
        '--update-history',
        history,
        '--run-id',
        '1',
        '--commit',
        'abc',
      ],
      { encoding: 'utf8' },
    );
    expect(run.stderr).toBe('');
    expect(run.status).toBe(0);
    expect(run.stdout).toContain('`webview.cold-start`');
    expect(run.stdout).toContain('new: no baseline yet');
    const kept = JSON.parse(readFileSync(history, 'utf8')) as {
      runs: { medians: Record<string, { value: number }> }[];
    };
    expect(kept.runs[0]?.medians['webview.cold-start']?.value).toBe(1004.5);
  });
});
