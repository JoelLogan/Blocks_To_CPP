/**
 * Scrolling the canvas through the test hook (canvas.ts `scrollIntoView`): one scroll brings a point
 * from anywhere to the comfortable place of the visible canvas, which a pan by dragging could not.
 * And where a pan by dragging starts (`panStartArea`): where the whole pan fits.
 */
import type { WebDriver } from 'selenium-webdriver';
import { describe, expect, it, vi } from 'vitest';

import type { E2ePoint } from '../../src/e2e/contract';
import {
  panStartArea,
  panTowards,
  type Rect,
  scrollIntoView,
  scrollTowards,
  wellInside,
} from './canvas';

const VISIBLE: Rect = { left: 300, top: 50, right: 1_000, bottom: 650 };

/** A fake canvas: a point at `start` that moves with every scroll. */
function canvas(start: E2ePoint, clamp?: E2ePoint) {
  let offset = { x: 0, y: 0 };
  const scrolls: E2ePoint[] = [];
  const driver = {
    executeScript: vi.fn(() =>
      Promise.resolve([VISIBLE.left, VISIBLE.top, VISIBLE.right, VISIBLE.bottom]),
    ),
  } as unknown as WebDriver;
  const scroller = {
    scrollCanvasBy: (dx: number, dy: number): Promise<void> => {
      scrolls.push({ x: dx, y: dy });
      // Blockly keeps the canvas within its content: at most `clamp` per scroll.
      const x = clamp === undefined ? dx : Math.sign(dx) * Math.min(Math.abs(dx), clamp.x);
      const y = clamp === undefined ? dy : Math.sign(dy) * Math.min(Math.abs(dy), clamp.y);
      offset = { x: offset.x + x, y: offset.y + y };
      return Promise.resolve();
    },
  };
  const locate = () => Promise.resolve({ x: start.x + offset.x, y: start.y + offset.y });
  return { driver, scroller, locate, scrolls };
}

describe('scrollTowards', () => {
  it('is the whole way to a third from the left and two fifths from the top', () => {
    expect(scrollTowards({ x: 20_000, y: -3_000 }, VISIBLE)).toEqual({
      x: Math.round(300 + 700 / 3 - 20_000),
      y: 50 + 240 + 3_000,
    });
  });
});

describe('scrollIntoView', () => {
  it('scrolls a far point into view at once, through the hook', async () => {
    const fake = canvas({ x: 25_000, y: 9_000 });
    const point = await scrollIntoView(fake.driver, fake.scroller, fake.locate);
    expect(fake.scrolls).toHaveLength(1);
    expect(wellInside(point, VISIBLE)).toBe(true);
  });

  it('does not scroll a point that is in view already', async () => {
    const fake = canvas({ x: 500, y: 300 });
    expect(await scrollIntoView(fake.driver, fake.scroller, fake.locate)).toEqual({
      x: 500,
      y: 300,
    });
    expect(fake.scrolls).toEqual([]);
  });

  it('scrolls again when the canvas moved less, and gives up after a few tries', async () => {
    const slow = canvas({ x: 3_000, y: 300 }, { x: 1_000, y: 1_000 });
    expect(wellInside(await scrollIntoView(slow.driver, slow.scroller, slow.locate), VISIBLE)).toBe(
      true,
    );
    expect(slow.scrolls.length).toBeGreaterThan(1);
    const stuck = canvas({ x: 30_000, y: 300 }, { x: 10, y: 10 });
    await expect(scrollIntoView(stuck.driver, stuck.scroller, stuck.locate)).rejects.toThrow(
      'Could not scroll',
    );
  });
});

describe('panStartArea', () => {
  it('keeps a pan to the right and down off the right and bottom edges by its length', () => {
    expect(panStartArea(VISIBLE, { x: 64, y: 40 })).toEqual({
      left: 304,
      top: 54,
      right: 1_000 - 4 - 64,
      bottom: 650 - 4 - 40,
    });
  });

  it('keeps a pan to the left and up off the left and top edges by its length', () => {
    expect(panStartArea(VISIBLE, { x: -64, y: -40 })).toEqual({
      left: 300 + 4 + 64,
      top: 50 + 4 + 40,
      right: 996,
      bottom: 646,
    });
  });

  it('leaves room for the longest pan towards a point, which ends inside the canvas', () => {
    // At 800 × 560 the canvas right of the flyout is 184 px wide; panTowards pans at most its width
    // less twice the margin, so the area keeps its start at least a few pixels wide.
    const narrow: Rect = { left: 314, top: 40, right: 498, bottom: 356 };
    for (const point of [
      { x: 218, y: 166 },
      { x: 900, y: -400 },
      { x: -300, y: 900 },
    ]) {
      const delta = panTowards(point, narrow);
      const area = panStartArea(narrow, delta);
      expect(area.right - area.left).toBeGreaterThan(0);
      expect(area.bottom - area.top).toBeGreaterThan(0);
      for (const start of [
        { x: area.left, y: area.top },
        { x: area.right, y: area.bottom },
      ]) {
        const end = { x: start.x + delta.x, y: start.y + delta.y };
        expect(end.x).toBeGreaterThanOrEqual(narrow.left + 4);
        expect(end.x).toBeLessThanOrEqual(narrow.right - 4);
        expect(end.y).toBeGreaterThanOrEqual(narrow.top + 4);
        expect(end.y).toBeLessThanOrEqual(narrow.bottom - 4);
      }
    }
  });
});
