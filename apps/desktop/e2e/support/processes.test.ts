/** Finding and killing the processes a test left behind, from a fake `/proc` and from real ones. */
import { spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import {
  descendantsOf,
  hasEnvironmentEntry,
  killProcesses,
  listProcesses,
  parseStat,
  readProcess,
  stillRunning,
  testProcesses,
} from './processes';
import { running } from './testing';

const folders: string[] = [];

function tempFolder(): string {
  const folder = mkdtempSync(path.join(tmpdir(), 'b2c-e2e-proc-'));
  folders.push(folder);
  return folder;
}

afterEach(() => {
  for (const folder of folders.splice(0)) {
    rmSync(folder, { recursive: true, force: true });
  }
});

/** A `stat` line with the fields the harness reads (and filler for the others). */
function stat(pid: number, name: string, state: string, ppid: number, startTime: number): string {
  const filler = Array.from({ length: 17 }, () => '0').join(' ');
  return `${String(pid)} (${name}) ${state} ${String(ppid)} ${filler} ${String(startTime)} 0 0\n`;
}

/** A fake `/proc` with these processes: [pid, ppid, state, environment]. */
function fakeProc(processes: readonly [number, number, string, readonly string[]][]): string {
  const proc = tempFolder();
  for (const [pid, ppid, state, environment] of processes) {
    const dir = path.join(proc, String(pid));
    mkdirSync(dir);
    writeFileSync(path.join(dir, 'stat'), stat(pid, `p${String(pid)}`, state, ppid, 1000 + pid));
    writeFileSync(path.join(dir, 'environ'), environment.map((entry) => `${entry}\0`).join(''));
  }
  mkdirSync(path.join(proc, 'self'));
  writeFileSync(path.join(proc, 'uptime'), '1.0 1.0\n');
  return proc;
}

describe('parseStat', () => {
  it('reads the state, the parent and the start time after the command name', () => {
    expect(parseStat(42, stat(42, 'sleep', 'S', 7, 123456))).toEqual({
      pid: 42,
      ppid: 7,
      state: 'S',
      startTime: '123456',
    });
    // A command name may hold spaces and parentheses.
    expect(parseStat(9, stat(9, 'Web Content) R 1 (x', 'Z', 3, 5))).toMatchObject({
      ppid: 3,
      state: 'Z',
      startTime: '5',
    });
  });

  it('refuses text that is not a stat line', () => {
    expect(parseStat(1, '')).toBeNull();
    expect(parseStat(1, '1 (short) S 0')).toBeNull();
    expect(parseStat(1, stat(1, 'x', 'S', 0, 5).replace(' S ', ' ? '))).toBeNull();
  });
});

describe('the processes of a test', () => {
  const marker = 'B2C_E2E_ROOT=/tmp/b2c-e2e-1/profile';
  const proc = () =>
    fakeProc([
      // The driver and the native driver, with the test's environment.
      [100, 1, 'S', ['PATH=/usr/bin', marker]],
      [101, 100, 'S', [marker]],
      // The app, reparented to init, and what it started without the environment.
      [102, 1, 'S', [marker]],
      [103, 102, 'R', ['PATH=/usr/bin']],
      [104, 103, 'S', []],
      // Another test's app, an unrelated process and a zombie of this test.
      [200, 1, 'S', ['B2C_E2E_ROOT=/tmp/b2c-e2e-2/profile']],
      [300, 1, 'S', ['B2C_E2E_ROOT=/tmp/b2c-e2e-1/profile-x']],
      [105, 1, 'Z', [marker]],
    ]);

  it('lists processes and their descendants', () => {
    const root = proc();
    const all = listProcesses(root);
    expect(all.map((entry) => entry.pid).sort()).toEqual([100, 101, 102, 103, 104, 105, 200, 300]);
    expect(
      descendantsOf(all, [102])
        .map((entry) => entry.pid)
        .sort(),
    ).toEqual([103, 104]);
    expect(descendantsOf(all, [999])).toEqual([]);
    expect(readProcess(103, root)).toMatchObject({ ppid: 102, state: 'R', startTime: '1103' });
    expect(readProcess(999, root)).toBeNull();
    expect(listProcesses(path.join(root, 'missing'))).toEqual([]);
  });

  it('matches the environment entry exactly', () => {
    const root = proc();
    expect(hasEnvironmentEntry(102, marker, root)).toBe(true);
    expect(hasEnvironmentEntry(300, marker, root)).toBe(false);
    expect(hasEnvironmentEntry(103, marker, root)).toBe(false);
    expect(hasEnvironmentEntry(999, marker, root)).toBe(false);
  });

  it('finds the marked processes and everything below them and the roots, not others', () => {
    const root = proc();
    const pids = (found: readonly { pid: number }[]) => found.map((entry) => entry.pid).sort();
    expect(pids(testProcesses({ roots: [], marker }, root))).toEqual([100, 101, 102, 103, 104]);
    expect(pids(testProcesses({ roots: [102], marker: null }, root))).toEqual([103, 104]);
    expect(pids(testProcesses({ roots: [], marker: null }, root))).toEqual([]);
  });
});

describe.skipIf(process.platform !== 'linux')('killing real processes', () => {
  it('kills what still runs, and only the process an entry names', async () => {
    const child = spawn('/bin/sleep', ['600'], { stdio: 'ignore' });
    const exited = new Promise((resolve) => child.once('exit', resolve));
    const pid = child.pid ?? 0;
    const entry = readProcess(pid);
    expect(entry).not.toBeNull();
    if (entry === null) {
      return;
    }
    expect(stillRunning(entry)).toBe(true);

    // The same PID with another start time is another process (the PID was reused): untouched.
    expect(await killProcesses([{ ...entry, startTime: '1' }], 200)).toEqual([]);
    expect(running(pid)).toBe(true);

    expect(await killProcesses([entry, entry], 5_000)).toEqual([]);
    await exited;
    expect(stillRunning(entry)).toBe(false);
  });

  it('finds a process by its environment after its parent is gone', async () => {
    const marker = `B2C_E2E_ROOT=${tempFolder()}`;
    const [name, value] = marker.split('=') as [string, string];
    // A shell that starts a sleep in the background and exits: the sleep is reparented.
    const shell = spawn('/bin/sh', ['-c', '/bin/sleep 600 & echo $!'], {
      env: { ...process.env, [name]: value },
      stdio: ['ignore', 'pipe', 'ignore'],
    });
    const output = await new Promise<string>((resolve) => {
      let text = '';
      shell.stdout.on('data', (chunk: Buffer) => {
        text += chunk.toString();
      });
      shell.once('exit', () => {
        resolve(text);
      });
    });
    const orphan = Number(output.trim());
    try {
      const found = testProcesses({ roots: [], marker });
      expect(found.map((entry) => entry.pid)).toEqual([orphan]);
      expect(await killProcesses(found, 5_000)).toEqual([]);
      expect(running(orphan)).toBe(false);
    } finally {
      try {
        process.kill(orphan, 'SIGKILL');
      } catch {
        // Killed already.
      }
    }
  });
});
