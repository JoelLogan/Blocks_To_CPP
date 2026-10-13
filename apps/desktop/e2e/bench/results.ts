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

/**
 * The fewest samples a metric may have in a run of its own: the comparison refuses fewer. The A/B
 * job's runs take fewer each ({@link benchSamples}) and the comparison adds them up.
 */
export const MIN_SAMPLES = 10;

/** The most samples {@link benchSamples} allows. */
export const MAX_RUN_SAMPLES = 1_000;

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

/**
 * How many samples each benchmark takes: `B2C_BENCH_SAMPLES` (a whole number from 1 to
 * {@link MAX_RUN_SAMPLES}), by default {@link MIN_SAMPLES}. The A/B job sets it lower and
 * alternates several runs of the two builds, whose samples the comparison then adds up
 * (tools/bench-compare.py `ab`).
 */
export function benchSamples(env: NodeJS.ProcessEnv = process.env): number {
  const value = env['B2C_BENCH_SAMPLES'];
  if (value === undefined || value === '') {
    return MIN_SAMPLES;
  }
  const count = Number(value);
  if (!/^[0-9]+$/.test(value) || count < 1 || count > MAX_RUN_SAMPLES) {
    throw new BenchResultsError(
      `B2C_BENCH_SAMPLES must be a whole number from 1 to ${String(MAX_RUN_SAMPLES)}`,
    );
  }
  return count;
}

/** The largest offset {@link benchOffset} allows. */
export const MAX_OFFSET = 100_000;

/**
 * Where in its list of targets a benchmark that goes round one starts: `B2C_BENCH_OFFSET` (a whole
 * number from 0 to {@link MAX_OFFSET}), by default 0. The A/B job's runs take fewer samples each
 * ({@link benchSamples}), so each of its rounds starts where the previous one stopped
 * (e2e/bench/ab-rounds.sh): its runs together click the targets a nightly run does.
 */
export function benchOffset(env: NodeJS.ProcessEnv = process.env): number {
  const value = env['B2C_BENCH_OFFSET'];
  if (value === undefined || value === '') {
    return 0;
  }
  const offset = Number(value);
  if (!/^[0-9]+$/.test(value) || offset > MAX_OFFSET) {
    throw new BenchResultsError(
      `B2C_BENCH_OFFSET must be a whole number from 0 to ${String(MAX_OFFSET)}`,
    );
  }
  return offset;
}

/**
 * Checks a metric before it is written; throws {@link BenchResultsError}. It needs at least
 * `minSamples` samples ({@link MIN_SAMPLES} unless a run takes fewer, see {@link benchSamples}).
 */
export function checkMetric(metric: BenchMetric, minSamples: number = MIN_SAMPLES): void {
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
  if (metric.samples.length < minSamples || metric.samples.length > MAX_SAMPLES) {
    throw new BenchResultsError(
      `${metric.name} has ${String(metric.samples.length)} samples; ${String(minSamples)} to ${String(MAX_SAMPLES)} are needed`,
    );
  }
  if (!metric.samples.every((sample) => Number.isFinite(sample) && sample >= 0)) {
    throw new BenchResultsError(`${metric.name}: every sample must be a finite time of 0 or more`);
  }
}

/** The results file of one metric, as tools/bench-compare.py reads it. */
export function resultsJson(
  metric: BenchMetric,
  os: BenchOs,
  minSamples: number = MIN_SAMPLES,
): string {
  checkMetric(metric, minSamples);
  const file = {
    format: RESULTS_FORMAT,
    formatVersion: 1,
    os,
    metrics: [{ ...metric, samples: metric.samples.map((sample) => rounded(sample)) }],
  };
  return `${JSON.stringify(file, null, 2)}\n`;
}

/**
 * Writes the metric's results file into `dir` and returns its path. A run that takes fewer than
 * {@link MIN_SAMPLES} samples ({@link benchSamples}) may write that many.
 */
export function writeMetric(
  metric: BenchMetric,
  dir: string = benchOutDir(),
  os: BenchOs = benchOs(),
  env: NodeJS.ProcessEnv = process.env,
): string {
  const text = resultsJson(metric, os, Math.min(MIN_SAMPLES, benchSamples(env)));
  mkdirSync(dir, { recursive: true });
  const file = path.join(dir, `${metric.name}.json`);
  writeFileSync(file, text);
  return file;
}
