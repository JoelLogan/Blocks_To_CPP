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
 * - on the flyout's middle, on its right part (12 px inside its right edge), on the category
 *   column's middle, and in the flyout's strip above the editor (over the toolbar) and below it
 *   (over the bottom dock), since the toolbox counts at any height: while the pointer is there,
 *   the toolbox is marked as a place that deletes (`blocklyToolboxDelete`), the category column is
 *   tinted as the flyout is and its names stay readable (4.5:1), and the block is faded; the drop
 *   deletes it, and Ctrl+Z puts it back at the end of `main`'s body;
 * - on the trash can: the block is faded and the can opens its lid, but the toolbox is neither
 *   marked nor tinted; the drop deletes it, and Ctrl+Z puts it back;
 * - 15 px right of the flyout, on the canvas: nothing is marked, nothing is faded, and the block
 *   stays.
 *
 * Blockly judges by the pointer, not by how much of the block lies over the toolbox. The block is
 * looked for in the document (the test hook's text), not by its element, which the hook also
 * looks for in the flyout. The canvas is dragged once first: Blockly records the drop places at the
 * first gesture on the canvas, which would hide a stale record.
 */
import { describe, expect, it } from 'vitest';

import type { E2ePoint } from '../../../src/e2e/contract';
import { type BlockNode, declareInt, moduleNodes, NodeIds, print } from '../../support/bdm';
import { bringIntoView, panCanvas } from '../../support/canvas';
import { pressAndMove, releaseButton } from '../../support/drag';
import { blockCorner, centerOn, currentDocument, grabPointOf } from '../../support/editor';
import { sleep, waitFor } from '../../support/wait';
import { contrastRatio } from './lib/contrast';
import { type FlowApp, startFlow } from './lib/launch';
import { mainBody, newEmptyProject } from './lib/project';
import { pressCtrl, UI_TIMEOUT_MS } from './lib/ui';

/** Where the toolbox, the editor and the trash can are, in viewport pixels. */
interface Layout {
  /** The category column's left and right edges. */
  readonly column: { readonly left: number; readonly right: number };
  /** The flyout's left and right edges. */
  readonly flyout: { readonly left: number; readonly right: number };
  /** The editor's (the toolbox's and the canvas's) top and bottom edges. */
  readonly editor: { readonly top: number; readonly bottom: number };
  /** The window's inner height. */
  readonly windowHeight: number;
  /** The middle of the trash can as drawn (its lid and its body). */
  readonly trash: E2ePoint;
}

/**
 * Runs in the webview: the edges of the category column (Blockly's toolbox `div`), of the flyout
 * (its SVG) and of the editor, the window's inner height, and the middle of the trash can. The
 * trash can's group is as large as its whole sprite sheet, so the middle is taken from the
 * rectangles that clip its lid and its body, in the can's own units, mapped to the window by its
 * screen transform. A plain script, so nothing the test runner adds to compiled functions can end
 * up in the page.
 */
const LAYOUT_SCRIPT = `
  const editor = document.querySelector('[data-testid="workspace"]');
  const column = editor?.querySelector('.blocklyToolbox');
  const flyout = editor?.querySelector('svg.blocklyToolboxFlyout');
  const trash = editor?.querySelector('.blocklyTrash');
  if (!editor || !column || !flyout || !trash) {
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
  const c = column.getBoundingClientRect();
  const f = flyout.getBoundingClientRect();
  const e = editor.getBoundingClientRect();
  return [
    c.left, c.right, f.left, f.right, e.top, e.bottom, innerHeight,
    m.a * x + m.c * y + m.e, m.b * x + m.d * y + m.f,
  ];
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
  /** How far the trash can's lid is open, in degrees (Blockly opens it to 45). */
  readonly lid: number;
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
  const lid = document.querySelectorAll('[data-testid="workspace"] .blocklyTrash image')[1];
  const turns = lid === undefined ? null : lid.transform.baseVal;
  return {
    dragging: document.querySelector('[data-testid="workspace"] .blocklyDragging') !== null,
    marked: toolbox !== null && toolbox.classList.contains('blocklyToolboxDelete'),
    faded: document.querySelector('.blocklyDragging.blocklyDraggingDelete') !== null,
    column: toolbox === null ? '' : getComputedStyle(toolbox).backgroundColor,
    flyout: background === null ? '' : getComputedStyle(background).fill,
    name: name === null ? '' : getComputedStyle(name).color,
    lid: turns === null || turns.numberOfItems === 0 ? 0 : Math.abs(turns.getItem(0).angle),
  };
`;

/** The lid angle from which the trash can counts as open (Blockly opens it to 45 degrees). */
const LID_OPEN = 40;

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

/** What a drop does with the block. */
type Outcome =
  /** Deletes it, over the toolbox: the toolbox is marked and tinted. */
  | 'deleted by the toolbox'
  /** Deletes it, over the trash can: the can opens its lid, the toolbox is not tinted. */
  | 'deleted by the trash can'
  /** Leaves it on the canvas. */
  | 'kept';

/** One drop: where the pointer is released, and what that does with the block. */
interface Drop {
  readonly where: string;
  readonly at: (layout: Layout, grab: E2ePoint) => E2ePoint;
  readonly outcome: Outcome;
  /** The place is above or below the editor (checked). */
  readonly outsideEditor?: true;
}

/** Halfway across the flyout. */
function flyoutMiddle(layout: Layout): number {
  return (layout.flyout.left + layout.flyout.right) / 2;
}

const DROPS: readonly Drop[] = [
  {
    where: 'the flyout’s middle',
    at: (layout, grab) => ({ x: flyoutMiddle(layout), y: grab.y }),
    outcome: 'deleted by the toolbox',
  },
  {
    where: 'the flyout’s right part',
    at: (layout, grab) => ({ x: layout.flyout.right - 12, y: grab.y }),
    outcome: 'deleted by the toolbox',
  },
  {
    where: 'the category column’s middle',
    at: (layout, grab) => ({ x: (layout.column.left + layout.column.right) / 2, y: grab.y }),
    outcome: 'deleted by the toolbox',
  },
  {
    where: 'the flyout’s strip above the editor, over the toolbar',
    at: (layout) => ({ x: flyoutMiddle(layout), y: layout.editor.top / 2 }),
    outcome: 'deleted by the toolbox',
    outsideEditor: true,
  },
  {
    where: 'the flyout’s strip below the editor, over the bottom dock',
    at: (layout) => ({
      x: flyoutMiddle(layout),
      y: (layout.editor.bottom + layout.windowHeight) / 2,
    }),
    outcome: 'deleted by the toolbox',
    outsideEditor: true,
  },
  {
    where: 'the trash can',
    at: (layout) => layout.trash,
    outcome: 'deleted by the trash can',
  },
  {
    where: 'the canvas, 15 px right of the flyout',
    at: (layout, grab) => ({ x: layout.flyout.right + 15, y: grab.y }),
    outcome: 'kept',
  },
];

/** The toolbox, the editor and the trash can on screen. */
async function layoutOf(app: FlowApp): Promise<Layout> {
  const found: unknown = await app.driver.executeScript(LAYOUT_SCRIPT);
  if (!Array.isArray(found) || found.length !== 9 || !found.every((n) => typeof n === 'number')) {
    throw new Error(`The toolbox, the editor or the trash can is not on screen: ${String(found)}`);
  }
  const [cl = 0, cr = 0, fl = 0, fr = 0, top = 0, bottom = 0, height = 0, tx = 0, ty = 0] =
    found.map(Number);
  return {
    column: { left: cl, right: cr },
    flyout: { left: fl, right: fr },
    editor: { top, bottom },
    windowHeight: height,
    trash: { x: tx, y: ty },
  };
}

/**
 * What the page shows during the drag, with the pointer at `drop`'s place. Blockly opens the trash
 * can's lid in a few frames, so over the trash can the page is read until the lid is open or a
 * second has passed.
 */
async function feedbackAt(app: FlowApp, drop: Drop): Promise<DragFeedback> {
  const deadline = Date.now() + 1000;
  for (;;) {
    const seen = await app.driver.executeScript<DragFeedback>(FEEDBACK_SCRIPT);
    if (
      drop.outcome !== 'deleted by the trash can' ||
      seen.lid >= LID_OPEN ||
      Date.now() > deadline
    ) {
      return seen;
    }
    await sleep(50);
  }
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
  it('deletes it anywhere over the categories and the open block list, at any height, after each rebuild', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const mainId = await newEmptyProject(app);
    const ids = new NodeIds('tray');

    // A first gesture on the canvas, so the drops below find what the toolbox's rebuilds left.
    await app.hook.insertBlocks(mainId, 'BODY', [print(ids, 'first')]);
    await panCanvas(app.driver, { x: 0, y: 40 });

    // The tint of a toolbox drop; the drops over the toolbox come first.
    let tint: number | null = null;
    for (const [index, drop] of DROPS.entries()) {
      // A new variable and a print after it, at the end of main. The Variables category lists
      // the new variable: the flyout is built again. The print is centred (and selected).
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

      const layout = await layoutOf(app);
      const grab = await grabPointOf(app, block.id);
      const target = drop.at(layout, grab);
      await pressAndMove(app.driver, grab, target);
      const seen = await feedbackAt(app, drop);
      await releaseButton(app.driver);
      const facts = `${drop.where}: ${JSON.stringify({ layout, grab, target, seen })}`;
      expect(seen.dragging, facts).toBe(true);
      if (drop.outsideEditor === true) {
        expect(target.y < layout.editor.top || target.y > layout.editor.bottom, facts).toBe(true);
      }

      if (drop.outcome === 'kept') {
        expect(seen.marked, facts).toBe(false);
        expect(seen.faded, facts).toBe(false);
        expect(colourOf(seen.column), facts).not.toBe(tint);
        // Blockly finishes a drop in the next frames.
        await sleep(400);
        expect(await inDocument(app, block.id), facts).toBe(true);
        continue;
      }

      expect(seen.faded, facts).toBe(true);
      if (drop.outcome === 'deleted by the toolbox') {
        expect(seen.marked, facts).toBe(true);
        // The category column is tinted as the flyout is, and its names stay readable.
        expect(colourOf(seen.column), facts).toBe(colourOf(seen.flyout));
        expect(
          contrastRatio(colourOf(seen.name), colourOf(seen.column)),
          facts,
        ).toBeGreaterThanOrEqual(4.5);
        tint = colourOf(seen.column);
      } else {
        // Over the trash can only the block is faded, and the can opens its lid.
        expect(seen.marked, facts).toBe(false);
        expect(colourOf(seen.column), facts).not.toBe(tint);
        expect(seen.lid, facts).toBeGreaterThanOrEqual(LID_OPEN);
      }
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
    }
  }, 240_000);
});
