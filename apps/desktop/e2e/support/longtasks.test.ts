/**
 * The long-task recorder (./longtasks.ts): its page scripts run against a fake page with a
 * deterministic event loop (./fake-page.ts), and the records, the report and its summary.
 */
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import type { WebDriver } from 'selenium-webdriver';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { type FakeEntry, FakeLoop, FakeObservers, runScript, TIMER_CLAMP_MS } from './fake-page';
import {
  clearLongTasks,
  finishLongTaskRecorder,
  KEPT_TASKS,
  LONG_TASK_MS,
  LONG_TASKS_KEY,
  LONG_TASKS_REPORT,
  LONG_TASKS_START_SCRIPT,
  LONG_TASKS_STOP_SCRIPT,
  LONG_TASKS_SUMMARY,
  longTaskRecord,
  longTaskSummary,
  MAX_MARKS,
  type PageLongTasks,
  parsePageLongTasks,
  startLongTaskRecorder,
  summaryLine,
  writeLongTaskSummary,
} from './longtasks';

/**
 * A fake page: the loop, its window and the document's visibility. With `observers`, the page has
 * a `PerformanceObserver`.
 */
function page(entries: FakeEntry[] = [], observers?: FakeObservers) {
  const loop = new FakeLoop();
  loop.clampTimers = true;
  const window: Record<string, unknown> = {};
  const document = { visibilityState: 'visible' };
  const globals = loop.globals({
    window,
    document,
    performance: {
      now: () => loop.now,
      timeOrigin: loop.timeOrigin,
      getEntriesByType: (type: string) => entries.filter((entry) => entry.type === type),
    },
    ...(observers === undefined ? {} : { PerformanceObserver: observers.constructorFor() }),
  });
  const start = () =>
    runScript(LONG_TASKS_START_SCRIPT, globals, [LONG_TASKS_KEY, LONG_TASK_MS, KEPT_TASKS]);
  const stop = () => runScript(LONG_TASKS_STOP_SCRIPT, globals, [LONG_TASKS_KEY, MAX_MARKS]);
  return { loop, window, document, start, stop };
}

describe('the recorder in the page', () => {
  it('finds the long tasks between heartbeats, within one beat of their length', () => {
    const { loop, start, stop } = page();
    expect(start()).toBe(true);
    loop.busy(30, 100);
    loop.busy(60, 300);
    loop.busy(120, 600);
    loop.runUntil(1_000);
    const found = parsePageLongTasks(stop());
    expect(found).not.toBeNull();
    if (found === null) {
      return;
    }
    // The 30 ms task is not long; the others are, each within one clamped beat.
    expect(found.count).toBe(2);
    expect(found.tasks.map((task) => task.ms)).toHaveLength(2);
    const [longest, second] = found.tasks;
    expect(longest?.ms).toBeGreaterThanOrEqual(120);
    expect(longest?.ms).toBeLessThanOrEqual(120 + TIMER_CLAMP_MS + 1);
    expect(longest?.start).toBeGreaterThanOrEqual(600 - TIMER_CLAMP_MS - 1);
    expect(longest?.start).toBeLessThanOrEqual(600);
    expect(second?.ms).toBeGreaterThanOrEqual(60);
    expect(found.totalMs).toBeCloseTo((longest?.ms ?? 0) + (second?.ms ?? 0), 6);
    expect(found.beats).toBeGreaterThan(100);
    expect(found.end - found.begin).toBeGreaterThanOrEqual(1_000);
  });

  it('keeps only the longest tasks, longest first, and counts them all', () => {
    const { loop, start, stop } = page();
    start();
    for (let index = 0; index < KEPT_TASKS + 3; index += 1) {
      loop.busy(LONG_TASK_MS + index * 10, 100 + index * 400);
    }
    loop.runUntil(100 + (KEPT_TASKS + 4) * 400);
    const found = parsePageLongTasks(stop());
    expect(found?.count).toBe(KEPT_TASKS + 3);
    const lengths = found?.tasks.map((task) => task.ms) ?? [];
    expect(lengths).toHaveLength(KEPT_TASKS);
    expect([...lengths].sort((a, b) => b - a)).toEqual(lengths);
    expect(lengths.at(-1)).toBeGreaterThanOrEqual(LONG_TASK_MS + 30);
  });

  it('does not count the time the window is hidden', () => {
    const { loop, document, start, stop } = page();
    start();
    loop.post(() => {
      document.visibilityState = 'hidden';
      loop.now += 900;
    }, 100);
    loop.post(() => {
      document.visibilityState = 'visible';
    }, 1_050);
    loop.runUntil(1_200);
    const found = parsePageLongTasks(stop());
    expect(found?.count).toBe(0);
    expect(found?.hiddenMs).toBeGreaterThanOrEqual(900);
  });

  it('starts once, stops for good and names the b2c: marks of a task', () => {
    const entries = [
      { name: 'b2c:preview', startTime: 210, duration: 70, type: 'measure' },
      { name: 'b2c:toolbox', startTime: 250, duration: 0, type: 'mark' },
      { name: 'other:thing', startTime: 220, duration: 10, type: 'measure' },
      { name: 'b2c:later', startTime: 900, duration: 5, type: 'measure' },
    ];
    const { loop, window, start, stop } = page(entries);
    expect(start()).toBe(true);
    expect(start()).toBe(false);
    loop.busy(80, 200);
    loop.runUntil(400);
    const found = parsePageLongTasks(stop());
    // Longest span first: the measure, then the mark (no duration).
    expect(found?.tasks[0]?.marks).toEqual(['b2c:preview', 'b2c:toolbox']);
    expect(Object.prototype.hasOwnProperty.call(window, LONG_TASKS_KEY)).toBe(false);
    // Stopped: no beat runs any more, so the loop runs dry.
    loop.busy(500, 10);
    loop.run(() => false);
    expect(stop()).toBeNull();
  });
});

describe('the recorder and the User Timing spans of the app', () => {
  // The app keeps only the latest entry of each span name in the browser's buffer
  // (src/lib/timing.ts), so the recorder must see the entries as they come.
  const measure = (name: string, startTime: number, duration: number): FakeEntry => ({
    name,
    startTime,
    duration,
    type: 'measure',
  });

  it('names a task with the spans an observer saw, whether they came before or after its beat', () => {
    const observers = new FakeObservers();
    const buffer: FakeEntry[] = [];
    const { loop, start, stop } = page(buffer, observers);
    start();
    // The first task's span reaches the observer within the task, before the beat that notes it.
    loop.post(() => {
      loop.now += 90;
      observers.deliver([measure('b2c:preview', 205, 85), measure('other:thing', 210, 5)]);
    }, 200);
    // The second task's span reaches the observer in a task of its own, after that beat.
    loop.post(() => {
      loop.now += 70;
      loop.post(() => {
        observers.deliver([measure('b2c:preview', 602, 66), measure('b2c:select', 601, 1)]);
      }, 20);
    }, 600);
    // A third task has no span; the span after it does not overlap it.
    loop.busy(60, 1_000);
    loop.post(() => {
      observers.deliver([measure('b2c:toolbox-refresh', 1_200, 10)]);
    }, 1_200);
    loop.runUntil(1_400);
    // The buffer only holds the latest entry of each name, as in the app.
    buffer.push(measure('b2c:preview', 602, 66), measure('b2c:toolbox-refresh', 1_200, 10));
    const found = parsePageLongTasks(stop());
    expect(found?.tasks.map((task) => task.marks)).toEqual([
      ['b2c:preview'],
      ['b2c:preview', 'b2c:select'],
      [],
    ]);
    expect(observers.observed).toEqual([['mark', 'measure']]);
    expect(observers.disconnected).toBe(1);
    expect(observers.observing).toBe(0);
  });

  it('lists the names of the longest spans of a task, longest first, each once', () => {
    const observers = new FakeObservers();
    const buffer: FakeEntry[] = [];
    const { loop, start, stop } = page(buffer, observers);
    start();
    loop.post(() => {
      loop.now += 80;
      // Span i lasts i ms; each comes twice, the second time shorter.
      const spans = Array.from({ length: MAX_MARKS + 3 }, (_, index) =>
        measure(`b2c:span-${String(index)}`, 301, index),
      );
      observers.deliver([...spans, ...spans.map((span) => ({ ...span, duration: 0 }))]);
    }, 300);
    loop.runUntil(600);
    // One more in the buffer only, longer than all of them.
    buffer.push(measure('b2c:buffered', 302, 50));
    const marks = parsePageLongTasks(stop())?.tasks[0]?.marks ?? [];
    expect(marks).toEqual([
      'b2c:buffered',
      ...Array.from(
        { length: MAX_MARKS - 1 },
        (_, index) => `b2c:span-${String(MAX_MARKS + 2 - index)}`,
      ),
    ]);
  });
});

describe('parsePageLongTasks', () => {
  const good: PageLongTasks = {
    begin: 10,
    end: 2_010,
    beats: 400,
    count: 1,
    totalMs: 75,
    hiddenMs: 0,
    tasks: [{ start: 500, ms: 75, marks: ['b2c:preview'] }],
  };

  it('accepts what the page returns and refuses anything else', () => {
    expect(parsePageLongTasks(good)).toEqual(good);
    expect(parsePageLongTasks(null)).toBeNull();
    expect(parsePageLongTasks({ ...good, end: Number.NaN })).toBeNull();
    expect(parsePageLongTasks({ ...good, tasks: 'none' })).toBeNull();
    expect(parsePageLongTasks({ ...good, tasks: [{ start: 1, ms: -1, marks: [] }] })).toBeNull();
    expect(parsePageLongTasks({ ...good, tasks: [{ start: 1, ms: 1, marks: [7] }] })).toBeNull();
    const many = Array.from({ length: KEPT_TASKS + 1 }, () => ({ start: 1, ms: 60, marks: [] }));
    expect(parsePageLongTasks({ ...good, tasks: many })).toBeNull();
  });
});

describe('records and the report', () => {
  const where = { test: 'flows: save and reload', launch: 2, platform: 'linux' as const };
  const found: PageLongTasks = {
    begin: 1_000,
    end: 13_345.67,
    beats: 3_000,
    count: 3,
    totalMs: 412.345,
    hiddenMs: 0,
    tasks: [
      { start: 3_000, ms: 90.04, marks: [] },
      { start: 5_000.06, ms: 250.26, marks: ['b2c:toolbox'] },
      { start: 9_000, ms: 72, marks: [] },
    ],
  };
  const folders: string[] = [];

  afterEach(() => {
    for (const folder of folders.splice(0)) {
      rmSync(folder, { recursive: true, force: true });
    }
  });

  it('turns what the page found into a record, times from the recorder start', () => {
    const record = longTaskRecord(found, where);
    expect(record).toMatchObject({
      ...where,
      longTaskMs: LONG_TASK_MS,
      recordedMs: 12_345.7,
      meanBeatMs: 4.1,
      longTasks: 3,
      longTasksTotalMs: 412.3,
      longest: { atMs: 4_000.1, ms: 250.3, marks: ['b2c:toolbox'] },
    });
    expect(record.top.map((task) => task.ms)).toEqual([250.3, 90, 72]);
    expect(summaryLine(record)).toBe(
      'Long tasks of flows: save and reload (launch 2): the longest 250.3 ms at 4.0 s ' +
        '(b2c:toolbox); 3 of 50 ms or more in 12.3 s',
    );
  });

  it('says when no task was long, and when the record was lost', () => {
    const quiet = longTaskRecord({ ...found, count: 0, totalMs: 0, tasks: [] }, where);
    expect(quiet.longest).toBeNull();
    expect(summaryLine(quiet)).toContain('none of 50 ms or more in 12.3 s');
    const lost = longTaskRecord(null, where);
    expect(lost.recordedMs).toBeNull();
    expect(summaryLine(lost)).toContain('not recorded');
  });

  it('summarises the report in Markdown, skipping malformed lines', () => {
    const lines = [
      JSON.stringify(longTaskRecord(found, where)),
      'not json',
      JSON.stringify({ test: 'x' }),
      JSON.stringify(longTaskRecord(null, { ...where, test: 'a | b', launch: 1 })),
    ].join('\n');
    const summary = longTaskSummary(lines);
    expect(summary).toContain('### Longest main-thread tasks');
    expect(summary).toContain(
      '| flows: save and reload | 2 | linux | 12.3 s (4.1 ms) | 250.3 ms | 3 (412.3 ms) | b2c:toolbox |',
    );
    expect(summary).toContain('| a b | 1 | linux | lost | — | 0 (0 ms) | — |');
    expect(longTaskSummary('')).toContain('(no launch recorded)');
  });

  it('starts and finishes through WebDriver, saving the artifact, the report and the summary', async () => {
    const root = mkdtempSync(path.join(tmpdir(), 'b2c-long-tasks-'));
    folders.push(root);
    const { loop, start, stop } = page();
    const executeScript = vi.fn((script: string) => {
      if (script === LONG_TASKS_START_SCRIPT) {
        return Promise.resolve(start());
      }
      loop.busy(70, 50);
      loop.runUntil(loop.now + 200);
      return Promise.resolve(stop());
    });
    const driver = { executeScript } as unknown as WebDriver;
    const output = vi.spyOn(process.stdout, 'write').mockReturnValue(true);
    try {
      await startLongTaskRecorder(driver);
      const record = await finishLongTaskRecorder(driver, { root, test: 'exit test', launch: 1 });
      expect(record.longest?.ms).toBeGreaterThanOrEqual(70);
      expect(output).toHaveBeenCalledWith(expect.stringContaining('Long tasks of exit test'));
    } finally {
      output.mockRestore();
    }
    const artifact = JSON.parse(
      readFileSync(path.join(root, 'exit-test', 'long-tasks-1.json'), 'utf8'),
    ) as unknown;
    expect(artifact).toMatchObject({ test: 'exit test', launch: 1, longTasks: 1 });
    expect(readFileSync(path.join(root, LONG_TASKS_REPORT), 'utf8').split('\n')).toHaveLength(2);
    writeLongTaskSummary(root);
    expect(readFileSync(path.join(root, LONG_TASKS_SUMMARY), 'utf8')).toContain(
      '| exit test | 1 |',
    );
    clearLongTasks(root);
    writeLongTaskSummary(root);
    expect(readFileSync(path.join(root, LONG_TASKS_SUMMARY), 'utf8')).toContain(
      '(no launch recorded)',
    );
  });

  it('never fails the test: a page that cannot be asked gives a lost record', async () => {
    const root = mkdtempSync(path.join(tmpdir(), 'b2c-long-tasks-'));
    folders.push(root);
    const driver = {
      executeScript: vi.fn(() => Promise.reject(new Error('no such window'))),
    } as unknown as WebDriver;
    const errors = vi.spyOn(process.stderr, 'write').mockReturnValue(true);
    const output = vi.spyOn(process.stdout, 'write').mockReturnValue(true);
    try {
      await startLongTaskRecorder(driver);
      const record = await finishLongTaskRecorder(driver, { root, test: 'gone', launch: 3 });
      expect(record.recordedMs).toBeNull();
      expect(errors).toHaveBeenCalledWith(expect.stringContaining('no such window'));
    } finally {
      errors.mockRestore();
      output.mockRestore();
    }
    expect(readFileSync(path.join(root, LONG_TASKS_REPORT), 'utf8')).toContain('"recordedMs":null');
  });
});
