/**
 * Where the visual diff's baselines and candidates live, and what it reports
 * (docs/spec/09-quality-and-delivery.md §9.2 "Visual diff"):
 *
 * - baselines, committed, one set per system: `apps/desktop/e2e/visual/baselines/<linux|windows>/`;
 * - candidates (this run's screenshots, and a diff image when there is a baseline):
 *   `<artifacts>/visual/<system>/`, which the CI job uploads for a person to review and commit;
 * - a Markdown summary of every screenshot: `<artifacts>/visual/visual-diff.md`.
 *
 * `B2C_VISUAL_UPDATE=1` (the manually started baseline job) only writes candidates and never
 * fails on a difference; a missing baseline is reported and does not fail either, so the first runs
 * on each system make the candidates to commit. {@link checkShot} does all of it for one
 * screenshot; the block canvas and every chrome scene go through it.
 */
import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { harnessSettings } from '../support/env';
import { benchOs, type BenchOs } from '../bench/results';
import {
  compareScreenshots,
  MAX_DIFF_RATIO,
  PIXEL_THRESHOLD,
  pngSize,
  VisualCompareError,
  type VisualResult,
} from './compare';

/** The committed baselines. */
export const BASELINES_DIR = path.join(import.meta.dirname, 'baselines');

/** The summary file in the visual artifacts folder. */
export const VISUAL_SUMMARY = 'visual-diff.md';

/** A screenshot name: lower-case words joined by dashes (it becomes a file name). */
const SHOT_NAME = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/** The visual diff's settings, from the environment. */
export interface VisualSettings {
  /** The system whose baselines apply. */
  readonly os: BenchOs;
  /** The baselines of this system. */
  readonly baselines: string;
  /** Where this run's candidates, diffs and summary go. */
  readonly out: string;
  /** Whether this run only makes candidates (`B2C_VISUAL_UPDATE=1`). */
  readonly update: boolean;
}

/** The visual diff's settings for `platform` from `env`. */
export function visualSettings(
  env: NodeJS.ProcessEnv = process.env,
  platform: NodeJS.Platform = process.platform,
): VisualSettings {
  const os = benchOs(platform);
  const out = path.join(harnessSettings(env, platform).artifacts, 'visual');
  return {
    os,
    baselines: path.join(BASELINES_DIR, os),
    out,
    update: env['B2C_VISUAL_UPDATE'] === '1',
  };
}

/** Checks a screenshot name. */
export function shotName(name: string): string {
  if (name.length > 64 || !SHOT_NAME.test(name)) {
    throw new Error(`${name} is not a screenshot name (lower-case words joined by dashes)`);
  }
  return name;
}

/** The committed baseline of screenshot `name`, or `null` when there is none yet. */
export function readBaseline(settings: VisualSettings, name: string): Buffer | null {
  const file = path.join(settings.baselines, `${shotName(name)}.png`);
  return existsSync(file) ? readFileSync(file) : null;
}

/**
 * Writes this run's screenshot (and the diff image, when there is one) as candidates:
 * `<out>/<os>/<name>.png` (named as the baseline it would replace) and `<name>.diff.png`.
 */
export function writeCandidate(
  settings: VisualSettings,
  name: string,
  screenshot: Buffer,
  diff: Buffer | null,
): string {
  const dir = path.join(settings.out, settings.os);
  mkdirSync(dir, { recursive: true });
  const file = path.join(dir, `${shotName(name)}.png`);
  writeFileSync(file, screenshot);
  if (diff !== null) {
    writeFileSync(path.join(dir, `${name}.diff.png`), diff);
  }
  return file;
}

/** What one comparison came to. */
export type VisualOutcome =
  | { readonly kind: 'compared'; readonly result: VisualResult }
  | { readonly kind: 'noBaseline'; readonly size: string }
  | { readonly kind: 'sizeMismatch'; readonly message: string }
  | { readonly kind: 'updated'; readonly size: string }
  /** The screenshot was never taken: the test failed first, or the scene could not be made. */
  | { readonly kind: 'notTaken'; readonly reason: string };

/** Text for one cell of a Markdown table: one line, no `|`, at most 300 characters. */
function cell(text: string): string {
  const flat = text.replace(/\s+/g, ' ').replace(/\|/g, '/').trim();
  return flat.length > 300 ? `${flat.slice(0, 299)}…` : flat;
}

/** One line of the summary table. */
export function summaryLine(os: BenchOs, name: string, outcome: VisualOutcome): string {
  switch (outcome.kind) {
    case 'compared': {
      const { result } = outcome;
      const share = `${(result.ratio * 100).toFixed(3)}%`;
      const verdict = result.passed ? 'pass' : '**fail**';
      return `| ${name} | ${os} | ${String(result.width)}×${String(result.height)} | ${String(result.diffPixels)} (${share}) | ${verdict} |`;
    }
    case 'noBaseline':
      return `| ${name} | ${os} | ${outcome.size} | — | no baseline yet: commit the candidate |`;
    case 'sizeMismatch':
      return `| ${name} | ${os} | — | — | **fail**: ${cell(outcome.message)} |`;
    case 'updated':
      return `| ${name} | ${os} | ${outcome.size} | — | candidate written (update run) |`;
    case 'notTaken':
      return `| ${name} | ${os} | — | — | **fail**: not taken: ${cell(outcome.reason)} |`;
  }
}

/** Appends one comparison to the summary (the table's head is written with the first line). */
export function recordOutcome(
  settings: VisualSettings,
  name: string,
  outcome: VisualOutcome,
): void {
  mkdirSync(settings.out, { recursive: true });
  const file = path.join(settings.out, VISUAL_SUMMARY);
  if (!existsSync(file)) {
    const head = [
      '### Visual diff of the block canvas and the chrome (docs/spec/09-quality-and-delivery.md §9.2)',
      '',
      `pixelmatch threshold ${String(PIXEL_THRESHOLD)}; more than ${String(MAX_DIFF_RATIO * 100)}% of differing pixels fails. ` +
        'Candidates are in the `visual-candidates-*` artifact; baselines in `apps/desktop/e2e/visual/baselines/`.',
      '',
      '| Screenshot | System | Size | Differing pixels | Result |',
      '| --- | --- | --- | ---: | --- |',
      '',
    ].join('\n');
    writeFileSync(file, head);
  }
  appendFileSync(file, `${summaryLine(settings.os, name, outcome)}\n`);
}

/** What {@link checkShot} found for one screenshot. */
export interface ShotCheck {
  readonly outcome: VisualOutcome;
  /** The candidate written for it. */
  readonly candidate: string;
  /** Why the screenshot fails, or `null` when it passes. */
  readonly failure: string | null;
  /** A warning to show when it passes without a comparison (no baseline yet), or `null`. */
  readonly warning: string | null;
}

/**
 * Compares screenshot `name` (a PNG) with this system's baseline, writes it (and the diff image)
 * as a candidate and appends its line to the summary. An update run compares nothing, and a
 * missing baseline passes with a warning; a screenshot of another size than its baseline, or with
 * more than {@link MAX_DIFF_RATIO} of its pixels differing, fails.
 */
export function checkShot(settings: VisualSettings, name: string, screenshot: Buffer): ShotCheck {
  let outcome: VisualOutcome;
  if (settings.update) {
    outcome = { kind: 'updated', size: pngSize(screenshot) };
  } else {
    const baseline = readBaseline(settings, name);
    if (baseline === null) {
      outcome = { kind: 'noBaseline', size: pngSize(screenshot) };
    } else {
      try {
        outcome = { kind: 'compared', result: compareScreenshots(baseline, screenshot) };
      } catch (error: unknown) {
        if (!(error instanceof VisualCompareError)) {
          throw error;
        }
        outcome = { kind: 'sizeMismatch', message: error.message };
      }
    }
  }
  const candidate = writeCandidate(
    settings,
    name,
    screenshot,
    outcome.kind === 'compared' ? outcome.result.diffPng : null,
  );
  recordOutcome(settings, name, outcome);
  let failure: string | null = null;
  let warning: string | null = null;
  if (outcome.kind === 'noBaseline') {
    // Not a failure: the first run on a system makes the baseline to review and commit.
    warning = `No ${settings.os} baseline for ${name} yet; the candidate is ${candidate}`;
  } else if (outcome.kind === 'sizeMismatch') {
    failure = `${outcome.message}. The candidate is ${candidate}`;
  } else if (outcome.kind === 'compared' && !outcome.result.passed) {
    const { result } = outcome;
    failure =
      `${String(result.diffPixels)} pixels of ${name} differ from the ${settings.os} baseline ` +
      `(${(result.ratio * 100).toFixed(3)}%, more than ${String(MAX_DIFF_RATIO * 100)}%); ` +
      `see ${candidate} and its .diff.png`;
  }
  return { outcome, candidate, failure, warning };
}
