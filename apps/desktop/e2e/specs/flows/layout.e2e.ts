/**
 * The editor's layout in the real webview (docs/spec/04-user-interface.md §4.1, §4.2), at window
 * sizes set through the app's window-size seam (`B2C_E2E_WINDOW_SIZE`), so that Linux and Windows
 * lay the editor out alike:
 *
 * - at 800 × 560, 1024 × 700, the Windows runner's 1028 × 749 and 1280 × 800: the C++ dock is
 *   shown at 30 % of the window's width (280–480 px), the toolbox's flyout draws its blocks at
 *   0.75 and is at most 320 px wide, and the canvas keeps at least 200 px beside it. After
 *   *Empty project*, the main block's left edge is at least 8 px right of the flyout's right edge:
 *   the start page opens projects while the editor is hidden, and the view Blockly then made put
 *   the block against the flyout's edge;
 * - the *Resize the toolbox* splitter is at the flyout's right edge, and it is what the pointer
 *   hits there. Dragging it widens the flyout and the blocks stay clear of it; End makes the
 *   flyout as wide as leaves the canvas 200 px.
 *
 * Where the screen is smaller than the size asked for (the Windows runner's is 1024 × 768), the
 * app keeps the window on the screen; the checks follow the window's real size.
 */
import { Key } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { RIGHT_DOCK, startSize } from '../../../src/app/layout/sizes';
import type { E2ePoint } from '../../../src/e2e/contract';
import { drag } from '../../support/drag';
import type { WindowSize } from '../../support/env';
import { pressKey, waitForFlyout } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { type FlowApp, startFlow } from './lib/launch';
import { newEmptyProject } from './lib/project';
import { UI_TIMEOUT_MS, waitForTestId } from './lib/ui';

/** The narrowest canvas the layout leaves beside the toolbox, in CSS pixels. */
const CANVAS_MIN = 200;

/** How far right of the flyout's edge the first view puts the blocks at least, in CSS pixels. */
const FIRST_VIEW_GAP = 8;

/** Where things are in the editor, from {@link LAYOUT_SCRIPT} (viewport pixels). */
interface EditorLayoutFacts {
  readonly viewport: readonly [number, number];
  readonly flyout: { readonly left: number; readonly right: number; readonly width: number };
  readonly flyoutScale: number | null;
  readonly canvasRight: number;
  readonly codePanel: { readonly shown: boolean; readonly width: number };
  /** The main block's left edge, or `null` when it is not on the canvas. */
  readonly blockLeft: number | null;
  /** The *Resize the toolbox* splitter: its middle, its value, and whether the pointer hits it. */
  readonly splitter: {
    readonly middle: E2ePoint;
    readonly left: number;
    readonly value: number;
    readonly max: number;
    readonly hit: boolean;
  } | null;
}

/**
 * Runs in the webview: the {@link EditorLayoutFacts}, with the main block's ID as `arguments[0]`.
 * A plain script, so nothing the test runner adds to compiled functions can end up in the page.
 */
const LAYOUT_SCRIPT = `
  const id = arguments[0];
  const workspace = document.querySelector('[data-testid="workspace"]');
  const canvas = workspace.querySelector('svg.blocklySvg');
  const flyout = workspace.querySelector('svg.blocklyToolboxFlyout');
  const blocks = flyout.querySelector('.blocklyBlockCanvas').getAttribute('transform') || '';
  const scale = /scale\\(([0-9.]+)\\)/.exec(blocks);
  const block = canvas.querySelector('[data-id="' + id + '"]');
  const dock = document.querySelector('aside.right-dock');
  const bar = workspace.querySelector('[role="separator"][aria-label="Resize the toolbox"]');
  const f = flyout.getBoundingClientRect();
  let splitter = null;
  if (bar !== null) {
    const r = bar.getBoundingClientRect();
    const middle = { x: r.left + r.width / 2, y: r.top + r.height / 2 };
    splitter = {
      middle,
      left: r.left,
      value: Number(bar.getAttribute('aria-valuenow')),
      max: Number(bar.getAttribute('aria-valuemax')),
      hit: document.elementFromPoint(middle.x, middle.y) === bar,
    };
  }
  return {
    viewport: [window.innerWidth, window.innerHeight],
    flyout: { left: f.left, right: f.right, width: f.width },
    flyoutScale: scale === null ? null : Number(scale[1]),
    canvasRight: canvas.getBoundingClientRect().right,
    codePanel: {
      shown: dock !== null && dock.getAttribute('data-collapsed') !== 'true',
      width: dock === null ? 0 : dock.getBoundingClientRect().width,
    },
    blockLeft: block === null ? null : block.getBoundingClientRect().left,
    splitter,
  };
`;

/** The editor's layout now. */
function layoutOf(app: FlowApp, mainId: string): Promise<EditorLayoutFacts> {
  return app.driver.executeScript<EditorLayoutFacts>(LAYOUT_SCRIPT, mainId);
}

/** Launches the app at `size` and creates an Empty project; returns the main block's ID. */
async function emptyProjectAt(
  context: Parameters<typeof startFlow>[0],
  size: WindowSize,
): Promise<{ app: FlowApp; mainId: string }> {
  const flow = startFlow(context);
  const app = await flow.launch({ windowSize: size });
  const mainId = await newEmptyProject(app);
  await waitForTestId(app.driver, 'workspace', true);
  await waitForFlyout(app.driver);
  // The first view settles once Blockly has drawn the canvas.
  await waitFor(async () => (await layoutOf(app, mainId)).blockLeft !== null, {
    timeout: UI_TIMEOUT_MS,
    message: 'the main block on the canvas',
  });
  return { app, mainId };
}

describe('The editor’s layout', () => {
  it.for([
    { width: 800, height: 560 },
    { width: 1024, height: 700 },
    { width: 1028, height: 749 },
    { width: 1280, height: 800 },
  ])(
    'leaves the canvas 200 px and shows the C++ dock at $width × $height',
    { timeout: 180_000 },
    async (size, context) => {
      const { app, mainId } = await emptyProjectAt(context, size);
      const layout = await layoutOf(app, mainId);
      const facts = JSON.stringify(layout);
      const [width, height] = layout.viewport;
      if (process.platform === 'linux') {
        // Linux CI's screen (1280 × 1024) holds every size asked for.
        expect([width, height], facts).toEqual([size.width, size.height]);
      } else {
        expect(width, facts).toBeLessThanOrEqual(size.width);
        expect(height, facts).toBeLessThanOrEqual(size.height);
      }

      expect(layout.codePanel.shown, facts).toBe(true);
      expect(Math.abs(layout.codePanel.width - startSize(RIGHT_DOCK, width)), facts).toBeLessThan(
        1,
      );
      expect(layout.flyoutScale, facts).toBe(0.75);
      expect(layout.flyout.width, facts).toBeLessThanOrEqual(320.5);
      expect(layout.canvasRight - layout.flyout.right, facts).toBeGreaterThanOrEqual(CANVAS_MIN);
      // The first view is clear of the flyout.
      expect((layout.blockLeft ?? -Infinity) - layout.flyout.right, facts).toBeGreaterThanOrEqual(
        FIRST_VIEW_GAP,
      );
      // The splitter is at the flyout's edge, on top.
      expect(layout.splitter?.hit, facts).toBe(true);
      expect(Math.abs((layout.splitter?.left ?? -1) - layout.flyout.right), facts).toBeLessThan(1);
    },
  );

  it('widens the toolbox’s flyout with its splitter, keeping the blocks clear of it', async (context) => {
    const { app, mainId } = await emptyProjectAt(context, { width: 1280, height: 800 });
    const before = await layoutOf(app, mainId);
    const splitter = before.splitter;
    if (splitter === null) {
      throw new Error(`No Resize the toolbox splitter: ${JSON.stringify(before)}`);
    }
    expect(splitter.value).toBe(Math.round(before.flyout.width));

    // By pointer: 80 px to the right.
    await drag(app.driver, splitter.middle, { x: splitter.middle.x + 80, y: splitter.middle.y });
    const dragged = await waitFor(
      async () => {
        const now = await layoutOf(app, mainId);
        return Math.abs(now.flyout.width - (before.flyout.width + 80)) < 1.5 ? now : null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'the flyout 80 px wider' },
    );
    const facts = JSON.stringify(dragged);
    expect(Math.abs((dragged.splitter?.left ?? -1) - dragged.flyout.right), facts).toBeLessThan(1);
    expect((dragged.blockLeft ?? -Infinity) - dragged.flyout.right, facts).toBeGreaterThanOrEqual(
      FIRST_VIEW_GAP,
    );

    // By keyboard: End makes it as wide as leaves the canvas 200 px. (A press on the splitter
    // does not take the focus, as on the docks' splitters: Tab does.)
    await app.driver.executeScript(
      `document.querySelector('[role="separator"][aria-label="Resize the toolbox"]').focus();`,
    );
    await pressKey(app.driver, Key.END);
    const widest = await waitFor(
      async () => {
        const now = await layoutOf(app, mainId);
        return now.splitter !== null && now.splitter.value === now.splitter.max ? now : null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'the flyout at its widest' },
    );
    expect(
      Math.abs(widest.canvasRight - widest.flyout.right - CANVAS_MIN),
      JSON.stringify(widest),
    ).toBeLessThan(1.5);
  }, 180_000);
});
