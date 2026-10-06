/** Waiting for the app: polling with a deadline, and a plain delay. */

/** Options of {@link waitFor}. */
export interface WaitOptions {
  /** How long to wait, in milliseconds. */
  readonly timeout: number;
  /** How long to wait between tries, in milliseconds (default 100). */
  readonly interval?: number;
  /** What was waited for, for the error message. */
  readonly message: string | (() => string | Promise<string>);
}

/** A wait ran out of time. */
export class WaitTimeoutError extends Error {
  override readonly name = 'WaitTimeoutError';
}

/** Resolves after `ms` milliseconds. */
export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Calls `probe` until it returns something other than `undefined`, `null` or `false`, and returns
 * that. A probe that throws counts as "not yet"; its last error is part of the timeout message.
 *
 * @throws WaitTimeoutError when `options.timeout` passes first.
 */
export async function waitFor<T>(
  probe: () => Promise<T | undefined | null | false> | T | undefined | null | false,
  options: WaitOptions,
): Promise<T> {
  const deadline = Date.now() + options.timeout;
  const interval = options.interval ?? 100;
  let lastError: unknown;
  for (;;) {
    try {
      const result = await probe();
      if (result !== undefined && result !== null && result !== false) {
        return result;
      }
      lastError = null;
    } catch (error: unknown) {
      lastError = error;
    }
    if (Date.now() >= deadline) {
      break;
    }
    await sleep(Math.min(interval, Math.max(0, deadline - Date.now())));
  }
  const what =
    typeof options.message === 'string' ? options.message : await describe(options.message);
  const seconds = (options.timeout / 1000).toFixed(1);
  const cause = lastError instanceof Error ? ` (last error: ${lastError.message})` : '';
  throw new WaitTimeoutError(`Timed out after ${seconds} s waiting for ${what}${cause}`, {
    cause: lastError ?? undefined,
  });
}

/**
 * `promise`'s result, or a {@link WaitTimeoutError} once `timeout` milliseconds have passed. The
 * promise itself is not stopped; a failure after the timeout is ignored (never unhandled).
 */
export async function withTimeout<T>(
  promise: Promise<T>,
  timeout: number,
  what: string,
): Promise<T> {
  promise.catch(() => undefined);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const expired = new Promise<never>((_resolve, reject) => {
    timer = setTimeout(() => {
      reject(
        new WaitTimeoutError(
          `Timed out after ${(timeout / 1000).toFixed(1)} s waiting for ${what}`,
        ),
      );
    }, timeout);
  });
  try {
    return await Promise.race([promise, expired]);
  } finally {
    clearTimeout(timer);
  }
}

/** The message from a describing function, or a note that it failed. */
async function describe(message: () => string | Promise<string>): Promise<string> {
  try {
    return await message();
  } catch (error: unknown) {
    return `(the description failed: ${error instanceof Error ? error.message : String(error)})`;
  }
}
