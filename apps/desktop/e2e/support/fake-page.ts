/**
 * A fake page for the unit tests of the scripts the harness and the benchmarks run in the webview
 * (support/longtasks.ts, bench/page.ts): a deterministic event loop on a virtual clock, where
 * tasks (timers, posted messages, animation frames) run in order and a task can take time, as the
 * editor's rendering and preview do. Test code only; nothing here runs against the app.
 */
import { runInNewContext } from 'node:vm';

/** How long the fake loop's own work takes per task, in virtual milliseconds. */
export const TASK_COST_MS = 0.25;

/** The shortest delay of a nested timer, as browsers clamp `setTimeout(…, 0)` chains. */
export const TIMER_CLAMP_MS = 4;

/** A queued task. */
interface QueuedTask {
  readonly at: number;
  readonly seq: number;
  readonly run: () => void;
}

/** A deterministic event loop on a virtual clock. */
export class FakeLoop {
  now = 0;
  readonly timeOrigin = 1_700_000_000_000;
  #queue: QueuedTask[] = [];
  #seq = 0;
  #frames: ((time: number) => void)[] = [];
  #frameQueued = false;
  #nextFrame = 0;
  /** Animation frames come every this many milliseconds; `null`: none at all. */
  frameEvery: number | null = 16;
  /** Whether `setTimeout` clamps its delay to {@link TIMER_CLAMP_MS} (off: a 0 ms timer is next). */
  clampTimers = false;

  /** Queues `run` as a task `delay` milliseconds from now. */
  post(run: () => void, delay = 0): void {
    this.#queue.push({ at: this.now + Math.max(0, delay), seq: this.#seq, run });
    this.#seq += 1;
  }

  /** Queues a task that keeps the main thread busy for `ms` milliseconds, `delay` from now. */
  busy(ms: number, delay = 0): void {
    this.post(() => {
      this.now += ms;
    }, delay);
  }

  /** Queues the next animation frame when callbacks wait for one. */
  #queueFrame(): void {
    const every = this.frameEvery;
    if (every === null || this.#frameQueued || this.#frames.length === 0) {
      return;
    }
    this.#frameQueued = true;
    this.#queue.push({
      at: Math.max(this.#nextFrame, this.now),
      seq: this.#seq,
      run: () => {
        this.#frameQueued = false;
        this.#nextFrame = this.now + every;
        for (const callback of this.#frames.splice(0)) {
          callback(this.now);
        }
      },
    });
    this.#seq += 1;
  }

  /** Runs tasks in order until `until()` holds or nothing is left (or a safety limit). */
  run(until: () => boolean, limit = 200_000): void {
    for (let step = 0; step < limit && !until(); step += 1) {
      this.#queueFrame();
      let next: QueuedTask | undefined;
      for (const task of this.#queue) {
        if (
          next === undefined ||
          task.at < next.at ||
          (task.at === next.at && task.seq < next.seq)
        ) {
          next = task;
        }
      }
      if (next === undefined) {
        return;
      }
      this.#queue.splice(this.#queue.indexOf(next), 1);
      this.now = Math.max(this.now, next.at);
      next.run();
      this.now += TASK_COST_MS;
    }
  }

  /** Runs tasks until the virtual clock reaches `time`. */
  runUntil(time: number): void {
    this.run(() => this.now >= time);
  }

  /** The globals a page script sees. */
  globals(extra: Record<string, unknown> = {}): Record<string, unknown> {
    const post = (run: () => void, delay = 0): void => {
      this.post(run, delay);
    };
    const clamp = (ms: number): number => (this.clampTimers ? Math.max(TIMER_CLAMP_MS, ms) : ms);
    /** A message channel whose messages are tasks of this loop. */
    class FakeChannel {
      readonly port1: { onmessage: (() => void) | null; close: () => void } = {
        onmessage: null,
        close: () => {
          this.closed = true;
        },
      };
      readonly port2 = {
        postMessage: () => {
          post(() => {
            if (!this.closed) {
              this.port1.onmessage?.();
            }
          });
        },
      };
      closed = false;
    }
    return {
      performance: { now: () => this.now, timeOrigin: this.timeOrigin },
      setTimeout: (run: () => void, ms = 0) => {
        post(run, clamp(ms));
        return 0;
      },
      requestAnimationFrame: (callback: (time: number) => void) => {
        this.#frames.push(callback);
        return this.#frames.length;
      },
      MessageChannel: FakeChannel,
      ...extra,
    };
  }
}

/** A User Timing entry of a fake page. */
export interface FakeEntry {
  readonly name: string;
  readonly startTime: number;
  readonly duration: number;
  readonly type: string;
}

/** What a fake observer's callback is given. */
interface FakeEntryList {
  getEntries(): FakeEntry[];
}

/**
 * The `PerformanceObserver`s of a fake page: a test hands them entries at once
 * ({@link FakeObservers.deliver}) or leaves entries pending, as a browser does until its observer
 * task runs ({@link FakeObservers.hold}), which `takeRecords()` then returns.
 */
export class FakeObservers {
  readonly #callbacks = new Set<(list: FakeEntryList) => void>();
  readonly #pending: FakeEntry[] = [];
  readonly #counts = { disconnected: 0 };
  /** The entry types each `observe()` asked for. */
  readonly observed: string[][] = [];

  /** How many observers were disconnected. */
  get disconnected(): number {
    return this.#counts.disconnected;
  }

  /** How many observers observe. */
  get observing(): number {
    return this.#callbacks.size;
  }

  /** Hands `entries` to every observer that observes. */
  deliver(entries: FakeEntry[]): void {
    for (const callback of this.#callbacks) {
      callback({ getEntries: () => entries });
    }
  }

  /** Leaves `entries` pending: an observer's `takeRecords()` returns them. */
  hold(entries: FakeEntry[]): void {
    this.#pending.push(...entries);
  }

  /** A `PerformanceObserver` class for the page. */
  constructorFor(): unknown {
    const callbacks = this.#callbacks;
    const pending = this.#pending;
    const counts = this.#counts;
    const observed = this.observed;
    return class {
      readonly #callback: (list: FakeEntryList) => void;
      constructor(callback: (list: FakeEntryList) => void) {
        this.#callback = callback;
      }
      observe(options: { entryTypes: string[] }): void {
        observed.push([...options.entryTypes]);
        callbacks.add(this.#callback);
      }
      takeRecords(): FakeEntry[] {
        return pending.splice(0);
      }
      disconnect(): void {
        counts.disconnected += 1;
        callbacks.delete(this.#callback);
      }
    };
  }
}

/** Runs a page script as WebDriver does: a function body called with the arguments. */
export function runScript(
  script: string,
  globals: Record<string, unknown>,
  args: unknown[],
  done?: (value: unknown) => void,
): unknown {
  const body = runInNewContext(`(function () {${script}})`, globals) as (
    ...values: unknown[]
  ) => unknown;
  return done === undefined ? body(...args) : body(...args, done);
}
