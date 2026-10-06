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
 */
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { Button, Origin, type WebDriver } from 'selenium-webdriver';
import { afterAll, describe, expect, it } from 'vitest';

import type { E2ePoint } from '../../src/e2e/contract';
import { type App, launchApp } from '../support/app';
import { centerOn, foldCodePanel } from '../support/editor';
import { waitFor } from '../support/wait';
import { handleId, MAIN_ID, writeBenchProject } from './document';
import {
  FRAMES_KEY,
  FRAMES_START_SCRIPT,
  FRAMES_STOP_SCRIPT,
  isTimestamps,
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

/** How far each drag moves the handle (back again on the next one), in CSS pixels. */
const DISTANCE = { x: 240, y: 120 };

/** The fewest frame times a drag must give for its p95 to mean anything. */
const MIN_FRAMES = 20;

/** How long opening the document, rendering it and the first preview may take. */
const OPEN_TIMEOUT_MS = 300_000;

/** After a drop: this many frames in a row within {@link QUIET_FRAME_MS} mean the page is idle. */
const QUIET_FRAMES = 10;
const QUIET_FRAME_MS = 50;
const QUIET_TIMEOUT_MS = 60_000;

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

/** Waits until the page has finished what the last drop started. */
async function waitUntilQuiet(app: App): Promise<void> {
  const result: unknown = await app.driver.executeAsyncScript(
    QUIET_SCRIPT,
    QUIET_FRAMES,
    QUIET_FRAME_MS,
    QUIET_TIMEOUT_MS,
  );
  if (result !== 'quiet') {
    throw new Error(`The page was still busy ${String(QUIET_TIMEOUT_MS / 1000)} s after a drop`);
  }
}

/** Drags the handle by `delta` and returns the frame times recorded during the drag. */
async function timedDrag(app: App, id: string, delta: E2ePoint): Promise<number[]> {
  const grab = await waitFor(() => app.hook.grabPoint(id), {
    timeout: 10_000,
    interval: 200,
    message: `the drag handle ${id} to be on screen`,
  });
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
  if (!isTimestamps(frames)) {
    throw new Error('The frame recorder returned no timestamps');
  }
  const times = deltas(frames);
  if (times.length < MIN_FRAMES) {
    throw new Error(
      `Only ${String(times.length)} frame times were recorded during a drag (at least ${String(MIN_FRAMES)} are needed)`,
    );
  }
  return times;
}

describe('webview benchmark: dragging in a 5,000-block workspace', () => {
  it(`times the frames of ${String(PAIRS)} pairs of drags`, async (context) => {
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
    await centerOn(app, handle);
    await waitUntilQuiet(app);

    const samples: number[] = [];
    for (let pair = 0; pair <= PAIRS; pair += 1) {
      const away = await timedDrag(app, handle, DISTANCE);
      await waitUntilQuiet(app);
      const back = await timedDrag(app, handle, { x: -DISTANCE.x, y: -DISTANCE.y });
      await waitUntilQuiet(app);
      // The first pair warms up (Blockly's drag surface, the connection database): not kept.
      if (pair > 0) {
        samples.push(percentile([...away, ...back], 95));
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
      `Dragging at ${String(BLOCKS)} blocks: p95 frame time per pair, median ${String(rounded(median(samples), 1))} ms\n`,
    );
    expect(samples).toHaveLength(PAIRS);
  });
});
