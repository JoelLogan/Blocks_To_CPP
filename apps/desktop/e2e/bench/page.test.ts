/**
 * The scripts the benchmarks run in the page (page.ts), run here against a fake page with a
 * deterministic event loop: tasks (timers, posted messages, animation frames) run in order on a
 * virtual clock, and a task can take time, as the editor's rendering and preview do.
 */
import { runInNewContext } from 'node:vm';

import { describe, expect, it } from 'vitest';

import {
  EDIT_SCRIPT,
  type EditResult,
  FRAMES_KEY,
  FRAMES_START_SCRIPT,
  FRAMES_STOP_SCRIPT,
  GONE_SCRIPT,
  isTimestamps,
  PAGE_SCRIPT,
  PLACE_SCRIPT,
  previewTask,
  QUIET_SCRIPT,
  READY_SCRIPT,
  type ReadyResult,
} from './page';

/** How long the fake loop's own work takes per task, in virtual milliseconds. */
const TASK_COST_MS = 0.25;

/** A deterministic event loop on a virtual clock. */
class FakeLoop {
  now = 0;
  readonly timeOrigin = 1_700_000_000_000;
  #queue: { at: number; seq: number; run: () => void }[] = [];
  #seq = 0;
  #frames: ((time: number) => void)[] = [];
  #frameQueued = false;
  #nextFrame = 0;
  /** Animation frames come every this many milliseconds; `null`: none at all. */
  frameEvery: number | null = 16;

  post(run: () => void, delay = 0): void {
    this.#queue.push({ at: this.now + Math.max(0, delay), seq: this.#seq, run });
    this.#seq += 1;
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
      let next: { at: number; seq: number; run: () => void } | undefined;
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

  /** The globals a page script sees. */
  globals(extra: Record<string, unknown> = {}): Record<string, unknown> {
    const post = (run: () => void, delay = 0): void => {
      this.post(run, delay);
    };
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
        post(run, ms);
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

/** Runs a page script as WebDriver does: a function body called with the arguments. */
function start(
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

describe('READY_SCRIPT', () => {
  function page(loop: FakeLoop, readyAt: number) {
    const ready = () => loop.now >= readyAt;
    return {
      document: {
        querySelector: (selector: string) => (ready() && selector.length > 0 ? {} : null),
        querySelectorAll: () => ({ length: ready() ? 9 : 0 }),
      },
    };
  }

  it('reports when the marker first holds, on the page clock', () => {
    const loop = new FakeLoop();
    let result: ReadyResult | null = null;
    start(READY_SCRIPT, loop.globals(page(loop, 1_234)), [60_000], (value) => {
      result = value as ReadyResult;
    });
    loop.run(() => result !== null);
    expect(result).toMatchObject({ kind: 'ready', already: false });
    const at = (result as unknown as { at: number }).at - loop.timeOrigin;
    expect(at).toBeGreaterThanOrEqual(1_234);
    expect(at).toBeLessThan(1_234 + 6);
  });

  it('says when the marker held already', () => {
    const loop = new FakeLoop();
    let result: unknown = null;
    start(READY_SCRIPT, loop.globals(page(loop, 0)), [60_000], (value) => {
      result = value;
    });
    expect(result).toMatchObject({ kind: 'ready', already: true });
  });

  it('gives up after the timeout', () => {
    const loop = new FakeLoop();
    let result: unknown = null;
    start(READY_SCRIPT, loop.globals(page(loop, Number.POSITIVE_INFINITY)), [500], (value) => {
      result = value;
    });
    loop.run(() => result !== null);
    expect(result).toEqual({ kind: 'timeout' });
    expect(loop.now).toBeLessThan(520);
  });
});

describe('PAGE_SCRIPT', () => {
  function look(loop: FakeLoop, readyAt: number, href: string): unknown {
    return start(
      PAGE_SCRIPT,
      loop.globals({
        location: { href },
        document: {
          querySelector: (selector: string) =>
            loop.now >= readyAt && selector.length > 0 ? {} : null,
          querySelectorAll: () => ({ length: loop.now >= readyAt ? 9 : 0 }),
        },
      }),
      [],
    );
  }

  it('says which page the window shows and when the marker held there', () => {
    const loop = new FakeLoop();
    expect(look(loop, 1_000, 'about:blank')).toEqual({
      href: 'about:blank',
      timeOrigin: loop.timeOrigin,
      readyAt: null,
    });
    loop.now = 1_250;
    expect(look(loop, 1_000, 'tauri://localhost/')).toEqual({
      href: 'tauri://localhost/',
      timeOrigin: loop.timeOrigin,
      readyAt: loop.timeOrigin + 1_250,
    });
  });
});

/** A fake editor: an insertion re-renders, then (after the debounce) the preview runs. */
function editor(
  loop: FakeLoop,
  options: { renderMs: number; previewMs: number; failInsert?: boolean; neverPreview?: boolean },
) {
  let code = 'int main() {}';
  const hook = {
    insertBlocks: (_parent: string, _input: string, blocks: { text: string }[]) => {
      if (options.failInsert === true) {
        throw new Error('Block b9 has no input BODY');
      }
      // Blockly renders in a later task; the change event starts the 50 ms debounce.
      loop.post(() => {
        loop.now += options.renderMs;
      });
      if (options.neverPreview !== true) {
        loop.post(() => {
          loop.now += options.previewMs;
          code = `int main() { ${blocks.map((block) => block.text).join(' ')} }`;
        }, options.renderMs + 50);
      }
    },
    code: () => code,
  };
  return { __B2C_E2E__: hook };
}

function edit(
  loop: FakeLoop,
  window: Record<string, unknown>,
  longTaskMs = 4,
  timeoutMs = 30_000,
): EditResult | null {
  let result: EditResult | null = null;
  start(
    EDIT_SCRIPT,
    loop.globals({ window }),
    ['__B2C_E2E__', 'b0', 'BODY', [{ text: 'marker 1' }], 'marker 1', timeoutMs, longTaskMs],
    (value) => {
      result = value as EditResult;
    },
  );
  loop.run(() => result !== null);
  return result;
}

describe('EDIT_SCRIPT', () => {
  it('times the edit and finds the preview pipeline’s task', () => {
    const loop = new FakeLoop();
    const result = edit(loop, editor(loop, { renderMs: 800, previewMs: 120 }));
    expect(result?.kind).toBe('seen');
    if (result?.kind !== 'seen') {
      return;
    }
    const task = previewTask(result);
    expect(task).not.toBeNull();
    expect(task?.ms).toBeGreaterThanOrEqual(120);
    expect(task?.ms).toBeLessThan(121);
    expect(result.seen - result.inserted).toBeGreaterThanOrEqual(800 + 50 + 120);
    expect(result.seen - result.inserted).toBeLessThan(800 + 50 + 120 + 5);
  });

  it('finds the task when the preview is quick', () => {
    const loop = new FakeLoop();
    const result = edit(loop, editor(loop, { renderMs: 300, previewMs: 6 }));
    expect(result?.kind === 'seen' ? previewTask(result)?.ms : null).toBeGreaterThanOrEqual(6);
  });

  it('has no task when the preview is shorter than a long task', () => {
    const loop = new FakeLoop();
    const result = edit(loop, editor(loop, { renderMs: 300, previewMs: 1 }), 4);
    expect(result?.kind).toBe('seen');
    expect(result?.kind === 'seen' ? previewTask(result) : 'none').toBeNull();
  });

  it('reports a refused insertion', () => {
    const loop = new FakeLoop();
    const result = edit(loop, editor(loop, { renderMs: 1, previewMs: 1, failInsert: true }));
    expect(result).toEqual({ kind: 'error', message: 'Error: Block b9 has no input BODY' });
  });

  it('times out when the edit never shows', () => {
    const loop = new FakeLoop();
    const result = edit(
      loop,
      editor(loop, { renderMs: 1, previewMs: 1, neverPreview: true }),
      4,
      300,
    );
    expect(result).toEqual({ kind: 'timeout' });
  });

  it('reports a missing hook', () => {
    const loop = new FakeLoop();
    expect(edit(loop, {})).toEqual({
      kind: 'error',
      message: 'The end-to-end hook is not installed',
    });
  });
});

describe('previewTask', () => {
  const seen = { kind: 'seen', begin: 0, inserted: 1, seen: 1_000 } as const;

  it('accepts a task between the insertion and the preview', () => {
    expect(previewTask({ ...seen, task: { start: 800, ms: 200 } })).toEqual({
      start: 800,
      ms: 200,
    });
  });

  it('refuses tasks that do not fit', () => {
    expect(previewTask({ ...seen, task: null })).toBeNull();
    expect(previewTask({ ...seen, task: { start: 0, ms: 50 } })).toBeNull();
    expect(previewTask({ ...seen, task: { start: 900, ms: 200 } })).toBeNull();
    expect(previewTask({ ...seen, task: { start: 900, ms: 0 } })).toBeNull();
  });
});

describe('GONE_SCRIPT', () => {
  it('calls back once the marker has left the code', () => {
    const loop = new FakeLoop();
    let code = 'print("marker")';
    loop.post(() => {
      code = 'nothing';
    }, 200);
    let result: unknown = null;
    start(
      GONE_SCRIPT,
      loop.globals({ window: { hook: { code: () => code } } }),
      ['hook', 'marker', 5_000],
      (value) => {
        result = value;
      },
    );
    loop.run(() => result !== null);
    expect(result).toBe('gone');
    expect(loop.now).toBeGreaterThanOrEqual(200);
  });

  it('times out, and reports a failing hook', () => {
    const loop = new FakeLoop();
    let result: unknown = null;
    start(
      GONE_SCRIPT,
      loop.globals({ window: { hook: { code: () => 'marker' } } }),
      ['hook', 'marker', 100],
      (value) => {
        result = value;
      },
    );
    loop.run(() => result !== null);
    expect(result).toBe('timeout');
    result = null;
    start(GONE_SCRIPT, loop.globals({ window: {} }), ['hook', 'marker', 100], (value) => {
      result = value;
    });
    expect(result).toBe('error');
  });
});

/** A window that keeps its event listeners, so the test can dispatch pointer events. */
function pointerWindow() {
  const listeners = new Map<string, Set<() => void>>();
  return {
    window: {
      addEventListener: (type: string, listener: () => void) => {
        const set = listeners.get(type) ?? new Set();
        set.add(listener);
        listeners.set(type, set);
      },
      removeEventListener: (type: string, listener: () => void) => {
        listeners.get(type)?.delete(listener);
      },
    } as Record<string, unknown>,
    dispatch: (type: string) => {
      for (const listener of listeners.get(type) ?? []) {
        listener();
      }
    },
    count: () => [...listeners.values()].reduce((sum, set) => sum + set.size, 0),
  };
}

describe('the frame recorder', () => {
  it('records the frames of a drag only, and cleans up when stopped', () => {
    const loop = new FakeLoop();
    const page = pointerWindow();
    const globals = loop.globals({ window: page.window });
    expect(start(FRAMES_START_SCRIPT, globals, [FRAMES_KEY])).toBe(true);
    expect(start(FRAMES_START_SCRIPT, globals, [FRAMES_KEY])).toBe(false);
    const until = (time: number) => () => loop.now >= time;
    loop.run(until(100));
    page.dispatch('pointerdown');
    loop.run(until(150));
    // Pressed but not moved yet: nothing is recorded.
    page.dispatch('pointermove');
    loop.run(until(300));
    page.dispatch('pointerup');
    loop.run(until(400));
    const frames = start(FRAMES_STOP_SCRIPT, globals, [FRAMES_KEY]);
    expect(isTimestamps(frames)).toBe(true);
    const times = frames as number[];
    expect(times.length).toBeGreaterThanOrEqual(8);
    expect(times.length).toBeLessThanOrEqual(11);
    // From the first move (at 150) to the release (just after 300, a frame later at most).
    expect(times.every((time) => time >= 150 && time <= 300 + 16)).toBe(true);
    expect(page.count()).toBe(0);
    expect(Object.prototype.hasOwnProperty.call(page.window, FRAMES_KEY)).toBe(false);
    expect(start(FRAMES_STOP_SCRIPT, globals, [FRAMES_KEY])).toBeNull();
  });
});

describe('QUIET_SCRIPT', () => {
  it('waits for frames in a row that come on time', () => {
    const loop = new FakeLoop();
    // A long task (a drop's preview) right at the start.
    loop.post(() => {
      loop.now += 400;
    });
    let result: unknown = null;
    start(QUIET_SCRIPT, loop.globals(), [10, 40, 5_000], (value) => {
      result = value;
    });
    loop.run(() => result !== null);
    expect(result).toBe('quiet');
    expect(loop.now).toBeGreaterThanOrEqual(400 + 10 * 16);
  });

  it('says busy when the frames never settle, or never come', () => {
    const loop = new FakeLoop();
    loop.frameEvery = 100;
    let result: unknown = null;
    start(QUIET_SCRIPT, loop.globals(), [10, 40, 1_000], (value) => {
      result = value;
    });
    loop.run(() => result !== null);
    expect(result).toBe('busy');

    const stopped = new FakeLoop();
    stopped.frameEvery = null;
    result = null;
    start(QUIET_SCRIPT, stopped.globals(), [10, 40, 1_000], (value) => {
      result = value;
    });
    stopped.run(() => result !== null);
    expect(result).toBe('busy');
  });
});

describe('PLACE_SCRIPT', () => {
  /**
   * A block at (x, y) on a canvas scrolled by (scrollX, scrollY) and zoomed by `scale`, inside an
   * SVG at (10, 20) on screen: the screen transforms Blockly's groups get.
   */
  function canvasWith(block: { x: number; y: number; width: number; height: number } | null) {
    const scale = 0.8;
    const scroll = { x: -300, y: 40 };
    const canvas = { a: scale, d: scale, e: 10 + scroll.x, f: 20 + scroll.y };
    const parent = { getScreenCTM: () => canvas };
    const element =
      block === null
        ? null
        : {
            parentNode: parent,
            getScreenCTM: () => ({
              ...canvas,
              e: canvas.e + scale * block.x,
              f: canvas.f + scale * block.y,
            }),
            getBoundingClientRect: () => {
              const left = canvas.e + scale * block.x;
              const top = canvas.f + scale * block.y;
              return {
                left,
                top,
                right: left + scale * block.width,
                bottom: top + scale * block.height,
                x: left,
                y: top,
              };
            },
          };
    const asked: unknown[] = [];
    const hook = {
      blockElement: (id: unknown) => {
        asked.push(id);
        return element;
      },
    };
    return { window: { __B2C_E2E__: hook }, asked };
  }

  it('gives the block’s box on screen and its place on the canvas', () => {
    const page = canvasWith({ x: 800, y: 40, width: 250, height: 100 });
    const place = start(PLACE_SCRIPT, { window: page.window }, ['__B2C_E2E__', 'b4998']);
    expect(page.asked).toEqual(['b4998']);
    expect(place).toEqual({
      box: { left: 350, top: 92, right: 550, bottom: 172 },
      offset: { x: 640, y: 32 },
    });
  });

  it('is null for a block that is not on the canvas, and fails without the hook', () => {
    const page = canvasWith(null);
    expect(start(PLACE_SCRIPT, { window: page.window }, ['__B2C_E2E__', 'b1'])).toBeNull();
    expect(() => start(PLACE_SCRIPT, { window: {} }, ['__B2C_E2E__', 'b1'])).toThrow(
      /hook is not installed/,
    );
  });
});

describe('isTimestamps', () => {
  it('accepts lists of finite numbers only', () => {
    expect(isTimestamps([1, 2.5])).toBe(true);
    expect(isTimestamps([])).toBe(true);
    expect(isTimestamps([1, Number.NaN])).toBe(false);
    expect(isTimestamps(['1'])).toBe(false);
    expect(isTimestamps(null)).toBe(false);
  });
});
