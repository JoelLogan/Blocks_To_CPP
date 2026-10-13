/**
 * The console's own wheel scrolling (docs/spec/04-user-interface.md §4.5).
 *
 * xterm.js 5.5 scrolls for a wheel event by moving its viewport element's `scrollTop` and reading
 * the row back from the scroll event that follows. While a program writes, every render sets
 * `scrollTop` from the buffer again and marks the next scroll event as its own, so the person's
 * scroll is undone or ignored: in WebKitGTK at a device scale of 2 (which GNOME also uses for its
 * fractional scales, such as 125% and 150%) the wheel does nothing while output streams, and the
 * same happens at fractional scales in WebView2. The console therefore takes the wheel over and
 * scrolls the buffer itself, by whole lines, with {@link WheelScroller}.
 *
 * It takes over only where the wheel scrolls the scrollback: on the normal screen, with no
 * modifier key held, while the program has not asked for mouse events. Everything else is left to
 * xterm.js: on a program's alternate screen it sends arrow keys, with mouse tracking on it reports
 * the wheel to the program, and a modifier keeps its own meaning (Shift scrolls sideways, Ctrl
 * zooms).
 *
 * xterm.js.6 replaces its viewport and removes the problem (M3, after the upgrade spike); this
 * handler goes with it.
 */

/** `WheelEvent.deltaMode`: the delta is in pixels, lines or pages. */
const DOM_DELTA_PIXEL = 0;
const DOM_DELTA_LINE = 1;
const DOM_DELTA_PAGE = 2;

/** What the wheel handler reads from and does to the terminal. */
export interface WheelTarget {
  /**
   * Whether the wheel scrolls the scrollback: the normal screen is shown and the program has not
   * turned mouse tracking on.
   */
  scrollsScrollback(): boolean;
  /** The height of one row in CSS pixels; 0 (or less) when it cannot be measured. */
  rowHeight(): number;
  /** The terminal's height in rows, for deltas given in pages. */
  rows(): number;
  /** Scrolls the view by whole lines (negative: up), as `Terminal.scrollLines` does. */
  scrollLines(lines: number): void;
}

/** The lines `event` asks to scroll, possibly fractional, or `null` when they cannot be told. */
function linesOf(event: WheelEvent, target: WheelTarget): number | null {
  switch (event.deltaMode) {
    case DOM_DELTA_PIXEL: {
      const height = target.rowHeight();
      return Number.isFinite(height) && height > 0 ? event.deltaY / height : null;
    }
    case DOM_DELTA_LINE:
      return event.deltaY;
    case DOM_DELTA_PAGE:
      return event.deltaY * target.rows();
    default:
      return null;
  }
}

/**
 * Scrolls the terminal for wheel events (see the module documentation). Pass {@link handle} to
 * `Terminal.attachCustomWheelEventHandler`.
 *
 * A touchpad sends many small deltas: each event scrolls the whole lines it adds up to, and the
 * fraction left over is kept for the next event in the same direction, so slow scrolling moves the
 * view as far as fast scrolling does. Turning the other way starts afresh.
 */
export class WheelScroller {
  readonly #target: WheelTarget;
  /** The fraction of a line scrolled but not yet applied; its sign is the direction. */
  #remainder = 0;

  constructor(target: WheelTarget) {
    this.#target = target;
  }

  /**
   * The custom wheel event handler: returns `false` when it scrolled the terminal (it has then
   * cancelled the event, so the browser does not scroll the viewport as well), `true` to leave the
   * event to xterm.js.
   */
  readonly handle = (event: WheelEvent): boolean => {
    if (
      event.ctrlKey ||
      event.altKey ||
      event.shiftKey ||
      event.metaKey ||
      event.deltaY === 0 ||
      !this.#target.scrollsScrollback()
    ) {
      this.#remainder = 0;
      return true;
    }
    const lines = linesOf(event, this.#target);
    if (lines === null || !Number.isFinite(lines)) {
      this.#remainder = 0;
      return true;
    }
    event.preventDefault();
    if (Math.sign(lines) !== Math.sign(this.#remainder)) {
      this.#remainder = 0;
    }
    const total = this.#remainder + lines;
    const whole = Math.trunc(total);
    this.#remainder = total - whole;
    if (whole !== 0) {
      this.#target.scrollLines(whole);
    }
    return false;
  };
}
