/**
 * `waitUntilReady`: how long it waits by default, and what its timeout says about how far the
 * window's start got. The hook is a fake that answers as WebDriver did in nightly 17 on Windows:
 * one call into the page held for 34.2 s by a navigation in progress.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { type App, READY_TIMEOUT_MS, waitUntilReady } from './app';
import { WaitTimeoutError } from './wait';

/** What one call of the hook's `ready()` does: throw, or answer after `delay` milliseconds. */
type Answer = { readonly error: string } | { readonly ready: boolean; readonly delay?: number };

/** An app whose hook gives `answers` in order, then the last one again and again. */
function appAnswering(...answers: Answer[]): App {
  let next = 0;
  const call = vi.fn(async (method: string) => {
    expect(method).toBe('ready');
    const answer = answers[Math.min(next, answers.length - 1)];
    next += 1;
    if (answer === undefined || 'error' in answer) {
      throw new Error(answer?.error ?? 'no answer');
    }
    await new Promise((resolve) => setTimeout(resolve, answer.delay ?? 0));
    return answer.ready;
  });
  return { hook: { call } } as unknown as App;
}

/** The error `promise` rejects with, while the fake clock runs on. */
async function failure(promise: Promise<void>): Promise<unknown> {
  const caught = promise.then(
    () => null,
    (error: unknown) => error,
  );
  await vi.runAllTimersAsync();
  return caught;
}

const NOT_INSTALLED =
  'The end-to-end hook is not installed: is this the e2e build (pnpm build:e2e)?';

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

/**
 * Nightly 17: the hook was not there yet, then one call waited 34.2 s for the page to load and
 * found the app still starting.
 */
function nightly17(): App {
  return appAnswering(
    { error: NOT_INSTALLED },
    { ready: false, delay: 34_200 },
    { ready: false },
    { ready: true },
  );
}

describe('waitUntilReady', () => {
  it('waits as long as a page load may take', async () => {
    const waiting = waitUntilReady(nightly17()).then(
      () => 'ready',
      (error: unknown) => error,
    );
    await vi.advanceTimersByTimeAsync(36_000);
    expect(await waiting).toBe('ready');
    expect(READY_TIMEOUT_MS).toBe(60_000);
  });

  it('says how late the page loaded when that was after the timeout', async () => {
    // The 30 s the harness allowed before.
    const error = await failure(waitUntilReady(nightly17(), 30_000));
    expect(error).toBeInstanceOf(WaitTimeoutError);
    expect((error as Error).message).toBe(
      'Timed out after 30.0 s waiting for the window to start: the page loaded and window.__B2C_E2E__ answered after 34.5 s, but ready() stayed false (the backend or the features did not start)',
    );
  });

  it('says when the hook never answered, with the last error', async () => {
    const error = await failure(waitUntilReady(appAnswering({ error: NOT_INSTALLED }), 5_000));
    expect(error).toBeInstanceOf(WaitTimeoutError);
    expect((error as Error).message).toBe(
      `Timed out after 5.0 s waiting for the window to start: window.__B2C_E2E__ never answered (the page did not load, or this is not the end-to-end build) (last error: ${NOT_INSTALLED})`,
    );
  });

  it('says when the page loaded but the app never became ready', async () => {
    const app = appAnswering({ error: NOT_INSTALLED }, { ready: false, delay: 3_350 });
    const error = await failure(waitUntilReady(app, 10_000));
    expect(error).toBeInstanceOf(WaitTimeoutError);
    expect((error as Error).message).toBe(
      'Timed out after 10.0 s waiting for the window to start: the page loaded and window.__B2C_E2E__ answered after 3.6 s, but ready() stayed false (the backend or the features did not start)',
    );
  });
});
