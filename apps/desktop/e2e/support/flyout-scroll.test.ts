/**
 * Scrolling the toolbox's flyout down to a block (editor.ts `flyoutGrabPoint`), against a fake
 * scrollbar: a step moves the handle by at most half its height, so on a long toolbox (a 9-pixel
 * handle on a 222-pixel track at 800 × 560) the handle needs over fifty steps to reach the bottom
 * of its track. The steps end when the block shows, when the handle has no room left or stops
 * moving, and at a ceiling derived from the track, never at a fixed count (20 steps of 4 pixels
 * reached only 80 of the 213 pixels).
 */
import { describe, expect, it } from 'vitest';

import type { E2ePoint } from '../../src/e2e/contract';
import {
  type FlyoutScrollbar,
  flyoutScrollCeiling,
  nextFlyoutScroll,
  scrollFlyoutUntil,
} from './editor';

/** The handle and track of the toolbox at 800 × 560 with W2's categories, in pixels. */
const HANDLE = 9;
const TRACK = 222;

/** A scrollbar whose handle moves along its track as the test drags it. */
class FakeScrollbar {
  /** The handle's top, from the track's top. */
  top = 0;
  /** Drags made, by how far each moved the pointer. */
  readonly moves: number[] = [];
  /** Whether a drag moves the handle (false: the drag misses it). */
  moving = true;
  readonly handle = HANDLE;
  readonly track = TRACK;

  get room(): number {
    return this.track - this.handle - this.top;
  }

  read(): FlyoutScrollbar {
    return {
      x: 10,
      y: 100 + this.top + this.handle / 2,
      room: this.room,
      handle: this.handle,
      track: this.track,
    };
  }

  drag(from: E2ePoint, dy: number): void {
    expect(from).toEqual({ x: 10, y: 100 + this.top + this.handle / 2 });
    this.moves.push(dy);
    if (this.moving) {
      this.top = Math.min(this.track - this.handle, this.top + dy);
    }
  }
}

/** Scrolls `bar` until its handle is within `at` pixels of the bottom of its track (a block there). */
function scrollTo(bar: FakeScrollbar, at = 0): Promise<string | null> {
  return scrollFlyoutUntil(
    () => Promise.resolve(bar.room <= at ? 'found' : null),
    () => Promise.resolve(bar.read()),
    (from, dy) => {
      bar.drag(from, dy);
      return Promise.resolve();
    },
  );
}

describe('scrolling the toolbox flyout to a block', () => {
  it('reaches a block at the bottom of the track with a 9-pixel handle on a 222-pixel track', async () => {
    const bar = new FakeScrollbar();
    await expect(scrollTo(bar)).resolves.toBe('found');
    expect(bar.room).toBe(0);
    // Half the handle (4 pixels) a step, the last step only the room left: more than the 20 steps
    // a fixed count allowed.
    expect(bar.moves.length).toBe(Math.ceil((TRACK - HANDLE) / 4));
    expect(bar.moves.length).toBeGreaterThan(20);
    expect(bar.moves.every((dy) => dy > 0 && dy <= 4)).toBe(true);
    expect(bar.moves.length).toBeLessThanOrEqual(flyoutScrollCeiling(bar.read()));
  });

  it('stops scrolling as soon as the block shows', async () => {
    const bar = new FakeScrollbar();
    await expect(scrollTo(bar, 150)).resolves.toBe('found');
    expect(bar.room).toBeGreaterThan(146);
    expect(bar.room).toBeLessThanOrEqual(150);
  });

  it('gives up when the handle reaches the bottom without the block', async () => {
    const bar = new FakeScrollbar();
    await expect(scrollTo(bar, -1)).resolves.toBeNull();
    expect(bar.room).toBe(0);
  });

  it('gives up when a drag does not move the handle', async () => {
    const bar = new FakeScrollbar();
    bar.moving = false;
    await expect(scrollTo(bar)).resolves.toBeNull();
    expect(bar.moves.length).toBe(1);
  });

  it('gives up at once when no scrollbar shows', async () => {
    await expect(
      scrollFlyoutUntil(
        () => Promise.resolve(null),
        () => Promise.resolve(null),
        () => Promise.reject(new Error('no drag without a scrollbar')),
      ),
    ).resolves.toBeNull();
  });

  it('steps half the handle, at least 2 and at most 40 pixels, never past the room left', () => {
    const bar = (handle: number, room: number, y = 50): FlyoutScrollbar => ({
      x: 0,
      y,
      room,
      handle,
      track: 400,
    });
    expect(nextFlyoutScroll(bar(9, 100), null, 0)).toBe(4);
    expect(nextFlyoutScroll(bar(2, 100), null, 0)).toBe(2);
    expect(nextFlyoutScroll(bar(200, 100), null, 0)).toBe(40);
    expect(nextFlyoutScroll(bar(9, 3), null, 0)).toBe(3);
    expect(nextFlyoutScroll(bar(9, 0.5), null, 0)).toBeNull();
    // The handle did not move since the step before.
    expect(nextFlyoutScroll(bar(9, 100), bar(9, 100), 1)).toBeNull();
    expect(nextFlyoutScroll(bar(9, 96, 54), bar(9, 100), 1)).toBe(4);
  });

  it('bounds the steps by the track: enough to cross it, and one more', () => {
    const long: FlyoutScrollbar = { x: 0, y: 0, room: 213, handle: HANDLE, track: TRACK };
    expect(flyoutScrollCeiling(long)).toBe(Math.ceil(TRACK / 4) + 1);
    expect(nextFlyoutScroll(long, null, flyoutScrollCeiling(long) - 1)).toBe(4);
    expect(nextFlyoutScroll(long, null, flyoutScrollCeiling(long))).toBeNull();
  });
});
