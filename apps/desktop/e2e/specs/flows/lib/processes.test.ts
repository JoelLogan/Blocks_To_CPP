/** The flow tests' view of the system's processes: parsing, trees, paths and real processes. */
import { spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import {
  descendants,
  isInside,
  isRunning,
  listOsProcesses,
  type OsProcess,
  parseCimProcesses,
  pathSpellings,
  ProcessQueryError,
  processesRunningFrom,
  waitForProcessEnd,
} from './processes';

const folders: string[] = [];

function tempFolder(): string {
  const folder = mkdtempSync(path.join(tmpdir(), 'b2c-e2e-flows-'));
  folders.push(folder);
  return folder;
}

afterEach(() => {
  for (const folder of folders.splice(0)) {
    rmSync(folder, { recursive: true, force: true });
  }
});

function entry(pid: number, ppid: number, exe: string | null = null): OsProcess {
  return { pid, ppid, exe, commandLine: null };
}

describe('parseCimProcesses', () => {
  it('reads the listing of Get-CimInstance Win32_Process', () => {
    const json = JSON.stringify([
      { ProcessId: 4, ParentProcessId: 0, ExecutablePath: null, CommandLine: null },
      {
        ProcessId: 812,
        ParentProcessId: 640,
        ExecutablePath: 'C:\\app\\blocks2cpp-desktop.exe',
        CommandLine: '"C:\\app\\blocks2cpp-desktop.exe"',
      },
    ]);
    expect(parseCimProcesses(json)).toEqual([
      { pid: 4, ppid: 0, exe: null, commandLine: null },
      {
        pid: 812,
        ppid: 640,
        exe: 'C:\\app\\blocks2cpp-desktop.exe',
        commandLine: '"C:\\app\\blocks2cpp-desktop.exe"',
      },
    ]);
  });

  it('takes a single object, an empty listing, and skips entries without a process ID', () => {
    expect(parseCimProcesses('{"ProcessId": 7, "ParentProcessId": 1}')).toEqual([
      { pid: 7, ppid: 1, exe: null, commandLine: null },
    ]);
    expect(parseCimProcesses('')).toEqual([]);
    expect(parseCimProcesses('[{"ProcessId": -1}, {"ProcessId": "9"}, {}]')).toEqual([]);
    expect(parseCimProcesses('[{"ProcessId": 3, "ExecutablePath": ""}]')).toEqual([
      { pid: 3, ppid: 0, exe: null, commandLine: null },
    ]);
  });

  it('refuses text that is not a listing', () => {
    expect(() => parseCimProcesses('not json')).toThrow(ProcessQueryError);
    expect(() => parseCimProcesses('[1, 2]')).toThrow(ProcessQueryError);
  });
});

describe('descendants', () => {
  it('finds children and their children, whatever the listing order, without the roots', () => {
    const all = [entry(30, 20), entry(10, 1), entry(20, 10), entry(40, 99), entry(21, 10)];
    expect(
      descendants(all, [10])
        .map((found) => found.pid)
        .sort(),
    ).toEqual([20, 21, 30]);
    expect(descendants(all, [77])).toEqual([]);
  });

  it('stops at a process that names itself as its parent', () => {
    expect(descendants([entry(5, 5), entry(6, 5)], [5]).map((found) => found.pid)).toEqual([6]);
  });
});

describe('paths', () => {
  it('compares Windows paths without case and with their 8.3 or long spelling', () => {
    const spellings = pathSpellings(
      'C:\\Users\\RUNNER~1\\Temp\\b2c',
      'win32',
      () => 'C:\\Users\\runneradmin\\Temp\\b2c',
    );
    expect(spellings).toContain(path.normalize('c:\\users\\runner~1\\temp\\b2c'));
    expect(spellings).toContain(path.normalize('c:\\users\\runneradmin\\temp\\b2c'));
  });

  it('keeps the given spelling when the path cannot be resolved', () => {
    const missing = path.normalize(path.join(tmpdir(), 'b2c-e2e-flows-no-such-folder'));
    // Windows paths are compared without case, so they come back in lower case there.
    expect(pathSpellings(missing)).toEqual([
      process.platform === 'win32' ? missing.toLowerCase() : missing,
    ]);
  });

  it.skipIf(process.platform === 'win32')(
    'tells a folder from a sibling with a longer name',
    () => {
      const spellings = ['/tmp/profile/cache'];
      expect(isInside('/tmp/profile/cache/builds/p/out/game', spellings, 'linux')).toBe(true);
      expect(isInside('/tmp/profile/cache', spellings, 'linux')).toBe(true);
      expect(isInside('/tmp/profile/cache2/game', spellings, 'linux')).toBe(false);
      expect(isInside('/tmp/profile', spellings, 'linux')).toBe(false);
    },
  );

  it.skipIf(process.platform !== 'win32')('matches Windows paths without regard to case', () => {
    const spellings = ['c:\\temp\\profile\\cache'];
    expect(isInside('C:\\Temp\\Profile\\Cache\\builds\\out\\game.exe', spellings, 'win32')).toBe(
      true,
    );
    expect(isInside('C:\\Temp\\Profile\\Cache2\\game.exe', spellings, 'win32')).toBe(false);
  });

  it.skipIf(process.platform === 'win32')('resolves a folder named through a link', () => {
    const folder = tempFolder();
    const real = path.join(folder, 'real');
    mkdirSync(real);
    const link = path.join(folder, 'link');
    symlinkSync(real, link, 'dir');
    expect(pathSpellings(link)).toEqual([link, real]);
    expect(isInside(path.join(real, 'out', 'game'), pathSpellings(link))).toBe(true);
  });
});

/**
 * On Windows each listing starts PowerShell (`Get-CimInstance Win32_Process`), which takes several
 * seconds on a CI runner.
 */
const LISTING_TEST_TIMEOUT_MS = 60_000;

describe('real processes', () => {
  it(
    'sees this process and its executable',
    async () => {
      expect(isRunning(process.pid)).toBe(true);
      const self = (await listOsProcesses()).find((found) => found.pid === process.pid);
      expect(self).toBeDefined();
      const exe = self?.exe ?? null;
      expect(exe === null ? null : pathSpellings(exe)).toEqual(pathSpellings(process.execPath));
    },
    LISTING_TEST_TIMEOUT_MS,
  );

  it(
    'finds a process by the folder its executable is in, and waits for it to end',
    async () => {
      const folder = path.dirname(process.execPath);
      const child = spawn(process.execPath, ['-e', 'setTimeout(() => {}, 30000)'], {
        stdio: 'ignore',
      });
      try {
        const pid = child.pid ?? 0;
        expect(pid).toBeGreaterThan(0);
        expect((await processesRunningFrom(folder)).map((found) => found.pid)).toContain(pid);
        expect(await waitForProcessEnd(pid, 200)).toBe(false);
        child.kill('SIGKILL');
        expect(await waitForProcessEnd(pid, 10_000)).toBe(true);
        expect(isRunning(pid)).toBe(false);
      } finally {
        child.kill('SIGKILL');
      }
    },
    LISTING_TEST_TIMEOUT_MS,
  );
});
