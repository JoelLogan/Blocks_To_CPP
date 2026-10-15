/**
 * Measuring a chrome scene in the page (capture.ts): the page scripts run here against a fake
 * page, and the parsing of what they return.
 */
import { describe, expect, it } from 'vitest';

import { runScript } from '../support/fake-page';
import { FIX_WIDTHS_SCRIPT, LAYOUT_SCRIPT, parseLayout } from './capture';
import { TOOLCHAIN_MASK, WINDOW_AREA } from './scenes';

/** A fake element: a box, and the inline style properties set on it. */
class FakeElement {
  readonly style = {
    values: new Map<string, string>(),
    setProperty: (name: string, value: string) => {
      this.style.values.set(name, value);
    },
  };

  readonly box: { left: number; top: number; width: number; height: number };

  constructor(box: { left: number; top: number; width: number; height: number }) {
    this.box = box;
  }

  getBoundingClientRect() {
    return {
      ...this.box,
      right: this.box.left + this.box.width,
      bottom: this.box.top + this.box.height,
    };
  }
}

/** A fake document whose selectors find the elements given for them. */
function fakePage(elements: Record<string, FakeElement[]>, viewport = { width: 800, height: 560 }) {
  return {
    window: { innerWidth: viewport.width, innerHeight: viewport.height },
    document: {
      querySelector: (selector: string) => elements[selector]?.[0] ?? null,
      querySelectorAll: (selector: string) => elements[selector] ?? [],
    },
  };
}

describe('FIX_WIDTHS_SCRIPT', () => {
  it('fixes the width of every element of a mask that has one, and only those', () => {
    const toolchain = new FakeElement({ left: 6, top: 536, width: 136, height: 24 });
    const other = new FakeElement({ left: 0, top: 0, width: 10, height: 10 });
    const page = fakePage({ [TOOLCHAIN_MASK.selector]: [toolchain], '.other': [other] });
    const changed = runScript(FIX_WIDTHS_SCRIPT, page, [
      [
        { selector: TOOLCHAIN_MASK.selector, width: 200 },
        { selector: '.other', width: null },
        { selector: '.missing', width: 50 },
      ],
    ]);
    expect(changed).toBe(1);
    expect(Object.fromEntries(toolchain.style.values)).toEqual({
      'box-sizing': 'border-box',
      width: '200px',
      'min-width': '200px',
      'max-width': '200px',
      flex: 'none',
      overflow: 'hidden',
      'white-space': 'nowrap',
    });
    expect(other.style.values.size).toBe(0);
  });
});

describe('LAYOUT_SCRIPT and parseLayout', () => {
  const toolbox = new FakeElement({ left: 0, top: 40, width: 100, height: 332 });
  const flyout = new FakeElement({ left: 100, top: 40, width: 214, height: 332 });
  const mask = new FakeElement({ left: 6, top: 536, width: 200, height: 24 });
  const page = fakePage({
    '.blocklyToolbox': [toolbox],
    'svg.blocklyToolboxFlyout': [flyout],
    '.mask': [mask, mask],
  });

  it('measures the box around the area’s elements, and every mask', () => {
    const area = ['.blocklyToolbox', 'svg.blocklyToolboxFlyout'];
    const layout = parseLayout(runScript(LAYOUT_SCRIPT, page, [area, ['.mask', '.none']]), area);
    expect(layout).toEqual({
      viewport: { width: 800, height: 560 },
      clip: { left: 0, top: 40, width: 314, height: 332 },
      masks: [mask.box, mask.box],
    });
  });

  it('measures the whole window', () => {
    const layout = parseLayout(runScript(LAYOUT_SCRIPT, page, [WINDOW_AREA, []]), WINDOW_AREA);
    expect(layout).toEqual({
      viewport: { width: 800, height: 560 },
      clip: { left: 0, top: 0, width: 800, height: 560 },
      masks: [],
    });
  });

  it('names an element of the area that is missing or not shown', () => {
    const area = ['.blocklyToolbox', '.gone'];
    expect(() => parseLayout(runScript(LAYOUT_SCRIPT, page, [area, []]), area)).toThrow(
      /\.gone is not on the page or not shown/,
    );
    const hidden = fakePage({ '.x': [new FakeElement({ left: 0, top: 0, width: 0, height: 9 })] });
    expect(() => parseLayout(runScript(LAYOUT_SCRIPT, hidden, [['.x'], []]), ['.x'])).toThrow(
      /not shown/,
    );
  });

  it('refuses what is not a measurement', () => {
    expect(() => parseLayout(null, WINDOW_AREA)).toThrow(/did not measure/);
    expect(() => parseLayout({ viewport: [0, 5], masks: [] }, WINDOW_AREA)).toThrow(/its size/);
    expect(() => parseLayout({ viewport: [5, 5], areaBoxes: [] }, ['.a'])).toThrow(
      /did not measure the scene’s elements/,
    );
    // A mask that is not a box is left out rather than painted somewhere.
    expect(
      parseLayout(
        { viewport: [5, 5], masks: [{ left: 1 }, { left: 0, top: 0, width: 1, height: 1 }] },
        WINDOW_AREA,
      ).masks,
    ).toEqual([{ left: 0, top: 0, width: 1, height: 1 }]);
  });
});
