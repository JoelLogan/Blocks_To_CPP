/**
 * Where the visual diff's baselines and candidates live, and what it reports
 * (docs/spec/09-quality-and-delivery.md §9.2 "Visual diff"):
 *
 * - baselines, committed, one set per system: `apps/desktop/e2e/visual/baselines/<linux|windows>/`;
 * - candidates (this run's screenshots, and a diff image when there is a baseline):
 *   `<artifacts>/visual/<system>/`, which the CI job uploads for a person to review and commit;
 * - a Markdown summary of every comparison: `<artifacts>/visual/visual-diff.md`.
 *
 * `B2C_VISUAL_UPDATE=1` (the manually started baseline job) only writes candidates and never
 * fails on a difference; a missing baseline is reported and does not fail either, so the first runs
 * on each system make the candidates to commit.
 */
import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { harnessSettings } from '../support/env';
import { benchOs, type BenchOs } from '../bench/results';
import { MAX_DIFF_RATIO, PIXEL_THRESHOLD, type VisualResult } from './compare';

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
  | { readonly kind: 'updated'; readonly size: string };

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
      return `| ${name} | ${os} | — | — | **fail**: ${outcome.message.replace(/\|/g, '/')} |`;
    case 'updated':
      return `| ${name} | ${os} | ${outcome.size} | — | candidate written (update run) |`;
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
      '### Visual diff of the block canvas (docs/spec/09-quality-and-delivery.md §9.2)',
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
