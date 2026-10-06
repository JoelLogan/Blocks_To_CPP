/**
 * The webview benchmarks' results files, which tools/bench-compare.py compares with the baseline
 * (docs/spec/09-quality-and-delivery.md §9.2): one `blocks2cpp/bench-results` file per metric,
 * `<out>/<metric>.json`, where `<out>` is `B2C_BENCH_OUT` (absolute) or `bench/` in the harness's
 * artifacts folder.
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { harnessSettings } from '../support/env';
import { rounded } from './stats';

/** The results format tag (tools/bench-compare.py RESULTS_FORMAT). */
export const RESULTS_FORMAT = 'blocks2cpp/bench-results';

/** The fewest samples a metric may have: the comparison refuses fewer. */
export const MIN_SAMPLES = 10;

/** The most samples a metric may have (tools/bench-compare.py MAX_SAMPLES). */
export const MAX_SAMPLES = 100_000;

/** A metric name: lower-case words joined by `.` or `-` (tools/bench-compare.py METRIC_NAME). */
const METRIC_NAME = /^[a-z][a-z0-9]*(?:[.-][a-z0-9]+)*$/;

/** The system names of the results files. */
export type BenchOs = 'linux' | 'windows' | 'macos';

/** One metric of a run. */
export interface BenchMetric {
  /** For example `webview.cold-start`. */
  readonly name: string;
  readonly unit: 'ms';
  /** Every webview metric is a time: lower is better. */
  readonly better: 'lower';
  /** Whether a regression fails the nightly job (otherwise it is only reported). */
  readonly gate: boolean;
  /** One line saying what is measured. */
  readonly description: string;
  readonly samples: readonly number[];
}

/** A metric that cannot be written. */
export class BenchResultsError extends Error {
  override readonly name = 'BenchResultsError';
}

/** The results files' name for a Node.js platform. */
export function benchOs(platform: NodeJS.Platform = process.platform): BenchOs {
  switch (platform) {
    case 'linux':
      return 'linux';
    case 'win32':
      return 'windows';
    case 'darwin':
      return 'macos';
    default:
      throw new BenchResultsError(`No benchmark baselines are kept for ${platform}`);
  }
}

/** Where the results files go: `B2C_BENCH_OUT`, or `bench/` in the harness's artifacts folder. */
export function benchOutDir(env: NodeJS.ProcessEnv = process.env): string {
  const out = env['B2C_BENCH_OUT'];
  if (out !== undefined && out !== '') {
    if (!path.isAbsolute(out)) {
      throw new BenchResultsError('B2C_BENCH_OUT must be an absolute path');
    }
    return out;
  }
  return path.join(harnessSettings(env).artifacts, 'bench');
}

/** Checks a metric before it is written; throws {@link BenchResultsError}. */
export function checkMetric(metric: BenchMetric): void {
  if (metric.name.length > 64 || !METRIC_NAME.test(metric.name)) {
    throw new BenchResultsError(`${metric.name} is not a metric name`);
  }
  let controls = false;
  for (let index = 0; index < metric.description.length; index += 1) {
    controls ||= metric.description.charCodeAt(index) < 0x20;
  }
  if (metric.description.length > 200 || controls) {
    throw new BenchResultsError(`${metric.name}: the description must be one short line`);
  }
  if (metric.samples.length < MIN_SAMPLES || metric.samples.length > MAX_SAMPLES) {
    throw new BenchResultsError(
      `${metric.name} has ${String(metric.samples.length)} samples; ${String(MIN_SAMPLES)} to ${String(MAX_SAMPLES)} are needed`,
    );
  }
  if (!metric.samples.every((sample) => Number.isFinite(sample) && sample >= 0)) {
    throw new BenchResultsError(`${metric.name}: every sample must be a finite time of 0 or more`);
  }
}

/** The results file of one metric, as tools/bench-compare.py reads it. */
export function resultsJson(metric: BenchMetric, os: BenchOs): string {
  checkMetric(metric);
  const file = {
    format: RESULTS_FORMAT,
    formatVersion: 1,
    os,
    metrics: [{ ...metric, samples: metric.samples.map((sample) => rounded(sample)) }],
  };
  return `${JSON.stringify(file, null, 2)}\n`;
}

/** Writes the metric's results file into `dir` and returns its path. */
export function writeMetric(
  metric: BenchMetric,
  dir: string = benchOutDir(),
  os: BenchOs = benchOs(),
): string {
  const text = resultsJson(metric, os);
  mkdirSync(dir, { recursive: true });
  const file = path.join(dir, `${metric.name}.json`);
  writeFileSync(file, text);
  return file;
}
