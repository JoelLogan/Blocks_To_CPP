/**
 * The continuous flyout (docs/spec/04-user-interface.md §4.2):
 *
 * - it lands on a chosen category at every zoom level: the scroll goes half a unit past the
 *   category's heading, so that the plugin's own selection by scroll position
 *   (`@blockly/continuous-toolbox` 7.0.9) picks that category again;
 * - it draws its blocks at one scale whatever the canvas zoom, and is at most 320 px wide unless
 *   the user chose another width (160 px and up), narrower when the canvas would have less than
 *   200 px beside it;
 * - it tells the window's layout where it is and how wide it may be made, and takes the width
 *   the user chose from it;
 * - after every showing, and after every change of its width or place, the canvas knows where it
 *   is as a place where a drop deletes, at any height (03 §3.3 "Placing blocks"): from the next
 *   animation frame on, or from the start of a drag that comes first, and a showing reads nothing
 *   of the laid-out page for it (../sync/dragTargets.ts).
 */
import { ContinuousFlyout } from '@blockly/continuous-toolbox';
import { b2cLightTheme } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import { type FlyoutLayout, listenToFlyoutLayout } from '../../app/layout/flyoutLayout';
import { recordDragTargetsIfStale } from '../sync/dragTargets';
import {
  B2cContinuousFlyout,
  B2cContinuousToolbox,
  CATEGORY_SCROLL_SLACK,
  FLYOUT_SCALE,
  FLYOUT_WIDTH,
  flyoutWidth,
  flyoutWidthLimit,
} from './continuous';
import { toolboxInjectOptions } from './register';
import { disposeTestWorkspaces, injectedWorkspace, setUpToolboxBlocks } from './testing';

beforeAll(() => {
  setUpToolboxBlocks();
});

afterEach(() => {
  vi.restoreAllMocks();
  disposeTestWorkspaces();
});

/** The plugin's selection by scroll position, which is private in 7.0.9. */
type SelectByScroll = (this: unknown, scroll: number) => void;

/**
 * The category the plugin selects when a scroll ends at `scroll` (pixels): its own
 * `selectCategoryByScrollPosition`, run on a stand-in that has the recorded heading positions
 * (workspace units), the flyout's scale and, while an animated scroll runs, its target.
 */
function selectedAt(
  positions: ReadonlyMap<string, number>,
  scale: number,
  scroll: number,
  scrollTarget?: number,
): string | null {
  let selected: string | null = null;
  const standIn = {
    scrollTarget,
    scrollPositions: positions,
    getWorkspace: () => ({ scale }),
    getParentToolbox: () => ({
      selectCategoryByName: (name: string) => {
        selected = name;
      },
    }),
  };
  const select = (ContinuousFlyout.prototype as unknown as Record<string, unknown>)[
    'selectCategoryByScrollPosition'
  ];
  if (typeof select !== 'function') {
    throw new Error('the plugin no longer selects categories by scroll position this way');
  }
  (select as SelectByScroll).call(standIn, scroll);
  return selected;
}

/**
 * The flyout's own scale, and Blockly's zoom steps around the editor's start scale of 0.9 (scale
 * speed 1.2), which the flyout followed before it had a scale of its own.
 */
const SCALES = [FLYOUT_SCALE, ...[-3, -2, -1, 0, 1, 2, 3].map((step) => 0.9 * 1.2 ** step)];

/** Heading positions as the plugin records them: a label's top less half a gap, often fractional. */
const POSITIONS = [12, 12.4, 98.25, 312.4, 312.5, 640.75, 1234.0001, 1234.4999, 2047.9];

/** How far the scroll the flyout ends at may be off its target, relative to it. */
const RELATIVE_ERROR = 1e-12;

describe('choosing a category', () => {
  it('ends the scroll where the plugin selects the chosen category, at every zoom level', () => {
    for (const scale of SCALES) {
      for (const position of POSITIONS) {
        const positions = new Map([
          ['Before', 0],
          ['Chosen', position],
          ['After', position + 40],
        ]);
        const target = (position + CATEGORY_SCROLL_SLACK) * scale;
        for (const end of [target * (1 - RELATIVE_ERROR), target, target * (1 + RELATIVE_ERROR)]) {
          expect(
            selectedAt(positions, scale, end),
            `scale ${String(scale)}, ${String(position)}`,
          ).toBe('Chosen');
        }
      }
    }
  });

  it('needs the slack: without it, a heading with a fraction under one half selected the one before', () => {
    const missed = SCALES.flatMap((scale) =>
      POSITIONS.filter(
        (position) =>
          selectedAt(
            new Map([
              ['Before', 0],
              ['Chosen', position],
            ]),
            scale,
            position * scale * (1 - RELATIVE_ERROR),
          ) !== 'Chosen',
      ),
    );
    expect(missed.length).toBeGreaterThan(0);
  });

  it('keeps the selection while scrolling up to the first category', () => {
    const positions = new Map([
      ['First', 0],
      ['Second', 300],
    ]);
    // The plugin selects nothing while its scroll target is set (truthy) …
    expect(selectedAt(positions, 0.9, 150, CATEGORY_SCROLL_SLACK * 0.9)).toBeNull();
    // … and a target of 0 would count as no scroll at all.
    expect(selectedAt(positions, 0.9, 150, 0)).toBe('First');
  });

  it('asks the plugin to scroll half a unit past the heading', () => {
    const workspace = injectedWorkspace('continuous');
    const flyout = workspace.getFlyout();
    if (!(flyout instanceof B2cContinuousFlyout)) {
      throw new Error('not the continuous flyout');
    }
    const scroll = vi
      .spyOn(ContinuousFlyout.prototype, 'scrollTo')
      .mockImplementation(() => undefined);
    for (const position of [0, 12.4, 312.4]) {
      flyout.scrollTo(position);
      expect(scroll).toHaveBeenLastCalledWith(position + 0.5);
    }
    expect(scroll).toHaveBeenCalledTimes(3);
  });
});

/** The toolbox's flyout of an injected workspace with the continuous toolbox. */
function continuousFlyout(workspace: Blockly.WorkspaceSvg): B2cContinuousFlyout {
  const flyout = workspace.getFlyout();
  if (!(flyout instanceof B2cContinuousFlyout)) {
    throw new Error('not the continuous flyout');
  }
  return flyout;
}

/** What the continuous toolbox shows in its flyout now. */
function workspaceContents(
  workspace: Blockly.WorkspaceSvg,
): Blockly.utils.toolbox.FlyoutItemInfoArray {
  const toolbox = workspace.getToolbox();
  if (!(toolbox instanceof B2cContinuousToolbox)) {
    throw new Error('not the continuous toolbox');
  }
  return toolbox.flyoutContents();
}

/** Gives the workspace a size (happy-dom has no layout) and lets Blockly fit its chrome to it. */
function resizeWorkspace(workspace: Blockly.WorkspaceSvg, width: number, height = 700): void {
  workspace.setCachedParentSvgSize(width, height);
  workspace.resize();
}

/** Makes the flyout's contents one item `width` workspace units wide, as its reflow measures them. */
function widestItem(flyout: B2cContinuousFlyout, width: number): void {
  const element = {
    getBoundingRectangle: () => new Blockly.utils.Rect(0, 40, 0, width),
  };
  const item = { getElement: () => element } as unknown as Blockly.FlyoutItem;
  vi.spyOn(flyout, 'getContents').mockReturnValue([item]);
}

/** The flyout's width for contents `widest` units wide at its widest, as Blockly measures it. */
function naturalFor(flyout: B2cContinuousFlyout, widest: number): number {
  return (
    (widest + flyout.MARGIN * 1.5 + (flyout as unknown as { tabWidth_: number }).tabWidth_) *
      FLYOUT_SCALE +
    Blockly.Scrollbar.scrollbarThickness
  );
}

describe('the flyout width rule', () => {
  it('is as wide as its contents, at most 320 px, and leaves the canvas 200 px down to 160 px', () => {
    // [natural width, room for the flyout and the canvas, width]
    const cases: [number, number, number][] = [
      [600, 794, 320], // 1280 × 800 with the C++ panel shown: cut off at 320 px
      [250, 794, 250], // narrow contents: their own width
      [600, 542, 320], // the Windows runner's 1028 × 749: 222 px of canvas left
      [600, 450, 250], // 200 px of canvas left
      [600, 314, 160], // the smallest window (800 px) with the C++ panel shown: no narrower
      [120, 314, 120],
      [600, 0, 160], // not laid out yet
      [0, 794, 0], // nothing shown yet
    ];
    for (const [natural, room, width] of cases) {
      expect(flyoutWidth(natural, room), `${String(natural)} in ${String(room)}`).toBe(width);
    }
    expect(FLYOUT_WIDTH).toEqual({ preferred: 320, min: 160, max: 1200, canvas: 200 });
  });

  it('takes the width the user chose, from 160 px up to what leaves the canvas 200 px', () => {
    // [chosen width, room for the flyout and the canvas, width]
    const cases: [number, number, number][] = [
      [500, 794, 500], // wider than the default and than its blocks
      [200, 794, 200], // narrower than its blocks: they are cut off
      [700, 794, 594], // 200 px of canvas left
      [100, 794, 160], // no narrower than 160 px
      [500, 314, 160], // the smallest window with the C++ panel shown
      [2000, 3000, 1200], // no wider than 1200 px
    ];
    for (const [chosen, room, width] of cases) {
      expect(flyoutWidth(600, room, chosen), `${String(chosen)} in ${String(room)}`).toBe(width);
      // Whatever the contents need.
      expect(flyoutWidth(0, room, chosen)).toBe(width);
    }
    expect(flyoutWidthLimit(794)).toBe(594);
    expect(flyoutWidthLimit(314)).toBe(160);
    expect(flyoutWidthLimit(5000)).toBe(1200);
  });
});

describe('the flyout scale and width', () => {
  it('draws the blocks at 0.75 whatever the canvas zoom', () => {
    const workspace = injectedWorkspace('continuous');
    const flyout = continuousFlyout(workspace);
    expect(flyout.getFlyoutScale()).toBe(0.75);
    expect(flyout.getWorkspace().scale).toBe(0.75);
    for (const scale of [1.5, 0.5, 0.9]) {
      workspace.setScale(scale);
      expect(workspace.scale).toBe(scale);
      expect(flyout.getWorkspace().scale).toBe(0.75);
    }
    // Showing the contents again (Blockly's layout_ sets the canvas's scale) keeps it, too.
    workspace.setScale(1.5);
    flyout.show(workspaceContents(workspace));
    expect(flyout.getWorkspace().scale).toBe(0.75);
  });

  it('is as wide as its widest block at 0.75, and never wider than 320 px', () => {
    const workspace = injectedWorkspace('continuous');
    resizeWorkspace(workspace, 1000);
    const flyout = continuousFlyout(workspace);
    widestItem(flyout, 200);
    flyout.reflow();
    expect(flyout.getWidth()).toBeCloseTo(naturalFor(flyout, 200), 9);
    expect(flyout.getWidth()).toBeLessThan(FLYOUT_WIDTH.preferred);
    widestItem(flyout, 900);
    flyout.reflow();
    expect(naturalFor(flyout, 900)).toBeGreaterThan(FLYOUT_WIDTH.preferred);
    expect(flyout.getWidth()).toBe(FLYOUT_WIDTH.preferred);
    // A zoomed canvas does not widen it.
    workspace.setScale(2);
    expect(flyout.getWidth()).toBe(FLYOUT_WIDTH.preferred);
  });

  it('narrows when the window leaves the canvas less than 200 px, down to 160 px', () => {
    const workspace = injectedWorkspace('continuous');
    resizeWorkspace(workspace, 1000);
    const flyout = continuousFlyout(workspace);
    widestItem(flyout, 900);
    flyout.reflow();
    const toolbox = workspace.getToolbox()?.getWidth() ?? 0;
    expect(flyout.getWidth()).toBe(320);
    resizeWorkspace(workspace, toolbox + 450);
    expect(flyout.getWidth()).toBe(250);
    resizeWorkspace(workspace, toolbox + 300);
    expect(flyout.getWidth()).toBe(160);
    resizeWorkspace(workspace, 1000);
    expect(flyout.getWidth()).toBe(320);
  });

  it('leaves the canvas to the right of the toolbox and the flyout', () => {
    const workspace = injectedWorkspace('continuous');
    resizeWorkspace(workspace, 1000);
    const flyout = continuousFlyout(workspace);
    widestItem(flyout, 900);
    flyout.reflow();
    const toolbox = workspace.getToolbox()?.getWidth() ?? 0;
    expect(workspace.getMetricsManager().getAbsoluteMetrics().left).toBe(toolbox + 320);
    expect(workspace.getMetricsManager().getViewMetrics().width).toBe(1000 - toolbox - 320);
  });

  it('resizes the canvas only when its width changes, not on every showing', () => {
    const workspace = injectedWorkspace('continuous');
    resizeWorkspace(workspace, 1000);
    const flyout = continuousFlyout(workspace);
    const contents = workspaceContents(workspace);
    flyout.show(contents);
    const resize = vi.spyOn(workspace, 'resizeContents');
    const width = flyout.getWidth();
    flyout.show(contents);
    flyout.show(contents);
    expect(flyout.getWidth()).toBe(width);
    expect(resize).not.toHaveBeenCalled();
    // A new width does resize it, once.
    widestItem(flyout, 900);
    flyout.reflow();
    expect(flyout.getWidth()).not.toBe(width);
    expect(resize).toHaveBeenCalledTimes(1);
  });
});

/** The category column's width in the tests below, in pixels. */
const COLUMN = 100;

/** Gives the category column a width (happy-dom has no layout); Blockly reads it when it places it. */
function setColumnWidth(workspace: Blockly.WorkspaceSvg, width: number): void {
  const column = workspace.getInjectionDiv().querySelector('.blocklyToolbox');
  if (!(column instanceof HTMLElement)) {
    throw new Error('the toolbox has no category column');
  }
  Object.defineProperty(column, 'offsetWidth', { configurable: true, get: () => width });
}

/**
 * How often the flyout's place on screen was read since the counts were last reset: `shown` while
 * the flyout was displayed (in a browser, the first such read after a rebuild lays the page out
 * with the flyout's new blocks at once), `hidden` while it was not.
 */
interface FlyoutReads {
  shown: number;
  hidden: number;
}

/**
 * Lays the flyout's SVG out on screen as a browser does, which happy-dom does not: its box is where
 * Blockly places it (its `transform`) and as wide as its `width`, and it is empty while the SVG is
 * not displayed. `Flyout.show` records the canvas's drag targets while that is so
 * (`setVisible(true)` records them before it displays the flyout again). Returns the count of the
 * reads of that box.
 */
function layOutFlyout(workspace: Blockly.WorkspaceSvg, height = 700): FlyoutReads {
  const svg = workspace.getInjectionDiv().querySelector('.blocklyToolboxFlyout');
  if (!(svg instanceof SVGElement)) {
    throw new Error('the toolbox has no flyout SVG');
  }
  const reads: FlyoutReads = { shown: 0, hidden: 0 };
  vi.spyOn(svg, 'getBoundingClientRect').mockImplementation(() => {
    if (svg.style.display === 'none') {
      reads.hidden += 1;
      return new DOMRect(0, 0, 0, 0);
    }
    reads.shown += 1;
    const left = Number(/translate\(\s*(-?[\d.]+)px/.exec(svg.style.transform)?.[1] ?? 'NaN');
    return new DOMRect(left, 0, Number(svg.getAttribute('width')), height);
  });
  return reads;
}

/**
 * Waits for the next animation frame and for what the editor does in it once Blockly has drawn
 * the blocks it queued: the canvas records its stale drag targets then (../sync/dragTargets.ts).
 */
async function afterNextFrame(): Promise<void> {
  await new Promise<void>((resolve) => {
    requestAnimationFrame(() => {
      resolve();
    });
  });
  await Blockly.renderManagement.finishQueuedRenders();
  await Promise.resolve();
}

/**
 * What a drop at `x`, `y` (screen pixels; by default halfway down the flyout) would land on: the
 * toolbox, another target, or nothing.
 */
function dropTargetAt(workspace: Blockly.WorkspaceSvg, x: number, y = 300): string | null {
  const target = workspace.getDragTarget(
    new PointerEvent('pointermove', { clientX: x, clientY: y }),
  );
  const toolbox: unknown = workspace.getToolbox();
  return target === null ? null : target === toolbox ? 'the toolbox' : target.id;
}

/**
 * A shown editor canvas with the continuous toolbox, a category column {@link COLUMN} wide, and
 * the count of the reads of its flyout's place on screen.
 */
function laidOut(): { workspace: Blockly.WorkspaceSvg; reads: FlyoutReads } {
  const workspace = injectedWorkspace('continuous');
  setColumnWidth(workspace, COLUMN);
  resizeWorkspace(workspace, 1000);
  const reads = layOutFlyout(workspace);
  return { workspace, reads };
}

/** A shown editor canvas with the continuous toolbox, a category column {@link COLUMN} wide. */
function laidOutWorkspace(): Blockly.WorkspaceSvg {
  return laidOut().workspace;
}

/**
 * Expects drops left of `right` (screen pixels) to land on the toolbox, and drops right of it on
 * nothing.
 */
function expectToolboxUpTo(workspace: Blockly.WorkspaceSvg, right: number): void {
  // The toolbox stands for its flyout (the plugin's getClientRect), and comes first.
  for (const x of [COLUMN / 2, COLUMN + 1, (COLUMN + right) / 2, right - 1]) {
    expect(dropTargetAt(workspace, x), `x = ${String(x)}`).toBe('the toolbox');
  }
  expect(dropTargetAt(workspace, right + 1)).toBeNull();
}

describe('the flyout as a place where a drop deletes', () => {
  it('is recorded where it is after a showing that keeps its width', async () => {
    const workspace = laidOutWorkspace();
    const flyout = continuousFlyout(workspace);
    const contents = workspaceContents(workspace);
    flyout.show(contents);
    const width = flyout.getWidth();
    expect(width).toBeGreaterThan(0);
    expect(flyout.getX()).toBe(COLUMN);
    await afterNextFrame();

    // A rebuild with the same contents (a dynamic category built again), twice.
    flyout.show(contents);
    flyout.show(contents);
    expect(flyout.getWidth()).toBe(width);
    await afterNextFrame();
    expectToolboxUpTo(workspace, COLUMN + width);
  });

  it('is recorded at the start of a drag that comes before the next frame', async () => {
    const workspace = laidOutWorkspace();
    const flyout = continuousFlyout(workspace);
    const contents = workspaceContents(workspace);
    flyout.show(contents);
    await afterNextFrame();

    // Shown again from hidden: Blockly recorded the flyout while it was not displayed, as ending
    // at the window's edge (a flyout shown again while displayed stays displayed, and Blockly
    // records nothing then).
    flyout.hide();
    flyout.show(contents);
    expect(dropTargetAt(workspace, COLUMN + 1)).toBeNull();
    // What the editor's block dragger does when a drag starts.
    expect(recordDragTargetsIfStale(workspace)).toBe(true);
    expectToolboxUpTo(workspace, COLUMN + flyout.getWidth());
    // The frame then has nothing left to record.
    const record = vi.spyOn(workspace, 'recordDragTargets');
    await afterNextFrame();
    expect(record).not.toHaveBeenCalled();
  });

  it('costs a rebuild no layout: the record waits for the next frame, once', async () => {
    const { workspace, reads } = laidOut();
    const flyout = continuousFlyout(workspace);
    const contents = workspaceContents(workspace);
    flyout.show(contents);
    await afterNextFrame();
    const record = vi.spyOn(workspace, 'recordDragTargets');
    reads.shown = 0;
    reads.hidden = 0;

    // Two rebuilds in one task (a refresh after an edit shows the flyout once). The flyout stays
    // displayed, so Blockly's hide() and setVisible(true) record nothing.
    flyout.show(contents);
    flyout.show(contents);
    expect(record).not.toHaveBeenCalled();
    expect(reads.shown).toBe(0);
    expect(reads.hidden).toBe(0);

    // One record in the next frame: the toolbox and the flyout each read the flyout once.
    await afterNextFrame();
    expect(record).toHaveBeenCalledTimes(1);
    expect(reads.shown).toBe(2);
    // A drag that starts now records nothing.
    expect(recordDragTargetsIfStale(workspace)).toBe(false);
    await afterNextFrame();
    expect(record).toHaveBeenCalledTimes(1);
  });

  it('is recorded again where it moves to, also with the same width', async () => {
    const { workspace, reads } = laidOut();
    const flyout = continuousFlyout(workspace);
    flyout.show(workspaceContents(workspace));
    await afterNextFrame();
    const width = flyout.getWidth();
    expect(dropTargetAt(workspace, COLUMN + width + 20)).toBeNull();
    const record = vi.spyOn(workspace, 'recordDragTargets');
    reads.shown = 0;

    // A wider category column: Blockly places the toolbox and its flyout again.
    setColumnWidth(workspace, COLUMN + 40);
    workspace.getToolbox()?.position();
    expect(flyout.getX()).toBe(COLUMN + 40);
    expect(flyout.getWidth()).toBe(width);
    expect(record).not.toHaveBeenCalled();
    expect(reads.shown).toBe(0);
    await afterNextFrame();
    expect(record).toHaveBeenCalledTimes(1);
    expect(dropTargetAt(workspace, COLUMN + width + 20)).toBe('the toolbox');
    expect(dropTargetAt(workspace, COLUMN + 40 + width + 1)).toBeNull();

    // Placed again where it is: nothing to record.
    workspace.getToolbox()?.position();
    await afterNextFrame();
    expect(record).toHaveBeenCalledTimes(1);
  });

  it('reaches up and down without end, as Blockly makes it', async () => {
    // A drop with the pointer left of the flyout's right edge deletes at any height, also over
    // the toolbar above the editor or the bottom dock below it (03 §3.3 "Placing blocks").
    const workspace = laidOutWorkspace();
    const flyout = continuousFlyout(workspace);
    const contents = workspaceContents(workspace);
    flyout.show(contents);
    flyout.show(contents);
    await afterNextFrame();
    const right = COLUMN + flyout.getWidth();
    for (const y of [-500, -1, 701, 5000]) {
      for (const x of [COLUMN / 2, COLUMN + 1, right - 1]) {
        expect(dropTargetAt(workspace, x, y), `x = ${String(x)}, y = ${String(y)}`).toBe(
          'the toolbox',
        );
      }
      expect(dropTargetAt(workspace, right + 1, y), `y = ${String(y)}`).toBeNull();
    }
  });
});

/** Stops the listeners of {@link announcements}. */
const stopListening: (() => void)[] = [];

afterEach(() => {
  for (const stop of stopListening.splice(0)) {
    stop();
  }
});

/** Everything a flyout on the page announces to the layout around it, from now on. */
function announcements(): FlyoutLayout[] {
  const announced: FlyoutLayout[] = [];
  stopListening.push(
    listenToFlyoutLayout(document.body, (layout, from) => {
      expect(from.classList.contains('injectionDiv')).toBe(true);
      announced.push(layout);
    }),
  );
  return announced;
}

/** The layout the flyout announced last. */
function lastOf(announced: readonly FlyoutLayout[]): FlyoutLayout {
  const last = announced.at(-1);
  if (last === undefined) {
    throw new Error('nothing was announced');
  }
  return last;
}

describe('the width the user chooses (the Resize the toolbox splitter)', () => {
  it('is offered to the layout with the flyout’s place and how wide it may be made', () => {
    const announced = announcements();
    const workspace = laidOutWorkspace();
    const flyout = continuousFlyout(workspace);
    const contents = workspaceContents(workspace);
    flyout.show(contents);
    const layout = lastOf(announced);
    expect(layout).toMatchObject({ left: COLUMN, width: flyout.getWidth(), min: 160 });
    // 1000 px less the category column, less 200 px of canvas.
    expect(layout.max).toBe(1000 - COLUMN - 200);

    // Nothing changed: the layout hears nothing (a rebuild of a dynamic category).
    const count = announced.length;
    flyout.show(contents);
    flyout.show(contents);
    expect(announced).toHaveLength(count);

    // A smaller window: the flyout may be made less wide.
    resizeWorkspace(workspace, 700);
    expect(lastOf(announced).max).toBe(700 - COLUMN - 200);

    // Only the toolbox's flyout: the trash can's own (the same class, never shown) is not heard of.
    expect(workspace.trashcan?.flyout).toBeInstanceOf(B2cContinuousFlyout);
    expect(new Set(announced.map((layout) => layout.setWidth)).size).toBe(1);
  });

  it('is taken, kept within 160 px and what leaves the canvas 200 px, and kept as the window changes', () => {
    const announced = announcements();
    const workspace = laidOutWorkspace();
    const flyout = continuousFlyout(workspace);
    widestItem(flyout, 900);
    flyout.reflow();
    expect(flyout.getWidth()).toBe(320);

    lastOf(announced).setWidth(500);
    expect(flyout.getWidth()).toBe(500);
    expect(lastOf(announced).width).toBe(500);
    expect(workspace.getMetricsManager().getAbsoluteMetrics().left).toBe(COLUMN + 500);

    // A narrower window narrows it; a wider one gives it back the width the user chose.
    resizeWorkspace(workspace, COLUMN + 450);
    expect(flyout.getWidth()).toBe(250);
    resizeWorkspace(workspace, 1000);
    expect(flyout.getWidth()).toBe(500);

    lastOf(announced).setWidth(50);
    expect(flyout.getWidth()).toBe(160);
    lastOf(announced).setWidth(5000);
    expect(flyout.getWidth()).toBe(1000 - COLUMN - 200);
    // Back to the default: as wide as its blocks, at most 320 px.
    lastOf(announced).setWidth(null);
    expect(flyout.getWidth()).toBe(320);
  });

  it('is kept when the blocks are shown again and when the canvas zooms', () => {
    const announced = announcements();
    const workspace = laidOutWorkspace();
    const flyout = continuousFlyout(workspace);
    const contents = workspaceContents(workspace);
    flyout.show(contents);
    lastOf(announced).setWidth(400);
    const resize = vi.spyOn(workspace, 'resizeContents');
    flyout.show(contents);
    expect(flyout.getWidth()).toBe(400);
    workspace.setScale(2);
    expect(flyout.getWidth()).toBe(400);
    // Choosing the width it has already resizes nothing.
    lastOf(announced).setWidth(400);
    expect(resize).not.toHaveBeenCalled();
    expect(flyout.getWorkspace().scale).toBe(FLYOUT_SCALE);
  });

  it('moves the place where a drop deletes with the flyout’s edge', async () => {
    const announced = announcements();
    const workspace = laidOutWorkspace();
    const flyout = continuousFlyout(workspace);
    flyout.show(workspaceContents(workspace));
    for (const width of [480, 200]) {
      lastOf(announced).setWidth(width);
      await afterNextFrame();
      expect(dropTargetAt(workspace, COLUMN + width - 1)).toBe('the toolbox');
      expect(dropTargetAt(workspace, COLUMN + width + 1)).toBeNull();
    }
  });

  it('tells the layout when the flyout is gone', () => {
    const announced = announcements();
    // A workspace of the test's own, which it disposes itself.
    const host = document.createElement('div');
    document.body.append(host);
    const options = toolboxInjectOptions();
    const workspace = Blockly.inject(host, {
      renderer: 'zelos',
      theme: b2cLightTheme,
      sounds: false,
      toolbox: options.toolbox,
      plugins: { ...options.plugins },
    });
    resizeWorkspace(workspace, 1000);
    continuousFlyout(workspace).show(workspaceContents(workspace));
    expect(lastOf(announced).width).toBeGreaterThan(0);
    workspace.dispose();
    expect(lastOf(announced).width).toBe(0);
  });
});
