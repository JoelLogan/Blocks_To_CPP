/**
 * What a flow test may change about the app's launch, and the one failure a launch is retried
 * after: which errors count (messages as msedgedriver and WebKitWebDriver write them), the retry
 * itself with fake attempts, the files and messages of each attempt, and launches whose session the
 * fake `tauri-driver` (../../../support/testing.ts) refuses.
 */
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { error as webdriver } from 'selenium-webdriver';
import { afterEach, describe, expect, it, type TestContext, vi } from 'vitest';

import { TauriDriverError } from '../../../support/driver';
import { testProcesses } from '../../../support/processes';
import { writeFakeDriver } from '../../../support/testing';
import {
  attemptFiles,
  checkedOverrides,
  isRendererAttachFailure,
  isRetryableLaunchFailure,
  NewSessionError,
  notStartedMessage,
  portOf,
  RETRY_DELAY_MS,
  retryNote,
  retryOnce,
  startFlow,
} from './launch';

/** msedgedriver's answer to the session request in the Windows flow launch of Nightly 2026-10-19. */
const RENDERER_ERROR = 'session not created: from disconnected: unable to connect to renderer';

describe('checkedOverrides', () => {
  it('lets a test set the toolchain folders, the log level and the GTK theme', () => {
    expect(
      checkedOverrides({
        B2C_E2E_TOOLCHAIN_DIRS: '/tmp/x',
        B2C_LOG: 'debug',
        GTK_THEME: 'Adwaita:dark',
      }),
    ).toEqual({
      B2C_E2E_TOOLCHAIN_DIRS: '/tmp/x',
      B2C_LOG: 'debug',
      GTK_THEME: 'Adwaita:dark',
    });
    expect(checkedOverrides({})).toEqual({});
  });

  it('refuses the profile, the dialog script and anything else', () => {
    for (const name of ['B2C_E2E_ROOT', 'B2C_E2E_DIALOGS', 'B2C_E2E_APP', 'PATH', '__proto__']) {
      expect(() => checkedOverrides({ [name]: 'x' }), name).toThrow(`may not set ${name}`);
    }
  });
});

describe('portOf', () => {
  it('reads the port of a tauri-driver URL', () => {
    expect(portOf('http://127.0.0.1:43375/')).toBe(43375);
  });

  it('refuses a URL without a port', () => {
    expect(() => portOf('http://127.0.0.1/')).toThrow('no port');
  });
});

describe('isRendererAttachFailure', () => {
  it('knows msedgedriver failing to attach to the renderer, whatever the error type', () => {
    const errors = [
      new webdriver.SessionNotCreatedError(RENDERER_ERROR),
      new webdriver.SessionNotCreatedError(
        `${RENDERER_ERROR}\n  (Session info: MicrosoftEdge=154.0.4258.62)`,
      ),
      new webdriver.SessionNotCreatedError(
        'session not created\nfrom disconnected: Unable to receive message from renderer',
      ),
      new webdriver.SessionNotCreatedError(
        'session not created: from disconnected: not connected to DevTools',
      ),
      new webdriver.WebDriverError('disconnected: unable to connect to renderer'),
      new Error(RENDERER_ERROR),
    ];
    for (const error of errors) {
      expect(isRendererAttachFailure(error), error.message).toBe(true);
    }
  });

  it('leaves out timeouts, missing or mismatched browsers and WebKitWebDriver', () => {
    const errors = [
      // msedgedriver
      new webdriver.TimeoutError('timeout: Timed out receiving message from renderer: 10.000'),
      new webdriver.SessionNotCreatedError(
        'session not created: from timeout: Timed out receiving message from renderer: 600.000',
      ),
      new webdriver.SessionNotCreatedError('session not created: cannot find MSEdge binary'),
      new webdriver.SessionNotCreatedError(
        'session not created: This version of Microsoft Edge WebDriver only supports Microsoft Edge version 153\nCurrent browser version is 154.0.4258.62 with binary path C:\\Program Files (x86)\\Microsoft\\EdgeWebView\\Application\\154.0.4258.62\\msedgewebview2.exe',
      ),
      new webdriver.SessionNotCreatedError(
        'session not created: MSEdge failed to start: exited normally.',
      ),
      new webdriver.WebDriverError('unknown error: MSEdge failed to start: crashed.'),
      // WebKitWebDriver
      new webdriver.SessionNotCreatedError('Failed to match capabilities'),
      new webdriver.SessionNotCreatedError(
        'Failed to connect to browser: Failed to execute child process “/opt/b2c/blocks2cpp-desktop” (No such file or directory)',
      ),
      // tauri-driver, and no driver at all
      new TauriDriverError(
        'tauri-driver (tauri-driver) ended before it answered (exit code 1). Its output:\ncan not find the supplied binary path',
      ),
      new Error('connect ECONNREFUSED 127.0.0.1:4444'),
    ];
    for (const error of errors) {
      expect(isRendererAttachFailure(error), error.message).toBe(false);
    }
    expect(isRendererAttachFailure(undefined)).toBe(false);
    expect(isRendererAttachFailure(null)).toBe(false);
  });
});

describe('isRetryableLaunchFailure', () => {
  it('retries only a session that was not created because the renderer was out of reach', () => {
    const attach = new webdriver.SessionNotCreatedError(RENDERER_ERROR);
    expect(isRetryableLaunchFailure(new NewSessionError(attach))).toBe(true);
    expect(
      isRetryableLaunchFailure(
        new NewSessionError(new webdriver.SessionNotCreatedError('Failed to match capabilities')),
      ),
    ).toBe(false);
    // The same words from another step of the launch (tauri-driver quoting its output, a session
    // that did not take its timeouts) are not a session that was not created.
    expect(isRetryableLaunchFailure(attach)).toBe(false);
    expect(
      isRetryableLaunchFailure(
        new TauriDriverError(
          `tauri-driver ended before it answered. Its output:\n${RENDERER_ERROR}`,
        ),
      ),
    ).toBe(false);
  });

  it('keeps the driver message of a session that was not created', () => {
    const cause = new webdriver.SessionNotCreatedError(RENDERER_ERROR);
    const error = new NewSessionError(cause);
    expect(error.message).toBe(RENDERER_ERROR);
    expect(error.cause).toBe(cause);
  });
});

describe('retryOnce', () => {
  /** The options of a retry that notes what happens in `events` and waits for nothing. */
  function noting(events: unknown[], retryable: (error: unknown) => boolean) {
    return {
      retryable,
      onRetry: (error: unknown) => {
        events.push(error);
      },
      delay: 2_000,
      wait: (ms: number) => {
        events.push(`wait ${String(ms)}`);
        return Promise.resolve();
      },
    };
  }

  it('returns the first attempt that succeeds, without waiting', async () => {
    const events: unknown[] = [];
    const result = await retryOnce(
      (attempt) => {
        events.push(`attempt ${String(attempt)}`);
        return Promise.resolve(attempt);
      },
      noting(events, () => true),
    );
    expect(result).toBe(1);
    expect(events).toEqual(['attempt 1']);
  });

  it('makes a second attempt after a retryable error, once told why and after the delay', async () => {
    const first = new Error('first');
    const events: unknown[] = [];
    const result = await retryOnce(
      (attempt) => {
        events.push(`attempt ${String(attempt)}`);
        return attempt === 1 ? Promise.reject(first) : Promise.resolve('second');
      },
      noting(events, (error) => error === first),
    );
    expect(result).toBe('second');
    expect(events).toEqual(['attempt 1', first, 'wait 2000', 'attempt 2']);
  });

  it('fails at once with an error that is not retryable', async () => {
    const first = new Error('first');
    const events: unknown[] = [];
    await expect(
      retryOnce(
        (attempt) => {
          events.push(`attempt ${String(attempt)}`);
          return Promise.reject(first);
        },
        noting(events, () => false),
      ),
    ).rejects.toBe(first);
    expect(events).toEqual(['attempt 1']);
  });

  it('never makes a third attempt: the second error is the outcome', async () => {
    const first = new Error('first');
    const second = new Error('second');
    const events: unknown[] = [];
    await expect(
      retryOnce(
        (attempt) => {
          events.push(`attempt ${String(attempt)}`);
          return Promise.reject(attempt === 1 ? first : second);
        },
        noting(events, () => true),
      ),
    ).rejects.toBe(second);
    expect(events).toEqual(['attempt 1', first, 'wait 2000', 'attempt 2']);
  });
});

describe('attemptFiles', () => {
  it('gives the second attempt of a launch a web view folder and a driver log of its own', () => {
    const root = path.join(tmpdir(), 'b2c-e2e-flow-x');
    expect(attemptFiles(root, 3, 1)).toEqual({
      webviewData: path.join(root, 'webview-3'),
      driverLog: path.join(root, 'tauri-driver-3.log'),
    });
    expect(attemptFiles(root, 3, 2)).toEqual({
      webviewData: path.join(root, 'webview-3-retry'),
      driverLog: path.join(root, 'tauri-driver-3-retry.log'),
    });
  });
});

describe('notStartedMessage and retryNote', () => {
  it('names the error of the one attempt, or of both', () => {
    expect(notStartedMessage(2, [new Error('no session')])).toBe(
      'The app under test did not start (launch 2): no session',
    );
    expect(notStartedMessage(2, [new Error('first'), 'second'])).toBe(
      'The app under test did not start (launch 2, retried once). First attempt: first\nSecond attempt: second',
    );
  });

  it('says on one line that a launch is retried, and why', () => {
    const error = new webdriver.SessionNotCreatedError(
      `${RENDERER_ERROR}\n  (Session info: MicrosoftEdge=154.0.4258.62)`,
    );
    expect(retryNote(4, error)).toBe(
      `Launch 4 is retried once: the native driver could not attach to the web view (${RENDERER_ERROR} (Session info: MicrosoftEdge=154.0.4258.62))`,
    );
  });
});

/** The clean-up a flow registers for the end of its test. */
type FinishFlow = (result: { task: { result: { state: string } } }) => Promise<void>;

const folders: string[] = [];
const finishes: FinishFlow[] = [];

afterEach(async () => {
  for (const finish of finishes.splice(0)) {
    await finish({ task: { result: { state: 'pass' } } });
  }
  vi.unstubAllEnvs();
  vi.restoreAllMocks();
  for (const folder of folders.splice(0)) {
    rmSync(folder, { recursive: true, force: true });
  }
});

describe.skipIf(process.platform !== 'linux')('FlowSession.launch', () => {
  /**
   * A flow whose sessions the fake driver refuses with `message`: every one, or with `refuse-once`
   * only the first, after which it stands in for the app.
   */
  function refusedFlow(message: string, mode: 'refuse' | 'refuse-once' = 'refuse') {
    const folder = mkdtempSync(path.join(tmpdir(), 'b2c-e2e-flow-launch-'));
    folders.push(folder);
    vi.stubEnv('B2C_E2E_APP', process.execPath);
    vi.stubEnv('B2C_E2E_TAURI_DRIVER', writeFakeDriver(folder));
    vi.stubEnv('B2C_E2E_NATIVE_DRIVER', '');
    vi.stubEnv('B2C_E2E_NATIVE_DRIVER_LOG', '');
    vi.stubEnv('B2C_E2E_ARTIFACTS', path.join(folder, 'artifacts'));
    vi.stubEnv('FAKE_MODE', mode);
    vi.stubEnv('FAKE_PIDS', folder);
    vi.stubEnv('FAKE_SESSION_ERROR', message);
    const context = {
      task: { name: 'A refused launch' },
      onTestFinished: (finish: FinishFlow) => {
        finishes.push(finish);
      },
    };
    return startFlow(context as unknown as TestContext);
  }

  /** The retry notes written to standard error. */
  function retryNotes(stderr: { mock: { calls: unknown[][] } }): string[] {
    return stderr.mock.calls
      .map(([text]) => String(text))
      .filter((text) => text.startsWith('Launch '));
  }

  it('retries a launch once when the native driver cannot attach to the web view', async () => {
    const stderr = vi.spyOn(process.stderr, 'write').mockImplementation(() => true);
    const flow = refusedFlow(RENDERER_ERROR);
    const start = Date.now();
    await expect(flow.launch()).rejects.toThrow(
      `The app under test did not start (launch 1, retried once). First attempt: ${RENDERER_ERROR}\nSecond attempt: ${RENDERER_ERROR}`,
    );
    expect(Date.now() - start).toBeGreaterThanOrEqual(RETRY_DELAY_MS);
    expect(retryNotes(stderr)).toEqual([
      `Launch 1 is retried once: the native driver could not attach to the web view (${RENDERER_ERROR})\n`,
    ]);
    // Each attempt gave the app a web view folder of its own and kept its own driver log, and
    // nothing either attempt started still runs.
    const { root, profile } = flow.folders;
    expect(readFileSync(path.join(root, 'tauri-driver-1.log'), 'utf8')).toContain(
      `B2C_E2E_WEBVIEW_DATA=${path.join(root, 'webview-1')}\n`,
    );
    expect(readFileSync(path.join(root, 'tauri-driver-1-retry.log'), 'utf8')).toContain(
      `B2C_E2E_WEBVIEW_DATA=${path.join(root, 'webview-1-retry')}\n`,
    );
    expect(testProcesses({ roots: [], marker: `B2C_E2E_ROOT=${profile}` })).toEqual([]);

    // Both attempts were one launch: the next is launch 2.
    vi.stubEnv('FAKE_SESSION_ERROR', 'Failed to match capabilities');
    await expect(flow.launch()).rejects.toThrow(
      /^The app under test did not start \(launch 2\): Failed to match capabilities$/,
    );
  }, 30_000);

  it('starts the app at the second attempt, as the same launch and in its own folder', async () => {
    const stderr = vi.spyOn(process.stderr, 'write').mockImplementation(() => true);
    const flow = refusedFlow(RENDERER_ERROR, 'refuse-once');
    const app = await flow.launch();
    expect(app.launch).toBe(1);
    expect(flow.current).toBe(app);
    expect(retryNotes(stderr)).toEqual([
      `Launch 1 is retried once: the native driver could not attach to the web view (${RENDERER_ERROR})\n`,
    ]);
    // The launch checked the second attempt's web view folder once the window was ready: the
    // refused first attempt made none.
    const { root, profile } = flow.folders;
    expect(existsSync(path.join(root, 'webview-1-retry'))).toBe(true);
    expect(existsSync(path.join(root, 'webview-1'))).toBe(false);
    // Quitting ends the second attempt's session, and with it the fake app.
    await app.quit();
    expect(app.end).toBe('quit');
    expect(flow.current).toBeNull();
    expect(testProcesses({ roots: [], marker: `B2C_E2E_ROOT=${profile}` })).toEqual([]);
  }, 30_000);

  it('does not retry a launch whose session fails for another reason', async () => {
    const stderr = vi.spyOn(process.stderr, 'write').mockImplementation(() => true);
    const message = 'session not created: cannot find MSEdge binary';
    const flow = refusedFlow(message);
    await expect(flow.launch()).rejects.toThrow(
      new RegExp(`^The app under test did not start \\(launch 1\\): ${message}$`),
    );
    expect(retryNotes(stderr)).toEqual([]);
    expect(existsSync(path.join(flow.folders.root, 'tauri-driver-1.log'))).toBe(true);
    expect(existsSync(path.join(flow.folders.root, 'tauri-driver-1-retry.log'))).toBe(false);
  }, 30_000);
});
