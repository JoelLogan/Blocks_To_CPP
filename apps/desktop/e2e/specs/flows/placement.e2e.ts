/**
 * Placing blocks with real pointer drags in the real webview (docs/spec/03-block-language.md §3.3
 * "Placing blocks", 04 §4.1):
 *
 * - **Off its connection into a C-block.** A *print* is dropped with its top connection 32 px
 *   right of and 16 px above the empty mouth of a *repeat* (about 36 px away, up and to the right,
 *   away from the places below the mouth), at the canvas's start zoom (90 %) and zoomed out three
 *   steps with *Zoom out* (52 %): it lands in the mouth. Blockly's own 28 canvas units are 25 px at
 *   90 % and 15 px at 52 %, so Blockly alone left it loose; the editor snaps from about 44 px at
 *   every zoom. *Show all blocks* then brings the program back into view at exactly that zoom.
 * - **On the toolbox while a place is previewed.** A *print* at the end of `main` is dragged until
 *   a place between `main`'s first two statements is previewed (a grey insertion marker), then
 *   dropped with the pointer on the toolbox's flyout: once after moving over it (no place is
 *   previewed there any more and the block is faded), once with the jump onto the flyout and the
 *   release in one go, without a pause, as a fast flick ends. Each time only the dragged block is
 *   deleted: nothing was connected first, so the statements below the previewed place stay. (A
 *   WebDriver action always moves the pointer before it releases it; a release with no move at
 *   all before it, as a touch can end, is covered by `src/editor/sync/drag.test.ts`.)
 * - **On the trash can**, from the same preview: the block is deleted.
 * - After each deleting drop, Ctrl+Z puts the block back at the end of `main`.
 *
 * Window sizes come from `B2C_E2E_WINDOW_SIZE` (800 × 560, 1024 × 700, 1028 × 749 and the default
 * 1280 × 800); the C++ panel is folded away to give the canvas room.
 */
import { Button, Origin, type WebDriver } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import type { E2ePoint } from '../../../src/e2e/contract';
import {
  type BlockNode,
  moduleNodes,
  NodeIds,
  num,
  onlyTopBlock,
  print,
  statementsOf,
} from '../../support/bdm';
import { bringIntoView, visibleCanvas, wellInside } from '../../support/canvas';
import { drag, dropPoint, pressAndMove, releaseButton } from '../../support/drag';
import { centerOn, currentDocument, foldCodePanel, grabPointOf } from '../../support/editor';
import { clickTestId, waitForFlyout } from '../../support/ui';
import { sleep, waitFor } from '../../support/wait';
import { type FlowApp, startFlow } from './lib/launch';
import { newEmptyProject, repeat } from './lib/project';
import { pressCtrl, UI_TIMEOUT_MS } from './lib/ui';

/** The zoom a new canvas starts at, and one zoom step (Blockly's `scaleSpeed`). */
const START_SCALE = 0.9;
const STEP = 1.2;

/** How near a place must come, on screen, for a dragged block to snap there (03 §3.3). */
const SNAP_PIXELS = 44;

/** Blockly's own snap radius, in canvas units, whatever the zoom. */
const BLOCKLY_SNAP_UNITS = 28;

/**
 * Where the dragged block's top connection is dropped from the empty mouth, in CSS pixels: up and
 * to the right, about 36 px away, where the mouth is the only place in reach.
 */
const OFF_TARGET: E2ePoint = { x: 32, y: -16 };

/** Pointer moves of the drags that the test reads the page during, and how long each takes. */
const STEPS = 12;
const STEP_MS = 25;

/** Runs in the webview: the canvas's zoom, from its block layer's `transform`, or `null`. */
const CANVAS_SCALE_SCRIPT = `
  const layer = document.querySelector(
    '[data-testid="workspace"] svg.blocklySvg > g.blocklyWorkspace > g.blocklyBlockCanvas',
  );
  const match = /scale\\(([0-9.e+-]+)\\)/.exec(layer === null ? '' : layer.getAttribute('transform') || '');
  return match === null ? null : Number(match[1]);
`;

/** What the page shows during a drag, from {@link DRAG_STATE_SCRIPT}. */
interface DragState {
  /** Blockly is dragging a block. */
  readonly dragging: boolean;
  /** A place is previewed: an insertion marker is drawn. */
  readonly previewed: boolean;
  /** The dragged block is faded: a drop now deletes it (`blocklyDraggingDelete`). */
  readonly faded: boolean;
}

/** Runs in the webview: the {@link DragState} on the page now. */
const DRAG_STATE_SCRIPT = `
  const workspace = document.querySelector('[data-testid="workspace"]');
  return {
    dragging: workspace !== null && workspace.querySelector('.blocklyDragging') !== null,
    previewed: workspace !== null && workspace.querySelector('.blocklyInsertionMarker') !== null,
    faded: document.querySelector('.blocklyDragging.blocklyDraggingDelete') !== null,
  };
`;

/**
 * Runs in the webview: the middle of the toolbox's flyout (its left and right edges) and the
 * middle of the trash can as drawn, or `null`. The trash can's group is as large as its whole
 * sprite sheet, so its middle is taken from the rectangles that clip its lid and its body, in the
 * can's own units, mapped to the window by its screen transform.
 */
const DROP_PLACES_SCRIPT = `
  const workspace = document.querySelector('[data-testid="workspace"]');
  const flyout = workspace?.querySelector('svg.blocklyToolboxFlyout');
  const trash = workspace?.querySelector('.blocklyTrash');
  if (!flyout || !trash) {
    return null;
  }
  let width = 0;
  let height = 0;
  for (const clip of trash.querySelectorAll('rect')) {
    width = Math.max(width, Number(clip.getAttribute('width')));
    height = Math.max(height, Number(clip.getAttribute('y')) + Number(clip.getAttribute('height')));
  }
  const m = trash.getScreenCTM();
  const [x, y] = [width / 2, height / 2];
  const f = flyout.getBoundingClientRect();
  return [f.left, f.right, m.a * x + m.c * y + m.e, m.b * x + m.d * y + m.f];
`;

/** Where the drops on a delete area go, in viewport pixels. */
interface DropPlaces {
  /** The toolbox flyout's left and right edges. */
  readonly flyout: { readonly left: number; readonly right: number };
  /** The middle of the trash can. */
  readonly trash: E2ePoint;
}

/** The canvas's zoom now. */
async function canvasScale(driver: WebDriver): Promise<number> {
  const scale = await driver.executeScript<number | null>(CANVAS_SCALE_SCRIPT);
  if (scale === null) {
    throw new Error('The canvas has no zoom in its transform');
  }
  return scale;
}

/** Waits until the canvas is drawn at `expected` (within rounding), and returns the zoom. */
function waitForScale(driver: WebDriver, expected: number): Promise<number> {
  return waitFor(
    async () => {
      const scale = await canvasScale(driver);
      return Math.abs(scale - expected) < 1e-6 ? scale : null;
    },
    {
      timeout: UI_TIMEOUT_MS,
      message: async () =>
        `the canvas at zoom ${String(expected)} (it is at ${String(await canvasScale(driver))})`,
    },
  );
}

/** A connection of a block on screen (`previous`, `next` or an input's name). */
function connectionOf(app: FlowApp, blockId: string, connection: string): Promise<E2ePoint> {
  return waitFor(() => app.hook.connectionPoint(blockId, connection), {
    timeout: UI_TIMEOUT_MS,
    message: `the ${connection} connection of block ${blockId}`,
  });
}

/** The flyout and the trash can on screen. */
async function dropPlaces(driver: WebDriver): Promise<DropPlaces> {
  const found: unknown = await driver.executeScript(DROP_PLACES_SCRIPT);
  if (!Array.isArray(found) || found.length !== 4 || !found.every((n) => typeof n === 'number')) {
    throw new Error(`The toolbox's flyout or the trash can is not on screen: ${String(found)}`);
  }
  const [left = 0, right = 0, x = 0, y = 0] = found.map(Number);
  return { flyout: { left, right }, trash: { x, y } };
}

/** What the page shows now, during a drag. */
function dragState(driver: WebDriver): Promise<DragState> {
  return driver.executeScript<DragState>(DRAG_STATE_SCRIPT);
}

/** A point with whole pixels (WebDriver's pointer coordinates are integers). */
function whole(point: E2ePoint): { x: number; y: number } {
  return { x: Math.round(point.x), y: Math.round(point.y) };
}

/** Moves the pointer, with the button still held, from `from` to `to` in {@link STEPS} moves. */
async function moveHeld(driver: WebDriver, from: E2ePoint, to: E2ePoint): Promise<void> {
  let actions = driver.actions({ async: true });
  for (let step = 1; step <= STEPS; step += 1) {
    actions = actions.move({
      ...whole({
        x: from.x + ((to.x - from.x) * step) / STEPS,
        y: from.y + ((to.y - from.y) * step) / STEPS,
      }),
      origin: Origin.VIEWPORT,
      duration: STEP_MS,
    });
  }
  await actions.pause(120).perform();
}

/** Jumps the held pointer to `to` and releases it there in one go, without a pause (a flick). */
async function jumpAndRelease(driver: WebDriver, to: E2ePoint): Promise<void> {
  await driver
    .actions({ async: true })
    .move({ ...whole(to), origin: Origin.VIEWPORT, duration: 0 })
    .release(Button.LEFT)
    .perform();
}

/** The IDs of `main`'s statements, from the canvas. */
async function mainIds(app: FlowApp): Promise<string[]> {
  const main = onlyTopBlock(await currentDocument(app), 'program.main');
  return statementsOf(main, 'BODY').map((node) => node.id);
}

/** Waits until `main`'s statements are `expected`. */
function waitForMain(app: FlowApp, expected: readonly string[], message: string): Promise<true> {
  return waitFor(async () => ((await mainIds(app)).join() === expected.join() ? true : null), {
    timeout: UI_TIMEOUT_MS,
    message: async () => `${message} (main holds ${(await mainIds(app)).join(', ')})`,
  });
}

/** The IDs of the statements in the mouth of block `id`, from the canvas. */
async function mouthIds(app: FlowApp, id: string): Promise<string[] | null> {
  const loop = moduleNodes(await currentDocument(app)).find((node: BlockNode) => node.id === id);
  return loop === undefined ? null : statementsOf(loop, 'BODY').map((node) => node.id);
}

/**
 * Drags the block `blockId` (the statement right after the *repeat* `loopId` in `main`) with the
 * pointer so that its top connection lands {@link OFF_TARGET} from the repeat's empty mouth, and
 * returns the facts of the drag for the test's messages. The repeat is centred first, and the
 * mouth brought well inside the visible canvas.
 */
async function dropOffTarget(app: FlowApp, loopId: string, blockId: string): Promise<string> {
  await centerOn(app, loopId);
  await bringIntoView(app.driver, () => connectionOf(app, loopId, 'BODY'));
  const mouth = await connectionOf(app, loopId, 'BODY');
  const grab = await grabPointOf(app, blockId);
  const own = await connectionOf(app, blockId, 'previous');
  const release = dropPoint(grab, own, { x: mouth.x + OFF_TARGET.x, y: mouth.y + OFF_TARGET.y });
  const canvas = await visibleCanvas(app.driver);
  const facts = JSON.stringify({ mouth, grab, own, release, canvas });
  // Released over the canvas, not over the toolbox (which would delete the block).
  expect(wellInside(release, canvas), facts).toBe(true);
  await drag(app.driver, grab, release);
  return facts;
}

describe('Placing blocks', () => {
  it('snaps a block dropped off its connection into an empty C-block at 90 % and at 52 %', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const { driver } = app;
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);
    await waitForFlyout(driver);
    await waitForScale(driver, START_SCALE);
    const ids = new NodeIds('place');

    for (const [index, zoom] of [START_SCALE, START_SCALE / STEP ** 3].entries()) {
      if (index > 0) {
        for (let click = 0; click < 3; click += 1) {
          await clickTestId(driver, 'canvas-zoom-out');
        }
        await waitForScale(driver, zoom);
      }
      // Off the connection by more than Blockly's own radius, and within the editor's.
      const distance = Math.hypot(OFF_TARGET.x, OFF_TARGET.y);
      expect(distance).toBeGreaterThan(BLOCKLY_SNAP_UNITS * zoom + 2);
      expect(distance).toBeLessThan(SNAP_PIXELS - 2);

      const loop = repeat(ids, num(3), []);
      const block = print(ids, `placed at ${String(Math.round(zoom * 100))} %`);
      await app.hook.insertBlocks(mainId, 'BODY', [loop, block]);
      const facts = await dropOffTarget(app, loop.id, block.id);
      await waitFor(
        async () => ((await mouthIds(app, loop.id))?.join() === block.id ? true : null),
        {
          timeout: UI_TIMEOUT_MS,
          message: async () =>
            `the print in the repeat's mouth at zoom ${String(zoom)} (the mouth holds ${JSON.stringify(await mouthIds(app, loop.id))}; ${facts})`,
        },
      );
      // The drop changed nothing else in main: the repeat is its last statement.
      expect((await mainIds(app)).at(-1)).toBe(loop.id);
    }

    // Show all blocks brings the program into view and keeps the zoom exactly.
    const zoomed = await canvasScale(driver);
    await app.hook.scrollCanvasBy(-400, -300);
    await clickTestId(driver, 'canvas-show-all');
    await waitFor(() => app.hook.grabPoint(mainId), {
      timeout: UI_TIMEOUT_MS,
      interval: 200,
      message: 'the program block on screen after Show all blocks',
    });
    await sleep(200);
    expect(await canvasScale(driver)).toBe(zoomed);
  }, 180_000);

  it('deletes only the dragged block dropped on the toolbox or the trash can while a place is previewed, and Ctrl+Z brings it back', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const { driver } = app;
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);
    await waitForFlyout(driver);
    const ids = new NodeIds('drop');
    const first = print(ids, 'first');
    const second = print(ids, 'second');
    await app.hook.insertBlocks(mainId, 'BODY', [first, second]);

    type Release =
      'over the flyout' | 'at once after a jump onto the flyout' | 'over the trash can';
    const releases: readonly Release[] = [
      'over the flyout',
      'at once after a jump onto the flyout',
      'over the trash can',
    ];
    for (const [index, how] of releases.entries()) {
      // A print at the end of main, dragged until the place between first and second is
      // previewed.
      const dragged = print(ids, `dropped ${String(index)}`);
      await app.hook.insertBlocks(mainId, 'BODY', [dragged]);
      const before = await mainIds(app);
      expect(before.at(-1)).toBe(dragged.id);
      const kept = before.slice(0, -1);
      await centerOn(app, second.id);
      await bringIntoView(app.driver, () => connectionOf(app, first.id, 'next'));
      const grab = await grabPointOf(app, dragged.id);
      const own = await connectionOf(app, dragged.id, 'previous');
      const target = await connectionOf(app, first.id, 'next');
      const near = dropPoint(grab, own, { x: target.x + 4, y: target.y + 4 });
      await pressAndMove(driver, grab, near);
      const previewing = await dragState(driver);
      const places = await dropPlaces(driver);
      const over =
        how === 'over the trash can'
          ? places.trash
          : { x: (places.flyout.left + places.flyout.right) / 2, y: near.y };
      const facts = `${how}: ${JSON.stringify({ grab, own, target, near, over, places, previewing })}`;
      if (how === 'at once after a jump onto the flyout') {
        await jumpAndRelease(driver, over);
      } else {
        await moveHeld(driver, near, over);
        const deleting = await dragState(driver);
        await releaseButton(driver);
        expect(deleting.previewed, `${facts}, then ${JSON.stringify(deleting)}`).toBe(false);
        expect(deleting.faded, `${facts}, then ${JSON.stringify(deleting)}`).toBe(true);
      }
      expect(previewing, facts).toEqual({ dragging: true, previewed: true, faded: false });

      // Only the dragged print is gone: nothing was connected before the delete.
      await waitForMain(app, kept, `main without the print dropped ${how} (${facts})`);
      await pressCtrl(driver, 'z');
      await waitForMain(app, before, `Ctrl+Z to put back the print dropped ${how}`);
    }
  }, 180_000);
});
