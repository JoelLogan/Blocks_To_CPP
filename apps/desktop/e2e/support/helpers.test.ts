import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import {
  artifactDir,
  artifactName,
  clearTrustedTypes,
  copyFiles,
  recordTrustedTypes,
  TRUSTED_TYPES_REPORT,
  TRUSTED_TYPES_SUMMARY,
  trustedTypesSummary,
  writeTrustedTypesSummary,
} from './artifacts';
import { freePort } from './driver';
import { dropPoint } from './drag';
import { mainFunction } from './editor';
import { byTestId, errorCount } from './ui';
import { sleep, waitFor, WaitTimeoutError } from './wait';

const folders: string[] = [];

function tempFolder(): string {
  const folder = mkdtempSync(path.join(tmpdir(), 'b2c-e2e-support-'));
  folders.push(folder);
  return folder;
}

afterEach(() => {
  for (const folder of folders.splice(0)) {
    rmSync(folder, { recursive: true, force: true });
  }
});

describe('waitFor', () => {
  it('returns the first accepted result', async () => {
    let calls = 0;
    const result = await waitFor(
      () => {
        calls += 1;
        return calls >= 3 ? 'done' : null;
      },
      { timeout: 2_000, interval: 5, message: 'three calls' },
    );
    expect(result).toBe('done');
    expect(calls).toBe(3);
  });

  it('treats a throwing probe as not yet, and names its last error on timeout', async () => {
    const waiting = waitFor(
      () => {
        throw new Error('not there');
      },
      { timeout: 50, interval: 10, message: () => 'the thing' },
    );
    await expect(waiting).rejects.toThrow(WaitTimeoutError);
    await expect(waiting).rejects.toThrow(/the thing \(last error: not there\)/);
  });

  it('accepts falsy results other than null, undefined and false', async () => {
    expect(await waitFor(() => 0, { timeout: 50, message: 'zero' })).toBe(0);
    expect(await waitFor(() => '', { timeout: 50, message: 'empty' })).toBe('');
    await expect(waitFor(() => false, { timeout: 20, message: 'never' })).rejects.toThrow(
      'Timed out after 0.0 s waiting for never',
    );
  });

  it('sleeps', async () => {
    const start = Date.now();
    await sleep(20);
    expect(Date.now() - start).toBeGreaterThanOrEqual(15);
  });
});

describe('artifacts', () => {
  it('names folders after tests', () => {
    expect(artifactName('M2 exit: the "guessing" game!')).toBe('m2-exit-the-guessing-game');
    expect(artifactName('***')).toBe('test');
    expect(artifactName('x'.repeat(200))).toHaveLength(80);
  });

  it('copies logs and appends Trusted Types counts as JSON lines', () => {
    const root = tempFolder();
    const dir = artifactDir(root, 'A test');
    expect(dir).toBe(path.join(root, 'a-test'));
    const logs = tempFolder();
    writeFileSync(path.join(logs, 'blocks2cpp.log'), 'line\n');
    copyFiles(logs, dir, 'app-');
    copyFiles(path.join(logs, 'missing'), dir);
    expect(readFileSync(path.join(dir, 'app-blocks2cpp.log'), 'utf8')).toBe('line\n');

    const entry = {
      test: 'A test',
      platform: 'linux' as const,
      count: 2,
      directives: ['require-trusted-types-for'],
      policyActive: true,
    };
    recordTrustedTypes(root, entry);
    recordTrustedTypes(root, { ...entry, count: 0, directives: [], policyActive: null });
    const lines = readFileSync(path.join(root, TRUSTED_TYPES_REPORT), 'utf8').trim().split('\n');
    expect(lines.map((line) => JSON.parse(line) as unknown)).toEqual([
      entry,
      { ...entry, count: 0, directives: [], policyActive: null },
    ]);
  });
});

describe('the Trusted Types summary', () => {
  it('turns the report into a Markdown table, skipping lines it did not write', () => {
    const report = [
      JSON.stringify({
        test: 'game | one',
        platform: 'win32',
        count: 3,
        directives: ['require-trusted-types-for'],
        policyActive: true,
      }),
      'not json',
      JSON.stringify({ test: 'x' }),
      JSON.stringify({
        test: 'flood',
        platform: 'linux',
        count: 0,
        directives: [],
        policyActive: null,
      }),
      '',
    ].join('\n');
    const summary = trustedTypesSummary(report);
    expect(summary).toContain('| game one | win32 | 3 | require-trusted-types-for | yes |');
    expect(summary).toContain('| flood | linux | 0 | — | unknown |');
    // The header, the separator and the two valid lines.
    expect(summary.split('\n').filter((line) => line.startsWith('| ')).length).toBe(4);
    expect(trustedTypesSummary('')).toContain('(no test reported)');
  });

  it('writes the summary next to the report, and starts afresh', () => {
    const root = tempFolder();
    recordTrustedTypes(root, {
      test: 'a',
      platform: 'linux',
      count: 1,
      directives: ['img-src'],
      policyActive: false,
    });
    writeTrustedTypesSummary(root);
    expect(readFileSync(path.join(root, TRUSTED_TYPES_SUMMARY), 'utf8')).toContain(
      '| a | linux | 1 | img-src | no |',
    );
    clearTrustedTypes(root);
    writeTrustedTypesSummary(root);
    expect(readFileSync(path.join(root, TRUSTED_TYPES_SUMMARY), 'utf8')).toContain(
      '(no test reported)',
    );
  });
});

describe('small helpers', () => {
  it('finds a free port', async () => {
    const port = await freePort();
    expect(port).toBeGreaterThan(0);
    expect(port).toBeLessThan(65536);
  });

  it('computes where to drop a block so its connection lands on the target', () => {
    expect(dropPoint({ x: 110, y: 210 }, { x: 100, y: 200 }, { x: 300, y: 400 })).toEqual({
      x: 310,
      y: 410,
    });
  });

  it('reads the number of errors from the Problems summary', () => {
    expect(errorCount('No problems')).toBe(0);
    expect(errorCount('1 error')).toBe(1);
    expect(errorCount('1,204 errors, 3 warnings')).toBe(1204);
    expect(errorCount('2 warnings')).toBe(0);
  });

  it('builds test ID locators and refuses odd IDs', () => {
    expect(byTestId('toolbar-run').value).toBe('[data-testid="toolbar-run"]');
    expect(() => byTestId('a"]')).toThrow('Not a test ID');
  });

  it('cuts the main function out of generated C++', () => {
    expect(mainFunction('// header\nint helper() {}\nint main() {\n    return 0;\n}\n')).toBe(
      'int main() {\n    return 0;\n}',
    );
    expect(() => mainFunction('int helper() {}')).toThrow('no main function');
  });
});
