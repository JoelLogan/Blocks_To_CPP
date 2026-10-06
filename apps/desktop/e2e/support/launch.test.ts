/**
 * `launchApp` when no WebDriver session starts: it fails at once, and its message points at the
 * copy of the driver's log in the test's artifacts (the test's own folder is deleted).
 */
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it, type TestContext, vi } from 'vitest';

import { launchApp } from './app';
import { writeFakeDriver } from './testing';

const folders: string[] = [];

afterEach(() => {
  vi.unstubAllEnvs();
  for (const folder of folders.splice(0)) {
    rmSync(folder, { recursive: true, force: true });
  }
});

describe.skipIf(process.platform !== 'linux')('launchApp', () => {
  it('fails at once when the driver exits, and names the saved copy of its log', async () => {
    const folder = mkdtempSync(path.join(tmpdir(), 'b2c-e2e-launch-'));
    folders.push(folder);
    const artifacts = path.join(folder, 'artifacts');
    vi.stubEnv('B2C_E2E_APP', process.execPath);
    vi.stubEnv('B2C_E2E_TAURI_DRIVER', writeFakeDriver(folder));
    vi.stubEnv('B2C_E2E_NATIVE_DRIVER', '');
    vi.stubEnv('B2C_E2E_ARTIFACTS', artifacts);
    vi.stubEnv('FAKE_MODE', 'exit');
    const onTestFinished = vi.fn();
    const context = { task: { name: 'A launch that fails' }, onTestFinished };

    const start = Date.now();
    const failure: unknown = await launchApp(context as unknown as TestContext).then(
      () => null,
      (error: unknown) => error,
    );
    expect(Date.now() - start).toBeLessThan(10_000);
    expect(failure).toBeInstanceOf(Error);
    const message = failure instanceof Error ? failure.message : '';
    expect(message).toMatch(
      /^The app under test did not start: tauri-driver \(\S+\) ended before it answered \(exit code 1\)/,
    );
    expect(message).toContain('can not find the supplied binary path');
    const log = path.join(artifacts, 'a-launch-that-fails', 'driver-tauri-driver.log');
    expect(message).toContain(`The driver's log is ${log}`);
    expect(readFileSync(log, 'utf8')).toContain('can not find the supplied binary path');
    // No session, so no clean-up was registered for the end of the test.
    expect(onTestFinished).not.toHaveBeenCalled();
  });
});
