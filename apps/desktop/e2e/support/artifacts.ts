/**
 * What the tests leave behind: on failure a screenshot, the page, the console transcript, the
 * project and the logs of the app and the driver; after every test the Trusted Types trial's
 * counts (docs/spec/08-security.md §8.8), which the Windows CI job writes to its summary.
 */
import {
  appendFileSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import path from 'node:path';

/** The file the Trusted Types counts are appended to, one JSON object per line. */
export const TRUSTED_TYPES_REPORT = 'trusted-types.jsonl';

/** The Markdown summary of {@link TRUSTED_TYPES_REPORT}, for the CI job's summary. */
export const TRUSTED_TYPES_SUMMARY = 'trusted-types.md';

/** One test's line in {@link TRUSTED_TYPES_REPORT}. */
export interface TrustedTypesEntry {
  readonly test: string;
  readonly platform: NodeJS.Platform;
  /** The violations during the test (before the probe). */
  readonly count: number;
  readonly directives: readonly string[];
  /**
   * Whether the probe's deliberate HTML-sink call was reported: the report-only Trusted Types
   * policy reached the page. `null` when the probe could not run.
   */
  readonly policyActive: boolean | null;
}

/** A folder name for a test: letters, digits and dashes. */
export function artifactName(test: string): string {
  const name = test
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
    .slice(0, 80);
  return name === '' ? 'test' : name;
}

/** The folder for a test's artifacts (created). */
export function artifactDir(root: string, test: string): string {
  const dir = path.join(root, artifactName(test));
  mkdirSync(dir, { recursive: true });
  return dir;
}

/** Writes one artifact file; a failure is reported on stderr and otherwise ignored. */
export function writeArtifact(dir: string, name: string, contents: string | Buffer): void {
  try {
    writeFileSync(path.join(dir, name), contents);
  } catch (error: unknown) {
    process.stderr.write(`Could not write the artifact ${name}: ${String(error)}\n`);
  }
}

/** Copies every file of `from` (not recursively) into `to`, if `from` exists. */
export function copyFiles(from: string, to: string, prefix = ''): void {
  if (!existsSync(from)) {
    return;
  }
  for (const entry of readdirSync(from, { withFileTypes: true })) {
    if (entry.isFile()) {
      try {
        copyFileSync(path.join(from, entry.name), path.join(to, `${prefix}${entry.name}`));
      } catch (error: unknown) {
        process.stderr.write(`Could not copy ${entry.name}: ${String(error)}\n`);
      }
    }
  }
}

/** Appends a test's Trusted Types counts to the report in `root`. */
export function recordTrustedTypes(root: string, entry: TrustedTypesEntry): void {
  mkdirSync(root, { recursive: true });
  appendFileSync(path.join(root, TRUSTED_TYPES_REPORT), `${JSON.stringify(entry)}\n`);
}

/** Whether `value` is a {@link TrustedTypesEntry} (the report is read back from a file). */
function isEntry(value: unknown): value is TrustedTypesEntry {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const entry = value as Partial<Record<keyof TrustedTypesEntry, unknown>>;
  return (
    typeof entry.test === 'string' &&
    typeof entry.platform === 'string' &&
    typeof entry.count === 'number' &&
    Array.isArray(entry.directives) &&
    entry.directives.every((directive) => typeof directive === 'string') &&
    (entry.policyActive === null || typeof entry.policyActive === 'boolean')
  );
}

/** Text for a Markdown table cell: one line, no pipes, at most 120 characters. */
function cell(text: string): string {
  return text
    .replace(/[\s|]+/g, ' ')
    .trim()
    .slice(0, 120);
}

/** The Markdown summary of the Trusted Types report's lines (malformed lines are skipped). */
export function trustedTypesSummary(report: string): string {
  const entries: TrustedTypesEntry[] = [];
  for (const line of report.split('\n')) {
    if (line.trim() === '') {
      continue;
    }
    try {
      const value: unknown = JSON.parse(line);
      if (isEntry(value)) {
        entries.push(value);
      }
    } catch {
      // Not a line the harness wrote.
    }
  }
  const lines = [
    '### Trusted Types trial (report-only, docs/spec/08-security.md §8.8)',
    '',
    'Content Security Policy violations each test saw (directive names only; the trial never fails a test).',
    '',
    '| Test | Platform | Violations | Directives | Report-only policy active |',
    '| --- | --- | ---: | --- | --- |',
  ];
  for (const entry of entries) {
    const active = entry.policyActive === null ? 'unknown' : entry.policyActive ? 'yes' : 'no';
    const directives = entry.directives.length === 0 ? '—' : entry.directives.join(', ');
    lines.push(
      `| ${cell(entry.test)} | ${cell(entry.platform)} | ${String(entry.count)} | ${cell(directives)} | ${active} |`,
    );
  }
  if (entries.length === 0) {
    lines.push('| (no test reported) | | | | |');
  }
  return `${lines.join('\n')}\n`;
}

/** Deletes the report of an earlier run in `root`. */
export function clearTrustedTypes(root: string): void {
  rmSync(path.join(root, TRUSTED_TYPES_REPORT), { force: true });
  rmSync(path.join(root, TRUSTED_TYPES_SUMMARY), { force: true });
}

/** Writes the Markdown summary of the report in `root` next to it. */
export function writeTrustedTypesSummary(root: string): void {
  const report = path.join(root, TRUSTED_TYPES_REPORT);
  const text = existsSync(report) ? readFileSync(report, 'utf8') : '';
  mkdirSync(root, { recursive: true });
  writeFileSync(path.join(root, TRUSTED_TYPES_SUMMARY), trustedTypesSummary(text));
}
