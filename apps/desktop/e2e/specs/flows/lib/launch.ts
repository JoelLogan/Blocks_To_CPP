/**
 * Launching the app for the flow tests, which need more than one launch per test (crash recovery,
 * settings that survive a restart), the app's environment changed (the toolchain setup page) or
 * the app ended the way a person or a crash ends it (closing the window, killing the process).
 *
 * It is ../../../support/app.ts's `launchApp` with a profile that outlives one launch: a
 * {@link FlowSession} owns the test's temporary folder (the profile `B2C_E2E_ROOT`, the dialog
 * scripts, the drivers' logs and the test's project files) and starts the app as often as the test
 * asks, one at a time, each with its own `tauri-driver`, dialog script and environment. When the
 * test finishes, a failed test's artifacts are saved as `launchApp` saves them, the Trusted Types
 * counts of the last app are reported, the app is closed and the folder removed.
 *
 * The harness's own pieces are reused: `TauriDriver`, `HookClient`, the settings and the app's
 * environment, the artifact files and `waitUntilReady`.
 */
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { Builder, Capabilities, type WebDriver } from 'selenium-webdriver';
import type { TestContext } from 'vitest';

import { type App, type DialogScript, QUIT_TIMEOUT_MS, waitUntilReady } from '../../../support/app';
import {
  artifactDir,
  copyFiles,
  recordTrustedTypes,
  writeArtifact,
} from '../../../support/artifacts';
import { TauriDriver } from '../../../support/driver';
import { appEnvironment, harnessSettings } from '../../../support/env';
import { HookClient } from '../../../support/hook';
import { sleep, waitFor, withTimeout } from '../../../support/wait';
import { crashProcess, findAppProcess, waitForProcessEnd } from './processes';
import { requestWindowClose } from './window';

/** The most launches one test may make. */
export const MAX_LAUNCHES = 8;

/** How long the app may take to exit once it was asked to (a quit, a closed window, a kill). */
export const EXIT_TIMEOUT_MS = 20_000;

/**
 * How long the app's process may take to show up in the process list after it started (on
 * Windows each look starts PowerShell).
 */
const FIND_TIMEOUT_MS = 30_000;

/**
 * How long the app may take to exit after its WebDriver session ended. WebKitWebDriver closes the
 * app; msedgedriver may close only WebView2, and then stopping `tauri-driver` ends the app (its
 * job object), so this is not waited for long.
 */
const QUIT_EXIT_TIMEOUT_MS = 10_000;

/** How long each artifact of a failed test may take to read. */
const ARTIFACT_TIMEOUT_MS = 10_000;

/** How long reading and probing the Trusted Types counts may take. */
const TRUSTED_TYPES_TIMEOUT_MS = 10_000;

/** How long the clean-up after a test may take in all (Vitest's hook timeout). */
const FINISH_TIMEOUT_MS = 120_000;

/** The variables a test may set for the app; the harness sets the rest itself. */
const ENVIRONMENT_OVERRIDES: ReadonlySet<string> = new Set(['B2C_E2E_TOOLCHAIN_DIRS', 'B2C_LOG']);

/** Options of {@link FlowSession.launch}. */
export interface FlowLaunchOptions {
  /** The dialog script of this launch; none answers every dialog with Cancel. */
  readonly dialogs?: DialogScript;
  /**
   * Variables of the app's environment that differ from the harness's: only
   * `B2C_E2E_TOOLCHAIN_DIRS` and `B2C_LOG`.
   */
  readonly env?: Readonly<Record<string, string>>;
}

/** The folders of a flow test. */
export interface FlowFolders {
  /** The test's temporary folder; everything below is removed when the test finishes. */
  readonly root: string;
  /** The app's profile (`B2C_E2E_ROOT`), the same for every launch of the test. */
  readonly profile: string;
  /** Where the test keeps its project files (the dialogs open and save them). */
  readonly projects: string;
}

/** How a launched app ended. */
export type AppEnd = 'running' | 'quit' | 'exited' | 'killed';

/** One launch of the app under test. */
export interface FlowApp extends App {
  /** The test's folders. */
  readonly folders: FlowFolders;
  /** Which launch of the test this is, from 1. */
  readonly launch: number;
  /** Whether it still runs, or how it ended. */
  readonly end: AppEnd;
  /** The app's process ID (looked up once, then remembered). */
  pid(): Promise<number>;
  /**
   * Ends the WebDriver session, which closes the app, waits a little for it to exit, and then
   * stops `tauri-driver`, which ends whatever of this launch is left.
   */
  quit(): Promise<void>;
  /** Kills the app as a crash would (`SIGKILL`, or `taskkill /F /T`), and waits for it to end. */
  kill(): Promise<void>;
  /** Asks the window to close, as its close button does; see ./window.ts. */
  closeWindow(): Promise<void>;
  /** Waits for the app to exit by itself (after {@link closeWindow}); see {@link EXIT_TIMEOUT_MS}. */
  waitForExit(timeout?: number): Promise<void>;
}

/** Removes a folder; on Windows a program that just exited may still hold a file for a moment. */
async function removeFolder(folder: string): Promise<void> {
  for (let attempt = 0; attempt < 10; attempt += 1) {
    try {
      rmSync(folder, { recursive: true, force: true });
      return;
    } catch {
      await sleep(500);
    }
  }
  process.stderr.write(`Could not remove the test folder ${folder}\n`);
}

/**
 * The environment overrides of a launch, checked: only the variables of
 * {@link ENVIRONMENT_OVERRIDES}, so a test cannot point the app at another profile or dialog script.
 *
 * @throws Error naming a variable a test may not set.
 */
export function checkedOverrides(env: Readonly<Record<string, string>>): Record<string, string> {
  const result: Record<string, string> = {};
  for (const [name, value] of Object.entries(env)) {
    if (!ENVIRONMENT_OVERRIDES.has(name)) {
      throw new Error(`A flow test may not set ${name} for the app`);
    }
    result[name] = value;
  }
  return result;
}

/**
 * The port of a `tauri-driver` URL (`http://127.0.0.1:<port>/`).
 *
 * @throws Error when the URL has no port.
 */
export function portOf(url: string): number {
  const port = Number(new URL(url).port);
  if (!Number.isSafeInteger(port) || port <= 0) {
    throw new Error(`tauri-driver's URL has no port: ${url}`);
  }
  return port;
}

/** The internals of one launch. */
class Launch implements FlowApp {
  readonly driver: WebDriver;
  readonly hook: HookClient;
  readonly settings = harnessSettings();
  readonly folders: FlowFolders;
  readonly launch: number;
  readonly #driverProcess: TauriDriver;
  #end: AppEnd = 'running';
  #pid: number | null = null;

  constructor(
    session: WebDriver,
    driverProcess: TauriDriver,
    folders: FlowFolders,
    launch: number,
  ) {
    this.driver = session;
    this.hook = new HookClient(session);
    this.#driverProcess = driverProcess;
    this.folders = folders;
    this.launch = launch;
  }

  get root(): string {
    return this.folders.root;
  }

  get end(): AppEnd {
    return this.#end;
  }

  async pid(): Promise<number> {
    if (this.#pid !== null) {
      return this.#pid;
    }
    const query = {
      executable: this.settings.app,
      marker: `B2C_E2E_ROOT=${this.folders.profile}`,
      driverPort: portOf(this.#driverProcess.url),
    };
    const pid = await waitFor(() => findAppProcess(query), {
      timeout: FIND_TIMEOUT_MS,
      interval: 250,
      message: `the app's process (${this.settings.app})`,
    });
    this.#pid = pid;
    return pid;
  }

  async quit(): Promise<void> {
    if (this.#end !== 'running') {
      return;
    }
    const pid = await this.pid().catch(() => null);
    this.#end = 'quit';
    await withTimeout(this.driver.quit(), QUIT_TIMEOUT_MS, 'the WebDriver session to end').catch(
      (error: unknown) => {
        process.stderr.write(`The WebDriver session did not end cleanly: ${String(error)}\n`);
      },
    );
    if (pid !== null) {
      await waitForProcessEnd(pid, QUIT_EXIT_TIMEOUT_MS);
    }
    // Ends whatever is left of this launch (on Linux the app's processes, on Windows the job).
    await this.stopDriver();
  }

  async kill(): Promise<void> {
    if (this.#end !== 'running') {
      throw new Error(`The app is not running (${this.#end})`);
    }
    const pid = await this.pid();
    this.#end = 'killed';
    await crashProcess(pid);
    if (!(await waitForProcessEnd(pid, EXIT_TIMEOUT_MS))) {
      throw new Error(`The app (process ${String(pid)}) still runs after it was killed`);
    }
    await this.stopDriver();
  }

  async closeWindow(): Promise<void> {
    if (this.#end !== 'running') {
      throw new Error(`The app is not running (${this.#end})`);
    }
    await requestWindowClose(await this.pid());
  }

  async waitForExit(timeout = EXIT_TIMEOUT_MS): Promise<void> {
    if (this.#end !== 'running') {
      return;
    }
    const pid = await this.pid();
    if (!(await waitForProcessEnd(pid, timeout))) {
      throw new Error(
        `The app (process ${String(pid)}) did not exit within ${String(timeout / 1000)} s`,
      );
    }
    this.#end = 'exited';
    // The session's browser is gone; ending the session would only wait for an answer.
    await this.stopDriver();
  }

  /** Stops `tauri-driver` and, on Linux, whatever of this launch is still running. */
  async stopDriver(): Promise<void> {
    await this.#driverProcess.stop().catch((error: unknown) => {
      process.stderr.write(`tauri-driver did not stop cleanly: ${String(error)}\n`);
    });
  }
}

/** Saves what helps to understand a failure (each part bounded in time and on its own). */
async function saveFailureArtifacts(app: Launch, dir: string): Promise<void> {
  const prefix = `launch-${String(app.launch)}-`;
  const attempts: [string, () => Promise<void>][] = [
    [
      'screenshot',
      async () => {
        const png = Buffer.from(await app.driver.takeScreenshot(), 'base64');
        writeArtifact(dir, `${prefix}screenshot.png`, png);
      },
    ],
    [
      'page',
      async () => {
        writeArtifact(dir, `${prefix}page.html`, await app.driver.getPageSource());
      },
    ],
    [
      'console',
      async () => {
        writeArtifact(dir, `${prefix}console.txt`, await app.hook.consoleText());
      },
    ],
    [
      'project',
      async () => {
        writeArtifact(dir, `${prefix}project.b2c`, await app.hook.documentText());
      },
    ],
  ];
  for (const [what, attempt] of attempts) {
    try {
      await withTimeout(attempt(), ARTIFACT_TIMEOUT_MS, `the ${what}`);
    } catch (error: unknown) {
      writeArtifact(dir, `${prefix}${what}.error.txt`, String(error));
    }
  }
}

/** Records the Trusted Types counts of a running app, as `launchApp` does after every test. */
async function reportTrustedTypes(app: Launch, test: string): Promise<void> {
  const before = await app.hook.trustedTypes();
  let policyActive: boolean | null;
  try {
    await app.hook.probeTrustedTypes();
    const after = await waitFor(
      async () => {
        const report = await app.hook.trustedTypes();
        return report.count > before.count ? report : null;
      },
      { timeout: 2_000, interval: 100, message: 'the probe' },
    ).catch(() => null);
    policyActive = after?.directives.includes('require-trusted-types-for') === true;
  } catch {
    policyActive = null;
  }
  recordTrustedTypes(app.settings.artifacts, {
    test,
    platform: process.platform,
    count: before.count,
    directives: before.directives,
    policyActive,
  });
}

/**
 * The app launches of one test with a shared profile; see the module comment. Create it with
 * {@link startFlow}.
 */
export class FlowSession {
  readonly folders: FlowFolders;
  readonly #test: string;
  #launches = 0;
  #current: Launch | null = null;

  private constructor(test: string, folders: FlowFolders) {
    this.#test = test;
    this.folders = folders;
  }

  /** Creates the test's folders and registers the clean-up when the test finishes. */
  static create(context: TestContext): FlowSession {
    const root = mkdtempSync(path.join(tmpdir(), 'b2c-e2e-flow-'));
    const folders: FlowFolders = {
      root,
      profile: path.join(root, 'profile'),
      projects: path.join(root, 'projects'),
    };
    mkdirSync(folders.projects);
    const session = new FlowSession(context.task.name, folders);
    context.onTestFinished(async ({ task }) => {
      await session.finish(task.result?.state === 'fail');
    }, FINISH_TIMEOUT_MS);
    return session;
  }

  /** The app that runs now, or `null`. */
  get current(): FlowApp | null {
    return this.#current?.end === 'running' ? this.#current : null;
  }

  /**
   * Starts the app with the test's profile and waits until its window is ready. The app of an
   * earlier launch must have ended (quit, exited or killed).
   */
  async launch(options: FlowLaunchOptions = {}): Promise<FlowApp> {
    if (this.#current?.end === 'running') {
      throw new Error('The app of the previous launch still runs: end it first');
    }
    if (this.#launches >= MAX_LAUNCHES) {
      throw new Error(`A flow test may launch the app at most ${String(MAX_LAUNCHES)} times`);
    }
    const settings = harnessSettings();
    if (!existsSync(settings.app)) {
      throw new Error(
        `The app under test is missing: ${settings.app}. Build it first (see apps/desktop/e2e/README.md) or set B2C_E2E_APP.`,
      );
    }
    this.#launches += 1;
    const launch = this.#launches;
    const dialogs = path.join(this.folders.root, `dialogs-${String(launch)}.json`);
    writeFileSync(dialogs, JSON.stringify(options.dialogs ?? {}));
    const env = {
      ...appEnvironment(settings, { root: this.folders.profile, dialogs }),
      ...checkedOverrides(options.env ?? {}),
    };

    let driverProcess: TauriDriver | null = null;
    let session: WebDriver | null = null;
    try {
      driverProcess = await TauriDriver.start({
        command: settings.tauriDriver,
        nativeDriver: settings.nativeDriver,
        env,
        logFile: path.join(this.folders.root, `tauri-driver-${String(launch)}.log`),
        marker: `B2C_E2E_ROOT=${this.folders.profile}`,
      });
      const capabilities = new Capabilities();
      capabilities.set('tauri:options', { application: settings.app });
      capabilities.setBrowserName('wry');
      session = await new Builder()
        .usingServer(driverProcess.url)
        .withCapabilities(capabilities)
        .build();
      await session.manage().setTimeouts({ script: 30_000, implicit: 0, pageLoad: 60_000 });
    } catch (error: unknown) {
      if (session !== null) {
        await withTimeout(session.quit(), QUIT_TIMEOUT_MS, 'the session to end').catch(
          () => undefined,
        );
      }
      await driverProcess?.stop().catch(() => undefined);
      throw new Error(
        `The app under test did not start (launch ${String(launch)}): ${error instanceof Error ? error.message : String(error)}`,
        { cause: error },
      );
    }
    const app = new Launch(session, driverProcess, this.folders, launch);
    this.#current = app;
    await waitUntilReady(app);
    return app;
  }

  /** Saves a failed test's artifacts, reports the Trusted Types counts, ends the app, cleans up. */
  async finish(failed: boolean): Promise<void> {
    const app = this.#current;
    const settings = harnessSettings();
    const dir = failed ? artifactDir(settings.artifacts, this.#test) : null;
    try {
      if (app !== null && app.end === 'running') {
        if (dir !== null) {
          await saveFailureArtifacts(app, dir);
        }
        await withTimeout(
          reportTrustedTypes(app, this.#test),
          TRUSTED_TYPES_TIMEOUT_MS,
          'the counts',
        ).catch((error: unknown) => {
          process.stderr.write(`The Trusted Types counts could not be read: ${String(error)}\n`);
        });
        await app.quit();
      } else {
        await app?.stopDriver();
      }
    } finally {
      if (dir !== null) {
        try {
          copyFiles(path.join(this.folders.profile, 'state', 'logs'), dir, 'app-');
          copyFiles(this.folders.root, dir, 'driver-');
        } catch (error: unknown) {
          process.stderr.write(`The test's logs could not be saved: ${String(error)}\n`);
        }
      }
      await removeFolder(this.folders.root);
    }
  }
}

/** Starts a flow test: its folders, and the app launches it makes; see {@link FlowSession}. */
export function startFlow(context: TestContext): FlowSession {
  return FlowSession.create(context);
}
