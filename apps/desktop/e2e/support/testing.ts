/**
 * Helpers for the harness's own unit tests (not for the specs): a fake `tauri-driver` that starts a
 * process tree the way the real one does on Linux, so that starting and stopping the driver can be
 * tested without an app or a browser.
 */
import { chmodSync, existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { readProcess } from './processes';
import { waitFor } from './wait';

/**
 * The fake driver, a Node.js script. Its role comes from `FAKE_ROLE`:
 *
 * - `driver` (the command itself): with `FAKE_MODE=exit` it prints why it cannot start and exits
 *   with code 1, as `tauri-driver` does without a native driver. Otherwise it answers every HTTP
 *   request on `--port` and starts the native driver; on SIGTERM it kills the native driver and
 *   exits, as `tauri-driver` 2.1.0 does, which leaves the app running.
 * - `native`: starts the app and waits; with `FAKE_MODE=orphan` it exits at once instead, so the
 *   app is reparented before the driver is stopped.
 * - `app`: starts a child with an empty environment (as the app starts g++ or the program), writes
 *   both PIDs into `FAKE_PIDS`, and waits.
 */
const FAKE_DRIVER = String.raw`'use strict';
const { spawn } = require('node:child_process');
const { writeFileSync } = require('node:fs');
const http = require('node:http');
const path = require('node:path');

const role = process.env.FAKE_ROLE || 'driver';
const mode = process.env.FAKE_MODE || 'serve';
const start = (next) =>
  spawn(process.execPath, [__filename], {
    env: { ...process.env, FAKE_ROLE: next },
    stdio: 'ignore',
  });
const forever = () => setInterval(() => undefined, 60000);

if (role === 'driver') {
  if (mode === 'exit') {
    process.stderr.write('can not find the supplied binary path\n');
    process.exit(1);
  }
  const port = Number(process.argv.find((arg) => arg.startsWith('--port=')).slice(7));
  const native = start('native');
  http
    .createServer((request, response) => {
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end('{"value":{"ready":true}}');
    })
    .listen(port, '127.0.0.1');
  process.on('SIGTERM', () => {
    native.kill('SIGKILL');
    process.exit(0);
  });
} else if (role === 'native') {
  start('app');
  if (mode === 'orphan') {
    setTimeout(() => process.exit(0), 200);
  } else {
    forever();
  }
} else if (role === 'app') {
  const child = spawn('/bin/sleep', ['600'], { env: {}, stdio: 'ignore' });
  writeFileSync(path.join(process.env.FAKE_PIDS, 'clean'), String(child.pid));
  writeFileSync(path.join(process.env.FAKE_PIDS, 'app'), String(process.pid));
  forever();
}
`;

/**
 * Writes the fake driver into `folder` and returns the command that runs it (a shell script, so
 * that it can be spawned like the real executable). Unix only.
 */
export function writeFakeDriver(folder: string): string {
  const script = path.join(folder, 'fake-tauri-driver.cjs');
  writeFileSync(script, FAKE_DRIVER);
  const command = path.join(folder, 'tauri-driver');
  writeFileSync(
    command,
    `#!/bin/sh\nexec ${shellQuote(process.execPath)} ${shellQuote(script)} "$@"\n`,
  );
  chmodSync(command, 0o755);
  return command;
}

/** `text` as one single-quoted shell word. */
function shellQuote(text: string): string {
  return `'${text.replace(/'/g, `'\\''`)}'`;
}

/** The PIDs of the fake app and its child once the app has written them into `folder`. */
export async function fakeAppPids(folder: string): Promise<{ app: number; clean: number }> {
  return waitFor(
    () => {
      const app = path.join(folder, 'app');
      const clean = path.join(folder, 'clean');
      if (!existsSync(app) || !existsSync(clean)) {
        return null;
      }
      return {
        app: Number(readFileSync(app, 'utf8')),
        clean: Number(readFileSync(clean, 'utf8')),
      };
    },
    { timeout: 10_000, interval: 50, message: 'the fake app to start' },
  );
}

/** Whether process `pid` runs (exists and is not a zombie). */
export function running(pid: number): boolean {
  const entry = readProcess(pid);
  return entry !== null && entry.state !== 'Z' && entry.state !== 'X';
}
