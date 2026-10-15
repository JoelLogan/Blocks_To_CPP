/**
 * Measuring and taking a chrome scene (capture.ts): the page scripts run here against a fake page,
 * the parsing of what they return, and the screenshots taken until two in a row are the same,
 * against a fake driver.
 */
import { PNG } from 'pngjs';
import type { WebDriver } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { runScript } from '../support/fake-page';
import {
  CAPTURE_ATTEMPTS,
  captureScene,
  FIX_WIDTHS_SCRIPT,
  LAYOUT_SCRIPT,
  parseLayout,
  UnstableSceneError,
} from './capture';
import { samePixels } from './image';
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

/** A 4×3 PNG of one grey, with pixel (x, 0) black when `dot` is x. */
function frame(dot: number | null = null): Buffer {
  const image = new PNG({ width: 4, height: 3 });
  image.data.fill(200);
  for (let index = 3; index < image.data.length; index += 4) {
    image.data[index] = 255;
  }
  if (dot !== null) {
    image.data.fill(0, dot * 4, dot * 4 + 3);
  }
  return PNG.sync.write(image);
}

/** A driver whose page is 4×3 and whose screenshots are `frames`, one per call (the last again). */
function fakeDriver(frames: Buffer[]): WebDriver & { shots: () => number } {
  let shots = 0;
  const driver = {
    executeScript: () =>
      Promise.resolve({ viewport: [4, 3], areaBoxes: null, masks: [] as unknown[] }),
    takeScreenshot: () => {
      const shot = frames[Math.min(shots, frames.length - 1)];
      shots += 1;
      return Promise.resolve(shot?.toString('base64') ?? '');
    },
    shots: () => shots,
  };
  return driver as unknown as WebDriver & { shots: () => number };
}

describe('captureScene', () => {
  it('takes screenshots until two in a row are the same, and gives that image', async () => {
    const driver = fakeDriver([frame(0), frame(1), frame(2), frame(2)]);
    const image = await captureScene(driver, WINDOW_AREA, []);
    expect(samePixels(image, frame(2))).toBe(true);
    expect(driver.shots()).toBe(4);
  });

  it('is done with two screenshots of a page at rest', async () => {
    const driver = fakeDriver([frame()]);
    expect(samePixels(await captureScene(driver, WINDOW_AREA, []), frame())).toBe(true);
    expect(driver.shots()).toBe(2);
  });

  it('gives up on a scene that keeps changing, with its last two images and what moved', async () => {
    const frames = Array.from({ length: CAPTURE_ATTEMPTS }, (_, index) => frame(index % 2));
    const driver = fakeDriver(frames);
    const error: unknown = await captureScene(driver, WINDOW_AREA, []).catch(
      (thrown: unknown) => thrown,
    );
    expect(error).toBeInstanceOf(UnstableSceneError);
    const unstable = error as UnstableSceneError;
    expect(driver.shots()).toBe(CAPTURE_ATTEMPTS);
    expect(samePixels(unstable.last, frames.at(-1) ?? Buffer.alloc(0))).toBe(true);
    expect(samePixels(unstable.previous, frames.at(-2) ?? Buffer.alloc(0))).toBe(true);
    expect(unstable.changed).toEqual({ left: 0, top: 0, right: 2, bottom: 1 });
    expect(unstable.message).toContain('pixels changed within (0,0)–(2,1) of the scene');
  });
});
