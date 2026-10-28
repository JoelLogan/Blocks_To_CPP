/**
 * Working with the block editor in the tests: reading the open project, waiting until Blockly has
 * delivered a change (before an undo), dragging blocks from the toolbox onto connections, and
 * waiting for the live analysis (Problems, the C++ panel).
 */
import { By, type WebDriver, type WebElement } from 'selenium-webdriver';

import type { E2ePoint } from '../../src/e2e/contract';
import type { App } from './app';
import { parseDocument, type ProjectDocument } from './bdm';
import { drag, dropPoint } from './drag';
import { bringIntoView } from './canvas';
import {
  byTestId,
  clickTestId,
  errorCount,
  middleOf,
  openCategory,
  testIdText,
  waitForFlyout,
} from './ui';
import { sleep, waitFor } from './wait';

/** The open project as the canvas has it now. */
export async function currentDocument(app: App): Promise<ProjectDocument> {
  return parseDocument(await app.hook.documentText());
}

/**
 * Runs in the webview (`executeAsyncScript`): calls back after the next animation frame and then
 * two zero timeouts. Blockly delivers a change's events in a zero timeout that it queues in the
 * first animation frame after the change (`requestAnimationFrame(() => setTimeout(fireNow, 0))`,
 * `core/events/utils.ts` of Blockly 12.5.1). Blockly asked for that frame callback while the change
 * was made (or earlier, for events still queued), before this script ran, so it runs first in the
 * same frame or in an earlier one, and queues its timeout first: the first zero timeout here runs
 * after the delivery, the second after anything the delivery queued with a zero timeout in turn.
 * There is no fallback timer: without animation frames the script never calls back, and WebDriver's
 * script timeout fails the test. A plain script, so nothing the test runner adds to compiled
 * functions can end up in the page.
 */
export const DELIVERY_SCRIPT = `
  const done = arguments[arguments.length - 1];
  requestAnimationFrame(() => {
    setTimeout(() => {
      setTimeout(() => done(true), 0);
    }, 0);
  });
`;

/**
 * Waits until Blockly has delivered the events of a change already seen on the canvas (see
 * {@link DELIVERY_SCRIPT}), as a person sees a drop drawn before pressing a key. The canvas, and so
 * {@link currentDocument}, shows a change at once, but Blockly puts it on its undo stack only when
 * it delivers its events: Ctrl+Z pressed before that undoes what came before the change, and the
 * late delivery then puts the change on the undo stack and empties the redo stack.
 *
 * @throws WebDriver's script timeout error when no animation frame comes within the session's
 *   script timeout (`SCRIPT_TIMEOUT_MS`).
 */
export async function waitForDelivery(driver: WebDriver): Promise<void> {
  await driver.executeAsyncScript(DELIVERY_SCRIPT);
}

/**
 * The Problems panel's summary (`No problems`, `1 error`, …). Read it with the Problems tab shown
 * ({@link showProblems}): a hidden Problems panel keeps what it showed last and catches up once
 * when it is shown again.
 */
export function problemsSummary(driver: WebDriver): Promise<string> {
  return testIdText(driver, 'problems-summary');
}

/**
 * Shows the Problems tab of the bottom dock (opening the dock if it is collapsed), as a person
 * looking at Problems does. A run shows the Console tab again by itself.
 */
export async function showProblems(driver: WebDriver): Promise<void> {
  await clickTestId(driver, 'dock-tab-problems');
  await waitFor(
    async () =>
      (await driver.findElements(By.css('[data-testid="dock-tab-problems"][aria-selected="true"]')))
        .length > 0 &&
      (await driver.findElements(By.css('section.bottom-dock[data-collapsed="false"]'))).length > 0,
    { timeout: 10_000, message: 'the Problems tab' },
  );
}

/**
 * Shows the Problems tab and waits until it names `count` errors (or, with `atLeast`, that many or
 * more).
 */
export async function waitForErrors(
  app: App,
  count: number,
  options: { readonly atLeast?: boolean; readonly timeout?: number } = {},
): Promise<string> {
  await showProblems(app.driver);
  return waitFor(
    async () => {
      const summary = await problemsSummary(app.driver);
      const errors = errorCount(summary);
      const matches = options.atLeast === true ? errors >= count : errors === count;
      return summary !== '' && matches ? summary : null;
    },
    {
      timeout: options.timeout ?? 15_000,
      message: async () =>
        `${options.atLeast === true ? 'at least ' : ''}${String(count)} errors in Problems (it says "${await problemsSummary(app.driver)}")`,
    },
  );
}

/** Whether Run is held back (`aria-disabled`), as the run gate decides. */
export async function runHeldBack(driver: WebDriver): Promise<boolean> {
  const run = await driver.findElement(byTestId('toolbar-run'));
  return (await run.getAttribute('aria-disabled')) === 'true';
}

/**
 * Gives the canvas room: the toolbox's flyout stays open over the canvas's left part, so the C++
 * panel is folded away and only a narrow strip would be left otherwise.
 */
export async function foldCodePanel(app: App): Promise<void> {
  await clickTestId(app.driver, 'right-dock-toggle');
  await waitFor(
    async () => {
      const dock = await app.driver.findElements(By.css('aside.right-dock[data-collapsed="true"]'));
      return dock.length > 0;
    },
    { timeout: 5_000, message: 'the C++ panel to fold away' },
  );
  await sleep(300);
}

/** Shows the C++ panel again after {@link foldCodePanel}. */
export async function showCodePanel(app: App): Promise<void> {
  await clickTestId(app.driver, 'right-dock-toggle');
  await waitFor(
    async () => {
      const dock = await app.driver.findElements(
        By.css('aside.right-dock[data-collapsed="false"]'),
      );
      return dock.length > 0;
    },
    { timeout: 5_000, message: 'the C++ panel to show' },
  );
  await sleep(300);
}

/** Scrolls the canvas so block `blockId` is in the middle of what is visible, and selects it. */
export async function centerOn(app: App, blockId: string): Promise<void> {
  await app.hook.selectBlock(blockId);
  // Blockly renders the scrolled canvas in the next frame.
  await sleep(250);
}

/**
 * The SVG group of a field (of the block, or of the block in `input`), with the block centred and
 * the canvas panned until the field is well inside the visible canvas.
 */
export async function fieldOf(
  app: App,
  blockId: string,
  field: string,
  input: string | null = null,
): Promise<WebElement> {
  const find = () =>
    waitFor(() => app.hook.fieldElement(blockId, field, input), {
      timeout: 5_000,
      message: `field ${field}${input === null ? '' : ` in ${input}`} of block ${blockId}`,
    });
  await centerOn(app, blockId);
  await bringIntoView(app.driver, async () => middleOf(app.driver, await find()));
  return find();
}

/** Waits for a block's grab point: the block must be on screen and not covered. */
export function grabPointOf(app: App, blockId: string): Promise<E2ePoint> {
  return waitFor(() => app.hook.grabPoint(blockId), {
    timeout: 10_000,
    interval: 200,
    message: `block ${blockId} to be on screen`,
  });
}

/**
 * Runs in the webview: the middle of the handle of the toolbox flyout's vertical scrollbar, how
 * far the handle can still move down, the handle's height and its track's height, in viewport
 * pixels, or `null` when no flyout shows one (the trash can's flyout, hidden, has a scrollbar of no
 * size).
 */
const FLYOUT_SCROLLBAR = `
  for (const bar of document.querySelectorAll('.blocklyFlyoutScrollbar.blocklyScrollbarVertical')) {
    const handle = bar.querySelector('.blocklyScrollbarHandle');
    const track = bar.querySelector('.blocklyScrollbarBackground');
    if (handle === null || track === null) {
      continue;
    }
    const h = handle.getBoundingClientRect();
    const t = track.getBoundingClientRect();
    if (h.width > 0 && h.height > 0) {
      return [
        h.left + h.width / 2,
        h.top + h.height / 2,
        Math.max(0, t.bottom - h.bottom),
        h.height,
        t.height,
      ];
    }
  }
  return null;
`;

/** The toolbox flyout's vertical scrollbar, as {@link FLYOUT_SCROLLBAR} measures it. */
export interface FlyoutScrollbar {
  /** The middle of the handle, in viewport pixels. */
  readonly x: number;
  readonly y: number;
  /** How far the handle can still move down. */
  readonly room: number;
  /** The handle's height. */
  readonly handle: number;
  /** The track's height. */
  readonly track: number;
}

/**
 * The most one step of {@link flyoutGrabPoint} moves the flyout's scrollbar handle, in pixels. A
 * step moves it by at most half its height as well: the handle is as much shorter than its track
 * as the flyout's visible part is than its blocks, so half its height scrolls the blocks by half a
 * view. A fixed step skips blocks once the toolbox is long: with W2's Types and Files categories
 * the handle is 9 pixels high on a 222-pixel track at 800 × 560, and 40 pixels of it scroll the
 * blocks by about four views.
 */
const FLYOUT_SCROLL_STEP = 40;

/** How far one step of {@link flyoutGrabPoint} moves the handle, before the room left limits it. */
function flyoutStep(bar: FlyoutScrollbar): number {
  return Math.max(2, Math.min(FLYOUT_SCROLL_STEP, Math.floor(bar.handle / 2)));
}

/**
 * The most steps {@link flyoutGrabPoint} takes for scrollbar `bar`: enough to move the handle down
 * the whole track a step at a time, and one more. Only a safety bound: the steps end before it when
 * the handle reaches the bottom or stops moving.
 */
export function flyoutScrollCeiling(bar: FlyoutScrollbar): number {
  return Math.ceil(bar.track / flyoutStep(bar)) + 1;
}

/**
 * How far to drag the flyout's scrollbar handle down next, in pixels, or `null` to stop: when the
 * handle has no room left (`room` < 1), when the last step did not move it (`previous` is the
 * scrollbar before that step, `null` before the first), or after {@link flyoutScrollCeiling}
 * steps (`steps` taken so far). A step is half the handle's height, at least 2 and at most
 * {@link FLYOUT_SCROLL_STEP} pixels, and never more than the room left. A pure function, so the
 * support tests can drive it through a long flyout.
 */
export function nextFlyoutScroll(
  bar: FlyoutScrollbar,
  previous: FlyoutScrollbar | null,
  steps: number,
): number | null {
  if (bar.room < 1) {
    return null;
  }
  if (previous !== null && Math.abs(bar.y - previous.y) < 0.5) {
    return null;
  }
  if (steps >= flyoutScrollCeiling(bar)) {
    return null;
  }
  return Math.min(flyoutStep(bar), bar.room);
}

/**
 * Scrolls the toolbox's flyout down a step at a time ({@link nextFlyoutScroll}) until `find`
 * gives something, and returns it; `null` once the steps stop first. `scrollbar` reads the
 * flyout's scrollbar (`null` when none shows) and `move` drags its handle from a point down by
 * some pixels and waits until the flyout has settled.
 */
export async function scrollFlyoutUntil<T>(
  find: () => Promise<T | null>,
  scrollbar: () => Promise<FlyoutScrollbar | null>,
  move: (from: E2ePoint, dy: number) => Promise<void>,
): Promise<T | null> {
  let previous: FlyoutScrollbar | null = null;
  for (let steps = 0; ; steps += 1) {
    const found = await find();
    if (found !== null) {
      return found;
    }
    const bar = await scrollbar();
    if (bar === null) {
      return null;
    }
    const dy = nextFlyoutScroll(bar, previous, steps);
    if (dy === null) {
      return null;
    }
    await move({ x: bar.x, y: bar.y }, dy);
    previous = bar;
  }
}

/** The toolbox flyout's vertical scrollbar on the page ({@link FLYOUT_SCROLLBAR}), or `null`. */
async function flyoutScrollbar(driver: WebDriver): Promise<FlyoutScrollbar | null> {
  const bar: unknown = await driver.executeScript(FLYOUT_SCROLLBAR);
  if (!Array.isArray(bar) || bar.length !== 5 || !bar.every((n) => typeof n === 'number')) {
    return null;
  }
  const [x = 0, y = 0, room = 0, handle = 0, track = 0] = bar.map(Number);
  return { x, y, room, handle, track };
}

/**
 * The grab point of block `blockId` of the toolbox's flyout (its category open), once it is on
 * screen. In a short window (800 × 560) a category's later blocks, such as the getters below
 * Variables' generic blocks, lie below the flyout's visible part, and a long toolbox can put a
 * block many views down: the flyout is scrolled down with its scrollbar, half a view at a time,
 * until the block shows, the handle reaches the bottom of its track or stops moving
 * ({@link scrollFlyoutUntil}).
 */
export async function flyoutGrabPoint(app: App, blockId: string): Promise<E2ePoint> {
  const grab = await scrollFlyoutUntil(
    () => app.hook.grabPoint(blockId),
    () => flyoutScrollbar(app.driver),
    async (from, dy) => {
      await drag(app.driver, from, { x: from.x, y: from.y + dy });
      await waitForFlyout(app.driver);
    },
  );
  return grab ?? grabPointOf(app, blockId);
}

/** A connection's point on screen. */
function connectionOf(app: App, blockId: string, connection: string) {
  return waitFor(() => app.hook.connectionPoint(blockId, connection), {
    timeout: 10_000,
    message: `the ${connection} connection of block ${blockId}`,
  });
}

/** A block of the toolbox: its category, its type and (to tell entries apart) field values. */
export interface ToolboxEntry {
  readonly category: string;
  readonly type: string;
  readonly fields?: Readonly<Record<string, string>>;
}

/**
 * Drags a block from the toolbox so that its `previous` connection lands on connection `target` of
 * block `targetId`, where Blockly snaps it in.
 *
 * The target is centred (and selected) first, then the category is opened and the flyout block
 * looked up.
 */
export async function dragFromToolbox(
  app: App,
  entry: ToolboxEntry,
  targetId: string,
  target: string,
): Promise<void> {
  await centerOn(app, targetId);
  await bringIntoView(app.driver, () => connectionOf(app, targetId, target));
  await openCategory(app.driver, entry.category);
  const blockId = await waitFor(() => app.hook.flyoutBlockId(entry.type, entry.fields ?? {}), {
    timeout: 10_000,
    message: `${entry.type} in the toolbox's ${entry.category} category`,
  });
  const grab = await flyoutGrabPoint(app, blockId);
  const own = await connectionOf(app, blockId, 'previous');
  const targetPoint = await connectionOf(app, targetId, target);
  await drag(app.driver, grab, dropPoint(grab, own, targetPoint));
  // Blockly finishes the drop (snapping, bumping) in the next frames.
  await sleep(300);
}

/** How far inside a block's top left corner {@link blockCorner} lies, in CSS pixels. */
const CORNER_INSET = 12;

/**
 * A point just inside block `blockId`'s top left corner, where its grab point is searched from
 * first, whether or not the toolbox's flyout covers it there.
 */
export async function blockCorner(app: App, blockId: string): Promise<E2ePoint> {
  const element = await waitFor(() => app.hook.blockElement(blockId), {
    timeout: 10_000,
    message: `block ${blockId} on the canvas`,
  });
  const box: unknown = await app.driver.executeScript(
    'const r = arguments[0].getBoundingClientRect(); return [r.left, r.top];',
    element,
  );
  if (!Array.isArray(box) || box.length !== 2 || !box.every((n) => typeof n === 'number')) {
    throw new Error(`Block ${blockId} has no position on screen`);
  }
  const [left, top] = box as [number, number];
  return { x: Math.round(left + CORNER_INSET), y: Math.round(top + CORNER_INSET) };
}

/**
 * Drags block `blockId` by `dx`, `dy` pixels on the canvas. The block is centred, and then the
 * canvas is panned until its top left corner is well inside the visible canvas: Blockly centres a
 * block in the canvas's view, which the toolbox's flyout overlaps, so a wide block's start (where
 * it is grabbed) can lie under the flyout.
 */
export async function dragBy(app: App, blockId: string, dx: number, dy: number): Promise<void> {
  await centerOn(app, blockId);
  await bringIntoView(app.driver, () => blockCorner(app, blockId));
  const grab = await grabPointOf(app, blockId);
  await drag(app.driver, grab, { x: grab.x + dx, y: grab.y + dy });
  await sleep(300);
}

/** The function `int main() { … }` of generated C++ (to the end of the file), trimmed. */
export function mainFunction(cpp: string): string {
  const start = cpp.indexOf('int main() {');
  if (start === -1) {
    throw new Error('The C++ has no main function');
  }
  return cpp.slice(start).trim();
}
