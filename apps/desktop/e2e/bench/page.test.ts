/**
 * The scripts the benchmarks run in the page (page.ts), run here against a fake page with a
 * deterministic event loop: tasks (timers, posted messages, animation frames) run in order on a
 * virtual clock, and a task can take time, as the editor's rendering and preview do.
 */
import { describe, expect, it } from 'vitest';

import { FakeLoop, FakeObservers, runScript as start } from '../support/fake-page';
import {
  EDIT_SCRIPT,
  type EditResult,
  FRAMES_KEY,
  FRAMES_START_SCRIPT,
  FRAMES_STOP_SCRIPT,
  GONE_SCRIPT,
  isTimestamps,
  DROP_REPORT_SCRIPT,
  PAGE_SCRIPT,
  PLACE_SCRIPT,
  POINTER_KEY,
  POINTER_RECORD_SCRIPT,
  previewTask,
  QUIET_SCRIPT,
  READY_SCRIPT,
  type ReadyResult,
  RUN_OUTPUT_SCRIPT,
  type ClickResult,
  longestAfterPress,
  TASKS_KEY,
  TASKS_SETTLE_SCRIPT,
  TASKS_START_SCRIPT,
} from './page';

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

/**
 * A fake editor: an insertion (taking `insertMs` itself) re-renders, then (after the debounce)
 * the preview runs, and then, `afterMs` later, a task of `panelMs` updates the C++ panel.
 */
function editor(
  loop: FakeLoop,
  options: {
    renderMs: number;
    previewMs: number;
    insertMs?: number;
    panelMs?: number;
    afterMs?: number;
    failInsert?: boolean;
    neverPreview?: boolean;
  },
) {
  let code = 'int main() {}';
  const hook = {
    insertBlocks: (_parent: string, _input: string, blocks: { text: string }[]) => {
      if (options.failInsert === true) {
        throw new Error('Block b9 has no input BODY');
      }
      loop.now += options.insertMs ?? 0;
      // Blockly renders in a later task; the change event starts the 50 ms debounce.
      loop.post(() => {
        loop.now += options.renderMs;
      });
      if (options.neverPreview !== true) {
        loop.post(() => {
          loop.now += options.previewMs;
          code = `int main() { ${blocks.map((block) => block.text).join(' ')} }`;
          if (options.panelMs !== undefined) {
            loop.busy(options.panelMs, options.afterMs ?? 0);
          }
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
  settleMs = 0,
  extra: Record<string, unknown> = {},
): EditResult | null {
  let result: EditResult | null = null;
  start(
    EDIT_SCRIPT,
    loop.globals({ window, ...extra }),
    [
      '__B2C_E2E__',
      'b0',
      'BODY',
      [{ text: 'marker 1' }],
      'marker 1',
      timeoutMs,
      longTaskMs,
      settleMs,
    ],
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

  it('stops at the preview without a settle time, whatever follows it', () => {
    const loop = new FakeLoop();
    const window = editor(loop, { renderMs: 300, previewMs: 90, panelMs: 200, afterMs: 5 });
    const result = edit(loop, window);
    expect(result?.kind).toBe('seen');
    if (result?.kind !== 'seen') {
      return;
    }
    expect(result.settled).toBe(result.seen);
    expect(result.longest?.ms).toBeGreaterThanOrEqual(300);
    expect(result.longest?.ms).toBeLessThan(301);
  });

  it('with a settle time, waits for the work after the preview and finds the longest task', () => {
    const loop = new FakeLoop();
    const window = editor(loop, {
      renderMs: 300,
      previewMs: 90,
      insertMs: 40,
      panelMs: 450,
      afterMs: 5,
    });
    const result = edit(loop, window, 4, 30_000, 200);
    expect(result?.kind).toBe('seen');
    if (result?.kind !== 'seen') {
      return;
    }
    // The preview task is still found, and the C++ panel's update is part of the settled time.
    expect(previewTask(result)?.ms).toBeGreaterThanOrEqual(90);
    expect(previewTask(result)?.ms).toBeLessThan(91);
    expect(result.settled - result.seen).toBeGreaterThanOrEqual(5 + 450);
    expect(result.settled - result.seen).toBeLessThan(5 + 450 + 5);
    expect(result.longest?.ms).toBeGreaterThanOrEqual(450);
    expect(result.longest?.ms).toBeLessThan(451);
    // It called back once the page had been quiet for the settle time, not much later.
    expect(loop.now - result.settled).toBeGreaterThanOrEqual(200);
    expect(loop.now - result.settled).toBeLessThan(210);
  });

  it('counts the insertion itself as a task', () => {
    const loop = new FakeLoop();
    const window = editor(loop, { renderMs: 10, previewMs: 10, insertMs: 700 });
    const result = edit(loop, window, 4, 30_000, 100);
    expect(result?.kind === 'seen' ? result.longest?.ms : null).toBeGreaterThanOrEqual(700);
  });

  it('times out when the page never settles', () => {
    const loop = new FakeLoop();
    const window = editor(loop, { renderMs: 10, previewMs: 10 });
    // A task of 50 ms every 100 ms: never 200 ms without a long task.
    for (let index = 0; index < 50; index += 1) {
      loop.busy(50, 100 * index);
    }
    expect(edit(loop, window, 4, 1_000, 200)).toEqual({ kind: 'timeout' });
  });

  it('gives the app’s spans of the edit, those not delivered yet too, and stops observing', () => {
    const loop = new FakeLoop();
    const observers = new FakeObservers();
    const window = editor(loop, { renderMs: 300, previewMs: 90 });
    // Spans of the edit, before its preview task (which starts at 350): one delivered at once,
    // one still pending when the marker shows.
    loop.post(() => {
      observers.deliver([
        { name: 'b2c:preview:update', startTime: 360, duration: 40, type: 'measure' },
      ]);
      observers.hold([{ name: 'b2c:preview', startTime: 351, duration: 90, type: 'measure' }]);
      observers.deliver([{ name: 'other', startTime: 360, duration: 1, type: 'measure' }]);
    }, 340);
    const result = edit(loop, window, 4, 30_000, 0, {
      PerformanceObserver: observers.constructorFor(),
    });
    expect(result?.kind === 'seen' ? result.spans : null).toEqual([
      { name: 'b2c:preview:update', start: 360, ms: 40 },
      { name: 'b2c:preview', start: 351, ms: 90 },
    ]);
    expect(observers.observed).toEqual([['measure']]);
    expect(observers.disconnected).toBe(1);
    expect(observers.observing).toBe(0);
  });

  it('gives no spans in a page without an observer', () => {
    const loop = new FakeLoop();
    const result = edit(loop, editor(loop, { renderMs: 30, previewMs: 9 }));
    expect(result?.kind === 'seen' ? result.spans : null).toEqual([]);
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
  const seen = {
    kind: 'seen',
    begin: 0,
    inserted: 1,
    seen: 1_000,
    settled: 1_000,
    longest: null,
    spans: [],
  } as const;

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

/** The console's state line of a fake page: what `document.querySelector` finds for it. */
const STATE_SELECTOR = '[data-testid="console-state"]';

/**
 * A fake window whose pointer presses run the capture listeners, and a press that takes time. Its
 * document has one element, the console's state line, whose text is `stateLine.text`.
 */
function clickPage(loop: FakeLoop, observers?: FakeObservers) {
  const stateLine = { text: 'Finished (exit code 0)' };
  const document = {
    querySelector: (selector: string) =>
      selector === STATE_SELECTOR ? { textContent: stateLine.text } : null,
  };
  const listeners = new Map<string, Set<() => void>>();
  const window: Record<string, unknown> = {
    addEventListener: (type: string, listener: () => void) => {
      const set = listeners.get(type) ?? new Set();
      set.add(listener);
      listeners.set(type, set);
    },
    removeEventListener: (type: string, listener: () => void) => {
      listeners.get(type)?.delete(listener);
    },
  };
  const globals = loop.globals({
    window,
    document,
    ...(observers === undefined ? {} : { PerformanceObserver: observers.constructorFor() }),
  });
  /** A press at `delay` whose handling (selecting a block) takes `ms`. */
  const press = (delay: number, ms: number) => {
    loop.post(() => {
      for (const listener of listeners.get('pointerdown') ?? []) {
        listener();
      }
      loop.now += ms;
    }, delay);
  };
  const settle = (
    settleMs: number,
    timeoutMs: number,
    ...until: [selector: string, pattern: string] | []
  ): ClickResult | null => {
    let result: ClickResult | null = null;
    start(TASKS_SETTLE_SCRIPT, globals, [TASKS_KEY, settleMs, timeoutMs, ...until], (value) => {
      result = value as ClickResult;
    });
    loop.run(() => result !== null);
    return result;
  };
  /** Sets the console's state line to `text` at `delay`. */
  const showState = (text: string, delay: number) => {
    loop.post(() => {
      stateLine.text = text;
    }, delay);
  };
  return {
    window,
    listeners,
    press,
    settle,
    showState,
    startRecorder: () => start(TASKS_START_SCRIPT, globals, [TASKS_KEY, 4]),
  };
}

describe('TASKS_START_SCRIPT and TASKS_SETTLE_SCRIPT', () => {
  it('time the tasks a click starts until the page settles', () => {
    const loop = new FakeLoop();
    const page = clickPage(loop);
    expect(page.startRecorder()).toBe(true);
    expect(page.startRecorder()).toBe(false);
    loop.busy(40, 20); // before the press: not the click's
    page.press(100, 15); // the press's own task
    loop.busy(160, 130); // the toolbox's rebuild
    loop.busy(30, 330); // a follow-up
    const result = page.settle(300, 10_000);
    expect(result?.kind).toBe('settled');
    if (result?.kind !== 'settled') {
      return;
    }
    expect(result.pressed).toBeGreaterThanOrEqual(100);
    expect(result.settled).toBeGreaterThanOrEqual(360);
    expect(result.settled).toBeLessThan(362);
    expect(loop.now - result.settled).toBeGreaterThanOrEqual(300);
    const longest = longestAfterPress(result);
    expect(longest?.ms).toBeGreaterThanOrEqual(160);
    expect(longest?.ms).toBeLessThan(161);
    // Stopped: the listener is gone and the key deleted.
    expect(page.listeners.get('pointerdown')?.size).toBe(0);
    expect(Object.prototype.hasOwnProperty.call(page.window, TASKS_KEY)).toBe(false);
  });

  it('give the app’s spans from the start of the recording, and stop observing', () => {
    const loop = new FakeLoop();
    const observers = new FakeObservers();
    const page = clickPage(loop, observers);
    page.startRecorder();
    page.press(100, 15);
    loop.post(() => {
      observers.deliver([{ name: 'b2c:select', startTime: 100, duration: 14, type: 'measure' }]);
    }, 116);
    loop.post(() => {
      loop.now += 160;
      observers.deliver([
        { name: 'b2c:toolbox-refresh', startTime: 130, duration: 159, type: 'measure' },
      ]);
    }, 130);
    const result = page.settle(300, 10_000);
    expect(result?.kind === 'settled' ? result.spans : null).toEqual([
      { name: 'b2c:select', start: 100, ms: 14 },
      { name: 'b2c:toolbox-refresh', start: 130, ms: 159 },
    ]);
    expect(observers.disconnected).toBe(1);
    expect(observers.observing).toBe(0);
  });

  it('counts the task that handled the press, which began before it', () => {
    const loop = new FakeLoop();
    const page = clickPage(loop);
    page.startRecorder();
    page.press(50, 220);
    const result = page.settle(100, 10_000);
    expect(
      result?.kind === 'settled' ? longestAfterPress(result)?.ms : null,
    ).toBeGreaterThanOrEqual(220);
  });

  it('says when no press came, or the page never settled', () => {
    const quiet = new FakeLoop();
    const none = clickPage(quiet);
    none.startRecorder();
    expect(none.settle(100, 500)).toEqual({ kind: 'no-press' });

    const busy = new FakeLoop();
    const page = clickPage(busy);
    page.startRecorder();
    page.press(10, 5);
    for (let index = 0; index < 40; index += 1) {
      busy.busy(30, 20 + 50 * index);
    }
    expect(page.settle(100, 1_000)).toEqual({ kind: 'busy' });
  });

  it('needs a running recorder', () => {
    const loop = new FakeLoop();
    expect(clickPage(loop).settle(100, 500)).toEqual({
      kind: 'error',
      message: 'No task recorder is running',
    });
  });

  it('wait, when asked, until an element has left a text and come back to it (a run)', () => {
    const loop = new FakeLoop();
    const page = clickPage(loop);
    page.startRecorder();
    page.press(100, 15);
    page.showState('Running', 150);
    // The console writes the program's output, then the page is quiet for longer than the settle
    // time before the run ends.
    loop.busy(50, 200);
    loop.busy(50, 300);
    page.showState('Finished (exit code 0)', 1_000);
    loop.busy(20, 1_010); // the last of the output, after the state line
    const result = page.settle(300, 10_000, STATE_SELECTOR, '^Finished');
    expect(result?.kind).toBe('settled');
    if (result?.kind !== 'settled') {
      return;
    }
    expect(result.until).toBeGreaterThanOrEqual(1_000);
    expect(result.until).toBeLessThan(1_002);
    expect(result.settled).toBeGreaterThanOrEqual(1_030);
    expect(result.settled).toBeLessThan(1_032);
    expect(longestAfterPress(result)?.ms).toBeGreaterThanOrEqual(50);
    // Without the condition the page settles before the run has ended.
    const plain = new FakeLoop();
    const early = clickPage(plain);
    early.startRecorder();
    early.press(100, 15);
    early.showState('Running', 150);
    plain.busy(50, 200);
    plain.busy(50, 300);
    early.showState('Finished (exit code 0)', 1_000);
    const settled = early.settle(300, 10_000);
    expect(settled?.kind === 'settled' ? settled.settled : null).toBeLessThan(400);
    expect(settled?.kind === 'settled' ? settled.until : undefined).toBeNull();
  });

  it('are busy when the text waited for does not come back, and refuse a bad expression', () => {
    const loop = new FakeLoop();
    const page = clickPage(loop);
    page.startRecorder();
    page.press(100, 15);
    page.showState('Running', 150);
    expect(page.settle(100, 2_000, STATE_SELECTOR, '^Finished')).toEqual({ kind: 'busy' });

    const bad = clickPage(new FakeLoop());
    bad.startRecorder();
    expect(bad.settle(100, 2_000, STATE_SELECTOR, '(')).toEqual({
      kind: 'error',
      message: 'Not a regular expression: (',
    });
    expect(Object.prototype.hasOwnProperty.call(bad.window, TASKS_KEY)).toBe(false);
  });
});

describe('longestAfterPress', () => {
  it('takes the longest task that ended after the press', () => {
    const result = {
      kind: 'settled',
      pressed: 100,
      settled: 500,
      until: null,
      tasks: [
        { start: 10, ms: 80 },
        { start: 95, ms: 20 },
        { start: 200, ms: 50 },
      ],
      spans: [],
    } as const;
    expect(longestAfterPress(result)).toEqual({ start: 200, ms: 50 });
    expect(longestAfterPress({ ...result, tasks: [{ start: 0, ms: 50 }] })).toBeNull();
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

describe('RUN_OUTPUT_SCRIPT', () => {
  /** The script's result for a transcript. */
  const outputOf = (transcript: string, line: string): unknown =>
    start(
      RUN_OUTPUT_SCRIPT,
      new FakeLoop().globals({ window: { hook: { consoleText: () => transcript } } }),
      ['hook', line],
    );

  it('counts the lines with the text, and gives the end without a skipped marker', () => {
    const transcript = ['START', 'a line', 'a line', 'other', 'a lineEND', '', ''].join('\n');
    expect(outputOf(transcript, 'a line')).toEqual({
      lines: 3,
      skipped: false,
      tail: 'START\na line\na line\nother\na lineEND',
    });
    expect(outputOf('x'.repeat(500), 'a line')).toMatchObject({ tail: 'x'.repeat(200) });
  });

  it('notices the flood protection’s markers', () => {
    expect(outputOf('START\nB2C line\n… 1,204 lines skipped\nEND\n', 'B2C line')).toEqual({
      lines: 1,
      skipped: true,
      tail: 'START\nB2C line\n… 1,204 lines skipped\nEND',
    });
    expect(outputOf('… output skipped\n', 'B2C line')).toMatchObject({
      skipped: true,
      lines: 0,
    });
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

describe('the drop report', () => {
  /** A fake element with a tag, classes, a box and a parent. */
  class FakeElement {
    readonly tagName: string;
    readonly className: string;
    readonly parentElement: FakeElement | null;
    readonly box: { left: number; top: number; right: number; bottom: number };

    constructor(
      tagName: string,
      className: string,
      parentElement: FakeElement | null,
      box = { left: 0, top: 0, right: 0, bottom: 0 },
    ) {
      this.tagName = tagName;
      this.className = className;
      this.parentElement = parentElement;
      this.box = box;
    }

    getAttribute(name: string): string | null {
      return name === 'class' ? this.className : null;
    }

    getBoundingClientRect() {
      return this.box;
    }
  }

  it('records the last press and release, and reports the page at a drop point', () => {
    const listeners = new Map<string, (event: unknown) => void>();
    const svg = new FakeElement('SVG', 'blocklySvg', null);
    const background = new FakeElement('RECT', 'blocklyMainBackground', svg);
    const trash = new FakeElement('G', 'blocklyTrash', svg, {
      left: 1160.4,
      top: 455,
      right: 1205,
      bottom: 500,
    });
    const window: Record<string, unknown> = {
      innerWidth: 1280,
      innerHeight: 800,
      devicePixelRatio: 1.25,
      addEventListener: (type: string, listener: (event: unknown) => void) => {
        listeners.set(type, listener);
      },
    };
    const document = {
      elementFromPoint: (x: number, y: number) => (x > 1100 && y > 400 ? trash : background),
      querySelector: (selector: string) => (selector === '.blocklyTrash' ? trash : null),
    };
    const globals = { window, document, Element: FakeElement };
    start(POINTER_RECORD_SCRIPT, globals, [POINTER_KEY]);
    // Installed once: a second call keeps the first recorder.
    start(POINTER_RECORD_SCRIPT, globals, [POINTER_KEY]);
    listeners.get('pointerdown')?.({ clientX: 986.4, clientY: 332, target: background });
    listeners.get('pointerup')?.({ clientX: 1180, clientY: 470, target: trash });

    expect(start(DROP_REPORT_SCRIPT, globals, [POINTER_KEY, 1180, 470])).toEqual({
      viewport: [1280, 800, 1.25],
      under: ['g.blocklyTrash', 'svg.blocklySvg'],
      trash: [1160, 455, 1205, 500],
      flyout: null,
      zoom: null,
      pointer: {
        down: { x: 986, y: 332, target: 'rect.blocklyMainBackground' },
        up: { x: 1180, y: 470, target: 'g.blocklyTrash' },
      },
    });
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
