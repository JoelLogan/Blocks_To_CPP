/**
 * A drop anywhere on the toolbox deletes (docs/spec/03-block-language.md §3.3 "Placing blocks"),
 * in the real webview with real pointer drags. Before each drop the toolbox's flyout is built
 * again, as opening a project, selecting a block in another scope or making a variable does: a new
 * variable adds its getter. Blockly records where the flyout is while it is not displayed, so after
 * such a rebuild a drop on the flyout or the category column deleted nothing until the layout
 * changed; only the trash can and the window's left edge still deleted.
 *
 * A *print* at the end of `main`'s body is dropped with the pointer
 *
 * - on the flyout's middle, on its right part (12 px inside its right edge) and on the category
 *   column's middle: while the pointer is there, the toolbox is marked as a place that deletes
 *   (`blocklyToolboxDelete`), the category column is tinted as the flyout is and its names stay
 *   readable (4.5:1), and the block is faded; the drop deletes it, and Ctrl+Z puts it back at the
 *   end of `main`'s body;
 * - 15 px right of the flyout, on the canvas: nothing is marked, nothing is faded, and the block
 *   stays.
 *
 * Blockly judges by the pointer, not by how much of the block lies over the toolbox. The block is
 * looked for in the document (the test hook's text), not by its element, which the hook also
 * looks for in the flyout. The canvas is dragged once first: Blockly records the drop places at the
 * first gesture on the canvas, which would hide a stale record.
 */
import { describe, expect, it } from 'vitest';

import { type BlockNode, declareInt, moduleNodes, NodeIds, print } from '../../support/bdm';
import { bringIntoView, panCanvas } from '../../support/canvas';
import { pressAndMove, releaseButton } from '../../support/drag';
import { blockCorner, centerOn, currentDocument, grabPointOf } from '../../support/editor';
import { sleep, waitFor } from '../../support/wait';
import { contrastRatio } from './lib/contrast';
import { type FlowApp, startFlow } from './lib/launch';
import { mainBody, newEmptyProject } from './lib/project';
import { pressCtrl, UI_TIMEOUT_MS } from './lib/ui';

/** Where the toolbox's two parts are across the window, in viewport pixels. */
interface ToolboxBox {
  readonly column: { readonly left: number; readonly right: number };
  readonly flyout: { readonly left: number; readonly right: number };
}

/**
 * Runs in the webview: the left and right edges of the category column (Blockly's toolbox
 * `div`) and of the flyout (its SVG). A plain script, so nothing the test runner adds to compiled
 * functions can end up in the page.
 */
const TOOLBOX_BOX_SCRIPT = `
  const column = document.querySelector('[data-testid="workspace"] .blocklyToolbox');
  const flyout = document.querySelector('[data-testid="workspace"] svg.blocklyToolboxFlyout');
  if (column === null || flyout === null) {
    return null;
  }
  const c = column.getBoundingClientRect();
  const f = flyout.getBoundingClientRect();
  return [c.left, c.right, f.left, f.right];
`;

/** What the page shows during a drag, from {@link FEEDBACK_SCRIPT}. */
interface DragFeedback {
  /** Blockly is dragging a block. */
  readonly dragging: boolean;
  /** The toolbox has `blocklyToolboxDelete`: a drop now deletes. */
  readonly marked: boolean;
  /** The dragged block is faded (`blocklyDraggingDelete`). */
  readonly faded: boolean;
  /** The category column's background colour, as computed. */
  readonly column: string;
  /** The flyout's fill, as computed. */
  readonly flyout: string;
  /** The colour of the name of a category that is not selected, as computed. */
  readonly name: string;
}

/** Runs in the webview: the {@link DragFeedback} on the page now. */
const FEEDBACK_SCRIPT = `
  const toolbox = document.querySelector('[data-testid="workspace"] .blocklyToolbox');
  const background = document.querySelector(
    '[data-testid="workspace"] .blocklyToolboxFlyout .blocklyFlyoutBackground',
  );
  const name = document.querySelector(
    '[data-testid="workspace"] .blocklyToolboxCategory:not(.blocklyToolboxSelected) .blocklyToolboxCategoryLabel',
  );
  return {
    dragging: document.querySelector('[data-testid="workspace"] .blocklyDragging') !== null,
    marked: toolbox !== null && toolbox.classList.contains('blocklyToolboxDelete'),
    faded: document.querySelector('.blocklyDragging.blocklyDraggingDelete') !== null,
    column: toolbox === null ? '' : getComputedStyle(toolbox).backgroundColor,
    flyout: background === null ? '' : getComputedStyle(background).fill,
    name: name === null ? '' : getComputedStyle(name).color,
  };
`;

/** A computed colour (`rgb(r, g, b)`, `rgba(…)` or `#rrggbb`) as `0xrrggbb`. */
function colourOf(value: string): number {
  const rgb = /^rgba?\(\s*(\d+)[\s,]+(\d+)[\s,]+(\d+)/.exec(value.trim());
  if (rgb !== null) {
    const [, r = '0', g = '0', b = '0'] = rgb;
    return (Number(r) << 16) | (Number(g) << 8) | Number(b);
  }
  const hex = /^#([0-9a-f]{6})$/i.exec(value.trim());
  if (hex !== null) {
    return Number.parseInt(hex[1] ?? '', 16);
  }
  throw new Error(`Not a colour: "${value}"`);
}

/** One drop: where the pointer is released, and whether that deletes the block. */
interface Drop {
  readonly where: string;
  readonly x: (box: ToolboxBox) => number;
  readonly deletes: boolean;
}

const DROPS: readonly Drop[] = [
  {
    where: 'the flyout’s middle',
    x: (box) => (box.flyout.left + box.flyout.right) / 2,
    deletes: true,
  },
  { where: 'the flyout’s right part', x: (box) => box.flyout.right - 12, deletes: true },
  {
    where: 'the category column’s middle',
    x: (box) => (box.column.left + box.column.right) / 2,
    deletes: true,
  },
  {
    where: 'the canvas, 15 px right of the flyout',
    x: (box) => box.flyout.right + 15,
    deletes: false,
  },
];

/** The toolbox's parts on screen. */
async function toolboxBox(app: FlowApp): Promise<ToolboxBox> {
  const found: unknown = await app.driver.executeScript(TOOLBOX_BOX_SCRIPT);
  if (!Array.isArray(found) || found.length !== 4 || !found.every((n) => typeof n === 'number')) {
    throw new Error('The toolbox is not on screen');
  }
  const [columnLeft = 0, columnRight = 0, flyoutLeft = 0, flyoutRight = 0] = found.map(Number);
  return {
    column: { left: columnLeft, right: columnRight },
    flyout: { left: flyoutLeft, right: flyoutRight },
  };
}

/** Whether block `id` is in the open project, at any depth. */
async function inDocument(app: FlowApp, id: string): Promise<boolean> {
  return moduleNodes(await currentDocument(app)).some((node) => node.id === id);
}

/** Waits until `main`'s last statement is block `id`. */
function waitForLastInMain(app: FlowApp, id: string, message: string): Promise<true> {
  return waitFor(
    async () => {
      const body: readonly BlockNode[] = await mainBody(app);
      return body.at(-1)?.id === id ? true : null;
    },
    { timeout: UI_TIMEOUT_MS, message },
  );
}

describe('Dropping a block on the toolbox', () => {
  it('deletes it anywhere over the categories and the open block list, after each rebuild', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const mainId = await newEmptyProject(app);
    const ids = new NodeIds('tray');

    // A first gesture on the canvas, so the drops below find what the toolbox's rebuilds left.
    await app.hook.insertBlocks(mainId, 'BODY', [print(ids, 'first')]);
    await panCanvas(app.driver, { x: 0, y: 40 });

    let tint: number | null = null;
    for (const [index, drop] of DROPS.entries()) {
      // A new variable and a print after it, at the end of main. The print is selected (centring
      // it), so the Variables category lists the variable: the flyout is built again.
      const sym = `s_tray_${String(index)}`;
      const block = print(ids, `drop ${String(index)}`);
      await app.hook.insertBlocks(mainId, 'BODY', [
        declareInt(ids, sym, `v${String(index)}`, index),
        block,
      ]);
      await centerOn(app, block.id);
      await bringIntoView(app.driver, () => blockCorner(app, block.id));
      await waitFor(() => app.hook.flyoutBlockId('var.get', { VAR: sym }), {
        timeout: UI_TIMEOUT_MS,
        message: `the getter of ${sym} in the toolbox`,
      });
      // The toolbox's rebuild is debounced; let any second one run before the drag.
      await sleep(400);

      const box = await toolboxBox(app);
      const grab = await grabPointOf(app, block.id);
      const target = { x: Math.round(drop.x(box)), y: grab.y };
      await pressAndMove(app.driver, grab, target);
      const seen = await app.driver.executeScript<DragFeedback>(FEEDBACK_SCRIPT);
      await releaseButton(app.driver);
      const facts = `${drop.where}: ${JSON.stringify({ box, grab, target, seen })}`;
      expect(seen.dragging, facts).toBe(true);

      if (drop.deletes) {
        expect(seen.marked, facts).toBe(true);
        expect(seen.faded, facts).toBe(true);
        // The category column is tinted as the flyout is, and its names stay readable.
        expect(colourOf(seen.column), facts).toBe(colourOf(seen.flyout));
        expect(
          contrastRatio(colourOf(seen.name), colourOf(seen.column)),
          facts,
        ).toBeGreaterThanOrEqual(4.5);
        tint = colourOf(seen.column);
        await waitFor(async () => !(await inDocument(app, block.id)), {
          timeout: UI_TIMEOUT_MS,
          message: `the print dropped on ${drop.where} to be deleted`,
        });
        await pressCtrl(app.driver, 'z');
        await waitForLastInMain(
          app,
          block.id,
          `Ctrl+Z to put back the print dropped on ${drop.where}`,
        );
      } else {
        expect(seen.marked, facts).toBe(false);
        expect(seen.faded, facts).toBe(false);
        if (tint !== null) {
          expect(colourOf(seen.column), facts).not.toBe(tint);
        }
        // Blockly finishes a drop in the next frames.
        await sleep(400);
        expect(await inDocument(app, block.id), facts).toBe(true);
      }
    }
  }, 240_000);
});
