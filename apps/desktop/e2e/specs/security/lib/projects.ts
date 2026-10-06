/**
 * Project files for the security tests: the malicious-project suite (tests/security/projects, whose
 * README table is the expected outcome of every file), this suite's own fixtures
 * (e2e/fixtures/security), copies of them in a folder of the test's own, and the machine-local
 * trust store of the app's test profile.
 */
import {
  copyFileSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import type { TestContext } from 'vitest';

import { REPOSITORY_ROOT } from '../../../support/env';

/** The malicious-project regression suite (docs/spec/08-security.md §8.12). */
export const SECURITY_PROJECTS = path.join(REPOSITORY_ROOT, 'tests', 'security', 'projects');

/** This suite's own project files. */
export const SECURITY_FIXTURES = path.join(
  REPOSITORY_ROOT,
  'apps',
  'desktop',
  'e2e',
  'fixtures',
  'security',
);

/** What the loader does with a file of the suite: it accepts it, or rejects it with these codes. */
export type LoaderOutcome = 'accepted' | readonly string[];

/** One row of the suite's table. */
export interface SuiteCase {
  /** The file name (`proto-key.b2c`). */
  readonly file: string;
  readonly loader: LoaderOutcome;
}

/** The suite's table could not be read, or does not match the folder. */
export class SuiteTableError extends Error {
  override readonly name = 'SuiteTableError';
}

/** The cells of a Markdown table row (`| a | b |`), trimmed; `\|` stays inside its cell. */
export function tableCells(line: string): string[] {
  const inner = line.trim().replace(/^\|/, '').replace(/\|$/, '');
  return inner.split(/(?<!\\)\|/).map((cell) => cell.trim());
}

/**
 * The rows of the suite README's *Cases* table: file (column 1) and loader outcome (column 4,
 * `accepted` or a comma-separated list of `` `B2C-E0nnn` `` codes).
 *
 * @throws SuiteTableError for a row whose loader cell is neither.
 */
export function parseSuiteTable(markdown: string): SuiteCase[] {
  const cases: SuiteCase[] = [];
  for (const line of markdown.split(/\r?\n/)) {
    const cells = tableCells(line);
    const file = /^`([A-Za-z0-9_.-]+\.b2c)`$/.exec(cells[0] ?? '')?.[1];
    if (!line.trim().startsWith('|') || file === undefined) {
      continue;
    }
    const loader = cells[3] ?? '';
    if (loader === 'accepted') {
      cases.push({ file, loader: 'accepted' });
      continue;
    }
    const codes = [...loader.matchAll(/`(B2C-E\d{4})`/g)].map((match) => match[1] ?? '');
    const rest = loader.replace(/`B2C-E\d{4}`/g, '').replace(/[\s,]/g, '');
    if (codes.length === 0 || rest !== '') {
      throw new SuiteTableError(
        `The loader outcome of ${file} is neither accepted nor codes: ${loader}`,
      );
    }
    cases.push({ file, loader: codes });
  }
  return cases;
}

/**
 * The suite's cases, checked against its folder: every `.b2c` file has exactly one row and every
 * row a file (as the Rust suite tests check), so a new attack file is never skipped here.
 *
 * @throws SuiteTableError when they differ.
 */
export function readSuite(folder = SECURITY_PROJECTS): SuiteCase[] {
  const cases = parseSuiteTable(readFileSync(path.join(folder, 'README.md'), 'utf8'));
  const rows = cases.map((entry) => entry.file);
  const files = readdirSync(folder)
    .filter((name) => name.endsWith('.b2c'))
    .sort();
  const duplicates = rows.filter((file, index) => rows.indexOf(file) !== index);
  const missing = files.filter((file) => !rows.includes(file));
  const extra = rows.filter((file) => !files.includes(file));
  if (duplicates.length > 0 || missing.length > 0 || extra.length > 0) {
    throw new SuiteTableError(
      `The suite's table and folder differ: duplicate rows ${duplicates.join(', ') || 'none'}; files without a row ${missing.join(', ') || 'none'}; rows without a file ${extra.join(', ') || 'none'}`,
    );
  }
  return [...cases].sort((a, b) => a.file.localeCompare(b.file));
}

/**
 * A new empty folder of the test's own, deleted when the test finishes. Project files are copied
 * here before they are opened, so the app never watches or writes a folder of the repository.
 */
export function scratchFolder(context: TestContext, prefix = 'b2c-e2e-security-'): string {
  const folder = mkdtempSync(path.join(tmpdir(), prefix));
  context.onTestFinished(async () => {
    // On Windows the app (its file watcher) may hold the folder for a moment after it closed.
    for (let attempt = 0; attempt < 10; attempt += 1) {
      try {
        rmSync(folder, { recursive: true, force: true });
        return;
      } catch {
        await new Promise((resolve) => setTimeout(resolve, 500));
      }
    }
    process.stderr.write(`Could not remove the test folder ${folder}\n`);
  });
  return folder;
}

/** Copies `source` into `folder` (as `name`, default its own name) and returns the copy's path. */
export function copyInto(folder: string, source: string, name = path.basename(source)): string {
  const target = path.join(folder, name);
  copyFileSync(source, target);
  return target;
}

/** One of this suite's fixtures (e2e/fixtures/security/`name`). */
export function fixture(name: string): string {
  return path.join(SECURITY_FIXTURES, name);
}

/** Reads a JSON file. */
export function readJson(file: string): unknown {
  return JSON.parse(readFileSync(file, 'utf8')) as unknown;
}

/**
 * Changes a JSON file as an editor outside the app would: reads it, lets `edit` change the value
 * (or return a new one), and writes it back with two-space indentation.
 */
export function editJson(file: string, edit: (value: Record<string, unknown>) => unknown): void {
  const value = readJson(file);
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new Error(`${file} does not hold a JSON object`);
  }
  const record = value as Record<string, unknown>;
  const result = edit(record);
  writeFileSync(file, `${JSON.stringify(result === undefined ? record : result, null, 2)}\n`);
}

/** The value at `keys` inside a JSON object, which must be an object there. */
export function objectAt(
  value: Record<string, unknown>,
  ...keys: string[]
): Record<string, unknown> {
  let current: unknown = value;
  for (const key of keys) {
    if (typeof current !== 'object' || current === null || Array.isArray(current)) {
      throw new Error(`No object at ${keys.join('.')}`);
    }
    current = (current as Record<string, unknown>)[key];
  }
  if (typeof current !== 'object' || current === null || Array.isArray(current)) {
    throw new Error(`No object at ${keys.join('.')}`);
  }
  return current as Record<string, unknown>;
}

/** The test profile of an app (the harness's `B2C_E2E_ROOT`, support/app.ts). */
export function profileOf(app: { readonly root: string }): string {
  return path.join(app.root, 'profile');
}

/** The trust store of a test profile (`Dirs::under_root`: `<profile>/machine/trust.json`). */
export function trustStore(profile: string): string {
  return path.join(profile, 'machine', 'trust.json');
}

/** The app's current log file in a test profile (`<profile>/state/logs/blocks2cpp.log`). */
export function appLog(profile: string): string {
  return path.join(profile, 'state', 'logs', 'blocks2cpp.log');
}
