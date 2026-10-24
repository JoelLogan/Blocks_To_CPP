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
  benchOffset,
  benchOs,
  benchSamples,
  benchOutDir,
  BenchResultsError,
  checkMetric,
  MAX_OFFSET,
  metricValue,
  MIN_SAMPLES,
  RESULTS_FORMAT,
  RESULTS_FORMAT_VERSION,
  resultsJson,
  writeMetric,
} from './results';
import { median, percentile } from './stats';

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

/**
 * Ten rounds of 10 edits, as the preview benchmark makes them: about 1,000 ms each, and in five of
 * the rounds one slow edit (3,000 ms), 5 of the run's 100 edits.
 */
const editRounds: number[][] = Array.from({ length: 10 }, (_, round) =>
  Array.from({ length: 10 }, (_, edit) => (round % 2 === 0 && edit === 3 ? 3_000 : 1_000 + edit)),
);

/** The edits' metric pooled over the run (D19): every edit is a sample, valued at their p95. */
const pooled: BenchMetric = {
  name: 'webview.edit-1000.p95-all',
  unit: 'ms',
  better: 'lower',
  gate: true,
  statistic: 'p95',
  description: 'Edits, pooled',
  samples: editRounds.flat(),
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

describe('benchSamples', () => {
  it('is B2C_BENCH_SAMPLES, a whole number from 1 to 1,000, or 10', () => {
    expect(benchSamples({})).toBe(MIN_SAMPLES);
    expect(benchSamples({ B2C_BENCH_SAMPLES: '' })).toBe(MIN_SAMPLES);
    expect(benchSamples({ B2C_BENCH_SAMPLES: '5' })).toBe(5);
    expect(benchSamples({ B2C_BENCH_SAMPLES: '1000' })).toBe(1_000);
    for (const bad of ['0', '1001', '2.5', '-3', 'ten', ' 5']) {
      expect(() => benchSamples({ B2C_BENCH_SAMPLES: bad }), bad).toThrow(BenchResultsError);
    }
  });
});

describe('benchOffset', () => {
  it('is B2C_BENCH_OFFSET, a whole number from 0 to 100,000, or 0', () => {
    expect(benchOffset({})).toBe(0);
    expect(benchOffset({ B2C_BENCH_OFFSET: '' })).toBe(0);
    expect(benchOffset({ B2C_BENCH_OFFSET: '0' })).toBe(0);
    expect(benchOffset({ B2C_BENCH_OFFSET: '5' })).toBe(5);
    expect(benchOffset({ B2C_BENCH_OFFSET: String(MAX_OFFSET) })).toBe(MAX_OFFSET);
    for (const bad of ['100001', '2.5', '-3', 'five', ' 5', '1e3']) {
      expect(() => benchOffset({ B2C_BENCH_OFFSET: bad }), bad).toThrow(BenchResultsError);
    }
  });
});

describe('checkMetric', () => {
  it('accepts a good metric', () => {
    expect(() => {
      checkMetric(metric);
    }).not.toThrow();
  });

  it('refuses a statistic the comparison does not know', () => {
    expect(() => {
      checkMetric({ ...metric, statistic: 'p50' as unknown as 'p95' });
    }).toThrow(/statistic must be one of median, p95/);
    expect(() => {
      checkMetric(pooled);
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

describe('metricValue', () => {
  it('is the median of the samples, as the comparison takes it by default', () => {
    expect(metricValue(metric)).toBe(1004.5);
    expect(metricValue({ ...metric, statistic: 'median' })).toBe(1004.5);
  });

  it('is the p95 of every sample for a pooled metric: five slow edits of 100 do not decide it', () => {
    // The 95th smallest of the 100 edits: the slowest edit of the rounds without a slow one.
    expect(metricValue(pooled)).toBe(1_009);
    expect(metricValue(pooled)).toBe(percentile(editRounds.flat(), 95));
    // Each round's p95 is its slowest edit, so half the rounds' samples are slow ones and
    // their median moves halfway to them.
    const perRound = editRounds.map((values) => percentile(values, 95));
    expect(perRound.filter((value) => value === 3_000)).toHaveLength(5);
    expect(median(perRound)).toBe(2_004.5);
    // A sixth slow edit does decide it.
    const six = [...pooled.samples.slice(1), 3_000];
    expect(metricValue({ ...pooled, samples: six })).toBe(3_000);
  });
});

describe('resultsJson and writeMetric', () => {
  it('write the comparison script’s format, rounding the samples', () => {
    const value = JSON.parse(resultsJson(metric, 'linux')) as Record<string, unknown>;
    expect(Object.keys(value).sort()).toEqual(['format', 'formatVersion', 'metrics', 'os']);
    expect(value['format']).toBe(RESULTS_FORMAT);
    expect(value['formatVersion']).toBe(RESULTS_FORMAT_VERSION);
    expect(RESULTS_FORMAT_VERSION).toBe(2);
    expect(value['os']).toBe('linux');
    const [written] = value['metrics'] as Record<string, unknown>[];
    expect(Object.keys(written ?? {}).sort()).toEqual([
      'better',
      'description',
      'gate',
      'name',
      'samples',
      'statistic',
      'unit',
    ]);
    expect(written?.['statistic']).toBe('median');
    expect((written?.['samples'] as number[])[0]).toBe(1000.123);
  });

  it('write a pooled metric’s statistic and every one of its samples', () => {
    const value = JSON.parse(resultsJson(pooled, 'linux')) as {
      metrics: Record<string, unknown>[];
    };
    expect(value.metrics[0]?.['statistic']).toBe('p95');
    expect(value.metrics[0]?.['samples']).toHaveLength(100);
  });

  it('writes <name>.json into the folder', () => {
    const folder = tempFolder();
    const file = writeMetric(metric, folder, 'windows', {});
    expect(file).toBe(path.join(folder, 'webview.cold-start.json'));
    expect(JSON.parse(readFileSync(file, 'utf8'))).toMatchObject({ os: 'windows' });
  });

  it('writes as few samples as a run takes (an A/B run), but no fewer', () => {
    const folder = tempFolder();
    const five = { ...metric, samples: metric.samples.slice(0, 5) };
    expect(() => writeMetric(five, folder, 'linux', {})).toThrow(BenchResultsError);
    expect(writeMetric(five, folder, 'linux', { B2C_BENCH_SAMPLES: '5' })).toBe(
      path.join(folder, 'webview.cold-start.json'),
    );
    const four = { ...metric, samples: metric.samples.slice(0, 4) };
    expect(() => writeMetric(four, folder, 'linux', { B2C_BENCH_SAMPLES: '5' })).toThrow(
      BenchResultsError,
    );
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

  it.skipIf(!python)(
    'produces pooled metrics that tools/bench-compare.py values at their p95',
    () => {
      const folder = tempFolder();
      const file = writeMetric(pooled, folder, 'linux');
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
      expect(run.stdout).toContain('| `webview.edit-1000.p95-all` | yes | 100 | 1009.0 ms (p95) |');
      const kept = JSON.parse(readFileSync(history, 'utf8')) as {
        runs: { medians: Record<string, { value: number }> }[];
      };
      expect(kept.runs[0]?.medians['webview.edit-1000.p95-all']?.value).toBe(metricValue(pooled));
    },
  );
});
