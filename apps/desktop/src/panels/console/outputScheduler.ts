/**
 * Paces program output into the terminal (docs/spec/04-user-interface.md §4.5, 07 §7.6.5): at most
 * one write every {@link MIN_WRITE_INTERVAL_MS} (under 60 a second), with everything that arrived
 * in between combined into that write, and at most one write in progress. Each `write` resolves
 * once the terminal has processed its bytes, which is when the console may acknowledge them to the
 * backend (`run_ack`).
 */

/** The shortest time between two writes into the terminal: 17 ms, so under 60 writes a second. */
export const MIN_WRITE_INTERVAL_MS = 17;

/**
 * The most bytes one write passes to the terminal. More waits for the next write, which keeps each
 * write well below xterm.js's own limit for unprocessed data (50 MB, where it throws).
 */
export const MAX_WRITE_BYTES = 4 * 1024 * 1024;

/** Where the output goes: the terminal. */
export interface TerminalSink {
  /** Writes bytes; calls `done` once the terminal has processed them. */
  write(data: Uint8Array, done: () => void): void;
  /** Clears the terminal's screen and scrollback. */
  clear(): void;
}

/** The clock and timers the scheduler uses; tests pass fake ones. */
export interface SchedulerTimers {
  now(): number;
  setTimeout(callback: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
}

const browserTimers: SchedulerTimers = {
  now: () => performance.now(),
  setTimeout: (callback, ms) => globalThis.setTimeout(callback, ms),
  clearTimeout: (handle) => {
    globalThis.clearTimeout(handle as ReturnType<typeof globalThis.setTimeout>);
  },
};

interface Pending {
  readonly data: Uint8Array;
  readonly resolve: () => void;
}

const encoder = new TextEncoder();

/** Combines output into paced terminal writes. See the module documentation. */
export class OutputScheduler {
  readonly #timers: SchedulerTimers;
  #sink: TerminalSink | null = null;
  #pending: Pending[] = [];
  /** The resolvers of the write in progress, or `null` when none is. */
  #inFlight: (() => void)[] | null = null;
  #timer: unknown = null;
  #lastWrite = Number.NEGATIVE_INFINITY;
  #clearRequested = false;

  constructor(timers: SchedulerTimers = browserTimers) {
    this.#timers = timers;
  }

  /**
   * Connects the terminal. `null` disconnects it: everything not yet written is dropped and every
   * waiting `write` resolves, so a console that goes away never leaves acknowledgements hanging.
   */
  attach(sink: TerminalSink | null): void {
    if (sink === null) {
      this.#cancelTimer();
      const waiting = [...(this.#inFlight ?? []), ...this.#pending.map((entry) => entry.resolve)];
      this.#sink = null;
      this.#pending = [];
      this.#inFlight = null;
      this.#clearRequested = false;
      for (const resolve of waiting) {
        resolve();
      }
      return;
    }
    this.#sink = sink;
    this.#schedule();
  }

  /**
   * Queues program output. The promise resolves when the terminal has processed it (or when the
   * terminal is disconnected). The bytes must not change until then.
   */
  write(data: Uint8Array): Promise<void> {
    return new Promise((resolve) => {
      this.#pending.push({ data, resolve });
      this.#schedule();
    });
  }

  /** Queues text of our own (such as the "lines skipped" marker), in order with the output. */
  writeText(text: string): Promise<void> {
    return this.write(encoder.encode(text));
  }

  /**
   * Clears the terminal after what it has already been given; output still queued is written
   * after the clear.
   */
  clear(): void {
    if (this.#sink !== null && this.#inFlight === null) {
      this.#sink.clear();
    } else {
      this.#clearRequested = true;
    }
  }

  /** How many bytes are queued and not yet passed to the terminal. */
  get queuedBytes(): number {
    return this.#pending.reduce((sum, entry) => sum + entry.data.length, 0);
  }

  #schedule(): void {
    if (this.#timer !== null || this.#inFlight !== null || this.#sink === null) {
      return;
    }
    if (this.#pending.length === 0 && !this.#clearRequested) {
      return;
    }
    const delay = Math.max(0, this.#lastWrite + MIN_WRITE_INTERVAL_MS - this.#timers.now());
    this.#timer = this.#timers.setTimeout(() => {
      this.#timer = null;
      this.#flush();
    }, delay);
  }

  #cancelTimer(): void {
    if (this.#timer !== null) {
      this.#timers.clearTimeout(this.#timer);
      this.#timer = null;
    }
  }

  #flush(): void {
    const sink = this.#sink;
    if (sink === null || this.#inFlight !== null) {
      return;
    }
    if (this.#clearRequested) {
      this.#clearRequested = false;
      sink.clear();
    }
    if (this.#pending.length === 0) {
      return;
    }

    // Take whole entries up to the size limit (always at least one).
    let take = 0;
    let size = 0;
    for (const entry of this.#pending) {
      if (take > 0 && size + entry.data.length > MAX_WRITE_BYTES) {
        break;
      }
      size += entry.data.length;
      take++;
    }
    const batch = this.#pending.splice(0, take);
    const data = concat(batch, size);
    const resolvers = batch.map((entry) => entry.resolve);
    this.#inFlight = resolvers;
    this.#lastWrite = this.#timers.now();

    let finished = false;
    const done = () => {
      if (finished || this.#inFlight !== resolvers) {
        return;
      }
      finished = true;
      this.#inFlight = null;
      for (const resolve of resolvers) {
        resolve();
      }
      this.#schedule();
    };
    try {
      sink.write(data, done);
    } catch {
      // The terminal refused the data (it never does under the backend's flow control): count it
      // as written so the acknowledgements carry on.
      done();
    }
  }
}

function concat(entries: readonly Pending[], size: number): Uint8Array {
  const [first] = entries;
  if (entries.length === 1 && first !== undefined) {
    return first.data;
  }
  const data = new Uint8Array(size);
  let offset = 0;
  for (const entry of entries) {
    data.set(entry.data, offset);
    offset += entry.data.length;
  }
  return data;
}
