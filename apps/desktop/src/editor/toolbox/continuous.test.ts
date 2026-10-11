/**
 * The continuous flyout lands on a chosen category at every zoom level (docs/spec/04-user-interface.md
 * §4.2): the scroll goes half a unit past the category's heading, so that the plugin's own
 * selection by scroll position (`@blockly/continuous-toolbox` 7.0.9) picks that category again.
 */
import { ContinuousFlyout } from '@blockly/continuous-toolbox';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import { B2cContinuousFlyout, CATEGORY_SCROLL_SLACK } from './continuous';
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

/** Blockly's zoom steps around the editor's start scale of 0.9 (scale speed 1.2). */
const SCALES = [-3, -2, -1, 0, 1, 2, 3].map((step) => 0.9 * 1.2 ** step);

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
