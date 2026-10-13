/**
 * The canvas controls and zoom keys in the real webview (docs/spec/04-user-interface.md §4.1,
 * §4.7; decision D10 of the M2 polish plan):
 *
 * - the controls *Zoom out*, *Zoom in*, *Reset zoom* and *Show all blocks* sit over the canvas,
 *   right of the toolbox's flyout, and each is what the pointer hits at its middle;
 * - *Zoom in* twice zooms by two steps; with the canvas scrolled so the program is off the middle
 *   of the view, *Show all blocks* brings `main` back into the middle and leaves the zoom exactly
 *   as it was;
 *   *Reset zoom* goes to 100 % (not the 90 % a new canvas starts at);
 * - with a block of the canvas focused, `Ctrl+=`, `Ctrl+-` and `Ctrl+0` zoom, and the editor's
 *   live region says the new zoom; Tab from the canvas reaches *Zoom out* next.
 *
 * The zoom is read from the canvas's own transform (`scale(…)` of its block layer), not from the
 * controls, so the test checks what is drawn.
 */
import type { WebDriver } from 'selenium-webdriver';
import { Key } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { foldCodePanel } from '../../support/editor';
import { clickTestId, testIdText, waitForFlyout } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { clickBlock } from './lib/editor';
import { type FlowApp, startFlow } from './lib/launch';
import { newEmptyProject } from './lib/project';
import { pressCtrl, UI_TIMEOUT_MS, waitForTestId } from './lib/ui';

/** The zoom a new canvas starts at, and one zoom step (Blockly's `scaleSpeed`). */
const START_SCALE = 0.9;
const STEP = 1.2;

/** Runs in the webview: the canvas's zoom, from its block layer's `transform`, or `null`. */
const CANVAS_SCALE_SCRIPT = `
  const layer = document.querySelector(
    '[data-testid="workspace"] svg.blocklySvg > g.blocklyWorkspace > g.blocklyBlockCanvas',
  );
  const match = /scale\\(([0-9.e+-]+)\\)/.exec(layer === null ? '' : layer.getAttribute('transform') || '');
  return match === null ? null : Number(match[1]);
`;

/** Where the controls are, from {@link LAYOUT_SCRIPT}. */
interface ControlsLayout {
  /** The controls' group: its edges. */
  readonly group: { left: number; top: number; right: number; bottom: number } | null;
  /** The right edge of the toolbox's flyout, and the visible canvas (right of it). */
  readonly flyoutRight: number;
  readonly view: { left: number; top: number; right: number; bottom: number };
  /** For each control, its test ID and whether it is what the pointer hits at its middle. */
  readonly hits: readonly { id: string; hit: boolean; what: string }[];
}

/** Runs in the webview: the {@link ControlsLayout}. */
const LAYOUT_SCRIPT = `
  const workspace = document.querySelector('[data-testid="workspace"]');
  const group = workspace.querySelector('[data-testid="canvas-controls"]');
  const svg = workspace.querySelector('svg.blocklySvg').getBoundingClientRect();
  const flyout = workspace.querySelector('svg.blocklyToolboxFlyout').getBoundingClientRect();
  const box = (element) => {
    const r = element.getBoundingClientRect();
    return { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
  };
  const describe = (element) =>
    element === null ? 'nothing' : element.tagName.toLowerCase() + '.' + String(element.className);
  return {
    group: group === null ? null : box(group),
    flyoutRight: flyout.right,
    view: { left: flyout.right, top: svg.top, right: svg.right, bottom: svg.bottom },
    hits: group === null ? [] : [...group.querySelectorAll('button')].map((button) => {
      const r = button.getBoundingClientRect();
      const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      return { id: button.dataset.testid, hit: hit !== null && button.contains(hit), what: describe(hit) };
    }),
  };
`;

/** Runs in the webview: the box of the element `arguments[0]` on screen. */
const BOX_SCRIPT = `
  const r = arguments[0].getBoundingClientRect();
  return { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
`;

/**
 * How far the middle of block `blockId` is from the middle of the visible canvas (right of the
 * flyout), in CSS pixels on each axis.
 */
async function offCentre(app: FlowApp, blockId: string): Promise<{ x: number; y: number }> {
  const block = await app.hook.blockElement(blockId);
  if (block === null) {
    throw new Error(`Block ${blockId} has no element`);
  }
  const box = await app.driver.executeScript<ControlsLayout['view']>(BOX_SCRIPT, block);
  const { view } = await app.driver.executeScript<ControlsLayout>(LAYOUT_SCRIPT);
  return {
    x: Math.abs((box.left + box.right) / 2 - (view.left + view.right) / 2),
    y: Math.abs((box.top + box.bottom) / 2 - (view.top + view.bottom) / 2),
  };
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

/** The text of the editor's polite live region (the keyboard announcer), trimmed. */
async function announced(app: FlowApp): Promise<string> {
  const text = await app.driver.executeScript<string>(
    `const status = document.querySelector('[data-testid="keyboard-announcer"] [role="status"]');
     return status === null ? '' : status.textContent;`,
  );
  return text.replace(/\u00a0/g, ' ').trim();
}

/** Opens a new Empty project with the C++ panel folded; returns the program block's ID. */
async function openEmptyProject(app: FlowApp): Promise<string> {
  const mainId = await newEmptyProject(app);
  await waitForTestId(app.driver, 'workspace', true);
  await foldCodePanel(app);
  await waitForFlyout(app.driver);
  await waitForTestId(app.driver, 'canvas-controls', true);
  return mainId;
}

describe('Canvas controls', () => {
  it('zoom in twice, then show all blocks at the same zoom, then reset to 100 %', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const { driver } = app;
    const mainId = await openEmptyProject(app);

    // Over the canvas, right of the flyout, and each control is what the pointer hits.
    const layout = await driver.executeScript<ControlsLayout>(LAYOUT_SCRIPT);
    const facts = JSON.stringify(layout);
    expect(layout.group, facts).not.toBeNull();
    expect(layout.group?.left ?? 0, facts).toBeGreaterThanOrEqual(layout.flyoutRight);
    expect(layout.group?.right ?? Infinity, facts).toBeLessThanOrEqual(layout.view.right);
    expect(layout.hits.map((hit) => hit.id)).toEqual([
      'canvas-zoom-out',
      'canvas-zoom-in',
      'canvas-zoom-reset',
      'canvas-show-all',
    ]);
    expect(
      layout.hits.filter((hit) => !hit.hit),
      facts,
    ).toEqual([]);

    // A new canvas starts at 90 %; two steps in.
    await waitForScale(driver, START_SCALE);
    expect(await testIdText(driver, 'canvas-zoom-reset')).toBe('90%');
    await clickTestId(driver, 'canvas-zoom-in');
    await clickTestId(driver, 'canvas-zoom-in');
    const zoomed = await waitForScale(driver, START_SCALE * STEP * STEP);
    expect(await testIdText(driver, 'canvas-zoom-reset')).toBe('130%');

    // Scroll the program away from the middle of the view, then Show all blocks: it is back in
    // the middle, and the zoom has not changed at all.
    const before = await offCentre(app, mainId);
    await app.hook.scrollCanvasBy(-400, -300);
    if (Math.max(...Object.values(await offCentre(app, mainId))) < 100) {
      // Blockly keeps the view near the blocks: the other way, then.
      await app.hook.scrollCanvasBy(800, 600);
    }
    const away = await offCentre(app, mainId);
    expect(Math.max(away.x, away.y), JSON.stringify({ before, away })).toBeGreaterThanOrEqual(100);
    await clickTestId(driver, 'canvas-show-all');
    const shown = await waitFor(
      async () => {
        const off = await offCentre(app, mainId);
        // Centred on the view (Blockly's view includes the scroll bars' strips).
        return off.x < 24 && off.y < 24 ? off : null;
      },
      {
        timeout: UI_TIMEOUT_MS,
        message: async () =>
          `the program block in the middle of the view (it is off by ${JSON.stringify(await offCentre(app, mainId))})`,
      },
    );
    expect(shown.x).toBeLessThan(24);
    expect(await canvasScale(driver)).toBe(zoomed);

    // Reset zoom: 100 %, not the start zoom.
    await clickTestId(driver, 'canvas-zoom-reset');
    await waitForScale(driver, 1);
    expect(await testIdText(driver, 'canvas-zoom-reset')).toBe('100%');
  }, 180_000);

  it('zoom from the keyboard on the canvas, say the new zoom, and come next in the Tab order', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const { driver } = app;
    const mainId = await openEmptyProject(app);
    await clickBlock(app, mainId);

    await pressCtrl(driver, '=');
    await waitForScale(driver, START_SCALE * STEP);
    await waitFor(async () => (await announced(app)) === 'Zoom 108%', {
      timeout: UI_TIMEOUT_MS,
      message: async () => `"Zoom 108%" announced (it says "${await announced(app)}")`,
    });
    await pressCtrl(driver, '-');
    await waitForScale(driver, START_SCALE);
    await pressCtrl(driver, '-');
    await waitForScale(driver, START_SCALE / STEP);
    await pressCtrl(driver, '0');
    await waitForScale(driver, 1);
    await waitFor(async () => (await announced(app)) === 'Zoom 100%', {
      timeout: UI_TIMEOUT_MS,
      message: async () => `"Zoom 100%" announced (it says "${await announced(app)}")`,
    });

    // Tab leaves the canvas for the first control.
    await driver.actions({ async: true }).sendKeys(Key.TAB).perform();
    await waitFor(
      async () =>
        (await driver.executeScript<string | null>(
          'return document.activeElement && document.activeElement.dataset.testid || null;',
        )) === 'canvas-zoom-out',
      { timeout: UI_TIMEOUT_MS, message: 'the focus on Zoom out after Tab from the canvas' },
    );
  }, 180_000);
});
