/** Where the drag benchmark's handle is, and whether a drag moved it (handle.ts). */
import { describe, expect, it } from 'vitest';

import {
  aimDelta,
  AIMS,
  checkLanding,
  describePlace,
  isBlockPlace,
  landingError,
  MAX_LANDING_ERROR,
} from './handle';
import type { BlockPlace } from './page';

/** A block 250 by 100 pixels at `left`, `top` on screen, at `x`, `y` on the canvas. */
function place(left: number, top: number, x: number, y: number): BlockPlace {
  return {
    box: { left, top, right: left + 250, bottom: top + 100 },
    offset: { x, y },
  };
}

describe('aimDelta', () => {
  // The visible canvas of a small window with a wide flyout, as on the Windows runner.
  const canvas = { left: 736, top: 128, right: 1012, bottom: 498 };

  it('ends both drags of a pair inside the visible canvas, away up right and back down left', () => {
    const grab = { x: 828, y: 276 };
    const away = aimDelta(grab, canvas, 'away');
    const end = { x: grab.x + away.x, y: grab.y + away.y };
    expect(end).toEqual({
      x: Math.round(736 + 276 * AIMS.away.x),
      y: Math.round(128 + 370 * AIMS.away.y),
    });
    expect(away.x).toBeGreaterThan(0);
    expect(away.y).toBeLessThan(0);
    const back = aimDelta(end, canvas, 'back');
    const home = { x: end.x + back.x, y: end.y + back.y };
    expect(back.x).toBeLessThan(0);
    expect(back.y).toBeGreaterThan(0);
    for (const point of [end, home]) {
      expect(point.x).toBeGreaterThan(canvas.left + 60);
      expect(point.x).toBeLessThan(canvas.right);
      expect(point.y).toBeGreaterThan(canvas.top);
      expect(point.y).toBeLessThan(canvas.bottom);
    }
  });

  it('refuses a drag too short to time', () => {
    expect(() => aimDelta({ x: 929, y: 220 }, canvas, 'away')).toThrow('no room to drag');
  });
});

describe('isBlockPlace', () => {
  it('accepts what PLACE_SCRIPT returns', () => {
    expect(isBlockPlace(place(1, 2, 3.5, -4))).toBe(true);
  });

  it('refuses anything else', () => {
    expect(isBlockPlace(null)).toBe(false);
    expect(isBlockPlace({ box: place(1, 2, 3, 4).box })).toBe(false);
    expect(isBlockPlace({ ...place(1, 2, 3, 4), offset: { x: 1, y: '2' } })).toBe(false);
    expect(isBlockPlace({ ...place(1, 2, 3, 4), box: { left: 1, top: 2, right: 3 } })).toBe(false);
    expect(isBlockPlace({ ...place(1, 2, 3, 4), offset: { x: Number.NaN, y: 0 } })).toBe(false);
  });
});

describe('describePlace', () => {
  const canvas = { left: 300, top: 50, right: 1260, bottom: 780 };

  it('says where the block is against the visible canvas', () => {
    expect(describePlace(place(600, 300, 0, 0), canvas)).toBe(
      'its box on screen is (600, 300)–(850, 400), inside the visible canvas (300, 50)–(1260, 780)',
    );
    expect(describePlace(place(1100.4, 300, 0, 0), canvas)).toMatch(
      /^its box on screen is \(1100, 300\)–\(1350, 400\), partly outside the visible canvas/,
    );
    expect(describePlace(place(-400, 300, 0, 0), canvas)).toMatch(/, outside the visible canvas/);
    expect(describePlace(place(600, 790, 0, 0), canvas)).toMatch(/, outside the visible canvas/);
  });

  it('says when the canvas is not known, or the block is not on it', () => {
    expect(describePlace(place(10, 20, 0, 0), null)).toBe(
      'its box on screen is (10, 20)–(260, 120)',
    );
    expect(describePlace(null, canvas)).toBe('it is not on the canvas');
  });
});

describe('landingError', () => {
  it('is how far from the pointer’s end the block landed, on the canvas', () => {
    expect(
      landingError(place(600, 300, 640, 32), place(840, 420, 880, 152), { x: 240, y: 120 }),
    ).toBe(0);
    // The canvas scrolled after the drop: the box moved, the place on the canvas did not.
    expect(
      landingError(place(600, 300, 640, 32), place(500, 200, 883, 148), { x: 240, y: 120 }),
    ).toBe(5);
  });
});

describe('checkLanding', () => {
  const before = place(600, 300, 640, 32);
  const delta = { x: -240, y: -120 };

  it('returns the landing error of a drag that moved the block', () => {
    expect(checkLanding('b9', delta, before, place(360, 180, 400, -88))).toBe(0);
    // Bumped by a few pixels on the drop: still the drag of the block.
    expect(checkLanding('b9', delta, before, place(360, 180, 403, -84))).toBe(5);
  });

  it('fails when the drag missed the block, or the block is gone', () => {
    expect(() => checkLanding('b9', delta, before, before)).toThrow(
      /drag by \(-240, -120\) moved the drag handle b9 by \(0, 0\): the drag missed it/,
    );
    const length = Math.hypot(delta.x, delta.y);
    const justOver = place(0, 0, 400 + MAX_LANDING_ERROR * length + 1, -88);
    expect(() => checkLanding('b9', delta, before, justOver)).toThrow(/missed it/);
    expect(() => checkLanding('b9', delta, before, null)).toThrow(/b9 is no longer on the canvas/);
  });
});
