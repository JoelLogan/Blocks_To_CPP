/**
 * `tauri-driver`, the WebDriver intermediary that starts the platform's own driver
 * (`WebKitWebDriver` on Linux, `msedgedriver` on Windows) and translates `tauri:options` for it.
 * Each test starts its own on free ports, with the app's environment: the native driver starts
 * the app, which inherits it (that is how each test gets a fresh profile).
 */
import { type ChildProcess, spawn } from 'node:child_process';
import { closeSync, openSync, readFileSync } from 'node:fs';
import { createServer } from 'node:net';

import { killProcesses, type ProcessEntry, stillRunning, testProcesses } from './processes';
import { sleep, waitFor } from './wait';

/** How long `tauri-driver` may take to answer its status request. */
const START_TIMEOUT_MS = 30_000;

/** How long `tauri-driver` gets to stop (it then stops the native driver) before it is killed. */
const STOP_TIMEOUT_MS = 5_000;

/** How long the processes left behind (Linux) may take to end once they are killed. */
const LEFTOVER_TIMEOUT_MS = 5_000;

/** The most characters of the driver's output an error message quotes. */
export const LOG_TAIL_CHARS = 2_000;

/** What {@link TauriDriver.start} needs. */
export interface TauriDriverOptions {
  /** The `tauri-driver` command. */
  readonly command: string;
  /** `--native-driver`, or `null`. */
  readonly nativeDriver: string | null;
  /** The environment (the app inherits it). */
  readonly env: NodeJS.ProcessEnv;
  /** Where its output goes. */
  readonly logFile: string;
  /**
   * An entry of `env` (`NAME=value`) that no other process has: the harness passes the test's
   * `B2C_E2E_ROOT`. On Linux, {@link TauriDriver.stop} kills every process that still has it
   * (the app outlives `tauri-driver` when its session did not end; see ./processes.ts).
   */
  readonly marker?: string | null;
}

/** `tauri-driver` could not be started, or did not answer. */
export class TauriDriverError extends Error {
  override readonly name = 'TauriDriverError';
}

/** A free TCP port on the loopback interface (free when asked; taken by the caller at once). */
export function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      server.close(() => {
        if (typeof address === 'object' && address !== null) {
          resolve(address.port);
        } else {
          reject(new Error('No free port was found'));
        }
      });
    });
  });
}

/**
 * The end of a log file for an error message (at most {@link LOG_TAIL_CHARS} characters, on lines
 * of its own after a colon), or '' when it is empty or cannot be read.
 */
export function logTail(file: string, limit = LOG_TAIL_CHARS): string {
  let text: string;
  try {
    text = readFileSync(file, 'utf8').trim();
  } catch {
    return '';
  }
  if (text === '') {
    return '';
  }
  return `. Its output${text.length > limit ? ' (the end)' : ''}:\n${text.slice(-limit)}`;
}

/** A running `tauri-driver`. */
export class TauriDriver {
  /** The WebDriver server URL. */
  readonly url: string;
  readonly #process: ChildProcess;
  readonly #marker: string | null;
  #exit: { code: number | null; signal: NodeJS.Signals | null } | null = null;
  /** Why it could not be started (the `error` event), or `null`. */
  #spawnError: Error | null = null;
  readonly #exited: Promise<void>;

  private constructor(url: string, child: ChildProcess, marker: string | null) {
    this.url = url;
    this.#process = child;
    this.#marker = marker;
    this.#exited = new Promise((resolve) => {
      child.once('exit', (code, signal) => {
        this.#exit = { code, signal };
        resolve();
      });
      child.once('error', (error) => {
        this.#spawnError = error;
        this.#exit ??= { code: null, signal: null };
        resolve();
      });
    });
  }

  /**
   * Starts `tauri-driver` and waits until it answers.
   *
   * @throws TauriDriverError at once when it exits (or cannot be started) before it answers, and
   *   after `START_TIMEOUT_MS` when it runs without answering; the message quotes the end of its
   *   output.
   */
  static async start(options: TauriDriverOptions): Promise<TauriDriver> {
    const port = await freePort();
    const nativePort = await freePort();
    const args = [`--port=${String(port)}`, `--native-port=${String(nativePort)}`];
    if (options.nativeDriver !== null) {
      args.push(`--native-driver=${options.nativeDriver}`);
    }
    const log = openSync(options.logFile, 'a');
    let child: ChildProcess;
    try {
      child = spawn(options.command, args, {
        env: options.env,
        stdio: ['ignore', log, log],
        windowsHide: true,
      });
    } finally {
      // The child has its own copy of the descriptor.
      closeSync(log);
    }
    const driver = new TauriDriver(
      `http://127.0.0.1:${String(port)}/`,
      child,
      options.marker ?? null,
    );
    let outcome: 'answered' | 'exited';
    try {
      outcome = await waitFor(
        async () => {
          // A driver that is gone ends the wait at once instead of being asked until the timeout.
          if (driver.#exit !== null) {
            return 'exited' as const;
          }
          const response = await fetch(new URL('status', driver.url), {
            signal: AbortSignal.timeout(2_000),
          });
          return response.ok ? ('answered' as const) : null;
        },
        {
          timeout: START_TIMEOUT_MS,
          interval: 200,
          message: `tauri-driver (${options.command}) to answer on port ${String(port)}`,
        },
      );
    } catch (error: unknown) {
      await driver.stop();
      const message = error instanceof Error ? error.message : String(error);
      throw new TauriDriverError(`${message}${logTail(options.logFile)}`, { cause: error });
    }
    if (outcome === 'exited') {
      await driver.stop();
      throw new TauriDriverError(
        `tauri-driver (${options.command}) ended before it answered (${driver.exitDescription()})${logTail(options.logFile)}`,
      );
    }
    return driver;
  }

  /** How the process ended, for messages. */
  exitDescription(): string {
    if (this.#spawnError !== null) {
      return `it could not be started: ${this.#spawnError.message}`;
    }
    const exit = this.#exit;
    if (exit === null) {
      return 'running';
    }
    return exit.signal === null ? `exit code ${String(exit.code)}` : `signal ${exit.signal}`;
  }

  /**
   * Stops `tauri-driver`: a polite stop first (on Linux it then stops the native driver; on
   * Windows its job object does), then a kill. On Linux whatever it started that is still left
   * then (the app and WebKit's processes when the session did not end, and what the app started)
   * is killed as well; see ./processes.ts. Every wait is bounded.
   */
  async stop(): Promise<void> {
    const linux = process.platform === 'linux';
    const pid = this.#process.pid;
    // Its children are reparented once it has exited, so they are listed first. A driver that
    // has exited may have had its PID reused, so it is no root then.
    const before: ProcessEntry[] = linux
      ? testProcesses({
          roots: this.#exit === null && pid !== undefined ? [pid] : [],
          marker: this.#marker,
        })
      : [];
    if (this.#exit === null) {
      this.#process.kill('SIGTERM');
      const stopped = await Promise.race([
        this.#exited.then(() => true),
        sleep(STOP_TIMEOUT_MS).then(() => false),
      ]);
      if (!stopped) {
        this.#process.kill('SIGKILL');
        await Promise.race([this.#exited, sleep(STOP_TIMEOUT_MS)]);
      }
    }
    if (linux) {
      const roots = before.filter((entry) => stillRunning(entry)).map((entry) => entry.pid);
      const after = testProcesses({ roots, marker: this.#marker });
      const left = await killProcesses([...before, ...after], LEFTOVER_TIMEOUT_MS);
      if (left.length > 0) {
        process.stderr.write(
          `Processes the test started are still running after SIGKILL: ${left.join(', ')}\n`,
        );
      }
    }
  }
}
