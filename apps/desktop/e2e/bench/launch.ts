/**
 * One cold start of the app under test, timed (docs/spec/01-overview.md §1.4 N2, 09 §9.2): from
 * the moment the WebDriver session is requested, which makes the native driver start the app,
 * until the readiness marker holds in the page ({@link READY_SCRIPT}: start page, injected
 * workspace, rendered toolbox). Each launch gets a fresh profile, its own `tauri-driver` and its
 * own clean-up, like `launchApp` (support/app.ts), which cannot be used here: it starts timing too
 * late and keeps every app it starts until the test ends.
 */
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { Builder, Capabilities, type WebDriver } from 'selenium-webdriver';

import { QUIT_TIMEOUT_MS } from '../support/app';
import { artifactDir, copyFiles } from '../support/artifacts';
import { TauriDriver } from '../support/driver';
import { appEnvironment, type HarnessSettings } from '../support/env';
import { sleep, withTimeout } from '../support/wait';
import { READY_SCRIPT, type ReadyResult } from './page';

/** How long one start may take before it counts as failed. */
export const COLD_START_TIMEOUT_MS = 60_000;

/** One timed start. */
export interface ColdStart {
  /** Milliseconds from the session request to the readiness marker. */
  readonly ms: number;
  /**
   * Whether the marker already held when the page could first be asked (the time is then an upper
   * bound: it includes WebDriver's own start-up after the window was ready).
   */
  readonly already: boolean;
}

/** A start that failed. */
export class ColdStartError extends Error {
  override readonly name = 'ColdStartError';
}

/** Whether `value` is a {@link ReadyResult}. */
export function isReadyResult(value: unknown): value is ReadyResult {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const result = value as Record<string, unknown>;
  if (result['kind'] === 'timeout') {
    return true;
  }
  return (
    result['kind'] === 'ready' &&
    typeof result['at'] === 'number' &&
    Number.isFinite(result['at']) &&
    typeof result['already'] === 'boolean'
  );
}

/**
 * The cold-start time of one launch from `launched` (epoch ms, just before the session request)
 * and the page's result.
 */
export function coldStartOf(launched: number, result: unknown): ColdStart {
  if (!isReadyResult(result)) {
    throw new ColdStartError('The readiness script returned something unexpected');
  }
  if (result.kind === 'timeout') {
    throw new ColdStartError(
      `The window did not reach the readiness marker within ${String(COLD_START_TIMEOUT_MS / 1000)} s`,
    );
  }
  const ms = result.at - launched;
  if (!(ms > 0 && ms < 10 * COLD_START_TIMEOUT_MS)) {
    throw new ColdStartError(`The measured start time ${String(ms)} ms is not plausible`);
  }
  return { ms, already: result.already };
}

/** Removes a folder; on Windows a process that just exited may still hold a file for a moment. */
async function removeFolder(folder: string): Promise<void> {
  for (let attempt = 0; attempt < 10; attempt += 1) {
    try {
      rmSync(folder, { recursive: true, force: true });
      return;
    } catch {
      await sleep(500);
    }
  }
  process.stderr.write(`Could not remove the benchmark folder ${folder}\n`);
}

/**
 * Starts the app once with a fresh profile, times the start and closes it again. On failure the
 * driver's and the app's logs are kept in the artifacts folder under `label`.
 */
export async function measureColdStart(
  settings: HarnessSettings,
  label: string,
): Promise<ColdStart> {
  const root = mkdtempSync(path.join(tmpdir(), 'b2c-bench-'));
  const profile = path.join(root, 'profile');
  const dialogs = path.join(root, 'dialogs.json');
  writeFileSync(dialogs, '{}');
  let driver: TauriDriver | null = null;
  let session: WebDriver | null = null;
  let failed = true;
  try {
    driver = await TauriDriver.start({
      command: settings.tauriDriver,
      nativeDriver: settings.nativeDriver,
      env: appEnvironment(settings, { root: profile, dialogs }),
      logFile: path.join(root, 'tauri-driver.log'),
      marker: `B2C_E2E_ROOT=${profile}`,
    });
    const capabilities = new Capabilities();
    capabilities.set('tauri:options', { application: settings.app });
    capabilities.setBrowserName('wry');
    const launched = Date.now();
    session = await new Builder().usingServer(driver.url).withCapabilities(capabilities).build();
    await session
      .manage()
      .setTimeouts({ script: COLD_START_TIMEOUT_MS + 10_000, implicit: 0, pageLoad: 60_000 });
    const result: unknown = await session.executeAsyncScript(READY_SCRIPT, COLD_START_TIMEOUT_MS);
    const start = coldStartOf(launched, result);
    failed = false;
    return start;
  } finally {
    if (session !== null) {
      await withTimeout(session.quit(), QUIT_TIMEOUT_MS, 'the WebDriver session to end').catch(
        (error: unknown) => {
          process.stderr.write(`The WebDriver session did not end cleanly: ${String(error)}\n`);
        },
      );
    }
    await driver?.stop().catch((error: unknown) => {
      process.stderr.write(`tauri-driver did not stop cleanly: ${String(error)}\n`);
    });
    if (failed) {
      const dir = artifactDir(settings.artifacts, label);
      copyFiles(path.join(profile, 'state', 'logs'), dir, 'app-');
      copyFiles(root, dir, 'driver-');
    }
    await removeFolder(root);
  }
}
