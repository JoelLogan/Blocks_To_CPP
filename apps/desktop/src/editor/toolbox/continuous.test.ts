/**
 * The continuous flyout (docs/spec/04-user-interface.md §4.2):
 *
 * - it lands on a chosen category at every zoom level: the scroll goes half a unit past the
 *   category's heading, so that the plugin's own selection by scroll position
 *   (`@blockly/continuous-toolbox` 7.0.9) picks that category again;
 * - it draws its blocks at one scale whatever the canvas zoom, and is at most 320 px wide,
 *   narrower when the canvas would have less than 200 px beside it.
 */
import { ContinuousFlyout } from '@blockly/continuous-toolbox';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import {
  B2cContinuousFlyout,
  B2cContinuousToolbox,
  CATEGORY_SCROLL_SLACK,
  FLYOUT_SCALE,
  FLYOUT_WIDTH,
  flyoutWidth,
} from './continuous';
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
    expect(FLYOUT_WIDTH).toEqual({ preferred: 320, min: 160, canvas: 200 });
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
