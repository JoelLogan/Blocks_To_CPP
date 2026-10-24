/**
 * Program output for screen readers (docs/spec/04-user-interface.md §4.5, §4.8).
 *
 * The console's terminal runs in xterm.js's screen reader mode: an accessibility tree mirrors the
 * rows on screen as a list a screen reader can read line by line (moving past its first or last
 * row scrolls the terminal), and a live region announces new output in batches, at most once a
 * second. The rows xterm.js draws are hidden from screen readers (`aria-hidden`), so without the
 * mode the console says nothing.
 *
 * The mode has a cost while output pours in: xterm.js then reports every printed character to the
 * accessibility tree. Measured in WebKitGTK 2.52 with xterm.js 5.5, 200,000 short lines took about
 * 90% longer to process when written as fast as possible, and 5–25% longer when they arrived in
 * batches as the backend sends them. Nobody can listen to that much output (xterm.js announces 20
 * lines and then says there is too much output to announce), so {@link ScreenReaderSupport} turns
 * the mode off while the terminal is given more than {@link FLOOD_WRITE_BYTES} at once, and back on,
 * with a fresh accessibility tree showing the rows on screen, once no such write has come for
 * {@link FLOOD_QUIET_MS}.
 *
 * The tree is also built afresh when the terminal is cleared or reset, so neither the rows nor the
 * announcements of output that is gone linger, and when a run starts: xterm.js announces at most 20
 * lines and then nothing until a key is pressed in the terminal or it loses the focus, and a run
 * started with F5 from the console (which keeps the focus) is neither.
 */

/** Writes larger than this (about a thousand short lines in one write) count as a flood. */
export const FLOOD_WRITE_BYTES = 16 * 1024;

/** How long after the last flood-sized write the screen reader mode comes back. */
export const FLOOD_QUIET_MS = 1000;

/** The timer functions the screen reader support uses; tests pass fake ones. */
export interface ScreenReaderTimers {
  setTimeout(callback: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
}

const browserTimers: ScreenReaderTimers = {
  setTimeout: (callback, ms) => globalThis.setTimeout(callback, ms),
  clearTimeout: (handle) => {
    globalThis.clearTimeout(handle as ReturnType<typeof globalThis.setTimeout>);
  },
};

/**
 * Switches the terminal's screen reader mode (`term.options.screenReaderMode`). Turning it on
 * builds a new accessibility tree from the rows on screen; turning it off removes the tree.
 */
export type SetScreenReaderMode = (on: boolean) => void;

/**
 * Keeps the terminal's screen reader mode on, except during floods (see the module
 * documentation). The terminal starts with the mode on.
 */
export class ScreenReaderSupport {
  readonly #setMode: SetScreenReaderMode;
  readonly #timers: ScreenReaderTimers;
  /** The timer that ends a pause, or `null` while the mode is on. */
  #resume: unknown = null;

  constructor(setMode: SetScreenReaderMode, timers: ScreenReaderTimers = browserTimers) {
    this.#setMode = setMode;
    this.#timers = timers;
  }

  /** Whether the mode is paused for a flood. */
  get paused(): boolean {
    return this.#resume !== null;
  }

  /** The terminal is about to be given `bytes` bytes of output. */
  beforeWrite(bytes: number): void {
    if (bytes <= FLOOD_WRITE_BYTES) {
      return;
    }
    if (this.#resume === null) {
      this.#setMode(false);
    } else {
      this.#timers.clearTimeout(this.#resume);
    }
    this.#resume = this.#timers.setTimeout(() => {
      this.#resume = null;
      this.#setMode(true);
    }, FLOOD_QUIET_MS);
  }

  /**
   * The terminal was cleared or reset, or a run starts: the accessibility tree is built afresh, and
   * new output is announced again (during a pause it is built when the pause ends).
   */
  restart(): void {
    if (this.#resume === null) {
      this.#setMode(false);
      this.#setMode(true);
    }
  }

  /** Cancels a pending resume (the console goes away). */
  dispose(): void {
    if (this.#resume !== null) {
      this.#timers.clearTimeout(this.#resume);
      this.#resume = null;
    }
  }
}
