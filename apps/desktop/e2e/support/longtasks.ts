/**
 * The long-task recorder (docs/spec/09-quality-and-delivery.md §9.2 "Benchmarks": every E2E flow
 * records its longest main-thread task). `launchApp` starts it in the page once the window is
 * ready, and reads it when the test finishes; the flow tests' launcher does the same for each of
 * its launches. What it found goes to the test's artifacts (`long-tasks-<launch>.json`), to a line
 * of the run's report (`long-tasks.jsonl`, summarised as `long-tasks.md` when the run ends) and to
 * a summary line on standard output. It is a report only (N12 is met in M5): it never fails a
 * test, and a record that cannot be read is reported as lost.
 *
 * In the page a heartbeat, a timer that sets itself again as soon as it runs, notes the gaps
 * between its beats: while the page runs something else (a task, its rendering) the beat waits,
 * so a gap of {@link LONG_TASK_MS} or more is a long task. Browsers run such a timer every few
 * milliseconds (4 ms at the least; each record has the mean beat), which costs nothing
 * measurable; a gap is therefore an upper bound of the task in it, at most one beat longer. (The
 * benchmarks time their own tasks exactly with a message the page posts to itself again and
 * again, bench/page.ts; kept running for a whole test, that heartbeat takes about one and a half
 * cores of WebKitGTK, which would slow the flows down.) Gaps while the window is hidden are not
 * counted: browsers slow hidden pages' timers down.
 *
 * User Timing entries whose names start with `b2c:` (marks, and measures, which the app may add
 * around its pipeline, toolbox refresh and selection handling) that overlap a recorded task are
 * named with it, so the report can say what ran; without them the report has the times only.
 */
import {
  appendFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import path from 'node:path';

import type { WebDriver } from 'selenium-webdriver';

import { artifactDir, writeArtifact } from './artifacts';
import { withTimeout } from './wait';

/** The window property the recorder keeps itself under while it runs. */
export const LONG_TASKS_KEY = '__B2C_E2E_LONG_TASKS__';

/** A main-thread task of at least this many milliseconds counts as long (N12's 50 ms). */
export const LONG_TASK_MS = 50;

/** How many of the longest tasks a record keeps. */
export const KEPT_TASKS = 10;

/** The most `b2c:` User Timing names a task lists. */
export const MAX_MARKS = 8;

/** The run's report: one JSON object per recorded launch. */
export const LONG_TASKS_REPORT = 'long-tasks.jsonl';

/** The Markdown summary of {@link LONG_TASKS_REPORT}, for the CI job's summary. */
export const LONG_TASKS_SUMMARY = 'long-tasks.md';

/** How long starting or reading the recorder may take. */
const SCRIPT_TIMEOUT_MS = 10_000;

/**
 * Runs in the webview (`executeScript(LONG_TASKS_START_SCRIPT, key, longTaskMs, keep)`): starts
 * the recorder; `false` when one already runs. Only the recorder's own state is added to the page.
 */
export const LONG_TASKS_START_SCRIPT = `
  const [key, longTaskMs, keep] = arguments;
  if (Object.prototype.hasOwnProperty.call(window, key)) {
    return false;
  }
  const begin = performance.now();
  const state = { beats: 0, count: 0, totalMs: 0, hiddenMs: 0, tasks: [], stopped: false };
  let last = begin;
  const note = (start, ms) => {
    state.count += 1;
    state.totalMs += ms;
    const tasks = state.tasks;
    if (tasks.length < keep || ms > tasks[tasks.length - 1].ms) {
      tasks.push({ start, ms });
      tasks.sort((a, b) => b.ms - a.ms);
      if (tasks.length > keep) {
        tasks.pop();
      }
    }
  };
  const beat = () => {
    if (state.stopped) {
      return;
    }
    const now = performance.now();
    const gap = now - last;
    if (document.visibilityState === 'hidden') {
      state.hiddenMs += gap;
    } else if (gap >= longTaskMs) {
      note(last, gap);
    }
    last = now;
    state.beats += 1;
    setTimeout(beat, 0);
  };
  setTimeout(beat, 0);
  const stop = () => {
    state.stopped = true;
    return { begin, end: performance.now(), beats: state.beats, count: state.count,
      totalMs: state.totalMs, hiddenMs: state.hiddenMs, tasks: state.tasks.slice() };
  };
  Object.defineProperty(window, key, { value: { stop }, configurable: true });
  return true;
`;

/**
 * Runs in the webview (`executeScript(LONG_TASKS_STOP_SCRIPT, key, maxMarks)`): stops the recorder
 * and returns what it found (a {@link PageLongTasks}), with the `b2c:` User Timing names that
 * overlap each kept task; `null` when no recorder runs (the page was replaced since it started).
 */
export const LONG_TASKS_STOP_SCRIPT = `
  const [key, maxMarks] = arguments;
  const recorder = window[key];
  if (typeof recorder !== 'object' || recorder === null || typeof recorder.stop !== 'function') {
    return null;
  }
  const found = recorder.stop();
  delete window[key];
  let entries = [];
  try {
    entries = performance.getEntriesByType('mark').concat(performance.getEntriesByType('measure'))
      .filter((entry) => typeof entry.name === 'string' && entry.name.startsWith('b2c:'));
  } catch (error) {
    entries = [];
  }
  found.tasks = found.tasks.map((task) => {
    const names = [];
    for (const entry of entries) {
      const end = entry.startTime + (entry.duration || 0);
      if (entry.startTime <= task.start + task.ms && end >= task.start && names.length < maxMarks
          && !names.includes(entry.name)) {
        names.push(entry.name.slice(0, 80));
      }
    }
    return { start: task.start, ms: task.ms, marks: names };
  });
  return found;
`;

/** A recorded task, on the page clock (milliseconds). */
export interface PageTask {
  readonly start: number;
  readonly ms: number;
  /** The `b2c:` User Timing names that overlap the task. */
  readonly marks: readonly string[];
}

/** What {@link LONG_TASKS_STOP_SCRIPT} returns (page clock, milliseconds). */
export interface PageLongTasks {
  /** When the recorder started. */
  readonly begin: number;
  /** When it stopped. */
  readonly end: number;
  readonly beats: number;
  /** How many long tasks there were, and their time together. */
  readonly count: number;
  readonly totalMs: number;
  /** The time the window was hidden (not counted). */
  readonly hiddenMs: number;
  /** The longest tasks, longest first, at most {@link KEPT_TASKS}. */
  readonly tasks: readonly PageTask[];
}

/** A task in a record: when it began, in milliseconds since the recorder started. */
export interface RecordedTask {
  readonly atMs: number;
  readonly ms: number;
  readonly marks: readonly string[];
}

/** What the recorder found during one launch of the app. */
export interface LongTaskRecord {
  readonly test: string;
  /** Which launch of the test this was, from 1. */
  readonly launch: number;
  readonly platform: NodeJS.Platform;
  /** The threshold of a long task. */
  readonly longTaskMs: number;
  /** `null` when the record was lost: the page was replaced, or the app could not be asked. */
  readonly recordedMs: number | null;
  /** The mean time between beats, which bounds how much longer a gap is than its task. */
  readonly meanBeatMs: number | null;
  readonly longTasks: number;
  readonly longTasksTotalMs: number;
  /** The longest task, or `null` when none was long. */
  readonly longest: RecordedTask | null;
  /** The longest tasks, longest first. */
  readonly top: readonly RecordedTask[];
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

/** Whether `value` is a {@link PageTask} (it comes from the page, so it is checked). */
function isPageTask(value: unknown): value is PageTask {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const task = value as Partial<Record<keyof PageTask, unknown>>;
  return (
    isFiniteNumber(task.start) &&
    isFiniteNumber(task.ms) &&
    task.ms >= 0 &&
    Array.isArray(task.marks) &&
    task.marks.length <= MAX_MARKS &&
    task.marks.every((mark) => typeof mark === 'string' && mark.length <= 80)
  );
}

/** What the page returned, checked; `null` when it is not a {@link PageLongTasks}. */
export function parsePageLongTasks(value: unknown): PageLongTasks | null {
  if (typeof value !== 'object' || value === null) {
    return null;
  }
  const found = value as Partial<Record<keyof PageLongTasks, unknown>>;
  const numbers = [found.begin, found.end, found.beats, found.count, found.totalMs, found.hiddenMs];
  if (
    !numbers.every(isFiniteNumber) ||
    !Array.isArray(found.tasks) ||
    found.tasks.length > KEPT_TASKS ||
    !found.tasks.every(isPageTask)
  ) {
    return null;
  }
  return found as PageLongTasks;
}

/** A number of milliseconds with one decimal. */
function tenth(value: number): number {
  return Math.round(value * 10) / 10;
}

/** The record of one launch from what the page found (`null`: the record was lost). */
export function longTaskRecord(
  found: PageLongTasks | null,
  where: { readonly test: string; readonly launch: number; readonly platform: NodeJS.Platform },
): LongTaskRecord {
  if (found === null) {
    return {
      ...where,
      longTaskMs: LONG_TASK_MS,
      recordedMs: null,
      meanBeatMs: null,
      longTasks: 0,
      longTasksTotalMs: 0,
      longest: null,
      top: [],
    };
  }
  const top = [...found.tasks]
    .sort((a, b) => b.ms - a.ms)
    .map((task) => ({
      atMs: tenth(task.start - found.begin),
      ms: tenth(task.ms),
      marks: [...task.marks],
    }));
  return {
    ...where,
    longTaskMs: LONG_TASK_MS,
    recordedMs: tenth(found.end - found.begin),
    meanBeatMs: tenth((found.end - found.begin) / Math.max(1, found.beats)),
    longTasks: found.count,
    longTasksTotalMs: tenth(found.totalMs),
    longest: top[0] ?? null,
    top,
  };
}

/** The line a test prints about its launch. */
export function summaryLine(record: LongTaskRecord): string {
  const which = `${record.test} (launch ${String(record.launch)})`;
  if (record.recordedMs === null) {
    return `Long tasks of ${which}: not recorded (the page was replaced or could not be asked)`;
  }
  const seconds = (record.recordedMs / 1000).toFixed(1);
  if (record.longest === null) {
    return `Long tasks of ${which}: none of ${String(record.longTaskMs)} ms or more in ${seconds} s`;
  }
  const marks = record.longest.marks.length === 0 ? '' : ` (${record.longest.marks.join(', ')})`;
  return (
    `Long tasks of ${which}: the longest ${String(record.longest.ms)} ms at ` +
    `${(record.longest.atMs / 1000).toFixed(1)} s${marks}; ${String(record.longTasks)} of ` +
    `${String(record.longTaskMs)} ms or more in ${seconds} s`
  );
}

/** Whether `value` is a {@link LongTaskRecord} (the report is read back from a file). */
function isRecord(value: unknown): value is LongTaskRecord {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const record = value as Partial<Record<keyof LongTaskRecord, unknown>>;
  const longest = record.longest as Partial<Record<keyof RecordedTask, unknown>> | null | undefined;
  return (
    typeof record.test === 'string' &&
    isFiniteNumber(record.launch) &&
    typeof record.platform === 'string' &&
    isFiniteNumber(record.longTaskMs) &&
    (record.recordedMs === null || isFiniteNumber(record.recordedMs)) &&
    (record.meanBeatMs === null || isFiniteNumber(record.meanBeatMs)) &&
    isFiniteNumber(record.longTasks) &&
    isFiniteNumber(record.longTasksTotalMs) &&
    (longest === null ||
      (typeof longest === 'object' &&
        isFiniteNumber(longest.atMs) &&
        isFiniteNumber(longest.ms) &&
        Array.isArray(longest.marks) &&
        longest.marks.every((mark) => typeof mark === 'string')))
  );
}

/** Text for a Markdown table cell: one line, no pipes, at most 120 characters. */
function cell(text: string): string {
  return text
    .replace(/[\s|]+/g, ' ')
    .trim()
    .slice(0, 120);
}

/** The Markdown summary of the report's lines (malformed lines are skipped). */
export function longTaskSummary(report: string): string {
  const records: LongTaskRecord[] = [];
  for (const line of report.split('\n')) {
    if (line.trim() === '') {
      continue;
    }
    try {
      const value: unknown = JSON.parse(line);
      if (isRecord(value)) {
        records.push(value);
      }
    } catch {
      // Not a line the harness wrote.
    }
  }
  const lines = [
    '### Longest main-thread tasks (report-only, docs/spec/09-quality-and-delivery.md §9.2)',
    '',
    `Each launch's longest task and its tasks of ${String(LONG_TASK_MS)} ms or more (N12's limit, ` +
      'met in M5), from a heartbeat in the page: a time is at most one beat (its mean is given) ' +
      'longer than the task. Never fails a test.',
    '',
    '| Test | Launch | Platform | Recorded (beat) | Longest task | Long tasks (their time) | Named in the longest |',
    '| --- | ---: | --- | ---: | ---: | ---: | --- |',
  ];
  for (const record of records) {
    const recorded =
      record.recordedMs === null
        ? 'lost'
        : `${(record.recordedMs / 1000).toFixed(1)} s (${String(record.meanBeatMs)} ms)`;
    const longest = record.longest === null ? '—' : `${String(record.longest.ms)} ms`;
    const marks =
      record.longest === null || record.longest.marks.length === 0
        ? '—'
        : record.longest.marks.join(', ');
    lines.push(
      `| ${cell(record.test)} | ${String(record.launch)} | ${cell(record.platform)} | ${recorded} | ` +
        `${longest} | ${String(record.longTasks)} (${String(record.longTasksTotalMs)} ms) | ${cell(marks)} |`,
    );
  }
  if (records.length === 0) {
    lines.push('| (no launch recorded) | | | | | | |');
  }
  return `${lines.join('\n')}\n`;
}

/** Deletes the report and summary of an earlier run in `root`. */
export function clearLongTasks(root: string): void {
  rmSync(path.join(root, LONG_TASKS_REPORT), { force: true });
  rmSync(path.join(root, LONG_TASKS_SUMMARY), { force: true });
}

/** Writes the Markdown summary of the report in `root` next to it. */
export function writeLongTaskSummary(root: string): void {
  const report = path.join(root, LONG_TASKS_REPORT);
  const text = existsSync(report) ? readFileSync(report, 'utf8') : '';
  mkdirSync(root, { recursive: true });
  writeFileSync(path.join(root, LONG_TASKS_SUMMARY), longTaskSummary(text));
}

/** Writes a launch's record: the test's artifact, the run's report and the summary line. */
export function saveLongTaskRecord(root: string, record: LongTaskRecord): void {
  try {
    const dir = artifactDir(root, record.test);
    writeArtifact(
      dir,
      `long-tasks-${String(record.launch)}.json`,
      `${JSON.stringify(record, null, 2)}\n`,
    );
    appendFileSync(path.join(root, LONG_TASKS_REPORT), `${JSON.stringify(record)}\n`);
  } catch (error: unknown) {
    process.stderr.write(`The long-task record could not be saved: ${String(error)}\n`);
  }
  process.stdout.write(`${summaryLine(record)}\n`);
}

/**
 * Starts the recorder in the app's page. Report-only: a failure is written to standard error and
 * otherwise ignored (the record of the launch is then lost).
 */
export async function startLongTaskRecorder(driver: WebDriver): Promise<void> {
  try {
    await withTimeout(
      driver.executeScript(LONG_TASKS_START_SCRIPT, LONG_TASKS_KEY, LONG_TASK_MS, KEPT_TASKS),
      SCRIPT_TIMEOUT_MS,
      'the long-task recorder to start',
    );
  } catch (error: unknown) {
    process.stderr.write(`The long-task recorder did not start: ${String(error)}\n`);
  }
}

/**
 * Stops the recorder of a launch and saves its record (see {@link saveLongTaskRecord}). Never
 * throws: a recorder that cannot be read gives a lost record.
 */
export async function finishLongTaskRecorder(
  driver: WebDriver,
  where: { readonly root: string; readonly test: string; readonly launch: number },
): Promise<LongTaskRecord> {
  let found: PageLongTasks | null = null;
  try {
    const value: unknown = await withTimeout(
      driver.executeScript(LONG_TASKS_STOP_SCRIPT, LONG_TASKS_KEY, MAX_MARKS),
      SCRIPT_TIMEOUT_MS,
      'the long-task recorder',
    );
    found = parsePageLongTasks(value);
  } catch (error: unknown) {
    process.stderr.write(`The long-task recorder could not be read: ${String(error)}\n`);
  }
  const record = longTaskRecord(found, {
    test: where.test,
    launch: where.launch,
    platform: process.platform,
  });
  saveLongTaskRecord(where.root, record);
  return record;
}
