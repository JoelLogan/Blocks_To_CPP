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
 * Before every pair, outside what is timed, the canvas is centred on the handle again and the page
 * left to settle: a drop does not always leave the handle (or the canvas's view) exactly where the
 * pointer moves suggest, and over many drags it could drift off screen. The drag back starts where
 * the drag away ended, so it never crosses the toolbox (a drop there would delete the handle).
 * After every drop the handle must have moved with the pointer (bench/handle.ts), so a drag that
 * missed it fails the run instead of being measured.
 */
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { Button, Origin, type WebDriver } from 'selenium-webdriver';
import { afterAll, describe, expect, it } from 'vitest';

import { E2E_HOOK_NAME, type E2ePoint } from '../../src/e2e/contract';
import { type App, launchApp } from '../support/app';
import { visibleCanvas } from '../support/canvas';
import { centerOn, foldCodePanel } from '../support/editor';
import { waitFor } from '../support/wait';
import { handleId, MAIN_ID, writeBenchProject } from './document';
import { awayDelta, checkLanding, describePlace, isBlockPlace } from './handle';
import {
  type BlockPlace,
  FRAMES_KEY,
  FRAMES_START_SCRIPT,
  FRAMES_STOP_SCRIPT,
  isTimestamps,
  PLACE_SCRIPT,
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
 * How long the whole benchmark may take: on Windows a drag and what its drop starts take about
 * 20 s at 5,000 blocks, and each of the 22 drags is preceded by centring the handle and waiting for
 * the page to settle.
 */
const TEST_TIMEOUT_MS = 1_200_000;

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

/** One timed drag. */
interface TimedDrag {
  /** How far the pointer moved, in CSS pixels. */
  readonly delta: E2ePoint;
  /** The frame times recorded while the pointer dragged. */
  readonly times: number[];
  /** How far from the pointer's end the drop left the handle, in CSS pixels. */
  readonly landing: number;
}

/**
 * Drags the handle and returns the frame times recorded during the drag (only the drag itself is
 * timed), once the drop has settled and the handle is known to have moved with the pointer. With
 * `move` `'away'` the canvas is first centred on the handle and the drag ends up and to the right
 * of the centre ({@link awayDelta}); otherwise the handle is dragged by `move`.
 */
async function timedDrag(app: App, id: string, move: 'away' | E2ePoint): Promise<TimedDrag> {
  if (move === 'away') {
    await centerOn(app, id);
    await waitUntilQuiet(app, 'centring the canvas on the drag handle');
  }
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
  const delta = move === 'away' ? awayDelta(grab, await visibleCanvas(app.driver)) : move;
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
  const landing = checkLanding(id, delta, before, await placeOf(app, id));
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
      const app = await launchApp(context, { dialogs: { open: [file] } });
      await openProjectFile(app, {
        blockId: MAIN_ID,
        code: 'void dragMe()',
        timeout: OPEN_TIMEOUT_MS,
      });
      await foldCodePanel(app);

      const samples: number[] = [];
      let worstLanding = 0;
      for (let pair = 0; pair <= PAIRS; pair += 1) {
        const away = await timedDrag(app, handle, 'away');
        const back = await timedDrag(app, handle, { x: -away.delta.x, y: -away.delta.y });
        worstLanding = Math.max(worstLanding, away.landing, back.landing);
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
