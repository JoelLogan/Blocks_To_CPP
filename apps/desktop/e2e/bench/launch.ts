/**
 * One cold start of the app under test, timed (docs/spec/01-overview.md §1.4 N2, 09 §9.2): from
 * the moment the WebDriver session is requested, which makes the native driver start the app,
 * until the readiness marker holds in the page ({@link READY_SCRIPT}: start page, injected
 * workspace, rendered toolbox). Each launch gets a fresh profile, its own `tauri-driver` and its
 * own clean-up, like `launchApp` (support/app.ts), which cannot be used here: it starts timing too
 * late and keeps every app it starts until the test ends.
 *
 * Right after the session starts, the window may not show the app's page yet, and it may replace
 * its document while the readiness script waits in it (WebView2's msedgedriver then reports a
 * script timeout long before the script's timeout). So the window is first looked at with short
 * synchronous scripts until it shows the app's own page, and a wait cut short by a new document is
 * started again in that document, until {@link COLD_START_TIMEOUT_MS} after the session request
 * ({@link timeReadiness}). The time is always the one from the session request to the page clock's
 * time when the marker was seen, so starting again can only make a start look slower.
 */
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { Builder, Capabilities, error as webdriverError, type WebDriver } from 'selenium-webdriver';

import { QUIT_TIMEOUT_MS } from '../support/app';
import { artifactDir, copyFiles } from '../support/artifacts';
import { TauriDriver } from '../support/driver';
import { appEnvironment, type HarnessSettings } from '../support/env';
import { sleep, withTimeout } from '../support/wait';
import { PAGE_SCRIPT, type PageLook, READY_SCRIPT, type ReadyResult } from './page';

/** How long one start may take before it counts as failed. */
export const COLD_START_TIMEOUT_MS = 60_000;

/** How long to wait between two looks at a window that does not show the app's page yet. */
export const PAGE_LOOK_INTERVAL_MS = 10;

/**
 * How many times one start may wait for the marker: once, and again each time the window replaced
 * its document while the readiness script waited.
 */
export const MAX_READY_WAITS = 5;

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

/** One timed start, with how it was taken. */
export interface TimedStart extends ColdStart {
  /**
   * How many times the wait for the marker was started again because the window replaced its
   * document (or the driver lost the readiness script) while it waited; 0 normally.
   */
  readonly restarts: number;
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

/** Whether `value` is a {@link PageLook}. */
export function isPageLook(value: unknown): value is PageLook {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const look = value as Record<string, unknown>;
  const readyAt = look['readyAt'];
  return (
    typeof look['href'] === 'string' &&
    typeof look['timeOrigin'] === 'number' &&
    Number.isFinite(look['timeOrigin']) &&
    (readyAt === null || (typeof readyAt === 'number' && Number.isFinite(readyAt)))
  );
}

/**
 * Whether `href` is the app's own page on `platform`: at the origin the bundled frontend is served
 * from (src-tauri/src/window.rs `APP_ORIGIN`), `http://tauri.localhost` on Windows and
 * `tauri://localhost` elsewhere, with no port. Nothing else is: not `about:blank`, not the other
 * platform's spelling.
 */
export function isAppPage(href: string, platform: NodeJS.Platform): boolean {
  let url: URL;
  try {
    url = new URL(href);
  } catch {
    return false;
  }
  const [protocol, host] =
    platform === 'win32' ? ['http:', 'tauri.localhost'] : ['tauri:', 'localhost'];
  return url.protocol === protocol && url.hostname === host && url.port === '';
}

/**
 * Whether the readiness script failed because its document went away while it waited (the window
 * replaced its page): msedgedriver reports that as a script timeout, long before the script's own
 * timeout, or as an error about the unloaded document or its lost execution context.
 */
export function isInterruption(error: unknown): boolean {
  if (error instanceof webdriverError.ScriptTimeoutError) {
    return true;
  }
  return (
    error instanceof webdriverError.WebDriverError &&
    /unload|navigat|execution context|detached/i.test(error.message)
  );
}

/** An error's name and the first line of its message. */
function errorText(error: unknown): string {
  if (error instanceof Error) {
    return `${error.name}: ${error.message.split('\n', 1)[0] ?? ''}`;
  }
  return String(error);
}

/** The window while the app starts, as {@link timeReadiness} asks it (a fake in unit tests). */
export interface StartingWindow {
  /** Runs {@link PAGE_SCRIPT} and returns what it returned. */
  look(): Promise<unknown>;
  /** Runs {@link READY_SCRIPT} with `timeoutMs` and returns what it called back with. */
  waitForMarker(timeoutMs: number): Promise<unknown>;
}

/** The clock, the platform and the log {@link timeReadiness} works with (fakes in unit tests). */
export interface StartContext {
  /** Epoch milliseconds, on the clock of `launched`. */
  readonly now: () => number;
  readonly sleep: (ms: number) => Promise<void>;
  readonly platform: NodeJS.Platform;
  /** Notes that a wait for the marker was cut short and starts again. */
  readonly note: (text: string) => void;
}

/**
 * Looks at the window every {@link PAGE_LOOK_INTERVAL_MS} until it shows the app's own page, and
 * returns that look. A look that fails (the document is changing) counts as "not yet".
 *
 * @throws ColdStartError when `deadline` passes first.
 */
async function lookForAppPage(
  window: StartingWindow,
  deadline: number,
  context: StartContext,
): Promise<PageLook> {
  for (;;) {
    let last: string;
    try {
      const look: unknown = await window.look();
      if (!isPageLook(look)) {
        last = 'an unexpected answer';
      } else if (isAppPage(look.href, context.platform)) {
        return look;
      } else {
        last = `a page at ${look.href}`;
      }
    } catch (error: unknown) {
      last = errorText(error);
    }
    if (context.now() >= deadline) {
      throw new ColdStartError(
        `The window did not show the app's page within ${String(COLD_START_TIMEOUT_MS / 1000)} s (last look: ${last})`,
      );
    }
    await context.sleep(PAGE_LOOK_INTERVAL_MS);
  }
}

/**
 * Times one start in `window` from `launched` (epoch ms, just before the session request): looks
 * until the window shows the app's page, then waits there for the readiness marker. A wait that the
 * window cuts short by replacing its document ({@link isInterruption}) is started again in the new
 * document, as long as {@link COLD_START_TIMEOUT_MS} has not passed since `launched` and at most
 * {@link MAX_READY_WAITS} waits in all. The time is always the one from `launched` to when the
 * marker was seen in the page. When the marker already held at the first look at a document or
 * when its wait began, the start is `already` (the time is an upper bound), restarted or not.
 *
 * @throws ColdStartError when the start fails or takes too long.
 */
export async function timeReadiness(
  window: StartingWindow,
  launched: number,
  context: StartContext,
): Promise<TimedStart> {
  const deadline = launched + COLD_START_TIMEOUT_MS;
  const interruptions: string[] = [];
  for (;;) {
    const look = await lookForAppPage(window, deadline, context);
    const restarts = interruptions.length;
    if (look.readyAt !== null) {
      const ready: ReadyResult = { kind: 'ready', at: look.readyAt, already: true };
      return { ...coldStartOf(launched, ready), restarts };
    }
    let result: unknown;
    try {
      result = await window.waitForMarker(Math.max(1, deadline - context.now()));
    } catch (error: unknown) {
      const opened = new Date(look.timeOrigin);
      const when = Number.isNaN(opened.getTime()) ? String(look.timeOrigin) : opened.toISOString();
      const what = `${errorText(error)} (in the page at ${look.href} opened at ${when})`;
      // Once the start's time is up, a script timeout is a timeout, not a replaced page.
      if (!isInterruption(error) || context.now() >= deadline) {
        const earlier =
          interruptions.length === 0
            ? ''
            : ` (earlier waits cut short: ${interruptions.join('; ')})`;
        throw new ColdStartError(`The readiness script failed: ${what}${earlier}`, {
          cause: error,
        });
      }
      interruptions.push(what);
      if (interruptions.length >= MAX_READY_WAITS) {
        throw new ColdStartError(
          `The wait for the readiness marker was cut short ${String(interruptions.length)} times: ${interruptions.join('; ')}`,
          { cause: error },
        );
      }
      context.note(`the wait for the readiness marker was cut short, waiting again: ${what}`);
      continue;
    }
    return { ...coldStartOf(launched, result), restarts };
  }
}

/** The WebDriver session's window, for {@link timeReadiness}. */
function sessionWindow(session: WebDriver): StartingWindow {
  return {
    look: () => session.executeScript(PAGE_SCRIPT),
    waitForMarker: (timeoutMs) => session.executeAsyncScript(READY_SCRIPT, timeoutMs),
  };
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
): Promise<TimedStart> {
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
    // The readiness script's own timeout (what is left of COLD_START_TIMEOUT_MS) ends it first.
    await session
      .manage()
      .setTimeouts({ script: COLD_START_TIMEOUT_MS + 10_000, implicit: 0, pageLoad: 60_000 });
    const start = await timeReadiness(sessionWindow(session), launched, {
      now: () => Date.now(),
      sleep,
      platform: process.platform,
      note: (text) => {
        process.stderr.write(`${label}: ${text}\n`);
      },
    });
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
