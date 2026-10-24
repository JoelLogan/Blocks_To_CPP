/**
 * The time source of the build and run feature: the wall clock for elapsed times and the timers
 * that pace acknowledgements, input and resizes. Tests pass a fake one.
 */

/** A clock with timers. */
export interface Clock {
  /** Milliseconds since the epoch (as `Date.now()`). */
  now(): number;
  /** Calls `callback` once after `ms` milliseconds; returns a handle for {@link clearTimeout}. */
  setTimeout(callback: () => void, ms: number): unknown;
  /** Cancels a timer that has not fired yet. */
  clearTimeout(handle: unknown): void;
}

/** The real clock (`Date.now` and the global timers, which Vitest's fake timers replace). */
export const systemClock: Clock = {
  now: () => Date.now(),
  setTimeout: (callback, ms) => globalThis.setTimeout(callback, ms),
  clearTimeout: (handle) => {
    globalThis.clearTimeout(handle as ReturnType<typeof globalThis.setTimeout>);
  },
};

/** Resolves after `ms` milliseconds of `clock`. */
export function sleep(clock: Clock, ms: number): Promise<void> {
  return new Promise((resolve) => {
    clock.setTimeout(resolve, ms);
  });
}
