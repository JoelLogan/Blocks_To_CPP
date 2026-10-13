/**
 * The webview benchmarks' interactions with the editor, timed inside the page (bench/page.ts):
 * an edit made through the test hook (its preview, and everything it starts until the page is
 * quiet again) and a click on a block with real pointer input (the longest task it causes). The
 * preview, selection and guessing-game benchmarks use them. Each also gives the app's User Timing
 * spans of the interaction (src/lib/timing.ts), which the benchmarks report next to their metrics
 * to say where the time went; an app without them gives none. A click is checked to have rebuilt
 * the toolbox for the block it selected, so a sample never lacks that work.
 */
import { Button, Key, Origin } from 'selenium-webdriver';

import { E2E_HOOK_NAME, type E2ePoint } from '../../src/e2e/contract';
import { type App, SCRIPT_TIMEOUT_MS } from '../support/app';
import type { BlockNode } from '../support/bdm';
import { scrollIntoView } from '../support/canvas';
import { blockCorner, grabPointOf } from '../support/editor';
import { byTestId, middleOf, pressKey } from '../support/ui';
import {
  type ClickResult,
  EDIT_SCRIPT,
  type EditResult,
  GONE_SCRIPT,
  longestAfterPress,
  MAX_SPANS,
  type PageSpan,
  PREVIEW_DEBOUNCE_MS,
  previewTask,
  QUIET_SCRIPT,
  TASKS_KEY,
  TASKS_SETTLE_SCRIPT,
  TASKS_START_SCRIPT,
} from './page';
import { median, rounded } from './stats';
import type { ClickTarget } from './targets';

/** The shortest main-thread task the heartbeats record (bench/page.ts). */
export const LONG_TASK_MS = 4;

/**
 * How long the page must run no long task before what an interaction started counts as done: the
 * editor's follow-up work (the toolbox, the C++ panel) comes in tasks close behind each other.
 */
export const SETTLE_MS = 300;

/**
 * How long an edit's preview, or a click's work, may take before the benchmark fails: below
 * WebDriver's script timeout (support/app.ts `SCRIPT_TIMEOUT_MS`), so the page's own timeout is
 * what reports it.
 */
export const INTERACTION_TIMEOUT_MS = 25_000;

/** How much longer than the page's own timeout WebDriver waits for its answer. */
const SCRIPT_TIMEOUT_MARGIN_MS = 5_000;

/** The time of each User Timing span of the app in one interaction, by name (milliseconds). */
export type SpanTimes = Readonly<Record<string, number>>;

function isPageSpan(value: unknown): value is PageSpan {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const span = value as Record<string, unknown>;
  return (
    typeof span['name'] === 'string' &&
    span['name'].startsWith('b2c:') &&
    span['name'].length <= 80 &&
    typeof span['start'] === 'number' &&
    Number.isFinite(span['start']) &&
    typeof span['ms'] === 'number' &&
    Number.isFinite(span['ms']) &&
    span['ms'] >= 0
  );
}

/**
 * The time of each span name that began at `from` or later (page clock), added up: the spans of
 * the interaction that began then. What the page returned is checked; anything else is none.
 */
export function spanTimes(spans: unknown, from: number): SpanTimes {
  const times: Record<string, number> = {};
  if (!Array.isArray(spans) || spans.length > MAX_SPANS) {
    return times;
  }
  for (const span of spans) {
    if (isPageSpan(span) && span.start >= from) {
      times[span.name] = (times[span.name] ?? 0) + span.ms;
    }
  }
  return times;
}

/** How many spans {@link spanSummary} names. */
export const SUMMARY_SPANS = 6;

/**
 * One line naming the spans with the longest medians over the interactions (an interaction where
 * a span did not run counts 0 for it), longest first: `b2c:toolbox-refresh 812.4 ms, …`. Says so
 * when the app measured no span.
 */
export function spanSummary(interactions: readonly SpanTimes[]): string {
  const names = new Set<string>();
  for (const times of interactions) {
    for (const name of Object.keys(times)) {
      names.add(name);
    }
  }
  if (names.size === 0 || interactions.length === 0) {
    return 'no User Timing spans (this build of the app measures none)';
  }
  const medians = [...names].map((name) => ({
    name,
    ms: median(interactions.map((times) => times[name] ?? 0)),
  }));
  medians.sort((a, b) => b.ms - a.ms || a.name.localeCompare(b.name));
  return medians
    .slice(0, SUMMARY_SPANS)
    .map((span) => `${span.name} ${String(rounded(span.ms, 1))} ms`)
    .join(', ');
}

/** One timed edit (milliseconds). */
export interface TimedEdit {
  /** The inserted block. */
  readonly id: string;
  /** The preview pipeline's task, or `null` when it was shorter than {@link LONG_TASK_MS}. */
  readonly previewMs: number | null;
  /** From the edit to its C++ in the editor's state, less the preview's debounce. */
  readonly editMs: number;
  /**
   * From the edit until the page had finished what it started (without a settle time: until its
   * C++ was in the editor's state), less the preview's debounce.
   */
  readonly settledMs: number;
  /** The longest task from the edit until then (0 when none was as long as a long task). */
  readonly longestMs: number;
  /** The app's spans from the edit until the page called back. */
  readonly spans: SpanTimes;
}

/** What an edit measured, from {@link EDIT_SCRIPT}'s result; throws when the edit failed. */
export function editMeasurement(id: string, result: unknown): TimedEdit {
  if (typeof result !== 'object' || result === null || !('kind' in result)) {
    throw new Error('The edit script returned something unexpected');
  }
  const edit = result as EditResult;
  if (edit.kind === 'error') {
    throw new Error(`The edit failed: ${edit.message}`);
  }
  if (edit.kind === 'timeout') {
    throw new Error(
      `The edit's preview did not show, or the page did not settle, within ${String(INTERACTION_TIMEOUT_MS / 1000)} s`,
    );
  }
  const less = (end: number) => Math.max(0, end - edit.inserted - PREVIEW_DEBOUNCE_MS);
  return {
    id,
    previewMs: previewTask(edit)?.ms ?? null,
    editMs: less(edit.seen),
    settledMs: less(edit.settled),
    longestMs: edit.longest?.ms ?? 0,
    spans: spanTimes(edit.spans, edit.begin),
  };
}

/** Options of {@link timedEdit}. */
export interface EditOptions {
  /** The block the edit adds to, and its statement input. */
  readonly parentId: string;
  readonly input: string;
  /** Text only the edit's C++ contains. */
  readonly marker: string;
  /** How long the page must be quiet after the preview; 0 stops timing at the preview. */
  readonly settleMs: number;
}

/** Adds `block` (which shows `options.marker` in the C++) through the test hook and times it. */
export async function timedEdit(
  app: App,
  block: BlockNode,
  options: EditOptions,
): Promise<TimedEdit> {
  const result: unknown = await app.driver.executeAsyncScript(
    EDIT_SCRIPT,
    E2E_HOOK_NAME,
    options.parentId,
    options.input,
    [block],
    options.marker,
    INTERACTION_TIMEOUT_MS,
    LONG_TASK_MS,
    options.settleMs,
  );
  return editMeasurement(block.id, result);
}

/**
 * Deletes block `id` with the keyboard (selected, then Delete) and waits until `marker` has left
 * the C++; with `quiet`, also until the page has finished what the deletion started.
 */
export async function deleteBlock(
  app: App,
  id: string,
  marker: string,
  quiet = false,
): Promise<void> {
  await app.hook.selectBlock(id);
  await pressKey(app.driver, Key.DELETE);
  const gone: unknown = await app.driver.executeAsyncScript(
    GONE_SCRIPT,
    E2E_HOOK_NAME,
    marker,
    INTERACTION_TIMEOUT_MS,
  );
  if (gone !== 'gone') {
    throw new Error(
      `Deleting block ${id} did not remove "${marker}" from the C++ (${String(gone)})`,
    );
  }
  if (quiet) {
    await waitUntilQuiet(app, `deleting block ${id}`);
  }
}

/** Animation frames in a row within {@link QUIET_FRAME_MS} that mean the page is idle. */
const QUIET_FRAMES = 10;
const QUIET_FRAME_MS = 50;

/** Waits until the page has finished what `after` started (bench/page.ts `QUIET_SCRIPT`). */
export async function waitUntilQuiet(app: App, after: string): Promise<void> {
  const result: unknown = await app.driver.executeAsyncScript(
    QUIET_SCRIPT,
    QUIET_FRAMES,
    QUIET_FRAME_MS,
    INTERACTION_TIMEOUT_MS,
  );
  if (result !== 'quiet') {
    throw new Error(`The page did not settle after ${after} (${String(result)})`);
  }
}

/** Whether block `id` is the selected one (Blockly marks its SVG group `blocklySelected`). */
export async function isSelected(app: App, id: string): Promise<boolean> {
  const selected: unknown = await app.driver.executeScript(
    `const [hookName, id] = arguments;
     const element = window[hookName].blockElement(id);
     return element !== null && element.classList.contains('blocklySelected');`,
    E2E_HOOK_NAME,
    id,
  );
  return selected === true;
}

/**
 * Scrolls block `id` into view through the test hook, so that the selection stays as it is, and
 * returns the point where a press grabs it.
 */
export async function revealBlock(app: App, id: string): Promise<E2ePoint> {
  await scrollIntoView(app.driver, app.hook, () => blockCorner(app, id));
  return scrollIntoView(app.driver, app.hook, () => grabPointOf(app, id));
}

/** What a click measured (milliseconds). */
export interface TimedClick {
  /** The longest task from the press on (0 when none was as long as a long task). */
  readonly longestMs: number;
  /** From the press until the page had finished what the click started. */
  readonly respondMs: number;
  /** The app's spans from the press until the page had finished. */
  readonly spans: SpanTimes;
}

/** What a click measured, from {@link TASKS_SETTLE_SCRIPT}'s result; throws when it failed. */
export function clickMeasurement(id: string, result: unknown): TimedClick {
  if (typeof result !== 'object' || result === null || !('kind' in result)) {
    throw new Error('The task recorder returned something unexpected');
  }
  const click = result as ClickResult;
  switch (click.kind) {
    case 'settled':
      return {
        longestMs: longestAfterPress(click)?.ms ?? 0,
        respondMs: Math.max(0, click.settled - click.pressed),
        spans: spanTimes(click.spans, click.pressed),
      };
    case 'no-press':
      throw new Error(`The click on block ${id} never reached the page`);
    case 'busy':
      throw new Error(
        `The page was still busy ${String(INTERACTION_TIMEOUT_MS / 1000)} s after the click on block ${id}`,
      );
    case 'error':
      throw new Error(`Timing the click on block ${id} failed: ${click.message}`);
  }
}

/**
 * What is wrong with the toolbox's listing after a click on `target`, given the symbols of
 * `target.lists` and `target.omits` whose getters the flyout shows (`listed`); `null` when it is
 * the target's listing.
 */
export function listingMismatch(target: ClickTarget, listed: ReadonlySet<string>): string | null {
  const missing = target.lists.filter((sym) => !listed.has(sym));
  const extra = target.omits.filter((sym) => listed.has(sym));
  if (missing.length === 0 && extra.length === 0) {
    return null;
  }
  const parts = [
    ...(missing.length > 0 ? [`no getter of ${missing.join(', ')}`] : []),
    ...(extra.length > 0 ? [`a getter of ${extra.join(', ')}`] : []),
  ];
  return (
    `After the click on block ${target.id} the toolbox shows ${parts.join(' and ')}: ` +
    'it was not rebuilt for the new selection while the click was timed'
  );
}

/**
 * Which of `symbols` the toolbox's flyout shows a getter (`var.get`) of, asked in one script.
 */
async function listedGetters(app: App, symbols: readonly string[]): Promise<Set<string>> {
  const listed: unknown = await app.driver.executeScript(
    `const [hookName, symbols] = arguments;
     return symbols.filter((sym) => window[hookName].flyoutBlockId('var.get', { VAR: sym }) !== null);`,
    E2E_HOOK_NAME,
    symbols,
  );
  if (!Array.isArray(listed) || !listed.every((sym) => typeof sym === 'string')) {
    throw new Error('The page did not say which getters the toolbox shows');
  }
  return new Set(listed);
}

/**
 * Clicks block `target.id` with real pointer input, holding the button 50 ms as a person's click
 * does, and times what the click starts (bench/page.ts `TASKS_START_SCRIPT`). The block is
 * scrolled into view first and the page left to settle, both outside the timing. The block must
 * not be selected before and must be selected after: the click changes the selection. Once the
 * page has settled, the toolbox must list what the block sees (`target.lists` and
 * `target.omits`): a click whose rebuild of the toolbox did not happen while it was timed fails
 * the run instead of giving a sample without that work.
 */
export async function timedClick(app: App, target: ClickTarget): Promise<TimedClick> {
  const id = target.id;
  const point = await revealBlock(app, id);
  await waitUntilQuiet(app, `scrolling block ${id} into view`);
  if (await isSelected(app, id)) {
    throw new Error(`Block ${id} is selected already, so a click would not change the selection`);
  }
  const started: unknown = await app.driver.executeScript(
    TASKS_START_SCRIPT,
    TASKS_KEY,
    LONG_TASK_MS,
  );
  if (started !== true) {
    throw new Error('A task recorder was already running in the page');
  }
  await app.driver
    .actions({ async: true })
    .move({ x: Math.round(point.x), y: Math.round(point.y), origin: Origin.VIEWPORT })
    .press(Button.LEFT)
    .pause(50)
    .release(Button.LEFT)
    .perform();
  const result: unknown = await app.driver.executeAsyncScript(
    TASKS_SETTLE_SCRIPT,
    TASKS_KEY,
    SETTLE_MS,
    INTERACTION_TIMEOUT_MS,
  );
  const timed = clickMeasurement(id, result);
  if (!(await isSelected(app, id))) {
    throw new Error(
      `The click at (${String(point.x)}, ${String(point.y)}) did not select block ${id}`,
    );
  }
  const mismatch = listingMismatch(
    target,
    await listedGetters(app, [...target.lists, ...target.omits]),
  );
  if (mismatch !== null) {
    throw new Error(mismatch);
  }
  return timed;
}

/** The console's state line, and what it says once a program has ended (panels/console/header.ts). */
export const CONSOLE_STATE = '[data-testid="console-state"]';
export const CONSOLE_FINISHED = 'Finished';

/** What a run measured (milliseconds). */
export interface TimedRun {
  /**
   * The longest task from the press on ► Run until the page had finished what the run started
   * (0 when none was as long as a long task).
   */
  readonly longestMs: number;
  /**
   * From the press until the console's state said the program had finished. The console takes
   * the output in order before the end of the run, and its work keeps the page busy most of that
   * time, so this is how long the console took to show it; the page's few short tasks after
   * that (the run's end in the window's chrome) are left out, as their timers make it noisy.
   */
  readonly runMs: number;
  /** The app's spans from the press until the page had finished. */
  readonly spans: SpanTimes;
}

/**
 * What a run measured, from {@link TASKS_SETTLE_SCRIPT}'s result; throws when it failed.
 * `timeoutMs` is the time the page was given ({@link timedRun}), for the message.
 */
export function runMeasurement(
  result: unknown,
  timeoutMs: number = INTERACTION_TIMEOUT_MS,
): TimedRun {
  if (typeof result !== 'object' || result === null || !('kind' in result)) {
    throw new Error('The task recorder returned something unexpected');
  }
  const run = result as ClickResult;
  switch (run.kind) {
    case 'settled':
      if (run.until === null) {
        throw new Error('The run was timed without waiting for the console to say it finished');
      }
      return {
        longestMs: longestAfterPress(run)?.ms ?? 0,
        runMs: Math.max(0, run.until - run.pressed),
        spans: spanTimes(run.spans, run.pressed),
      };
    case 'no-press':
      throw new Error('The click on ► Run never reached the page');
    case 'busy':
      throw new Error(
        `The program had not finished, or the page was still busy, ${String(timeoutMs / 1000)} s after the click on ► Run`,
      );
    case 'error':
      throw new Error(`Timing the run failed: ${run.message}`);
  }
}

/**
 * Clicks ► Run in the toolbar with real pointer input (the button held 50 ms) and times the run
 * until the console's state says the program has finished, then records the page's tasks until
 * it has run no long task for {@link SETTLE_MS} (bench/page.ts `TASKS_SETTLE_SCRIPT` with the
 * state line to wait for). The project must have been built already, so the run does not include
 * a compile, and the page is left to settle first, outside the timing. The run may take
 * `timeoutMs`; WebDriver's script timeout is raised for it when that is longer than the session's,
 * and set back afterwards.
 */
export async function timedRun(
  app: App,
  timeoutMs: number = INTERACTION_TIMEOUT_MS,
): Promise<TimedRun> {
  await waitUntilQuiet(app, 'before the run');
  const point = await middleOf(app.driver, await app.driver.findElement(byTestId('toolbar-run')));
  const started: unknown = await app.driver.executeScript(
    TASKS_START_SCRIPT,
    TASKS_KEY,
    LONG_TASK_MS,
  );
  if (started !== true) {
    throw new Error('A task recorder was already running in the page');
  }
  await app.driver
    .actions({ async: true })
    .move({ x: Math.round(point.x), y: Math.round(point.y), origin: Origin.VIEWPORT })
    .press(Button.LEFT)
    .pause(50)
    .release(Button.LEFT)
    .perform();
  const longer = timeoutMs + SCRIPT_TIMEOUT_MARGIN_MS > SCRIPT_TIMEOUT_MS;
  if (longer) {
    await app.driver.manage().setTimeouts({ script: timeoutMs + SCRIPT_TIMEOUT_MARGIN_MS });
  }
  let result: unknown;
  try {
    result = await app.driver.executeAsyncScript(
      TASKS_SETTLE_SCRIPT,
      TASKS_KEY,
      SETTLE_MS,
      timeoutMs,
      CONSOLE_STATE,
      CONSOLE_FINISHED,
    );
  } finally {
    if (longer) {
      await app.driver.manage().setTimeouts({ script: SCRIPT_TIMEOUT_MS });
    }
  }
  return runMeasurement(result, timeoutMs);
}
