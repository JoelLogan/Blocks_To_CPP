/**
 * Waiting for the toolbox's flyout to stop scrolling (ui.ts `SETTLE_SCRIPT`), run against a fake
 * page whose animation frames the test advances: the continuous toolbox's scroll animation lasts a
 * number of frames, so a slow frame rate makes it last longer than any fixed sleep.
 */
import { runInNewContext } from 'node:vm';

import { describe, expect, it } from 'vitest';

import {
  FLYOUT_CANVAS,
  FLYOUT_QUIET_MS,
  FLYOUT_SETTLE_TIMEOUT_MS,
  SETTLE_SCRIPT,
  type SettleResult,
} from './ui';

/** A page with one element whose `transform` the test animates, and a clock and frames it drives. */
class FakePage {
  transform = 'translate(0,0)';
  present = true;
  now = 0;
  result: SettleResult | null = null;
  /** When the script called back. */
  doneAt: number | null = null;
  readonly #frames: ((time: number) => void)[] = [];
  readonly #timers: { readonly at: number; readonly run: () => void }[] = [];

  /** Starts the script, as `executeAsyncScript` does. */
  start(): void {
    const globals = {
      document: {
        querySelector: (selector: string) =>
          this.present && selector === FLYOUT_CANVAS
            ? { getAttribute: (name: string) => (name === 'transform' ? this.transform : null) }
            : null,
      },
      performance: { now: () => this.now },
      requestAnimationFrame: (callback: (time: number) => void) => {
        this.#frames.push(callback);
        return this.#frames.length;
      },
      setTimeout: (run: () => void, ms: number) => {
        this.#timers.push({ at: this.now + ms, run });
        return this.#timers.length;
      },
    };
    // The script runs as a function body with the page's globals, as WebDriver runs it.
    const script = runInNewContext(`(function () {${SETTLE_SCRIPT}})`, globals) as (
      ...args: unknown[]
    ) => void;
    script(FLYOUT_CANVAS, FLYOUT_QUIET_MS, FLYOUT_SETTLE_TIMEOUT_MS, (result: SettleResult) => {
      this.result = result;
      this.doneAt = this.now;
    });
  }

  /** Lets `ms` pass and then runs one animation frame (`move` first, as the flyout's step). */
  frame(ms: number, move?: () => void): void {
    this.now += ms;
    move?.();
    for (const callback of this.#frames.splice(0)) {
      callback(this.now);
    }
    this.#runTimers();
  }

  /** Lets `ms` pass without any animation frame. */
  idle(ms: number): void {
    this.now += ms;
    this.#runTimers();
  }

  #runTimers(): void {
    for (const timer of this.#timers.filter((entry) => entry.at <= this.now)) {
      this.#timers.splice(this.#timers.indexOf(timer), 1);
      timer.run();
    }
  }
}

/**
 * Runs a scroll animation of `steps` frames, `frameMs` apart, and then still frames until the
 * script calls back (or 20 s pass). Returns when the animation ended.
 */
function animate(page: FakePage, steps: number, frameMs: number): number {
  let position = 0;
  let endedAt = 0;
  for (let frame = 0; page.result === null && page.now < 20_000; frame += 1) {
    page.frame(frameMs, () => {
      if (frame < steps) {
        position += 100;
        page.transform = `translate(0,${String(-position)})`;
        endedAt = page.now;
      }
    });
  }
  return endedAt;
}

describe('waiting for the flyout', () => {
  it('waits until the scroll animation has ended and stayed still', () => {
    const page = new FakePage();
    page.start();
    const endedAt = animate(page, 22, 16);
    expect(page.result).toBe('settled');
    expect(page.doneAt).toBeGreaterThanOrEqual(endedAt + FLYOUT_QUIET_MS);
    expect(page.doneAt).toBeLessThan(endedAt + FLYOUT_QUIET_MS + 50);
  });

  it('waits as long as the animation takes on a slow machine (not a fixed 600 ms)', () => {
    const page = new FakePage();
    page.start();
    // 22 frames at 20 frames a second: the scroll takes 1.1 s.
    const endedAt = animate(page, 22, 50);
    expect(endedAt).toBe(1_100);
    expect(page.result).toBe('settled');
    expect(page.doneAt).toBeGreaterThanOrEqual(endedAt + FLYOUT_QUIET_MS);
  });

  it('needs a few frames, not only time, to count as settled', () => {
    const page = new FakePage();
    page.start();
    // The page was busy: one late frame after a long pause proves nothing yet (the animation
    // moves only in frames), nor does a second one.
    page.frame(400);
    expect(page.result).toBeNull();
    page.frame(16);
    expect(page.result).toBeNull();
    page.frame(16);
    expect(page.result).toBe('settled');
  });

  it('reports a flyout that never stops, and one that is not there', () => {
    const moving = new FakePage();
    moving.start();
    animate(moving, Number.POSITIVE_INFINITY, 16);
    expect(moving.result).toBe('moving');
    expect(moving.doneAt).toBeGreaterThanOrEqual(FLYOUT_SETTLE_TIMEOUT_MS);

    const missing = new FakePage();
    missing.present = false;
    missing.start();
    expect(missing.result).toBe('missing');
  });

  it('calls back even when no animation frame ever comes', () => {
    const page = new FakePage();
    page.start();
    page.idle(FLYOUT_SETTLE_TIMEOUT_MS);
    expect(page.result).toBeNull();
    page.idle(1_000);
    expect(page.result).toBe('moving');
  });
});
