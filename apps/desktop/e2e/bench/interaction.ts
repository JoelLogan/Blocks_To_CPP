/**
 * The webview benchmarks' interactions with the editor, timed inside the page (bench/page.ts):
 * an edit made through the test hook (its preview, and everything it starts until the page is
 * quiet again) and a click on a block with real pointer input (the longest task it causes). The
 * preview, selection and guessing-game benchmarks use them.
 */
import { Button, Key, Origin } from 'selenium-webdriver';

import { E2E_HOOK_NAME, type E2ePoint } from '../../src/e2e/contract';
import type { App } from '../support/app';
import type { BlockNode } from '../support/bdm';
import { scrollIntoView } from '../support/canvas';
import { blockCorner, grabPointOf } from '../support/editor';
import { pressKey } from '../support/ui';
import {
  type ClickResult,
  EDIT_SCRIPT,
  type EditResult,
  GONE_SCRIPT,
  longestAfterPress,
  PREVIEW_DEBOUNCE_MS,
  previewTask,
  QUIET_SCRIPT,
  TASKS_KEY,
  TASKS_SETTLE_SCRIPT,
  TASKS_START_SCRIPT,
} from './page';

/** The shortest main-thread task the heartbeats record (bench/page.ts). */
export const LONG_TASK_MS = 4;

/**
 * How long the page must run no long task before what an interaction started counts as done: the
 * editor's follow-up work (the toolbox, the C++ panel) comes in tasks close behind each other.
 */
export const SETTLE_MS = 300;

/**
 * How long an edit's preview, or a click's work, may take before the benchmark fails: below
 * WebDriver's script timeout (30 s, support/app.ts), so the page's own timeout is what reports it.
 */
export const INTERACTION_TIMEOUT_MS = 25_000;

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
 * Clicks block `id` with real pointer input and times what the click starts (bench/page.ts
 * `TASKS_START_SCRIPT`). The block is scrolled into view first and the page left to settle, both
 * outside the timing. The block must not be selected before and must be selected after: the
 * click changes the selection.
 */
export async function timedClick(app: App, id: string): Promise<TimedClick> {
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
  return timed;
}
