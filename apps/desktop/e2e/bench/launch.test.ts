/**
 * Timing a cold start (launch.ts): turning the page's readiness result into a cold-start time, and
 * asking a window that may not show the app's page yet, or replaces it while it is asked.
 */
import { error as webdriverError } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import {
  COLD_START_TIMEOUT_MS,
  ColdStartError,
  coldStartOf,
  isAppPage,
  isInterruption,
  isPageLook,
  isReadyResult,
  MAX_READY_WAITS,
  PAGE_LOOK_INTERVAL_MS,
  type StartContext,
  type StartingWindow,
  timeReadiness,
} from './launch';

describe('isReadyResult', () => {
  it('accepts what READY_SCRIPT calls back with', () => {
    expect(isReadyResult({ kind: 'ready', at: 1.5, already: false })).toBe(true);
    expect(isReadyResult({ kind: 'timeout' })).toBe(true);
  });

  it('refuses anything else', () => {
    expect(isReadyResult(null)).toBe(false);
    expect(isReadyResult('ready')).toBe(false);
    expect(isReadyResult({ kind: 'ready', at: '1', already: false })).toBe(false);
    expect(isReadyResult({ kind: 'ready', at: Number.NaN, already: false })).toBe(false);
    expect(isReadyResult({ kind: 'ready', at: 1 })).toBe(false);
    expect(isReadyResult({ kind: 'other' })).toBe(false);
  });
});

describe('coldStartOf', () => {
  const launched = 1_700_000_000_000;

  it('is the time from the launch to the marker', () => {
    expect(
      coldStartOf(launched, { kind: 'ready', at: launched + 1_966.5, already: false }),
    ).toEqual({ ms: 1_966.5, already: false });
    expect(coldStartOf(launched, { kind: 'ready', at: launched + 2_500, already: true })).toEqual({
      ms: 2_500,
      already: true,
    });
  });

  it('fails on a timeout, a malformed result or an implausible time', () => {
    expect(() => coldStartOf(launched, { kind: 'timeout' })).toThrow(ColdStartError);
    expect(() => coldStartOf(launched, { kind: 'timeout' })).toThrow(
      `${String(COLD_START_TIMEOUT_MS / 1000)} s`,
    );
    expect(() => coldStartOf(launched, undefined)).toThrow(/unexpected/);
    // The page's clock before the launch, or ten times the timeout after it.
    expect(() =>
      coldStartOf(launched, { kind: 'ready', at: launched - 5, already: false }),
    ).toThrow(/not plausible/);
    expect(() =>
      coldStartOf(launched, {
        kind: 'ready',
        at: launched + 10 * COLD_START_TIMEOUT_MS,
        already: false,
      }),
    ).toThrow(/not plausible/);
  });
});

describe('isPageLook', () => {
  it('accepts what PAGE_SCRIPT returns', () => {
    expect(isPageLook({ href: 'tauri://localhost/', timeOrigin: 1.5, readyAt: null })).toBe(true);
    expect(isPageLook({ href: 'about:blank', timeOrigin: 1, readyAt: 2.5 })).toBe(true);
  });

  it('refuses anything else', () => {
    expect(isPageLook(null)).toBe(false);
    expect(isPageLook({ href: 'about:blank', timeOrigin: 1 })).toBe(false);
    expect(isPageLook({ href: 1, timeOrigin: 1, readyAt: null })).toBe(false);
    expect(isPageLook({ href: 'x', timeOrigin: Number.NaN, readyAt: null })).toBe(false);
    expect(isPageLook({ href: 'x', timeOrigin: 1, readyAt: '2' })).toBe(false);
  });
});

describe('isAppPage', () => {
  it('is the bundled frontend at the platform’s app origin', () => {
    expect(isAppPage('tauri://localhost/', 'linux')).toBe(true);
    expect(isAppPage('tauri://localhost/index.html', 'linux')).toBe(true);
    expect(isAppPage('http://tauri.localhost/', 'win32')).toBe(true);
    expect(isAppPage('http://tauri.localhost/index.html?x=1', 'win32')).toBe(true);
  });

  it('is nothing else', () => {
    for (const href of ['about:blank', '', 'http://tauri.localhost/', 'tauri://localhost:1420/']) {
      expect(isAppPage(href, 'linux')).toBe(false);
    }
    for (const href of [
      'about:blank',
      'tauri://localhost/',
      'https://tauri.localhost/',
      'http://tauri.localhost:8080/',
      'http://tauri.localhost.example.com/',
    ]) {
      expect(isAppPage(href, 'win32')).toBe(false);
    }
  });
});

describe('isInterruption', () => {
  it('is a script timeout, or an error about the document going away', () => {
    expect(isInterruption(new webdriverError.ScriptTimeoutError('script timeout'))).toBe(true);
    expect(
      isInterruption(
        new webdriverError.JavascriptError(
          'javascript error: document unloaded while waiting for result',
        ),
      ),
    ).toBe(true);
    expect(
      isInterruption(new webdriverError.WebDriverError('Execution context was destroyed.')),
    ).toBe(true);
  });

  it('is no other failure', () => {
    expect(
      isInterruption(new webdriverError.JavascriptError('ReferenceError: x is not defined')),
    ).toBe(false);
    expect(isInterruption(new webdriverError.NoSuchSessionError('invalid session id'))).toBe(false);
    expect(isInterruption(new Error('document unloaded'))).toBe(false);
    expect(isInterruption('script timeout')).toBe(false);
  });
});

describe('timeReadiness', () => {
  const launched = 1_700_000_000_000;
  const deadline = launched + COLD_START_TIMEOUT_MS;
  /** How long each WebDriver call takes on the fake clock. */
  const CALL_MS = 3;

  /** What one look or wait does: answer, or fail. */
  type Step = { readonly value: unknown } | { readonly error: Error };

  /**
   * A fake window that answers looks and waits from lists (the last look repeats), on a fake clock
   * that starts `startedAfter` ms after the launch (the session's own start-up).
   */
  function fake(options: {
    readonly looks: readonly Step[];
    readonly waits?: readonly Step[];
    readonly platform?: NodeJS.Platform;
    readonly startedAfter?: number;
  }) {
    let now = launched + (options.startedAfter ?? 900);
    let looks = 0;
    const timeouts: number[] = [];
    const notes: string[] = [];
    const answer = (step: Step | undefined): Promise<unknown> => {
      now += CALL_MS;
      if (step === undefined) {
        return Promise.reject(new Error('The fake window has no more answers'));
      }
      return 'error' in step ? Promise.reject(step.error) : Promise.resolve(step.value);
    };
    const window: StartingWindow = {
      look: () => {
        const step = options.looks[Math.min(looks, options.looks.length - 1)];
        looks += 1;
        return answer(step);
      },
      waitForMarker: (timeoutMs) => {
        timeouts.push(timeoutMs);
        return answer(options.waits?.[timeouts.length - 1]);
      },
    };
    const context: StartContext = {
      now: () => now,
      sleep: (ms) => {
        now += ms;
        return Promise.resolve();
      },
      platform: options.platform ?? 'linux',
      note: (text) => {
        notes.push(text);
      },
    };
    return {
      time: () => timeReadiness(window, launched, context),
      timeouts,
      notes,
      looks: () => looks,
      now: () => now,
    };
  }

  const page = (href: string, timeOrigin: number, readyAt: number | null = null): Step => ({
    value: { href, timeOrigin, readyAt },
  });
  const ready = (at: number, already = false): Step => ({ value: { kind: 'ready', at, already } });
  const timeout = (): Step => ({ error: new webdriverError.ScriptTimeoutError('script timeout') });
  const blank = page('about:blank', launched + 500);
  const app = page('tauri://localhost/', launched + 950);

  it('looks until the window shows the app’s page, then waits there for the marker', async () => {
    const window = fake({ looks: [blank, blank, blank, app], waits: [ready(launched + 1_966.5)] });
    await expect(window.time()).resolves.toEqual({ ms: 1_966.5, already: false, restarts: 0 });
    expect(window.looks()).toBe(4);
    // The wait gets what is left of the start's time.
    expect(window.timeouts).toHaveLength(1);
    expect(window.timeouts[0]).toBeLessThan(COLD_START_TIMEOUT_MS - 900);
    expect(window.timeouts[0]).toBeGreaterThan(COLD_START_TIMEOUT_MS - 1_000);
  });

  it('takes only the app page of the platform it runs on', async () => {
    const window = fake({
      platform: 'win32',
      looks: [app, page('http://tauri.localhost/', launched + 960)],
      waits: [ready(launched + 2_000)],
    });
    await expect(window.time()).resolves.toMatchObject({ ms: 2_000, restarts: 0 });
    expect(window.looks()).toBe(2);
  });

  it('counts a marker that holds at the first look as already there', async () => {
    const window = fake({
      looks: [blank, page('tauri://localhost/', launched + 950, launched + 1_400)],
    });
    await expect(window.time()).resolves.toEqual({ ms: 1_400, already: true, restarts: 0 });
    expect(window.timeouts).toEqual([]);
  });

  it('waits again in the page that replaced the first one, timing from the launch', async () => {
    const window = fake({
      // After the cut-short wait the window still shows the old document once.
      looks: [app, app, page('tauri://localhost/', launched + 1_800)],
      waits: [timeout(), ready(launched + 2_100)],
    });
    await expect(window.time()).resolves.toEqual({ ms: 2_100, already: false, restarts: 1 });
    expect(window.timeouts).toHaveLength(2);
    expect(window.notes).toHaveLength(1);
    expect(window.notes[0]).toMatch(/cut short.*ScriptTimeoutError: script timeout/);
    expect(window.notes[0]).toContain(
      `in the page at tauri://localhost/ opened at ${new Date(launched + 950).toISOString()}`,
    );
  });

  it('keeps a restarted start an upper bound when the marker already holds', async () => {
    const unloaded = {
      error: new webdriverError.JavascriptError(
        'javascript error: document unloaded while waiting for result',
      ),
    };
    const looked = fake({
      looks: [app, page('tauri://localhost/', launched + 1_800, launched + 2_500)],
      waits: [unloaded],
    });
    await expect(looked.time()).resolves.toEqual({ ms: 2_500, already: true, restarts: 1 });
    const waited = fake({ looks: [app], waits: [unloaded, ready(launched + 2_600, true)] });
    await expect(waited.time()).resolves.toEqual({ ms: 2_600, already: true, restarts: 1 });
  });

  it('fails at once on any other failure of the readiness script', async () => {
    const window = fake({
      looks: [app],
      waits: [{ error: new webdriverError.JavascriptError('ReferenceError: x is not defined') }],
    });
    await expect(window.time()).rejects.toThrow(
      /readiness script failed: JavascriptError: ReferenceError/,
    );
    expect(window.timeouts).toHaveLength(1);
  });

  it('gives up after so many cut-short waits', async () => {
    const window = fake({
      looks: [app],
      waits: Array.from({ length: MAX_READY_WAITS + 1 }, timeout),
    });
    await expect(window.time()).rejects.toThrow(
      new RegExp(`cut short ${String(MAX_READY_WAITS)} times`),
    );
    expect(window.timeouts).toHaveLength(MAX_READY_WAITS);
  });

  it('does not wait again once the start’s time is up', async () => {
    const window = fake({
      looks: [app],
      waits: [timeout(), ready(launched + 70_000)],
      startedAfter: COLD_START_TIMEOUT_MS - CALL_MS,
    });
    await expect(window.time()).rejects.toThrow(
      /^The readiness script failed: ScriptTimeoutError: script timeout \(in the page at tauri:\/\/localhost\/ opened at [^)]*\)$/,
    );
    expect(window.timeouts).toEqual([1]);
    // After waits cut short before, the failure names them too.
    const later = fake({
      looks: [app],
      waits: [timeout(), { error: new webdriverError.JavascriptError('TypeError: oops') }],
    });
    await expect(later.time()).rejects.toThrow(
      /failed: JavascriptError: TypeError: oops .*\(earlier waits cut short: ScriptTimeoutError/,
    );
  });

  it('fails when the window never shows the app’s page, or the marker never holds', async () => {
    const window = fake({ looks: [{ error: new Error('no such window') }, blank] });
    await expect(window.time()).rejects.toThrow(
      /did not show the app's page within 60 s \(last look: a page at about:blank\)/,
    );
    expect(window.now()).toBeGreaterThanOrEqual(deadline);
    expect(window.now()).toBeLessThan(deadline + PAGE_LOOK_INTERVAL_MS + 2 * CALL_MS);

    const never = fake({ looks: [app], waits: [{ value: { kind: 'timeout' } }] });
    await expect(never.time()).rejects.toThrow(/did not reach the readiness marker/);
  });

  it('refuses a time before the launch', async () => {
    const window = fake({ looks: [page('tauri://localhost/', launched - 10, launched - 5)] });
    await expect(window.time()).rejects.toThrow(/not plausible/);
  });
});
