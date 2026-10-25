/**
 * Webview benchmarks of typed slots (ADR-0018, docs/plans/m3-contracts/slots.md §8), both
 * reported, not gated (bench/typed-slots-metrics.ts):
 *
 * - `webview.drag-typed-1000.frame-p95-all`: the generated document (bench/typed-document.ts) has a
 *   column of 200 `create` blocks of alternating types (`int`, `string`) in a 1,000-block module,
 *   and a loose number block beside it. The number block is dragged slowly with real pointer input
 *   down the column, its output connection passing over each value input on screen in turn (so
 *   every pointer move asks the connection checker about typed slots, which refuses it for half of
 *   them), and back to where it lay, where it is let go and stays loose. While the pointer drags,
 *   the page records the timestamps of its animation frames (bench/page.ts
 *   `FRAMES_START_SCRIPT`); every frame time of the timed drags is a sample, the metric their p95.
 *   A first drag warms up.
 * - `webview.keyboard-list-1000.ms`: the number block is selected (which focuses it) and picked
 *   up with `M`, which lists the places the keyboard mover offers it: every free value
 *   input of the canvas, each typed (src/editor/keyboard/targets.ts). The listing's time is the
 *   app's User Timing span `b2c:keyboard-targets`; Escape leaves the block where it was. One
 *   listing per sample, after a warm-up one.
 *
 * Before every drag, outside what is timed, the canvas is panned until the number block is well
 * inside its visible part, and the column's value inputs that are on screen are found; after every
 * drop the page is left to settle and the number block must still lie loose (a drop that connected
 * it fails the run instead of being measured). An app built before the span existed (the A/B job's
 * base) has no `b2c:keyboard-targets`: with `B2C_BENCH_SKIP_NOTE` set the listing is skipped and
 * the note says why, otherwise the benchmark fails.
 */
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { Button, Key, Origin, type WebDriver } from 'selenium-webdriver';
import { afterAll, describe, expect, it } from 'vitest';

import { E2E_HOOK_NAME, type E2ePoint } from '../../src/e2e/contract';
import { type App, launchApp } from '../support/app';
import { bringIntoView, visibleCanvas, wellInside } from '../support/canvas';
import { centerOn, foldCodePanel } from '../support/editor';
import { waitFor } from '../support/wait';
import { requireHookMethods, skipNotePath } from './capabilities';
import {
  FRAMES_KEY,
  FRAMES_START_SCRIPT,
  FRAMES_STOP_SCRIPT,
  isTimestamps,
  QUIET_SCRIPT,
} from './page';
import { openProjectFile } from './project';
import { benchSamples, metricValue, writeMetric } from './results';
import { deltas, median, rounded } from './stats';
import {
  columnPath,
  createId,
  NUMBER_ID,
  TYPED_BLOCKS,
  TYPED_CREATES,
  TYPED_MAIN_ID,
  writeTypedProject,
} from './typed-document';
import {
  keyboardListMetric,
  typedDragMetric,
  WARM_UP_DRAGS,
  WARM_UP_LISTINGS,
} from './typed-slots-metrics';

/** Timed drags and timed listings (results.ts `benchSamples`, 10 by default). */
const SAMPLES = benchSamples();

/** Pointer moves per drag (down the column and back), and how long each takes. */
const STEPS = 120;
const STEP_MS = 16;

/** The fewest value inputs of the column a drag must pass over. */
const MIN_TARGETS = 4;

/** The fewest frame times a drag must give. */
const MIN_FRAMES = 20;

/** How long opening the document, rendering it and the first preview may take. */
const OPEN_TIMEOUT_MS = 300_000;

/** After a drop or a listing: this many frames in a row within the time mean the page is idle. */
const QUIET_FRAMES = 10;
const QUIET_FRAME_MS = 50;
const QUIET_TIMEOUT_MS = 60_000;

/** How long the editor may take to answer a key. */
const KEY_TIMEOUT_MS = 30_000;

/** The span of the keyboard listing (src/lib/timing.ts). */
const LISTING_SPAN = 'b2c:keyboard-targets';

/** How long the whole benchmark may take: a slow Windows runner opens the document in minutes. */
const TEST_TIMEOUT_MS = 1_500_000;

const folders: string[] = [];

afterAll(() => {
  for (const folder of folders) {
    rmSync(folder, { recursive: true, force: true });
  }
});

/** Notes progress on stderr, with the seconds since `since`. */
function progress(since: number, text: string): void {
  process.stderr.write(
    `typed-slots benchmark, ${String(Math.round((Date.now() - since) / 1000))} s: ${text}\n`,
  );
}

/** Waits until the page has finished what `after` started. */
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

/** A point the hook reports, once it is on screen. */
function pointOf(locate: () => Promise<E2ePoint | null>, what: string): Promise<E2ePoint> {
  return waitFor(locate, { timeout: 10_000, interval: 200, message: what });
}

/** Whether the number block still lies loose on the canvas (a top-level block of the module). */
async function numberIsLoose(app: App): Promise<boolean> {
  const doc = JSON.parse(await app.hook.documentText()) as {
    modules?: { workspace?: { blocks?: { id?: string }[] } }[];
  };
  return (doc.modules?.[0]?.workspace?.blocks ?? []).some((block) => block.id === NUMBER_ID);
}

/** Moves the pointer along `path` with the left button held from `from`, then lets it go. */
async function dragAlong(
  driver: WebDriver,
  from: E2ePoint,
  path: readonly E2ePoint[],
): Promise<void> {
  let actions = driver
    .actions({ async: true })
    .move({ x: Math.round(from.x), y: Math.round(from.y), origin: Origin.VIEWPORT })
    .press(Button.LEFT)
    .pause(100);
  for (const point of path) {
    actions = actions.move({
      x: Math.round(point.x),
      y: Math.round(point.y),
      origin: Origin.VIEWPORT,
      duration: STEP_MS,
    });
  }
  await actions.pause(100).release(Button.LEFT).perform();
}

/**
 * One timed drag of the number block down the column's value inputs on screen and back, and the
 * frame times recorded during it; the drop must leave the block loose.
 */
async function timedDrag(app: App): Promise<number[]> {
  await bringIntoView(app.driver, () =>
    pointOf(() => app.hook.grabPoint(NUMBER_ID), 'the number block on screen'),
  );
  await waitUntilQuiet(app, 'panning the canvas to the number block');
  const grab = await pointOf(() => app.hook.grabPoint(NUMBER_ID), 'the number block on screen');
  const own = await pointOf(
    () => app.hook.connectionPoint(NUMBER_ID, 'output'),
    'the number block’s output connection',
  );
  const canvas = await visibleCanvas(app.driver);
  const targets: E2ePoint[] = [];
  const offscreen: E2ePoint[] = [];
  for (let k = 0; k < TYPED_CREATES; k += 1) {
    const point = await app.hook.connectionPoint(createId(k), 'VALUE');
    const pointer =
      point === null ? null : { x: point.x + grab.x - own.x, y: point.y + grab.y - own.y };
    if (
      point !== null &&
      pointer !== null &&
      wellInside(point, canvas) &&
      wellInside(pointer, canvas)
    ) {
      targets.push(point);
    } else if (targets.length > 0) {
      break;
    } else if (point !== null && offscreen.length < 3) {
      offscreen.push(point);
    }
  }
  if (targets.length < MIN_TARGETS) {
    throw new Error(
      `Only ${String(targets.length)} value inputs of the column are on screen beside the number block (at least ${String(MIN_TARGETS)} are needed): the number at ${JSON.stringify(grab)}, the visible canvas ${JSON.stringify(canvas)}, the first inputs at ${JSON.stringify(offscreen)}`,
    );
  }
  const started: unknown = await app.driver.executeScript(FRAMES_START_SCRIPT, FRAMES_KEY);
  if (started !== true) {
    throw new Error('The frame recorder could not start (one is already running)');
  }
  let frames: unknown;
  try {
    await dragAlong(app.driver, grab, columnPath(grab, own, targets, STEPS));
  } finally {
    frames = await app.driver.executeScript(FRAMES_STOP_SCRIPT, FRAMES_KEY);
  }
  await waitUntilQuiet(app, 'a drop');
  if (!(await numberIsLoose(app))) {
    throw new Error('The number block was connected by a drop meant to leave it where it lay');
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

/** Runs in the webview: what the block editor's live region says now (trimmed). */
const ANNOUNCEMENT_SCRIPT = `
  const status = document.querySelector('[data-testid="keyboard-announcer"] [role="status"]');
  return status === null ? '' : (status.textContent || '').trim();
`;

/** Runs in the webview: the latest duration of span `arguments[1]`, `null`, or `undefined`. */
const SPAN_SCRIPT = `
  const [hookName, span] = arguments;
  const timings = window[hookName].timings();
  return span in timings ? timings[span] : 'absent';
`;

/** Waits until the editor's announcement passes `test`. */
function waitForAnnouncement(
  app: App,
  test: (text: string) => boolean,
  what: string,
): Promise<true> {
  return waitFor(
    async () => (test(await app.driver.executeScript<string>(ANNOUNCEMENT_SCRIPT)) ? true : null),
    { timeout: KEY_TIMEOUT_MS, interval: 20, message: what },
  );
}

/**
 * One keyboard listing of the number block's places: the time of the span, or `'absent'` when
 * the app has no such span.
 */
async function timedListing(app: App): Promise<number | 'absent'> {
  // Selected (and so focused) through the hook: a click on a number block opens its field.
  await centerOn(app, NUMBER_ID);
  await waitUntilQuiet(app, 'selecting the number block');
  await app.driver.actions({ async: true }).sendKeys('m').perform();
  await waitForAnnouncement(app, (text) => text.startsWith('Moving'), 'the keyboard move to start');
  const span: unknown = await app.driver.executeScript(SPAN_SCRIPT, E2E_HOOK_NAME, LISTING_SPAN);
  await app.driver.actions({ async: true }).sendKeys(Key.ESCAPE).perform();
  await waitForAnnouncement(app, (text) => text.endsWith('stays where it was.'), 'Escape');
  await waitUntilQuiet(app, 'a keyboard move');
  if (span === 'absent') {
    return 'absent';
  }
  if (typeof span !== 'number' || !Number.isFinite(span)) {
    throw new Error(`The span ${LISTING_SPAN} was not recorded: ${JSON.stringify(span)}`);
  }
  return span;
}

describe('webview benchmarks: typed slots at 1,000 blocks', () => {
  it(
    `times ${String(SAMPLES)} drags down a column of typed inputs and ${String(SAMPLES)} keyboard listings`,
    async (context) => {
      const folder = mkdtempSync(path.join(tmpdir(), 'b2c-bench-typed-'));
      folders.push(folder);
      const file = writeTypedProject(folder);
      const started = Date.now();
      const app = await launchApp(context, { dialogs: { open: [file] }, recordLongTasks: false });
      await requireHookMethods(context, app, 'typed-slots');
      await openProjectFile(app, {
        blockId: TYPED_MAIN_ID,
        code: 'int t_0 = 0;',
        timeout: OPEN_TIMEOUT_MS,
      });
      await foldCodePanel(app);
      await centerOn(app, NUMBER_ID);
      await waitUntilQuiet(app, 'centring the canvas on the number block');
      progress(started, `opened the ${String(TYPED_BLOCKS)}-block document`);

      const drags: number[][] = [];
      for (let drag = 0; drag < WARM_UP_DRAGS + SAMPLES; drag += 1) {
        drags.push(await timedDrag(app));
      }
      progress(started, `${String(drags.length)} drags done`);

      const listings: number[] = [];
      let absent = false;
      for (let listing = 0; listing < WARM_UP_LISTINGS + SAMPLES && !absent; listing += 1) {
        const time = await timedListing(app);
        if (time === 'absent') {
          absent = true;
        } else {
          listings.push(time);
        }
      }
      progress(started, `${String(listings.length)} keyboard listings done`);

      const frames = typedDragMetric(drags);
      writeMetric(frames);
      process.stdout.write(
        `Dragging past typed slots at ${String(TYPED_BLOCKS)} blocks: p95 frame time of all ` +
          `${String(frames.samples.length)} frames ${String(rounded(metricValue(frames), 1))} ms\n`,
      );
      if (!absent) {
        const listing = keyboardListMetric(listings);
        writeMetric(listing);
        process.stdout.write(
          `Listing the keyboard mover's places at ${String(TYPED_BLOCKS)} blocks: median ` +
            `${String(rounded(median(listing.samples), 1))} ms\n`,
        );
        expect(listing.samples).toHaveLength(SAMPLES);
        return;
      }
      const reason = `the app has no User Timing span ${LISTING_SPAN}, which the keyboard listing needs`;
      const note = skipNotePath();
      if (note === null) {
        throw new Error(
          `The ${reason}: build the app under test from the commit of the benchmarks (or set B2C_BENCH_SKIP_NOTE to skip it, as the A/B job does for its base)`,
        );
      }
      mkdirSync(path.dirname(note), { recursive: true });
      writeFileSync(note, `${reason}\n`);
      process.stdout.write(`Skipped the keyboard listing: ${reason}\n`);
    },
    TEST_TIMEOUT_MS,
  );
});
