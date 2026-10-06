/**
 * `tauri-driver`, the WebDriver intermediary that starts the platform's own driver
 * (`WebKitWebDriver` on Linux, `msedgedriver` on Windows) and translates `tauri:options` for it.
 * Each test starts its own on free ports, with the app's environment: the native driver starts
 * the app, which inherits it (that is how each test gets a fresh profile).
 */
import { type ChildProcess, spawn } from 'node:child_process';
import { closeSync, openSync } from 'node:fs';
import { createServer } from 'node:net';

import { sleep, waitFor } from './wait';

/** How long `tauri-driver` may take to answer its status request. */
const START_TIMEOUT_MS = 30_000;

/** How long `tauri-driver` gets to stop (it then stops the native driver) before it is killed. */
const STOP_TIMEOUT_MS = 5_000;

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

/** A running `tauri-driver`. */
export class TauriDriver {
  /** The WebDriver server URL. */
  readonly url: string;
  readonly #process: ChildProcess;
  #exit: { code: number | null; signal: NodeJS.Signals | null } | null = null;
  readonly #exited: Promise<void>;

  private constructor(url: string, child: ChildProcess) {
    this.url = url;
    this.#process = child;
    this.#exited = new Promise((resolve) => {
      child.once('exit', (code, signal) => {
        this.#exit = { code, signal };
        resolve();
      });
      child.once('error', () => {
        this.#exit ??= { code: null, signal: null };
        resolve();
      });
    });
  }

  /** Starts `tauri-driver` and waits until it answers. */
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
    const driver = new TauriDriver(`http://127.0.0.1:${String(port)}/`, child);
    try {
      await waitFor(
        async () => {
          if (driver.#exit !== null) {
            throw new Error(`tauri-driver exited (${driver.exitDescription()})`);
          }
          const response = await fetch(new URL('status', driver.url), {
            signal: AbortSignal.timeout(2_000),
          });
          return response.ok;
        },
        {
          timeout: START_TIMEOUT_MS,
          interval: 200,
          message: `tauri-driver to answer on port ${String(port)} (log: ${options.logFile})`,
        },
      );
    } catch (error: unknown) {
      await driver.stop();
      throw error;
    }
    return driver;
  }

  /** How the process ended, for messages. */
  exitDescription(): string {
    const exit = this.#exit;
    if (exit === null) {
      return 'running';
    }
    return exit.signal === null ? `exit code ${String(exit.code)}` : `signal ${exit.signal}`;
  }

  /**
   * Stops `tauri-driver`: a polite stop first (on Linux it then stops the native driver; on
   * Windows its job object does), then a kill.
   */
  async stop(): Promise<void> {
    if (this.#exit !== null) {
      return;
    }
    this.#process.kill('SIGTERM');
    const stopped = await Promise.race([
      this.#exited.then(() => true),
      sleep(STOP_TIMEOUT_MS).then(() => false),
    ]);
    if (!stopped) {
      this.#process.kill('SIGKILL');
      await this.#exited;
    }
  }
}
