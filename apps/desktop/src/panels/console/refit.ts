/**
 * Fitting the console's terminal to its box (docs/spec/04-user-interface.md §4.5, 07 §7.6.2).
 *
 * The box changes size whenever the window or a dock is resized, often many times a second while
 * a splitter is dragged. {@link Refitter} fits the terminal at most once per animation frame. A
 * new number of rows is applied at once: it is cheap. A new number of columns makes xterm.js
 * reflow every line of the scrollback (11–38 ms at 10,000 lines, about 100 ms at 100,000), and the
 * program learns the new width, so columns follow only once the width has stayed the same for
 * {@link COLUMN_SETTLE_MS}.
 */
import type { TerminalSize } from './ConsolePanel';

/** How long the width must stay the same before the terminal takes its new number of columns. */
export const COLUMN_SETTLE_MS = 100;

/** What the refitter measures and resizes. */
export interface FitTarget {
  /** The size that fits the box now, or `null` when the box cannot be measured (it is hidden). */
  propose(): TerminalSize | null;
  /** The terminal's size now. */
  current(): TerminalSize;
  /** Resizes the terminal. */
  resize(size: TerminalSize): void;
}

/** The frame and timer functions the refitter uses; tests pass fake ones. */
export interface RefitTimers {
  requestAnimationFrame(callback: () => void): unknown;
  cancelAnimationFrame(handle: unknown): void;
  setTimeout(callback: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
}

const browserTimers: RefitTimers = {
  requestAnimationFrame: (callback) => globalThis.requestAnimationFrame(callback),
  cancelAnimationFrame: (handle) => {
    globalThis.cancelAnimationFrame(handle as number);
  },
  setTimeout: (callback, ms) => globalThis.setTimeout(callback, ms),
  clearTimeout: (handle) => {
    globalThis.clearTimeout(handle as ReturnType<typeof globalThis.setTimeout>);
  },
};

/** Fits the terminal to its box; see the module documentation. */
export class Refitter {
  readonly #target: FitTarget;
  readonly #timers: RefitTimers;
  #frame: unknown = null;
  #columnsTimer: unknown = null;

  constructor(target: FitTarget, timers: RefitTimers = browserTimers) {
    this.#target = target;
    this.#timers = timers;
  }

  /** Fits the terminal now, rows and columns (the terminal has just been opened). */
  fitNow(): void {
    this.#cancelColumns();
    const proposed = this.#target.propose();
    if (proposed !== null && !sameSize(proposed, this.#target.current())) {
      this.#target.resize(proposed);
    }
  }

  /** The box changed size: the terminal is fitted in the next animation frame. */
  request(): void {
    if (this.#frame !== null) {
      return;
    }
    this.#frame = this.#timers.requestAnimationFrame(() => {
      this.#frame = null;
      this.#fit();
    });
  }

  /** Cancels what is still to come (the console goes away). */
  dispose(): void {
    if (this.#frame !== null) {
      this.#timers.cancelAnimationFrame(this.#frame);
      this.#frame = null;
    }
    this.#cancelColumns();
  }

  #fit(): void {
    const proposed = this.#target.propose();
    if (proposed === null) {
      return;
    }
    const current = this.#target.current();
    if (proposed.cols === current.cols) {
      // The width is (back to) what the terminal has: nothing to reflow.
      this.#cancelColumns();
      if (proposed.rows !== current.rows) {
        this.#target.resize(proposed);
      }
      return;
    }
    if (proposed.rows !== current.rows) {
      this.#target.resize({ cols: current.cols, rows: proposed.rows });
    }
    this.#cancelColumns();
    this.#columnsTimer = this.#timers.setTimeout(() => {
      this.#columnsTimer = null;
      this.fitNow();
    }, COLUMN_SETTLE_MS);
  }

  #cancelColumns(): void {
    if (this.#columnsTimer !== null) {
      this.#timers.clearTimeout(this.#columnsTimer);
      this.#columnsTimer = null;
    }
  }
}

function sameSize(a: TerminalSize, b: TerminalSize): boolean {
  return a.cols === b.cols && a.rows === b.rows;
}
