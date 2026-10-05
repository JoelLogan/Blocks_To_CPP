import { describe, expect, it } from 'vitest';

import {
  MAX_WRITE_BYTES,
  MIN_WRITE_INTERVAL_MS,
  OutputScheduler,
  type SchedulerTimers,
  type TerminalSink,
} from './outputScheduler';

/** A manual clock with timers that run when the test advances it. */
class FakeTimers implements SchedulerTimers {
  time = 0;
  #next = 1;
  readonly #timers = new Map<number, { at: number; callback: () => void }>();

  now(): number {
    return this.time;
  }

  setTimeout(callback: () => void, ms: number): unknown {
    const id = this.#next++;
    this.#timers.set(id, { at: this.time + ms, callback });
    return id;
  }

  clearTimeout(handle: unknown): void {
    this.#timers.delete(handle as number);
  }

  /** Moves the clock forward one millisecond at a time, running due timers. */
  advance(ms: number): void {
    const end = this.time + ms;
    while (this.time < end) {
      this.time++;
      for (const [id, timer] of [...this.#timers]) {
        if (timer.at <= this.time) {
          this.#timers.delete(id);
          timer.callback();
        }
      }
    }
  }

  get pending(): number {
    return this.#timers.size;
  }
}

/** A terminal that records writes and finishes each one when told to (or at once). */
class FakeSink implements TerminalSink {
  readonly writes: { at: number; text: string }[] = [];
  readonly events: string[] = [];
  readonly #done: (() => void)[] = [];
  readonly #timers: FakeTimers;
  readonly #manual: boolean;

  constructor(timers: FakeTimers, manual = false) {
    this.#timers = timers;
    this.#manual = manual;
  }

  write(data: Uint8Array, done: () => void): void {
    const text = new TextDecoder().decode(data);
    this.writes.push({ at: this.#timers.now(), text });
    this.events.push(`write:${text}`);
    if (this.#manual) {
      this.#done.push(done);
    } else {
      done();
    }
  }

  clear(): void {
    this.events.push('clear');
  }

  /** Finishes the oldest unfinished write. */
  finish(): void {
    this.#done.shift()?.();
  }
}

const bytes = (text: string) => new TextEncoder().encode(text);

/** Resolves when the microtasks queued so far (promise callbacks) have run. */
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

describe('OutputScheduler', () => {
  it('writes under 60 times a second however often output arrives', () => {
    const timers = new FakeTimers();
    const sink = new FakeSink(timers);
    const scheduler = new OutputScheduler(timers);
    scheduler.attach(sink);

    for (let ms = 0; ms < 2000; ms++) {
      void scheduler.write(bytes('x'));
      timers.advance(1);
    }
    timers.advance(100);

    for (let start = 0; start <= 1000; start++) {
      const inWindow = sink.writes.filter((w) => w.at >= start && w.at < start + 1000);
      expect(inWindow.length).toBeLessThanOrEqual(60);
    }
    for (let i = 1; i < sink.writes.length; i++) {
      const gap = (sink.writes[i]?.at ?? 0) - (sink.writes[i - 1]?.at ?? 0);
      expect(gap).toBeGreaterThanOrEqual(MIN_WRITE_INTERVAL_MS);
    }
    // Nothing is lost or reordered.
    expect(sink.writes.map((w) => w.text).join('')).toBe('x'.repeat(2000));
  });

  it('combines what arrives between writes and resolves each write when it is processed', async () => {
    const timers = new FakeTimers();
    const sink = new FakeSink(timers, true);
    const scheduler = new OutputScheduler(timers);
    scheduler.attach(sink);

    const resolved: string[] = [];
    void scheduler.write(bytes('a')).then(() => resolved.push('a'));
    void scheduler.write(bytes('b')).then(() => resolved.push('b'));
    timers.advance(1);
    expect(sink.writes.map((w) => w.text)).toEqual(['ab']);

    // While a write is in progress nothing else is written, however long it takes.
    void scheduler.write(bytes('c')).then(() => resolved.push('c'));
    timers.advance(100);
    expect(sink.writes).toHaveLength(1);
    await settle();
    expect(resolved).toEqual([]);

    sink.finish();
    await settle();
    expect(resolved).toEqual(['a', 'b']);
    timers.advance(MIN_WRITE_INTERVAL_MS);
    expect(sink.writes.map((w) => w.text)).toEqual(['ab', 'c']);
    sink.finish();
    await settle();
    expect(resolved).toEqual(['a', 'b', 'c']);
  });

  it('splits very large output over several writes', () => {
    const timers = new FakeTimers();
    const sink = new FakeSink(timers);
    const scheduler = new OutputScheduler(timers);
    scheduler.attach(sink);
    const big = new Uint8Array(MAX_WRITE_BYTES).fill(0x61);
    void scheduler.write(big);
    void scheduler.write(bytes('tail'));
    timers.advance(1);
    expect(sink.writes).toHaveLength(1);
    expect(sink.writes[0]?.text.length).toBe(MAX_WRITE_BYTES);
    expect(scheduler.queuedBytes).toBe(4);
    timers.advance(MIN_WRITE_INTERVAL_MS);
    expect(sink.writes[1]?.text).toBe('tail');
  });

  it('clears after the output already given to the terminal, before the output still queued', () => {
    const timers = new FakeTimers();
    const sink = new FakeSink(timers, true);
    const scheduler = new OutputScheduler(timers);
    scheduler.attach(sink);

    void scheduler.write(bytes('old'));
    timers.advance(1);
    void scheduler.write(bytes('new'));
    scheduler.clear();
    expect(sink.events).toEqual(['write:old']);
    sink.finish();
    timers.advance(MIN_WRITE_INTERVAL_MS);
    expect(sink.events).toEqual(['write:old', 'clear', 'write:new']);

    // With nothing in progress, a clear happens at once.
    sink.finish();
    scheduler.clear();
    expect(sink.events.at(-1)).toBe('clear');
  });

  it('keeps output until a terminal is attached', () => {
    const timers = new FakeTimers();
    const scheduler = new OutputScheduler(timers);
    void scheduler.writeText('early');
    timers.advance(50);
    expect(timers.pending).toBe(0);
    const sink = new FakeSink(timers);
    scheduler.attach(sink);
    timers.advance(1);
    expect(sink.writes.map((w) => w.text)).toEqual(['early']);
  });

  it('resolves every waiting write when the terminal goes away', async () => {
    const timers = new FakeTimers();
    const sink = new FakeSink(timers, true);
    const scheduler = new OutputScheduler(timers);
    scheduler.attach(sink);
    const resolved: string[] = [];
    void scheduler.write(bytes('in flight')).then(() => resolved.push('in flight'));
    timers.advance(1);
    void scheduler.write(bytes('queued')).then(() => resolved.push('queued'));

    scheduler.attach(null);
    await settle();
    expect(resolved).toEqual(['in flight', 'queued']);
    // A late callback from the old terminal changes nothing.
    sink.finish();
    expect(scheduler.queuedBytes).toBe(0);
  });

  it('counts a write the terminal refuses as written', async () => {
    const timers = new FakeTimers();
    const scheduler = new OutputScheduler(timers);
    scheduler.attach({
      write: () => {
        throw new Error('write data discarded');
      },
      clear: () => undefined,
    });
    const written = scheduler.write(bytes('x'));
    timers.advance(1);
    await expect(written).resolves.toBeUndefined();
  });
});
