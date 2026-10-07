/**
 * Webview benchmark: dragging in a 5,000-block workspace (docs/spec/01-overview.md §1.4 N3, 09
 * §9.2). The generated 5,000-block document with its drag handle (bench/document.ts: a separate
 * `func.define dragMe` beside `main`) is opened, and the handle is dragged away and back again
 * {@link PAIRS} times across empty canvas with real pointer input, in {@link STEPS} small moves per
 * drag. While the pointer drags, the page records the timestamps of its animation frames
 * (bench/page.ts `FRAMES_START_SCRIPT`); the p95 of the frame times of one pair (both directions,
 * about 120 frames) is one sample of `webview.drag-5000.frame-p95` (N3's 30 fps is 33.3 ms). A
 * first pair warms up and is not counted, and after every drop the page is left to finish what the
 * drop started (its preview) before the next drag.
 *
 * Before every drag, outside what is timed, the canvas is panned until the handle is well inside
 * the part of it that is visible then, and the drag ends at a point of that part: up and to the
 * right for the drag away, down and to the left for the drag back (bench/handle.ts `aimDelta`).
 * The toolbox's flyout stays open over the canvas's left part and its width changes as the toolbox
 * follows the program, and a drop on it would delete the handle; on the Windows runner the window
 * is small (about 1,030 by 750), so this leaves little canvas. After every drop the handle must
 * have moved with the pointer (bench/handle.ts), so a drag that missed it fails the run instead of
 * being measured, and a drop that loses it reports what the page looked like.
 */
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { Button, Origin, type WebDriver } from 'selenium-webdriver';
import { afterAll, describe, expect, it } from 'vitest';

import { E2E_HOOK_NAME, type E2ePoint } from '../../src/e2e/contract';
import { type App, launchApp } from '../support/app';
import { bringIntoView, visibleCanvas } from '../support/canvas';
import { centerOn, foldCodePanel } from '../support/editor';
import { waitFor } from '../support/wait';
import { handleId, MAIN_ID, writeBenchProject } from './document';
import { aimDelta, checkLanding, describePlace, type DragAim, isBlockPlace } from './handle';
import {
  type BlockPlace,
  FRAMES_KEY,
  FRAMES_START_SCRIPT,
  DROP_REPORT_SCRIPT,
  FRAMES_STOP_SCRIPT,
  isTimestamps,
  PLACE_SCRIPT,
  POINTER_KEY,
  POINTER_RECORD_SCRIPT,
  QUIET_SCRIPT,
} from './page';
import { openProjectFile } from './project';
import { writeMetric } from './results';
import { deltas, median, percentile, rounded } from './stats';

/** The workspace size. */
const BLOCKS = 5_000;

/**
 * Timed pairs of drags (away and back); each pair's p95 frame time is one sample (the comparison
 * needs at least 10).
 */
const PAIRS = 10;

/** Pointer moves per drag, and how long each takes: a drag of about a second. */
const STEPS = 60;
const STEP_MS = 16;

/** The fewest frame times a drag must give for its p95 to mean anything. */
const MIN_FRAMES = 20;

/** How long opening the document, rendering it and the first preview may take. */
const OPEN_TIMEOUT_MS = 300_000;

/** After a drop: this many frames in a row within {@link QUIET_FRAME_MS} mean the page is idle. */
const QUIET_FRAMES = 10;
const QUIET_FRAME_MS = 50;
const QUIET_TIMEOUT_MS = 60_000;

/**
 * How long the whole benchmark may take: on the Windows runner opening the document takes minutes,
 * and a drag with what its drop starts (the preview of 5,000 blocks, Blockly's rendering) takes
 * about 40 s, before and after each of which the page is left to settle; 22 drags in all.
 */
const TEST_TIMEOUT_MS = 2_400_000;

/** Notes progress on stderr, with the seconds since `since`, so a slow run shows where it is. */
function progress(since: number, text: string): void {
  process.stderr.write(
    `drag benchmark, ${String(Math.round((Date.now() - since) / 1000))} s: ${text}\n`,
  );
}

const folders: string[] = [];

afterAll(() => {
  for (const folder of folders) {
    rmSync(folder, { recursive: true, force: true });
  }
});

/** Drags with the left button from `from` to `to` in {@link STEPS} moves of {@link STEP_MS}. */
async function slowDrag(driver: WebDriver, from: E2ePoint, to: E2ePoint): Promise<void> {
  const x0 = Math.round(from.x);
  const y0 = Math.round(from.y);
  let actions = driver
    .actions({ async: true })
    .move({ x: x0, y: y0, origin: Origin.VIEWPORT })
    .press(Button.LEFT)
    .pause(100);
  for (let step = 1; step <= STEPS; step += 1) {
    actions = actions.move({
      x: Math.round(x0 + ((to.x - x0) * step) / STEPS),
      y: Math.round(y0 + ((to.y - y0) * step) / STEPS),
      origin: Origin.VIEWPORT,
      duration: STEP_MS,
    });
  }
  await actions.pause(100).release(Button.LEFT).perform();
}

/** Waits until the page has finished what `after` (a drop, a scroll) started. */
async function waitUntilQuiet(app: App, after: string): Promise<void> {
  const result: unknown = await app.driver.executeAsyncScript(
    QUIET_SCRIPT,
    QUIET_FRAMES,
    QUIET_FRAME_MS,
    QUIET_TIMEOUT_MS,
  );
  if (result !== 'quiet') {
    throw new Error(`The page was still busy ${String(QUIET_TIMEOUT_MS / 1000)} s after ${after}`);
  }
}

/** Where block `id` is now, or `null` when it is not on the canvas. */
async function placeOf(app: App, id: string): Promise<BlockPlace | null> {
  const place: unknown = await app.driver.executeScript(PLACE_SCRIPT, E2E_HOOK_NAME, id);
  if (place === null) {
    return null;
  }
  if (!isBlockPlace(place)) {
    throw new Error(`The page described where block ${id} is in an unexpected way`);
  }
  return place;
}

/** Where block `id` is, against the visible canvas, for an error message. */
async function whereIs(app: App, id: string): Promise<string> {
  const place = await placeOf(app, id);
  const canvas = await visibleCanvas(app.driver).catch(() => null);
  return describePlace(place, canvas);
}

/**
 * What the page looked like at a drop that went wrong, for the error message: the drag, whether
 * the handle is still in the document, and the page's report at the drop point.
 */
async function dropReport(app: App, id: string, grab: E2ePoint, end: E2ePoint): Promise<string> {
  const parts = [
    `The drag went from (${String(Math.round(grab.x))}, ${String(Math.round(grab.y))}) to (${String(Math.round(end.x))}, ${String(Math.round(end.y))})`,
  ];
  try {
    const inDocument = (await app.hook.documentText()).includes(`"${id}"`);
    parts.push(`the handle is ${inDocument ? 'still' : 'no longer'} in the document`);
    const report: unknown = await app.driver.executeScript(
      DROP_REPORT_SCRIPT,
      POINTER_KEY,
      Math.round(end.x),
      Math.round(end.y),
    );
    parts.push(`the page at the drop point: ${JSON.stringify(report)}`);
  } catch (error: unknown) {
    parts.push(`the page could not be asked (${String(error)})`);
  }
  return `${parts.join('; ')}.`;
}

/** One timed drag. */
interface TimedDrag {
  /** How far the pointer moved, in CSS pixels. */
  readonly delta: E2ePoint;
  /** The frame times recorded while the pointer dragged. */
  readonly times: number[];
  /** How far from the pointer's end the drop left the handle, in CSS pixels. */
  readonly landing: number;
}

/** How far inside the handle's box its top left corner is located for panning, in CSS pixels. */
const CORNER_INSET = 16;

/** The handle's top left corner, covered or not: where the canvas is panned to. */
async function cornerOf(app: App, id: string): Promise<E2ePoint> {
  const place = await placeOf(app, id);
  if (place === null) {
    throw new Error(`The drag handle ${id} is not on the canvas`);
  }
  return { x: place.box.left + CORNER_INSET, y: place.box.top + CORNER_INSET };
}

/**
 * Drags the handle `aim` ({@link aimDelta}) and returns the frame times recorded during the drag
 * (only the drag itself is timed), once the drop has settled and the handle is known to have moved
 * with the pointer. First, outside the timed part, the canvas is panned until the handle is well
 * inside the part of it that is visible now: the flyout's width changes as the toolbox follows the
 * program, and a small window leaves little canvas.
 */
async function timedDrag(app: App, id: string, aim: DragAim): Promise<TimedDrag> {
  await bringIntoView(app.driver, () => cornerOf(app, id));
  await waitUntilQuiet(app, 'panning the canvas to the drag handle');
  const before = await placeOf(app, id);
  if (before === null) {
    throw new Error(`The drag handle ${id} is not on the canvas`);
  }
  const grab = await waitFor(() => app.hook.grabPoint(id), {
    timeout: 10_000,
    interval: 200,
    message: async () =>
      `the drag handle ${id} to be on screen and not covered (${await whereIs(app, id)})`,
  });
  const delta = aimDelta(grab, await visibleCanvas(app.driver), aim);
  await app.driver.executeScript(POINTER_RECORD_SCRIPT, POINTER_KEY);
  const started: unknown = await app.driver.executeScript(FRAMES_START_SCRIPT, FRAMES_KEY);
  if (started !== true) {
    throw new Error('The frame recorder could not start (one is already running)');
  }
  let frames: unknown;
  try {
    await slowDrag(app.driver, grab, { x: grab.x + delta.x, y: grab.y + delta.y });
  } finally {
    // Stopped whatever happened, so a failed drag leaves no recorder running in the page.
    frames = await app.driver.executeScript(FRAMES_STOP_SCRIPT, FRAMES_KEY);
  }
  await waitUntilQuiet(app, 'a drop');
  const end = { x: grab.x + delta.x, y: grab.y + delta.y };
  let landing: number;
  try {
    landing = checkLanding(id, delta, before, await placeOf(app, id));
  } catch (error: unknown) {
    throw new Error(
      `${error instanceof Error ? error.message : String(error)}. ${await dropReport(app, id, grab, end)}`,
      { cause: error },
    );
  }
  if (!isTimestamps(frames)) {
    throw new Error('The frame recorder returned no timestamps');
  }
  const times = deltas(frames);
  if (times.length < MIN_FRAMES) {
    throw new Error(
      `Only ${String(times.length)} frame times were recorded during a drag (at least ${String(MIN_FRAMES)} are needed)`,
    );
  }
  return { delta, times, landing };
}

describe('webview benchmark: dragging in a 5,000-block workspace', () => {
  it(
    `times the frames of ${String(PAIRS)} pairs of drags`,
    async (context) => {
      const folder = mkdtempSync(path.join(tmpdir(), 'b2c-bench-drag-'));
      folders.push(folder);
      const file = writeBenchProject(folder, { blocks: BLOCKS, dragHandle: true });
      const handle = handleId(BLOCKS);
      const started = Date.now();
      const app = await launchApp(context, { dialogs: { open: [file] } });
      await openProjectFile(app, {
        blockId: MAIN_ID,
        code: 'void dragMe()',
        timeout: OPEN_TIMEOUT_MS,
      });
      await foldCodePanel(app);
      await centerOn(app, handle);
      await waitUntilQuiet(app, 'centring the canvas on the drag handle');
      progress(started, `opened the ${String(BLOCKS)}-block document`);

      const samples: number[] = [];
      let worstLanding = 0;
      for (let pair = 0; pair <= PAIRS; pair += 1) {
        const away = await timedDrag(app, handle, 'away');
        const back = await timedDrag(app, handle, 'back');
        worstLanding = Math.max(worstLanding, away.landing, back.landing);
        progress(
          started,
          `pair ${String(pair)} of ${String(PAIRS)} done (${pair === 0 ? 'warm-up' : 'timed'})`,
        );
        // The first pair warms up (Blockly's drag surface, the connection database): not kept.
        if (pair > 0) {
          samples.push(percentile([...away.times, ...back.times], 95));
        }
      }
      writeMetric({
        name: 'webview.drag-5000.frame-p95',
        unit: 'ms',
        better: 'lower',
        gate: true,
        description: `p95 frame time dragging a block away and back in a ${String(BLOCKS)}-block workspace; one pair per sample`,
        samples,
      });
      process.stdout.write(
        `Dragging at ${String(BLOCKS)} blocks: p95 frame time per pair, median ${String(rounded(median(samples), 1))} ms ` +
          `(every drop within ${String(rounded(worstLanding, 1))} px of the pointer's end)\n`,
      );
      expect(samples).toHaveLength(PAIRS);
    },
    TEST_TIMEOUT_MS,
  );
});
