/**
 * Starting and stopping `tauri-driver`, with a fake driver that starts processes the way the real
 * one does on Linux (./testing.ts): a driver that exits at once fails at once, and stopping the
 * driver leaves none of the processes it started behind.
 */
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { LOG_TAIL_CHARS, logTail, TauriDriver, TauriDriverError } from './driver';
import { fakeAppPids, running, writeFakeDriver } from './testing';

const folders: string[] = [];
const started: number[] = [];

function tempFolder(): string {
  const folder = mkdtempSync(path.join(tmpdir(), 'b2c-e2e-driver-'));
  folders.push(folder);
  return folder;
}

afterEach(() => {
  // Whatever a failed test left behind.
  for (const pid of started.splice(0)) {
    try {
      process.kill(pid, 'SIGKILL');
    } catch {
      // Gone already.
    }
  }
  for (const folder of folders.splice(0)) {
    rmSync(folder, { recursive: true, force: true });
  }
});

/** Options for the fake driver in a fresh folder, with `mode` and its own marker. */
function fakeOptions(mode: 'exit' | 'serve' | 'orphan') {
  const folder = tempFolder();
  const marker = `B2C_E2E_ROOT=${path.join(folder, 'profile')}`;
  return {
    folder,
    options: {
      command: writeFakeDriver(folder),
      nativeDriver: null,
      env: {
        ...process.env,
        B2C_E2E_ROOT: path.join(folder, 'profile'),
        FAKE_MODE: mode,
        FAKE_PIDS: folder,
      },
      logFile: path.join(folder, 'tauri-driver.log'),
      marker,
    },
  };
}

describe('logTail', () => {
  it('quotes the end of a log, or nothing', () => {
    const folder = tempFolder();
    const log = path.join(folder, 'log');
    expect(logTail(log)).toBe('');
    writeFileSync(log, '  \n');
    expect(logTail(log)).toBe('');
    writeFileSync(log, 'no native driver\n');
    expect(logTail(log)).toBe('. Its output:\nno native driver');
    writeFileSync(log, `${'x'.repeat(LOG_TAIL_CHARS)}END`);
    expect(logTail(log)).toBe(`. Its output (the end):\n${'x'.repeat(LOG_TAIL_CHARS - 3)}END`);
  });
});

describe.skipIf(process.platform !== 'linux')('TauriDriver', () => {
  it('fails at once when tauri-driver exits before it answers, and quotes why', async () => {
    const { options } = fakeOptions('exit');
    const start = Date.now();
    const starting = TauriDriver.start(options);
    await expect(starting).rejects.toThrow(TauriDriverError);
    await expect(starting).rejects.toThrow(
      /ended before it answered \(exit code 1\)\. Its output:\ncan not find the supplied binary path/,
    );
    // Not the 30 s start timeout.
    expect(Date.now() - start).toBeLessThan(10_000);
  });

  it('fails at once when tauri-driver cannot be started, and says why', async () => {
    const { folder, options } = fakeOptions('serve');
    const start = Date.now();
    await expect(
      TauriDriver.start({ ...options, command: path.join(folder, 'no-such-driver') }),
    ).rejects.toThrow(/it could not be started: spawn \S*no-such-driver ENOENT/);
    expect(Date.now() - start).toBeLessThan(10_000);
  });

  it('kills the app and what it started when the session did not end', async () => {
    const { folder, options } = fakeOptions('serve');
    const driver = await TauriDriver.start(options);
    const pids = await fakeAppPids(folder);
    started.push(pids.app, pids.clean);
    expect(running(pids.app)).toBe(true);
    expect(running(pids.clean)).toBe(true);

    // As after a failed or hung `session.quit()`: the driver is stopped with the app still open.
    await driver.stop();
    expect(running(pids.app)).toBe(false);
    // The app's child has no marker in its environment: it was found as the app's descendant.
    expect(running(pids.clean)).toBe(false);
    expect(driver.exitDescription()).toBe('exit code 0');
  });

  it('kills an app that was reparented before the driver stopped, by its environment', async () => {
    const { folder, options } = fakeOptions('orphan');
    const driver = await TauriDriver.start(options);
    const pids = await fakeAppPids(folder);
    started.push(pids.app, pids.clean);
    // The fake native driver exits, so the app no longer descends from the driver.
    await new Promise((resolve) => setTimeout(resolve, 500));
    expect(running(pids.app)).toBe(true);

    await driver.stop();
    expect(running(pids.app)).toBe(false);
    expect(running(pids.clean)).toBe(false);
  });

  it('stops a driver that has already ended without touching other processes', async () => {
    const { options } = fakeOptions('serve');
    const driver = await TauriDriver.start(options);
    await driver.stop();
    // A second stop has nothing left to do.
    await driver.stop();
    expect(running(process.pid)).toBe(true);
  });
});
