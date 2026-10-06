/**
 * Launching the app under test (docs/adr/0009-e2e-tooling-and-test-seams.md): each test gets a
 * fresh temporary profile (`B2C_E2E_ROOT`), a dialog script (`B2C_E2E_DIALOGS`, empty by default,
 * so every native dialog is cancelled instead of waiting for a person), the toolchain folders
 * (`B2C_E2E_TOOLCHAIN_DIRS`), its own `tauri-driver` and a WebDriver session. When the test ends
 * the app is closed and the profile deleted; a failed test first saves its artifacts.
 */
import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { Builder, Capabilities, type WebDriver } from 'selenium-webdriver';
import type { TestContext } from 'vitest';

import { artifactDir, copyFiles, recordTrustedTypes, writeArtifact } from './artifacts';
import { TauriDriver } from './driver';
import { appEnvironment, harnessSettings, type HarnessSettings } from './env';
import { HookClient } from './hook';
import { sleep, waitFor, withTimeout } from './wait';

/** How long the window may take to start (02 §2.5.7 start-up, backend and features). */
export const READY_TIMEOUT_MS = 30_000;

/**
 * How long ending the WebDriver session (which closes the app) may take before the driver and
 * whatever it started are stopped anyway. selenium-webdriver's requests have no timeout of their
 * own, so a hung app or native driver would otherwise hold the test until its hook times out.
 */
export const QUIT_TIMEOUT_MS = 15_000;

/** How long each artifact of a failed test (screenshot, page, …) may take to read. */
const ARTIFACT_TIMEOUT_MS = 10_000;

/** How long reading and probing the Trusted Types counts may take. */
const TRUSTED_TYPES_TIMEOUT_MS = 10_000;

/** How long the clean-up after a test may take in all (Vitest's hook timeout). */
const FINISH_TIMEOUT_MS = 120_000;

/** What the native dialogs answer (src-tauri/src/e2e.rs `DialogScript`). */
export interface DialogScript {
  readonly open?: readonly (string | null)[];
  readonly saveAs?: readonly (string | null)[];
  readonly pickCompiler?: readonly (string | null)[];
  readonly trust?: readonly ('trustProject' | 'trustFolder' | 'stayRestricted')[];
}

/** Options of {@link launchApp}. */
export interface LaunchOptions {
  /** The dialog script; none answers every dialog with Cancel. */
  readonly dialogs?: DialogScript;
}

/** The app under test. */
export interface App {
  readonly driver: WebDriver;
  readonly hook: HookClient;
  readonly settings: HarnessSettings;
  /** The temporary folder of this test (the profile is `profile/` in it). */
  readonly root: string;
}

/** Waits up to `timeout` for the window to start and the hook to answer. */
export async function waitUntilReady(app: App, timeout = READY_TIMEOUT_MS): Promise<void> {
  await waitFor(() => app.hook.ready(), {
    timeout,
    interval: 250,
    message: 'the window to start (window.__B2C_E2E__.ready())',
  });
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
 * Saves what helps to understand a failure (best effort: each part on its own, and each bounded
 * in time, so that a hung app cannot keep the clean-up from closing it).
 */
async function saveFailureArtifacts(app: App, test: string): Promise<void> {
  const dir = artifactDir(app.settings.artifacts, test);
  const attempts: [string, () => Promise<void>][] = [
    [
      'screenshot',
      async () => {
        writeArtifact(
          dir,
          'screenshot.png',
          Buffer.from(await app.driver.takeScreenshot(), 'base64'),
        );
      },
    ],
    [
      'page',
      async () => {
        writeArtifact(dir, 'page.html', await app.driver.getPageSource());
      },
    ],
    [
      'console',
      async () => {
        writeArtifact(dir, 'console.txt', await app.hook.consoleText());
      },
    ],
    [
      'project',
      async () => {
        writeArtifact(dir, 'project.b2c', await app.hook.documentText());
      },
    ],
  ];
  for (const [what, attempt] of attempts) {
    try {
      await withTimeout(attempt(), ARTIFACT_TIMEOUT_MS, `the ${what}`);
    } catch (error: unknown) {
      writeArtifact(dir, `${what}.error.txt`, String(error));
    }
  }
}

/** The name of `tauri-driver`'s log in the test's folder (`driver-` is put in front when saved). */
const TAURI_DRIVER_LOG = 'tauri-driver.log';

/** Where an app that did not start left its logs, for the error message ('' when nowhere). */
function launchLogsNote(dir: string | null): string {
  if (dir === null) {
    return '';
  }
  const driverLog = path.join(dir, `driver-${TAURI_DRIVER_LOG}`);
  return existsSync(driverLog)
    ? `\nThe driver's log is ${driverLog}; the app's logs, if any, are next to it.`
    : `\nThe logs, if any, are in ${dir}.`;
}

/**
 * Copies the app's and the driver's logs (`root` is the test's folder) into the test's artifact
 * folder, and returns that folder.
 */
function saveLogs(artifacts: string, root: string, test: string): string {
  const dir = artifactDir(artifacts, test);
  copyFiles(path.join(root, 'profile', 'state', 'logs'), dir, 'app-');
  copyFiles(root, dir, 'driver-');
  return dir;
}

/**
 * Reads the Trusted Types counts of the test, then probes whether the report-only policy is in
 * force (one deliberate violation), and appends both to the report.
 */
async function reportTrustedTypes(app: App, test: string): Promise<void> {
  const before = await app.hook.trustedTypes();
  let policyActive: boolean | null;
  try {
    await app.hook.probeTrustedTypes();
    // Violation events are delivered in a later task.
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
 * Starts the app for the test `context` and waits until its window is ready. It is closed (and on
 * failure its artifacts saved) when the test finishes.
 */
export async function launchApp(context: TestContext, options: LaunchOptions = {}): Promise<App> {
  const settings = harnessSettings();
  if (!existsSync(settings.app)) {
    throw new Error(
      `The app under test is missing: ${settings.app}. Build it first (see apps/desktop/e2e/README.md) or set B2C_E2E_APP.`,
    );
  }
  const root = mkdtempSync(path.join(tmpdir(), 'b2c-e2e-'));
  const profile = path.join(root, 'profile');
  const dialogs = path.join(root, 'dialogs.json');
  writeFileSync(dialogs, JSON.stringify(options.dialogs ?? {}));

  const test = context.task.name;
  let driverProcess: TauriDriver | null = null;
  let driver: WebDriver | null = null;
  /**
   * Ends the session (which closes the app), stops the driver and whatever it started, and deletes
   * the folder. A session that does not end within {@link QUIT_TIMEOUT_MS} (a hung app or native
   * driver) or fails to end is left to the driver's stop, which on Linux kills the app's
   * processes too; every step runs whatever happened before it.
   */
  const close = async (beforeRemoving?: () => void): Promise<void> => {
    const session = driver;
    driver = null;
    if (session !== null) {
      await withTimeout(session.quit(), QUIT_TIMEOUT_MS, 'the WebDriver session to end').catch(
        (error: unknown) => {
          process.stderr.write(`The WebDriver session did not end cleanly: ${String(error)}\n`);
        },
      );
    }
    await driverProcess?.stop().catch((error: unknown) => {
      process.stderr.write(`tauri-driver did not stop cleanly: ${String(error)}\n`);
    });
    try {
      beforeRemoving?.();
    } catch (error: unknown) {
      process.stderr.write(`The test's logs could not be saved: ${String(error)}\n`);
    }
    await removeFolder(root);
  };

  let app: App;
  try {
    driverProcess = await TauriDriver.start({
      command: settings.tauriDriver,
      nativeDriver: settings.nativeDriver,
      env: appEnvironment(settings, { root: profile, dialogs }),
      logFile: path.join(root, TAURI_DRIVER_LOG),
      marker: `B2C_E2E_ROOT=${profile}`,
    });
    const capabilities = new Capabilities();
    capabilities.set('tauri:options', { application: settings.app });
    capabilities.setBrowserName('wry');
    const session = await new Builder()
      .usingServer(driverProcess.url)
      .withCapabilities(capabilities)
      .build();
    driver = session;
    await session.manage().setTimeouts({ script: 30_000, implicit: 0, pageLoad: 60_000 });
    app = { driver: session, hook: new HookClient(session), settings, root };
  } catch (error: unknown) {
    // No session: keep what the driver and the app wrote (the native driver's output is in
    // tauri-driver.log), which is all there is to tell why. The test's folder is deleted, so the
    // message names the copies.
    let saved: string | null = null;
    await close(() => {
      saved = saveLogs(settings.artifacts, root, test);
    });
    throw new Error(
      `The app under test did not start: ${error instanceof Error ? error.message : String(error)}${launchLogsNote(saved)}`,
      { cause: error },
    );
  }

  context.onTestFinished(async ({ task }) => {
    const failed = task.result?.state === 'fail';
    try {
      if (failed) {
        await saveFailureArtifacts(app, test);
      }
      await withTimeout(
        reportTrustedTypes(app, test),
        TRUSTED_TYPES_TIMEOUT_MS,
        'the counts',
      ).catch((error: unknown) => {
        process.stderr.write(`The Trusted Types counts could not be read: ${String(error)}\n`);
      });
    } finally {
      await close(
        failed
          ? () => {
              saveLogs(settings.artifacts, root, test);
            }
          : undefined,
      );
    }
  }, FINISH_TIMEOUT_MS);

  await waitUntilReady(app);
  return app;
}
