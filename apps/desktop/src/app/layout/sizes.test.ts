/** The docks' sizes (docs/spec/04-user-interface.md §4.1): start sizes and largest sizes. */
import { describe, expect, it } from 'vitest';

import {
  BOTTOM_DOCK,
  clampSize,
  dockMaximum,
  RIGHT_DOCK,
  rightDockMaximum,
  SPLITTER_SIZE,
  startSize,
} from './sizes';

describe('the docks’ start sizes', () => {
  it('are 30 % of the width (280–480 px) and 28 % of the height (120–280 px)', () => {
    const right: [number, number][] = [
      [800, 280],
      [1024, 307],
      [1028, 308],
      [1280, 384],
      [1366, 410],
      [1920, 480],
    ];
    for (const [width, size] of right) {
      expect(startSize(RIGHT_DOCK, width), `${String(width)} px`).toBe(size);
    }
    const bottom: [number, number][] = [
      [400, 120],
      [560, 157],
      [700, 196],
      [768, 215],
      [800, 224],
      [1080, 280],
    ];
    for (const [height, size] of bottom) {
      expect(startSize(BOTTOM_DOCK, height), `${String(height)} px`).toBe(size);
    }
  });
});

describe('the docks’ largest sizes', () => {
  it('are at most 65 % of the editor area and their own largest size, never below their smallest', () => {
    expect(dockMaximum(BOTTOM_DOCK, 704)).toBe(457);
    expect(dockMaximum(BOTTOM_DOCK, 2000)).toBe(720);
    expect(dockMaximum(BOTTOM_DOCK, 100)).toBe(96);
  });

  it('leave the canvas 200 px beside the narrowest flyout, for the C++ dock', () => {
    const toolbox = { left: 100, min: 160 };
    // 65 % of the area decides on a wide window, the canvas on a narrow one.
    expect(rightDockMaximum(1366, toolbox)).toBe(887);
    expect(rightDockMaximum(1024, toolbox)).toBe(1024 - SPLITTER_SIZE - 100 - 160 - 200);
    expect(rightDockMaximum(800, toolbox)).toBe(334);
    expect(rightDockMaximum(3000, toolbox)).toBe(960);
    // Below the dock's smallest width: it does not fit, and is hidden.
    expect(rightDockMaximum(640, toolbox)).toBeLessThan(RIGHT_DOCK.min);
    // Before the flyout is laid out, only the share counts.
    expect(rightDockMaximum(1024, null)).toBe(665);
  });

  it('keep a size within them, as a whole number', () => {
    expect(clampSize(300.4, 200, 500)).toBe(300);
    expect(clampSize(100, 200, 500)).toBe(200);
    expect(clampSize(900, 200, 500)).toBe(500);
  });
});
