import { describe, expect, it } from 'vitest';

import { type WheelTarget, WheelScroller } from './wheel';

/** A terminal of 20 rows of 16 px that records how it was scrolled. */
function target(overrides: Partial<WheelTarget> = {}) {
  const scrolled: number[] = [];
  const terminal: WheelTarget = {
    scrollsScrollback: () => true,
    rowHeight: () => 16,
    rows: () => 20,
    scrollLines: (lines) => {
      scrolled.push(lines);
    },
    ...overrides,
  };
  return { terminal, scrolled };
}

const PIXEL = 0;
const LINE = 1;
const PAGE = 2;

/**
 * A cancelable wheel event. happy-dom's `WheelEvent` is a `UIEvent` without the modifier keys a
 * browser's has (it is a `MouseEvent` there), so they are set on the event itself.
 */
function wheel(deltaY: number, init: WheelEventInit = {}): WheelEvent {
  const event = new WheelEvent('wheel', { deltaY, deltaMode: PIXEL, cancelable: true, ...init });
  for (const key of ['ctrlKey', 'altKey', 'shiftKey', 'metaKey'] as const) {
    Object.defineProperty(event, key, { value: init[key] === true });
  }
  return event;
}

describe('WheelScroller', () => {
  it('scrolls whole lines for pixel deltas, keeping the remainder for the next event', () => {
    const { terminal, scrolled } = target();
    const scroller = new WheelScroller(terminal);
    // A touchpad: ten events of 10 px each are 100 px, six whole rows of 16 px.
    for (let i = 0; i < 10; i++) {
      const event = wheel(10);
      expect(scroller.handle(event)).toBe(false);
      expect(event.defaultPrevented).toBe(true);
    }
    expect(scrolled.reduce((sum, lines) => sum + lines, 0)).toBe(6);
    expect(scrolled.every((lines) => lines === 1)).toBe(true);

    // A mouse notch of 53 px up at a row height of 15.5 px (a fractional display scale).
    const notch = target({ rowHeight: () => 15.5 });
    const mouse = new WheelScroller(notch.terminal);
    mouse.handle(wheel(-53));
    mouse.handle(wheel(-53));
    // 3.42 lines, then 6.84 in all: 3, then 3 more.
    expect(notch.scrolled).toEqual([-3, -3]);
  });

  it('starts afresh when the direction changes', () => {
    const { terminal, scrolled } = target();
    const scroller = new WheelScroller(terminal);
    scroller.handle(wheel(12)); // 0.75 of a line down, kept
    scroller.handle(wheel(-12)); // up: the 0.75 down is dropped, 0.75 up kept
    scroller.handle(wheel(-12)); // 1.5 up: one line
    expect(scrolled).toEqual([-1]);
  });

  it('scrolls by lines and by pages', () => {
    const { terminal, scrolled } = target();
    const scroller = new WheelScroller(terminal);
    expect(scroller.handle(wheel(3, { deltaMode: LINE }))).toBe(false);
    expect(scroller.handle(wheel(-1, { deltaMode: PAGE }))).toBe(false);
    expect(scroller.handle(wheel(0.5, { deltaMode: PAGE }))).toBe(false);
    expect(scrolled).toEqual([3, -20, 10]);
  });

  it('leaves the wheel to xterm.js with a modifier key held', () => {
    const { terminal, scrolled } = target();
    const scroller = new WheelScroller(terminal);
    for (const modifier of ['ctrlKey', 'altKey', 'shiftKey', 'metaKey'] as const) {
      const event = wheel(-53, { [modifier]: true });
      expect(scroller.handle(event)).toBe(true);
      expect(event.defaultPrevented).toBe(false);
    }
    expect(scrolled).toEqual([]);
  });

  it("leaves the wheel to xterm.js on a program's alternate screen or with mouse tracking", () => {
    let scrollback = false;
    const { terminal, scrolled } = target({ scrollsScrollback: () => scrollback });
    const scroller = new WheelScroller(terminal);
    const event = wheel(-53);
    expect(scroller.handle(event)).toBe(true);
    expect(event.defaultPrevented).toBe(false);

    // The remainder of a scroll does not survive a pass-through.
    scrollback = true;
    scroller.handle(wheel(12));
    scrollback = false;
    scroller.handle(wheel(12));
    scrollback = true;
    scroller.handle(wheel(12));
    expect(scrolled).toEqual([]);
  });

  it('leaves sideways scrolling, and pixels it cannot convert, to xterm.js', () => {
    const { terminal, scrolled } = target({ rowHeight: () => 0 });
    const scroller = new WheelScroller(terminal);
    expect(scroller.handle(wheel(0, { deltaX: 40 }))).toBe(true);
    // The rows are not laid out yet: no row height to convert pixels with.
    expect(scroller.handle(wheel(53))).toBe(true);
    // Lines need no row height.
    expect(scroller.handle(wheel(2, { deltaMode: LINE }))).toBe(false);
    expect(scrolled).toEqual([2]);
  });
});
